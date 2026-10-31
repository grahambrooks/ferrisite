//! Resolve: turns discovered pages into a `Site`. Every cross-page fact
//! (visibility, ordering, sections, taxonomies, pagination, aliases,
//! summaries) is computed here, so templates only loop and print and can't
//! make a build non-deterministic.
//!
//! Two passes: the first works on frontmatter alone (visibility, URLs, order,
//! lists), which is what `ref` and list-showing shortcodes need; the second
//! renders markup with shortcodes expanded against that index.

use crate::config::{Config, SectionConfig};
use crate::content::frontmatter::{string_list, toml_to_yaml};
use crate::content::{Discovered, Format, Kind, Page};
use crate::diagrams::Diagrams;
use crate::markup::Fences;
use crate::markup::shortcodes::{Call, Expand};
use crate::markup::{self, Heading, text};
use crate::render::Renderer;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset};
use rayon::prelude::*;
use serde_yaml::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

/// A page with its markup rendered and derived values computed.
#[derive(Debug, Clone)]
pub struct Entry {
    pub page: Page,
    pub content: String,
    pub summary: String,
    pub plain: String,
    pub headings: Vec<Heading>,
    pub word_count: usize,
    pub reading_time: usize,
}

impl Entry {
    pub fn title(&self) -> &str {
        self.page.fm.title.as_deref().unwrap_or("")
    }
    pub fn date(&self) -> Option<DateTime<FixedOffset>> {
        self.page.fm.date
    }
    pub fn param(&self, key: &str) -> Option<&Value> {
        self.page.fm.params.get(Value::String(key.into()))
    }
}

/// One page of a paginated list.
#[derive(Debug, Clone)]
pub struct Pager {
    /// Site-relative URL of this list page (`/post/` or `/post/page/2/`).
    pub url: String,
    pub number: usize,
    pub total: usize,
    /// Indexes into `Site::entries`.
    pub items: Vec<usize>,
    pub prev: Option<String>,
    pub next: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub url: String,
    pub title: String,
    /// Index into `Site::entries` of the section's `_index` page, if any.
    pub index: Option<usize>,
    /// Every regular page in the section, in Hugo's default order (the feed).
    pub pages: Vec<usize>,
    /// What the HTML list shows, after `source`/`include`/`exclude`.
    pub list: Vec<usize>,
    pub pagers: Vec<Pager>,
    /// Whether the section gets `/<section>/index.xml`.
    pub feed: bool,
}

#[derive(Debug, Clone)]
pub struct Term {
    /// The term as written in frontmatter.
    pub name: String,
    pub url: String,
    pub title: String,
    pub index: Option<usize>,
    pub pages: Vec<usize>,
    pub pagers: Vec<Pager>,
    /// Whether the term list paginates (and so has a `page/1/` alias).
    pub paginated: bool,
}

#[derive(Debug, Clone)]
pub struct Taxonomy {
    pub name: String,
    pub url: String,
    pub title: String,
    pub index: Option<usize>,
    pub terms: Vec<Term>,
}

/// A redirect stub: `path` is the output file, `target` a site-relative URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub path: String,
    pub target: String,
}

#[derive(Debug)]
pub struct Site {
    pub config: Config,
    pub entries: Vec<Entry>,
    pub home: Option<usize>,
    /// Regular pages in global collections (feeds, search, recent), in Hugo's
    /// default order. Excludes pages with `build.list: local` or `never`.
    pub regular: Vec<usize>,
    pub sections: Vec<Section>,
    pub taxonomies: Vec<Taxonomy>,
    pub aliases: Vec<Alias>,
}

#[derive(Debug, Clone, Copy)]
pub struct Visibility {
    pub drafts: bool,
    pub future: bool,
    pub now: DateTime<FixedOffset>,
}

/// What pass one knows about a page, for `ref` and list shortcodes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Brief {
    pub title: String,
    pub url: String,
    pub permalink: String,
    pub date: Option<String>,
    pub section: String,
    pub params: serde_yaml::Mapping,
}

pub struct Index {
    pub briefs: Vec<Brief>,
    /// Section name → its HTML list, as briefs (for `recent-posts`-style shortcodes).
    pub lists: BTreeMap<String, Vec<Brief>>,
    sources: Vec<String>,
}

impl Index {
    /// Resolve a Hugo `ref` argument: a content path (with or without
    /// extension) or a file stem, optionally with `#fragment`.
    pub fn resolve_ref(&self, arg: &str) -> Result<String> {
        let (path, fragment) = match arg.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (arg, None),
        };
        let path = path.trim_start_matches('/');
        let matches: Vec<usize> = self
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let no_ext = s.rsplit_once('.').map(|(a, _)| a).unwrap_or(s);
                let stem = no_ext.rsplit('/').next().unwrap_or(no_ext);
                *s == path || no_ext == path || stem == path
            })
            .map(|(i, _)| i)
            .collect();
        let i = match matches.as_slice() {
            [i] => *i,
            [] => bail!("ref {arg:?}: no page found"),
            many => bail!(
                "ref {arg:?} is ambiguous: {}",
                many.iter()
                    .map(|&i| self.sources[i].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        let permalink = &self.briefs[i].permalink;
        Ok(match fragment {
            Some(f) if !f.is_empty() => format!("{permalink}#{f}"),
            _ => permalink.clone(),
        })
    }
}

pub fn resolve(
    config: Config,
    root: &std::path::Path,
    found: Discovered,
    vis: Visibility,
    renderer: &Renderer,
    diagrams: &Diagrams,
) -> Result<Site> {
    // Pass one: visibility, order, sections and lists from frontmatter alone.
    let mut pages: Vec<Page> = found
        .pages
        .into_iter()
        .filter(|p| vis.drafts || !p.fm.draft)
        .filter(|p| vis.future || p.fm.date.is_none_or(|d| d <= vis.now))
        .collect();
    pages.sort_by(|a, b| a.source.cmp(&b.source));
    let briefs: Vec<Brief> = pages.iter().map(|p| brief(&config, p)).collect();

    let order = |a: usize, b: usize| hugo_order(&pages[a], &pages[b]);
    let mut regular_all: Vec<usize> = (0..pages.len())
        .filter(|&i| pages[i].kind == Kind::Page)
        .collect();
    regular_all.sort_by(|&a, &b| order(a, b));
    let regular: Vec<usize> = regular_all
        .iter()
        .copied()
        .filter(|&i| listed_globally(&pages[i]))
        .collect();

    let page_size = config.pagination.page_size;
    let mut names: Vec<String> = pages
        .iter()
        .filter(|p| p.kind == Kind::Section)
        .map(|p| p.url.trim_matches('/').to_string())
        .chain(found.implicit_sections)
        .collect();
    names.sort();
    names.dedup();
    let mut sections = Vec::new();
    for name in names {
        let url = format!("/{name}/");
        let index = pages
            .iter()
            .position(|p| p.kind == Kind::Section && p.url == url);
        let in_section = |section: &str| -> Vec<usize> {
            regular_all
                .iter()
                .copied()
                .filter(|&i| pages[i].url.starts_with(&format!("/{section}/")))
                .collect()
        };
        let own = in_section(&name);
        let cfg: SectionConfig = config.section(&name);
        let source = cfg.source.clone().unwrap_or_else(|| name.clone());
        let list: Vec<usize> = in_section(&source)
            .into_iter()
            .filter(|&i| filter_matches(&pages[i], &cfg.include, &cfg.exclude))
            .collect();
        let pagers = paginate(&url, &list, page_size);
        let feed = index.is_none_or(|i| outputs_allow(&pages[i], "rss"));
        let title = index
            .and_then(|i| pages[i].fm.title.clone())
            .unwrap_or_else(|| go_title(&name));
        sections.push(Section {
            name,
            url,
            title,
            index,
            pages: own,
            list,
            pagers,
            feed,
        });
    }

    let taxonomies = build_taxonomies(&config, &pages, &regular_all, page_size)?;

    let lists = sections
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                s.list.iter().map(|&i| briefs[i].clone()).collect(),
            )
        })
        .collect();
    let index = Index {
        sources: pages.iter().map(|p| p.source.clone()).collect(),
        briefs,
        lists,
    };

    // Pass two: markup, with shortcodes expanded against the index.
    let summary_length = config.summary_length;
    let hooks = markup::Hooks {
        inline_svg_from: config.markup.inline_svg_from.as_ref().map(|d| root.join(d)),
        table_wrapper: config.markup.table_wrapper.clone(),
    };
    let include_base = root.join(&config.markup.include_base);
    let env = RenderEnv {
        index: &index,
        renderer,
        diagrams,
        hooks: &hooks,
        include_base: &include_base,
        summary_length,
    };
    let entries: Vec<Entry> = pages
        .into_par_iter()
        .enumerate()
        .map(|(i, page)| {
            let source = page.source.clone();
            render_entry(page, i, &env).with_context(|| source.clone())
        })
        .collect::<Result<_>>()?;

    let home = entries.iter().position(|e| e.page.kind == Kind::Home);
    let aliases = collect_aliases(&entries, &sections, &taxonomies)?;
    Ok(Site {
        config,
        entries,
        home,
        regular,
        sections,
        taxonomies,
        aliases,
    })
}

fn brief(config: &Config, p: &Page) -> Brief {
    Brief {
        title: p.fm.title.clone().unwrap_or_default(),
        url: p.url.clone(),
        permalink: config.permalink(&p.url),
        date: p.fm.date.map(|d| d.to_rfc3339()),
        section: p.section.clone(),
        params: p.fm.params.clone(),
    }
}

fn build_taxonomies(
    config: &Config,
    pages: &[Page],
    regular_all: &[usize],
    page_size: usize,
) -> Result<Vec<Taxonomy>> {
    let mut out = Vec::new();
    for name in &config.taxonomies {
        let url = format!("/{name}/");
        let index = pages
            .iter()
            .position(|p| p.kind == Kind::Taxonomy && p.url == url);
        // Term name → pages, from frontmatter. BTreeMap keeps term order stable.
        let mut terms: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for &i in regular_all {
            if let Some(v) = pages[i].fm.params.get(Value::String(name.clone())) {
                for term in string_list(v) {
                    terms.entry(term).or_default().push(i);
                }
            }
        }
        // Term files with no pages still get a term page (Hugo does the same).
        let mut by_url: HashMap<String, String> = HashMap::new();
        for term in terms.keys() {
            let term_url = term_url(name, term);
            if let Some(other) = by_url.insert(term_url.clone(), term.clone()) {
                bail!(
                    "{name}: terms {other:?} and {term:?} both map to {term_url}; give each term one spelling"
                );
            }
        }
        for p in pages.iter().filter(|p| p.kind == Kind::Term) {
            if p.url.starts_with(&url) && !by_url.contains_key(&p.url) {
                let seg = p.url[url.len()..].trim_end_matches('/').to_string();
                by_url.insert(p.url.clone(), seg.clone());
                terms.entry(seg).or_default();
            }
        }
        let terms = terms
            .into_iter()
            .map(|(term, mut items)| {
                items.sort_by(|&a, &b| hugo_order(&pages[a], &pages[b]));
                let turl = term_url(name, &term);
                let tindex = pages
                    .iter()
                    .position(|p| p.kind == Kind::Term && p.url == turl);
                let title = tindex
                    .and_then(|i| pages[i].fm.title.clone())
                    .unwrap_or_else(|| go_title(&term));
                let paginated = config.taxonomy_options.get(name).is_none_or(|o| o.paginate);
                let size = if paginated {
                    page_size
                } else {
                    items.len().max(1)
                };
                let pagers = paginate(&turl, &items, size);
                Term {
                    name: term,
                    url: turl,
                    title,
                    index: tindex,
                    pages: items,
                    pagers,
                    paginated,
                }
            })
            .collect();
        let title = index
            .and_then(|i| pages[i].fm.title.clone())
            .unwrap_or_else(|| go_title(name));
        out.push(Taxonomy {
            name: name.clone(),
            url,
            title,
            index,
            terms,
        });
    }
    Ok(out)
}

pub fn term_url(taxonomy: &str, term: &str) -> String {
    format!("/{taxonomy}/{}/", term.replace(' ', "-"))
}

/// Go's `strings.Title`, which Hugo uses for default term and section titles:
/// uppercase the first letter of every word, where anything other than a
/// letter, digit or `_` separates words. `open-source` → `Open-Source`.
pub fn go_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_start = true;
    for c in s.chars() {
        if at_start && c.is_alphabetic() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        at_start = !(c.is_alphanumeric() || c == '_');
    }
    out
}

fn listed_globally(p: &Page) -> bool {
    let list =
        p.fm.params
            .get(Value::String("build".into()))
            .and_then(|b| b.get("list"))
            .and_then(Value::as_str);
    !matches!(list, Some("local") | Some("never"))
}

fn outputs_allow(p: &Page, format: &str) -> bool {
    match p.fm.params.get(Value::String("outputs".into())) {
        Some(v) => string_list(v)
            .iter()
            .any(|o| o.eq_ignore_ascii_case(format)),
        None => true,
    }
}

pub fn filter_matches(
    p: &Page,
    include: &BTreeMap<String, toml::Value>,
    exclude: &BTreeMap<String, toml::Value>,
) -> bool {
    let has = |k: &String, v: &toml::Value| {
        p.fm.params.get(Value::String(k.clone())) == Some(&toml_to_yaml(v.clone()))
    };
    include.iter().all(|(k, v)| has(k, v))
        && !(!exclude.is_empty() && exclude.iter().all(|(k, v)| has(k, v)))
}

/// What rendering one entry needs from the rest of the build.
struct RenderEnv<'a> {
    index: &'a Index,
    renderer: &'a Renderer,
    diagrams: &'a Diagrams,
    hooks: &'a markup::Hooks,
    include_base: &'a std::path::Path,
    summary_length: usize,
}

/// A page-unique prefix for ids inside inline diagrams.
fn id_prefix(source: &str) -> String {
    let mut p: String = source
        .rsplit_once('.')
        .map(|(a, _)| a)
        .unwrap_or(source)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    p.push_str("-d");
    p
}

fn render_entry(page: Page, me: usize, env: &RenderEnv) -> Result<Entry> {
    let RenderEnv {
        index,
        renderer,
        hooks,
        summary_length,
        ..
    } = *env;
    let prefix = id_prefix(&page.source);
    let fences = Fences {
        diagrams: Some(env.diagrams),
        id_prefix: &prefix,
        line_offset: page.body_line - 1,
    };
    let expander = PageExpander {
        index,
        renderer,
        me,
    };
    let mut rendered = match page.format {
        Format::Markdown => markup::markdown_with_shortcodes(&page.body, &expander, fences)?,
        Format::AsciiDoc => {
            markup::asciidoc::render(&page.body, &page.source, env.include_base, fences)?
        }
    };
    rendered.html = hooks.apply(&rendered.html)?;
    let plain = text::plain(&rendered.html)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let word_count = text::word_count(&rendered.html);
    let summary = if page.format == Format::AsciiDoc {
        text::auto_summary(&rendered.html, summary_length)
    } else if let Some((before, _)) = page.body.split_once("<!--more-->") {
        markup::markdown_with_shortcodes(before, &expander, fences)?
            .html
            .trim_end()
            .to_string()
    } else if let Some(s) = &page.fm.summary {
        markup::markdown_with_shortcodes(s, &expander, fences)?
            .html
            .trim_end()
            .to_string()
    } else {
        text::auto_summary(&rendered.html, summary_length)
    };
    Ok(Entry {
        reading_time: text::reading_time(word_count),
        word_count,
        summary,
        plain,
        headings: rendered.headings,
        content: rendered.html,
        page,
    })
}

/// Built-in shortcodes (`ref`, `relref`, `highlight`), then site templates in
/// `templates/shortcodes/<name>.html`. Anything else fails the build.
struct PageExpander<'a> {
    index: &'a Index,
    renderer: &'a Renderer,
    me: usize,
}

impl Expand for PageExpander<'_> {
    fn expand(&self, call: &Call, inner: Option<&str>) -> Result<String> {
        match call.name.as_str() {
            "ref" | "relref" => {
                let arg = call.args.first().context("needs a page argument")?;
                let permalink = self.index.resolve_ref(arg)?;
                if call.name == "relref" {
                    let base = &self.index.briefs[self.me].permalink;
                    let root = base
                        .find("://")
                        .and_then(|i| base[i + 3..].find('/').map(|j| i + 3 + j))
                        .unwrap_or(0);
                    return Ok(permalink[root..].to_string());
                }
                Ok(permalink)
            }
            "highlight" => {
                let lang = call.args.first().map(String::as_str).unwrap_or("");
                let code = inner.unwrap_or("");
                let code = code.strip_prefix('\n').unwrap_or(code);
                let code = code.strip_suffix('\n').unwrap_or(code);
                Ok(markup::highlight::block(lang, &format!("{code}\n")))
            }
            name => self
                .renderer
                .render_shortcode(name, &call.args, inner, &self.index.briefs[self.me], &self.index.lists)?
                .with_context(|| {
                    format!("unknown shortcode `{name}` (no built-in and no templates/shortcodes/{name}.html)")
                }),
        }
    }
}

fn collect_aliases(
    entries: &[Entry],
    sections: &[Section],
    taxonomies: &[Taxonomy],
) -> Result<Vec<Alias>> {
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    let mut add = |from: &str, target: &str, origin: &str| -> Result<()> {
        let path = alias_path(from);
        if let Some(existing) = aliases.get(&path)
            && existing != target
        {
            bail!("alias {from} ({origin}) already redirects to {existing}, not {target}");
        }
        aliases.insert(path, target.to_string());
        Ok(())
    };
    for e in entries {
        for a in &e.page.fm.aliases {
            add(a, &e.page.url, &e.page.source)?;
        }
    }
    // Hugo makes `<list>/page/1/` an alias of the list's first page.
    for s in sections.iter().filter(|s| !s.list.is_empty()) {
        add(&format!("{}page/1/", s.url), &s.url, "pagination")?;
    }
    for t in taxonomies {
        for term in t
            .terms
            .iter()
            .filter(|t| t.paginated && !t.pages.is_empty())
        {
            add(&format!("{}page/1/", term.url), &term.url, "pagination")?;
        }
    }
    let real: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| crate::render::index_path(&e.page.url))
        .collect();
    if let Some(clash) = aliases.keys().find(|p| real.contains(*p)) {
        bail!("alias output {clash} would overwrite a real page");
    }
    Ok(aliases
        .into_iter()
        .map(|(path, target)| Alias { path, target })
        .collect())
}

/// Output file for an alias: `/blog/x/` → `blog/x/index.html`,
/// `/2010/10/20/x.html` → `2010/10/20/x.html`.
pub fn alias_path(alias: &str) -> String {
    let a = alias.trim_start_matches('/');
    if a.ends_with(".html") || a.ends_with(".htm") {
        a.to_string()
    } else if a.is_empty() {
        "index.html".to_string()
    } else {
        format!("{}/index.html", a.trim_end_matches('/'))
    }
}

/// Hugo's `.ByDate.Reverse`: date descending, and on equal dates the default
/// order's tie-break (weight, title, path) reversed as well.
pub fn by_date_reverse(a: &Page, b: &Page) -> Ordering {
    b.fm.date.cmp(&a.fm.date).then_with(|| hugo_order(b, a))
}

/// Hugo's default page order: weight (unweighted last), date descending,
/// title, then source path as the final tie-break.
pub fn hugo_order(a: &Page, b: &Page) -> Ordering {
    let weight = |p: &Page| match p.fm.weight {
        0 => i64::MAX,
        w => w,
    };
    let title = |p: &Page| p.fm.title.clone().unwrap_or_default();
    weight(a)
        .cmp(&weight(b))
        .then_with(|| b.fm.date.cmp(&a.fm.date))
        .then_with(|| title(a).cmp(&title(b)))
        .then_with(|| a.source.cmp(&b.source))
}

pub fn paginate(base: &str, items: &[usize], size: usize) -> Vec<Pager> {
    let total = items.len().div_ceil(size).max(1);
    let url = |n: usize| {
        if n == 1 {
            base.to_string()
        } else {
            format!("{base}page/{n}/")
        }
    };
    (1..=total)
        .map(|n| Pager {
            url: url(n),
            number: n,
            total,
            items: items
                .iter()
                .skip((n - 1) * size)
                .take(size)
                .copied()
                .collect(),
            prev: (n > 1).then(|| url(n - 1)),
            next: (n < total).then(|| url(n + 1)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_urls() {
        let items: Vec<usize> = (0..23).collect();
        let pagers = paginate("/post/", &items, 10);
        assert_eq!(pagers.len(), 3);
        assert_eq!(pagers[0].url, "/post/");
        assert_eq!(pagers[1].url, "/post/page/2/");
        assert_eq!(pagers[2].items, vec![20, 21, 22]);
        assert_eq!(pagers[0].prev, None);
        assert_eq!(pagers[0].next.as_deref(), Some("/post/page/2/"));
        assert_eq!(pagers[2].next, None);
    }

    #[test]
    fn empty_section_still_has_one_page() {
        let pagers = paginate("/releases/", &[], 10);
        assert_eq!(pagers.len(), 1);
        assert!(pagers[0].items.is_empty());
    }

    #[test]
    fn hugo_default_titles() {
        assert_eq!(go_title("mcp"), "Mcp");
        assert_eq!(go_title("open-source"), "Open-Source");
        assert_eq!(go_title("C++"), "C++");
        assert_eq!(
            go_title("Software development with AI"),
            "Software Development With AI"
        );
    }

    #[test]
    fn alias_paths() {
        assert_eq!(alias_path("/blog/x/"), "blog/x/index.html");
        assert_eq!(alias_path("/blog/x"), "blog/x/index.html");
        assert_eq!(alias_path("/2010/10/20/x.html"), "2010/10/20/x.html");
        assert_eq!(alias_path("/tags/C/C++/"), "tags/C/C++/index.html");
    }
}
