//! The build pipeline: discover → resolve → render → emit.

use crate::config::Config;
use crate::content;
use crate::diagrams::Diagrams;
use crate::output;
use crate::render::{OutputFile, Renderer};
use crate::site::{self, Site, Visibility};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use std::path::{Path, PathBuf};

/// Written into every output directory ferrisite creates. A non-empty
/// directory without it is never cleaned, so a mistyped `--out` can't wipe
/// something else. It lists the generated (non-static) files, one per line,
/// so `check` can tell pages from copied static files.
pub const MARKER: &str = ".ferrisite-output";

pub struct Options {
    pub root: PathBuf,
    pub out: PathBuf,
    pub drafts: bool,
    pub future: bool,
    /// Overrides `base_url` from ferrisite.toml (e.g. localhost for `serve`).
    pub base_url: Option<String>,
}

pub struct Report {
    pub pages: usize,
    pub lists: usize,
    pub files: usize,
    pub static_files: usize,
    pub ignored: Vec<String>,
}

pub fn build(opts: &Options) -> Result<Report> {
    let mut config = Config::load(&opts.root)?;
    if let Some(base) = &opts.base_url {
        config.base_url = if base.ends_with('/') {
            base.clone()
        } else {
            format!("{base}/")
        };
    }
    let found = content::discover(&opts.root.join("content"), &config.taxonomies)?;
    let ignored = found.ignored.clone();
    let vis = Visibility {
        drafts: opts.drafts,
        future: opts.future,
        now: now()?,
    };
    let renderer = Renderer::new(&config, &opts.root.join("templates"))?;
    let diagrams = Diagrams::new(&opts.root.join(&config.markup.include_base))?;
    let stylesheet = OutputFile {
        path: config.markup.stylesheet.trim_start_matches('/').to_string(),
        contents: format!(
            "{}\n{}",
            crate::markup::highlight::css(
                &config.markup.highlight_light,
                &config.markup.highlight_dark
            )?,
            crate::diagrams::css()
        ),
    };
    let site = site::resolve(config, &opts.root, found, vis, &renderer, &diagrams)?;
    let mut files = renderer.render_site(&site)?;
    files.extend(output::all(&site));
    files.extend(crate::agents::all(&site));
    files.push(stylesheet);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if let Some(w) = files.windows(2).find(|w| w[0].path == w[1].path) {
        bail!("two outputs claim {}", w[0].path);
    }

    prepare_output(&opts.out)?;
    let mut static_paths = Vec::new();
    for dir in &site.config.static_dirs {
        copy_dir(
            &opts.root.join(dir),
            &opts.out,
            Path::new(""),
            &mut static_paths,
        )?;
    }
    static_paths.sort();
    static_paths.dedup();
    if let Some(clash) = files
        .iter()
        .find(|f| static_paths.binary_search(&f.path).is_ok())
    {
        bail!(
            "generated file {} would overwrite a static file",
            clash.path
        );
    }
    write_files(&opts.out, &files)?;
    let manifest: String = files.iter().map(|f| format!("{}\n", f.path)).collect();
    std::fs::write(opts.out.join(MARKER), manifest)?;
    let cards = write_cards(&site, &opts.root, &opts.out, &static_paths, &files)?;
    let mut report = report(&site, &files, static_paths.len(), ignored);
    report.files += cards;
    Ok(report)
}

fn report(site: &Site, files: &[OutputFile], static_files: usize, ignored: Vec<String>) -> Report {
    Report {
        pages: site
            .entries
            .iter()
            .filter(|e| e.page.kind == content::Kind::Page)
            .count(),
        lists: site.sections.iter().map(|s| s.pagers.len()).sum::<usize>()
            + site
                .taxonomies
                .iter()
                .flat_map(|t| &t.terms)
                .map(|t| t.pagers.len())
                .sum::<usize>(),
        files: files.len(),
        static_files,
        ignored,
    }
}

/// "Now", for excluding future-dated pages. Honours `SOURCE_DATE_EPOCH` so a
/// build can be reproduced exactly.
fn now() -> Result<DateTime<FixedOffset>> {
    let utc = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(v) => {
            let secs: i64 = v
                .trim()
                .parse()
                .with_context(|| format!("SOURCE_DATE_EPOCH={v:?} is not an integer"))?;
            Utc.timestamp_opt(secs, 0)
                .single()
                .context("SOURCE_DATE_EPOCH out of range")?
        }
        Err(_) => Utc::now(),
    };
    Ok(utc.fixed_offset())
}

fn prepare_output(out: &Path) -> Result<()> {
    if out.exists() {
        let mut entries = std::fs::read_dir(out)
            .with_context(|| format!("reading {}", out.display()))?
            .peekable();
        if entries.peek().is_some() && !out.join(MARKER).exists() {
            bail!(
                "{} is not empty and wasn't created by ferrisite (no {MARKER}); refusing to clean it",
                out.display()
            );
        }
        std::fs::remove_dir_all(out).with_context(|| format!("cleaning {}", out.display()))?;
    }
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    std::fs::write(out.join(MARKER), "")?;
    Ok(())
}

fn write_files(out: &Path, files: &[OutputFile]) -> Result<()> {
    for f in files {
        let path = out.join(&f.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &f.contents)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// Social cards for every page URL, rendered in parallel. Returns the count.
fn write_cards(
    site: &Site,
    root: &Path,
    out: &Path,
    static_paths: &[String],
    files: &[OutputFile],
) -> Result<usize> {
    use rayon::prelude::*;
    if !site.config.cards.enabled {
        return Ok(0);
    }
    let cards = crate::cards::Cards::new(&site.config.cards, root)?;
    let author = site.config.author.clone().unwrap_or_default();
    let mut jobs: Vec<(String, String, String)> = Vec::new();
    for e in &site.entries {
        let byline = match (e.page.kind, e.date()) {
            (content::Kind::Page, Some(d)) => format!("{author}  ·  {}", d.format("%-d %B %Y")),
            _ => author.clone(),
        };
        let title = if e.page.kind == content::Kind::Home || e.title().is_empty() {
            site.config.title.clone()
        } else {
            e.title().to_string()
        };
        jobs.push((e.page.url.clone(), title, byline));
    }
    for s in site.sections.iter().filter(|s| s.index.is_none()) {
        jobs.push((s.url.clone(), s.title.clone(), author.clone()));
    }
    for t in &site.taxonomies {
        if t.index.is_none() {
            jobs.push((t.url.clone(), t.title.clone(), author.clone()));
        }
        for term in t.terms.iter().filter(|t| t.index.is_none()) {
            jobs.push((term.url.clone(), term.title.clone(), author.clone()));
        }
    }
    jobs.push((
        crate::render::NOT_FOUND.to_string(),
        crate::render::NOT_FOUND_TITLE.to_string(),
        author.clone(),
    ));
    jobs.sort();
    jobs.dedup_by(|a, b| a.0 == b.0);
    let rendered: Vec<(String, Vec<u8>)> = jobs
        .par_iter()
        .map(|(url, title, byline)| {
            let png = cards
                .render(title, byline)
                .with_context(|| format!("social card for {url}"))?;
            Ok((crate::cards::card_path(url), png))
        })
        .collect::<Result<_>>()?;
    for (path, png) in &rendered {
        if static_paths.binary_search(path).is_ok() || files.iter().any(|f| &f.path == path) {
            bail!("social card {path} would overwrite another file");
        }
        let full = out.join(path);
        std::fs::create_dir_all(full.parent().expect("card dir"))?;
        std::fs::write(&full, png).with_context(|| format!("writing {path}"))?;
    }
    Ok(rendered.len())
}

/// Copy a static directory verbatim to the output root, recording the
/// site-relative path of every file. A later directory overwrites an earlier
/// one, as Hugo layers a site's `static/` over its theme's.
fn copy_dir(from: &Path, out: &Path, rel: &Path, seen: &mut Vec<String>) -> Result<()> {
    let src_dir = from.join(rel);
    if !src_dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&src_dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name.to_string_lossy() == ".DS_Store" {
            continue;
        }
        let child = rel.join(&name);
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(out.join(&child))?;
            copy_dir(from, out, &child, seen)?;
        } else {
            std::fs::copy(entry.path(), out.join(&child))
                .with_context(|| format!("copying {}", entry.path().display()))?;
            seen.push(
                child
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    Ok(())
}
