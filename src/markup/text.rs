//! Plain-text helpers over rendered HTML: word counts, reading time and
//! Hugo's automatic summaries.

/// Strip tags and decode the entities that rendering produces.
pub fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

pub fn word_count(html: &str) -> usize {
    plain(html).split_whitespace().count()
}

/// Minutes, rounded up, at Hugo's 213 words per minute.
pub fn reading_time(words: usize) -> usize {
    words.div_ceil(213).max(1)
}

/// Hugo's automatic summary (0.134+): whole top-level blocks of the rendered
/// HTML, taken in order until the word count reaches `length`. Never cuts a
/// paragraph.
pub fn auto_summary(html: &str, length: usize) -> String {
    let mut out = String::new();
    let mut words = 0;
    for block in top_level_blocks(html) {
        out.push_str(block);
        words += word_count(block);
        if words >= length {
            break;
        }
    }
    out.trim_end().to_string()
}

/// Split rendered HTML into its top-level elements. comrak writes each
/// top-level block ending with a newline, so a block ends at a newline where
/// the element nesting depth returns to zero.
fn top_level_blocks(html: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &html[i..];
            let end = rest.find('>').map(|e| i + e).unwrap_or(bytes.len() - 1);
            let tag = &html[i + 1..end];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let void = matches!(
                name.as_str(),
                "br" | "hr" | "img" | "input" | "meta" | "link" | "source" | "wbr"
            ) || tag.ends_with('/')
                || tag.starts_with('!');
            if tag.starts_with('/') {
                depth -= 1;
            } else if !void {
                depth += 1;
            }
            i = end + 1;
            continue;
        }
        if bytes[i] == b'\n' && depth <= 0 {
            let block = &html[start..=i];
            if !block.trim().is_empty() {
                blocks.push(block);
            }
            start = i + 1;
            depth = 0;
        }
        i += 1;
    }
    if start < html.len() && !html[start..].trim().is_empty() {
        blocks.push(&html[start..]);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_counts() {
        assert_eq!(word_count("<p>One <em>two</em> three&amp;four</p>"), 3);
        assert_eq!(reading_time(1), 1);
        assert_eq!(reading_time(214), 2);
    }

    #[test]
    fn summary_takes_whole_paragraphs() {
        let html = "<p>one two three</p>\n<p>four five</p>\n<p>six</p>\n";
        assert_eq!(
            auto_summary(html, 4),
            "<p>one two three</p>\n<p>four five</p>"
        );
        assert_eq!(auto_summary(html, 2), "<p>one two three</p>");
    }

    #[test]
    fn nested_blocks_stay_whole() {
        let html = "<ul>\n<li>a b</li>\n<li>c</li>\n</ul>\n<p>d</p>\n";
        assert_eq!(
            auto_summary(html, 1),
            "<ul>\n<li>a b</li>\n<li>c</li>\n</ul>"
        );
    }
}
