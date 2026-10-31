//! Golden test: heading IDs match Hugo 0.169's output for every ATX heading
//! in grahambrooks.com's published posts (fixture extracted from the Hugo
//! build on 2026-10-31). Headings are rendered per post, in order, so
//! duplicate numbering (`-1`, `-2`) is checked as well.

use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
struct Heading {
    md: String,
    id: String,
}

#[test]
fn heading_ids_match_hugo() {
    let fixture: BTreeMap<String, Vec<Heading>> =
        serde_yaml::from_str(include_str!("fixtures/hugo_heading_ids.json")).unwrap();
    let mut failures = Vec::new();
    let mut total = 0;
    for (post, headings) in &fixture {
        let source: String = headings.iter().map(|h| format!("{}\n\n", h.md)).collect();
        let got = ferrisite::markup::markdown(&source).headings;
        assert_eq!(got.len(), headings.len(), "{post}: heading count");
        for (h, g) in headings.iter().zip(&got) {
            total += 1;
            if h.id != g.id {
                failures.push(format!(
                    "{post}: {:?}\n    hugo:      {}\n    ferrisite: {}",
                    h.md, h.id, g.id
                ));
            }
        }
    }
    assert_eq!(total, 409, "fixture should cover 409 headings");
    assert!(
        failures.is_empty(),
        "{} of {total} heading IDs differ from Hugo:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
