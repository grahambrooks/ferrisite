//! Heading IDs in Hugo's default ("github") style, so existing `#fragment`
//! links and tables of contents keep working.

use std::collections::HashSet;

/// Hugo's `github` auto-ID: letters, digits and `_` are kept (lowercased),
/// spaces and hyphens become `-`, everything else is dropped.
pub fn anchorize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.trim().chars() {
        if c == '-' || c == ' ' {
            out.push('-');
        } else if c == '_' || c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Makes IDs unique within one page: the second `foo` becomes `foo-1`.
#[derive(Debug, Default)]
pub struct IdFactory {
    seen: HashSet<String>,
}

impl IdFactory {
    pub fn unique(&mut self, text: &str) -> String {
        let mut id = anchorize(text);
        if id.is_empty() {
            id = "heading".to_string();
        }
        if self.seen.contains(&id) {
            let mut n = 1;
            while self.seen.contains(&format!("{id}-{n}")) {
                n += 1;
            }
            id = format!("{id}-{n}");
        }
        self.seen.insert(id.clone());
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_style() {
        assert_eq!(
            anchorize("Where the 92 releases came from"),
            "where-the-92-releases-came-from"
        );
        assert_eq!(anchorize("What it can’t do"), "what-it-cant-do");
        assert_eq!(
            anchorize("C++ & Rust: a (brief) note"),
            "c--rust-a-brief-note"
        );
        assert_eq!(anchorize("snake_case stays"), "snake_case-stays");
    }

    #[test]
    fn duplicates_get_numbered() {
        let mut ids = IdFactory::default();
        assert_eq!(ids.unique("Notes"), "notes");
        assert_eq!(ids.unique("Notes"), "notes-1");
        assert_eq!(ids.unique("Notes"), "notes-2");
    }
}
