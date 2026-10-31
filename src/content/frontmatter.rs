//! Frontmatter: YAML between `---` lines or TOML between `+++` lines, as Hugo
//! reads it. Known keys are typed; everything else is kept in `params`.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone, Default)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub date: Option<DateTime<FixedOffset>>,
    pub lastmod: Option<DateTime<FixedOffset>>,
    pub draft: bool,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub aliases: Vec<String>,
    pub weight: i64,
    /// Every key, including the typed ones, for templates and later stages.
    pub params: Mapping,
}

/// Split a file into its frontmatter and body.
pub fn split(raw: &str) -> Result<(FrontMatter, &str)> {
    let (delim, is_toml) = if raw.starts_with("---") {
        ("---", false)
    } else if raw.starts_with("+++") {
        ("+++", true)
    } else {
        return Ok((FrontMatter::default(), raw));
    };
    let after_open = raw[3..]
        .strip_prefix("\r\n")
        .or_else(|| raw[3..].strip_prefix('\n'))
        .context("frontmatter delimiter must be followed by a newline")?;
    let close = find_closing(after_open, delim).context("unclosed frontmatter")?;
    let header = &after_open[..close];
    let mut body = &after_open[close + delim.len()..];
    body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .unwrap_or(body);
    let params = if is_toml {
        toml_to_yaml(toml::from_str::<toml::Value>(header).context("TOML frontmatter")?)
    } else if header.trim().is_empty() {
        Value::Mapping(Mapping::new())
    } else {
        serde_yaml::from_str::<Value>(header).context("YAML frontmatter")?
    };
    let Value::Mapping(params) = params else {
        bail!("frontmatter is not a mapping");
    };
    Ok((typed(params)?, body))
}

/// Position of the closing delimiter: a line consisting only of `delim`.
fn find_closing(s: &str, delim: &str) -> Option<usize> {
    let mut offset = 0;
    for line in s.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == delim {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn typed(params: Mapping) -> Result<FrontMatter> {
    let get = |k: &str| params.get(Value::String(k.into()));
    let string = |k: &str| get(k).and_then(scalar_string);
    let date = |k: &str| -> Result<Option<DateTime<FixedOffset>>> {
        match get(k).and_then(scalar_string) {
            None => Ok(None),
            Some(s) if s.is_empty() => Ok(None),
            Some(s) => parse_date(&s)
                .map(Some)
                .with_context(|| format!("`{k}`: unrecognised date {s:?}")),
        }
    };
    Ok(FrontMatter {
        title: string("title"),
        date: date("date")?,
        lastmod: date("lastmod")?,
        draft: matches!(get("draft"), Some(Value::Bool(true))),
        description: string("description"),
        summary: string("summary"),
        aliases: get("aliases").map(string_list).unwrap_or_default(),
        weight: get("weight").and_then(Value::as_i64).unwrap_or(0),
        params,
    })
}

fn scalar_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A list of strings, accepting a single scalar as a one-item list.
pub fn string_list(v: &Value) -> Vec<String> {
    match v {
        Value::Sequence(items) => items.iter().filter_map(scalar_string).collect(),
        other => scalar_string(other).into_iter().collect(),
    }
}

/// The date shapes in use: RFC 3339 (`2026-03-22T12:13:20-06:00`,
/// `2009-07-18T00:00:00Z`) and a bare `2026-03-22` (midnight UTC, as Hugo).
pub fn parse_date(s: &str) -> Result<DateTime<FixedOffset>> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Ok(d);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let utc = FixedOffset::east_opt(0).expect("zero offset");
        return Ok(d
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_local_timezone(utc)
            .single()
            .expect("UTC is unambiguous"));
    }
    bail!("expected RFC 3339 or YYYY-MM-DD")
}

fn toml_to_yaml(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => Value::Number(f.into()),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Sequence(a.into_iter().map(toml_to_yaml).collect()),
        toml::Value::Table(t) => Value::Mapping(
            t.into_iter()
                .map(|(k, v)| (Value::String(k), toml_to_yaml(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_frontmatter_with_quoted_and_unquoted_dates() {
        let (fm, body) = split(
            "---\ntitle: Hello\ndate: '2009-07-18T00:00:00Z'\nlastmod: 2026-03-22T12:13:20-06:00\naliases:\n- /blog/hello/\ndraft: true\n---\nBody\n",
        )
        .unwrap();
        assert_eq!(fm.title.as_deref(), Some("Hello"));
        assert_eq!(fm.date.unwrap().to_rfc3339(), "2009-07-18T00:00:00+00:00");
        assert_eq!(
            fm.lastmod.unwrap().to_rfc3339(),
            "2026-03-22T12:13:20-06:00"
        );
        assert_eq!(fm.aliases, vec!["/blog/hello/"]);
        assert!(fm.draft);
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn toml_frontmatter() {
        let (fm, body) =
            split("+++\ntitle = \"Coding Architect\"\ndate = 2026-01-02\n+++\n\nHi").unwrap();
        assert_eq!(fm.title.as_deref(), Some("Coding Architect"));
        assert_eq!(fm.date.unwrap().to_rfc3339(), "2026-01-02T00:00:00+00:00");
        assert_eq!(body, "\nHi");
    }

    #[test]
    fn no_frontmatter() {
        let (fm, body) = split("# Just markdown").unwrap();
        assert!(fm.title.is_none());
        assert_eq!(body, "# Just markdown");
    }

    #[test]
    fn delimiter_inside_body_is_not_a_close() {
        let (_, body) = split("---\ntitle: a\n---\ntext\n---\nmore\n").unwrap();
        assert_eq!(body, "text\n---\nmore\n");
    }

    #[test]
    fn bad_date_is_an_error() {
        assert!(split("---\ndate: next tuesday\n---\n").is_err());
    }
}
