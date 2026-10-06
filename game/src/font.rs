//! Bitmap fonts: the game's `.fnt` metrics (XGSF) + the matching white-on-alpha glyph sheet.
//!
//! `.fnt` layout (little endian), reverse-engineered from the v1.0.1 APK:
//!   0x00  "XGSF"
//!   0x04  u32 glyph sheet width, 0x08 u32 glyph count, 0x0C u32 page count
//!   0x14  glyph records, 12 bytes each: u16 0, u16 code point, u16 x, u16 y, u16 w, u16 h
//! Glyphs are white with an alpha mask, so any colour can be applied as a tint.
use crate::gfx::{Draw, SpriteRenderer};
use crate::ui::{tinted, Rect, Tex};
use std::collections::HashMap;
use std::path::Path;

pub struct Font {
    tex: Tex,
    glyphs: HashMap<char, Rect>,
    pub line_height: f32,
}

impl Font {
    /// `name` is the file stem, e.g. `bold_32` -> `pak/localisation/bold_32.fnt` + `textures_pak/localisation/bold_32_00.png`.
    pub fn load(renderer: &mut SpriteRenderer, assets: &Path, name: &str) -> Result<Font, String> {
        let metrics_path = assets.join(format!("pak/localisation/{name}.fnt"));
        let data = std::fs::read(&metrics_path).map_err(|e| format!("{}: {e}", metrics_path.display()))?;
        if data.len() < 0x14 || &data[0..4] != b"XGSF" {
            return Err(format!("{}: not an XGSF font", metrics_path.display()));
        }
        let count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let mut glyphs = HashMap::with_capacity(count);
        let mut line_height = 0.0f32;
        for i in 0..count {
            let at = 0x14 + 12 * i;
            let Some(rec) = data.get(at..at + 12) else { break };
            let field = |k: usize| u16::from_le_bytes([rec[k * 2], rec[k * 2 + 1]]) as f32;
            let code = field(1) as u32;
            let rect = [field(2), field(3), field(4), field(5)];
            line_height = line_height.max(rect[3]);
            if let Some(ch) = char::from_u32(code) {
                glyphs.insert(ch, rect);
            }
        }
        let (id, w, h) = renderer.load_png(&assets.join(format!("textures_pak/localisation/{name}_00.png")))?;
        Ok(Font { tex: Tex { id, w: w as f32, h: h as f32 }, glyphs, line_height })
    }

    fn advance(&self, ch: char, scale: f32) -> f32 {
        match self.glyphs.get(&ch) {
            Some(g) if g[2] > 0.0 => (g[2] + 1.0) * scale,
            _ => self.line_height * 0.3 * scale,
        }
    }

    pub fn width(&self, text: &str, scale: f32) -> f32 {
        text.chars().map(|c| self.advance(c, scale)).sum()
    }

    /// Draws `text` with its top-left corner at (x, y); `color` is straight rgba.
    pub fn draw(&self, text: &str, x: f32, y: f32, scale: f32, color: [f32; 4], out: &mut Vec<Draw>) {
        let tint = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
        let mut pen = x;
        for ch in text.chars() {
            if let Some(g) = self.glyphs.get(&ch) {
                if g[2] > 0.0 {
                    out.push(tinted(&self.tex, *g, [pen, y, g[2] * scale, g[3] * scale], tint));
                }
            }
            pen += self.advance(ch, scale);
        }
    }

    /// Draws `text` horizontally centred on `cx`, vertically centred on `cy`.
    pub fn draw_centered(&self, text: &str, cx: f32, cy: f32, scale: f32, color: [f32; 4], out: &mut Vec<Draw>) {
        let w = self.width(text, scale);
        self.draw(text, cx - w / 2.0, cy - self.line_height * scale / 2.0, scale, color, out);
    }
}
