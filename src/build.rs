//! The build pipeline: discover → resolve → render → emit.

use crate::config::Config;
use crate::content;
use crate::diagrams::Diagrams;
use crate::output;
use crate::render::{OutputFile, Renderer};
use crate::site::{self, Site, Visibility};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// Render social cards (when the site enables them). `serve` turns this
    /// off: a preview never shows them, and they were most of a build's CPU.
    pub cards: bool,
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
    let diagrams = Diagrams::new(&opts.root.join(&config.markup.include_base))?
        .with_palettes(&config.markup.diagram_light, &config.markup.diagram_dark)?;
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
    let statics = collect_static(&opts.root, &site.config.static_dirs)?;
    if let Some(clash) = files.iter().find(|f| statics.contains_key(&f.path)) {
        bail!(
            "generated file {} would overwrite a static file",
            clash.path
        );
    }
    write_static(&opts.out, &statics)?;
    write_files(&opts.out, &files)?;
    let manifest: String = files.iter().map(|f| format!("{}\n", f.path)).collect();
    write_if_changed(&opts.out.join(MARKER), manifest.as_bytes())?;
    let mut keep: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    keep.extend(statics.keys().cloned());
    keep.insert(MARKER.to_string());
    let cards = if opts.cards {
        let cards = write_cards(&site, &opts.root, &opts.out, &keep)?;
        keep.insert(CARD_CACHE.to_string());
        let n = cards.len();
        keep.extend(cards);
        n
    } else {
        0
    };
    remove_stale(&opts.out, Path::new(""), &keep)?;
    let mut report = report(&site, &files, statics.len(), ignored);
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

/// Check that `out` is ferrisite's to write into, and create it. Nothing is
/// deleted here: files are rewritten in place only when their contents
/// change, and `remove_stale` deletes what the build no longer produces. So a
/// rebuild touches only what changed, and `serve` never serves an emptied
/// directory mid-build.
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
    }
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    if !out.join(MARKER).exists() {
        std::fs::write(out.join(MARKER), "")?;
    }
    Ok(())
}

/// Write `contents` to `path` unless it already holds exactly that. The new
/// file is written beside the old and renamed over it, so a reader (`serve`)
/// sees the old file or the new one, never half of one. A directory, or a
/// file standing where a parent directory must go, is replaced.
fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() == contents.len() as u64 => {
            if std::fs::read(path).is_ok_and(|b| b == contents) {
                return Ok(());
            }
        }
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path)
            .with_context(|| format!("replacing directory {}", path.display()))?,
        _ => {}
    }
    let parent = path.parent().context("output path has no parent")?;
    if std::fs::create_dir_all(parent).is_err() {
        for dir in parent.ancestors() {
            if dir.is_file() {
                std::fs::remove_file(dir)?;
            }
        }
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    // A temp name unique to this write: on a case-insensitive disk two
    // outputs that differ only in case (alias stubs such as /tags/Design/
    // and /tags/design/) are one file, and parallel writers sharing a temp
    // name would rename it out from under each other.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().context("output path has no file name")?;
    let tmp = parent.join(format!(
        ".{}.{}-{n}.ferrisite-tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", path.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_files(out: &Path, files: &[OutputFile]) -> Result<()> {
    files
        .par_iter()
        .try_for_each(|f| write_if_changed(&out.join(&f.path), f.contents.as_bytes()))
}

fn write_static(out: &Path, statics: &BTreeMap<String, PathBuf>) -> Result<()> {
    statics.par_iter().try_for_each(|(rel, src)| {
        let bytes = std::fs::read(src).with_context(|| format!("copying {}", src.display()))?;
        write_if_changed(&out.join(rel), &bytes)
    })
}

/// Delete files under `out` that this build didn't produce, then any
/// directories left empty.
fn remove_stale(out: &Path, rel: &Path, keep: &HashSet<String>) -> Result<()> {
    let dir = out.join(rel);
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let child = rel.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            remove_stale(out, &child, keep)?;
            if std::fs::read_dir(out.join(&child))?.next().is_none() {
                std::fs::remove_dir(out.join(&child))?;
            }
        } else if !keep.contains(&site_path(&child)) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn site_path(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Cached card hashes, one `<hash> <path>` per line, so a rebuild into the
/// same directory only re-renders cards whose drawing changed.
const CARD_CACHE: &str = ".ferrisite-cards";

/// Social cards for every page URL, rendered in parallel. Returns their
/// site-relative paths.
fn write_cards(
    site: &Site,
    root: &Path,
    out: &Path,
    taken: &HashSet<String>,
) -> Result<Vec<String>> {
    if !site.config.cards.enabled {
        return Ok(Vec::new());
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
    let cached: HashMap<String, String> = std::fs::read_to_string(out.join(CARD_CACHE))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(h, p)| (p.to_string(), h.to_string()))
        .collect();
    let written: Vec<(String, String)> = jobs
        .par_iter()
        .map(|(url, title, byline)| {
            let path = crate::cards::card_path(url);
            if taken.contains(&path) {
                bail!("social card {path} would overwrite another file");
            }
            let svg = cards.svg(title, byline);
            let hash = format!("{:016x}", fnv1a(svg.as_bytes()));
            let full = out.join(&path);
            if cached.get(&path) != Some(&hash) || !full.is_file() {
                let png = cards
                    .png(&svg)
                    .with_context(|| format!("social card for {url}"))?;
                write_if_changed(&full, &png)?;
            }
            Ok((path, hash))
        })
        .collect::<Result<_>>()?;
    let cache: String = written.iter().map(|(p, h)| format!("{h} {p}\n")).collect();
    write_if_changed(&out.join(CARD_CACHE), cache.as_bytes())?;
    Ok(written.into_iter().map(|(p, _)| p).collect())
}

/// FNV-1a: a hash that is the same on every platform and toolchain, unlike
/// `DefaultHasher`, so the card cache survives a compiler upgrade.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// Every static file, keyed by its site-relative path. A later directory
/// overrides an earlier one, as Hugo layers a site's `static/` over its
/// theme's.
fn collect_static(root: &Path, dirs: &[String]) -> Result<BTreeMap<String, PathBuf>> {
    fn walk(dir: &Path, rel: &Path, out: &mut BTreeMap<String, PathBuf>) -> Result<()> {
        let here = dir.join(rel);
        if !here.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&here)? {
            let entry = entry?;
            if entry.file_name() == ".DS_Store" {
                continue;
            }
            let child = rel.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                walk(dir, &child, out)?;
            } else {
                out.insert(site_path(&child), entry.path());
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    for d in dirs {
        walk(&root.join(d), Path::new(""), &mut out)?;
    }
    Ok(out)
}
