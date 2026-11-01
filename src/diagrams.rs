//! Diagrams rendered at build time with Graham's tools (DESIGN.md section 7):
//! Mermaid via `mmd`, PlantUML via `puml`, forge models via `forge-dsl`, all
//! painted through `diagram-scene`.
//!
//! Each diagram is laid out once and painted twice, with the light and dark
//! palettes, into `<div class="diagram-light">` and `<div class="diagram-dark">`;
//! CSS shows the one matching the page's mode (decision O2). Every `id` inside
//! each copy gets a unique prefix, so the two copies (and other diagrams on the
//! page) can't resolve each other's markers or clip paths.
//!
//! Unsupported syntax is an error, never a fallback (decision C).

use anyhow::{Context, Result, anyhow, bail};
use diagram_scene::{Scene, Theme};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Mermaid,
    PlantUml,
    Forge,
}

impl Lang {
    /// The diagram language a code-fence info string or AsciiDoc block style names.
    pub fn from_name(name: &str) -> Option<Lang> {
        match name.trim().to_ascii_lowercase().as_str() {
            "mermaid" | "mmd" => Some(Lang::Mermaid),
            "plantuml" | "puml" => Some(Lang::PlantUml),
            "forge" => Some(Lang::Forge),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Mermaid => "mermaid",
            Lang::PlantUml => "plantuml",
            Lang::Forge => "forge",
        }
    }
}

pub struct Diagrams {
    fonts: mmd::text::BundledFonts,
    /// Where PlantUML `!include` and forge imports resolve from.
    base_dir: PathBuf,
    light: Theme,
    dark: Theme,
}

impl Diagrams {
    pub fn new(base_dir: &Path) -> Result<Diagrams> {
        Ok(Diagrams {
            fonts: mmd::text::BundledFonts::new()
                .ok_or_else(|| anyhow!("mmd's bundled fonts could not be loaded"))?,
            base_dir: base_dir.to_path_buf(),
            light: Theme::light(),
            dark: Theme::dark(),
        })
    }

    /// Apply the site's palette overrides to the light and dark themes.
    pub fn with_palettes(
        mut self,
        light: &crate::config::DiagramPalette,
        dark: &crate::config::DiagramPalette,
    ) -> Result<Diagrams> {
        apply_palette(&mut self.light, light).context("[markup.diagram_light]")?;
        apply_palette(&mut self.dark, dark).context("[markup.diagram_dark]")?;
        Ok(self)
    }

    /// Render one diagram to HTML holding a light and a dark copy. `id` must
    /// be unique within the page (e.g. the diagram's index); it prefixes the
    /// SVGs' internal ids. `args` is anything after the language in the info
    /// string (forge: the view keys to render; default all views).
    pub fn render(&self, lang: Lang, source: &str, args: &str, id: &str) -> Result<String> {
        let copies: Vec<(String, String)> = match lang {
            Lang::Mermaid => {
                let scene = mmd::scene(source, &self.fonts).map_err(|e| anyhow!("{e}"))?;
                vec![self.paint(&scene, true)]
            }
            Lang::PlantUml => self.plantuml(source)?,
            Lang::Forge => self.forge(source, args)?,
        };
        let mut out = format!("<div class=\"diagram diagram-{}\">", lang.name());
        for (n, (light, dark)) in copies.iter().enumerate() {
            out.push_str(&format!(
                "<div class=\"diagram-light\">{}</div><div class=\"diagram-dark\">{}</div>",
                prefix_ids(light, &format!("{id}-{n}-l-")),
                prefix_ids(dark, &format!("{id}-{n}-d-"))
            ));
        }
        out.push_str("</div>\n");
        Ok(out)
    }

    /// Paint a scene in both palettes. `bundled_font` names mmd's font, which
    /// is what the scene was measured with.
    fn paint(&self, scene: &Scene, bundled_font: bool) -> (String, String) {
        let theme = |base: Theme| {
            if bundled_font {
                Theme {
                    font_family: mmd::text::BundledFonts::FAMILY.into(),
                    ..base
                }
            } else {
                base
            }
        };
        (
            diagram_scene::svg::to_svg(scene, &theme(self.light.clone())),
            diagram_scene::svg::to_svg(scene, &theme(self.dark.clone())),
        )
    }

    fn plantuml(&self, source: &str) -> Result<Vec<(String, String)>> {
        // A bare block (no @startuml) is one diagram, as asciidoctor-diagram allows.
        let wrapped;
        let source = if source.contains("@start") {
            source
        } else {
            wrapped = format!("@startuml\n{source}\n@enduml\n");
            &wrapped
        };
        let diagrams = puml::parser::preprocessor::preprocess(source, Some(&self.base_dir));
        if diagrams.is_empty() {
            bail!("no @startuml … @enduml diagram found");
        }
        diagrams
            .iter()
            .map(|d| {
                let ast = puml::parser::parse(d).map_err(|e| anyhow!("{e}"))?;
                Ok(match puml::scene::scene(&ast) {
                    Some(scene) => self.paint(&scene, false),
                    // Use case, timing, mind map and Gantt diagrams have no
                    // scene yet: puml's own SVG, in its light palette, serves
                    // both modes.
                    None => {
                        let svg = puml::render::render(&ast).to_string();
                        (svg.clone(), svg)
                    }
                })
            })
            .collect()
    }

    fn forge(&self, source: &str, args: &str) -> Result<Vec<(String, String)>> {
        let model = forge_dsl::parse(source, &self.base_dir).map_err(|e| anyhow!("{e}"))?;
        let all = forge_dsl::views(&model);
        let wanted: Vec<&str> = args.split_whitespace().collect();
        let keys: Vec<String> = if wanted.is_empty() {
            all.iter().map(|v| v.key.clone()).collect()
        } else {
            for w in &wanted {
                if !all.iter().any(|v| v.key == *w) {
                    bail!(
                        "no view {w:?} in the model (views: {})",
                        all.iter()
                            .map(|v| v.key.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            wanted.iter().map(|s| s.to_string()).collect()
        };
        if keys.is_empty() {
            bail!("the model defines no views to draw");
        }
        keys.iter()
            .map(|k| {
                let scene = forge_dsl::scene(&model, k, &self.fonts)
                    .with_context(|| format!("view {k:?} could not be laid out"))?;
                Ok(self.paint(&scene, true))
            })
            .collect()
    }
}

fn apply_palette(theme: &mut Theme, p: &crate::config::DiagramPalette) -> Result<()> {
    for (slot, value) in [
        (&mut theme.background, &p.background),
        (&mut theme.surface, &p.surface),
        (&mut theme.text, &p.text),
        (&mut theme.muted, &p.muted),
        (&mut theme.line, &p.line),
        (&mut theme.accent, &p.accent),
    ] {
        if let Some(hex) = value {
            *slot = parse_hex(hex)?;
        }
    }
    Ok(())
}

/// `#rrggbb` (or `#rgb`) to a diagram-scene colour.
fn parse_hex(s: &str) -> Result<diagram_scene::theme::Color> {
    let h = s
        .strip_prefix('#')
        .ok_or_else(|| anyhow!("colour {s:?} must start with #"))?;
    let h: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => bail!("colour {s:?} must be #rgb or #rrggbb"),
    };
    let byte = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| anyhow!("colour {s:?} isn't hex"))
    };
    Ok(diagram_scene::theme::Color(byte(0)?, byte(2)?, byte(4)?))
}

/// Prefix every `id="…"` in an SVG and every reference to it (`url(#…)`,
/// `href="#…"`, `xlink:href="#…"`), so several inline SVGs can share a page.
pub fn prefix_ids(svg: &str, prefix: &str) -> String {
    let mut ids = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find(" id=\"") {
        let after = &rest[i + 5..];
        if let Some(end) = after.find('"') {
            ids.push(after[..end].to_string());
        }
        rest = after;
    }
    if ids.is_empty() {
        return svg.to_string();
    }
    // Longest first, so `arrow-end` isn't rewritten by a rule for `arrow`.
    ids.sort_by_key(|id| std::cmp::Reverse(id.len()));
    ids.dedup();
    let mut out = svg.to_string();
    for id in &ids {
        for (from, to) in [
            (format!(" id=\"{id}\""), format!(" id=\"{prefix}{id}\"")),
            (format!("url(#{id})"), format!("url(#{prefix}{id})")),
            (format!("href=\"#{id}\""), format!("href=\"#{prefix}{id}\"")),
        ] {
            out = out.replace(&from, &to);
        }
    }
    out
}

/// CSS that shows the diagram copy matching the page's mode. The site sets
/// `data-mode` on `<html>` to `light`, `dark` or `auto`; `auto` (or no
/// attribute) follows the operating system.
pub fn css() -> &'static str {
    r#"/* ferrisite diagrams: show the copy matching the page's colour mode. */
.diagram { margin: 1.5rem auto; text-align: center; overflow-x: auto; }
.diagram svg { max-width: 100%; height: auto; }
.diagram-dark { display: none; }
html[data-mode="dark"] .diagram-light { display: none; }
html[data-mode="dark"] .diagram-dark { display: block; }
@media (prefers-color-scheme: dark) {
  html:not([data-mode="light"]) .diagram-light { display: none; }
  html:not([data-mode="light"]) .diagram-dark { display: block; }
}
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagrams() -> Diagrams {
        Diagrams::new(Path::new(".")).unwrap()
    }

    #[test]
    fn palette_overrides_reach_the_svg() {
        let light = crate::config::DiagramPalette {
            line: Some("#2b6cb0".into()),
            background: Some("#f2f5f7".into()),
            ..Default::default()
        };
        let dark = crate::config::DiagramPalette {
            line: Some("#7fb3ff".into()),
            ..Default::default()
        };
        let d = diagrams().with_palettes(&light, &dark).unwrap();
        let html = d
            .render(Lang::Mermaid, "flowchart LR\n  A --> B\n", "", "t")
            .unwrap();
        assert!(
            html.contains("#2b6cb0") && html.contains("#f2f5f7"),
            "{html}"
        );
        assert!(html.contains("#7fb3ff"), "{html}");
    }

    #[test]
    fn hex_colours() {
        assert_eq!(
            parse_hex("#abc").unwrap(),
            diagram_scene::theme::Color(0xaa, 0xbb, 0xcc)
        );
        assert_eq!(
            parse_hex("#1a1d24").unwrap(),
            diagram_scene::theme::Color(0x1a, 0x1d, 0x24)
        );
        assert!(parse_hex("1a1d24").is_err());
        assert!(parse_hex("#12345").is_err());
        assert!(parse_hex("#zzzzzz").is_err());
    }

    #[test]
    fn mermaid_renders_both_palettes_with_real_text() {
        let html = diagrams()
            .render(
                Lang::Mermaid,
                "flowchart LR\n  A[Start] --> B[End]\n",
                "",
                "d0",
            )
            .unwrap();
        assert!(html.starts_with("<div class=\"diagram diagram-mermaid\">"));
        assert_eq!(html.matches("<svg").count(), 2, "light and dark copies");
        assert!(html.contains("diagram-light") && html.contains("diagram-dark"));
        assert!(
            html.contains("Start") && html.contains("<text"),
            "labels are real text"
        );
    }

    #[test]
    fn plantuml_sequence_renders() {
        let html = diagrams()
            .render(Lang::PlantUml, "Alice -> Bob: hello\n", "", "d1")
            .unwrap();
        assert_eq!(html.matches("<svg").count(), 2);
        assert!(html.contains("Alice"));
    }

    #[test]
    fn unsupported_syntax_fails() {
        let err = diagrams()
            .render(Lang::Mermaid, "notADiagram\n  x --> y\n", "", "d2")
            .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn ids_are_prefixed_with_their_references() {
        let svg = r##"<svg><defs><marker id="arrow"/><marker id="arrow-end"/></defs><path marker-end="url(#arrow)"/><use href="#arrow-end"/></svg>"##;
        let out = prefix_ids(svg, "p-");
        assert!(out.contains(r#"id="p-arrow""#) && out.contains(r#"id="p-arrow-end""#));
        assert!(out.contains("url(#p-arrow)") && out.contains(r##"href="#p-arrow-end""##));
        assert!(!out.contains("p-p-"));
    }

    #[test]
    fn language_names() {
        assert_eq!(Lang::from_name("Mermaid"), Some(Lang::Mermaid));
        assert_eq!(Lang::from_name("puml"), Some(Lang::PlantUml));
        assert_eq!(Lang::from_name("rust"), None);
    }
}
