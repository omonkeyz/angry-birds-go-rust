//! Shared UI helpers: the textures every screen uses, sprite/solid draw builders, hit testing.
use crate::font::Font;
use crate::gfx::{Draw, Instance, SpriteRenderer, TextureId};
use std::path::Path;

/// Virtual stage every 2D screen is laid out on (16:9, same shape as the splash art).
pub const STAGE_W: f32 = 1920.0;
pub const STAGE_H: f32 = 1080.0;

/// Rectangle as [x, y, w, h].
pub type Rect = [f32; 4];

pub struct Tex {
    pub id: TextureId,
    pub w: f32,
    pub h: f32,
}

/// Every texture the 2D screens draw from, loaded once at start-up.
pub struct Textures {
    pub rovio: Tex,
    pub splash: Tex,
    pub landing: Tex,
    pub trackselect: Tex,
    pub ingame: Tex,
    pub white: TextureId,
    /// Heading font (large, outlined, drop shadow) and body font.
    pub title_font: Font,
    pub body_font: Font,
}

impl Textures {
    pub fn load(renderer: &mut SpriteRenderer, assets: &Path) -> Result<Self, String> {
        let mut load = |relative: &str| -> Result<Tex, String> {
            let (id, w, h) = renderer.load_png(&assets.join(relative))?;
            Ok(Tex { id, w: w as f32, h: h as f32 })
        };
        let rovio = load("textures/core/rovio_unc_alpha_16_1_of_1.png")?;
        let splash = load("textures/screens/splash_cmp_noalpha_1_of_1.png")?;
        let landing = load("textures/screens/landing_unc_alpha_16_1_of_1.png")?;
        let trackselect = load("textures/screens/trackselect_unc_alpha_16_1_of_1.png")?;
        let ingame = load("textures/screens/ingame_unc_alpha_16_1_of_1.png")?;
        let white = renderer.white_pixel();
        let title_font = Font::load(renderer, assets, "bold_outline_dropshadow_72")?;
        let body_font = Font::load(renderer, assets, "bold_32")?;
        Ok(Textures { rovio, splash, landing, trackselect, ingame, white, title_font, body_font })
    }
}

/// One atlas sprite (`src` = x, y, w, h in the texture) drawn into `dst`, faded by `alpha`.
pub fn sprite(tex: &Tex, src: Rect, dst: Rect, alpha: f32) -> Draw {
    tinted(tex, src, dst, [alpha, alpha, alpha, alpha])
}

/// Same, with a PREMULTIPLIED rgba tint (e.g. `[0, 0, 0, a]` draws the sprite as a black silhouette).
pub fn tinted(tex: &Tex, src: Rect, dst: Rect, tint: [f32; 4]) -> Draw {
    // half-texel inset keeps neighbouring atlas sprites from bleeding in
    let uv = [(src[0] + 0.5) / tex.w, (src[1] + 0.5) / tex.h, (src[0] + src[2] - 0.5) / tex.w, (src[1] + src[3] - 0.5) / tex.h];
    Draw { tex: tex.id, inst: Instance { rect: dst, uv, tint } }
}

/// A solid colour rectangle.
pub fn solid(white: TextureId, dst: Rect, rgb: [f32; 3], alpha: f32) -> Draw {
    let tint = [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha];
    Draw { tex: white, inst: Instance { rect: dst, uv: [0.5, 0.5, 0.5, 0.5], tint } }
}

pub fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3]
}

/// `r` scaled by `grow` around its own centre.
pub fn grown(r: Rect, grow: f32) -> Rect {
    let (w, h) = (r[2] * grow, r[3] * grow);
    [r[0] + (r[2] - w) / 2.0, r[1] + (r[3] - h) / 2.0, w, h]
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
