//! Social cards (DESIGN.md section 10): a 1200×630 PNG per page, drawn as SVG
//! and rasterised with resvg. The layout follows the cards the Hugo site
//! generated: dark background, accent bar, the title wrapped with 72 px
//! margins, a divider, a round headshot, byline and site label.
//!
//! Text uses a bundled font (Libertinus Serif from typst-assets) and wrapping
//! is measured from its glyph advances, so output is byte-deterministic.

use crate::config::CardsConfig;
use anyhow::{Context, Result, anyhow};
use resvg::{tiny_skia, usvg};
use std::path::Path;
use std::sync::Arc;

const W: f32 = 1200.0;
const H: f32 = 630.0;
const MARGIN: f32 = 72.0;
const FAMILY: &str = "Libertinus Serif";

pub struct Cards {
    options: usvg::Options<'static>,
    measure_font: &'static [u8],
    image: Option<String>,
    cfg: CardsConfig,
}

/// Site-relative path of a page's card: `/post/x/` → `images/cards/post-x.png`.
pub fn card_path(url: &str) -> String {
    let url = url.strip_suffix(".html").unwrap_or(url);
    let slug = url.trim_matches('/').replace('/', "-");
    let slug = if slug.is_empty() {
        "home".to_string()
    } else {
        slug
    };
    format!("images/cards/{slug}.png")
}

impl Cards {
    pub fn new(cfg: &CardsConfig, root: &Path) -> Result<Cards> {
        let mut db = usvg::fontdb::Database::new();
        for font in typst_assets::fonts() {
            db.load_font_data(font.to_vec());
        }
        let measure_font = typst_assets::fonts()
            .find(|f| {
                ttf_parser::Face::parse(f, 0).is_ok_and(|face| {
                    face.names().into_iter().any(|n| {
                        n.name_id == ttf_parser::name_id::FULL_NAME
                            && n.to_string().as_deref() == Some("Libertinus Serif Regular")
                    })
                })
            })
            .context("bundled Libertinus Serif font not found")?;
        let image = match &cfg.image {
            Some(rel) => {
                let path = root.join(rel);
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("card image {}", path.display()))?;
                let mime = match path.extension().and_then(|e| e.to_str()) {
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    other => return Err(anyhow!("card image must be PNG or JPEG, not {other:?}")),
                };
                Some(format!("data:{mime};base64,{}", base64(&bytes)))
            }
            None => None,
        };
        let options = usvg::Options {
            fontdb: Arc::new(db),
            font_family: FAMILY.to_string(),
            ..usvg::Options::default()
        };
        Ok(Cards {
            options,
            measure_font,
            image,
            cfg: cfg.clone(),
        })
    }

    /// The card as PNG bytes. `byline` is drawn under the divider, e.g.
    /// "Graham Brooks · 22 September 2026".
    pub fn render(&self, title: &str, byline: &str) -> Result<Vec<u8>> {
        let svg = self.svg(title, byline);
        let tree = usvg::Tree::from_str(&svg, &self.options).context("card SVG")?;
        let mut pixmap = tiny_skia::Pixmap::new(W as u32, H as u32).context("card pixmap")?;
        resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
        pixmap.encode_png().context("card PNG")
    }

    pub fn svg(&self, title: &str, byline: &str) -> String {
        let c = &self.cfg;
        let chars = title.chars().count();
        let size = if chars > 90 {
            46.0
        } else if chars > 50 {
            56.0
        } else {
            64.0
        };
        let lines = self.wrap(title, size, W - 2.0 * MARGIN, 4);
        let line_height = size * 1.18;
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}"><rect width="{W}" height="{H}" fill="{bg}"/><rect width="{W}" height="10" fill="{accent}"/>"#,
            bg = c.background,
            accent = c.accent
        );
        svg.push_str(&format!(
            r#"<text font-family="{FAMILY}" font-size="{size}" font-weight="600" fill="{}">"#,
            c.text
        ));
        for (i, line) in lines.iter().enumerate() {
            let y = 80.0 + size * 0.9 + i as f32 * line_height;
            svg.push_str(&format!(
                r#"<tspan x="{MARGIN}" y="{y:.1}">{}</tspan>"#,
                xml(line)
            ));
        }
        svg.push_str("</text>");
        let divider_y = H - 72.0 - 96.0 - 36.0;
        svg.push_str(&format!(
            r#"<line x1="{MARGIN}" y1="{divider_y}" x2="{x2}" y2="{divider_y}" stroke="{}" stroke-width="2"/>"#,
            c.divider,
            x2 = W - MARGIN
        ));
        let text_x = if let Some(data) = &self.image {
            let (x, y, r) = (MARGIN, H - 72.0 - 96.0, 48.0);
            svg.push_str(&format!(
                r#"<clipPath id="head"><circle cx="{cx}" cy="{cy}" r="{r}"/></clipPath><image href="{data}" x="{x}" y="{y}" width="96" height="96" preserveAspectRatio="xMidYMid slice" clip-path="url(#head)"/>"#,
                cx = x + r,
                cy = y + r
            ));
            MARGIN + 120.0
        } else {
            MARGIN
        };
        svg.push_str(&format!(
            r#"<text x="{text_x}" y="{y}" font-family="{FAMILY}" font-size="30" fill="{}">{}</text>"#,
            c.text,
            xml(byline),
            y = H - 72.0 - 96.0 + 34.0
        ));
        if let Some(label) = &c.label {
            svg.push_str(&format!(
                r#"<text x="{text_x}" y="{y}" font-family="{FAMILY}" font-size="26" fill="{}">{}</text>"#,
                c.accent,
                xml(label),
                y = H - 72.0 - 96.0 + 76.0
            ));
        }
        svg.push_str("</svg>");
        svg
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        let Ok(face) = ttf_parser::Face::parse(self.measure_font, 0) else {
            return text.chars().count() as f32 * size * 0.5;
        };
        let scale = size / face.units_per_em() as f32;
        text.chars()
            .map(|ch| {
                face.glyph_index(ch)
                    .and_then(|g| face.glyph_hor_advance(g))
                    .unwrap_or(face.units_per_em() / 2) as f32
                    * scale
            })
            .sum()
    }

    /// Greedy word wrap to `max` px; more than `max_lines` lines end in "…".
    fn wrap(&self, text: &str, size: f32, max: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if self.width(&candidate, size) <= max || current.is_empty() {
                current = candidate;
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            let last = lines.last_mut().expect("at least one line");
            while !last.is_empty() && self.width(&format!("{last}…"), size) > max {
                last.pop();
            }
            last.push('…');
        }
        lines
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards() -> Cards {
        Cards::new(&CardsConfig::default(), Path::new(".")).unwrap()
    }

    #[test]
    fn card_paths() {
        assert_eq!(card_path("/"), "images/cards/home.png");
        assert_eq!(card_path("/post/x/"), "images/cards/post-x.png");
        assert_eq!(card_path("/404.html"), "images/cards/404.png");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn long_titles_wrap_within_margins_and_cap_lines() {
        let c = cards();
        let lines = c.wrap(&"word ".repeat(200), 46.0, W - 2.0 * MARGIN, 4);
        assert_eq!(lines.len(), 4);
        assert!(lines[3].ends_with('…'));
        for l in &lines {
            assert!(c.width(l, 46.0) <= W - 2.0 * MARGIN + 0.5, "{l}");
        }
    }

    #[test]
    fn renders_a_deterministic_png() {
        let c = cards();
        let a = c
            .render("A Title & More", "Graham Brooks · 1 November 2026")
            .unwrap();
        let b = c
            .render("A Title & More", "Graham Brooks · 1 November 2026")
            .unwrap();
        assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(a, b);
    }
}
