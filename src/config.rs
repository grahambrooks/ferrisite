//! Site configuration, read from `ferrisite.toml` in the site root.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Absolute base URL with a trailing slash, e.g. `https://www.grahambrooks.com/`.
    pub base_url: String,
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Feed `<language>`, e.g. `en-us`.
    #[serde(default = "default_language")]
    pub language: String,
    /// Feed `<copyright>`.
    #[serde(default)]
    pub copyright: Option<String>,
    #[serde(default)]
    pub pagination: Pagination,
    /// Taxonomies, by their plural name, which is both the frontmatter key and
    /// the content directory for term files (`content/tags/rust/_index.md`).
    #[serde(default)]
    pub taxonomies: Vec<String>,
    /// Words in an automatic summary (Hugo's `summaryLength`, default 70).
    #[serde(default = "default_summary_length")]
    pub summary_length: usize,
    /// Directories copied verbatim to the output root, in order; later ones
    /// win on conflict (a theme's static directory, then the site's).
    #[serde(default = "default_static_dirs")]
    pub static_dirs: Vec<String>,
    /// Per-section list settings, keyed by section name.
    #[serde(default)]
    pub sections: BTreeMap<String, SectionConfig>,
    /// Extra RSS feeds beyond the home, section and taxonomy feeds.
    #[serde(default)]
    pub feeds: Vec<FeedConfig>,
    /// Post-processing of rendered Markdown (the equivalent of Hugo render hooks).
    #[serde(default)]
    pub markup: MarkupConfig,
    /// Social cards (DESIGN.md section 10).
    #[serde(default)]
    pub cards: CardsConfig,
    /// `ferrisite check` settings.
    #[serde(default)]
    pub check: CheckConfig,
    /// Agent-readable output (DESIGN.md section 9).
    #[serde(default)]
    pub agents: AgentsConfig,
    /// Per-taxonomy options, keyed by taxonomy name.
    #[serde(default)]
    pub taxonomy_options: BTreeMap<String, TaxonomyOptions>,
    /// Free-form values for templates, exposed as `site.params`.
    #[serde(default)]
    pub params: BTreeMap<String, toml::Value>,
    /// Extra paths that get a byte-identical copy of the home feed
    /// (`/index.xml`), e.g. a pre-migration feed URL like `rss.xml`.
    #[serde(default)]
    pub home_feed_copies: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pagination {
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

/// How a section's HTML list is built. Section feeds are not filtered (Hugo's
/// embedded feed template lists every page in the section).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionConfig {
    /// List pages from another section instead of this one (e.g. a
    /// `releases` section listing posts marked `release: true`).
    #[serde(default)]
    pub source: Option<String>,
    /// Only pages whose frontmatter matches every key/value.
    #[serde(default)]
    pub include: BTreeMap<String, toml::Value>,
    /// Drop pages whose frontmatter matches every key/value.
    #[serde(default)]
    pub exclude: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedConfig {
    /// Output path relative to the site root, e.g. `essays.xml`.
    pub path: String,
    pub title: String,
    pub description: String,
    /// Section whose pages the feed lists.
    pub section: String,
    #[serde(default)]
    pub include: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub exclude: BTreeMap<String, toml::Value>,
    /// `default` (Hugo's default page order) or `date-reverse` (Hugo's
    /// `.ByDate.Reverse`, whose tie-break on equal dates is reversed too).
    #[serde(default)]
    pub order: FeedOrder,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkupConfig {
    /// Inline local `.svg` images from this directory (relative to the site
    /// root) instead of emitting `<img>`, so `currentColor` in the SVG follows
    /// the page's text colour. A missing file fails the build.
    #[serde(default)]
    pub inline_svg_from: Option<String>,
    /// Wrap every table in `<div class="...">`, e.g. for horizontal scrolling.
    #[serde(default)]
    pub table_wrapper: Option<String>,
    /// Directory (relative to the site root) that AsciiDoc `include::` and
    /// PlantUML `!include` paths resolve from. Default: the site root.
    #[serde(default = "dot")]
    pub include_base: String,
    /// Generated stylesheet for syntax highlighting and diagram light/dark
    /// switching, relative to the output root.
    #[serde(default = "default_stylesheet")]
    pub stylesheet: String,
    /// syntect theme names for code in light and dark mode.
    #[serde(default = "default_light_theme")]
    pub highlight_light: String,
    #[serde(default = "default_dark_theme")]
    pub highlight_dark: String,
}

impl Default for MarkupConfig {
    fn default() -> Self {
        MarkupConfig {
            inline_svg_from: None,
            table_wrapper: None,
            include_base: dot(),
            stylesheet: default_stylesheet(),
            highlight_light: default_light_theme(),
            highlight_dark: default_dark_theme(),
        }
    }
}

fn dot() -> String {
    ".".to_string()
}

fn default_stylesheet() -> String {
    "css/ferrisite.css".to_string()
}

fn default_light_theme() -> String {
    "InspiredGitHub".to_string()
}

fn default_dark_theme() -> String {
    "base16-ocean.dark".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckConfig {
    /// Broken links accepted until the content is fixed, each with a reason.
    /// `link` is the site-relative target as `check` prints it.
    #[serde(default)]
    pub known_broken: Vec<KnownBroken>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownBroken {
    pub link: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardsConfig {
    /// Generate `images/cards/<slug>.png` for every page.
    #[serde(default)]
    pub enabled: bool,
    /// PNG or JPEG (relative to the site root) drawn as a round headshot.
    #[serde(default)]
    pub image: Option<String>,
    /// Second line under the byline, e.g. the site's domain.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "card_bg")]
    pub background: String,
    #[serde(default = "card_accent")]
    pub accent: String,
    #[serde(default = "card_text")]
    pub text: String,
    #[serde(default = "card_divider")]
    pub divider: String,
}

impl Default for CardsConfig {
    fn default() -> Self {
        CardsConfig {
            enabled: false,
            image: None,
            label: None,
            background: card_bg(),
            accent: card_accent(),
            text: card_text(),
            divider: card_divider(),
        }
    }
}

fn card_bg() -> String {
    "#15171c".into()
}
fn card_accent() -> String {
    "#7fb3ff".into()
}
fn card_text() -> String {
    "#e6e1d6".into()
}
fn card_divider() -> String {
    "#2a2f3a".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    /// Markdown copies, `llms.txt` and `llms-full.txt`.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The `llms.txt` blockquote; defaults to the site description.
    #[serde(default)]
    pub summary: Option<String>,
    /// Taxonomies listed as sections of `llms.txt` (e.g. series, projects).
    #[serde(default)]
    pub taxonomies: Vec<String>,
    /// Section whose list is "Recent …" in `llms.txt` and the content of
    /// `llms-full.txt`.
    #[serde(default)]
    pub recent_section: Option<String>,
    #[serde(default = "default_recent_count")]
    pub recent_count: usize,
    #[serde(default = "default_full_max_bytes")]
    pub full_max_bytes: usize,
    /// A `Content-Signal:` value for robots.txt (Cloudflare's convention),
    /// e.g. `search=yes, ai-input=yes, ai-train=no`. Omitted when unset.
    #[serde(default)]
    pub content_signal: Option<String>,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        AgentsConfig {
            enabled: true,
            summary: None,
            taxonomies: Vec::new(),
            recent_section: None,
            recent_count: default_recent_count(),
            full_max_bytes: default_full_max_bytes(),
            content_signal: None,
        }
    }
}

fn default_recent_count() -> usize {
    20
}

fn default_full_max_bytes() -> usize {
    2_000_000
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedOrder {
    #[default]
    Default,
    DateReverse,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyOptions {
    /// Term pages paginate (with a `page/1/` alias). `false` lists every page
    /// on one term page.
    #[serde(default = "yes")]
    pub paginate: bool,
}

fn yes() -> bool {
    true
}

impl Default for Pagination {
    fn default() -> Self {
        Pagination {
            page_size: default_page_size(),
        }
    }
}

fn default_page_size() -> usize {
    10
}

fn default_summary_length() -> usize {
    70
}

fn default_language() -> String {
    "en-us".to_string()
}

fn default_static_dirs() -> Vec<String> {
    vec!["static".to_string()]
}

impl Config {
    pub fn load(root: &Path) -> Result<Config> {
        let path = root.join("ferrisite.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Config::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config> {
        let mut config: Config = toml::from_str(text)?;
        if !config.base_url.ends_with('/') {
            config.base_url.push('/');
        }
        anyhow::ensure!(
            config.pagination.page_size > 0,
            "pagination.page_size must be at least 1"
        );
        Ok(config)
    }

    /// The absolute URL for a site-relative path such as `/post/foo/`.
    pub fn permalink(&self, rel: &str) -> String {
        format!("{}{}", self.base_url, rel.trim_start_matches('/'))
    }

    pub fn section(&self, name: &str) -> SectionConfig {
        self.sections.get(name).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_trailing_slash_and_defaults() {
        let c = Config::parse("base_url = \"https://example.com\"\ntitle = \"T\"\n").unwrap();
        assert_eq!(c.base_url, "https://example.com/");
        assert_eq!(c.pagination.page_size, 10);
        assert_eq!(c.summary_length, 70);
        assert_eq!(c.static_dirs, vec!["static"]);
        assert_eq!(c.permalink("/post/a/"), "https://example.com/post/a/");
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(Config::parse("base_url = \"x\"\ntitle = \"T\"\nbogus = 1\n").is_err());
    }

    #[test]
    fn section_filters_and_feeds() {
        let c = Config::parse(
            r#"base_url = "x"
title = "T"
[sections.releases]
source = "post"
include = { release = true }
[[feeds]]
path = "essays.xml"
title = "Essays"
description = "d"
section = "post"
exclude = { release = true }
"#,
        )
        .unwrap();
        assert_eq!(c.section("releases").source.as_deref(), Some("post"));
        assert!(c.section("missing").include.is_empty());
        assert_eq!(c.feeds[0].exclude["release"], toml::Value::Boolean(true));
    }
}
