//! Fixed-format outputs: RSS feeds, sitemap, search index and alias stubs.
//! These follow Hugo's embedded templates, so feed readers and existing
//! subscribers see the same items.

use crate::render::OutputFile;
use crate::site::{Entry, Site};
use chrono::{DateTime, FixedOffset};

struct Item<'a> {
    title: &'a str,
    url: &'a str,
    date: Option<DateTime<FixedOffset>>,
    summary: &'a str,
}

struct Channel<'a> {
    title: String,
    description: String,
    url: &'a str,
    feed_path: &'a str,
}

pub fn all(site: &Site) -> Vec<OutputFile> {
    let mut out = Vec::new();
    feeds(site, &mut out);
    out.push(atom(site));
    out.push(json_feed(site));
    out.push(sitemap(site));
    out.push(search_index(site));
    for a in &site.aliases {
        out.push(OutputFile {
            path: a.path.clone(),
            contents: alias_stub(&site.config.permalink(&a.target)),
        });
    }
    out
}

fn item(e: &Entry) -> Item<'_> {
    Item {
        title: e.title(),
        url: &e.page.url,
        date: e.date(),
        summary: &e.summary,
    }
}

fn feeds(site: &Site, out: &mut Vec<OutputFile>) {
    let name = &site.config.title;
    let items =
        |ids: &[usize]| -> Vec<Item> { ids.iter().map(|&i| item(&site.entries[i])).collect() };

    let home = rss(
        site,
        &Channel {
            title: name.clone(),
            description: format!("Recent content on {name}"),
            url: "/",
            feed_path: "index.xml",
        },
        &items(&site.regular),
    );
    for copy in &site.config.home_feed_copies {
        out.push(OutputFile {
            path: copy.trim_start_matches('/').to_string(),
            contents: home.clone(),
        });
    }
    out.push(OutputFile {
        path: "index.xml".to_string(),
        contents: home,
    });

    for s in site.sections.iter().filter(|s| s.feed) {
        let path = format!("{}index.xml", s.url.trim_start_matches('/'));
        out.push(OutputFile {
            contents: rss(
                site,
                &Channel {
                    title: format!("{} on {name}", s.title),
                    description: format!("Recent content in {} on {name}", s.title),
                    url: &s.url,
                    feed_path: &path,
                },
                &items(&s.pages),
            ),
            path,
        });
    }

    for t in &site.taxonomies {
        // A taxonomy's feed lists its terms, dated by their newest page.
        let mut terms: Vec<&crate::site::Term> = t.terms.iter().collect();
        let newest = |term: &crate::site::Term| {
            term.pages
                .iter()
                .filter_map(|&i| site.entries[i].date())
                .max()
        };
        // Hugo orders term pages like any pages: weight (from the term's
        // content file; unweighted last), newest first, then title.
        let weight =
            |term: &crate::site::Term| match term.index.map(|i| site.entries[i].page.fm.weight) {
                None | Some(0) => i64::MAX,
                Some(w) => w,
            };
        terms.sort_by(|a, b| {
            weight(a)
                .cmp(&weight(b))
                .then_with(|| newest(b).cmp(&newest(a)))
                .then_with(|| a.title.cmp(&b.title))
        });
        let term_items: Vec<Item> = terms
            .into_iter()
            .map(|term| Item {
                title: &term.title,
                url: &term.url,
                date: term
                    .pages
                    .iter()
                    .filter_map(|&i| site.entries[i].date())
                    .max(),
                summary: term
                    .index
                    .map(|i| site.entries[i].summary.as_str())
                    .unwrap_or(""),
            })
            .collect();
        let path = format!("{}index.xml", t.url.trim_start_matches('/'));
        out.push(OutputFile {
            contents: rss(
                site,
                &Channel {
                    title: format!("{} on {name}", t.title),
                    description: format!("Recent content in {} on {name}", t.title),
                    url: &t.url,
                    feed_path: &path,
                },
                &term_items,
            ),
            path,
        });
        for term in &t.terms {
            let path = format!("{}index.xml", term.url.trim_start_matches('/'));
            out.push(OutputFile {
                contents: rss(
                    site,
                    &Channel {
                        title: format!("{} on {name}", term.title),
                        description: format!("Recent content in {} on {name}", term.title),
                        url: &term.url,
                        feed_path: &path,
                    },
                    &items(&term.pages),
                ),
                path,
            });
        }
    }

    for f in &site.config.feeds {
        let mut ids: Vec<usize> = site
            .regular
            .iter()
            .copied()
            .filter(|&i| {
                let e = &site.entries[i];
                e.page.url.starts_with(&format!("/{}/", f.section))
                    && crate::site::filter_matches(&e.page, &f.include, &f.exclude)
            })
            .collect();
        if f.order == crate::config::FeedOrder::DateReverse {
            ids.sort_by(|&a, &b| {
                crate::site::by_date_reverse(&site.entries[a].page, &site.entries[b].page)
            });
        }
        let path = f.path.trim_start_matches('/').to_string();
        out.push(OutputFile {
            contents: rss(
                site,
                &Channel {
                    title: f.title.clone(),
                    description: f.description.clone(),
                    url: "/",
                    feed_path: &path,
                },
                &items(&ids),
            ),
            path,
        });
    }
}

fn rss(site: &Site, ch: &Channel, items: &[Item]) -> String {
    let cfg = &site.config;
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>\n");
    x.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n  <channel>\n");
    x.push_str(&format!("    <title>{}</title>\n", xml(&ch.title)));
    x.push_str(&format!(
        "    <link>{}</link>\n",
        xml(&cfg.permalink(ch.url))
    ));
    x.push_str(&format!(
        "    <description>{}</description>\n",
        xml(&ch.description)
    ));
    x.push_str("    <generator>ferrisite</generator>\n");
    x.push_str(&format!(
        "    <language>{}</language>\n",
        xml(&cfg.language)
    ));
    if let Some(c) = &cfg.copyright {
        x.push_str(&format!("    <copyright>{}</copyright>\n", xml(c)));
    }
    if let Some(latest) = items.iter().filter_map(|i| i.date).max() {
        x.push_str(&format!(
            "    <lastBuildDate>{}</lastBuildDate>\n",
            rfc1123(Some(latest))
        ));
    }
    x.push_str(&format!(
        "    <atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\" />\n",
        xml(&cfg.permalink(ch.feed_path))
    ));
    for i in items {
        let link = cfg.permalink(i.url);
        x.push_str("    <item>\n");
        x.push_str(&format!("      <title>{}</title>\n", xml(i.title)));
        x.push_str(&format!("      <link>{}</link>\n", xml(&link)));
        x.push_str(&format!("      <pubDate>{}</pubDate>\n", rfc1123(i.date)));
        x.push_str(&format!("      <guid>{}</guid>\n", xml(&link)));
        x.push_str(&format!(
            "      <description>{}</description>\n",
            xml(&absolutize(i.summary, &cfg.base_url))
        ));
        x.push_str("    </item>\n");
    }
    x.push_str("  </channel>\n</rss>\n");
    x
}

/// Hugo's RSS date format; an undated page gets Go's zero time, as in Hugo.
fn rfc1123(d: Option<DateTime<FixedOffset>>) -> String {
    match d {
        Some(d) => d.format("%a, %d %b %Y %H:%M:%S %z").to_string(),
        None => "Mon, 01 Jan 0001 00:00:00 +0000".to_string(),
    }
}

/// Rewrite root-relative `src="/..."` and `href="/..."` to absolute URLs, as
/// the site's Hugo feed template does, so feed readers can follow them.
fn absolutize(html: &str, base: &str) -> String {
    let mut out = html.to_string();
    for attr in ["src", "href"] {
        for q in ['"', '\''] {
            let from = format!("{attr}={q}/");
            let mut result = String::with_capacity(out.len());
            let mut rest = out.as_str();
            while let Some(i) = rest.find(&from) {
                result.push_str(&rest[..i]);
                let after = &rest[i + from.len()..];
                if after.starts_with('/') {
                    result.push_str(&from); // protocol-relative: leave alone
                } else {
                    result.push_str(&format!("{attr}={q}{base}"));
                }
                rest = after;
            }
            result.push_str(rest);
            out = result;
        }
    }
    out
}

/// `/atom.xml`: the home feed as Atom 1.0.
fn atom(site: &Site) -> OutputFile {
    let cfg = &site.config;
    let entries: Vec<&Entry> = site.regular.iter().map(|&i| &site.entries[i]).collect();
    let updated = entries
        .iter()
        .filter_map(|e| e.page.fm.lastmod.or(e.date()))
        .max();
    let iso = |d: DateTime<FixedOffset>| d.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
    let mut x = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n",
    );
    x.push_str(&format!("  <title>{}</title>\n", xml(&cfg.title)));
    x.push_str(&format!("  <id>{}</id>\n", xml(&cfg.base_url)));
    x.push_str(&format!("  <link href=\"{}\"/>\n", xml(&cfg.base_url)));
    x.push_str(&format!(
        "  <link rel=\"self\" href=\"{}\"/>\n",
        xml(&cfg.permalink("atom.xml"))
    ));
    if let Some(u) = updated {
        x.push_str(&format!("  <updated>{}</updated>\n", iso(u)));
    }
    if let Some(a) = &cfg.author {
        x.push_str(&format!("  <author><name>{}</name></author>\n", xml(a)));
    }
    for e in entries {
        let link = cfg.permalink(&e.page.url);
        x.push_str("  <entry>\n");
        x.push_str(&format!("    <title>{}</title>\n", xml(e.title())));
        x.push_str(&format!("    <link href=\"{}\"/>\n", xml(&link)));
        x.push_str(&format!("    <id>{}</id>\n", xml(&link)));
        if let Some(d) = e.date() {
            x.push_str(&format!("    <published>{}</published>\n", iso(d)));
            x.push_str(&format!(
                "    <updated>{}</updated>\n",
                iso(e.page.fm.lastmod.unwrap_or(d))
            ));
        }
        x.push_str(&format!(
            "    <summary type=\"html\">{}</summary>\n",
            xml(&absolutize(&e.summary, &cfg.base_url))
        ));
        x.push_str("  </entry>\n");
    }
    x.push_str("</feed>\n");
    OutputFile {
        path: "atom.xml".to_string(),
        contents: x,
    }
}

/// `/feed.json`: the home feed as JSON Feed 1.1.
fn json_feed(site: &Site) -> OutputFile {
    let cfg = &site.config;
    let items: Vec<serde_json::Value> = site
        .regular
        .iter()
        .map(|&i| {
            let e = &site.entries[i];
            let link = cfg.permalink(&e.page.url);
            let mut item = serde_json::json!({
                "id": link,
                "url": link,
                "title": e.title(),
                "content_html": absolutize(&e.summary, &cfg.base_url),
            });
            if let Some(d) = e.date() {
                item["date_published"] = d.to_rfc3339().into();
            }
            item
        })
        .collect();
    let mut feed = serde_json::json!({
        "version": "https://jsonfeed.org/version/1.1",
        "title": cfg.title,
        "home_page_url": cfg.base_url,
        "feed_url": cfg.permalink("feed.json"),
        "items": items,
    });
    if let Some(a) = &cfg.author {
        feed["authors"] = serde_json::json!([{ "name": a }]);
    }
    OutputFile {
        path: "feed.json".to_string(),
        contents: serde_json::to_string_pretty(&feed).expect("feed serialises") + "\n",
    }
}

pub fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&#34;")
        .replace('\'', "&#39;")
}

fn sitemap(site: &Site) -> OutputFile {
    let mut urls: Vec<(String, Option<DateTime<FixedOffset>>)> = Vec::new();
    let lastmod = |e: &Entry| e.page.fm.lastmod.or(e.page.fm.date);
    let newest = |ids: &[usize]| ids.iter().filter_map(|&i| lastmod(&site.entries[i])).max();
    urls.push(("/".to_string(), newest(&site.regular)));
    for s in &site.sections {
        urls.push((s.url.clone(), newest(&s.pages)));
    }
    for t in &site.taxonomies {
        urls.push((t.url.clone(), None));
        for term in &t.terms {
            urls.push((term.url.clone(), newest(&term.pages)));
        }
    }
    for e in site
        .entries
        .iter()
        .filter(|e| e.page.kind == crate::content::Kind::Page)
    {
        urls.push((e.page.url.clone(), lastmod(e)));
    }
    urls.sort();
    urls.dedup_by(|a, b| a.0 == b.0);
    let mut x = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for (url, lm) in urls {
        x.push_str(&format!(
            "  <url>\n    <loc>{}</loc>\n",
            xml(&site.config.permalink(&url))
        ));
        if let Some(d) = lm {
            x.push_str(&format!(
                "    <lastmod>{}</lastmod>\n",
                d.format("%Y-%m-%dT%H:%M:%S%:z")
            ));
        }
        x.push_str("  </url>\n");
    }
    x.push_str("</urlset>\n");
    OutputFile {
        path: "sitemap.xml".to_string(),
        contents: x,
    }
}

#[derive(serde::Serialize)]
struct Doc<'a> {
    body: &'a str,
    link: String,
    section: &'a str,
    title: &'a str,
}

fn doc<'a>(site: &'a Site, e: &'a Entry) -> Option<Doc<'a>> {
    (e.param("type").and_then(|v| v.as_str()) != Some("search")).then(|| Doc {
        body: &e.plain,
        link: site.config.permalink(&e.page.url),
        section: &e.page.section,
        title: e.title(),
    })
}

/// `/index.json` for the site's search box: every page in global collections
/// except the search page itself, as Hugo's theme template builds it.
fn search_index(site: &Site) -> OutputFile {
    let mut docs = Vec::new();
    if let Some(h) = site.home {
        docs.extend(doc(site, &site.entries[h]));
    }
    for s in &site.sections {
        match s.index {
            Some(i) => docs.extend(doc(site, &site.entries[i])),
            None => docs.push(Doc {
                body: "",
                link: site.config.permalink(&s.url),
                section: &s.name,
                title: &s.title,
            }),
        }
    }
    for t in &site.taxonomies {
        docs.push(Doc {
            body: t
                .index
                .map(|i| site.entries[i].plain.as_str())
                .unwrap_or(""),
            link: site.config.permalink(&t.url),
            section: &t.name,
            title: &t.title,
        });
        for term in &t.terms {
            docs.push(Doc {
                body: term
                    .index
                    .map(|i| site.entries[i].plain.as_str())
                    .unwrap_or(""),
                link: site.config.permalink(&term.url),
                section: &t.name,
                title: &term.title,
            });
        }
    }
    for &i in &site.regular {
        docs.extend(doc(site, &site.entries[i]));
    }
    OutputFile {
        path: "index.json".to_string(),
        contents: serde_json::to_string(&docs).expect("search index serialises"),
    }
}

/// Hugo's alias page, byte for byte apart from the target.
pub fn alias_stub(permalink: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en-us\">\n\t<head>\n\t\t<title>{p}</title>\n\t\t<link rel=\"canonical\" href=\"{p}\">\n\t\t<meta charset=\"utf-8\">\n\t\t<meta http-equiv=\"refresh\" content=\"0; url={p}\">\n\t</head>\n</html>\n",
        p = xml(permalink)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolutizes_root_relative_links_only() {
        let html = r#"<a href="/post/x/">a</a> <img src="/images/y.png"> <a href="//cdn/z">c</a> <a href="https://e.com/">d</a>"#;
        assert_eq!(
            absolutize(html, "https://www.example.com/"),
            r#"<a href="https://www.example.com/post/x/">a</a> <img src="https://www.example.com/images/y.png"> <a href="//cdn/z">c</a> <a href="https://e.com/">d</a>"#
        );
    }

    #[test]
    fn rss_dates() {
        let d = DateTime::parse_from_rfc3339("2026-10-25T09:07:00-06:00").unwrap();
        assert_eq!(rfc1123(Some(d)), "Sun, 25 Oct 2026 09:07:00 -0600");
        assert_eq!(rfc1123(None), "Mon, 01 Jan 0001 00:00:00 +0000");
    }

    #[test]
    fn alias_stub_matches_hugo() {
        assert_eq!(
            alias_stub("https://www.grahambrooks.com/post/2012-12-31-drying-out-code/"),
            "<!DOCTYPE html>\n<html lang=\"en-us\">\n\t<head>\n\t\t<title>https://www.grahambrooks.com/post/2012-12-31-drying-out-code/</title>\n\t\t<link rel=\"canonical\" href=\"https://www.grahambrooks.com/post/2012-12-31-drying-out-code/\">\n\t\t<meta charset=\"utf-8\">\n\t\t<meta http-equiv=\"refresh\" content=\"0; url=https://www.grahambrooks.com/post/2012-12-31-drying-out-code/\">\n\t</head>\n</html>\n"
        );
    }
}
