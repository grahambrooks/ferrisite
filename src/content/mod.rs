//! Content discovery: files under `content/` become `Page`s with Hugo's URL
//! rules (case preserved, as with Hugo's `disablePathToLower`).

pub mod frontmatter;

use anyhow::{Context, Result, bail};
use frontmatter::FrontMatter;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Home,
    Section,
    Page,
    /// `content/<taxonomy>/_index.md`
    Taxonomy,
    /// `content/<taxonomy>/<term>/_index.md`
    Term,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    AsciiDoc,
}

#[derive(Debug, Clone)]
pub struct Page {
    /// Path relative to `content/`, with `/` separators.
    pub source: String,
    pub kind: Kind,
    /// First directory under `content/`; empty for the home page and top-level pages.
    pub section: String,
    /// Site-relative URL with leading and trailing slash, e.g. `/post/foo/`.
    pub url: String,
    pub fm: FrontMatter,
    pub body: String,
    /// 1-based line of the file where `body` starts, so errors can name file lines.
    pub body_line: usize,
    pub format: Format,
}

/// What discovery found that isn't a page.
#[derive(Debug, Default)]
pub struct Discovered {
    pub pages: Vec<Page>,
    /// Section directories with no `_index` file of their own.
    pub implicit_sections: Vec<String>,
    /// Files that aren't content (reported, not copied).
    pub ignored: Vec<String>,
}

pub fn discover(content_dir: &Path, taxonomy_dirs: &[String]) -> Result<Discovered> {
    let mut files = Vec::new();
    walk(content_dir, &mut files)?;
    files.sort();
    let mut found = Discovered::default();
    for path in files {
        let rel = path
            .strip_prefix(content_dir)
            .expect("walked under content_dir")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let top = rel.split('/').next().unwrap_or("");
        let in_taxonomy = rel.contains('/') && taxonomy_dirs.iter().any(|t| t == top);
        let ext = Path::new(&rel)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let format = match ext {
            "md" | "markdown" => Format::Markdown,
            "adoc" | "asciidoc" => Format::AsciiDoc,
            _ => {
                found.ignored.push(rel);
                continue;
            }
        };
        let raw = std::fs::read_to_string(&path).with_context(|| format!("reading {rel}"))?;
        let (fm, body) = frontmatter::split(&raw).with_context(|| format!("{rel}: frontmatter"))?;
        let (mut kind, url) = url_for(&rel);
        if in_taxonomy {
            kind = match (kind, rel.matches('/').count()) {
                (Kind::Section, 1) => Kind::Taxonomy,
                (Kind::Section, 2) => Kind::Term,
                _ => bail!(
                    "{rel}: taxonomy directories may only hold `_index` files for the taxonomy and its terms"
                ),
            };
        }
        let section = if kind == Kind::Home || !rel.contains('/') {
            String::new()
        } else {
            top.to_string()
        };
        let body_line = raw[..raw.len() - body.len()].matches('\n').count() + 1;
        found.pages.push(Page {
            source: rel,
            kind,
            section,
            url,
            body_line,
            body: body.to_string(),
            fm,
            format,
        });
    }
    // A directory with pages but no _index is still a section (Hugo does the same).
    let mut sections: Vec<String> = found
        .pages
        .iter()
        .filter(|p| p.kind == Kind::Page && !p.section.is_empty())
        .filter(|p| !taxonomy_dirs.contains(&p.section))
        .map(|p| p.section.clone())
        .collect();
    sections.sort();
    sections.dedup();
    found.implicit_sections = sections
        .into_iter()
        .filter(|s| {
            !found
                .pages
                .iter()
                .any(|p| p.kind == Kind::Section && p.url == format!("/{s}/"))
        })
        .collect();
    Ok(found)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Hugo's URL for a content path: `_index` files are list pages for their
/// directory, other files become `/<dir>/<name>/`. Spaces become `-`.
pub fn url_for(rel: &str) -> (Kind, String) {
    let (dir, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    let join = |parts: &[&str]| -> String {
        let path: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
        if path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", path.join("/")).replace(' ', "-")
        }
    };
    if stem == "_index" {
        let kind = if dir.is_empty() {
            Kind::Home
        } else {
            Kind::Section
        };
        (kind, join(&[dir]))
    } else {
        (Kind::Page, join(&[dir, stem]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hugo_url_rules() {
        assert_eq!(url_for("_index.md"), (Kind::Home, "/".into()));
        assert_eq!(url_for("post/_index.md"), (Kind::Section, "/post/".into()));
        assert_eq!(
            url_for("post/2009-07-18-Java-Props.markdown"),
            (Kind::Page, "/post/2009-07-18-Java-Props/".into())
        );
        assert_eq!(url_for("about.md"), (Kind::Page, "/about/".into()));
        assert_eq!(
            url_for("a/b/my page.md"),
            (Kind::Page, "/a/b/my-page/".into())
        );
    }
}
