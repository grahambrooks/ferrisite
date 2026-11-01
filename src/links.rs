//! Internal links in a built site: listing its files, finding the links and
//! ids in its pages, and reporting links that resolve to no file or no id.
//! Used by `ferrisite check`, and by `ferrisite parity` while the Hugo build
//! is still the reference.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn broken_links(
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

pub(crate) fn read(root: &Path, rel: &str) -> Result<String> {
    let bytes = std::fs::read(root.join(rel)).with_context(|| format!("reading {rel}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn list_files(root: &Path) -> Result<Vec<String>> {
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

/// The redirect target of a Hugo-style alias stub, if the page is one.
pub fn alias_target(html: &str) -> Option<String> {
    let head = &html[..html.len().min(1024)];
    let i = head.find("http-equiv=\"refresh\"")?;
    let rest = &head[i..];
    let u = rest.find("url=")? + 4;
    let end = rest[u..].find('"')?;
    Some(unescape(&rest[u..u + end]))
}

/// Site-relative URL of an output file: `post/x/index.html` → `/post/x/`.
pub(crate) fn url_of(file: &str) -> String {
    match file.strip_suffix("index.html") {
        Some(dir) => format!("/{dir}"),
        None => format!("/{file}"),
    }
}

/// Internal link targets in `html`, normalised to site-relative paths with
/// fragments kept (`/post/x/#heading`): the `href` and `src` attributes of
/// every tag (and `xlink:href`). Script contents and text aren't links.
pub fn internal_links(html: &str, page_url: &str, base_url: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for token in crate::html::tokens(html) {
        let crate::html::Token::Start(tag) = token else {
            continue;
        };
        for a in &tag.attrs {
            if !matches!(a.name.as_str(), "href" | "src" | "xlink:href") {
                continue;
            }
            let Some(r) = a.value.clone() else { continue };
            if let Some(link) = normalize(&unescape(&html[r]), page_url, base_url) {
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

/// Fragment targets in a page: every `id`, and `name` on `<a>`.
fn element_ids(html: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for token in crate::html::tokens(html) {
        let crate::html::Token::Start(tag) = token else {
            continue;
        };
        for a in &tag.attrs {
            if (a.name == "id" || (a.name == "name" && tag.name == "a"))
                && let Some(r) = a.value.clone()
            {
                ids.insert(unescape(&html[r]));
            }
        }
    }
    ids
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_contents_are_not_links() {
        let html = "<script src=\"/js/a.js\"></script><script>x=`<use href=\"#${e}\">`</script><a href=\"/b/\">b</a>";
        let links = internal_links(html, "/", "https://www.e.com/");
        assert_eq!(
            links.into_iter().collect::<Vec<_>>(),
            vec!["/b/".to_string(), "/js/a.js".to_string()]
        );
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
}
