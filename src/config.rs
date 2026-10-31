//! Site configuration, read from `ferrisite.toml` in the site root.

use anyhow::{Context, Result};
use serde::Deserialize;
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
    #[serde(default)]
    pub pagination: Pagination,
    /// Content directories that hold taxonomy term files (`content/tags/...`).
    /// They are not sections. Taxonomies themselves arrive in M2.
    #[serde(default)]
    pub taxonomies: Vec<String>,
    /// Words in an automatic summary (Hugo's `summaryLength`, default 70).
    #[serde(default = "default_summary_length")]
    pub summary_length: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pagination {
    #[serde(default = "default_page_size")]
    pub page_size: usize,
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
        assert_eq!(c.permalink("/post/a/"), "https://example.com/post/a/");
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(Config::parse("base_url = \"x\"\ntitle = \"T\"\nbogus = 1\n").is_err());
    }
}
