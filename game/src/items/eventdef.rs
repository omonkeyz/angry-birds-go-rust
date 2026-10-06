//! `<TrackItem>` parsing and expansion of an event definition, ported from
//! `CEventDefinitionManager::GetItemCount @001002bc` and `CEventDefinitionManager::AddTrackItem @001005b0`.
//!
//! The original reads each `<TrackItem>` into a 0x160-byte record (`TEventTrackItemData`), then copies it `itemcount` times
//! (`+0x40` = index inside the pattern, `+0x44` = count). `ItemRecord` is that record, with the fields the placement and the
//! item objects use. Attribute names are matched exactly like the original (misspelled attributes in the shipped xml, e.g.
//! `patternspaceing`, are ignored by the game, so they are ignored here too).
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pattern {
    /// default 0: single item
    #[default]
    None,
    /// 1 = "ring" (not used by any shipped event; UNRESOLVED: placement maths is garbled in the decompile)
    Ring,
    /// 2 = "stripe": along the spline
    Stripe,
    /// 3 = "bar": across the track
    Bar,
    /// 4 = "tower": upwards
    Tower,
}

/// Parameters of a `slalom_gate` item (`AddTrackItem`, defaults read from the code).
#[derive(Debug, Clone, Copy)]
pub struct SlalomParams {
    pub separation: f32,
    pub time_penalty: f32,
    pub cols: i32,
    pub rows: i32,
    pub col_spacing: f32,
    pub row_spacing: f32,
    /// the running gate counter at creation (`this+0x5ac`); even = blue posts, odd = red posts
    pub gate_index: i32,
}

/// One `<TrackItem>` element as read from the xml.
#[derive(Debug, Clone, Default)]
pub struct ItemDef {
    pub helper: String,
    pub spline: String,
    /// record +0x50: `fractionalongspline`, or `percentagealongspline`*0.01, or `distancealongspline` (metres, used when >= 1.0)
    pub fraction: f32,
    /// +0x54 `heightabovespline`
    pub height: f32,
    /// +0x60 `lateraloffsetfromspline` (or `percentageoffsetfromspline`*0.01): fraction of the half track width, + = right
    pub lateral: f32,
    /// +0x64 `lateraloffsetspread`
    pub spread: f32,
    pub pattern: Pattern,
    /// +0x4c `patternspacing`
    pub spacing: f32,
    /// +0x6c `position="x y z"`
    pub position: Option<Vec3>,
    pub item_count: Option<i32>,
    pub min_item_count: Option<i32>,
    pub max_item_count: Option<i32>,
    pub structure_type: Option<String>,
    pub structure_material: Option<String>,
    pub separation: Option<f32>,
    pub time_penalty: Option<f32>,
    pub cols: Option<i32>,
    pub rows: Option<i32>,
    pub col_spacing: Option<f32>,
    pub row_spacing: Option<f32>,
}

/// One expanded item record (`TEventTrackItemData`, 0x160 bytes in the original).
#[derive(Debug, Clone)]
pub struct ItemRecord {
    pub helper: String,
    pub spline: String,
    /// `+0x40`
    pub index: i32,
    /// `+0x44`
    pub count: i32,
    pub pattern: Pattern,
    pub spacing: f32,
    pub fraction: f32,
    pub height: f32,
    /// `+0x58`: local offset along the ground normal
    pub local_y: f32,
    /// `+0x5c`: local offset along the item's x axis (`normal x tangent`)
    pub local_x: f32,
    pub lateral: f32,
    pub spread: f32,
    /// `+0x68`: rotation about the item's x axis (written by `LayoutStructure`)
    pub pitch: f32,
    pub position: Option<Vec3>,
    /// which `<TrackItem>` (document order) this record came from
    pub def_index: usize,
    pub slalom: Option<SlalomParams>,
}

pub struct EventDef {
    /// `Environment pathname="environments\themeNNN\tracks\runNNN"`: (theme number, run number)
    pub environment: Option<(u32, u32)>,
    pub game_mode: String,
    /// `<Difficulty level="..">` (default 0.5, `this+0x248`): only used for random structure materials
    pub difficulty_level: f32,
    pub defs: Vec<ItemDef>,
}

/// C `strtod` on the leading number of the string (0.0 when there is none), narrowed to f32 like the original's `(float)` cast.
pub fn strtod(s: &str) -> f32 {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_exp = false;
    while end < b.len() {
        let c = b[end];
        match c {
            b'+' | b'-' if end == 0 || matches!(b[end - 1], b'e' | b'E') => {}
            b'0'..=b'9' => seen_digit = true,
            b'.' if !seen_dot && !seen_exp => seen_dot = true,
            b'e' | b'E' if seen_digit && !seen_exp => seen_exp = true,
            _ => break,
        }
        end += 1;
    }
    let mut t = &s[..end];
    while !t.is_empty() && t.parse::<f64>().is_err() {
        t = &t[..t.len() - 1];
    }
    t.parse::<f64>().unwrap_or(0.0) as f32
}

fn atoi(s: &str) -> i32 {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut end = 0;
    while end < b.len() && (b[end].is_ascii_digit() || (end == 0 && (b[end] == b'-' || b[end] == b'+'))) {
        end += 1;
    }
    s[..end].parse::<i32>().unwrap_or(0)
}

pub fn parse(xml: &str) -> Result<EventDef, String> {
    let mut def = EventDef { environment: None, game_mode: String::new(), difficulty_level: 0.5, defs: Vec::new() };
    for t in scan_tags(xml) {
        match t.name.as_str() {
            "Environment" => {
                // `environments\theme%d\tracks\run%d` (sscanf in ReadEventDefinitionFromXML)
                if let Some(p) = t.attribute("pathname") {
                    let p = p.replace('\\', "/");
                    let theme = p.split('/').find_map(|s| s.strip_prefix("theme")).map(atoi);
                    let run = p.split('/').find_map(|s| s.strip_prefix("run")).map(atoi);
                    if let (Some(th), Some(r)) = (theme, run) {
                        def.environment = Some((th as u32, r as u32));
                    }
                }
            }
            "Difficulty" => {
                if let Some(v) = t.attribute("level") {
                    def.difficulty_level = strtod(v);
                }
            }
            "GameMode" => def.game_mode = t.attribute("name").unwrap_or("").to_string(),
            "TrackItem" => def.defs.push(parse_item(&t)),
            _ => {}
        }
    }
    Ok(def)
}


/// Minimal tolerant tag scanner. The shipped event xml contains repeated attributes on one element (`spline` twice), which
/// strict parsers reject; the original reader returns the first occurrence, so does this.
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attribute(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

fn scan_tags(xml: &str) -> Vec<Tag> {
    let b = xml.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= b.len() || b[i] == b'/' || b[i] == b'!' || b[i] == b'?' {
            continue;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        let mut tag = Tag { name: xml[ns..i].to_string(), attrs: Vec::new() };
        loop {
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
                i += 1;
            }
            if i >= b.len() || b[i] == b'>' {
                i += 1;
                break;
            }
            let ks = i;
            while i < b.len() && b[i] != b'=' && b[i] != b'>' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let key = xml[ks..i].to_string();
            if i < b.len() && b[i] == b'=' {
                i += 1;
                if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                    let q = b[i];
                    i += 1;
                    let vs = i;
                    while i < b.len() && b[i] != q {
                        i += 1;
                    }
                    tag.attrs.push((key, xml[vs..i.min(b.len())].to_string()));
                    i += 1;
                }
            }
        }
        out.push(tag);
    }
    out
}
fn parse_item(n: &Tag) -> ItemDef {
    let a = |k: &str| n.attribute(k);
    let mut d = ItemDef::default();
    d.helper = a("helpername").unwrap_or("").to_string();
    d.spline = a("spline").unwrap_or("").to_string();
    // the three alternatives all write record +0x50 in this order, the later present attribute wins
    if let Some(v) = a("fractionalongspline") {
        d.fraction = strtod(v);
    }
    if let Some(v) = a("percentagealongspline") {
        d.fraction = strtod(v) * 0.01;
    }
    if let Some(v) = a("distancealongspline") {
        d.fraction = strtod(v);
    }
    if let Some(v) = a("lateraloffsetfromspline") {
        d.lateral = strtod(v);
    }
    if let Some(v) = a("percentageoffsetfromspline") {
        d.lateral = strtod(v) * 0.01;
    }
    if let Some(v) = a("heightabovespline") {
        d.height = strtod(v);
    }
    if let Some(v) = a("lateraloffsetspread") {
        d.spread = strtod(v);
    }
    if let Some(v) = a("patterntype") {
        d.pattern = if v.eq_ignore_ascii_case("ring") {
            Pattern::Ring
        } else if v.eq_ignore_ascii_case("stripe") {
            Pattern::Stripe
        } else if v.eq_ignore_ascii_case("bar") {
            Pattern::Bar
        } else if v.eq_ignore_ascii_case("tower") {
            Pattern::Tower
        } else {
            Pattern::None
        };
    }
    if let Some(v) = a("patternspacing") {
        d.spacing = strtod(v);
    }
    if let Some(v) = a("position") {
        let f: Vec<f32> = v.split_whitespace().map(strtod).collect();
        if f.len() >= 3 {
            d.position = Some(Vec3::new(f[0], f[1], f[2]));
        }
    }
    d.item_count = a("itemcount").map(atoi);
    d.min_item_count = a("minitemcount").map(atoi);
    d.max_item_count = a("maxitemcount").map(atoi);
    d.structure_type = a("structuretype").map(str::to_string);
    d.structure_material = a("structurematerial").map(str::to_string);
    d.separation = a("separation").map(strtod);
    d.time_penalty = a("time_penalty").map(strtod);
    d.cols = a("cols").map(atoi);
    d.rows = a("rows").map(atoi);
    d.col_spacing = a("col_spacing").map(strtod);
    d.row_spacing = a("row_spacing").map(strtod);
    d
}

/// `CEventDefinitionManager::GetItemCount(node, 0) @001002bc` (children are not counted: no shipped event nests `<TrackItem>`s).
pub fn item_count(d: &ItemDef) -> i32 {
    if d.helper.eq_ignore_ascii_case("slalom_gate") {
        return 2;
    }
    if let Some(s) = &d.structure_type {
        return super::structures::structure_item_count(s);
    }
    if d.min_item_count.is_some() && d.max_item_count.is_some() {
        return d.max_item_count.unwrap();
    }
    if let Some(c) = d.item_count {
        return c;
    }
    1
}

/// Expands every `<TrackItem>` into its records (`AddTrackItem`), including slalom gate posts and structure layouts.
pub fn expand(defs: &[ItemDef], difficulty_level: f32) -> Vec<ItemRecord> {
    let mut out: Vec<ItemRecord> = Vec::new();
    let mut gate_counter = 0i32; // CEventDefinitionManager+0x5ac
    for (di, d) in defs.iter().enumerate() {
        let count = item_count(d);
        if count == 0 {
            continue; // AddTrackItem returns when GetItemCount is 0
        }
        let template = ItemRecord {
            helper: d.helper.clone(),
            spline: d.spline.clone(),
            index: 0,
            count,
            pattern: d.pattern,
            spacing: d.spacing,
            fraction: d.fraction,
            height: d.height,
            local_y: 0.0,
            local_x: 0.0,
            lateral: d.lateral,
            spread: d.spread,
            pitch: 0.0,
            position: d.position,
            def_index: di,
            slalom: None,
        };
        let base = out.len();
        for i in 0..count.max(0) {
            let mut r = template.clone();
            r.index = i;
            out.push(r);
        }
        if d.helper.eq_ignore_ascii_case("slalom_gate") {
            let sep = d.separation.unwrap_or(8.0);
            let params = SlalomParams {
                separation: sep,
                time_penalty: d.time_penalty.unwrap_or(5.0),
                cols: d.cols.unwrap_or(3),
                rows: d.rows.unwrap_or(3),
                col_spacing: d.col_spacing.unwrap_or(10.0),
                row_spacing: d.row_spacing.unwrap_or(10.0),
                gate_index: gate_counter,
            };
            let colour = if gate_counter & 1 == 0 { "blue" } else { "red" };
            out[base].helper = format!("slalom_post_{colour}_left");
            out[base + 1].helper = format!("slalom_post_{colour}_right");
            out[base].local_x -= sep * 0.5;
            out[base + 1].local_x += sep * 0.5;
            out[base].slalom = Some(params);
            out[base + 1].slalom = Some(params);
            gate_counter += 1;
        }
        if let Some(st) = &d.structure_type {
            super::structures::layout_structure(&mut out[base..], st, d.structure_material.as_deref(), difficulty_level, di as u32);
        }
    }
    out
}
