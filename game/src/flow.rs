//! Screen flow of the front end, driven by the global-state names the 2.9.1 layouts emit
//! (`onClickedGlobalState` / `onTappedGlobalState`).
use crate::gfx::{Draw, SpriteRenderer};
use crate::ui::contains;
use crate::uix::{Hit, UiSystem};
use crate::ui::{Rect, STAGE_H, STAGE_W};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

const SCREENS: [&str; 7] = ["uilandingscreen", "uimapscreen", "uitopbar", "uisettingsscreen", "uishopscreen", "uikartgaragescreen", "uikartselectscreen"];

#[derive(Clone, Debug, PartialEq)]
pub enum FlowEvent {
    None,
    StartDrive,
    StartEvent(usize),
}

pub struct UiFlow {
    ui: UiSystem,
    /// Screens on the stack, bottom first; each entry is the layers drawn together (e.g. map + top bar).
    stack: Vec<Vec<&'static str>>,
    hits: RefCell<Vec<Hit>>,
    pressed: Option<String>,
    split: std::cell::Cell<usize>,
    pub notice: Option<String>,
    /// Profile, economy and progression (assets292 definitions + the save file).
    pub meta: Option<crate::meta::Meta>,
    /// Campaign map: chapters (title key, campaign indices of their events) and the page shown.
    chapters: Vec<(String, Vec<usize>)>,
    page: usize,
}

impl UiFlow {
    pub fn new(renderer: &mut SpriteRenderer, root: &Path) -> Result<UiFlow, String> {
        let mut ui = UiSystem::new(renderer, root)?;
        for name in SCREENS {
            if let Err(e) = ui.load_screen(renderer, name) {
                eprintln!("ui: could not load {name}: {e}");
            }
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let meta = match root.parent().map(|p| p.join("assets292")).map(|r| crate::meta::Meta::open(&r, Some(crate::meta::default_save_path()), now)) {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => {
                eprintln!("meta game unavailable: {e}");
                None
            }
            None => None,
        };
        let chapters = load_chapters(&root.join("xml/xml/global/campaignmapdefinition.xml"));
        Ok(UiFlow { ui, stack: Vec::new(), hits: RefCell::new(Vec::new()), pressed: None, split: std::cell::Cell::new(usize::MAX), notice: None, meta, chapters, page: 0 })
    }

    pub fn open_landing(&mut self) {
        self.stack = vec![vec!["uilandingscreen"]];
        self.pressed = None;
    }

    pub fn open_map(&mut self) {
        self.stack = vec![vec!["uimapscreen", "uitopbar"]];
        self.pressed = None;
    }

    pub fn screen(&self) -> &'static str {
        self.stack.last().and_then(|l| l.first()).copied().unwrap_or("")
    }

    /// Esc / back: pops one screen; false when already at the bottom.
    pub fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    fn show_list(layer: &str) -> &'static [&'static str] {
        // windows the game code reveals although their layout starts them hidden
        match layer {
            "uilandingscreen" => &["Container"],
            "uimapscreen" => &["MapLayoutWindow"],
            _ => &[],
        }
    }

    pub fn draws(&self) -> Vec<Draw> {
        let mut out = Vec::new();
        let mut all_hits = Vec::new();
        let mut split = None;
        let top = self.stack.len().saturating_sub(1);
        for (depth, layers) in self.stack.iter().enumerate() {
            for layer in layers {
                let mut hits = Vec::new();
                let mut texts = HashMap::new();
                if *layer == "uitopbar" {
                    if let Some(m) = &self.meta {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                        let t = m.topbar(now);
                        texts.insert("CoinLabel".to_string(), t.coins.to_string());
                        texts.insert("GemLabel".to_string(), t.gems.to_string());
                        texts.insert("EnergyLabel".to_string(), format!("{}/{}", t.energy, t.energy_max));
                        texts.insert("RankLabel".to_string(), t.rank.to_string());
                        texts.insert("TokenLabel".to_string(), t.tickets.to_string());
                    }
                }
                if *layer == "uimapscreen" {
                    texts.insert("PageNumber".to_string(), format!("{}/{}", self.page + 1, self.chapters.len().max(1)));
                    if let Some((title, _)) = self.chapters.get(self.page) {
                        texts.insert("Title".to_string(), self.ui.string(title));
                    }
                }
                let at = out.len();
                if let Some(s) = self.ui.draw(layer, Self::show_list(layer), &texts, &mut out, &mut hits) {
                    split.get_or_insert(s);
                    let _ = at;
                }
                if *layer == "uimapscreen" {
                    self.campaign_markers(&mut out, &mut hits);
                }
                if depth == top {
                    all_hits.extend(hits);
                }
            }
        }
        *self.hits.borrow_mut() = all_hits;
        self.split.set(split.unwrap_or(usize::MAX));
        out
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<(String, Option<String>)> {
        self.hits
            .borrow()
            .iter()
            .rev()
            .find(|h| contains(h.rect, x, y) && h.action != "showTooltip")
            .map(|h| (h.action.clone(), h.sound.clone()))
    }

    pub fn pointer(&self, x: f32, y: f32) -> bool {
        self.hit_at(x, y).is_some()
    }

    pub fn press(&mut self, x: f32, y: f32) {
        self.pressed = self.hit_at(x, y).map(|h| h.0);
    }

    /// Returns the sound to play and what the window layer should do.
    pub fn release(&mut self, x: f32, y: f32) -> (Option<String>, FlowEvent) {
        let target = self.hit_at(x, y);
        let pressed = self.pressed.take();
        let Some((action, sound)) = target else { return (None, FlowEvent::None) };
        if pressed.as_deref() != Some(action.as_str()) {
            return (None, FlowEvent::None);
        }
        let event = self.act(&action);
        (sound, event)
    }

    fn push(&mut self, layers: &[&'static str]) {
        self.stack.push(layers.to_vec());
    }

    fn act(&mut self, action: &str) -> FlowEvent {
        match action {
            "LandingScreen_NewUser" | "ExistingUser" => self.open_map(),
            "settings" => self.push(&["uisettingsscreen"]),
            "shopScreen" | "gacha" | "coinShop" => self.push(&["uishopscreen"]),
            "kartGarage" => self.push(&["uikartgaragescreen", "uitopbar"]),
            "charSelect" => self.push(&["uikartselectscreen", "uitopbar"]),
            "NextCampaignPage" => self.page = (self.page + 1) % self.chapters.len().max(1),
            "PreviousCampaignPage" => self.page = (self.page + self.chapters.len().max(1) - 1) % self.chapters.len().max(1),
            "CloseSettingsScreen" | "CloseShopScreen" | "TapOutsideWindow" | "topbarBackButton" => {
                self.back();
            }
            // the campaign map: choosing an event starts the race; the tracks are not in either APK, so the baseplate stands in
            a if a.starts_with("CampaignMarkerSelected:") => return FlowEvent::StartEvent(a["CampaignMarkerSelected:".len()..].parse().unwrap_or(0)),
            "CampaignMapPressed" => {}
            other => self.notice = Some(format!("{other}: NOT AVAILABLE OFFLINE")),
        }
        FlowEvent::None
    }
}

/// `campaignmapdefinition.xml`: each chapter is a row of nine map tiles; tiles carrying an `EventMarker` are race events.
fn load_chapters(path: &Path) -> Vec<(String, Vec<usize>)> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(doc) = roxmltree::Document::parse(&text) else { return Vec::new() };
    doc.root_element()
        .children()
        .filter(|c| c.has_tag_name("Chapter"))
        .map(|c| {
            let title = c.attribute("title").unwrap_or("").to_string();
            let events = c
                .descendants()
                .filter(|e| e.has_tag_name("EventMarker"))
                .filter_map(|e| e.attribute("campaignIndex")?.parse().ok())
                .collect();
            (title, events)
        })
        .collect()
}

impl UiFlow {
    /// The events of the current chapter on the island map: one icon per `EventMarker` along the chapter's tile row, three stars below.
    fn campaign_markers(&self, out: &mut Vec<Draw>, hits: &mut Vec<Hit>) {
        let Some((_, events)) = self.chapters.get(self.page) else { return };
        let u = STAGE_H / 100.0;
        // CWindow_MapLayoutWindow: left 5pt, 55% down the 85pt map window that starts 8pt from the top, 57.5% wide
        let layout: Rect = [5.0 * u, 8.0 * u + 0.55 * 85.0 * u - 0.15 * 85.0 * u, 0.575 * STAGE_W, 0.30 * 85.0 * u];
        let cell = layout[2] / 9.0;
        let icon = 11.0 * u;
        const THEMES: [&str; 5] = ["seedway", "rockyroad", "air", "stunt", "subzero"];
        for (slot, &index) in events.iter().enumerate() {
            let cx = layout[0] + (slot as f32 * 2.0 + 0.5) * cell;
            let cy = layout[1] + layout[3] / 2.0;
            let rect = [cx - icon / 2.0, cy - icon / 2.0, icon, icon];
            let info = self.meta.as_ref().and_then(|m| m.map_event(index));
            let unlocked = info.as_ref().map(|e| e.unlocked).unwrap_or(true);
            let stars = info.as_ref().map(|e| e.stars).unwrap_or(0);
            let alpha = if unlocked { 1.0 } else { 0.45 };
            let theme = THEMES[(index / 11) % 5];
            if let Some(d) = self.ui.sprite(&format!("UIMapScreen/ABK_map_{theme}.png"), rect, alpha) {
                out.push(d);
            }
            if let Some(d) = self.ui.sprite("UIMapScreen/ABK_mapinner_race.png", [rect[0] + icon * 0.2, rect[1] + icon * 0.2, icon * 0.6, icon * 0.6], alpha) {
                out.push(d);
            }
            for s in 0..3 {
                let star = icon / 3.0;
                let r = [cx - icon / 2.0 + s as f32 * star, cy + icon * 0.5, star, star];
                let star_name = if s < stars { "shared:UIShared/ABK_StarOn.png" } else { "shared:UIShared/ABK_StarOff.png" };
                if let Some(d) = self.ui.sprite(star_name, r, alpha) {
                    out.push(d);
                }
            }
            if !unlocked {
                continue;
            }
            hits.push(Hit { rect, action: format!("CampaignMarkerSelected:{index}"), sound: Some("ABY_ui_forward".to_string()), id: format!("event{index}") });
        }
    }
}

impl UiFlow {
    /// Screenshot mode: jump to a screen by its global-state name (`settings`, `shopScreen`, `kartGarage`, ...).
    pub fn goto(&mut self, state: &str) {
        match state {
            "landing" => self.open_landing(),
            "map" => self.open_map(),
            other => {
                self.open_map();
                self.act(other);
            }
        }
    }
}

impl UiFlow {
    /// Index of the first draw that goes over a 3D view (the kart in the garage), `usize::MAX` when the screen has none.
    pub fn split(&self) -> usize {
        self.split.get()
    }
}
