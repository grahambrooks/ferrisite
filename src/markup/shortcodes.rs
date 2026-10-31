//! Hugo shortcode syntax, `{{< name "arg" >}}` and paired
//! `{{< name >}}inner{{< /name >}}`, expanded the way Hugo does it: top-level
//! shortcodes become placeholder tokens before Markdown rendering and are
//! swapped for their output afterwards; nested shortcodes are expanded straight
//! into their parent's `inner`. `{{</* name */>}}` escapes a call.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Text(String),
    Call(Call),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub args: Vec<String>,
    /// Children of a paired shortcode; `None` for a standalone one.
    pub inner: Option<Vec<Node>>,
    /// 1-based line of the opening tag, for error messages.
    pub line: usize,
}

/// Shortcodes that are never paired, so a later `{{< /ref >}}` can't capture them.
const STANDALONE: &[&str] = &["ref", "relref"];

pub fn parse(src: &str) -> Result<Vec<Node>> {
    parse_with_offset(src, 0)
}

/// Like `parse`, with line numbers shifted by `line_offset` (lines before
/// `src` in its file), so errors and `Call::line` name file lines.
pub fn parse_with_offset(src: &str, line_offset: usize) -> Result<Vec<Node>> {
    let mut pos = 0;
    parse_until(src, &mut pos, None, line_offset)
}

fn line_of(src: &str, offset: usize, line_offset: usize) -> usize {
    src[..offset].matches('\n').count() + 1 + line_offset
}

fn parse_until(
    src: &str,
    pos: &mut usize,
    until: Option<(&str, usize)>,
    line_offset: usize,
) -> Result<Vec<Node>> {
    let mut nodes = Vec::new();
    loop {
        let Some(rel) = src[*pos..].find("{{<") else {
            if let Some((name, line)) = until {
                bail!("line {line}: `{{{{< {name} >}}}}` is never closed");
            }
            push_text(&mut nodes, &src[*pos..]);
            *pos = src.len();
            return Ok(nodes);
        };
        let start = *pos + rel;
        push_text(&mut nodes, &src[*pos..start]);
        let line = line_of(src, start, line_offset);
        let Some(end_rel) = src[start..].find(">}}") else {
            bail!("line {line}: unterminated shortcode tag");
        };
        let end = start + end_rel + 3;
        let body = src[start + 3..end - 3].trim();
        *pos = end;
        if body.starts_with("/*") && body.ends_with("*/") {
            let literal = body[2..body.len() - 2].trim();
            push_text(&mut nodes, &format!("{{{{< {literal} >}}}}"));
            continue;
        }
        if let Some(closing) = body.strip_prefix('/') {
            let closing = closing.trim();
            match until {
                Some((name, _)) if name == closing => return Ok(nodes),
                _ => bail!("line {line}: unexpected closing tag `{{{{< /{closing} >}}}}`"),
            }
        }
        let self_closing = body.ends_with('/');
        let body = body.trim_end_matches('/').trim();
        let mut words = split_args(body).map_err(|e| anyhow::anyhow!("line {line}: {e}"))?;
        if words.is_empty() {
            bail!("line {line}: empty shortcode tag");
        }
        let name = words.remove(0);
        let paired = !self_closing
            && !STANDALONE.contains(&name.as_str())
            && has_closing(&src[*pos..], &name);
        let inner = if paired {
            Some(parse_until(src, pos, Some((&name, line)), line_offset)?)
        } else {
            None
        };
        nodes.push(Node::Call(Call {
            name,
            args: words,
            inner,
            line,
        }));
    }
}

fn push_text(nodes: &mut Vec<Node>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Node::Text(prev)) = nodes.last_mut() {
        prev.push_str(text);
    } else {
        nodes.push(Node::Text(text.to_string()));
    }
}

fn has_closing(rest: &str, name: &str) -> bool {
    let mut search = rest;
    while let Some(i) = search.find("{{<") {
        let tail = &search[i + 3..];
        if let Some(j) = tail.find(">}}") {
            let body = tail[..j].trim();
            if body.strip_prefix('/').map(str::trim) == Some(name) {
                return true;
            }
            search = &tail[j..];
        } else {
            return false;
        }
    }
    false
}

/// Split `name "quoted arg" bare 'single'` into words, honouring `\"` escapes
/// in double quotes and taking backtick strings raw.
fn split_args(s: &str) -> std::result::Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        match c {
            '"' => {
                chars.next();
                loop {
                    match chars.next() {
                        Some('\\') => word.extend(chars.next()),
                        Some('"') => break,
                        Some(ch) => word.push(ch),
                        None => return Err("unterminated quoted argument".into()),
                    }
                }
            }
            '`' => {
                chars.next();
                loop {
                    match chars.next() {
                        Some('`') => break,
                        Some(ch) => word.push(ch),
                        None => return Err("unterminated backtick argument".into()),
                    }
                }
            }
            _ => {
                while let Some(&ch) = chars.peek() {
                    if ch.is_whitespace() {
                        break;
                    }
                    word.push(ch);
                    chars.next();
                }
            }
        }
        out.push(word);
    }
    Ok(out)
}

/// Expands one shortcode call to its output. `inner` already has nested
/// shortcodes expanded.
pub trait Expand {
    fn expand(&self, call: &Call, inner: Option<&str>) -> Result<String>;
}

/// Replace top-level calls with placeholder tokens. Returns the source to feed
/// the Markdown renderer and the outputs to substitute afterwards.
pub fn expand_to_placeholders(
    src: &str,
    expander: &dyn Expand,
    line_offset: usize,
) -> Result<(String, Vec<String>)> {
    let nodes = parse_with_offset(src, line_offset)?;
    let mut text = String::with_capacity(src.len());
    let mut outputs = Vec::new();
    for node in &nodes {
        match node {
            Node::Text(t) => text.push_str(t),
            Node::Call(call) => {
                let out = expand_call(call, expander)?;
                text.push_str(&token(outputs.len()));
                outputs.push(out);
            }
        }
    }
    Ok((text, outputs))
}

/// Expand calls straight to their output (used for `inner` and for plain
/// rendering where placeholders aren't needed).
pub fn expand_inline(nodes: &[Node], expander: &dyn Expand) -> Result<String> {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Call(call) => out.push_str(&expand_call(call, expander)?),
        }
    }
    Ok(out)
}

fn expand_call(call: &Call, expander: &dyn Expand) -> Result<String> {
    let inner = match &call.inner {
        Some(children) => Some(expand_inline(children, expander)?),
        None => None,
    };
    expander
        .expand(call, inner.as_deref())
        .map_err(|e| anyhow::anyhow!("line {}: shortcode `{}`: {e:#}", call.line, call.name))
}

pub fn token(i: usize) -> String {
    format!("FERRISITESHORTCODE{i:04}END")
}

/// Swap placeholder tokens for shortcode output. A token that is a paragraph
/// on its own takes the `<p>` wrapper with it, as Hugo does.
pub fn substitute(html: &str, outputs: &[String]) -> String {
    let mut out = html.to_string();
    for (i, value) in outputs.iter().enumerate() {
        let t = token(i);
        out = out
            .replace(&format!("<p>{t}</p>\n"), value)
            .replace(&format!("<p>{t}</p>"), value)
            .replace(&t, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    impl Expand for Echo {
        fn expand(&self, call: &Call, inner: Option<&str>) -> Result<String> {
            if call.name == "boom" {
                bail!("exploded");
            }
            Ok(match inner {
                Some(i) => format!("<{}>{}</{}>", call.name, i, call.name),
                None => format!("[{}:{}]", call.name, call.args.join("|")),
            })
        }
    }

    #[test]
    fn parses_args_and_pairs() {
        let nodes = parse("a {{< ref \"x y\" >}} b {{< tip \"warning\" >}}hi{{< /tip >}}").unwrap();
        assert_eq!(nodes.len(), 4);
        let Node::Call(r) = &nodes[1] else { panic!() };
        assert_eq!(
            (r.name.as_str(), r.args.clone()),
            ("ref", vec!["x y".to_string()])
        );
        let Node::Call(t) = &nodes[3] else { panic!() };
        assert_eq!(t.inner, Some(vec![Node::Text("hi".into())]));
    }

    #[test]
    fn nested_calls_expand_into_inner() {
        let (text, outs) = expand_to_placeholders(
            "{{< block >}}{{< column >}}x{{< /column >}}{{< /block >}}",
            &Echo,
            0,
        )
        .unwrap();
        assert_eq!(text, token(0));
        assert_eq!(outs[0], "<block><column>x</column></block>");
    }

    #[test]
    fn paragraph_tokens_lose_their_p() {
        let html = format!(
            "<p>{}</p>\n<p>see <a href=\"{}\">x</a></p>\n",
            token(0),
            token(1)
        );
        let out = substitute(&html, &["<div>D</div>\n".into(), "/post/y/".into()]);
        assert_eq!(out, "<div>D</div>\n<p>see <a href=\"/post/y/\">x</a></p>\n");
    }

    #[test]
    fn errors_name_the_line() {
        let err = expand_to_placeholders("one\ntwo {{< boom >}}", &Echo, 0).unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        let err = parse("x\n{{< tip >}}never closed{{< /tip").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        let err = parse("{{< /tip >}}").unwrap_err();
        assert!(err.to_string().contains("unexpected closing"), "{err}");
    }

    #[test]
    fn escaped_calls_stay_literal() {
        let nodes = parse("{{</* ref \"x\" */>}}").unwrap();
        assert_eq!(nodes, vec![Node::Text("{{< ref \"x\" >}}".into())]);
    }

    #[test]
    fn standalone_ref_is_never_paired() {
        let nodes = parse("{{< ref \"a\" >}} text {{< /ref >}}");
        assert!(
            nodes.is_err(),
            "a stray closing ref is an error, not a pair"
        );
    }
}
