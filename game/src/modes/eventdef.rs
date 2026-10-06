//! Common eventdef header shared by every game mode: `<Name>`, `<Environment>`, `<Character>`, `<GameMode>`,
//! `<Difficulty>`, `<Stars>`, `<DifficultyAdjust>`, `<Spline>` (AI weighting).
//!
//! Ported from `CEventDefinitionManager::ReadEventDefinitionFromXML @ 00101478` (header part; the `<TrackItem>` part is
//! `items/eventdef.rs`), `CEventDefinitionManager::StringToGameMode @ 000fa1bc` (+ clone `.part.23 @ 000a3f90`) and
//! `CEventDefinitionManager::StringToFruitRushMode @ 000fa350`. Field offsets quote the manager object (`this+0x...`).
//!
//! The shipped xml is not always well formed (duplicate attributes, several `<EventDefinition>` concatenated in one file),
//! so this uses a small tolerant tag scanner instead of roxmltree. The first occurrence of an attribute wins; only the
//! first `<EventDefinition>` of a file is read. Attribute names are matched exactly like the original (`<Stars 1= 2= 3=>`
//! in the versus defs is therefore ignored => all stars 0, exactly what the game reads).
#![allow(dead_code)]

use super::types::GameModeKind;

/// Raw game-mode id stored at `manager+0x240` (`StringToGameMode`). Ids 3 and 15 are unused / "unknown string".
pub mod mode_id {
    pub const INTRO: u32 = 0;
    pub const INTRO2: u32 = 1;
    pub const INTRO3: u32 = 2;
    pub const RACE: u32 = 4;
    pub const VERSUS: u32 = 5;
    pub const TIME_ATTACK: u32 = 6;
    pub const SEED_RUSH: u32 = 7;
    /// "QMR" = quick multiplayer race
    pub const QMR: u32 = 8;
    /// "TMR"
    pub const TMR: u32 = 9;
    pub const JENGA: u32 = 10;
    pub const BOSS_BATTLE: u32 = 11;
    pub const BOSS_FRUIT_RUSH: u32 = 12;
    pub const SLALOM: u32 = 13;
    /// "LMR" = local multiplayer race
    pub const LMR: u32 = 14;
    /// any string not matched (the final `else` of `StringToGameMode .part.23`)
    pub const UNKNOWN: u32 = 15;
}

/// `CEventDefinitionManager::StringToGameMode @ 000fa1bc` (and `.part.23 @ 000a3f90` for TMR/LMR/JENGA/unknown).
pub fn string_to_game_mode(s: &str) -> u32 {
    match s {
        "TIME_ATTACK" => mode_id::TIME_ATTACK,
        "VERSUS" => mode_id::VERSUS,
        "RACE" => mode_id::RACE,
        "INTRO" => mode_id::INTRO,
        "INTRO2" => mode_id::INTRO2,
        "INTRO3" => mode_id::INTRO3,
        "SEED_RUSH" => mode_id::SEED_RUSH,
        "BOSS_BATTLE" => mode_id::BOSS_BATTLE,
        "BOSS_FRUIT_RUSH" => mode_id::BOSS_FRUIT_RUSH,
        "SLALOM" => mode_id::SLALOM,
        "QMR" => mode_id::QMR,
        "TMR" => mode_id::TMR,
        "LMR" => mode_id::LMR,
        "JENGA" => mode_id::JENGA,
        _ => mode_id::UNKNOWN,
    }
}

/// Map the original mode id onto the shared [`GameModeKind`] (QMR/TMR/LMR/unknown => `Other`; use [`EventHeader::mode_id`]).
pub fn kind_from_mode_id(id: u32) -> GameModeKind {
    match id {
        mode_id::RACE => GameModeKind::Race,
        mode_id::TIME_ATTACK => GameModeKind::TimeAttack,
        mode_id::SEED_RUSH => GameModeKind::SeedRush,
        mode_id::BOSS_BATTLE => GameModeKind::BossBattle,
        mode_id::BOSS_FRUIT_RUSH => GameModeKind::BossFruitRush,
        mode_id::VERSUS => GameModeKind::Versus,
        mode_id::SLALOM => GameModeKind::Slalom,
        mode_id::JENGA => GameModeKind::Jenga,
        mode_id::INTRO | mode_id::INTRO2 | mode_id::INTRO3 => GameModeKind::Intro,
        _ => GameModeKind::Other,
    }
}

/// `fruit_rush_mode=` (`manager+0x244`; `StringToFruitRushMode @ 000fa350`): "FRUIT" = 0 (default), "ICECREAM" = 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FruitRushMode {
    #[default]
    Fruit,
    IceCream,
}

/// The five difficulty tiers (`DifficultyAdjust` children, `timer_*`, `seed_amount_*` suffixes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyTier {
    VeryEasy,
    Easy,
    Medium,
    Hard,
    Impossible,
}

impl DifficultyTier {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// `<Spline name= min_ai_weighting= max_ai_weighting=>` entries (`manager+0x28c + i*0xc`).
#[derive(Clone, Debug, PartialEq)]
pub struct SplineWeight {
    pub name: String,
    pub min_ai_weighting: f32,
    pub max_ai_weighting: f32,
}

/// The header of an event definition (everything except the `<TrackItem>`s).
#[derive(Clone, Debug, PartialEq)]
pub struct EventHeader {
    /// `<Name tag=>` (`+0x68`, atoi-less: the original copies the 4 chars raw as an int; kept as the string)
    pub tag: String,
    /// `<Name title=>` (`+0x70`, 0x40 bytes) / `description` (`+0xb0`, 0x100 bytes)
    pub title: String,
    pub description: String,
    /// `<Environment pathname=>` (`+0x1b0`) and `config=` (`+0x1f8`)
    pub environment: String,
    pub env_config: i32,
    /// `sscanf("environments\\theme%d\\tracks\\run%d")` -> `+0x1f0` theme, `+0x1f4` run
    pub theme: i32,
    pub run: i32,
    /// `<Character name=>` (`+0x1fc`)
    pub character: String,
    /// `<GameMode name=>` string (`+0x21c`) and its id (`+0x240`)
    pub mode_name: String,
    pub mode_id: u32,
    /// `kartcount=` (`+0x23c`), -1 when absent
    pub kartcount: i32,
    /// `fruit_rush_mode=` (`+0x244`)
    pub fruit_rush_mode: FruitRushMode,
    /// `<Difficulty level=>` (`+0x248`, default 0.5)
    pub level: f32,
    /// `ai_upgrade=` (`+0x24c`, default -1)
    pub ai_upgrade: i32,
    /// `timer_veryeasy..impossible` (`+0x250..0x260`), defaults 60, 47.5, 35, 22.5, 10 (the constants stored before the read)
    pub timer: [f32; 5],
    /// `bosslevel=` (`+0x264`, default 1)
    pub boss_level: i32,
    /// `seed_amount_veryeasy..impossible` (`+0x268..0x278`), defaults 60, 47, 35, 22, 10
    pub seed_amount: [i32; 5],
    /// `level_index=` (`+0x288`, default 0)
    pub level_index: i32,
    /// `<Stars Star1= Star2= Star3=>` (`+0x27c/0x280/0x284`) after the monotonic fix-up (see [`fix_stars`])
    pub stars: [i32; 3],
    /// `<DifficultyAdjust><VeryEasy/><Easy/><Medium/><Hard/><Impossible/>` (`+0x590..0x5a0`). When the node is absent the
    /// original keeps `CDebugManager::GetDebugFloat(0xa9..0xad)`; UNRESOLVED: those debug floats are not extracted, 0.0 here.
    pub difficulty_adjust: [f32; 5],
    pub has_difficulty_adjust: bool,
    /// `<Spline>` AI weightings
    pub splines: Vec<SplineWeight>,
}

impl Default for EventHeader {
    fn default() -> Self {
        EventHeader {
            tag: String::new(),
            title: String::new(),
            description: String::new(),
            environment: String::new(),
            env_config: 0,
            theme: 0,
            run: 0,
            character: String::new(),
            mode_name: String::new(),
            mode_id: mode_id::INTRO,
            kartcount: -1,
            fruit_rush_mode: FruitRushMode::Fruit,
            level: 0.5,
            ai_upgrade: -1,
            // 0x42700000, 0x423e0000, 0x420c0000, 0x41b40000, 0x41200000
            timer: [60.0, 47.5, 35.0, 22.5, 10.0],
            boss_level: 1,
            // 0x3c, 0x2f, 0x23, 0x16, 10
            seed_amount: [60, 47, 35, 22, 10],
            level_index: 0,
            stars: [0, 0, 0],
            difficulty_adjust: [0.0; 5],
            has_difficulty_adjust: false,
            splines: Vec::new(),
        }
    }
}

/// Star fix-up at the end of the `<Stars>` read in `ReadEventDefinitionFromXML @ 00101478`: unless all three are 0,
/// `Star2 = max(Star2, Star1)` and `Star3 = max(Star3, Star2)`.
pub fn fix_stars(mut s: [i32; 3]) -> [i32; 3] {
    if s[0] == 0 && s[1] == 0 && s[2] == 0 {
        return s;
    }
    if s[1] < s[0] {
        s[1] = s[0];
    }
    if s[2] < s[1] {
        s[2] = s[1];
    }
    s
}

/// C `atoi` (leading whitespace, optional sign, digits; 0 when none).
pub fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let mut v: i64 = 0;
    for c in t.chars() {
        if let Some(d) = c.to_digit(10) {
            v = v * 10 + d as i64;
            if v > i32::MAX as i64 + 1 {
                break;
            }
        } else {
            break;
        }
    }
    (if neg { -v } else { v }) as i32
}

/// C `strtod` prefix parse narrowed to f32 (0.0 when none).
pub fn strtod(s: &str) -> f32 {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_exp = false;
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = false;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            digits = true;
            end = i + 1;
        } else if c == b'.' && !seen_dot && !seen_exp {
            seen_dot = true;
            if digits {
                end = i + 1;
            }
        } else if (c == b'e' || c == b'E') && digits && !seen_exp {
            seen_exp = true;
            if i + 1 < b.len() && (b[i + 1] == b'+' || b[i + 1] == b'-') {
                i += 1;
            }
        } else {
            break;
        }
        i += 1;
    }
    t[..end].parse::<f64>().unwrap_or(0.0) as f32
}

/// One scanned start tag.
struct Tag<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, &'a str)>,
    /// byte offset just after the `>`
    end: usize,
}

impl<'a> Tag<'a> {
    /// first occurrence wins (matches `GetAttribute` returning the first match)
    fn attr(&self, n: &str) -> Option<&'a str> {
        self.attrs.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
    }
}

fn next_tag(src: &str, from: usize) -> Option<Tag<'_>> {
    let b = src.as_bytes();
    let mut i = from;
    loop {
        while i < b.len() && b[i] != b'<' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if i + 1 < b.len() && (b[i + 1] == b'/' || b[i + 1] == b'?' || b[i + 1] == b'!') {
            // closing tag / pi / comment: skip to '>'
            while i < b.len() && b[i] != b'>' {
                i += 1;
            }
            continue;
        }
        break;
    }
    let start = i + 1;
    let mut j = start;
    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
        j += 1;
    }
    let name = &src[start..j];
    let mut attrs = Vec::new();
    loop {
        while j < b.len() && (b[j].is_ascii_whitespace() || b[j] == b'/') {
            j += 1;
        }
        if j >= b.len() {
            return None;
        }
        if b[j] == b'>' {
            j += 1;
            break;
        }
        let ks = j;
        while j < b.len() && b[j] != b'=' && b[j] != b'>' && !b[j].is_ascii_whitespace() {
            j += 1;
        }
        let key = &src[ks..j];
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                j += 1;
                let vs = j;
                while j < b.len() && b[j] != q {
                    j += 1;
                }
                attrs.push((key, &src[vs..j.min(b.len())]));
                j += 1;
            }
        } else if j < b.len() && b[j] != b'>' && key.is_empty() {
            j += 1;
        }
    }
    Some(Tag { name, attrs, end: j })
}

impl EventHeader {
    /// Parse the header of the first `<EventDefinition>` in `xml`. Returns `None` when there is none.
    ///
    /// Ported from the header part of `CEventDefinitionManager::ReadEventDefinitionFromXML @ 00101478` (defaults are the
    /// values the function stores before each attribute read).
    pub fn from_xml(xml: &str) -> Option<EventHeader> {
        let mut h = EventHeader::default();
        let mut i = 0;
        let mut in_def = false;
        let mut in_adjust = false;
        let mut seen_def = false;
        while let Some(t) = next_tag(xml, i) {
            let tag_start_end = t.end;
            match t.name {
                "EventDefinition" => {
                    if seen_def {
                        break; // concatenated second definition
                    }
                    seen_def = true;
                    in_def = true;
                }
                _ if !in_def => {}
                "Name" => {
                    if let Some(v) = t.attr("tag") {
                        h.tag = v.to_string();
                    }
                    if let Some(v) = t.attr("title") {
                        h.title = v.chars().take(0x40).collect();
                    }
                    if let Some(v) = t.attr("description") {
                        h.description = v.chars().take(0x100).collect();
                    }
                }
                "Environment" => {
                    if let Some(v) = t.attr("pathname") {
                        h.environment = v.chars().take(0x40).collect();
                    }
                    if let Some(v) = t.attr("config") {
                        h.env_config = atoi(v);
                    }
                    // sscanf("environments\\theme%d\\tracks\\run%d")
                    let (th, ru) = scan_env(&h.environment);
                    h.theme = th;
                    h.run = ru;
                }
                "Character" => {
                    if let Some(v) = t.attr("name") {
                        h.character = v.chars().take(0x20).collect();
                    }
                }
                "GameMode" => {
                    if let Some(v) = t.attr("name") {
                        h.mode_name = v.chars().take(0x20).collect();
                    }
                    h.mode_id = string_to_game_mode(&h.mode_name);
                    if let Some(v) = t.attr("kartcount") {
                        h.kartcount = atoi(v);
                    }
                    h.fruit_rush_mode = FruitRushMode::Fruit;
                    if let Some(v) = t.attr("fruit_rush_mode") {
                        if v == "ICECREAM" {
                            h.fruit_rush_mode = FruitRushMode::IceCream;
                        }
                    }
                }
                "Difficulty" => {
                    if let Some(v) = t.attr("level") {
                        h.level = strtod(v);
                    }
                    if let Some(v) = t.attr("ai_upgrade") {
                        h.ai_upgrade = atoi(v);
                    }
                    for (k, n) in ["timer_veryeasy", "timer_easy", "timer_medium", "timer_hard", "timer_impossible"].iter().enumerate() {
                        if let Some(v) = t.attr(n) {
                            h.timer[k] = strtod(v);
                        }
                    }
                    if let Some(v) = t.attr("bosslevel") {
                        h.boss_level = atoi(v);
                    }
                    for (k, n) in [
                        "seed_amount_veryeasy",
                        "seed_amount_easy",
                        "seed_amount_medium",
                        "seed_amount_hard",
                        "seed_amount_impossible",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if let Some(v) = t.attr(n) {
                            h.seed_amount[k] = atoi(v);
                        }
                    }
                    if let Some(v) = t.attr("level_index") {
                        h.level_index = atoi(v);
                    }
                }
                "Spline" => {
                    h.splines.push(SplineWeight {
                        name: t.attr("name").unwrap_or("").to_string(),
                        min_ai_weighting: t.attr("min_ai_weighting").map(strtod).unwrap_or(0.0),
                        max_ai_weighting: t.attr("max_ai_weighting").map(strtod).unwrap_or(0.0),
                    });
                }
                "DifficultyAdjust" => {
                    in_adjust = true;
                    h.has_difficulty_adjust = true;
                }
                "VeryEasy" | "Easy" | "Medium" | "Hard" | "Impossible" if in_adjust => {
                    let k = match t.name {
                        "VeryEasy" => 0,
                        "Easy" => 1,
                        "Medium" => 2,
                        "Hard" => 3,
                        _ => 4,
                    };
                    // element text up to the next '<'
                    let rest = &xml[tag_start_end..];
                    let txt = rest.split('<').next().unwrap_or("");
                    h.difficulty_adjust[k] = strtod(txt);
                }
                "Stars" => {
                    let mut s = [0i32; 3];
                    for (k, n) in ["Star1", "Star2", "Star3"].iter().enumerate() {
                        if let Some(v) = t.attr(n) {
                            s[k] = atoi(v);
                        }
                    }
                    h.stars = fix_stars(s);
                }
                _ => {}
            }
            i = tag_start_end;
        }
        if seen_def {
            Some(h)
        } else {
            None
        }
    }

    /// Shared kind (QMR/TMR/LMR/unknown => `GameModeKind::Other`).
    pub fn kind(&self) -> GameModeKind {
        kind_from_mode_id(self.mode_id)
    }

    /// `seed_amount_<tier>` / `timer_<tier>` helpers.
    pub fn timer_for(&self, t: DifficultyTier) -> f32 {
        self.timer[t.index()]
    }
    pub fn seed_amount_for(&self, t: DifficultyTier) -> i32 {
        self.seed_amount[t.index()]
    }
    pub fn difficulty_adjust_for(&self, t: DifficultyTier) -> f32 {
        self.difficulty_adjust[t.index()]
    }
}

/// `sscanf(path, "environments\\theme%d\\tracks\\run%d", &theme, &run)`; unmatched values stay 0 (fields were zero-init'ed).
fn scan_env(p: &str) -> (i32, i32) {
    let mut th = 0;
    let mut ru = 0;
    if let Some(r) = p.strip_prefix("environments\\theme") {
        let n = r.chars().take_while(|c| c.is_ascii_digit()).count();
        th = atoi(&r[..n]);
        if let Some(r2) = r[n..].strip_prefix("\\tracks\\run") {
            let m = r2.chars().take_while(|c| c.is_ascii_digit()).count();
            ru = atoi(&r2[..m]);
        }
    }
    (th, ru)
}

/// Read the header of an eventdef file below [`super::types::ASSETS292`] (`xml/gameplay/<rel>`), `None` if missing.
pub fn load_header(rel: &str) -> Option<EventHeader> {
    let p = format!("{}/xml/gameplay/{}", super::types::ASSETS292, rel);
    let s = std::fs::read_to_string(p).ok()?;
    EventHeader::from_xml(&s)
}

/// All `eventdef_*.xml` files (relative to `xml/gameplay`) in the shipped data; tests + tooling.
pub fn list_eventdefs() -> Vec<String> {
    let root = format!("{}/xml/gameplay", super::types::ASSETS292);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for d in rd.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            if !name.starts_with("eventdef_") {
                continue;
            }
            if let Ok(fr) = std::fs::read_dir(d.path()) {
                for f in fr.flatten() {
                    let fname = f.file_name().to_string_lossy().to_string();
                    if fname.ends_with(".xml") {
                        out.push(format!("{}/{}", name, fname));
                    }
                }
            }
        }
    }
    out.sort();
    out
}

// ------------------------------------------------------------------------------------------------------------
// Shared race-mode core: per-car mode data, score counters, star criteria, difficulty tier.
// Every mode that is a "race with a different rule set" (RACE, VERSUS, TIME_ATTACK, SEED_RUSH, SLALOM, BOSS ...) shares this.
// ------------------------------------------------------------------------------------------------------------

/// `CGameModeData+4` (per-car mode state, `piVar13[1]` in `CGameMode::ProcessUpdate @ 0017caac`).
/// 1 = running; 2 / 3 = completed (set on finish: `GetEventCompleted ? 2 : 3`, forced to 3 when `game+0x2cc == -1`);
/// 5 / 6 = the multiplayer "force finish" values handled at the top of the non-running branch of `ProcessUpdate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarModeState {
    Running,
    Completed,
    Failed,
    /// UNRESOLVED: only seen as `state - 5 < 2` in `ProcessUpdate` (multiplayer path), never written by the three modes of this port.
    ForcedWin,
    ForcedLose,
}

impl CarModeState {
    /// The integer stored at `CGameModeData+4`.
    pub fn raw(self) -> u32 {
        match self {
            CarModeState::Running => 1,
            CarModeState::Completed => 2,
            CarModeState::Failed => 3,
            CarModeState::ForcedWin => 5,
            CarModeState::ForcedLose => 6,
        }
    }
}

/// Base per-car mode data, `CGameModeData` (ctor `@ 0017ca38`, `Reset @ 0017c3a8`, `Update @ 0017c450`).
/// Layout: +4 state, +8 elapsed, +0xc final position, +0x10 current position, +0x14 best position, +0x18 bonus coins, +0x1c.
#[derive(Clone, Debug, PartialEq)]
pub struct CarModeData {
    pub state: CarModeState,
    /// `+8`: seconds the car has been racing (`state == 1` and `car+0x1b7c > 0`)
    pub elapsed: f32,
    /// `+0xc`: finishing position (0 until the local player finishes / the car finishes)
    pub final_position: i32,
    /// `+0x10`: current race position (copy of `car+0x1a9c`)
    pub position: i32,
    /// `+0x14`: best (lowest) position seen, starts at 1000
    pub best_position: i32,
    /// `+0x18`: `GetBonusCoins(car)` at finish
    pub bonus_coins: i32,
    /// `+0x1c`
    pub unknown_1c: i32,
    /// `GetEventCompleted(car)` evaluated at the finish (not stored in the original; the state 2/3 split plus the
    /// `game+0x2cc == -1` quirk loses it, so it is kept here).
    pub won: bool,
}

impl Default for CarModeData {
    /// `CGameModeData::Reset @ 0017c3a8`
    fn default() -> Self {
        CarModeData {
            state: CarModeState::Running,
            elapsed: 0.0,
            final_position: 0,
            position: 0,
            best_position: 1000,
            bonus_coins: 0,
            unknown_1c: 0,
            won: false,
        }
    }
}

impl CarModeData {
    /// `CGameModeData::Update(CCar*, float) @ 0017c450` (the debug "full repair" branch is a debug bool, not ported).
    /// `car_position` = `car+0x1a9c`; `clock_positive` = `car+0x1b7c > 0`.
    pub fn update_base(&mut self, car_position: i32, clock_positive: bool, dt: f32) {
        self.position = car_position;
        if car_position < self.best_position {
            self.best_position = car_position;
        }
        if self.state == CarModeState::Running && clock_positive {
            self.elapsed += dt;
        }
    }
}

/// `CGameMode::CheckGameOverCondition` overrides (RACE `@ 00184f08`, SLALOM `@ 001852f4`, SEED_RUSH `@ 0017e940`,
/// TIME_ATTACK `@ 001867dc`, INTRO `@ 0017f4cc`; all identical): the game is over when every local player is done.
#[derive(Clone, Copy, Debug)]
pub struct LocalPlayerStatus {
    /// `car+0x4d4 != 0` (UNRESOLVED: field meaning not identified; checked first)
    pub car_4d4_set: bool,
    /// `car+0x1b60 != 0` (finish line crossed)
    pub finish_line_crossed: bool,
    /// `CGameModeData+4 == 1`
    pub mode_state_running: bool,
    /// `car+0x1ae8 != 0`
    pub car_1ae8_set: bool,
}

/// The shared `CheckGameOverCondition` body. `locals` = the local players (at most 4, the original unrolls 4).
pub fn check_game_over_condition(locals: &[LocalPlayerStatus]) -> bool {
    let n = locals.len().min(4);
    if n == 0 {
        return true;
    }
    let mut done = 0usize;
    for l in &locals[..n] {
        let d = l.car_4d4_set || (!l.finish_line_crossed && !l.mode_state_running) || !l.car_1ae8_set;
        if d {
            done += 1;
        }
    }
    n <= done
}

/// `CMetagameManager::GetRaceMaxScore(int) @ 0012df04` (`CScoreSystem+0x10`, set by `CScoreSystem::SetRaceCC @ 001f4258`).
/// `cc` is the event CC (`campaignCC` of the `CampaignEvent`, `CGame+0x314`).
pub fn race_max_score(cc: i32) -> i32 {
    cc * 0x96
}

/// `relativeCC` of the five tiers (`economy.xml <DifficultyAdjust>`, `CMetagameManager+0x4708..0x4718`): 4, 0, -2, -4, -8.
pub const RELATIVE_CC_DEFAULT: [i32; 5] = [4, 0, -2, -4, -8];

/// Read the five `relativeCC` values from `economy.xml` text (document order = tier order).
pub fn relative_cc_from_economy(xml: &str) -> Option<[i32; 5]> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let da = doc.descendants().find(|n| n.has_tag_name("DifficultyAdjust"))?;
    let mut out = RELATIVE_CC_DEFAULT;
    let mut i = 0;
    for c in da.children().filter(|n| n.has_tag_name("Difficulty")) {
        if i < 5 {
            if let Some(v) = c.attribute("relativeCC") {
                out[i] = atoi(v);
            }
        }
        i += 1;
    }
    Some(out)
}

/// `CMetagameManager::GetDifficultyAdjust(int eventCC, int kartCC) @ 0012df90`, called by `CGame::CalcDifficultyAdjustEnum @ 0011b800`.
/// The last branch of the original returns 4 on both paths; that is kept.
pub fn get_difficulty_adjust(event_cc: i32, kart_cc: i32, rel: &[i32; 5]) -> DifficultyTier {
    if event_cc <= kart_cc - rel[0] {
        return DifficultyTier::VeryEasy;
    }
    if event_cc <= kart_cc - rel[1] {
        return DifficultyTier::Easy;
    }
    if event_cc <= kart_cc - rel[2] {
        return DifficultyTier::Medium;
    }
    if kart_cc - rel[3] < event_cc {
        // if (kart_cc - rel[4] < event_cc) return 4; return 4;
        return DifficultyTier::Impossible;
    }
    DifficultyTier::Hard
}

/// `<ScoreCounterFinishingFruit>` of `xml/global/scoreconfig.xml` (`CScoreCounterFinishingFruit::LoadProperties @ 001f19e4`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FruitCounterCfg {
    /// `+0x3fc` PercentForMinScore
    pub percent_for_min: f32,
    /// `+0x400` PercentForMaxScore
    pub percent_for_max: f32,
    /// `+0x404` MinFruitScore
    pub min_score: f32,
    /// `+0x408` MaxFruitScore
    pub max_score: f32,
}

impl Default for FruitCounterCfg {
    /// ctor `@ 001f1b74`: 0, 1.0, 0, 1.0 (the shipped xml overrides min score with 0.125)
    fn default() -> Self {
        FruitCounterCfg { percent_for_min: 0.0, percent_for_max: 1.0, min_score: 0.0, max_score: 1.0 }
    }
}

/// `<ScoreCounterFinishingTime>` (`CScoreCounterFinishingTime::LoadProperties @ 001f2110`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeCounterCfg {
    /// `+0x3fc` PercentOverForMaxScore
    pub percent_over_for_max: f32,
    /// `+0x400` PercentOverForMinScore
    pub percent_over_for_min: f32,
    /// `+0x404` MaxTimeScore
    pub max_score: f32,
    /// `+0x408` MinTimeScore
    pub min_score: f32,
}

impl Default for TimeCounterCfg {
    /// UNRESOLVED: the ctor `@ 001f22xx` defaults are loaded from literal-pool pairs that were not decoded
    /// (only `+0x3fc = 15.0` and `+0x404 = 1.0` are visible); the shipped xml always supplies all four, so those are used.
    fn default() -> Self {
        TimeCounterCfg { percent_over_for_max: 0.0, percent_over_for_min: 0.35, max_score: 1.0, min_score: 0.125 }
    }
}

/// The finishing-counter parameters of `scoreconfig.xml` this port needs.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScoreConfig {
    pub fruit: FruitCounterCfg,
    pub time: TimeCounterCfg,
}

impl ScoreConfig {
    /// Parse `xml/global/scoreconfig.xml` (`CScoreSystem::Init @ 001f49cc`, element texts are `strtod`'d with leading spaces).
    pub fn from_xml(xml: &str) -> Option<ScoreConfig> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let mut cfg = ScoreConfig::default();
        let text = |n: roxmltree::Node, name: &str| -> Option<f32> {
            n.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(strtod)
        };
        for n in doc.descendants() {
            if n.has_tag_name("ScoreCounterFinishingFruit") {
                if let Some(v) = text(n, "PercentForMinScore") {
                    cfg.fruit.percent_for_min = v;
                }
                if let Some(v) = text(n, "PercentForMaxScore") {
                    cfg.fruit.percent_for_max = v;
                }
                if let Some(v) = text(n, "MinFruitScore") {
                    cfg.fruit.min_score = v;
                }
                if let Some(v) = text(n, "MaxFruitScore") {
                    cfg.fruit.max_score = v;
                }
            } else if n.has_tag_name("ScoreCounterFinishingTime") {
                if let Some(v) = text(n, "PercentOverForMaxScore") {
                    cfg.time.percent_over_for_max = v;
                }
                if let Some(v) = text(n, "PercentOverForMinScore") {
                    cfg.time.percent_over_for_min = v;
                }
                if let Some(v) = text(n, "MaxTimeScore") {
                    cfg.time.max_score = v;
                }
                if let Some(v) = text(n, "MinTimeScore") {
                    cfg.time.min_score = v;
                }
            }
        }
        Some(cfg)
    }

    /// Load from the shipped assets (`assets292/xml/xml/global/scoreconfig.xml`).
    pub fn load() -> Option<ScoreConfig> {
        let s = std::fs::read_to_string(format!("{}/xml/xml/global/scoreconfig.xml", super::types::ASSETS292)).ok()?;
        ScoreConfig::from_xml(&s)
    }
}

/// `CScoreCounterFinishingPosition::SetPosition(int) @ 001f1d48`: `(int)((9 - pos) / 8.0 * raceMaxScore)`.
/// (`VectorSignedFixedToFloat(9 - pos, 0x20, 3)` = value / 2^3.)
pub fn finishing_position_score(position: i32, race_cc: i32) -> i32 {
    ((9 - position) as f32 / 8.0 * race_max_score(race_cc) as f32) as i32
}

/// `CScoreCounterFinishingFruit::SetFruitPercent(float) @ 001f1bcc` (only in mode 7, only the first call).
/// `frac = clamp((percent - pctMin) / (pctMax - pctMin), 0, 1)`; `max = raceMax`;
/// score = `(int)(max*MinFruit + frac * (max*MaxFruit - max*MinFruit))`.
pub fn finishing_fruit_score(percent: f32, race_cc: i32, cfg: &FruitCounterCfg) -> i32 {
    let max = race_max_score(race_cc) as f32;
    let mut f = (percent - cfg.percent_for_min) / (cfg.percent_for_max - cfg.percent_for_min);
    if f < 0.0 {
        f = 0.0; // DAT_001f1c90 = 0.0
    } else if f > 1.0 {
        f = 1.0;
    }
    (max * cfg.min_score + f * (max * cfg.max_score - max * cfg.min_score)) as i32
}

/// `CScoreCounterFinishingTime::SetTime() @ 001f230c` (only in mode 6, only the first call).
/// `over = -timeLeft / duration` (`data+0x24`, `data+0x20`); `t = clamp(over, pctOverForMax, pctOverForMin)`;
/// score = `max*MaxTimeScore + (t / pctOverForMin) * (max*MinTimeScore - max*MaxTimeScore)`.
// UNRESOLVED: the decompile prints the last term as `(fVar8 - fVar8)` (SSA artefact: both operands collapsed). The
// reconstruction pairs `MaxTimeScore (+0x404)` with `MinTimeScore (+0x408)` by analogy with the fruit counter and
// divides by `PercentOverForMinScore` exactly like the visible `fVar7 / fVar6`; with the shipped values
// (PercentOverForMax = 0) `(t - lo) / (hi - lo)` and `t / hi` are identical.
pub fn finishing_time_score(time_left: f32, duration: f32, race_cc: i32, cfg: &TimeCounterCfg) -> i32 {
    let max = race_max_score(race_cc) as f32;
    let over = -time_left / duration;
    let t = over.max(cfg.percent_over_for_max).min(cfg.percent_over_for_min);
    let hi = max * cfg.max_score;
    let lo = max * cfg.min_score;
    (hi + (t / cfg.percent_over_for_min) * (lo - hi)) as i32
}

/// `CScoreSystem::GetBonusScore() @ 001f43bc` rounding: `((x + 99) / 100) * 100` (C integer division).
pub fn round_bonus_score(x: i32) -> i32 {
    ((x + 99) / 100) * 100
}

/// Star criteria used by `CResultsScreen @ 004351bc` (the value saved by `CPlayerInfo::SetCurrentEventStars`, caller
/// `CResultsScreen @ 00434554` with `this+0x204`): `total = GetScore() + GetBonusScore()`; 1 star if `Star1 <= total`,
/// 2 if `Star2 <= total`, 3 if `Star3 <= total` (HIGHER IS BETTER in every mode, time attack included). Can be 0.
pub fn stars_from_total(total: i32, star: &[i32; 3]) -> u8 {
    let mut s = 0u8;
    if star[0] <= total {
        s = 1;
    }
    if star[1] <= total {
        s = 2;
    }
    if star[2] <= total {
        s = 3;
    }
    s
}

/// `CEventDefinitionManager::GetStarsFromScore(int) @ 000fee38` (used by challenges: `CChallengeWinWithKart::OnEvent`
/// etc., fed with `CScoreSystem::GetScore()` only): 0 while the local car is still running, else strict `>` comparisons
/// and never less than 1.
pub fn stars_from_score_strict(score: i32, star: &[i32; 3], local_running: bool) -> u8 {
    if local_running {
        return 0;
    }
    if star[2] < score {
        3
    } else if star[1] < score {
        2
    } else {
        1
    }
}

/// `GetStarThresholdScores` + the daily-race / tournament override of the results screen
/// (`CMetagameManager::CalculateScoreFromCC @ 0012df14`): `(int)(fAddition + cc * fMultiplier * fXStarMultiplier)`.
/// `economy.xml <Score fMultiplier=150 fAddition=0 fOneStarMultiplier=1.00 fTwoStarMultiplier=1.40 fThreeStarMultiplier=1.90>`.
pub fn star_scores_from_cc(cc: i32, addition: f32, mult: f32, star_mult: [f32; 3]) -> [i32; 3] {
    let c = cc as f32;
    [
        (addition + c * mult * star_mult[0]) as i32,
        (addition + c * mult * star_mult[1]) as i32,
        (addition + c * mult * star_mult[2]) as i32,
    ]
}

/// The common result bookkeeping of a local player's score (the counters the three modes own plus the race-wide base).
/// `base_score` = sum of the NON-bonus counters (TopSpeed, Acceleration, Deaths, ... `CScoreSystem::GetScore @ 001f4288`),
/// supplied by the host; UNRESOLVED here because those counters need per-frame telemetry that is not part of the mode files.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScoreTotals {
    /// `CScoreSystem::GetScore()` (non-bonus counters)
    pub base_score: i32,
    /// raw bonus counter score (FinishingPosition / FinishingTime / FinishingFruit by mode)
    pub bonus_raw: i32,
}

impl ScoreTotals {
    /// `CScoreSystem::GetBonusScore()` (rounded up to 100)
    pub fn bonus_score(&self) -> i32 {
        round_bonus_score(self.bonus_raw)
    }
    /// `GetScore() + GetBonusScore()` = `CResultsScreen+0x164`
    pub fn total(&self) -> i32 {
        self.base_score + self.bonus_score()
    }
}

/// `<TrackTimes><Episode><Tier time=>` of `gameplay/misc/tracktimes.xml` (`CEventDefinitionManager` loader at `@ 00105xxx`,
/// "GMISC:TrackTimes.xml"): stored into each stage record at `+0xc` (stride 0x18). Not read by any of the three modes
/// (UNRESOLVED consumer: the per-stage record's float at +0xc; it is not a time-attack medal threshold).
pub fn load_track_times(xml: &str) -> Vec<Vec<f32>> {
    let mut out = Vec::new();
    if let Ok(doc) = roxmltree::Document::parse(xml) {
        for ep in doc.descendants().filter(|n| n.has_tag_name("Episode")) {
            out.push(
                ep.children()
                    .filter(|n| n.has_tag_name("Tier"))
                    .map(|t| t.attribute("time").map(strtod).unwrap_or(0.0))
                    .collect(),
            );
        }
    }
    out
}

/// Helper for tests / the host: the kart CC of the local player is needed for the tier (`CKartManager::GetKartCC`).
/// Resolve `timer_*` / `seed_amount_*` for an event for a given tier.
pub fn timer_duration(h: &EventHeader, tier: DifficultyTier) -> f32 {
    // CEventDefinitionManager::GetTimerDuration @ 000fee20: *(this + tier*4 + 0x250)
    h.timer[tier.index()]
}

/// `CEventDefinitionManager::GetTokenThreshold @ 000fee2c`: `*(this + (tier + 0x9a)*4)` = `+0x268 + tier*4`.
pub fn token_threshold(h: &EventHeader, tier: DifficultyTier) -> i32 {
    h.seed_amount[tier.index()]
}

/// Everything a mode needs from the host beyond the eventdef itself (all are fields of `CGame` in the original).
#[derive(Clone, Debug)]
pub struct ModeParams {
    /// number of karts in the race (`CGame+0x31b8`); the eventdef `kartcount=` when present, else host decides
    pub kart_count: usize,
    /// local player's kart CC (`CKartManager::GetKartCC`), input of `CalcDifficultyAdjustEnum`
    pub kart_cc: i32,
    /// the event's CC (`campaignCC`, `CGame+0x314`); also `CScoreSystem::SetRaceCC` argument
    pub event_cc: i32,
    /// `CGame+0x2cc` current campaign event index, -1 when none (quick race / MP)
    pub event_index: i32,
    /// `economy.xml relativeCC` per tier
    pub relative_cc: [i32; 5],
    pub score_cfg: ScoreConfig,
    /// force a tier instead of computing it (tests)
    pub tier_override: Option<DifficultyTier>,
    /// sum of the non-bonus score counters (TopSpeed/Acceleration/...) at finish; host fills before the finish is processed
    pub base_score: i32,
}

impl Default for ModeParams {
    fn default() -> Self {
        ModeParams {
            kart_count: 1,
            kart_cc: 50,
            event_cc: 50,
            event_index: 0,
            relative_cc: RELATIVE_CC_DEFAULT,
            score_cfg: ScoreConfig::default(),
            tier_override: None,
            base_score: 0,
        }
    }
}

impl ModeParams {
    /// `CGame::CalcDifficultyAdjustEnum @ 0011b800` (non-multiplayer branch).
    pub fn tier(&self) -> DifficultyTier {
        self.tier_override.unwrap_or_else(|| get_difficulty_adjust(self.event_cc, self.kart_cc, &self.relative_cc))
    }
}

/// Finish handling shared by all race-derived modes: the state / final-position part of `CGameMode::ProcessUpdate @ 0017caac`
/// when a running car crosses the finish. Returns true when `CPlayerInfo::AddCurrentEventStarCompletion` is called
/// (human car, campaign event, state 2).
pub fn apply_finish(
    data: &mut CarModeData,
    won: bool,
    is_player: bool,
    event_index: i32,
    car_position: i32,
    bonus_coins: i32,
) -> bool {
    let mut star_completion = false;
    data.won = won;
    data.state = if won { CarModeState::Completed } else { CarModeState::Failed };
    if !is_player || event_index < 0 {
        if event_index == -1 {
            data.state = CarModeState::Failed;
        }
    } else if data.state == CarModeState::Completed {
        star_completion = true;
    }
    data.final_position = car_position;
    data.bonus_coins = bonus_coins;
    star_completion
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_ids() {
        assert_eq!(string_to_game_mode("SEED_RUSH"), 7);
        assert_eq!(string_to_game_mode("TIME_ATTACK"), 6);
        assert_eq!(string_to_game_mode("VERSUS"), 5);
        assert_eq!(string_to_game_mode("LMR"), 14);
        assert_eq!(string_to_game_mode("nope"), 15);
    }

    #[test]
    fn parses_inline_seed_rush() {
        let x = r#"<EventDefinition><Name tag="0000" title="T" description="D"/><Environment pathname="environments\theme003\tracks\run000" config="1"/><Character name="red"/><GameMode name="SEED_RUSH" kartcount="2" fruit_rush_mode="ICECREAM"/><Difficulty level="0.335" seed_amount_veryeasy="90" seed_amount_easy="95" seed_amount_medium="100" seed_amount_hard="105" seed_amount_impossible="110" level_index="50" level_index="9" boss_bonus="0.0"/><Stars Star1="21500" Star2="23500" Star3="24500"/><DifficultyAdjust><VeryEasy>0.0</VeryEasy><Easy>0.05</Easy><Medium>0.075</Medium><Hard>0.1</Hard><Impossible>0.3</Impossible></DifficultyAdjust></EventDefinition>"#;
        let h = EventHeader::from_xml(x).unwrap();
        assert_eq!(h.theme, 3);
        assert_eq!(h.run, 0);
        assert_eq!(h.mode_id, mode_id::SEED_RUSH);
        assert_eq!(h.kartcount, 2);
        assert_eq!(h.fruit_rush_mode, FruitRushMode::IceCream);
        assert_eq!(h.seed_amount, [90, 95, 100, 105, 110]);
        assert_eq!(h.level_index, 50);
        assert_eq!(h.stars, [21500, 23500, 24500]);
        assert!((h.difficulty_adjust[4] - 0.3).abs() < 1e-6);
        assert_eq!(h.kind(), GameModeKind::SeedRush);
    }

    #[test]
    fn stars_fixup() {
        assert_eq!(fix_stars([0, 0, 0]), [0, 0, 0]);
        assert_eq!(fix_stars([5, 3, 1]), [5, 5, 5]);
    }

    #[test]
    fn versus_stars_are_zero() {
        let x = r#"<EventDefinition><GameMode name="VERSUS" kartcount="2"/><Stars 1="1000" 2="20000" 3="25000"/></EventDefinition>"#;
        let h = EventHeader::from_xml(x).unwrap();
        assert_eq!(h.stars, [0, 0, 0]);
        assert_eq!(h.kind(), GameModeKind::Versus);
    }

    #[test]
    fn all_shipped_eventdefs_parse() {
        let l = list_eventdefs();
        if l.is_empty() {
            return;
        }
        assert!(l.len() > 500);
        for f in &l {
            let h = load_header(f).unwrap_or_else(|| panic!("no header in {f}"));
            assert!(!h.mode_name.is_empty(), "{f}");
            assert_ne!(h.mode_id, mode_id::UNKNOWN, "{f}: {}", h.mode_name);
        }
    }
    #[test]
    fn difficulty_tier_from_cc() {
        let r = RELATIVE_CC_DEFAULT;
        // kart cc 50: <=46 very easy, <=50 easy, <=52 medium, <=54 hard, else impossible
        assert_eq!(get_difficulty_adjust(46, 50, &r), DifficultyTier::VeryEasy);
        assert_eq!(get_difficulty_adjust(50, 50, &r), DifficultyTier::Easy);
        assert_eq!(get_difficulty_adjust(52, 50, &r), DifficultyTier::Medium);
        assert_eq!(get_difficulty_adjust(54, 50, &r), DifficultyTier::Hard);
        assert_eq!(get_difficulty_adjust(55, 50, &r), DifficultyTier::Impossible);
    }

    #[test]
    fn shipped_economy_and_scoreconfig() {
        let p = format!("{}/xml/gameplay/misc/economy.xml", super::super::types::ASSETS292);
        if let Ok(s) = std::fs::read_to_string(p) {
            assert_eq!(relative_cc_from_economy(&s), Some([4, 0, -2, -4, -8]));
        }
        if let Some(c) = ScoreConfig::load() {
            assert_eq!(c.fruit, FruitCounterCfg { percent_for_min: 0.0, percent_for_max: 1.0, min_score: 0.125, max_score: 1.0 });
            assert_eq!(c.time, TimeCounterCfg { percent_over_for_max: 0.0, percent_over_for_min: 0.35, max_score: 1.0, min_score: 0.125 });
        }
        let p = format!("{}/xml/gameplay/misc/tracktimes.xml", super::super::types::ASSETS292);
        if let Ok(s) = std::fs::read_to_string(p) {
            let t = load_track_times(&s);
            assert_eq!(t.len(), 5);
            assert_eq!(t[0], vec![42.0, 41.0]);
        }
    }

    #[test]
    fn game_over_condition_and_race_max() {
        assert_eq!(race_max_score(55), 8250);
        assert!(check_game_over_condition(&[]));
        let running = LocalPlayerStatus { car_4d4_set: false, finish_line_crossed: false, mode_state_running: true, car_1ae8_set: true };
        let done = LocalPlayerStatus { finish_line_crossed: true, mode_state_running: false, ..running };
        // finished = state not running and no finish-line flag... the original counts (1b60==0 && state!=1) as done
        let finished_no_flag = LocalPlayerStatus { finish_line_crossed: false, mode_state_running: false, ..running };
        assert!(!check_game_over_condition(&[running]));
        assert!(!check_game_over_condition(&[done]));
        assert!(check_game_over_condition(&[finished_no_flag]));
        assert!(!check_game_over_condition(&[finished_no_flag, running]));
    }

}
