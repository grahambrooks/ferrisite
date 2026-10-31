//! Resolve: turns discovered pages into a `Site`. Every cross-page fact
//! (visibility, ordering, sections, pagination, summaries) is computed here,
//! so templates only loop and print and can't make a build non-deterministic.

use crate::config::Config;
use crate::content::{Discovered, Kind, Page};
use crate::markup::{self, Heading, text};
use chrono::{DateTime, FixedOffset};
use rayon::prelude::*;
use std::cmp::Ordering;

/// A page with its markup rendered and derived values computed.
#[derive(Debug, Clone)]
pub struct Entry {
    pub page: Page,
    pub content: String,
    pub summary: String,
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
    /// Index into `Site::entries` of the section's `_index` page, if any.
    pub index: Option<usize>,
    /// Regular pages in the section, in Hugo's default order.
    pub pages: Vec<usize>,
    pub pagers: Vec<Pager>,
}

#[derive(Debug)]
pub struct Site {
    pub config: Config,
    pub entries: Vec<Entry>,
    pub home: Option<usize>,
    /// Every regular page, in Hugo's default order.
    pub regular: Vec<usize>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Copy)]
pub struct Visibility {
    pub drafts: bool,
    pub future: bool,
    pub now: DateTime<FixedOffset>,
}

pub fn resolve(config: Config, found: Discovered, vis: Visibility) -> Site {
    let visible: Vec<Page> = found
        .pages
        .into_iter()
        .filter(|p| vis.drafts || !p.fm.draft)
        .filter(|p| vis.future || p.fm.date.is_none_or(|d| d <= vis.now))
        .collect();
    let summary_length = config.summary_length;
    let mut entries: Vec<Entry> = visible
        .into_par_iter()
        .map(|page| render_entry(page, summary_length))
        .collect();
    entries.sort_by(|a, b| a.page.source.cmp(&b.page.source));

    let home = entries.iter().position(|e| e.page.kind == Kind::Home);
    let mut regular: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].page.kind == Kind::Page)
        .collect();
    regular.sort_by(|&a, &b| hugo_order(&entries[a], &entries[b]));

    let mut names: Vec<String> = entries
        .iter()
        .filter(|e| e.page.kind == Kind::Section)
        .map(|e| e.page.url.trim_matches('/').to_string())
        .chain(found.implicit_sections)
        .collect();
    names.sort();
    names.dedup();
    let page_size = config.pagination.page_size;
    let sections = names
        .into_iter()
        .map(|name| {
            let url = format!("/{name}/");
            let index = entries
                .iter()
                .position(|e| e.page.kind == Kind::Section && e.page.url == url);
            let pages: Vec<usize> = regular
                .iter()
                .copied()
                .filter(|&i| in_section(&entries[i].page, &name))
                .collect();
            let pagers = paginate(&url, &pages, page_size);
            Section {
                name,
                url,
                index,
                pages,
                pagers,
            }
        })
        .collect();

    Site {
        config,
        entries,
        home,
        regular,
        sections,
    }
}

/// A page belongs to a section if its URL sits under the section's directory
/// (nested sections are listed by their top-level section in M1).
fn in_section(page: &Page, section: &str) -> bool {
    page.url.starts_with(&format!("/{section}/"))
}

fn render_entry(page: Page, summary_length: usize) -> Entry {
    let rendered = markup::markdown(&page.body);
    let word_count = text::word_count(&rendered.html);
    let summary = if let Some((before, _)) = page.body.split_once("<!--more-->") {
        markup::markdown(before).html.trim_end().to_string()
    } else if let Some(s) = &page.fm.summary {
        markup::markdown(s).html.trim_end().to_string()
    } else {
        text::auto_summary(&rendered.html, summary_length)
    };
    Entry {
        reading_time: text::reading_time(word_count),
        word_count,
        summary,
        headings: rendered.headings,
        content: rendered.html,
        page,
    }
}

/// Hugo's default page order: weight (unweighted last), date descending,
/// title, then source path as the final tie-break.
fn hugo_order(a: &Entry, b: &Entry) -> Ordering {
    let weight = |e: &Entry| match e.page.fm.weight {
        0 => i64::MAX,
        w => w,
    };
    weight(a)
        .cmp(&weight(b))
        .then_with(|| b.date().cmp(&a.date()))
        .then_with(|| a.title().cmp(b.title()))
        .then_with(|| a.page.source.cmp(&b.page.source))
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
}
