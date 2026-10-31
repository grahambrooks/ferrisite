//! Headings in rendered HTML: finding them again after rendering (for the
//! heading template and the table of contents), and Hugo's table of
//! contents (`.TableOfContents`, levels 2–3, unordered).

use super::Heading;

/// Where one heading sits in the HTML: `<hN id="..">` starts at `start`,
/// its inner HTML is `inner_start..inner_end`, and `end` is just past
/// `</hN>`.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    inner_start: usize,
    inner_end: usize,
    end: usize,
}

/// Find the headings the Markdown renderer wrote, in document order. A
/// heading that can't be found (raw HTML that happens to share an id, or a
/// summary that stops before it) ends the search.
fn spans(html: &str, headings: &[Heading]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut from = 0;
    for h in headings {
        let open = format!("<h{} id=\"{}\">", h.level, h.id);
        let close = format!("</h{}>", h.level);
        let Some(i) = html[from..].find(&open) else {
            break;
        };
        let start = from + i;
        let inner_start = start + open.len();
        let Some(j) = html[inner_start..].find(&close) else {
            break;
        };
        let inner_end = inner_start + j;
        let end = inner_end + close.len();
        out.push(Span {
            start,
            inner_start,
            inner_end,
            end,
        });
        from = end;
    }
    out
}

/// Set each heading's `html` (its rendered inner HTML) from the page HTML.
pub fn fill_html(html: &str, headings: &mut [Heading]) {
    for (span, h) in spans(html, headings).into_iter().zip(headings.iter_mut()) {
        h.html = html[span.inner_start..span.inner_end].to_string();
    }
}

/// Replace each heading with `render(heading, inner_html)`, Hugo's heading
/// render hook. Headings in raw HTML are left alone, as in Hugo.
pub fn apply_hook(
    html: &str,
    headings: &[Heading],
    render: &dyn Fn(&Heading, &str) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let mut out = String::with_capacity(html.len() * 2);
    let mut last = 0;
    for (span, h) in spans(html, headings).into_iter().zip(headings) {
        out.push_str(&html[last..span.start]);
        let inner = &html[span.inner_start..span.inner_end];
        out.push_str(render(h, inner)?.trim_end());
        last = span.end;
    }
    out.push_str(&html[last..]);
    Ok(out)
}

#[derive(Default)]
struct Node {
    id: String,
    title: String,
    children: Vec<Node>,
}

impl Node {
    fn is_zero(&self) -> bool {
        self.id.is_empty() && self.title.is_empty()
    }
}

/// Hugo's `.TableOfContents` with its defaults (start level 2, end level 3,
/// `<ul>`), byte for byte: the same tree building (gaps in heading levels
/// become empty `<li>`s) and the same indentation. Empty without headings.
pub fn hugo_toc(headings: &[Heading]) -> String {
    if headings.is_empty() {
        return String::new();
    }
    let mut roots: Vec<Node> = Vec::new();
    let mut row: isize = -1;
    for h in headings {
        if h.level == 1 || row == -1 {
            row += 1;
        }
        let node = Node {
            id: h.id.clone(),
            title: toc_title(&h.html),
            children: Vec::new(),
        };
        add_at(&mut roots, node, row as usize, usize::from(h.level) - 1);
    }
    let mut s = String::from("<nav id=\"TableOfContents\">");
    write_headings(&mut s, 1, 0, &roots);
    s.push_str("</nav>");
    s
}

fn add_at(roots: &mut Vec<Node>, node: Node, row: usize, level: usize) {
    while roots.len() <= row {
        roots.push(Node::default());
    }
    if level == 0 {
        roots[row] = node;
        return;
    }
    let mut heading = &mut roots[row];
    for _ in 1..level {
        if heading.children.is_empty() {
            heading.children.push(Node::default());
        }
        heading = heading.children.last_mut().expect("just ensured");
    }
    heading.children.push(node);
}

const START: usize = 2;
const STOP: usize = 3;

fn write_headings(s: &mut String, level: usize, indent: usize, nodes: &[Node]) {
    if level < START {
        for n in nodes {
            write_headings(s, level + 1, indent, &n.children);
        }
        return;
    }
    if level > STOP {
        return;
    }
    let has_children = !nodes.is_empty();
    if has_children {
        s.push('\n');
        push_indent(s, indent + 1);
        s.push_str("<ul>\n");
    }
    for n in nodes {
        push_indent(s, indent + 2);
        s.push_str("<li>");
        if !n.is_zero() {
            s.push_str(&format!(
                "<a href=\"#{}\">{}</a>",
                super::highlight::escape(&n.id),
                n.title
            ));
        }
        write_headings(s, level + 1, indent + 2, &n.children);
        s.push_str("</li>\n");
    }
    if has_children {
        push_indent(s, indent + 1);
        s.push_str("</ul>\n");
        push_indent(s, indent);
    }
}

fn push_indent(s: &mut String, n: usize) {
    for _ in 0..n {
        s.push_str("  ");
    }
}

/// Hugo's TOC title: the heading's inner HTML with only inline formatting
/// elements kept (links are unwrapped), whitespace collapsed.
fn toc_title(html: &str) -> String {
    if !html.contains('<') {
        return html.to_string();
    }
    const ALLOWED: &[&str] = &[
        "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "del", "dfn", "em", "i", "ins",
        "kbd", "mark", "q", "rp", "rt", "ruby", "s", "samp", "small", "span", "strong", "sub",
        "sup", "time", "u", "var", "wbr",
    ];
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('>').map(|e| i + e + 1).unwrap_or(rest.len());
        let tag = &rest[i..end];
        let name: String = tag
            .trim_start_matches(['<', '/'])
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if ALLOWED.contains(&name.as_str()) {
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(level: u8, id: &str, html: &str) -> Heading {
        Heading {
            level,
            id: id.into(),
            text: html.into(),
            html: html.into(),
        }
    }

    #[test]
    fn toc_matches_hugo_layout() {
        let toc = hugo_toc(&[
            h(1, "title", "Title"),
            h(2, "a", "A"),
            h(2, "b", "B <code>x</code>"),
            h(3, "b1", "B1 <a href=\"/x\">link</a>"),
            h(4, "deep", "Deep"),
            h(2, "c", "C"),
        ]);
        assert_eq!(
            toc,
            "<nav id=\"TableOfContents\">\n  <ul>\n    <li><a href=\"#a\">A</a></li>\n    <li><a href=\"#b\">B <code>x</code></a>\n      <ul>\n        <li><a href=\"#b1\">B1 link</a></li>\n      </ul>\n    </li>\n    <li><a href=\"#c\">C</a></li>\n  </ul>\n</nav>"
        );
    }

    #[test]
    fn level_gaps_become_empty_items() {
        let toc = hugo_toc(&[h(3, "x", "X")]);
        assert_eq!(
            toc,
            "<nav id=\"TableOfContents\">\n  <ul>\n    <li>\n      <ul>\n        <li><a href=\"#x\">X</a></li>\n      </ul>\n    </li>\n  </ul>\n</nav>"
        );
        assert_eq!(hugo_toc(&[]), "");
    }

    #[test]
    fn hook_replaces_rendered_headings_only() {
        let html = "<h2 id=\"a\">A <em>b</em></h2>\n<p>x</p>\n<h2 id=\"z\">raw</h2>\n";
        let heads = vec![h(2, "a", "")];
        let out = apply_hook(html, &heads, &|h, inner| {
            Ok(format!(
                "<h{0} id=\"{1}\" class=x>{inner}</h{0}>\n",
                h.level, h.id
            ))
        })
        .unwrap();
        assert_eq!(
            out,
            "<h2 id=\"a\" class=x>A <em>b</em></h2>\n<p>x</p>\n<h2 id=\"z\">raw</h2>\n"
        );
        let mut heads = heads;
        fill_html(html, &mut heads);
        assert_eq!(heads[0].html, "A <em>b</em>");
    }
}
