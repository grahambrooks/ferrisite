//! `ferrisite parity`: prove a candidate build serves everything a reference
//! build does (DESIGN.md section 11).
//!
//! Checks, each failing the run:
//! 1. every reference file exists in the candidate (minus `ignore` patterns);
//! 2. every reference alias stub redirects to the same target;
//! 3. for configured pages, the internal links inside the content region match,
//!    and no internal link anywhere in the candidate is broken (fragments
//!    included);
//! 4. every RSS feed lists the same items (guid, link, title, pubDate) in order.
//!
//! Feed description differences are counted but don't fail the run.

use crate::links::{
    alias_target, broken_links, internal_links, list_files, read, unescape, url_of,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParityConfig {
    /// The site's base URL, so absolute links can be treated as internal.
    #[serde(default)]
    pub base_url: String,
    /// Reference files that may be missing from the candidate, with why.
    #[serde(default)]
    pub ignore: Vec<Ignore>,
    #[serde(default)]
    pub content: Option<ContentRegions>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ignore {
    pub pattern: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRegions {
    /// Globs (relative to the build root) of pages whose content links are compared.
    pub pages: Vec<String>,
    /// Skip links to a fragment of the same page (heading anchors, tables of
    /// contents) when comparing; the broken-link check still verifies them.
    #[serde(default)]
    pub ignore_self_fragments: bool,
    /// Links left out of the comparison on both sides, each with a reason:
    /// site-relative globs (`*` within a segment, `**` across).
    #[serde(default)]
    pub ignore_links: Vec<Ignore>,
    /// The part of each page to compare; the whole page when neither is
    /// given. Give both or neither.
    #[serde(default)]
    pub reference: Option<Region>,
    #[serde(default)]
    pub candidate: Option<Region>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub start: String,
    /// The region ends at the first of these markers (or the end of the page).
    pub end: Vec<String>,
    /// Spans removed from the region before links are compared, e.g. a
    /// generated table of contents.
    #[serde(default)]
    pub strip: Vec<Span>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub reference_files: usize,
    pub candidate_files: usize,
    pub ignored: BTreeMap<String, usize>,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub alias_mismatches: Vec<String>,
    pub content_link_mismatches: Vec<String>,
    pub content_pages_compared: usize,
    pub broken_links: Vec<String>,
    /// Broken in the candidate and equally broken in the reference.
    pub known_broken_links: Vec<String>,
    pub links_checked: usize,
    pub feed_mismatches: Vec<String>,
    pub feeds_compared: usize,
    pub feed_description_differences: usize,
}

impl Report {
    pub fn failed(&self) -> bool {
        !(self.missing.is_empty()
            && self.alias_mismatches.is_empty()
            && self.content_link_mismatches.is_empty()
            && self.broken_links.is_empty()
            && self.feed_mismatches.is_empty())
    }

    pub fn print(&self, limit: usize) {
        let mark = |ok: bool| if ok { "ok  " } else { "FAIL" };
        println!(
            "files: reference {}, candidate {}",
            self.reference_files, self.candidate_files
        );
        for (pattern, n) in &self.ignored {
            println!("  ignored {n:>4} matching {pattern}");
        }
        let sections: [(&str, &Vec<String>, String); 5] = [
            ("1. missing files", &self.missing, String::new()),
            ("2. alias targets", &self.alias_mismatches, String::new()),
            (
                "3. content links",
                &self.content_link_mismatches,
                format!(" ({} pages compared)", self.content_pages_compared),
            ),
            (
                "3. broken internal links",
                &self.broken_links,
                format!(" ({} links checked)", self.links_checked),
            ),
            (
                "4. feed items",
                &self.feed_mismatches,
                format!(
                    " ({} feeds compared; {} item descriptions differ, not a failure)",
                    self.feeds_compared, self.feed_description_differences
                ),
            ),
        ];
        for (name, list, note) in sections {
            println!("{} {name}: {}{note}", mark(list.is_empty()), list.len());
            for line in list.iter().take(limit) {
                println!("       {line}");
            }
            if list.len() > limit {
                println!("       … and {} more", list.len() - limit);
            }
        }
        println!(
            "     already broken in the reference too (not a failure): {}",
            self.known_broken_links.len()
        );
        for line in self.known_broken_links.iter().take(limit) {
            println!("       {line}");
        }
        println!("     extra files in candidate: {}", self.extra.len());
        for line in self.extra.iter().take(limit) {
            println!("       {line}");
        }
    }
}

pub fn compare(reference: &Path, candidate: &Path, cfg: &ParityConfig) -> Result<Report> {
    let ref_files = list_files(reference)?;
    let cand_files = list_files(candidate)?;
    let cand_set: BTreeSet<&String> = cand_files.iter().collect();
    let ref_set: BTreeSet<&String> = ref_files.iter().collect();
    let mut r = Report {
        reference_files: ref_files.len(),
        candidate_files: cand_files.len(),
        ..Report::default()
    };

    // 1. Missing files.
    for f in &ref_files {
        if let Some(ig) = cfg.ignore.iter().find(|i| glob(&i.pattern, f)) {
            *r.ignored.entry(ig.pattern.clone()).or_default() += 1;
            continue;
        }
        if !cand_set.contains(f) {
            r.missing.push(f.clone());
        }
    }
    r.extra = cand_files
        .iter()
        .filter(|f| !ref_set.contains(f) && !f.starts_with(".ferrisite"))
        .cloned()
        .collect();

    // 2. Alias targets.
    for f in ref_files.iter().filter(|f| f.ends_with(".html")) {
        let Some(target) = alias_target(&read(reference, f)?) else {
            continue;
        };
        if !cand_set.contains(f) {
            continue; // already reported as missing
        }
        match alias_target(&read(candidate, f)?) {
            Some(t) if t == target => {}
            Some(t) => r
                .alias_mismatches
                .push(format!("{f}: reference → {target}, candidate → {t}")),
            None => r.alias_mismatches.push(format!(
                "{f}: reference redirects to {target}; candidate is a page"
            )),
        }
    }

    // 3a. Content links on configured pages.
    if let Some(content) = &cfg.content {
        for f in ref_files
            .iter()
            .filter(|f| content.pages.iter().any(|p| glob(p, f)) && cand_set.contains(f))
        {
            let ref_html = read(reference, f)?;
            if alias_target(&ref_html).is_some() {
                continue;
            }
            let cand_html = read(candidate, f)?;
            let regions = match (&content.reference, &content.candidate) {
                (Some(ra), Some(rb)) => (region(&ref_html, ra), region(&cand_html, rb)),
                (None, None) => (Some(ref_html.clone()), Some(cand_html.clone())),
                _ => {
                    bail!("parity config: [content.reference] and [content.candidate] go together")
                }
            };
            let (Some(a), Some(b)) = regions else {
                r.content_link_mismatches
                    .push(format!("{f}: content region markers not found"));
                continue;
            };
            r.content_pages_compared += 1;
            let page_url = url_of(f);
            let mut la = internal_links(&a, &page_url, &cfg.base_url);
            let mut lb = internal_links(&b, &page_url, &cfg.base_url);
            if content.ignore_self_fragments {
                let own = format!("{page_url}#");
                la.retain(|l| !l.starts_with(&own));
                lb.retain(|l| !l.starts_with(&own));
            }
            for ig in &content.ignore_links {
                la.retain(|l| !glob(&ig.pattern, l));
                lb.retain(|l| !glob(&ig.pattern, l));
            }
            if la != lb {
                let only_ref: Vec<_> = la.difference(&lb).take(3).cloned().collect();
                let only_cand: Vec<_> = lb.difference(&la).take(3).cloned().collect();
                r.content_link_mismatches.push(format!(
                    "{f}: only in reference {only_ref:?}, only in candidate {only_cand:?}"
                ));
            }
        }
    }

    // 3b. Broken internal links anywhere in the candidate. A link that is
    // broken the same way in the reference is reported as known, not failed.
    let cand_broken = broken_links(candidate, &cand_files, &cfg.base_url, &mut r.links_checked)?;
    let mut ignore_count = 0;
    // Known if the same target is broken anywhere in the reference.
    let target = |b: &str| {
        b.split_once(" → ")
            .map(|(_, t)| t.to_string())
            .unwrap_or_default()
    };
    let ref_broken: BTreeSet<String> =
        broken_links(reference, &ref_files, &cfg.base_url, &mut ignore_count)?
            .iter()
            .map(|b| target(b))
            .collect();
    for b in cand_broken {
        if ref_broken.contains(&target(&b)) {
            r.known_broken_links.push(b);
        } else {
            r.broken_links.push(b);
        }
    }

    // 4. Feeds.
    for f in ref_files.iter().filter(|f| f.ends_with(".xml")) {
        let ref_xml = read(reference, f)?;
        if !ref_xml.contains("<rss") {
            continue;
        }
        if !cand_set.contains(f) {
            continue; // reported as missing
        }
        r.feeds_compared += 1;
        let a = feed_items(&ref_xml);
        let b = feed_items(&read(candidate, f)?);
        let key = |i: &FeedItem| {
            (
                i.guid.clone(),
                i.link.clone(),
                i.title.clone(),
                i.pub_date.clone(),
            )
        };
        let ka: Vec<_> = a.iter().map(key).collect();
        let kb: Vec<_> = b.iter().map(key).collect();
        if ka != kb {
            let first = ka
                .iter()
                .zip(&kb)
                .position(|(x, y)| x != y)
                .unwrap_or(ka.len().min(kb.len()));
            r.feed_mismatches.push(format!(
                "{f}: {} vs {} items; first difference at item {first}: reference {:?}, candidate {:?}",
                ka.len(),
                kb.len(),
                ka.get(first),
                kb.get(first)
            ));
        }
        r.feed_description_differences += a
            .iter()
            .zip(&b)
            .filter(|(x, y)| normalize_space(&x.description) != normalize_space(&y.description))
            .count();
    }
    Ok(r)
}

/// `*` matches within a path segment, `**` across segments.
pub fn glob(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = &p[2..];
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            Some(b'*') => {
                let rest = &p[1..];
                let mut i = 0;
                loop {
                    if go(rest, &s[i..]) {
                        return true;
                    }
                    if i == s.len() || s[i] == b'/' {
                        return false;
                    }
                    i += 1;
                }
            }
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

fn region(html: &str, r: &Region) -> Option<String> {
    let s = html.find(&r.start)? + r.start.len();
    let e = r
        .end
        .iter()
        .filter_map(|m| html[s..].find(m.as_str()).map(|e| s + e))
        .min()
        .unwrap_or(html.len());
    let mut text = html[s..e].to_string();
    for span in &r.strip {
        while let Some(a) = text.find(&span.start) {
            let Some(b) = text[a..].find(&span.end) else {
                break;
            };
            text.replace_range(a..a + b + span.end.len(), "");
        }
    }
    Some(text)
}

#[derive(Debug)]
struct FeedItem {
    title: String,
    link: String,
    guid: String,
    pub_date: String,
    description: String,
}

fn feed_items(xml: &str) -> Vec<FeedItem> {
    let tag = |block: &str, name: &str| -> String {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        block
            .find(&open)
            .and_then(|s| {
                let s = s + open.len();
                block[s..].find(&close).map(|e| unescape(&block[s..s + e]))
            })
            .unwrap_or_default()
    };
    xml.split("<item>")
        .skip(1)
        .map(|block| {
            let block = block.split("</item>").next().unwrap_or(block);
            FeedItem {
                title: tag(block, "title"),
                link: tag(block, "link"),
                guid: tag(block, "guid"),
                pub_date: tag(block, "pubDate"),
                description: unescape(&tag(block, "description")),
            }
        })
        .collect()
}

fn normalize_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn load_config(path: Option<&Path>) -> Result<ParityConfig> {
    match path {
        None => Ok(ParityConfig::default()),
        Some(p) => {
            let text =
                std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob("images/og-*.png", "images/og-base_hu_1.png"));
        assert!(!glob("images/*.png", "images/a/b.png"));
        assert!(glob("post/*/index.html", "post/x/index.html"));
        assert!(glob("**/*.css", "a/b/c.css"));
        assert!(glob("js/**", "js/x/y.js"));
    }

    #[test]
    fn feed_item_parsing() {
        let xml = "<rss><channel><item><title>A &amp; B</title><link>https://e/x/</link><pubDate>D</pubDate><guid>https://e/x/</guid><description>&lt;p&gt;hi&lt;/p&gt;</description></item></channel></rss>";
        let items = feed_items(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "A & B");
        assert_eq!(items[0].description, "<p>hi</p>");
    }
}
