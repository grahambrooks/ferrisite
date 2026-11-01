//! Markup rendering. Markdown goes through comrak, configured to match Hugo's
//! goldmark defaults (GFM tables, strikethrough, linkify, task lists,
//! footnotes, definition lists, typographer, raw HTML allowed).

pub mod anchor;
pub mod asciidoc;
pub mod highlight;
pub mod shortcodes;
pub mod text;
pub mod toc;

use crate::diagrams::{Diagrams, Lang};
use anchor::IdFactory;
use comrak::adapters::{
    CodefenceRendererAdapter, HeadingAdapter, HeadingMeta, SyntaxHighlighterAdapter,
};
use comrak::nodes::Sourcepos;
use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    /// Plain text.
    pub text: String,
    /// Rendered inner HTML (Markdown only; empty for AsciiDoc).
    pub html: String,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub html: String,
    pub headings: Vec<Heading>,
}

pub fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.description_lists = true;
    o.parse.smart = true;
    o.render.r#unsafe = true;
    o
}

/// Writes `<hN id="...">` the way Hugo does, collecting headings for the
/// table of contents.
struct Headings {
    state: Mutex<(IdFactory, Vec<Heading>)>,
}

impl HeadingAdapter for Headings {
    fn enter(
        &self,
        output: &mut dyn fmt::Write,
        heading: &HeadingMeta,
        _sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        let mut state = self.state.lock().expect("heading state");
        let id = state.0.unique(&heading.content);
        state.1.push(Heading {
            level: heading.level,
            id: id.clone(),
            text: heading.content.clone(),
            html: String::new(),
        });
        write!(output, "<h{} id=\"{}\">", heading.level, id)
    }

    fn exit(&self, output: &mut dyn fmt::Write, heading: &HeadingMeta) -> fmt::Result {
        writeln!(output, "</h{}>", heading.level)
    }
}

/// Highlighted code blocks via syntect (see `highlight`).
struct Highlighter;

impl SyntaxHighlighterAdapter for Highlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        let html = lang
            .filter(|l| !l.is_empty())
            .and_then(|l| highlight::spans(l, code))
            .unwrap_or_else(|| highlight::escape(code));
        output.write_str(&html)
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        _attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        output.write_str(highlight::OPEN)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        let lang = attributes
            .get("class")
            .and_then(|c| c.strip_prefix("language-").map(str::to_string));
        output.write_str(&highlight::code_tag(lang.as_deref()))
    }
}

/// What diagram blocks need: the renderer and a page-unique id prefix.
#[derive(Clone, Copy)]
pub struct Fences<'a> {
    pub diagrams: Option<&'a Diagrams>,
    /// Unique per page, so diagrams from several pages' summaries on one list
    /// page can't share ids.
    pub id_prefix: &'a str,
    /// Lines in the file before the source being rendered, so errors name
    /// file lines.
    pub line_offset: usize,
}

/// File lines of the diagram fences in `source`, in document order (the order
/// comrak renders them). comrak only reports positions to fence renderers
/// with its `sourcepos` option, which would also add attributes to all HTML,
/// and those positions would be relative to the placeholder-expanded text.
fn diagram_fence_lines(source: &str, line_offset: usize) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for (i, raw) in source.lines().enumerate() {
        let line = raw.trim_start_matches([' ', '>', '\t']);
        let Some(c) = line.chars().next().filter(|c| *c == '`' || *c == '~') else {
            continue;
        };
        let run = line.chars().take_while(|x| *x == c).count();
        if run < 3 {
            continue;
        }
        match open {
            Some((oc, olen)) => {
                if oc == c && run >= olen && line[run..].trim().is_empty() {
                    open = None;
                }
            }
            None => {
                let lang = line[run..].split_whitespace().next().unwrap_or("");
                if Lang::from_name(lang).is_some() {
                    lines.push(i + 1 + line_offset);
                }
                open = Some((c, run));
            }
        }
    }
    lines
}

/// Renders ```mermaid / ```plantuml / ```forge blocks. comrak adapters can
/// only return `fmt::Error`, so failures are collected and reported after
/// rendering, with the block's line.
struct DiagramFence<'a> {
    lang: Lang,
    diagrams: &'a Diagrams,
    prefix: &'a str,
    counter: &'a AtomicUsize,
    errors: &'a Mutex<Vec<String>>,
    /// File line of the n-th diagram fence.
    fence_lines: &'a [usize],
}

impl CodefenceRendererAdapter for DiagramFence<'_> {
    fn write(
        &self,
        output: &mut dyn fmt::Write,
        _lang: &str,
        meta: &str,
        code: &str,
        _sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let id = format!("{}{n}", self.prefix);
        match self.diagrams.render(self.lang, code, meta, &id) {
            Ok(html) => output.write_str(&html),
            Err(e) => {
                let line = self.fence_lines.get(n).copied().unwrap_or(0);
                self.errors
                    .lock()
                    .expect("diagram errors")
                    .push(format!("line {line}: {} diagram: {e:#}", self.lang.name()));
                Ok(())
            }
        }
    }
}

/// Markdown without diagram support (diagram blocks render as code).
pub fn markdown(source: &str) -> Rendered {
    markdown_with(
        source,
        Fences {
            diagrams: None,
            id_prefix: "",
            line_offset: 0,
        },
        &[],
    )
    .expect("rendering without diagrams cannot fail")
}

/// `fence_lines`: file lines of the diagram fences, for error messages.
pub fn markdown_with(
    source: &str,
    fences: Fences,
    fence_lines: &[usize],
) -> anyhow::Result<Rendered> {
    let options = options();
    let adapter = Headings {
        state: Mutex::new((IdFactory::default(), Vec::new())),
    };
    let counter = AtomicUsize::new(0);
    let errors = Mutex::new(Vec::new());
    let fence = |lang| {
        fences.diagrams.map(|diagrams| DiagramFence {
            lang,
            diagrams,
            prefix: fences.id_prefix,
            counter: &counter,
            errors: &errors,
            fence_lines,
        })
    };
    let (mermaid, plantuml, forge) = (
        fence(Lang::Mermaid),
        fence(Lang::PlantUml),
        fence(Lang::Forge),
    );
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&adapter);
    plugins.render.codefence_syntax_highlighter = Some(&Highlighter);
    for (names, adapter) in [
        (&["mermaid", "mmd"][..], &mermaid),
        (&["plantuml", "puml"][..], &plantuml),
        (&["forge"][..], &forge),
    ] {
        if let Some(a) = adapter {
            for name in names {
                plugins
                    .render
                    .codefence_renderers
                    .insert(name.to_string(), a as &dyn CodefenceRendererAdapter);
            }
        }
    }
    let html =
        highlight::close_wrappers(&markdown_to_html_with_plugins(source, &options, &plugins));
    let errors = errors.into_inner().expect("diagram errors");
    if !errors.is_empty() {
        anyhow::bail!("{}", errors.join("\n"));
    }
    let (_, mut headings) = adapter.state.into_inner().expect("heading state");
    toc::fill_html(&html, &mut headings);
    Ok(Rendered { html, headings })
}

/// Render Markdown containing Hugo shortcodes: calls become placeholders,
/// the Markdown is rendered, then placeholders become shortcode output.
pub fn markdown_with_shortcodes(
    source: &str,
    expander: &dyn shortcodes::Expand,
    fences: Fences,
) -> anyhow::Result<Rendered> {
    let fence_lines = diagram_fence_lines(source, fences.line_offset);
    let (text, outputs) = shortcodes::expand_to_placeholders(source, expander, fences.line_offset)?;
    let mut rendered = markdown_with(&text, fences, &fence_lines)?;
    if !outputs.is_empty() {
        rendered.html = shortcodes::substitute(&rendered.html, &outputs);
        for h in &mut rendered.headings {
            h.text = shortcodes::substitute(&h.text, &outputs);
            h.html = shortcodes::substitute(&h.html, &outputs);
        }
    }
    Ok(rendered)
}

/// Render-hook equivalents applied to rendered HTML.
#[derive(Debug, Clone, Default)]
pub struct Hooks {
    /// Directory holding files for root-relative `.svg` image paths.
    pub inline_svg_from: Option<std::path::PathBuf>,
    pub table_wrapper: Option<String>,
}

impl Hooks {
    /// One pass over the HTML: local SVG images written in Markdown are
    /// inlined, and Markdown tables (comrak's attribute-less `<table>`) are
    /// wrapped in the configured `<div>`.
    pub fn apply(&self, html: &str) -> anyhow::Result<String> {
        use crate::html::{Token, splice, tokens};
        if self.inline_svg_from.is_none() && self.table_wrapper.is_none() {
            return Ok(html.to_string());
        }
        let mut edits = Vec::new();
        // Per open <table>: whether it was wrapped.
        let mut tables: Vec<bool> = Vec::new();
        for token in tokens(html) {
            match token {
                Token::Start(tag) if tag.name == "img" => {
                    if let Some(dir) = &self.inline_svg_from
                        && let Some(svg) = inline_svg(html, &tag, dir)?
                    {
                        edits.push((tag.span.clone(), svg));
                    }
                }
                Token::Start(tag) if tag.name == "table" => {
                    let wrap = self.table_wrapper.as_ref().filter(|_| tag.attrs.is_empty());
                    if let Some(class) = wrap {
                        edits.push((
                            tag.span.clone(),
                            format!("<div class=\"{class}\">\n<table>"),
                        ));
                    }
                    tables.push(wrap.is_some());
                }
                // Pops every closing table; emits only for ones it wrapped.
                Token::End { name, span } if name == "table" && tables.pop() == Some(true) => {
                    edits.push((span, "</table>\n</div>".to_string()));
                }
                _ => {}
            }
        }
        Ok(splice(html, &edits))
    }
}

/// For an image written in Markdown whose `src` is a local `.svg`, the file's
/// contents in `<span class="inline-svg">`. Comrak always renders Markdown
/// images as `<img src=".." alt=".." [title=".."] />`; raw HTML `<img>` in
/// the source is left alone, as Hugo's render hook never sees it.
fn inline_svg(
    html: &str,
    tag: &crate::html::Tag,
    dir: &std::path::Path,
) -> anyhow::Result<Option<String>> {
    let from_markdown = tag.raw.ends_with(" />")
        && tag
            .attrs
            .iter()
            .all(|a| a.value.is_some() && matches!(a.name.as_str(), "src" | "alt" | "title"));
    let Some(src) = tag.attr(html, "src").filter(|_| from_markdown) else {
        return Ok(None);
    };
    let local_svg = src.to_ascii_lowercase().ends_with(".svg")
        && !src.starts_with("http:")
        && !src.starts_with("https:")
        && !src.starts_with("//");
    if !local_svg {
        return Ok(None);
    }
    let path = dir.join(src.trim_start_matches('/'));
    let svg = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("inline SVG {src}: {} ({e})", path.display()))?;
    let mut out = "<span class=\"inline-svg\"".to_string();
    if let Some(t) = tag.attr(html, "title").filter(|t| !t.is_empty()) {
        out.push_str(&format!(" title=\"{t}\""));
    }
    if let Some(a) = tag.attr(html, "alt").filter(|a| !a.is_empty()) {
        out.push_str(&format!(" aria-label=\"{a}\" role=\"img\""));
    }
    out.push('>');
    out.push_str(&svg);
    out.push_str("</span>");
    Ok(Some(out))
}

/// Hugo's `markdownify`: render Markdown, and drop the `<p>` wrapper when the
/// result is a single paragraph.
pub fn markdownify(source: &str) -> String {
    unwrap_paragraph(&markdown(source).html)
}

/// Drop the `<p>` wrapper of HTML that is a single paragraph.
pub fn unwrap_paragraph(html: &str) -> String {
    let trimmed = html.trim_end();
    if let Some(inner) = trimmed
        .strip_prefix("<p>")
        .and_then(|s| s.strip_suffix("</p>"))
        && !inner.contains("<p>")
    {
        return inner.to_string();
    }
    html.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_get_hugo_ids() {
        let r = markdown("# Title\n\nText\n\n## A **bold** move\n\n## Title\n");
        assert!(r.html.contains("<h1 id=\"title\">Title</h1>"));
        assert!(
            r.html
                .contains("<h2 id=\"a-bold-move\">A <strong>bold</strong> move</h2>")
        );
        assert!(r.html.contains("<h2 id=\"title-1\">"));
        assert_eq!(r.headings.len(), 3);
    }

    #[test]
    fn hooks_inline_local_svgs_and_wrap_tables() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        std::fs::write(dir.path().join("images/d.svg"), "<svg>D</svg>").unwrap();
        let hooks = Hooks {
            inline_svg_from: Some(dir.path().to_path_buf()),
            table_wrapper: Some("wrap".into()),
        };
        let html =
            markdown("![A diagram](/images/d.svg) ![x](/images/p.png)\n\n| a |\n|---|\n| b |\n")
                .html;
        let out = hooks.apply(&html).unwrap();
        assert!(out.contains("<span class=\"inline-svg\" aria-label=\"A diagram\" role=\"img\"><svg>D</svg></span>"), "{out}");
        assert!(out.contains("<img src=\"/images/p.png\""));
        assert!(out.contains("<div class=\"wrap\">\n<table>"));
        let raw = r#"<img src="/images/d.svg" alt="raw" style="width:1px">"#;
        assert_eq!(
            hooks.apply(raw).unwrap(),
            raw,
            "raw HTML images are left alone"
        );
        let missing = markdown("![m](/images/missing.svg)").html;
        assert!(
            hooks.apply(&missing).is_err(),
            "a missing SVG fails the build"
        );
    }

    #[test]
    fn markdownify_unwraps_single_paragraphs() {
        assert_eq!(markdownify("Read **this**"), "Read <strong>this</strong>");
        assert!(markdownify("one\n\ntwo").starts_with("<p>one</p>"));
    }

    #[test]
    fn goldmark_parity_features() {
        let r = markdown(
            "| a |\n|---|\n| b |\n\n~~x~~ \"q\" -- https://example.com\n\n<div>raw</div>\n",
        );
        assert!(r.html.contains("<table>"));
        assert!(r.html.contains("<del>x</del>"));
        assert!(r.html.contains("“q”"));
        assert!(r.html.contains("<a href=\"https://example.com\">"));
        assert!(r.html.contains("<div>raw</div>"));
    }

    #[test]
    fn table_wrapper_wraps_only_markdown_tables_and_stays_balanced() {
        let hooks = Hooks {
            inline_svg_from: None,
            table_wrapper: Some("tw".into()),
        };
        let html = "<table>\n<tr><td><table class=\"raw\"><tr><td>x</td></tr></table></td></tr>\n</table>\n<table class=\"raw\"></table>";
        let out = hooks.apply(html).unwrap();
        assert_eq!(out.matches("<div class=\"tw\">").count(), 1, "{out}");
        assert_eq!(out.matches("</div>").count(), 1, "{out}");
        assert!(out.starts_with("<div class=\"tw\">\n<table>"), "{out}");
        assert!(
            out.contains("</table>\n</div>\n<table class=\"raw\"></table>"),
            "{out}"
        );
    }
}
