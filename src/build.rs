//! The build pipeline: discover → resolve → render → emit.

use crate::config::Config;
use crate::content;
use crate::output;
use crate::render::{OutputFile, Renderer};
use crate::site::{self, Site, Visibility};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use std::path::{Path, PathBuf};

/// Written into every output directory ferrisite creates. A non-empty
/// directory without it is never cleaned, so a mistyped `--out` can't wipe
/// something else.
pub const MARKER: &str = ".ferrisite-output";

pub struct Options {
    pub root: PathBuf,
    pub out: PathBuf,
    pub drafts: bool,
    pub future: bool,
}

pub struct Report {
    pub pages: usize,
    pub lists: usize,
    pub files: usize,
    pub static_files: usize,
    pub ignored: Vec<String>,
}

pub fn build(opts: &Options) -> Result<Report> {
    let config = Config::load(&opts.root)?;
    let found = content::discover(&opts.root.join("content"), &config.taxonomies)?;
    let ignored = found.ignored.clone();
    let vis = Visibility {
        drafts: opts.drafts,
        future: opts.future,
        now: now()?,
    };
    let renderer = Renderer::new(&config, &opts.root.join("templates"))?;
    let site = site::resolve(config, &opts.root, found, vis, &renderer)?;
    let mut files = renderer.render_site(&site)?;
    files.extend(output::all(&site));
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
    Ok(report(&site, &files, static_paths.len(), ignored))
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
