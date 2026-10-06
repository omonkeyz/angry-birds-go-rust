//! The launch sequence: Rovio logo -> splash art -> landing screen with the Play button.
//! Sprite rectangles come from the game's own `.atlas` files (see `abgtool atlas`, JSON copies in `assets/atlas`).
use crate::gfx::Draw;
use crate::ui::{contains, ease, sprite, solid, Rect, Textures, STAGE_H, STAGE_W};

// Source rectangles in pixels: [x, y, w, h].
const SPLASH_ART: Rect = [0.0, 2.0, 2048.0, 1150.0];
const ROVIO_LOGO: Rect = [2.0, 2.0, 512.0, 640.0];
const PLAY_BUTTON: Rect = [1349.0, 2.0, 390.0, 323.0];
const TOONS_BUTTON: Rect = [2.0, 1356.0, 378.0, 312.0];
const ICON_TWITTER: Rect = [1743.0, 2.0, 212.0, 212.0];
const ICON_FACEBOOK: Rect = [1731.0, 329.0, 212.0, 212.0];

// Timeline in seconds.
const ROVIO_IN: f32 = 0.5;
const ROVIO_HOLD: f32 = 2.2;
const ROVIO_OUT: f32 = 0.5;
const SPLASH_IN: f32 = 0.6;
const SPLASH_HOLD: f32 = 1.8;
const ROVIO_END: f32 = ROVIO_IN + ROVIO_HOLD + ROVIO_OUT;
pub const LANDING_START: f32 = ROVIO_END + SPLASH_IN + SPLASH_HOLD;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Play,
}

#[derive(Default)]
pub struct LaunchScreen {
    hover: bool,
    pressed: bool,
}

impl LaunchScreen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Play button rectangle on the stage (x, y, w, h), including the hover / press scale.
    pub fn play_rect(&self) -> Rect {
        let grow = if self.pressed { 0.94 } else if self.hover { 1.06 } else { 1.0 };
        let w = 330.0 * grow;
        let h = w * PLAY_BUTTON[3] / PLAY_BUTTON[2];
        [1410.0 - w / 2.0, 905.0 - h / 2.0, w, h]
    }

    pub fn landing_active(t: f32) -> bool {
        t >= LANDING_START
    }

    /// Returns true when the cursor should be a pointer.
    pub fn cursor_moved(&mut self, x: f32, y: f32, t: f32) -> bool {
        self.hover = Self::landing_active(t) && contains(self.play_rect(), x, y);
        self.hover
    }

    pub fn press(&mut self, x: f32, y: f32, t: f32) {
        self.pressed = Self::landing_active(t) && contains(self.play_rect(), x, y);
    }

    pub fn release(&mut self, x: f32, y: f32, t: f32) -> Option<Action> {
        let was_pressed = self.pressed;
        self.pressed = false;
        (was_pressed && Self::landing_active(t) && contains(self.play_rect(), x, y)).then_some(Action::Play)
    }

    /// Forces the hover state (used by the screenshot mode).
    pub fn set_hover(&mut self, hover: bool) {
        self.hover = hover;
    }

    /// Clears hover / press, e.g. when another screen takes over.
    pub fn reset_input(&mut self) {
        self.hover = false;
        self.pressed = false;
    }

    /// Everything to draw at time `t`, in stage coordinates.
    pub fn draws(&self, t: f32, tx: &Textures) -> Vec<Draw> {
        let mut out = Vec::new();
        let stage = [0.0, 0.0, STAGE_W, STAGE_H];
        if t < ROVIO_END {
            let alpha = if t < ROVIO_IN {
                ease(t / ROVIO_IN)
            } else if t > ROVIO_IN + ROVIO_HOLD {
                1.0 - ease((t - ROVIO_IN - ROVIO_HOLD) / ROVIO_OUT)
            } else {
                1.0
            };
            out.push(solid(tx.white, stage, [1.0, 1.0, 1.0], 1.0));
            let h = 520.0;
            let w = h * ROVIO_LOGO[2] / ROVIO_LOGO[3];
            out.push(sprite(&tx.rovio, ROVIO_LOGO, [(STAGE_W - w) / 2.0, (STAGE_H - h) / 2.0, w, h], alpha));
        } else if t < LANDING_START {
            let s = t - ROVIO_END;
            let alpha = if s < SPLASH_IN { ease(s / SPLASH_IN) } else { 1.0 };
            out.push(sprite(&tx.splash, SPLASH_ART, stage, alpha));
        } else {
            let fade = ease((t - LANDING_START) / 0.7);
            out.push(sprite(&tx.splash, SPLASH_ART, stage, 1.0));
            out.push(solid(tx.white, [0.0, STAGE_H - 330.0, STAGE_W, 330.0], [0.0, 0.0, 0.0], 0.18 * fade));

            let p = self.play_rect();
            // cheap drop shadow: the same sprite as a translucent black silhouette
            out.push(crate::ui::tinted(&tx.landing, PLAY_BUTTON, [p[0], p[1] + 10.0, p[2], p[3]], [0.0, 0.0, 0.0, 0.35 * fade]));
            out.push(sprite(&tx.landing, PLAY_BUTTON, p, fade));

            let toons_w = 190.0;
            let toons_h = toons_w * TOONS_BUTTON[3] / TOONS_BUTTON[2];
            out.push(sprite(&tx.landing, TOONS_BUTTON, [60.0, STAGE_H - 250.0, toons_w, toons_h], fade));
            out.push(sprite(&tx.landing, ICON_TWITTER, [STAGE_W - 250.0, STAGE_H - 140.0, 90.0, 90.0], fade));
            out.push(sprite(&tx.landing, ICON_FACEBOOK, [STAGE_W - 140.0, STAGE_H - 140.0, 90.0, 90.0], fade));
        }
        out
    }
}
