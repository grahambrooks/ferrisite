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

use anyhow::{Context, Result};
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
    pub reference: Region,
    pub candidate: Region,
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
            let (Some(a), Some(b)) = (
                region(&ref_html, &content.reference),
                region(&cand_html, &content.candidate),
            ) else {
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

fn broken_links(
    root: &Path,
    files: &[String],
    base_url: &str,
    checked: &mut usize,
) -> Result<Vec<String>> {
    let set: BTreeSet<&String> = files.iter().collect();
    let mut id_cache: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut out = Vec::new();
    for f in files.iter().filter(|f| f.ends_with(".html")) {
        let html = read(root, f)?;
        if alias_target(&html).is_some() {
            continue;
        }
        let page_url = url_of(f);
        for link in internal_links(&html, &page_url, base_url) {
            *checked += 1;
            let (path, fragment) = link.split_once('#').unwrap_or((&link, ""));
            let Some(file) = resolve_file(path, &set) else {
                out.push(format!("{f} → {link}"));
                continue;
            };
            let fragment = percent_decode(fragment);
            if !fragment.is_empty() && file.ends_with(".html") {
                if !id_cache.contains_key(&file) {
                    let ids = element_ids(&read(root, &file)?);
                    id_cache.insert(file.clone(), ids);
                }
                if !id_cache[&file].contains(&fragment) {
                    out.push(format!("{f} → {link} (no such id)"));
                }
            }
        }
    }
    Ok(out)
}

fn read(root: &Path, rel: &str) -> Result<String> {
    let bytes = std::fs::read(root.join(rel)).with_context(|| format!("reading {rel}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn list_files(root: &Path) -> Result<Vec<String>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
        for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let p = e?.path();
            if p.is_dir() {
                walk(root, &p, out)?;
            } else {
                out.push(
                    p.strip_prefix(root)
                        .expect("under root")
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
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

/// The redirect target of a Hugo-style alias stub, if the page is one.
pub fn alias_target(html: &str) -> Option<String> {
    let head = &html[..html.len().min(1024)];
    let i = head.find("http-equiv=\"refresh\"")?;
    let rest = &head[i..];
    let u = rest.find("url=")? + 4;
    let end = rest[u..].find('"')?;
    Some(unescape(&rest[u..u + end]))
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

/// Site-relative URL of an output file: `post/x/index.html` → `/post/x/`.
fn url_of(file: &str) -> String {
    match file.strip_suffix("index.html") {
        Some(dir) => format!("/{dir}"),
        None => format!("/{file}"),
    }
}

/// Internal link targets in `html`, normalised to site-relative paths with
/// fragments kept (`/post/x/#heading`).
pub fn internal_links(html: &str, page_url: &str, base_url: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for attr in ["href=", "src="] {
        let mut rest = html;
        while let Some(i) = rest.find(attr) {
            let after = &rest[i + attr.len()..];
            rest = after;
            // Only real attributes: preceded by whitespace in the source.
            let quote = match after.chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => continue,
            };
            let Some(end) = after[1..].find(quote) else {
                continue;
            };
            let raw = unescape(&after[1..1 + end]);
            if let Some(link) = normalize(&raw, page_url, base_url) {
                out.insert(link);
            }
        }
    }
    out
}

fn normalize(raw: &str, page_url: &str, base_url: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    let path = if !base_url.is_empty() && raw.starts_with(base_url) {
        format!("/{}", &raw[base_url.len()..])
    } else if let Some(stripped) = base_url
        .strip_prefix("https://")
        .and_then(|host| raw.strip_prefix(&format!("http://{host}")))
    {
        format!("/{stripped}")
    } else if lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("mailto:")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("tel:")
        || raw.starts_with("//")
        || raw.contains("{{")
    {
        return None;
    } else if raw.starts_with('/') {
        raw.to_string()
    } else if raw.starts_with('#') {
        format!("{page_url}{raw}")
    } else {
        let dir = &page_url[..page_url.rfind('/').map(|i| i + 1).unwrap_or(0)];
        resolve_dots(&format!("{dir}{raw}"))
    };
    let path = path.split('?').next().unwrap_or(&path).to_string();
    let path = path.replace("//", "/");
    Some(path)
}

fn resolve_dots(path: &str) -> String {
    let (p, frag) = path
        .split_once('#')
        .map(|(a, b)| (a, Some(b)))
        .unwrap_or((path, None));
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut out = parts.join("/");
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    match frag {
        Some(f) => format!("{out}#{f}"),
        None => out,
    }
}

/// The candidate file a site-relative path serves, if any.
fn resolve_file(path: &str, files: &BTreeSet<&String>) -> Option<String> {
    let p = percent_decode(path.trim_start_matches('/'));
    let candidates = if p.is_empty() {
        vec!["index.html".to_string()]
    } else if p.ends_with('/') {
        vec![format!("{p}index.html")]
    } else {
        vec![p.clone(), format!("{p}/index.html")]
    };
    candidates
        .into_iter()
        .find(|c| files.contains(&c.to_string()))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn element_ids(html: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for attr in [" id=\"", " name=\""] {
        let mut rest = html;
        while let Some(i) = rest.find(attr) {
            let after = &rest[i + attr.len()..];
            if let Some(end) = after.find('"') {
                ids.insert(unescape(&after[..end]));
            }
            rest = after;
        }
    }
    ids
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

/// Decode the entities that HTML and XML serialisers emit.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(end) = after.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
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
    fn links_are_normalised() {
        let html = r##"<a href="https://www.e.com/post/a/#x">a</a> <a href="../b/">b</a>
            <a href="#top">t</a> <img src="/images/c.png"> <a href="https://other.com/">o</a>
            <a href="mailto:me@e.com">m</a> <a href="/tags/C&#43;&#43;/">c</a>"##;
        let links = internal_links(html, "/post/z/", "https://www.e.com/");
        let expect: BTreeSet<String> = [
            "/post/a/#x",
            "/post/b/",
            "/post/z/#top",
            "/images/c.png",
            "/tags/C++/",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(links, expect);
    }

    #[test]
    fn alias_and_entities() {
        let stub = "<html><head><meta http-equiv=\"refresh\" content=\"0; url=https://e.com/a&amp;b/\"></head></html>";
        assert_eq!(alias_target(stub).as_deref(), Some("https://e.com/a&b/"));
        assert_eq!(
            unescape("a &#43; b &lt;&#x27;&gt; &bogus"),
            "a + b <'> &bogus"
        );
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
