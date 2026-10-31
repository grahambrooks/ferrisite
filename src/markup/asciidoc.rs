//! AsciiDoc through `adoc` (DESIGN.md section 6.2), the way backstage-rs
//! TechDocs uses it: preprocess (includes resolved from a base directory) →
//! parse → convert to HTML5, keeping the body of `<main id="content">`.
//!
//! Before conversion, diagram blocks (`[plantuml]`, `[mermaid]`, `[forge]`)
//! and `[source,lang]` listings are replaced with passthrough blocks holding
//! ferrisite's own rendering, so both formats get the same diagrams and the
//! same highlighting. Error-severity diagnostics fail the build.

use super::{Fences, Heading, Rendered, highlight};
use crate::diagrams::Lang;
use adoc::ast::{Attributes, Block, DelimitedBlock, DelimitedContent, DelimitedStyle};
use adoc::convert::html5::{Html5Converter, Html5Options, Stylesheet};
use adoc::preprocessor::{Preprocessor, SafeMode};
use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::path::Path;

pub fn render(source: &str, path: &str, base_dir: &Path, fences: Fences) -> Result<Rendered> {
    let base = Utf8PathBuf::from_path_buf(base_dir.to_path_buf())
        .map_err(|p| anyhow!("base directory {} is not UTF-8", p.display()))?;
    // Includes resolve relative to the including document, so present the
    // document as living in `base_dir`: `include::_pages/x.skin[]` then
    // resolves from there, as it did under Jekyll with the site root.
    let name = Utf8Path::new(path).file_name().unwrap_or(path);
    let top = base.join(name);
    let lines = Preprocessor::with_attributes(Attributes::new())
        .with_base_dir(base)
        .with_safe_mode(SafeMode::Safe)
        .run(source, &top)
        .map_err(|e| anyhow!("AsciiDoc preprocessing: {e}"))?;
    let mut doc = adoc::parser::parse_with(&lines, Attributes::new())
        .map_err(|e| anyhow!("AsciiDoc: {e}"))?;

    let mut state = Transform {
        fences,
        counter: 0,
        errors: Vec::new(),
    };
    state.blocks(&mut doc.blocks);
    if !state.errors.is_empty() {
        bail!("{}", state.errors.join("\n"));
    }

    let (html, diagnostics) = Html5Converter::with_options(Html5Options {
        stylesheet: Stylesheet::None,
    })
    .convert_with_diagnostics(&doc)
    .map_err(|e| anyhow!("AsciiDoc conversion: {e:?}"))?;
    let errors: Vec<String> = diagnostics
        .iter()
        .filter(|d| format!("{:?}", d.severity) == "Error")
        .map(|d| {
            format!(
                "line {}: {} ({})",
                d.location.line as usize + fences.line_offset,
                d.message,
                d.code
            )
        })
        .collect();
    if !errors.is_empty() {
        bail!("AsciiDoc: {}", errors.join("; "));
    }

    let body = main_content(&html).context("AsciiDoc output has no <main id=\"content\">")?;
    let headings = headings(&body);
    Ok(Rendered {
        html: body,
        headings,
    })
}

struct Transform<'a> {
    fences: Fences<'a>,
    counter: usize,
    errors: Vec<String>,
}

impl Transform<'_> {
    fn blocks(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Section(s) => self.blocks(&mut s.blocks),
                Block::Delimited(d) => self.delimited(d),
                _ => {}
            }
        }
    }

    fn delimited(&mut self, d: &mut DelimitedBlock) {
        if let DelimitedContent::Blocks { blocks } = &mut d.content {
            self.blocks(blocks);
            return;
        }
        if !matches!(d.style, DelimitedStyle::Listing | DelimitedStyle::Literal) {
            return;
        }
        let DelimitedContent::Raw { text } = &d.content else {
            return;
        };
        let style = d.meta.style.clone().unwrap_or_default();
        let html = if let Some(lang) = Lang::from_name(&style) {
            let Some(diagrams) = self.fences.diagrams else {
                return;
            };
            let id = format!("{}a{}", self.fences.id_prefix, self.counter);
            self.counter += 1;
            // `[forge, context system]`: positional values after the style
            // name the views to draw.
            let args = d.meta.positional.join(" ");
            match diagrams.render(lang, text, &args, &id) {
                Ok(html) => html,
                Err(e) => {
                    self.errors.push(format!(
                        "line {}: {} diagram: {e:#}",
                        d.location.line as usize + self.fences.line_offset,
                        lang.name()
                    ));
                    return;
                }
            }
        } else if style == "source" {
            let lang = d
                .meta
                .positional
                .first()
                .cloned()
                .or_else(|| d.meta.named.get("language").cloned())
                .unwrap_or_default();
            highlight::block(&lang, text)
        } else {
            return;
        };
        let title = d.meta.title.take();
        d.style = DelimitedStyle::Passthrough;
        d.content = DelimitedContent::Raw { text: html };
        d.meta = Default::default();
        d.meta.title = title;
    }
}

/// The content of `<main id="content">…</main>`.
fn main_content(html: &str) -> Option<String> {
    let open = "<main id=\"content\">";
    let start = html.find(open)? + open.len();
    let end = html.rfind("</main>")?;
    Some(html[start..end].trim().to_string() + "\n")
}

/// Headings for a table of contents. adoc puts the id on the enclosing
/// `<section id="_x">`, with a bare `<h2>`…`<h6>` as its first child.
fn headings(html: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("<section id=\"") {
        let after = &rest[i + 13..];
        rest = after;
        let Some(id_end) = after.find('"') else {
            continue;
        };
        let id = after[..id_end].to_string();
        let Some(h) = after.find("<h") else {
            continue;
        };
        let tag = &after[h + 2..];
        let Some(level) = tag.chars().next().and_then(|c| c.to_digit(10)) else {
            continue;
        };
        let Some(open_end) = tag.find('>') else {
            continue;
        };
        let close = format!("</h{level}>");
        let text = tag[open_end + 1..]
            .split_once(close.as_str())
            .map(|(t, _)| super::text::plain(t).trim().to_string())
            .unwrap_or_default();
        out.push(Heading {
            level: level as u8,
            id,
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagrams::Diagrams;

    fn render_str(src: &str) -> Result<Rendered> {
        let diagrams = Diagrams::new(Path::new(".")).unwrap();
        render(
            src,
            "test.adoc",
            Path::new("."),
            Fences {
                diagrams: Some(&diagrams),
                id_prefix: "t",
                line_offset: 0,
            },
        )
    }

    #[test]
    fn converts_body_and_collects_headings() {
        let r = render_str("= Title\n\nIntro *bold*.\n\n== First section\n\nText.\n").unwrap();
        assert!(
            !r.html.contains("<h1>"),
            "the page template shows the title"
        );
        assert!(r.html.contains("<strong>bold</strong>"), "{}", r.html);
        assert_eq!(r.headings.len(), 1);
        assert_eq!(r.headings[0].text, "First section");
    }

    #[test]
    fn plantuml_blocks_become_diagrams() {
        let r = render_str("= T\n\n[plantuml, seq, svg]\n----\nAlice -> Bob: hi\n----\n").unwrap();
        assert_eq!(r.html.matches("<svg").count(), 2, "{}", r.html);
        assert!(r.html.contains("diagram-plantuml"));
    }

    #[test]
    fn source_blocks_are_highlighted() {
        let r = render_str("= T\n\n[source,rust]\n----\nfn main() {}\n----\n").unwrap();
        assert!(r.html.contains("class=\"sx-"), "{}", r.html);
    }

    #[test]
    fn header_attributes_reach_include_targets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("samples")).unwrap();
        std::fs::write(dir.path().join("samples/code.txt"), "included text\n").unwrap();
        let diagrams = Diagrams::new(dir.path()).unwrap();
        let r = render(
            "= T\n:dir: samples\n\n----\ninclude::{dir}/code.txt[]\n----\n",
            "doc.adoc",
            dir.path(),
            Fences {
                diagrams: Some(&diagrams),
                id_prefix: "t",
                line_offset: 0,
            },
        )
        .unwrap();
        assert!(r.html.contains("included text"), "{}", r.html);
    }

    #[test]
    fn bad_diagram_fails_with_line() {
        let err = render_str("= T\n\ntext\n\n[mermaid]\n----\nnot a diagram\n----\n").unwrap_err();
        assert!(err.to_string().contains("line"), "{err}");
    }
}
