//! Template filters that reproduce Hugo's string functions exactly, so a
//! ported template prints what the Hugo one printed: `plainify`,
//! `html_unescape`, `truncate` and `jsonify`. Each follows the Hugo 0.169
//! source it names.

use minijinja::Value;

/// Hugo's `plainify` (`tpl.StripHTML`): drop tags, comments, and the
/// contents of `script`/`style`; collapse runs of whitespace to their first
/// character, with newlines written as spaces; `<br>` and `</p>` become a
/// newline; block elements keep the text either side of them apart.
pub fn plainify(s: &str) -> String {
    if !s.contains('<') {
        return s.to_string();
    }
    let mut w = StripWriter::default();
    let mut sc = Scanner { s, pos: 0 };
    loop {
        let text_start = sc.pos;
        let Some(t) = sc.next() else {
            w.text(&s[text_start..]);
            break;
        };
        w.text(&s[text_start..t.start]);
        match t.kind {
            TokenKind::Start if t.name == "br" => w.space('\n'),
            TokenKind::End if t.name == "p" => w.space('\n'),
            TokenKind::Start if (t.name == "script" || t.name == "style") => {
                if !t.self_closing {
                    sc.skip_raw_text(&t.name);
                }
            }
            _ if is_block_element(&t.name) => w.needs_space = true,
            _ => {}
        }
    }
    w.out
}

#[derive(Default)]
struct StripWriter {
    out: String,
    was_space: bool,
    needs_space: bool,
}

impl StripWriter {
    fn text(&mut self, s: &str) {
        for c in s.chars() {
            if c.is_ascii() {
                if matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r') {
                    self.space(if c == '\n' { ' ' } else { c });
                } else {
                    self.non_space(c);
                }
            } else if c.is_whitespace() {
                if !self.was_space {
                    self.out.push(c);
                    self.was_space = true;
                }
            } else {
                self.non_space(c);
            }
        }
    }

    fn non_space(&mut self, c: char) {
        if self.needs_space && !self.was_space && !self.out.is_empty() {
            self.out.push(' ');
        }
        self.out.push(c);
        self.was_space = false;
        self.needs_space = false;
    }

    fn space(&mut self, c: char) {
        if !self.was_space {
            self.out.push(c);
            self.was_space = true;
        }
    }
}

#[derive(PartialEq)]
enum TokenKind {
    Start,
    End,
    Markup,
}

struct Token {
    kind: TokenKind,
    name: String,
    self_closing: bool,
    start: usize,
}

/// Port of Hugo's `common/hhtml` scanner: finds tags without building a tree.
struct Scanner<'a> {
    s: &'a str,
    pos: usize,
}

impl Scanner<'_> {
    fn next(&mut self) -> Option<Token> {
        let b = self.s.as_bytes();
        let limit = b.len();
        loop {
            let i = self.s[self.pos..].find('<')?;
            if self.pos + i + 1 >= limit {
                self.pos = limit;
                return None;
            }
            let start = self.pos + i;
            self.pos = start + 1;
            let c = b[self.pos];
            let kind = if c == b'!' || c == b'?' {
                let rest = &self.s[self.pos..];
                if rest.starts_with("!--") {
                    self.pos += 3;
                    self.skip_past("-->");
                } else if rest.starts_with("![CDATA[") {
                    self.skip_past("]]>");
                } else {
                    self.skip_past(">");
                }
                return Some(Token {
                    kind: TokenKind::Markup,
                    name: String::new(),
                    self_closing: false,
                    start,
                });
            } else if c == b'/' {
                self.pos += 1;
                if self.pos < limit && !b[self.pos].is_ascii_alphabetic() {
                    self.skip_past(">");
                    return Some(Token {
                        kind: TokenKind::Markup,
                        name: String::new(),
                        self_closing: false,
                        start,
                    });
                }
                TokenKind::End
            } else if c.is_ascii_alphabetic() {
                TokenKind::Start
            } else {
                continue;
            };
            let name_start = self.pos;
            while self.pos < limit && !is_tag_name_end(b[self.pos]) {
                self.pos += 1;
            }
            let name = self.s[name_start..self.pos].to_ascii_lowercase();
            let mut self_closing = false;
            while self.pos < limit {
                let c = b[self.pos];
                self.pos += 1;
                match c {
                    b'>' => break,
                    b'/' => {
                        self_closing =
                            kind == TokenKind::Start && self.pos < limit && b[self.pos] == b'>'
                    }
                    b'=' => {
                        while self.pos < limit && is_space(b[self.pos]) {
                            self.pos += 1;
                        }
                        if self.pos < limit && (b[self.pos] == b'"' || b[self.pos] == b'\'') {
                            let q = b[self.pos] as char;
                            self.pos += 1;
                            match self.s[self.pos..].find(q) {
                                Some(j) => self.pos += j + 1,
                                None => self.pos = limit,
                            }
                        }
                    }
                    _ => {}
                }
            }
            return Some(Token {
                kind,
                name,
                self_closing,
                start,
            });
        }
    }

    fn skip_raw_text(&mut self, name: &str) {
        loop {
            self.skip_past("</");
            if self.pos >= self.s.len() {
                return;
            }
            let rest = &self.s.as_bytes()[self.pos..];
            if rest.len() > name.len()
                && rest[..name.len()].eq_ignore_ascii_case(name.as_bytes())
                && is_tag_name_end(rest[name.len()])
            {
                self.pos -= 2;
                return;
            }
        }
    }

    fn skip_past(&mut self, sep: &str) {
        match self.s[self.pos..].find(sep) {
            Some(i) => self.pos += i + sep.len(),
            None => self.pos = self.s.len(),
        }
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

fn is_tag_name_end(c: u8) -> bool {
    is_space(c) || c == b'/' || c == b'>'
}

fn is_block_element(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "caption"
            | "dd"
            | "details"
            | "dialog"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "ul"
    )
}

/// Go's `html.UnescapeString` for the entities rendered content and
/// templates produce: numeric references and the common named ones.
pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map(|e| e + 1)
            .unwrap_or(rest.len());
        let name = &rest[1..end];
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse::<u32>().ok(),
            };
            code.and_then(char::from_u32).map(String::from)
        } else {
            named_entity(name).map(String::from)
        };
        match decoded {
            Some(d) => {
                out.push_str(&d);
                let consumed = if rest[end..].starts_with(';') {
                    end + 1
                } else {
                    end
                };
                rest = &rest[consumed..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201c}",
        "rdquo" => "\u{201d}",
        "ndash" => "\u{2013}",
        "mdash" => "\u{2014}",
        "hellip" => "\u{2026}",
        "laquo" => "\u{ab}",
        "raquo" => "\u{bb}",
        "copy" => "\u{a9}",
        "reg" => "\u{ae}",
        "trade" => "\u{2122}",
        "middot" => "\u{b7}",
        "rarr" => "\u{2192}",
        "larr" => "\u{2190}",
        "times" => "\u{d7}",
        _ => return None,
    })
}

/// Go's `html.EscapeString`.
pub fn go_html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&#34;"),
            c => out.push(c),
        }
    }
    out
}

/// Hugo's `truncate LENGTH TEXT` with the default ` …` ellipsis: cut at the
/// last word boundary within `length` characters. Plain text is escaped
/// with Go's `html.EscapeString`; text already marked safe is kept as HTML,
/// tags don't count towards the length, and tags left open are closed. The
/// result is safe either way, as Hugo returns `template.HTML`.
pub fn truncate(value: &Value, length: usize) -> Value {
    let text = value.to_string();
    let is_html = value.is_safe();
    let ellipsis = " …";
    if text.chars().count() <= length {
        return Value::from_safe_string(if is_html { text } else { go_html_escape(&text) });
    }
    let mut tags: Vec<(String, usize, bool)> = Vec::new();
    let (mut last_word, mut last_non_space, mut current, mut next_tag) = (0, 0, 0, 0);
    for (i, r) in text.char_indices() {
        if i < next_tag {
            continue;
        }
        if is_html && let Some((len, name, closing, singlet)) = html_tag_at(&text[i..]) {
            next_tag = i + len;
            last_word = last_non_space;
            if !singlet {
                tags.push((name, i, !closing));
            }
            continue;
        }
        current += 1;
        if r.is_whitespace() {
            last_word = last_non_space;
        } else if is_cjk(r) {
            last_word = last_non_space;
            last_non_space = i + r.len_utf8();
            if current <= length {
                last_word = last_non_space;
            }
        } else {
            last_non_space = i + r.len_utf8();
        }
        if current > length {
            let end = if last_word == 0 { i } else { last_word };
            let mut out = text[..end].to_string();
            if is_html {
                out.push_str(ellipsis);
                let mut skipping: Option<&str> = None;
                for (name, pos, open) in tags.iter().rev() {
                    if *pos >= end || skipping.is_some() {
                        if skipping == Some(name.as_str()) {
                            skipping = None;
                        }
                        continue;
                    }
                    if *open {
                        out.push_str(&format!("</{name}>"));
                    } else {
                        skipping = Some(name);
                    }
                }
                return Value::from_safe_string(out);
            }
            return Value::from_safe_string(go_html_escape(&out) + ellipsis);
        }
    }
    Value::from_safe_string(if is_html { text } else { go_html_escape(&text) })
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3005..=0x3007 | 0x3021..=0x3029
        | 0x3038..=0x303B | 0x3041..=0x30FF | 0x3131..=0x318E | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF
        | 0xF900..=0xFAFF | 0x20000..=0x3134F)
}

/// Hugo's `tagRE` (`^<(/)?([^ ]+?)(?:(\s*/)| .*?)?>`): the length of a tag
/// at the start of `s`, its name, whether it closes, and whether it is a
/// void element or written self-closing.
fn html_tag_at(s: &str) -> Option<(usize, String, bool, bool)> {
    let rest = s.strip_prefix('<')?;
    let (closing, rest) = match rest.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let gt = rest.find('>')?;
    let inner = &rest[..gt];
    if inner.is_empty() || inner.starts_with(' ') {
        return None;
    }
    let name_end = inner.find(' ').unwrap_or(inner.len());
    let mut name = &inner[..name_end];
    let mut self_closing = false;
    if name_end == inner.len() {
        let trimmed = name.trim_end_matches('/').trim_end();
        if trimmed.len() != name.len() && name.ends_with('/') {
            self_closing = true;
            name = trimmed;
        }
    } else if inner.trim_end().ends_with('/') && inner[name_end..].trim() == "/" {
        self_closing = true;
    }
    let singlet = self_closing
        || matches!(
            name,
            "br" | "col" | "link" | "base" | "img" | "param" | "area" | "hr" | "input"
        );
    let len = 1 + usize::from(closing) + gt + 1;
    Some((len, name.to_string(), closing, singlet))
}

/// Hugo's `jsonify`: Go's `encoding/json` output, with object keys sorted
/// and `<`, `>`, `&`, U+2028 and U+2029 escaped.
pub fn jsonify(value: &Value) -> Result<Value, minijinja::Error> {
    let json: serde_json::Value = serde_json::to_value(value).map_err(|e| {
        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
    })?;
    let mut out = String::new();
    write_json(&json, &mut out);
    Ok(Value::from_safe_string(out))
}

fn write_json(v: &serde_json::Value, out: &mut String) {
    use serde_json::Value as J;
    match v {
        J::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (n, k) in keys.into_iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                write_json(&map[k], out);
            }
            out.push('}');
        }
        J::Array(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        J::String(s) => write_json_string(s, out),
        other => out.push_str(&other.to_string()),
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plainify_collapses_whitespace_and_separates_blocks() {
        assert_eq!(plainify("no tags  here"), "no tags  here");
        assert_eq!(
            plainify(
                "<h1 id=\"t\">\n  Title\n  <a href=\"#t\"><svg></svg></a>\n</h1>\n<p>One <em>two</em>.</p>\n<p>Three</p>"
            ),
            " Title One two.\nThree\n"
        );
        assert_eq!(plainify("<li>a</li><li>b</li>"), "a b");
        assert_eq!(plainify("x<script>let a = '<p>';</script>y"), "xy");
        assert_eq!(plainify("a<!-- c -->b<br>c"), "ab\nc");
    }

    #[test]
    fn unescape_named_and_numeric() {
        assert_eq!(
            html_unescape("it&rsquo;s &amp; &#39;q&#39; &#x2014; &bogus; &"),
            "it\u{2019}s & 'q' \u{2014} &bogus; &"
        );
    }

    #[test]
    fn truncate_plain_text_like_hugo() {
        let v = Value::from("Lorem ipsum dolor sit amet");
        assert_eq!(truncate(&v, 10).to_string(), "Lorem …");
        assert_eq!(truncate(&v, 100).to_string(), "Lorem ipsum dolor sit amet");
        assert_eq!(
            truncate(&Value::from("it's \"a\" <b>"), 100).to_string(),
            "it&#39;s &#34;a&#34; &lt;b&gt;"
        );
        assert_eq!(
            truncate(&Value::from("Supercalifragilistic"), 5).to_string(),
            "Super …"
        );
    }

    #[test]
    fn truncate_html_keeps_and_closes_tags() {
        let v = Value::from_safe_string("<p>Lorem <em>ipsum dolor</em> sit</p>".into());
        assert_eq!(
            truncate(&v, 13).to_string(),
            "<p>Lorem <em>ipsum …</em></p>"
        );
        let e = Value::from_safe_string("It&rsquo;s here".into());
        assert_eq!(truncate(&e, 12).to_string(), "It&rsquo;s …");
    }

    #[test]
    fn jsonify_sorts_keys_and_escapes_like_go() {
        let v = Value::from(minijinja::value::Serde(
            serde_json::json!({"b": 1, "a": "<x> & 'y'", "c": [true]}),
        ));
        assert_eq!(
            jsonify(&v).unwrap().to_string(),
            r#"{"a":"\u003cx\u003e \u0026 'y'","b":1,"c":[true]}"#
        );
    }
}
