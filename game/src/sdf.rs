//! Signed-distance-field fonts of the 2.9.1 build (`*_sdf_32.fnt` + `_00.xgt` glyph sheet).
//!
//! `.fnt` (little endian): "XGSF", u32 version, u32 glyph count at 0x08, header 0x20 bytes, then
//! a 16-byte block (font size 32, line height 45, ascent 35, 32) and 24-byte glyph records:
//!   u32 code, u16 x, u16 y, u16 w, u16 h, u16 left bearing (1/64 px), u16 advance (1/64 px), u32 top bearing (px), u32 0
use crate::gfx::{Draw, SpriteRenderer};
use crate::ui::{tinted, Rect, Tex};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy)]
struct Glyph {
    rect: Rect,
    bearing_x: f32,
    advance: f32,
    bearing_y: f32,
}

pub struct SdfFont {
    tex: Tex,
    glyphs: HashMap<char, Glyph>,
    pub size: f32,
    pub line_height: f32,
    pub ascent: f32,
}

impl SdfFont {
    /// `name` is the file stem, e.g. `bold_ab_sdf_32`.
    pub fn load(renderer: &mut SpriteRenderer, root: &Path, name: &str) -> Result<SdfFont, String> {
        let path = root.join(format!("loc/{name}.fnt"));
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if data.len() < 0x30 || &data[0..4] != b"XGSF" {
            return Err(format!("{}: not an XGSF font", path.display()));
        }
        let u32_at = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
        let count = u32_at(8) as usize;
        let (size, line_height, ascent) = (u32_at(0x20) as f32, u32_at(0x24) as f32, u32_at(0x28) as f32);
        let mut glyphs = HashMap::with_capacity(count);
        for i in 0..count {
            let at = 0x30 + 24 * i;
            if at + 24 > data.len() {
                break;
            }
            let Some(ch) = char::from_u32(u32_at(at)) else { continue };
            glyphs.insert(
                ch,
                Glyph {
                    rect: [u16_at(at + 4) as f32, u16_at(at + 6) as f32, u16_at(at + 8) as f32, u16_at(at + 10) as f32],
                    bearing_x: u16_at(at + 12) as f32 / 64.0,
                    advance: u16_at(at + 14) as f32 / 64.0,
                    bearing_y: u32_at(at + 16) as f32,
                },
            );
        }
        let (id, w, h) = renderer.load_sdf_png(&root.join(format!("loc_png/{name}_00.png")))?;
        Ok(SdfFont { tex: Tex { id, w: w as f32, h: h as f32 }, glyphs, size, line_height, ascent })
    }

    fn advance(&self, ch: char) -> f32 {
        self.glyphs.get(&ch).map(|g| g.advance).unwrap_or(self.size * 0.3)
    }

    /// Width of `text` when the font's em is `em` pixels tall.
    pub fn width(&self, text: &str, em: f32) -> f32 {
        let s = em / self.size;
        text.chars().map(|c| self.advance(c)).sum::<f32>() * s
    }

    /// Draws `text` with the top-left of its line box at (x, y); `color` is straight rgba.
    pub fn draw(&self, text: &str, x: f32, y: f32, em: f32, color: [f32; 4], out: &mut Vec<Draw>) {
        let s = em / self.size;
        let tint = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
        let mut pen = x;
        for ch in text.chars() {
            if let Some(g) = self.glyphs.get(&ch) {
                if g.rect[2] > 0.0 && g.rect[3] > 0.0 {
                    let dst = [pen + g.bearing_x * s, y + (self.ascent - g.bearing_y) * s, g.rect[2] * s, g.rect[3] * s];
                    out.push(tinted(&self.tex, g.rect, dst, tint));
                }
            }
            pen += self.advance(ch) * s;
        }
    }
}
