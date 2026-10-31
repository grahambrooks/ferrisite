//! `ferrisite check`: verify a built site on its own (DESIGN.md section 9.9).
//!
//! Always: every internal link and `#fragment` resolves. With `--agents`:
//! every HTML page has a Markdown copy and a `rel="alternate"` link to it,
//! and `/llms.txt` has the spec's structure with every link resolving.

use crate::parity;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Report {
    pub links_checked: usize,
    pub broken_links: Vec<String>,
    /// Broken links listed in `[check] known_broken`, with their reasons.
    pub known_broken: Vec<String>,
    pub agent_problems: Vec<String>,
}

impl Report {
    pub fn failed(&self) -> bool {
        !self.broken_links.is_empty() || !self.agent_problems.is_empty()
    }
}

pub fn run(
    out: &Path,
    base_url: &str,
    agents: bool,
    known: &[crate::config::KnownBroken],
) -> Result<Report> {
    let files = parity::list_files(out)?;
    let mut report = Report::default();
    for b in parity::broken_links(out, &files, base_url, &mut report.links_checked)? {
        let target = b.split_once(" → ").map(|(_, t)| t).unwrap_or(&b);
        let target = target.trim_end_matches(" (no such id)");
        match known.iter().find(|k| k.link == target) {
            Some(k) => report.known_broken.push(format!("{b} ({})", k.reason)),
            None => report.broken_links.push(b),
        }
    }
    if agents {
        report.agent_problems = agent_problems(out, &files, base_url)?;
    }
    Ok(report)
}

fn agent_problems(out: &Path, files: &[String], base_url: &str) -> Result<Vec<String>> {
    let set: BTreeSet<&String> = files.iter().collect();
    // Only generated pages need copies; static HTML is copied verbatim. The
    // build's marker lists generated files (absent: check every page).
    let generated: Option<BTreeSet<String>> =
        std::fs::read_to_string(out.join(crate::build::MARKER))
            .ok()
            .filter(|m| !m.is_empty())
            .map(|m| m.lines().map(String::from).collect());
    let mut problems = Vec::new();
    for f in files
        .iter()
        .filter(|f| f.ends_with("index.html"))
        .filter(|f| generated.as_ref().is_none_or(|g| g.contains(*f)))
    {
        let html = std::fs::read_to_string(out.join(f))?;
        if parity::alias_target(&html).is_some() {
            continue;
        }
        let dir = &f[..f.len() - "index.html".len()];
        // Paginated list pages share their first page's copy.
        if dir.contains("/page/") || dir.starts_with("page/") {
            continue;
        }
        let md = format!("{dir}index.md");
        if !set.contains(&md) {
            problems.push(format!("{f}: no Markdown copy at {md}"));
        }
        if !html.contains("rel=\"alternate\" type=\"text/markdown\"") {
            problems.push(format!(
                "{f}: no <link rel=\"alternate\" type=\"text/markdown\">"
            ));
        }
    }
    match std::fs::read_to_string(out.join("llms.txt")) {
        Err(_) => problems.push("llms.txt is missing".to_string()),
        Ok(text) => {
            let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            if !first.starts_with("# ") {
                problems.push("llms.txt must start with an H1 (`# Title`)".to_string());
            }
            let mut links = 0;
            for line in text.lines() {
                let mut rest = line;
                while let Some(i) = rest.find("](") {
                    let after = &rest[i + 2..];
                    let Some(end) = after.find(')') else { break };
                    let target = &after[..end];
                    rest = &after[end..];
                    links += 1;
                    if let Some(path) = target.strip_prefix(base_url)
                        && !set.contains(&path.to_string())
                    {
                        problems.push(format!("llms.txt links to {target}, which doesn't exist"));
                    }
                }
            }
            if links == 0 {
                problems.push("llms.txt has no links".to_string());
            }
        }
    }
    Ok(problems)
}
