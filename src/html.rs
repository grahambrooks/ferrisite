//! A small HTML tokenizer for the passes that rewrite or inspect rendered
//! HTML (render-hook equivalents, diagram id prefixing, link checking).
//!
//! It splits a document into text, start tags (with attribute names and the
//! byte ranges of their values), end tags, and everything else (comments,
//! doctypes, CDATA). Quoted attribute values may contain `>`; the contents of
//! `<script>` and `<style>` are text, up to their closing tag. It doesn't
//! build a tree or decode entities: callers rewrite byte ranges of the
//! original, so everything they don't touch is copied through unchanged.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    Text(&'a str),
    Start(Tag<'a>),
    End {
        name: String,
        span: Range<usize>,
    },
    /// A comment, doctype, CDATA section or processing instruction.
    Other(&'a str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tag<'a> {
    /// The tag's source, `<` to `>`.
    pub raw: &'a str,
    /// Where `raw` sits in the tokenized source.
    pub span: Range<usize>,
    /// Lowercased.
    pub name: String,
    pub attrs: Vec<Attr>,
    pub self_closing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attr {
    /// Lowercased.
    pub name: String,
    /// The raw (still entity-encoded) value's range in the source, without
    /// quotes; `None` for a bare attribute such as `defer`.
    pub value: Option<Range<usize>>,
}

impl Tag<'_> {
    /// The raw value of attribute `name` (lowercase), from `src`.
    pub fn attr<'s>(&self, src: &'s str, name: &str) -> Option<&'s str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .and_then(|a| a.value.clone())
            .map(|r| &src[r])
    }
}

pub fn tokens(src: &str) -> Tokens<'_> {
    Tokens {
        src,
        pos: 0,
        raw_text: None,
    }
}

pub struct Tokens<'a> {
    src: &'a str,
    pos: usize,
    /// Inside `<script>` or `<style>`: the element's name.
    raw_text: Option<String>,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let src = self.src;
        let start = self.pos;
        if start >= src.len() {
            return None;
        }
        let rest = &src[start..];
        if let Some(name) = self.raw_text.take() {
            let end = find_ci(rest, &format!("</{name}")).unwrap_or(rest.len());
            if end > 0 {
                self.pos += end;
                return Some(Token::Text(&rest[..end]));
            }
        }
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(rest.len(), |e| e + 3);
            self.pos += end;
            return Some(Token::Other(&rest[..end]));
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = rest.find('>').map_or(rest.len(), |e| e + 1);
            self.pos += end;
            return Some(Token::Other(&rest[..end]));
        }
        let b = rest.as_bytes();
        if b.len() > 2 && b[0] == b'<' && b[1] == b'/' && b[2].is_ascii_alphabetic() {
            let end = rest.find('>').map_or(rest.len(), |e| e + 1);
            let name = rest[2..end]
                .trim_end_matches('>')
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            self.pos += end;
            return Some(Token::End {
                name,
                span: start..start + end,
            });
        }
        if b.len() > 1
            && b[0] == b'<'
            && b[1].is_ascii_alphabetic()
            && let Some(tag) = parse_tag(src, start)
        {
            self.pos = tag.span.end;
            if !tag.self_closing && (tag.name == "script" || tag.name == "style") {
                self.raw_text = Some(tag.name.clone());
            }
            return Some(Token::Start(tag));
        }
        // Text up to the next `<` (a stray `<` is text too).
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let end = rest[first..].find('<').map_or(rest.len(), |e| e + first);
        self.pos += end;
        Some(Token::Text(&rest[..end]))
    }
}

fn parse_tag(src: &str, start: usize) -> Option<Tag<'_>> {
    let b = src.as_bytes();
    let mut i = start + 1;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
        i += 1;
    }
    let name = src[start + 1..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        match b.get(i)? {
            b'>' => {
                i += 1;
                break;
            }
            b'/' => {
                i += 1;
                if b.get(i) == Some(&b'>') {
                    self_closing = true;
                    i += 1;
                    break;
                }
                continue;
            }
            _ => {}
        }
        let name_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let attr_name = src[name_start..i].to_ascii_lowercase();
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = None;
        if b.get(j) == Some(&b'=') {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            match b.get(j)? {
                q @ (b'"' | b'\'') => {
                    let close = src[j + 1..].find(*q as char)? + j + 1;
                    value = Some(j + 1..close);
                    i = close + 1;
                }
                _ => {
                    let v = j;
                    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                        j += 1;
                    }
                    value = Some(v..j);
                    i = j;
                }
            }
        }
        if !attr_name.is_empty() {
            attrs.push(Attr {
                name: attr_name,
                value,
            });
        }
    }
    Some(Tag {
        raw: &src[start..i],
        span: start..i,
        name,
        attrs,
        self_closing,
    })
}

/// ASCII case-insensitive `find`.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    (0..=h.len().checked_sub(n.len())?).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// `src` with each range replaced by its string. Ranges must be in order
/// and not overlap.
pub fn splice(src: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut out = String::with_capacity(src.len() + edits.len() * 16);
    let mut last = 0;
    for (r, with) in edits {
        out.push_str(&src[last..r.start]);
        out.push_str(with);
        last = r.end;
    }
    out.push_str(&src[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<String> {
        tokens(src)
            .map(|t| match t {
                Token::Text(s) => format!("T({s})"),
                Token::Start(t) => {
                    format!("S({}{})", t.name, if t.self_closing { "/" } else { "" })
                }
                Token::End { name, .. } => format!("E({name})"),
                Token::Other(s) => format!("O({s})"),
            })
            .collect()
    }

    #[test]
    fn splits_text_tags_and_comments() {
        assert_eq!(
            kinds("<p class=x>a &lt; b</p><!-- <p> --><br/>c < d"),
            [
                "S(p)",
                "T(a &lt; b)",
                "E(p)",
                "O(<!-- <p> -->)",
                "S(br/)",
                "T(c )",
                "T(< d)"
            ]
        );
    }

    #[test]
    fn quoted_values_may_hold_angle_brackets() {
        let src = r#"<img alt="a > b" src='/x.svg' />"#;
        let Some(Token::Start(t)) = tokens(src).next() else {
            panic!()
        };
        assert_eq!(t.raw, src);
        assert!(t.self_closing);
        assert_eq!(t.attr(src, "alt"), Some("a > b"));
        assert_eq!(t.attr(src, "src"), Some("/x.svg"));
    }

    #[test]
    fn script_and_style_contents_are_text() {
        assert_eq!(
            kinds("<script>if (a<b) x=`<a href=\"#y\">`</script><STYLE>p>a{}</STYLE>"),
            [
                "S(script)",
                "T(if (a<b) x=`<a href=\"#y\">`)",
                "E(script)",
                "S(style)",
                "T(p>a{})",
                "E(style)"
            ]
        );
    }

    #[test]
    fn bare_and_unquoted_attributes() {
        let src = "<input disabled value=3 data-x = 'y'>";
        let Some(Token::Start(t)) = tokens(src).next() else {
            panic!()
        };
        let names: Vec<&str> = t.attrs.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["disabled", "value", "data-x"]);
        assert_eq!(t.attr(src, "disabled"), None);
        assert_eq!(t.attr(src, "value"), Some("3"));
        assert_eq!(t.attr(src, "data-x"), Some("y"));
    }

    #[test]
    fn text_may_start_with_a_multibyte_character() {
        assert_eq!(kinds("<p>“é”</p>"), ["S(p)", "T(“é”)", "E(p)"]);
    }

    #[test]
    fn an_unterminated_tag_is_text() {
        assert_eq!(kinds("a <b class=\"x"), ["T(a )", "T(<b class=\"x)"]);
    }
}
