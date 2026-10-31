//! Syntax highlighting with syntect (pure-Rust regex engine), as CSS classes
//! so one stylesheet can carry a light and a dark theme.
//!
//! An unknown language renders as plain, escaped code: highlighting is
//! decoration, so it never fails a build.

use anyhow::{Result, anyhow};
use std::sync::OnceLock;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Classes are prefixed (`sx-keyword`) so they can't collide with site CSS.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "sx-" };

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Chroma/Hugo language names that syntect knows by another token.
fn token(lang: &str) -> &str {
    match lang {
        "shell" | "sh" | "zsh" | "console" => "bash",
        "golang" => "go",
        "c++" => "cpp",
        "yml" => "yaml",
        "js" | "javascript" => "js",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        other => other,
    }
}

/// The inner HTML (spans) for `code` in `lang`, or `None` when the language
/// is unknown.
pub fn spans(lang: &str, code: &str) -> Option<String> {
    let ss = syntaxes();
    let syntax = ss.find_syntax_by_token(token(&lang.to_ascii_lowercase()))?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, ss, CLASS_STYLE);
    for line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

/// A complete highlighted block, in Hugo/Chroma's markup:
/// `<div class="highlight"><pre class="chroma"><code class="language-x" data-lang="x">…`.
pub fn block(lang: &str, code: &str) -> String {
    let inner = if lang.is_empty() {
        None
    } else {
        spans(lang, code)
    };
    let inner = inner.unwrap_or_else(|| escape(code));
    format!(
        "{OPEN}{}{inner}</code></pre></div>\n",
        code_tag(if lang.is_empty() { None } else { Some(lang) })
    )
}

/// Opening markup of a highlighted block, matching Hugo/Chroma's
/// (`<div class="highlight"><pre class="chroma">`) so themes that decorate
/// code blocks (language label, copy button) recognise ferrisite's output.
pub const OPEN: &str = "<div class=\"highlight\"><pre tabindex=\"0\" class=\"chroma\">";

/// `<code class="language-x" data-lang="x">`, as Hugo writes it.
pub fn code_tag(lang: Option<&str>) -> String {
    match lang {
        Some(l) => {
            let l = escape(l);
            format!("<code class=\"language-{l}\" data-lang=\"{l}\">")
        }
        None => "<code>".to_string(),
    }
}

/// comrak writes a block's closing `</code></pre>` itself; add the wrapper's
/// `</div>` after the first one following each block ferrisite opened (raw
/// HTML `<pre>` in the source is left alone).
pub fn close_wrappers(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(i) = rest.find(OPEN) {
        let after = i + OPEN.len();
        let Some(close) = rest[after..].find("</code></pre>") else {
            break;
        };
        let end = after + close + "</code></pre>".len();
        out.push_str(&rest[..end]);
        if !rest[end..].starts_with("</div>") {
            out.push_str("</div>");
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Stylesheet for highlighted code: `light` theme by default, `dark` when the
/// page's `data-mode` is `dark`, or when it's `auto` (or unset) and the
/// operating system prefers dark.
pub fn css(light: &str, dark: &str) -> Result<String> {
    let themes = ThemeSet::load_defaults();
    let get = |name: &str| {
        themes.themes.get(name).ok_or_else(|| {
            anyhow!(
                "unknown highlight theme {name:?}; available: {}",
                themes.themes.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })
    };
    let light_css = css_for_theme_with_class_style(get(light)?, CLASS_STYLE)?;
    let dark_css = css_for_theme_with_class_style(get(dark)?, CLASS_STYLE)?;
    let mut out = format!("/* ferrisite syntax highlighting: {light} (light), {dark} (dark). */\n");
    out.push_str(&light_css);
    out.push_str(&scope(&dark_css, "html[data-mode=\"dark\"]"));
    out.push_str("@media (prefers-color-scheme: dark) {\n");
    out.push_str(&scope(&dark_css, "html:not([data-mode=\"light\"])"));
    out.push_str("}\n");
    Ok(out)
}

/// Prefix every selector in a flat stylesheet (no nested at-rules).
fn scope(css: &str, prefix: &str) -> String {
    let mut out = String::new();
    for rule in css.split('}') {
        let Some((selectors, body)) = rule.split_once('{') else {
            continue;
        };
        // Drop comments syntect emits before rules.
        let selectors = match selectors.rfind("*/") {
            Some(i) => &selectors[i + 2..],
            None => selectors,
        };
        let scoped: Vec<String> = selectors
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("{prefix} {s}"))
            .collect();
        if !scoped.is_empty() {
            out.push_str(&format!("{} {{{}}}\n", scoped.join(", "), body));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_known_languages_with_prefixed_classes() {
        let html = block("rust", "fn main() { let x = 1; }\n");
        assert!(html.starts_with(
            "<div class=\"highlight\"><pre tabindex=\"0\" class=\"chroma\"><code class=\"language-rust\" data-lang=\"rust\">"
        ));
        assert!(html.ends_with("</code></pre></div>\n"));
        assert!(html.contains("class=\"sx-"), "{html}");
        assert!(html.contains("main"));
    }

    #[test]
    fn unknown_languages_are_plain_and_escaped() {
        let html = block("nosuchlang", "a < b\n");
        assert_eq!(
            html,
            "<div class=\"highlight\"><pre tabindex=\"0\" class=\"chroma\"><code class=\"language-nosuchlang\" data-lang=\"nosuchlang\">a &lt; b\n</code></pre></div>\n"
        );
    }

    #[test]
    fn wrappers_close_after_their_own_block_only() {
        let html = format!(
            "{OPEN}<code>a</code></pre>\n<pre><code>raw</code></pre>\n{OPEN}<code>b</code></pre>"
        );
        assert_eq!(
            close_wrappers(&html),
            format!(
                "{OPEN}<code>a</code></pre></div>\n<pre><code>raw</code></pre>\n{OPEN}<code>b</code></pre></div>"
            )
        );
        let done = block("rust", "x\n");
        assert_eq!(
            close_wrappers(&done),
            done,
            "already closed blocks stay as they are"
        );
    }

    #[test]
    fn chroma_names_map_to_syntect() {
        assert!(spans("shell", "echo hi\n").is_some());
        assert!(spans("cpp", "int x;\n").is_some());
    }

    #[test]
    fn css_scopes_dark_rules() {
        let sheet = css("InspiredGitHub", "base16-ocean.dark").unwrap();
        assert!(sheet.contains(".sx-"));
        assert!(sheet.contains("html[data-mode=\"dark\"] .sx-"));
        assert!(sheet.contains("@media (prefers-color-scheme: dark)"));
        assert!(css("InspiredGitHub", "nope").is_err());
    }
}
