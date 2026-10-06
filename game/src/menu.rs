//! Event select: the screen Play leads to. Art from `trackselect_unc_alpha_16` (rectangles from its `.atlas`).
use crate::gfx::Draw;
use crate::ui::{contains, ease, grown, solid, sprite, Rect, Textures, STAGE_H, STAGE_W};

const SPLASH_ART: Rect = [0.0, 2.0, 2048.0, 1150.0];
const TITLE_BOARD: Rect = [1071.0, 2.0, 760.0, 144.0];
const EVENT_RACE: Rect = [2.0, 1560.0, 480.0, 440.0];
const EVENT_VS: Rect = [1064.0, 1002.0, 480.0, 440.0];
const EVENT_TIME_ATTACK: Rect = [2.0, 1116.0, 480.0, 440.0];
const EVENT_FRUIT: Rect = [1071.0, 502.0, 480.0, 440.0];
const BUTTON_FRAME: Rect = [486.0, 1116.0, 480.0, 440.0];
const BUTTON_ON: Rect = [1555.0, 502.0, 480.0, 440.0];
const LOCK_ICON: Rect = [1835.0, 2.0, 118.0, 134.0];
const BACK_ARROW: Rect = [1228.0, 1786.0, 133.0, 126.0];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Race,
    Versus,
    TimeAttack,
    Fruit,
}

impl Event {
    pub const ALL: [Event; 4] = [Event::Race, Event::Versus, Event::TimeAttack, Event::Fruit];

    fn art(self) -> Rect {
        match self {
            Event::Race => EVENT_RACE,
            Event::Versus => EVENT_VS,
            Event::TimeAttack => EVENT_TIME_ATTACK,
            Event::Fruit => EVENT_FRUIT,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Event::Race => "RACE",
            Event::Versus => "VERSUS",
            Event::TimeAttack => "TIME ATTACK",
            Event::Fruit => "FRUIT RUSH",
        }
    }

    /// Only the plain race is playable so far.
    pub fn playable(self) -> bool {
        self == Event::Race
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Back,
    Start(Event),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Back,
    Tile(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackAction {
    Back,
    Start(usize),
}

/// Track select: one round button per circuit, labelled with the track name and lap count.
#[derive(Default)]
pub struct TrackSelect {
    hover: Option<Hit>,
    pressed: Option<Hit>,
}

fn track_tile(index: usize) -> Rect {
    let (w, h, gap) = (384.0, 352.0, 70.0);
    let total = 3.0 * w + 2.0 * gap;
    [(STAGE_W - total) / 2.0 + index as f32 * (w + gap), 330.0, w, h]
}

impl TrackSelect {
    pub fn new() -> Self {
        Self::default()
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if contains(back_rect(), x, y) {
            return Some(Hit::Back);
        }
        (0..crate::track::TRACKS.len()).find(|&i| contains(track_tile(i), x, y)).map(Hit::Tile)
    }

    pub fn cursor_moved(&mut self, x: f32, y: f32) -> bool {
        self.hover = self.hit(x, y);
        self.hover.is_some()
    }

    pub fn press(&mut self, x: f32, y: f32) {
        self.pressed = self.hit(x, y);
    }

    pub fn release(&mut self, x: f32, y: f32) -> Option<TrackAction> {
        let pressed = self.pressed.take();
        let released = self.hit(x, y);
        if pressed.is_none() || pressed != released {
            return None;
        }
        match released? {
            Hit::Back => Some(TrackAction::Back),
            Hit::Tile(i) => Some(TrackAction::Start(i)),
        }
    }

    pub fn reset_input(&mut self) {
        self.hover = None;
        self.pressed = None;
    }

    pub fn draws(&self, t: f32, tx: &Textures) -> Vec<Draw> {
        let fade = ease(t / 0.35);
        let mut out = Vec::new();
        let stage = [0.0, 0.0, STAGE_W, STAGE_H];
        out.push(sprite(&tx.splash, SPLASH_ART, stage, 1.0));
        out.push(solid(tx.white, stage, [0.07, 0.09, 0.14], 0.9));
        out.push(solid(tx.white, [0.0, 0.0, STAGE_W, 190.0], [0.0, 0.0, 0.0], 0.25));
        let title_w = 684.0;
        let title_h = title_w * TITLE_BOARD[3] / TITLE_BOARD[2];
        out.push(sprite(&tx.trackselect, TITLE_BOARD, [(STAGE_W - title_w) / 2.0, 36.0, title_w, title_h], fade));
        tx.title_font.draw_centered("SELECT TRACK", STAGE_W / 2.0, 36.0 + title_h / 2.0, 0.9, [0.25, 0.16, 0.08, fade], &mut out);

        for (i, def) in crate::track::TRACKS.iter().enumerate() {
            let hovered = self.hover == Some(Hit::Tile(i));
            let pressed = self.pressed == Some(Hit::Tile(i));
            let grow = if pressed { 0.95 } else if hovered { 1.06 } else { 1.0 };
            let rect = grown(track_tile(i), grow);
            out.push(sprite(&tx.trackselect, if hovered { BUTTON_ON } else { BUTTON_FRAME }, rect, fade));
            // a small swatch of the track's own colours inside the round button
            let c = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
            out.push(solid(tx.white, [c[0] - 70.0, c[1] - 66.0, 140.0, 62.0], def.sky, 0.95 * fade));
            out.push(solid(tx.white, [c[0] - 70.0, c[1] - 4.0, 140.0, 40.0], def.grass, 0.95 * fade));
            out.push(solid(tx.white, [c[0] - 70.0, c[1] + 36.0, 140.0, 22.0], def.road, 0.95 * fade));
            tx.title_font.draw_centered(&format!("{}", i + 1), c[0], c[1] - 18.0, 1.0, [1.0, 1.0, 1.0, fade], &mut out);
            tx.body_font.draw_centered(def.name, c[0], rect[1] + rect[3] + 40.0, 1.0, [1.0, 1.0, 1.0, fade], &mut out);
            tx.body_font.draw_centered(&format!("{} LAPS", def.laps), c[0], rect[1] + rect[3] + 82.0, 0.8, [1.0, 0.85, 0.35, fade], &mut out);
        }
        let back_grow = if self.pressed == Some(Hit::Back) { 0.92 } else if self.hover == Some(Hit::Back) { 1.1 } else { 1.0 };
        out.push(sprite(&tx.trackselect, BACK_ARROW, grown(back_rect(), back_grow), fade));
        out
    }
}

#[derive(Default)]
pub struct EventSelect {
    hover: Option<Hit>,
    pressed: Option<Hit>,
}

fn tile_rect(index: usize) -> Rect {
    let (w, h, gap) = (384.0, 352.0, 40.0);
    let total = 4.0 * w + 3.0 * gap;
    [(STAGE_W - total) / 2.0 + index as f32 * (w + gap), 380.0, w, h]
}

fn back_rect() -> Rect {
    [40.0, 36.0, 106.0, 100.0]
}

impl EventSelect {
    pub fn new() -> Self {
        Self::default()
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if contains(back_rect(), x, y) {
            return Some(Hit::Back);
        }
        (0..Event::ALL.len()).find(|&i| contains(tile_rect(i), x, y)).map(Hit::Tile)
    }

    /// Returns true when the cursor should be a pointer.
    pub fn cursor_moved(&mut self, x: f32, y: f32) -> bool {
        self.hover = self.hit(x, y);
        match self.hover {
            Some(Hit::Back) => true,
            Some(Hit::Tile(i)) => Event::ALL[i].playable(),
            None => false,
        }
    }

    pub fn press(&mut self, x: f32, y: f32) {
        self.pressed = self.hit(x, y);
    }

    pub fn release(&mut self, x: f32, y: f32) -> Option<Action> {
        let pressed = self.pressed.take();
        let released = self.hit(x, y);
        if pressed.is_none() || pressed != released {
            return None;
        }
        match released? {
            Hit::Back => Some(Action::Back),
            Hit::Tile(i) if Event::ALL[i].playable() => Some(Action::Start(Event::ALL[i])),
            Hit::Tile(_) => None,
        }
    }

    pub fn reset_input(&mut self) {
        self.hover = None;
        self.pressed = None;
    }

    /// `t` = seconds since this screen opened.
    pub fn draws(&self, t: f32, tx: &Textures) -> Vec<Draw> {
        let fade = ease(t / 0.35);
        let mut out = Vec::new();
        let stage = [0.0, 0.0, STAGE_W, STAGE_H];
        // calm backdrop: the splash art almost fully dimmed, so the buttons stay readable
        out.push(sprite(&tx.splash, SPLASH_ART, stage, 1.0));
        out.push(solid(tx.white, stage, [0.07, 0.09, 0.14], 0.9));
        out.push(solid(tx.white, [0.0, 0.0, STAGE_W, 190.0], [0.0, 0.0, 0.0], 0.25));

        let title_w = 684.0;
        let title_h = title_w * TITLE_BOARD[3] / TITLE_BOARD[2];
        out.push(sprite(&tx.trackselect, TITLE_BOARD, [(STAGE_W - title_w) / 2.0, 36.0, title_w, title_h], fade));

        for (i, event) in Event::ALL.iter().enumerate() {
            let hovered = self.hover == Some(Hit::Tile(i));
            let pressed = self.pressed == Some(Hit::Tile(i));
            let grow = if !event.playable() { 1.0 } else if pressed { 0.95 } else if hovered { 1.06 } else { 1.0 };
            let rect = grown(tile_rect(i), grow);
            if event.playable() {
                out.push(sprite(&tx.trackselect, if hovered { BUTTON_ON } else { BUTTON_FRAME }, rect, fade));
                out.push(sprite(&tx.trackselect, event.art(), rect, fade));
            } else {
                out.push(sprite(&tx.trackselect, BUTTON_FRAME, rect, 0.4 * fade));
                out.push(sprite(&tx.trackselect, event.art(), rect, 0.4 * fade));
                let lock = [rect[0] + rect[2] / 2.0 - 45.0, rect[1] + rect[3] / 2.0 - 52.0, 90.0, 102.0];
                out.push(sprite(&tx.trackselect, LOCK_ICON, lock, fade));
            }
        }

        // text: heading on the title board, a label under every tile
        let title_cy = 36.0 + title_h / 2.0;
        tx.title_font.draw_centered("SELECT EVENT", STAGE_W / 2.0, title_cy, 0.9, [0.25, 0.16, 0.08, fade], &mut out);
        for (i, event) in Event::ALL.iter().enumerate() {
            let r = tile_rect(i);
            let (label, alpha) = if event.playable() { (event.label(), fade) } else { (event.label(), 0.5 * fade) };
            tx.body_font.draw_centered(label, r[0] + r[2] / 2.0, r[1] + r[3] + 42.0, 1.15, [1.0, 1.0, 1.0, alpha], &mut out);
        }

        let back_grow = if self.pressed == Some(Hit::Back) { 0.92 } else if self.hover == Some(Hit::Back) { 1.1 } else { 1.0 };
        out.push(sprite(&tx.trackselect, BACK_ARROW, grown(back_rect(), back_grow), fade));
        out
    }
}
