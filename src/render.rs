//! Render: entries and lists through minijinja templates.
//!
//! Template lookup: a site's `templates/` directory overrides the built-in
//! defaults by name. For a page in section `post`, `post/single.html` is tried
//! before `single.html`; top-level pages use `page.html`; section lists try
//! `<section>/list.html` then `list.html`; term pages `<taxonomy>/term.html`
//! then `term.html`; taxonomy indexes `<taxonomy>/taxonomy.html` then
//! `taxonomy.html`; the home page `home.html`; plus `404.html`, `robots.txt`
//! and shortcodes in `shortcodes/<name>.html`. See TEMPLATES.md.

use crate::config::Config;
use crate::content::Kind;
use crate::markup::{self, Heading};
use crate::site::{Brief, Entry, Pager, Section, Site, Taxonomy, Term};
use anyhow::{Context, Result};
use minijinja::{Environment, Value, context};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

const BUILTIN: &[(&str, &str)] = &[
    ("base.html", include_str!("templates/base.html")),
    ("single.html", include_str!("templates/single.html")),
    ("page.html", include_str!("templates/page.html")),
    ("list.html", include_str!("templates/list.html")),
    ("home.html", include_str!("templates/home.html")),
    ("term.html", include_str!("templates/term.html")),
    ("taxonomy.html", include_str!("templates/taxonomy.html")),
    ("404.html", include_str!("templates/404.html")),
    ("robots.txt", include_str!("templates/robots.txt")),
];

/// How many recent pages the home template gets.
const RECENT: usize = 10;

#[derive(Serialize, Clone)]
struct SiteCtx {
    title: String,
    base_url: String,
    author: Option<String>,
    description: Option<String>,
    params: BTreeMap<String, toml::Value>,
    /// Absolute URL of the generated stylesheet (highlighting, diagram modes).
    stylesheet: String,
    /// robots.txt `Content-Signal:` value, if configured.
    content_signal: Option<String>,
}

#[derive(Serialize)]
struct PageCtx<'a> {
    kind: &'static str,
    title: &'a str,
    url: &'a str,
    permalink: String,
    /// Absolute URL of the page's Markdown copy (`<link rel="alternate">`).
    markdown_url: String,
    /// Absolute URL of the page's social card (when cards are enabled).
    card: Option<String>,
    section: &'a str,
    date: Option<String>,
    lastmod: Option<String>,
    description: Option<&'a str>,
    summary: Value,
    content: Option<Value>,
    headings: &'a [Heading],
    word_count: usize,
    reading_time: usize,
    params: &'a serde_yaml::Mapping,
}

#[derive(Serialize)]
struct PagerCtx<'a> {
    url: &'a str,
    number: usize,
    total: usize,
    prev: Option<&'a str>,
    next: Option<&'a str>,
    items: Vec<PageCtx<'a>>,
}

#[derive(Serialize)]
struct TermCtx<'a> {
    name: &'a str,
    title: &'a str,
    url: &'a str,
    count: usize,
}

pub struct Renderer {
    env: Environment<'static>,
    site: SiteCtx,
}

/// One rendered file: site-relative output path and its contents.
pub struct OutputFile {
    pub path: String,
    pub contents: String,
}

impl Renderer {
    pub fn new(config: &Config, site_templates: &Path) -> Result<Renderer> {
        let mut env = Environment::new();
        for (name, source) in BUILTIN {
            env.add_template_owned(*name, *source)
                .with_context(|| format!("built-in template {name}"))?;
        }
        if site_templates.is_dir() {
            let mut files = Vec::new();
            collect_templates(site_templates, site_templates, &mut files)?;
            files.sort();
            for (name, path) in files {
                let source = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading template {}", path.display()))?;
                env.add_template_owned(name.clone(), source)
                    .with_context(|| format!("template {name}"))?;
            }
        }
        env.set_formatter(html_formatter);
        env.add_filter("date", date_filter);
        env.add_filter("markdownify", |s: String| {
            Value::from_safe_string(markup::markdownify(&s))
        });
        let card_base = config.base_url.clone();
        env.add_function("card", move |url: String| {
            format!("{card_base}{}", crate::cards::card_path(&url))
        });
        let base = config.base_url.clone();
        env.add_function("absurl", move |path: String| {
            format!("{}{}", base, path.trim_start_matches('/'))
        });
        Ok(Renderer {
            env,
            site: SiteCtx {
                title: config.title.clone(),
                base_url: config.base_url.clone(),
                author: config.author.clone(),
                description: config.description.clone(),
                params: config.params.clone(),
                stylesheet: config.permalink(&config.markup.stylesheet),
                content_signal: config.agents.content_signal.clone(),
            },
        })
    }

    /// Render `shortcodes/<name>.html`, or `None` if the site has no such
    /// template. Context: `args`, `inner` (already safe HTML/text), `page`
    /// (the calling page, as a brief), `lists` (section name → list items),
    /// `site`.
    pub fn render_shortcode(
        &self,
        name: &str,
        args: &[String],
        inner: Option<&str>,
        page: &Brief,
        lists: &BTreeMap<String, Vec<Brief>>,
    ) -> Result<Option<String>> {
        self.render_shortcode_as(name, "html", args, inner, page, lists)
    }

    /// `shortcodes/<name>.<ext>`: `html` for pages, `md` for Markdown copies.
    pub fn render_shortcode_as(
        &self,
        name: &str,
        ext: &str,
        args: &[String],
        inner: Option<&str>,
        page: &Brief,
        lists: &BTreeMap<String, Vec<Brief>>,
    ) -> Result<Option<String>> {
        let template = format!("shortcodes/{name}.{ext}");
        if self.env.get_template(&template).is_err() {
            return Ok(None);
        }
        let ctx = context! {
            site => ser(&self.site),
            args => ser(&args),
            inner => inner.map(|i| Value::from_safe_string(i.to_string())),
            page => ser(page),
            lists => ser(lists),
        };
        self.render(&[template], ctx).map(Some)
    }

    pub fn render_site(&self, site: &Site) -> Result<Vec<OutputFile>> {
        let site_ctx = ser(&self.site);
        let mut out = Vec::new();
        for entry in &site.entries {
            let template = match (entry.page.kind, entry.page.section.as_str()) {
                (Kind::Page, "") => vec!["page.html".to_string()],
                (Kind::Page, section) => {
                    vec![format!("{section}/single.html"), "single.html".to_string()]
                }
                // Home, section, taxonomy and term pages render with their lists below.
                _ => continue,
            };
            let html = self
                .render(
                    &template,
                    context! { site => site_ctx.clone(), current => self.current(site, &entry.page.url), page => ser(&page_ctx(site, entry, true)) },
                )
                .with_context(|| format!("rendering {}", entry.page.source))?;
            out.push(OutputFile {
                path: index_path(&entry.page.url),
                contents: html,
            });
        }
        for section in &site.sections {
            let page = section
                .index
                .map(|i| page_ctx(site, &site.entries[i], true));
            for pager in &section.pagers {
                let html = self
                    .render(
                        &[
                            format!("{}/list.html", section.name),
                            "list.html".to_string(),
                        ],
                        context! {
                            site => site_ctx.clone(), current => self.current(site, &pager.url),
                            page => ser(&page),
                            section => section_ctx(section),
                            pager => ser(&pager_ctx(site, pager)),
                        },
                    )
                    .with_context(|| format!("rendering list {}", pager.url))?;
                out.push(OutputFile {
                    path: index_path(&pager.url),
                    contents: html,
                });
            }
        }
        for taxonomy in &site.taxonomies {
            out.push(self.render_taxonomy(site, taxonomy, &site_ctx)?);
            for term in &taxonomy.terms {
                out.extend(self.render_term(site, taxonomy, term, &site_ctx)?);
            }
        }
        let home = site.home.map(|i| page_ctx(site, &site.entries[i], true));
        let recent: Vec<PageCtx> = site
            .regular
            .iter()
            .take(RECENT)
            .map(|&i| page_ctx(site, &site.entries[i], false))
            .collect();
        let html = self
            .render(
                &["home.html".to_string()],
                context! { site => site_ctx.clone(), current => self.current(site, "/"), page => ser(&home), recent => ser(&recent) },
            )
            .context("rendering home page")?;
        out.push(OutputFile {
            path: "index.html".to_string(),
            contents: html,
        });
        for (name, path) in [("404.html", "404.html"), ("robots.txt", "robots.txt")] {
            out.push(OutputFile {
                path: path.to_string(),
                contents: self
                    .render(&[name.to_string()], context! { site => site_ctx.clone() })
                    .with_context(|| format!("rendering {name}"))?,
            });
        }
        Ok(out)
    }

    fn render_taxonomy(&self, site: &Site, t: &Taxonomy, site_ctx: &Value) -> Result<OutputFile> {
        let terms: Vec<TermCtx> = t.terms.iter().map(term_ctx).collect();
        let page = t.index.map(|i| page_ctx(site, &site.entries[i], true));
        let html = self
            .render(
                &[format!("{}/taxonomy.html", t.name), "taxonomy.html".to_string()],
                context! {
                    site => site_ctx.clone(), current => self.current(site, &t.url),
                    page => ser(&page),
                    taxonomy => context! { name => &t.name, title => &t.title, url => &t.url, terms => ser(&terms) },
                },
            )
            .with_context(|| format!("rendering {}", t.url))?;
        Ok(OutputFile {
            path: index_path(&t.url),
            contents: html,
        })
    }

    fn render_term(
        &self,
        site: &Site,
        t: &Taxonomy,
        term: &Term,
        site_ctx: &Value,
    ) -> Result<Vec<OutputFile>> {
        let page = term.index.map(|i| page_ctx(site, &site.entries[i], true));
        let taxonomy = context! { name => &t.name, title => &t.title, url => &t.url };
        term.pagers
            .iter()
            .map(|pager| {
                let html = self
                    .render(
                        &[format!("{}/term.html", t.name), "term.html".to_string()],
                        context! {
                            site => site_ctx.clone(), current => self.current(site, &pager.url),
                            page => ser(&page),
                            taxonomy => taxonomy.clone(),
                            term => ser(&term_ctx(term)),
                            pager => ser(&pager_ctx(site, pager)),
                        },
                    )
                    .with_context(|| format!("rendering {}", pager.url))?;
                Ok(OutputFile {
                    path: index_path(&pager.url),
                    contents: html,
                })
            })
            .collect()
    }

    /// The page being rendered, whether or not it has a content file:
    /// `url`, `permalink`, `markdown_url` (its agent-readable copy; the first
    /// page's for paginated lists) and `card`.
    fn current(&self, site: &Site, url: &str) -> Value {
        let first = match url.find("page/") {
            Some(i) => &url[..i],
            None => url,
        };
        context! {
            url => url,
            permalink => site.config.permalink(url),
            markdown_url => crate::agents::md_url(site, first),
            card => site
                .config
                .cards
                .enabled
                .then(|| site.config.permalink(&crate::cards::card_path(first))),
        }
    }

    fn render(&self, names: &[String], ctx: Value) -> Result<String> {
        let name = names
            .iter()
            .find(|n| self.env.get_template(n).is_ok())
            .with_context(|| format!("no template found among {names:?}"))?;
        let tmpl = self.env.get_template(name)?;
        tmpl.render(ctx)
            .map_err(|e| anyhow::anyhow!("{name}: {e:#}"))
    }
}

fn term_ctx(term: &Term) -> TermCtx<'_> {
    TermCtx {
        name: &term.name,
        title: &term.title,
        url: &term.url,
        count: term.pages.len(),
    }
}

fn collect_templates(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_templates(root, &path, out)?;
        } else if path
            .extension()
            .is_some_and(|e| e == "html" || e == "xml" || e == "txt" || e == "md")
        {
            let name = path
                .strip_prefix(root)
                .expect("under root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((name, path));
        }
    }
    Ok(())
}

fn page_ctx<'a>(site: &'a Site, e: &'a Entry, with_content: bool) -> PageCtx<'a> {
    PageCtx {
        kind: match e.page.kind {
            Kind::Home => "home",
            Kind::Section => "section",
            Kind::Page => "page",
            Kind::Taxonomy => "taxonomy",
            Kind::Term => "term",
        },
        title: e.title(),
        url: &e.page.url,
        permalink: site.config.permalink(&e.page.url),
        markdown_url: crate::agents::md_url(site, &e.page.url),
        card: site
            .config
            .cards
            .enabled
            .then(|| site.config.permalink(&crate::cards::card_path(&e.page.url))),
        section: &e.page.section,
        date: e.page.fm.date.map(|d| d.to_rfc3339()),
        lastmod: e.page.fm.lastmod.or(e.page.fm.date).map(|d| d.to_rfc3339()),
        description: e.page.fm.description.as_deref(),
        summary: Value::from_safe_string(e.summary.clone()),
        content: with_content.then(|| Value::from_safe_string(e.content.clone())),
        headings: &e.headings,
        word_count: e.word_count,
        reading_time: e.reading_time,
        params: &e.page.fm.params,
    }
}

fn pager_ctx<'a>(site: &'a Site, pager: &'a Pager) -> PagerCtx<'a> {
    PagerCtx {
        url: &pager.url,
        number: pager.number,
        total: pager.total,
        prev: pager.prev.as_deref(),
        next: pager.next.as_deref(),
        items: pager
            .items
            .iter()
            .map(|&i| page_ctx(site, &site.entries[i], false))
            .collect(),
    }
}

fn ser<T: Serialize>(v: &T) -> Value {
    Value::from(minijinja::value::Serde(v))
}

fn section_ctx(section: &Section) -> Value {
    context! {
        name => &section.name,
        title => &section.title,
        url => &section.url,
        count => section.list.len(),
    }
}

/// HTML auto-escaping of `& < > " '` only. minijinja's default also encodes
/// `/` as `&#x2f;`, which is valid but makes every URL in the output
/// unreadable and hard to compare against Hugo's.
fn html_formatter(
    out: &mut minijinja::Output,
    state: &mut minijinja::State,
    value: &Value,
) -> Result<(), minijinja::Error> {
    if matches!(state.auto_escape(), minijinja::AutoEscape::Html)
        && !value.is_safe()
        && !value.is_none()
        && !value.is_undefined()
    {
        let text = value.to_string();
        let mut escaped = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '"' => escaped.push_str("&quot;"),
                '\'' => escaped.push_str("&#39;"),
                c => escaped.push(c),
            }
        }
        return out.write_str(&escaped).map_err(minijinja::Error::from);
    }
    minijinja::escape_formatter(out, state, value)
}

/// `{{ page.date | date('%-d %B %Y') }}`: format an RFC 3339 string.
fn date_filter(value: String, format: String) -> Result<String, minijinja::Error> {
    let d = chrono::DateTime::parse_from_rfc3339(&value).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("date filter: {value:?} is not RFC 3339: {e}"),
        )
    })?;
    Ok(d.format(&format).to_string())
}

/// `/post/foo/` → `post/foo/index.html`; `/` → `index.html`.
pub fn index_path(url: &str) -> String {
    let trimmed = url.trim_matches('/');
    if trimmed.is_empty() {
        "index.html".to_string()
    } else {
        format!("{trimmed}/index.html")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_but_not_slashes() {
        let mut env = Environment::new();
        env.set_formatter(html_formatter);
        env.add_template_owned("t.html", "{{ v }}|{{ s }}").unwrap();
        let out = env
            .get_template("t.html")
            .unwrap()
            .render(context! { v => "<a href='/x'>&", s => Value::from_safe_string("<b>".into()) })
            .unwrap();
        assert_eq!(out, "&lt;a href=&#39;/x&#39;&gt;&amp;|<b>");
    }

    #[test]
    fn index_paths() {
        assert_eq!(index_path("/"), "index.html");
        assert_eq!(index_path("/post/a/"), "post/a/index.html");
        assert_eq!(index_path("/post/page/2/"), "post/page/2/index.html");
    }
}
