//! Markup rendering. Markdown goes through comrak, configured to match Hugo's
//! goldmark defaults (GFM tables, strikethrough, linkify, task lists,
//! footnotes, definition lists, typographer, raw HTML allowed).

pub mod anchor;
pub mod text;

use anchor::IdFactory;
use comrak::adapters::{HeadingAdapter, HeadingMeta};
use comrak::nodes::Sourcepos;
use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};
use std::fmt;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    pub text: String,
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
        });
        write!(output, "<h{} id=\"{}\">", heading.level, id)
    }

    fn exit(&self, output: &mut dyn fmt::Write, heading: &HeadingMeta) -> fmt::Result {
        writeln!(output, "</h{}>", heading.level)
    }
}

pub fn markdown(source: &str) -> Rendered {
    let options = options();
    let adapter = Headings {
        state: Mutex::new((IdFactory::default(), Vec::new())),
    };
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&adapter);
    let html = markdown_to_html_with_plugins(source, &options, &plugins);
    let (_, headings) = adapter.state.into_inner().expect("heading state");
    Rendered { html, headings }
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
}
