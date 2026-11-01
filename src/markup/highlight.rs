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

/// Stylesheet for highlighted code: the `dark` theme when the page's
/// `data-mode` is `dark`, or when it's `auto` (or unset) and the operating
/// system prefers dark; the `light` theme otherwise. Each theme is scoped so
/// it only matches in its own mode: a theme's more specific selectors (e.g.
/// InspiredGitHub's JSON rules) would otherwise out-rank the other's.
pub fn css(
    light: &str,
    dark: &str,
    contrast: Option<&crate::config::HighlightContrast>,
) -> Result<String> {
    let themes = ThemeSet::load_defaults();
    let get = |name: &str| {
        themes.themes.get(name).ok_or_else(|| {
            anyhow!(
                "unknown highlight theme {name:?}; available: {}",
                themes.themes.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })
    };
    let mut light_theme = get(light)?.clone();
    let mut dark_theme = get(dark)?.clone();
    if let Some(c) = contrast {
        raise_contrast(&mut light_theme, rgb(&c.light_background)?, c.min);
        raise_contrast(&mut dark_theme, rgb(&c.dark_background)?, c.min);
    }
    let light_css = css_for_theme_with_class_style(&light_theme, CLASS_STYLE)?;
    let dark_css = css_for_theme_with_class_style(&dark_theme, CLASS_STYLE)?;
    let mut out = format!("/* ferrisite syntax highlighting: {light} (light), {dark} (dark). */\n");
    out.push_str(&scope(&light_css, "html[data-mode=\"light\"]"));
    out.push_str("@media not all and (prefers-color-scheme: dark) {\n");
    out.push_str(&scope(&light_css, "html:not([data-mode=\"dark\"])"));
    out.push_str("}\n");
    out.push_str(&scope(&dark_css, "html[data-mode=\"dark\"]"));
    out.push_str("@media (prefers-color-scheme: dark) {\n");
    out.push_str(&scope(&dark_css, "html:not([data-mode=\"light\"])"));
    out.push_str("}\n");
    Ok(out)
}

/// Mix every foreground colour in `theme` that has less than `min` contrast
/// against `bg` toward black (on a light background) or white (on a dark
/// one), by the least amount that reaches `min`.
fn raise_contrast(theme: &mut syntect::highlighting::Theme, bg: [u8; 3], min: f64) {
    let fix = |c: &mut syntect::highlighting::Color| {
        let fg = [c.r, c.g, c.b];
        if contrast_ratio(fg, bg) >= min {
            return;
        }
        let target = if luminance(bg) > 0.5 {
            [0u8; 3]
        } else {
            [255u8; 3]
        };
        let mix = |t: f64| -> [u8; 3] {
            std::array::from_fn(|i| {
                (fg[i] as f64 + (target[i] as f64 - fg[i] as f64) * t).round() as u8
            })
        };
        // Bisect for the smallest mix that passes (the ratio rises with t).
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if contrast_ratio(mix(mid), bg) >= min {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let [r, g, b] = mix(hi);
        (c.r, c.g, c.b) = (r, g, b);
    };
    if let Some(c) = theme.settings.foreground.as_mut() {
        fix(c);
    }
    for item in &mut theme.scopes {
        if let Some(c) = item.style.foreground.as_mut() {
            fix(c);
        }
    }
}

/// WCAG 2 relative luminance of an sRGB colour.
fn luminance(c: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG 2 contrast ratio between two colours (1 to 21).
fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `#rrggbb` or `#rgb`.
fn rgb(s: &str) -> Result<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    let h: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return Err(anyhow!("{s:?} is not a #rrggbb colour")),
    };
    let byte = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| anyhow!("{s:?} is not a #rrggbb colour"))
    };
    Ok([byte(0)?, byte(2)?, byte(4)?])
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
    fn css_scopes_each_theme_to_its_mode() {
        let sheet = css("InspiredGitHub", "base16-ocean.dark", None).unwrap();
        assert!(sheet.contains("html[data-mode=\"light\"] .sx-"));
        assert!(sheet.contains("html[data-mode=\"dark\"] .sx-"));
        assert!(sheet.contains("@media not all and (prefers-color-scheme: dark)"));
        assert!(sheet.contains("@media (prefers-color-scheme: dark)"));
        // No unscoped rule, so neither theme leaks into the other's mode.
        for line in sheet.lines() {
            assert!(!line.starts_with(".sx-"), "unscoped rule: {line}");
        }
        assert!(css("InspiredGitHub", "nope", None).is_err());
    }

    #[test]
    fn contrast_floor_raises_only_failing_colours() {
        let c = crate::config::HighlightContrast {
            min: 4.5,
            light_background: "#f2f5f7".into(),
            dark_background: "#1a1d24".into(),
        };
        let sheet = css("InspiredGitHub", "base16-ocean.dark", Some(&c)).unwrap();
        // Foreground colours of the rules scoped to one mode.
        let colours = |scope: &str| -> Vec<[u8; 3]> {
            sheet
                .split('}')
                .filter_map(|rule| rule.split_once('{'))
                .filter(|(sel, _)| sel.trim_start().starts_with(scope))
                .filter_map(|(_, body)| {
                    let decl = body
                        .split(';')
                        .map(str::trim)
                        .find(|d| d.starts_with("color:"))?;
                    rgb(decl.trim_start_matches("color:").trim()).ok()
                })
                .collect()
        };
        let light = colours("html[data-mode=\"light\"]");
        let dark = colours("html[data-mode=\"dark\"]");
        assert!(!light.is_empty() && !dark.is_empty());
        for fg in &light {
            assert!(contrast_ratio(*fg, [0xf2, 0xf5, 0xf7]) >= 4.5, "{fg:?}");
        }
        for fg in &dark {
            assert!(contrast_ratio(*fg, [0x1a, 0x1d, 0x24]) >= 4.5, "{fg:?}");
        }
        // A colour that already passes is left exactly as the theme has it
        // (base16-ocean's body text, #c0c5ce, is 9.7:1 on #1a1d24).
        assert!(sheet.contains("#c0c5ce"));
        // Without the floor, the theme's own failing colours remain.
        assert!(
            css("InspiredGitHub", "base16-ocean.dark", None)
                .unwrap()
                .contains("#65737e")
        );
    }

    #[test]
    fn contrast_ratios_match_wcag() {
        assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
        assert!((contrast_ratio([0x65, 0x73, 0x7e], [0x1a, 0x1d, 0x24]) - 3.46).abs() < 0.01);
    }
}
