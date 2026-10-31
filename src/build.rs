//! The build pipeline: discover → resolve → render → emit.

use crate::config::Config;
use crate::content;
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
    let site = site::resolve(config, found, vis);
    let files = renderer.render_site(&site)?;

    prepare_output(&opts.out)?;
    let static_files = copy_dir(&opts.root.join("static"), &opts.out)?;
    write_files(&opts.out, &files)?;
    Ok(report(&site, &files, static_files, ignored))
}

fn report(site: &Site, files: &[OutputFile], static_files: usize, ignored: Vec<String>) -> Report {
    Report {
        pages: site.regular.len(),
        lists: site.sections.iter().map(|s| s.pagers.len()).sum(),
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

/// Copy `static/` verbatim to the output root. Returns the file count.
fn copy_dir(from: &Path, to: &Path) -> Result<usize> {
    if !from.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    let mut entries: Vec<_> = std::fs::read_dir(from)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&dst)?;
            count += copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
            count += 1;
        }
    }
    Ok(count)
}
