//! Agent-readable output (DESIGN.md section 9): a Markdown copy of every page
//! at `<url>index.md`, `/llms.txt` and `/llms-full.txt`.
//!
//! The copies are built from the source (section 9.1), not converted back
//! from HTML. List pages get a complete, unpaginated Markdown list, the
//! cheapest way for an agent to walk the site.

use crate::content::Kind;
use crate::render::OutputFile;
use crate::site::{Entry, Site};
use serde::Serialize;

/// `/post/x/` → `post/x/index.md`; `/` → `index.md`.
pub fn md_path(url: &str) -> String {
    let t = url.trim_matches('/');
    if t.is_empty() {
        "index.md".to_string()
    } else {
        format!("{t}/index.md")
    }
}

/// Absolute URL of a page's Markdown copy.
pub fn md_url(site: &Site, url: &str) -> String {
    site.config
        .permalink(&format!("{}index.md", url.trim_start_matches('/')))
}

#[derive(Serialize)]
struct Header<'a> {
    title: &'a str,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lastmod: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    series: Vec<String>,
}

fn header(site: &Site, e: &Entry) -> String {
    let list = |key: &str| {
        e.param(key)
            .map(crate::content::frontmatter::string_list)
            .unwrap_or_default()
    };
    let h = Header {
        title: e.title(),
        url: site.config.permalink(&e.page.url),
        date: e.date().map(|d| d.to_rfc3339()),
        lastmod: e.page.fm.lastmod.map(|d| d.to_rfc3339()),
        author: site.config.author.as_deref(),
        description: e.page.fm.description.as_deref(),
        tags: list("tags"),
        series: list("series"),
    };
    format!(
        "---\n{}---\n",
        serde_yaml::to_string(&h).expect("header serialises")
    )
}

fn page_markdown(site: &Site, e: &Entry) -> String {
    let mut md = header(site, e);
    if !e.title().is_empty() {
        md.push_str(&format!("\n# {}\n", e.title()));
    }
    let body = e.markdown.trim();
    if !body.is_empty() {
        md.push_str(&format!("\n{body}\n"));
    }
    md
}

/// One line per page in a Markdown list: link to its copy, date, summary.
fn list_line(site: &Site, e: &Entry) -> String {
    let mut line = format!(
        "- [{}]({})",
        escape_link_text(e.title()),
        md_url(site, &e.page.url)
    );
    if let Some(d) = e.date() {
        line.push_str(&format!(" ({})", d.format("%Y-%m-%d")));
    }
    let summary = e
        .page
        .fm
        .description
        .clone()
        .unwrap_or_else(|| first_words(&crate::markup::text::plain(&e.summary), 40));
    if !summary.is_empty() {
        line.push_str(&format!(": {summary}"));
    }
    line.push('\n');
    line
}

fn escape_link_text(s: &str) -> String {
    s.replace('[', "\\[").replace(']', "\\]")
}

fn first_words(s: &str, n: usize) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() <= n {
        words.join(" ")
    } else {
        format!("{}…", words[..n].join(" "))
    }
}

fn list_page(
    site: &Site,
    title: &str,
    url: &str,
    intro: Option<&Entry>,
    items: &[usize],
) -> String {
    let mut md = format!(
        "---\ntitle: {title:?}\nurl: {}\n---\n\n# {title}\n",
        site.config.permalink(url)
    );
    if let Some(e) = intro {
        let body = e.markdown.trim();
        if !body.is_empty() {
            md.push_str(&format!("\n{body}\n"));
        }
    }
    if !items.is_empty() {
        md.push('\n');
        for &i in items {
            md.push_str(&list_line(site, &site.entries[i]));
        }
    }
    md
}

pub fn all(site: &Site) -> Vec<OutputFile> {
    if !site.config.agents.enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    for e in site.entries.iter().filter(|e| e.page.kind == Kind::Page) {
        out.push(OutputFile {
            path: md_path(&e.page.url),
            contents: page_markdown(site, e),
        });
    }
    let home = site.home.map(|i| &site.entries[i]);
    out.push(OutputFile {
        path: "index.md".to_string(),
        contents: list_page(site, &site.config.title, "/", home, &site.regular),
    });
    for s in &site.sections {
        let intro = s.index.map(|i| &site.entries[i]);
        out.push(OutputFile {
            path: md_path(&s.url),
            contents: list_page(site, &s.title, &s.url, intro, &s.list),
        });
    }
    for t in &site.taxonomies {
        let mut md = format!(
            "---\ntitle: {:?}\nurl: {}\n---\n\n# {}\n\n",
            t.title,
            site.config.permalink(&t.url),
            t.title
        );
        for term in &t.terms {
            md.push_str(&format!(
                "- [{}]({}) ({} pages)\n",
                escape_link_text(&term.title),
                md_url(site, &term.url),
                term.pages.len()
            ));
        }
        out.push(OutputFile {
            path: md_path(&t.url),
            contents: md,
        });
        for term in &t.terms {
            let intro = term.index.map(|i| &site.entries[i]);
            out.push(OutputFile {
                path: md_path(&term.url),
                contents: list_page(site, &term.title, &term.url, intro, &term.pages),
            });
        }
    }
    out.push(llms_txt(site));
    out.push(llms_full(site));
    out
}

/// `/llms.txt` per the llms.txt spec (v2): H1, blockquote summary, then H2
/// sections of links; `## Optional` can be skipped when context is short.
fn llms_txt(site: &Site) -> OutputFile {
    let cfg = &site.config.agents;
    let mut md = format!("# {}\n\n", site.config.title);
    if let Some(summary) = cfg.summary.as_ref().or(site.config.description.as_ref()) {
        md.push_str(&format!("> {summary}\n\n"));
    }
    md.push_str(&format!(
        "Every page is also available as Markdown: append `index.md` to its URL (for example {}).\n",
        md_url(site, "/")
    ));
    for name in &cfg.taxonomies {
        let Some(t) = site.taxonomies.iter().find(|t| &t.name == name) else {
            continue;
        };
        md.push_str(&format!("\n## {}\n\n", t.title));
        for term in &t.terms {
            let summary = term
                .index
                .and_then(|i| {
                    let e = &site.entries[i];
                    e.page.fm.description.clone().or_else(|| {
                        e.param("summary")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                })
                .unwrap_or_else(|| format!("{} pages", term.pages.len()));
            md.push_str(&format!(
                "- [{}]({}): {}\n",
                escape_link_text(&term.title),
                md_url(site, &term.url),
                summary
            ));
        }
    }
    if let Some(section) = cfg
        .recent_section
        .as_ref()
        .and_then(|n| site.sections.iter().find(|s| &s.name == n))
    {
        md.push_str(&format!("\n## Recent {}\n\n", section.title.to_lowercase()));
        for &i in section.list.iter().take(cfg.recent_count) {
            md.push_str(&list_line(site, &site.entries[i]));
        }
    }
    md.push_str("\n## Optional\n\n");
    let mut by_year: Vec<(String, Vec<usize>)> = Vec::new();
    for &i in &site.regular {
        let year = site.entries[i]
            .date()
            .map(|d| d.format("%Y").to_string())
            .unwrap_or_else(|| "Undated".to_string());
        match by_year.last_mut() {
            Some((y, items)) if *y == year => items.push(i),
            _ => by_year.push((year, vec![i])),
        }
    }
    for (year, items) in by_year {
        md.push_str(&format!("\n### {year}\n\n"));
        for i in items {
            let e = &site.entries[i];
            md.push_str(&format!(
                "- [{}]({})\n",
                escape_link_text(e.title()),
                md_url(site, &e.page.url)
            ));
        }
    }
    OutputFile {
        path: "llms.txt".to_string(),
        contents: md,
    }
}

/// `/llms-full.txt`: the Markdown copies of the recent section's list,
/// newest first, until `full_max_bytes`.
fn llms_full(site: &Site) -> OutputFile {
    let cfg = &site.config.agents;
    let ids: &[usize] = cfg
        .recent_section
        .as_ref()
        .and_then(|n| site.sections.iter().find(|s| &s.name == n))
        .map(|s| s.list.as_slice())
        .unwrap_or(&site.regular);
    let mut md = format!("# {}\n\n", site.config.title);
    for &i in ids {
        let page = page_markdown(site, &site.entries[i]);
        if md.len() + page.len() > cfg.full_max_bytes {
            md.push_str(&format!(
                "\n<!-- truncated at {} bytes; see /llms.txt for the full index -->\n",
                cfg.full_max_bytes
            ));
            break;
        }
        md.push('\n');
        md.push_str(&page);
    }
    OutputFile {
        path: "llms-full.txt".to_string(),
        contents: md,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(md_path("/"), "index.md");
        assert_eq!(md_path("/post/x/"), "post/x/index.md");
    }

    #[test]
    fn words() {
        assert_eq!(first_words("a b c", 2), "a b…");
        assert_eq!(first_words("a b", 2), "a b");
    }
}
