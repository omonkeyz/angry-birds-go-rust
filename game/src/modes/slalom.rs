//! SLALOM game mode (`<GameMode name="SLALOM"/>`, `EGameMode` id 0xd), ported from the Angry Birds Go 2.9.1 decompile.
//!
//! Original classes: `CGameModeSlalom` (vtable `0xd91c70`), `CGameModeSlalomData` (per-car data object, `CCar+0x1ae0`).
//! A slalom event is a timed run (like TIME_ATTACK, whose per-car data class is byte-identical) down a normal race track
//! with pairs of posts ("gates") placed across the road. The player has a countdown clock (`timer_*` of the eventdef,
//! chosen by `EDifficultyAdjust`). Every gate the kart crosses *between* its two posts is a clean pass; every gate the
//! kart passes *outside* the posts is a miss: the clock loses `time_penalty` seconds and a block of TNT crates is spawned
//! on the road ahead of the kart. There is no time bonus for clean gates. The event is "completed" when the kart crosses
//! the finish with `time_left >= 0`; the leftover seconds become bonus coins (x10).
//!
//! # What differs from a RACE (which `CGameMode` virtuals `CGameModeSlalom` overrides)
//! vtable slots (see `CGameMode` layout: 0 dtor, 1 delete, 2 InitialiseMode, 3 InitialiseCarData, 4 InitialiseGrid,
//! 5 Update, 6 UpdateGameEnd, 7 CheckGameOverCondition, 8 CheckFinishLineCrossed, 9 GetAICharacter, 10 GetAIKart,
//! 11 GetBonusCoins, 12 GetEventCompleted, 13 OnWeaponFire, 14 OnCarCollision, 15 OnQuitEvent, 16 OnFinishEvent,
//! 17 GetTimeLeft):
//! * `InitialiseMode @ 00186718`  : base + `mode_id=0xd`, `+0x24 = 0x7d` (TNT smackable type), `+4 = +8 = 1`.
//! * `InitialiseCarData @ 001865f4`: every car gets a `CGameModeSlalomData` (timer from `GetTimerDuration`), and
//!   `CGame+0x3244 = 5.0` (same as `CGameModeRace::InitialiseCarData @ 001851bc`).
//! * `Update @ 00185a04`           : base `CGameMode::Update` then the gate logic ([`SlalomMode::update_gates`]).
//! * `CheckGameOverCondition @ 001852f4`: "all human players finished or inactive" (same body as the race variant).
//! * `GetBonusCoins @ 00185480`    : `max(trunc(time_left), 0) * 10`.
//! * `GetEventCompleted @ 001854a4`: `time_left >= 0`.
//! * `GetTimeLeft @ 0018558c`      : local player's `data+0x24` (the HUD, `CXGSFE_SlalomTimerDisplay`, reads it).
//! * Everything else (grid, finish line, AI) is inherited from `CGameMode`. Time running out does NOT end the run in the
//!   original (nothing reads `time_left <= 0` except the HUD and the gate check); the kart can still finish, but
//!   `GetEventCompleted` is then false -> result state 3 (lost).
//!
//! Standalone: std + glam + (no xml crate needed; eventdefs in the shipped data are not always well formed, so a tolerant
//! tag scanner is used, first attribute occurrence wins).
#![allow(dead_code)]

use super::types::*;

/// `EGameMode` id of SLALOM (`StringToGameMode @ 000fa1bc`; `CGameModeSlalom::InitialiseMode @ 00186718`: `this+0xc = 0xd`).
pub const GAME_MODE_ID: u32 = 0xd;
/// Smackable type id of the TNT crates spawned on a missed gate (`CGameModeSlalom::InitialiseMode`: `this+0x24 = 0x7d`).
pub const TNT_SMACKABLE_TYPE: i32 = 0x7d;
/// Max gates the mode object can hold (`CGameModeSlalom` ctor loops `+0x3c/+0x4c` over `0x144`-byte records up to `+0x2880`).
pub const MAX_GATES: usize = 32;
/// Max coins linkable to one gate. A gate record is `0x144` bytes (`0x51` dwords) starting at `this+0x2c+idx*0x144`;
/// the coin count sits at `this+0x6c` (record `+0x40`) and the coin pointer array starts at `this+0x70` (record `+0x44`),
/// so the header is `0x44` bytes (17 dwords) and `0x51 - 0x11 = 64` coin slots remain (`AddCoinToSlalom` has no bound check).
pub const MAX_COINS_PER_GATE: usize = 0x51 - 0x11;
/// The smackable model of the TNT crate (type `0x7d` is an alias of `0x30`, `smck_TNT_Box_3m`; pak file
/// `smackables/smck_tnt_box_3m.xgm`).
pub const TNT_SMACKABLE_NAME: &str = "smck_TNT_Box_3m";
/// `CGameModeSlalomData::Update @ 00185864`: the low-time warning (`ABKSound::CUIController::OnEvent(0x1f)`) starts when
/// `time_left < 5.0`.
pub const LOW_TIME_WARNING_SECONDS: f32 = 5.0;
/// UI sound event ids sent through `ABKSound::CUIController::OnEvent`.
pub mod ui_event {
    /// low-time warning on (`CGameModeSlalomData::Update`)
    pub const LOW_TIME_START: u32 = 0x1f;
    /// low-time warning off (`CGameModeSlalomData::Update`/`Reset`/ctor)
    pub const LOW_TIME_STOP: u32 = 0x20;
    /// gate missed (time penalty)
    pub const GATE_MISSED: u32 = 0x27;
    /// gate cleared
    pub const GATE_CLEAN: u32 = 0x28;
    /// TNT crates spawned
    pub const GATE_TNT_SPAWN: u32 = 0x29;
}
/// Delay before the voice line for the last gate result (`CGameModeSlalom` ctor: `this+0x28ac = 0x3f400000`;
/// `Update` sets the same value after every gate).
pub const GATE_VOICE_DELAY: f32 = 0.75;
/// `CVoiceController::OnSlalomEvent(voiceCtl, 1)` after a clean gate, `(…, 0)` after a miss (`Update @ 00185a04`).
pub const VOICE_CLEAN: i32 = 1;
pub const VOICE_MISSED: i32 = 0;
/// Particle effect spawned on every TNT crate (`FindEffect("KingSlingSpawnIn")`, instance name
/// `"Slalom TnT Gate (%d) Col (%d) Row (%d)"`).
pub const TNT_SPAWN_EFFECT: &str = "KingSlingSpawnIn";
/// Crates are lifted by this much after the ground snap (`Update`: `y += 5.0`).
pub const TNT_SPAWN_LIFT: f32 = 5.0;
/// `GetGeometryBelow` hit is accepted when its squared distance to the probe point is `< 400` (`DAT_00185ac4`, i.e. 20 m).
pub const TNT_GROUND_SNAP_SQ_DIST: f32 = 400.0;
/// If there is no ground within that range the probe is raised by this much and tried once more (`Update`: `y += 10.0`).
pub const TNT_GROUND_RETRY_RAISE: f32 = 10.0;
/// Crates are placed `lookahead_param * 1.5` ahead of the kart on its spline (asm `0x185d04: vmov s21,#1.5`;
/// `CSpline::Lookahead(spline, car+0x1a8c, car+0x1ab4 * 1.5, NULL)`).
pub const TNT_LOOKAHEAD_FACTOR: f32 = 1.5;

/// `EDifficultyAdjust` (index into `CEventDefinitionManager+0x250` timers).
pub const DIFFICULTY_VERY_EASY: usize = 0;
pub const DIFFICULTY_EASY: usize = 1;
pub const DIFFICULTY_MEDIUM: usize = 2;
pub const DIFFICULTY_HARD: usize = 3;
pub const DIFFICULTY_IMPOSSIBLE: usize = 4;
/// Default timers when the eventdef has no `timer_*` attribute (`ReadEventDefinitionFromXML @ 00101478`:
/// `manager+0x250.. = 0x42700000, 0x423e0000, 0x420c0000, 0x41b40000, 0x41200000`).
pub const DEFAULT_TIMERS: [f32; 5] = [60.0, 47.5, 35.0, 22.5, 10.0];

// ------------------------------------------------------------------------------------------------------------
// Tolerant xml scanning (shipped eventdefs may contain duplicate attributes)
// ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

fn scan_tags(xml: &str) -> Vec<Tag> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= b.len() {
            break;
        }
        if b[i] == b'/' || b[i] == b'!' || b[i] == b'?' {
            while i < b.len() && b[i] != b'>' {
                i += 1;
            }
            continue;
        }
        let s = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        let mut tag = Tag { name: xml[s..i].to_string(), attrs: Vec::new() };
        loop {
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
                i += 1;
            }
            if i >= b.len() || b[i] == b'>' {
                break;
            }
            let ns = i;
            while i < b.len() && b[i] != b'=' && b[i] != b'>' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let an = xml[ns..i].to_string();
            if i < b.len() && b[i] == b'=' {
                i += 1;
                if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                    let q = b[i];
                    i += 1;
                    let vs = i;
                    while i < b.len() && b[i] != q {
                        i += 1;
                    }
                    tag.attrs.push((an, xml[vs..i].to_string()));
                    i += 1;
                }
            }
        }
        out.push(tag);
        i += 1;
    }
    out
}

fn fattr(t: &Tag, k: &str) -> Option<f32> {
    t.get(k).and_then(|v| v.trim().parse::<f32>().ok())
}

fn iattr(t: &Tag, k: &str) -> Option<i32> {
    // the original uses atoi()/strtod(); accept "3" and tolerate a trailing garbage like atoi does
    t.get(k).map(|v| {
        let v = v.trim();
        let end = v
            .char_indices()
            .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(0);
        v[..end].parse::<i32>().unwrap_or(0)
    })
}

// ------------------------------------------------------------------------------------------------------------
// Eventdef
// ------------------------------------------------------------------------------------------------------------

/// One `<TrackItem helpername="slalom_gate" .../>` (`CEventDefinitionManager::ReadTrackItem` @ ~0x100b60 expands it into
/// the two helpers `slalom_post_{blue,red}_left/right`, see [`SlalomGateDef::post_lateral_offsets`]).
/// Defaults are the ones of the original reader (`separation` 8, `time_penalty` 5, `cols` 3, `rows` 3, `col_spacing` 10,
/// `row_spacing` 10).
#[derive(Clone, Debug, PartialEq)]
pub struct SlalomGateDef {
    pub spline: String,
    pub fraction_along_spline: f32,
    pub lateral_offset_from_spline: f32,
    pub height_above_spline: f32,
    /// distance between the two posts (m)
    pub separation: f32,
    /// seconds removed from the clock when the gate is missed
    pub time_penalty: f32,
    /// TNT grid: columns along the road (`cols`) and rows across it (`rows`)
    pub cols: i32,
    pub rows: i32,
    /// spacing of the TNT grid along the road (`col_spacing`) / across it (`row_spacing`), metres
    pub col_spacing: f32,
    pub row_spacing: f32,
}

impl SlalomGateDef {
    /// The two posts are placed at `-separation*0.5` (left) and `+separation*0.5` (right) added to helper record field
    /// `+0x5c` (`ReadTrackItem` slalom_gate branch: `*(float*)(rec+0x5c) -/+ separation * 0.5`).
    /// UNRESOLVED: `+0x5c` is NOT `lateraloffsetfromspline` (that is stored at `+0x60`; `distancealongspline`/
    /// `fractionalongspline` land at `+0x50`); the decompile does not name `+0x5c`, it behaves as a metric sideways offset
    /// of the helper, so a host must add it on top of the fractional lateral offset and not subtract it from it.
    pub fn post_lateral_offsets(&self) -> (f32, f32) {
        (-self.separation * 0.5, self.separation * 0.5)
    }

    /// Helper names / colour of the two post helpers of gate number `gate_index` (0-based order of the `slalom_gate`
    /// items in the eventdef). `ReadTrackItem` picks blue when `manager+0x5ac & 1 == 0`; `+0x5ac` counts the gates already
    /// read (incremented at the end of the branch), so gate 0 is blue, gate 1 red, gate 2 blue ...
    pub fn post_helpers(gate_index: usize) -> GatePosts {
        if gate_index % 2 == 0 {
            GatePosts {
                red: false,
                left_helper: "slalom_post_blue_left",
                right_helper: "slalom_post_blue_right",
                left_smackable: ("smck_Slalom_BLUE_LT", 0x6d),
                right_smackable: ("smck_Slalom_BLUE_RT", 0x71),
            }
        } else {
            GatePosts {
                red: true,
                left_helper: "slalom_post_red_left",
                right_helper: "slalom_post_red_right",
                left_smackable: ("smck_Slalom_RED_LT", 0x75),
                right_smackable: ("smck_Slalom_RED_RT", 0x79),
            }
        }
    }
}

/// The post helpers of one gate and the smackable type-table entries (`CSmackableManager` table at `0xdad720`) whose names
/// match them (UNRESOLVED: the helper -> smackable type link goes through the env object table and was not traced; the
/// type names below are the only `Slalom_*` entries of the table, each with three `_Frag` debris entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatePosts {
    pub red: bool,
    pub left_helper: &'static str,
    pub right_helper: &'static str,
    pub left_smackable: (&'static str, i32),
    pub right_smackable: (&'static str, i32),
}

/// Instance name given to every crate's spawn effect (`sprintf("Slalom TnT Gate (%d) Col (%d) Row (%d)", gate, col, row)`).
pub fn tnt_instance_name(gate: usize, col: i32, row: i32) -> String {
    format!("Slalom TnT Gate ({gate}) Col ({col}) Row ({row})")
}

/// Slalom specific part of an eventdef (header values the mode needs: timers + stars).
#[derive(Clone, Debug, PartialEq)]
pub struct SlalomEventDef {
    /// `timer_veryeasy .. timer_impossible` seconds (index = `EDifficultyAdjust`).
    pub timers: [f32; 5],
    /// `<Stars Star1= Star2= Star3=>` score thresholds (0 when absent).
    pub stars: [i32; 3],
    pub gates: Vec<SlalomGateDef>,
    /// number of `pickup_coin` track items (coins are not linked to gates in the shipped slalom eventdefs, see
    /// [`SlalomMode::add_coin_to_gate`])
    pub coin_items: usize,
    pub game_mode_name: String,
}

impl SlalomEventDef {
    pub fn from_xml(xml: &str) -> Result<SlalomEventDef, String> {
        let tags = scan_tags(xml);
        let gm = tags
            .iter()
            .find(|t| t.name == "GameMode")
            .and_then(|t| t.get("name"))
            .ok_or_else(|| "no <GameMode name=...>".to_string())?
            .to_string();
        if gm != "SLALOM" {
            return Err(format!("GameMode is {gm}, not SLALOM"));
        }
        let mut timers = DEFAULT_TIMERS;
        if let Some(d) = tags.iter().find(|t| t.name == "Difficulty") {
            let names = ["timer_veryeasy", "timer_easy", "timer_medium", "timer_hard", "timer_impossible"];
            for (i, n) in names.iter().enumerate() {
                if let Some(v) = fattr(d, n) {
                    timers[i] = v;
                }
            }
        }
        let mut stars = [0i32; 3];
        if let Some(s) = tags.iter().find(|t| t.name == "Stars") {
            // original: Star1/Star2 only written when present; Star3 keeps its old value otherwise (0 here)
            for (i, n) in ["Star1", "Star2", "Star3"].iter().enumerate() {
                if let Some(v) = iattr(s, n) {
                    stars[i] = v;
                }
            }
            // `ReadEventDefinitionFromXML`: if Star2 < Star1 then Star2 = Star1; if Star3 < Star2 then Star3 = Star2
            if !(stars[0] == 0 && stars[1] == 0 && stars[2] == 0) {
                if stars[1] < stars[0] {
                    stars[1] = stars[0];
                }
                if stars[2] < stars[1] {
                    stars[2] = stars[1];
                }
            }
        }
        let mut gates = Vec::new();
        let mut coin_items = 0;
        for t in tags.iter().filter(|t| t.name == "TrackItem") {
            match t.get("helpername") {
                Some(h) if h.eq_ignore_ascii_case("slalom_gate") => gates.push(SlalomGateDef {
                    spline: t.get("spline").unwrap_or("").to_string(),
                    fraction_along_spline: fattr(t, "fractionalongspline").unwrap_or(0.0),
                    lateral_offset_from_spline: fattr(t, "lateraloffsetfromspline").unwrap_or(0.0),
                    height_above_spline: fattr(t, "heightabovespline").unwrap_or(0.0),
                    separation: fattr(t, "separation").unwrap_or(8.0),
                    time_penalty: fattr(t, "time_penalty").unwrap_or(5.0),
                    cols: iattr(t, "cols").unwrap_or(3),
                    rows: iattr(t, "rows").unwrap_or(3),
                    col_spacing: fattr(t, "col_spacing").unwrap_or(10.0),
                    row_spacing: fattr(t, "row_spacing").unwrap_or(10.0),
                }),
                Some(h) if h.eq_ignore_ascii_case("pickup_coin") => coin_items += 1,
                _ => {}
            }
        }
        Ok(SlalomEventDef { timers, stars, gates, coin_items, game_mode_name: gm })
    }

    /// `CEventDefinitionManager::GetTimerDuration(EDifficultyAdjust) @ 000fee20`.
    pub fn timer_duration(&self, difficulty_adjust: usize) -> f32 {
        self.timers[difficulty_adjust.min(4)]
    }
}

/// `CEventDefinitionManager::GetStarsFromScore(int) @ 000fee38`: 0 while the local player's data state is 1 (running);
/// 3 stars if `score > Star3`, 2 if `score > Star2`, otherwise 1 (also for `score <= Star1`).
pub fn stars_from_score(running: bool, score: i64, stars: [i32; 3]) -> u8 {
    if running {
        0
    } else if score > stars[2] as i64 {
        3
    } else if score > stars[1] as i64 {
        2
    } else {
        1
    }
}

// ------------------------------------------------------------------------------------------------------------
// World queries needed to place the TNT crates (host supplies them from the track / spline / env)
// ------------------------------------------------------------------------------------------------------------

/// Spline / geometry queries used by the miss branch of `CGameModeSlalom::Update @ 00185a04`.
pub trait SplineProbe {
    /// `CSpline::GetPosition(CSpline::Lookahead(spline, from_pos, distance, NULL))`: world position `distance` m of
    /// spline ahead of spline position `from_pos` (`Lookahead @ 0018dab4`, `GetPosition @ 0018d4c0`).
    fn position_ahead(&self, from_pos: f32, distance: f32) -> Vec3;
    /// `CSpline::GetClosestSplinePos(p)` @ 0018f280.
    fn closest_spline_pos(&self, p: Vec3) -> f32;
    /// `CSpline::GetLeftWidth(pos)` @ 0018dfb0 / `GetRightWidth(pos)` @ 0018e008.
    fn left_width(&self, spline_pos: f32) -> f32;
    fn right_width(&self, spline_pos: f32) -> f32;
    /// `GetRightVectorInterpolated @ 0018fc9c` / `GetForwardVectorInterpolated @ 0018fd58`.
    fn right_vector(&self, spline_pos: f32) -> Vec3;
    fn forward_vector(&self, spline_pos: f32) -> Vec3;
    /// `CXGSEnv::GetGeometryBelow(&hit, env, p) @ 00537a90`: the ground point below `p`, if any.
    fn ground_below(&self, p: Vec3) -> Option<Vec3>;
}

/// Where the kart is on its spline (`CCar+0x1a8c` spline position, `+0x1a98` signed lateral offset, `+0x1ab4` lookahead
/// parameter; `Update` multiplies the latter by 1.5 to get the crate lookahead distance). UNRESOLVED: the meaning of
/// `+0x1ab4` is not named in the decompile; it behaves like the kart's forward speed (m/s), so crates land ~1.5 s ahead.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlalomCarProbe {
    pub spline_pos: f32,
    pub lateral_offset: f32,
    pub lookahead_param: f32,
}

/// World positions of the TNT crates for one missed gate, exactly the arithmetic of `CGameModeSlalom::Update @ 00185a04`
/// (inner `do/while` over `cols` x `rows`). Returns one position per crate in `(col, row)` order (col outer, row inner).
///
/// For each crate: `base = position_ahead(spline_pos, lookahead_param*1.5)`; `s = closest_spline_pos(base)`;
/// `lat = if car_lateral <= 0 { min(car_lateral, left_width(s) + rows/2*row_spacing) }
///        else { min(car_lateral, right_width(s) - rows/2*row_spacing) }`
/// (the `min` on the left branch is as written in the original, `0x18641c`); `pos = base + lat*right(s)
/// + (row - rows/2)*row_spacing*right(s) + (col - cols/2)*col_spacing*forward(s)`; then snapped to the ground within 20 m
/// (raise 10 m and retry once) and lifted by 5.
pub fn tnt_crate_positions(gate: &SlalomGateDef, car: &SlalomCarProbe, spline: &dyn SplineProbe) -> Vec<Vec3> {
    let mut out = Vec::new();
    let rows_f = gate.rows as f32;
    let cols_f = gate.cols as f32;
    for col in 0..gate.cols.max(0) {
        for row in 0..gate.rows.max(0) {
            let base = spline.position_ahead(car.spline_pos, car.lookahead_param * TNT_LOOKAHEAD_FACTOR);
            let s = spline.closest_spline_pos(base);
            let lat = if car.lateral_offset <= 0.0 {
                let edge = spline.left_width(s) + rows_f * 0.5 * gate.row_spacing;
                if car.lateral_offset <= edge { car.lateral_offset } else { edge }
            } else {
                let edge = spline.right_width(s) - rows_f * 0.5 * gate.row_spacing;
                if car.lateral_offset <= edge { car.lateral_offset } else { edge }
            };
            let right = spline.right_vector(s);
            let fwd = spline.forward_vector(s);
            let mut p = base + right * lat;
            p += right * ((row as f32 - rows_f * 0.5) * gate.row_spacing);
            p += fwd * ((col as f32 - cols_f * 0.5) * gate.col_spacing);
            // ground snap
            let mut snapped = None;
            if let Some(h) = spline.ground_below(p) {
                if (h - p).length_squared() < TNT_GROUND_SNAP_SQ_DIST {
                    snapped = Some(h);
                }
            }
            if snapped.is_none() {
                let raised = Vec3::new(p.x, p.y + TNT_GROUND_RETRY_RAISE, p.z);
                if let Some(h) = spline.ground_below(raised) {
                    if (h - raised).length_squared() < TNT_GROUND_SNAP_SQ_DIST {
                        snapped = Some(h);
                    }
                }
            }
            let mut q = snapped.unwrap_or(p);
            q.y += TNT_SPAWN_LIFT;
            out.push(q);
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------
// Runtime
// ------------------------------------------------------------------------------------------------------------

/// `CGameModeData+4` result state of the local car (`CGameMode::UpdateGameEnd @ 0017c808`: 1 running, 2 completed,
/// 3 not completed; `ProcessUpdate` also uses 5/6 for multiplayer forced win/lose).
pub mod data_state {
    pub const RUNNING: u8 = 1;
    pub const COMPLETED: u8 = 2;
    pub const FAILED: u8 = 3;
}

/// One gate record (`0x144` bytes in the original, array at `CGameModeSlalom+0x2c`).
#[derive(Clone, Debug)]
pub struct SlalomGate {
    pub def: SlalomGateDef,
    /// world position of the left / right post (`CEnvObject+0xc8..0xd0`), `AddLeftGate`/`AddRightGate`
    pub left: Vec3,
    pub right: Vec3,
    /// closest spline position of the gate centre on the car's spline (what `Update` recomputes each frame from the
    /// midpoint of the posts via `GetClosestSplinePos`; the host precomputes it per spline)
    pub spline_pos: f32,
    /// `rec+0x2c`: already evaluated
    pub passed: bool,
    /// pickup ids deactivated when the gate is missed (`AddCoinToSlalom`; `coin+0x7c = 0, +0x80 = 1`)
    pub coins: Vec<u32>,
}

/// Result of evaluating one gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateResult {
    Clean,
    Missed,
}

/// The two dot-product tests of `CGameModeSlalom::Update @ 00185a04`. `P` = kart chassis position (`rb+0x38..0x40`),
/// `L`/`R` the post positions. Clean iff `(L-R).normalised . (P-L).normalised < 0` **and**
/// `(R-L).normalised . (P-R).normalised < 0`, i.e. the kart is strictly between the posts along the gate axis (3-D dot
/// products, so a kart far above/below counts the same as the original). If the first test fails the second is not
/// evaluated (miss).
pub fn classify_gate(left: Vec3, right: Vec3, p: Vec3) -> GateResult {
    let a = (left - right).normalize_or_zero();
    let b = (p - left).normalize_or_zero();
    if a.dot(b) < 0.0 {
        let c = (right - left).normalize_or_zero();
        let d = (p - right).normalize_or_zero();
        if c.dot(d) < 0.0 {
            return GateResult::Clean;
        }
    }
    GateResult::Missed
}

#[derive(Clone, Debug)]
pub struct SlalomMode {
    pub def: SlalomEventDef,
    pub difficulty_adjust: usize,
    pub gates: Vec<SlalomGate>,
    // ---- CGameModeSlalomData (local player's car) ----
    /// `data+0x20` initial time, `data+0x24` time left (seconds, can go negative)
    pub time_total: f32,
    pub time_left: f32,
    /// `data+4`
    pub data_state: u8,
    /// `data+0x2c`: the low-time warning has been started
    pub low_time_warning: bool,
    /// `data+0x30`: at least one gate was missed (set by the miss branch; nothing else reads it in 2.9.1)
    pub missed_any: bool,
    /// `data+8` race time (only advances while running and `race_time > 0`), `data+0x10` position, `data+0x14` best position
    pub data_race_time: f32,
    pub data_position: i32,
    pub data_best_position: i32,
    // ---- CGameModeSlalom ----
    /// `this+0x1c` mode clock
    pub mode_clock: f32,
    /// `this+0x28ac` / `this+0x28b0`: delayed voice line (0 none, 1 clean, 2 missed)
    pub msg_timer: f32,
    pub msg_type: u8,
    /// `CGame.playerinfo+0x9e4` ("gates passed" stat, incremented on every clean gate)
    pub gates_cleaned_stat: u32,
    pub gates_missed: u32,
    pub car: SlalomCarProbe,
    /// `CScoreSystem::GetScore()` of the host's score system (UNRESOLVED: the slalom score counters live in the score
    /// system, which is not part of this module)
    pub score_total: i64,
    pub finish_time: Option<f32>,
}

impl SlalomMode {
    /// `CGameModeSlalom::CGameModeSlalom @ 00186744` + `InitialiseCarData`: data timer = `GetTimerDuration(difficulty)`.
    /// `difficulty_adjust` is `CGame::CalcDifficultyAdjustEnum(level_index)` = `CMetagameManager::GetDifficultyAdjust`
    /// (meta rules, not part of this module).
    pub fn new(def: SlalomEventDef, difficulty_adjust: usize) -> SlalomMode {
        let t = def.timer_duration(difficulty_adjust);
        SlalomMode {
            def,
            difficulty_adjust,
            gates: Vec::new(),
            time_total: t,
            time_left: t,
            data_state: data_state::RUNNING,
            low_time_warning: false,
            missed_any: false,
            data_race_time: 0.0,
            data_position: 0,
            data_best_position: 1000,
            mode_clock: 0.0,
            msg_timer: GATE_VOICE_DELAY,
            msg_type: 0,
            gates_cleaned_stat: 0,
            gates_missed: 0,
            car: SlalomCarProbe::default(),
            score_total: 0,
            finish_time: None,
        }
    }

    /// `CGameModeSlalomData::Reset @ 00185910`: restart the per-car data (timer back to the duration, flags cleared).
    pub fn reset_data(&mut self) {
        self.data_state = data_state::RUNNING;
        self.data_position = 0;
        self.data_best_position = 1000;
        self.data_race_time = 0.0;
        let t = self.def.timer_duration(self.difficulty_adjust);
        self.time_total = t;
        self.time_left = t;
        self.low_time_warning = false;
        self.missed_any = false;
        self.finish_time = None;
        for g in &mut self.gates {
            g.passed = false;
        }
    }

    /// `AddLeftGate @ 001855d0` + `AddRightGate @ 001855e8`: register a gate once both posts exist. `spline_pos` is the
    /// closest spline position of the midpoint of the posts. Returns the gate index. Ignored (None) past `MAX_GATES`
    /// (the original has no bound check; the array simply ends at 32 records).
    pub fn add_gate(&mut self, def: SlalomGateDef, left: Vec3, right: Vec3, spline_pos: f32) -> Option<usize> {
        if self.gates.len() >= MAX_GATES {
            return None;
        }
        self.gates.push(SlalomGate { def, left, right, spline_pos, passed: false, coins: Vec::new() });
        Some(self.gates.len() - 1)
    }

    /// `AddCoinToSlalom(coin, gate_index) @ 0018582c`. In the shipped 2.9.x slalom eventdefs the `pickup_coin` items are
    /// siblings of the gates (not children), and the reader only links coins nested under a `slalom_gate` item, so this
    /// list stays empty for them. Ignored when `gate >= gate count` (same guard as the original).
    pub fn add_coin_to_gate(&mut self, gate: usize, coin: u32) {
        if let Some(g) = self.gates.get_mut(gate) {
            if g.coins.len() < MAX_COINS_PER_GATE {
                g.coins.push(coin);
            }
        }
    }

    pub fn set_car_probe(&mut self, probe: SlalomCarProbe) {
        self.car = probe;
    }

    /// `CGameModeSlalom::GetTimeLeft @ 0018558c`.
    pub fn time_left(&self) -> f32 {
        self.time_left
    }

    /// `GetBonusCoins @ 00185480`: `max((int)time_left, 0) * 10` (C float->int truncates toward zero).
    pub fn bonus_coins(&self) -> i32 {
        (self.time_left as i32).max(0) * 10
    }

    /// `GetEventCompleted @ 001854a4`.
    pub fn event_completed(&self) -> bool {
        0.0 <= self.time_left
    }

    /// `CGameModeSlalomData::Update @ 00185864` + `CGameModeData::Update @ 0017c450` for the local car.
    fn update_data(&mut self, dt: f32, kart: &KartState, events: &mut Vec<ModeEvent>) {
        if self.data_state == data_state::RUNNING {
            if kart.race_time > 0.0 {
                if !self.low_time_warning && self.time_left < LOW_TIME_WARNING_SECONDS {
                    events.push(ModeEvent::Message { tag: format!("ui_event:0x{:x}", ui_event::LOW_TIME_START) });
                    self.low_time_warning = true;
                }
                self.time_left -= dt;
                // (base) `data+8 += dt` while running and the race clock is > 0
                self.data_race_time += dt;
                events.push(ModeEvent::Timer { remaining: self.time_left });
            }
        } else {
            events.push(ModeEvent::Message { tag: format!("ui_event:0x{:x}", ui_event::LOW_TIME_STOP) });
        }
        // (base) position bookkeeping: data+0x10 = car+0x1a9c, data+0x14 = min(data+0x14, position)
        self.data_position = kart.race_position as i32;
        if self.data_position <= self.data_best_position {
            self.data_best_position = self.data_position;
        }
    }

    /// Gate evaluation (second half of `CGameModeSlalom::Update @ 00185a04`) for the local player `kart`.
    /// `world` supplies the spline / ground queries for the TNT crates; when `None` the crates are requested through a
    /// `CarEffect::Other { tag: "SlalomTnTSpawn" }` instead (vals = [gate, cols, rows, 0], pos = kart position).
    pub fn update_gates(
        &mut self,
        dt: f32,
        kart: &KartState,
        world: Option<&dyn SplineProbe>,
        effects: &mut Vec<CarEffect>,
        events: &mut Vec<ModeEvent>,
    ) {
        // delayed voice line
        if self.msg_type != 0 {
            self.msg_timer -= dt;
            if self.msg_timer <= 0.0 {
                let v = if self.msg_type == 1 { VOICE_CLEAN } else { VOICE_MISSED };
                events.push(ModeEvent::Other { tag: "slalom_voice", value: v as f32 });
                self.msg_type = 0;
            }
        }
        for gi in 0..self.gates.len() {
            if self.gates[gi].passed {
                continue;
            }
            // the kart must be past the gate on its spline AND time must be left (data+0x24 > 0)
            // `Update` recomputes `GetClosestSplinePos(car spline, midpoint of the posts)` every frame (the car may be on
            // another spline than the one the gate was registered with); the stored value is only the fallback.
            let gate_pos = match world {
                Some(w) => w.closest_spline_pos((self.gates[gi].left + self.gates[gi].right) * 0.5),
                None => self.gates[gi].spline_pos,
            };
            if !(self.car.spline_pos > gate_pos) {
                continue;
            }
            if !(self.time_left > 0.0) {
                continue;
            }
            self.gates[gi].passed = true;
            let res = classify_gate(self.gates[gi].left, self.gates[gi].right, kart.pos);
            match res {
                GateResult::Clean => {
                    self.msg_timer = GATE_VOICE_DELAY;
                    self.msg_type = 1;
                    self.gates_cleaned_stat += 1;
                    events.push(ModeEvent::Message { tag: format!("ui_event:0x{:x}", ui_event::GATE_CLEAN) });
                }
                GateResult::Missed => {
                    self.missed_any = true;
                    self.gates_missed += 1;
                    self.time_left -= self.gates[gi].def.time_penalty;
                    self.msg_timer = GATE_VOICE_DELAY;
                    self.msg_type = 2;
                    events.push(ModeEvent::Message { tag: format!("ui_event:0x{:x}", ui_event::GATE_MISSED) });
                    let def = self.gates[gi].def.clone();
                    match world {
                        Some(w) => {
                            for p in tnt_crate_positions(&def, &self.car, w) {
                                effects.push(CarEffect::SpawnObject {
                                    kind: TNT_SMACKABLE_NAME.to_string(),
                                    pos: p,
                                    velocity: Vec3::ZERO,
                                    owner: None,
                                });
                                effects.push(CarEffect::Particle { name: TNT_SPAWN_EFFECT.to_string(), car: None, pos: p });
                            }
                        }
                        None => effects.push(CarEffect::Other {
                            tag: "SlalomTnTSpawn",
                            car: Some(kart.id),
                            vals: [gi as f32, def.cols as f32, def.rows as f32, def.time_penalty],
                            pos: kart.pos,
                        }),
                    }
                    events.push(ModeEvent::Message { tag: format!("ui_event:0x{:x}", ui_event::GATE_TNT_SPAWN) });
                    for c in self.gates[gi].coins.clone() {
                        effects.push(CarEffect::Other { tag: "SlalomCoinDeactivate", car: None, vals: [c as f32, 0.0, 0.0, 0.0], pos: Vec3::ZERO });
                    }
                }
            }
            events.push(ModeEvent::Other { tag: "slalom_gate", value: if res == GateResult::Clean { 1.0 } else { 0.0 } });
        }
    }

    /// `CGameMode::UpdateGameEnd/ProcessUpdate` finish handling for the local kart: the default finish-line check ends the
    /// run; `state = GetEventCompleted ? 2 : 3`.
    pub fn finish(&mut self, time: f32, events: &mut Vec<ModeEvent>) {
        if self.data_state != data_state::RUNNING {
            return;
        }
        self.finish_time = Some(time);
        self.data_state = if self.event_completed() { data_state::COMPLETED } else { data_state::FAILED };
        let won = self.data_state == data_state::COMPLETED;
        events.push(ModeEvent::Finished { won, score: self.score_total, stars: self.stars() });
    }

    /// Like [`GameModeRules::update`] but with the spline queries available.
    pub fn update_with_world(
        &mut self,
        dt: f32,
        karts: &[KartState],
        world: Option<&dyn SplineProbe>,
        effects: &mut Vec<CarEffect>,
        events: &mut Vec<ModeEvent>,
    ) {
        self.mode_clock += dt;
        if let Some(k) = karts.iter().find(|k| k.is_player) {
            self.update_data(dt, k, events);
            if self.data_state == data_state::RUNNING {
                self.update_gates(dt, k, world, effects, events);
            }
        }
    }
}

impl GameModeRules for SlalomMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::Slalom
    }

    fn update(&mut self, dt: f32, karts: &[KartState], effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>) {
        self.update_with_world(dt, karts, None, effects, events);
    }

    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        match input {
            ModeInput::KartFinished { time, .. } => self.finish(*time, events),
            ModeInput::Other { tag: "slalom_car_spline_pos", value, .. } => self.car.spline_pos = *value,
            ModeInput::Other { tag: "slalom_car_lateral", value, .. } => self.car.lateral_offset = *value,
            ModeInput::Other { tag: "slalom_car_lookahead", value, .. } => self.car.lookahead_param = *value,
            ModeInput::Other { tag: "score_total", value, .. } => self.score_total = *value as i64,
            _ => {}
        }
    }

    fn state(&self) -> ModeState {
        match self.data_state {
            data_state::COMPLETED => ModeState::Won,
            data_state::FAILED => ModeState::Lost,
            _ => ModeState::Running,
        }
    }

    fn score(&self) -> i64 {
        self.score_total
    }

    /// `GetStarsFromScore`; a lost run records no stars (`AddCurrentEventStarCompletion` only runs for state 2).
    fn stars(&self) -> u8 {
        match self.state() {
            ModeState::Won => stars_from_score(false, self.score_total, self.def.stars),
            _ => 0,
        }
    }
}

// ------------------------------------------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const DIR: &str = "xml/gameplay/eventdef_episode04/eventdef_episode04_event02_stage00.xml";

    fn load(rel: &str) -> Option<String> {
        let p = Path::new(ASSETS292).join(rel);
        if !p.exists() {
            eprintln!("skip: {} missing", p.display());
            return None;
        }
        std::fs::read_to_string(p).ok()
    }

    #[test]
    fn parse_real_slalom_eventdefs() {
        let Some(xml) = load(DIR) else { return };
        let d = SlalomEventDef::from_xml(&xml).unwrap();
        assert_eq!(d.game_mode_name, "SLALOM");
        assert_eq!(d.timers, [110.59, 98.52, 89.32, 82.0, 75.0]);
        assert_eq!(d.stars, [60000, 115000, 125000]);
        assert_eq!(d.gates.len(), 12);
        let g0 = &d.gates[0];
        assert_eq!(g0.spline, "centre_001");
        assert_eq!((g0.cols, g0.rows), (1, 3));
        assert_eq!(g0.time_penalty, 5.0);
        assert_eq!(g0.separation, 8.0);
        assert_eq!(g0.col_spacing, 15.0);
        assert_eq!(g0.row_spacing, 8.0);
        assert_eq!(d.gates[3].separation, 11.0);
        assert!(d.coin_items > 20);
        // all nine slalom eventdefs parse
        let mut n = 0;
        for ev in ["event02", "event06", "event10"] {
            for st in 0..3 {
                let rel = format!("xml/gameplay/eventdef_episode04/eventdef_episode04_{ev}_stage0{st}.xml");
                let x = load(&rel).unwrap();
                let d = SlalomEventDef::from_xml(&x).unwrap();
                assert!(!d.gates.is_empty());
                assert!(d.timers[0] > d.timers[4]);
                assert!(d.stars[0] <= d.stars[1] && d.stars[1] <= d.stars[2]);
                n += 1;
            }
        }
        assert_eq!(n, 9);
    }

    #[test]
    fn rejects_other_modes() {
        let x = "<EventDefinition><GameMode name=\"RACE\"/></EventDefinition>";
        assert!(SlalomEventDef::from_xml(x).is_err());
    }

    #[test]
    fn defaults_when_attributes_missing() {
        let x = "<EventDefinition><GameMode name=\"SLALOM\"/><Difficulty level=\"0.1\"/><TrackItem helpername=\"slalom_gate\" spline=\"a\"/></EventDefinition>";
        let d = SlalomEventDef::from_xml(x).unwrap();
        assert_eq!(d.timers, DEFAULT_TIMERS);
        let g = &d.gates[0];
        assert_eq!((g.separation, g.time_penalty, g.cols, g.rows, g.col_spacing, g.row_spacing), (8.0, 5.0, 3, 3, 10.0, 10.0));
        assert_eq!(g.post_lateral_offsets(), (-4.0, 4.0));
    }

    #[test]
    fn gate_classification() {
        let l = Vec3::new(-4.0, 0.0, 10.0);
        let r = Vec3::new(4.0, 0.0, 10.0);
        assert_eq!(classify_gate(l, r, Vec3::new(0.0, 0.0, 10.0)), GateResult::Clean);
        assert_eq!(classify_gate(l, r, Vec3::new(3.0, 0.5, 10.0)), GateResult::Clean);
        assert_eq!(classify_gate(l, r, Vec3::new(-6.0, 0.0, 10.0)), GateResult::Missed);
        assert_eq!(classify_gate(l, r, Vec3::new(6.0, 0.0, 10.0)), GateResult::Missed);
    }

    fn mode_with_gates() -> SlalomMode {
        let xml = load(DIR).unwrap_or_else(|| {
            "<EventDefinition><GameMode name=\"SLALOM\"/><Difficulty timer_veryeasy=\"60\" timer_easy=\"50\" timer_medium=\"40\" timer_hard=\"30\" timer_impossible=\"20\"/><Stars Star1=\"1\" Star2=\"2\" Star3=\"3\"/><TrackItem helpername=\"slalom_gate\" spline=\"c\"/></EventDefinition>".to_string()
        });
        let def = SlalomEventDef::from_xml(&xml).unwrap();
        let mut m = SlalomMode::new(def.clone(), DIFFICULTY_MEDIUM);
        for (i, g) in def.gates.iter().take(2).enumerate() {
            m.add_gate(g.clone(), Vec3::new(-4.0, 0.0, 100.0 * (i + 1) as f32), Vec3::new(4.0, 0.0, 100.0 * (i + 1) as f32), 10.0 * (i + 1) as f32);
        }
        m
    }

    #[test]
    fn clean_and_missed_gate_flow() {
        let mut m = mode_with_gates();
        let t0 = m.time_left();
        let mut fx = Vec::new();
        let mut ev = Vec::new();
        let mut k = KartState { is_player: true, race_time: 1.0, race_position: 1, ..Default::default() };
        // before the first gate: nothing happens
        m.set_car_probe(SlalomCarProbe { spline_pos: 5.0, ..Default::default() });
        k.pos = Vec3::new(0.0, 0.0, 90.0);
        m.update(0.1, &[k.clone()], &mut fx, &mut ev);
        assert!(!m.gates[0].passed);
        assert!((m.time_left() - (t0 - 0.1)).abs() < 1e-4);
        // pass gate 0 in the middle -> clean, no time change beyond dt
        m.set_car_probe(SlalomCarProbe { spline_pos: 10.5, ..Default::default() });
        k.pos = Vec3::new(1.0, 0.0, 100.0);
        m.update(0.1, &[k.clone()], &mut fx, &mut ev);
        assert!(m.gates[0].passed);
        assert_eq!(m.gates_cleaned_stat, 1);
        assert_eq!(m.msg_type, 1);
        assert!((m.time_left() - (t0 - 0.2)).abs() < 1e-4);
        assert!(fx.is_empty());
        // pass gate 1 outside -> penalty + TNT request
        m.set_car_probe(SlalomCarProbe { spline_pos: 20.5, ..Default::default() });
        k.pos = Vec3::new(9.0, 0.0, 200.0);
        let pen = m.gates[1].def.time_penalty;
        m.update(0.1, &[k.clone()], &mut fx, &mut ev);
        assert!(m.gates[1].passed && m.missed_any);
        assert!((m.time_left() - (t0 - 0.3 - pen)).abs() < 1e-3);
        assert!(matches!(fx[0], CarEffect::Other { tag: "SlalomTnTSpawn", .. }));
        assert_eq!(m.msg_type, 2);
        // voice line fires 0.75 s later
        for _ in 0..8 {
            m.update(0.1, &[k.clone()], &mut fx, &mut ev);
        }
        assert_eq!(m.msg_type, 0);
        assert!(ev.iter().any(|e| matches!(e, ModeEvent::Other { tag: "slalom_voice", value } if *value == 0.0)));
    }

    #[test]
    fn no_gate_evaluation_without_time() {
        let mut m = mode_with_gates();
        m.time_left = 0.0;
        m.set_car_probe(SlalomCarProbe { spline_pos: 99.0, ..Default::default() });
        let k = KartState { is_player: true, race_time: 1.0, ..Default::default() };
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update_gates(0.1, &k, None, &mut fx, &mut ev);
        assert!(!m.gates[0].passed);
    }

    #[test]
    fn finish_win_lose_bonus_and_stars() {
        let mut m = mode_with_gates();
        m.time_left = 12.9;
        let mut ev = Vec::new();
        assert_eq!(m.bonus_coins(), 120);
        m.score_total = 130000;
        m.on_input(&ModeInput::KartFinished { car: 0, time: 80.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Won);
        assert_eq!(m.stars(), stars_from_score(false, 130000, m.def.stars));
        let mut m2 = mode_with_gates();
        m2.time_left = -0.5;
        assert_eq!(m2.bonus_coins(), 0);
        m2.on_input(&ModeInput::KartFinished { car: 0, time: 80.0 }, &mut ev);
        assert_eq!(m2.state(), ModeState::Lost);
        assert_eq!(m2.stars(), 0);
        // time exactly 0 still counts as completed
        let mut m3 = mode_with_gates();
        m3.time_left = 0.0;
        m3.on_input(&ModeInput::KartFinished { car: 0, time: 80.0 }, &mut ev);
        assert_eq!(m3.state(), ModeState::Won);
    }

    #[test]
    fn stars_from_score_thresholds() {
        let s = [60000, 115000, 125000];
        assert_eq!(stars_from_score(true, 999999, s), 0);
        assert_eq!(stars_from_score(false, 0, s), 1);
        assert_eq!(stars_from_score(false, 115000, s), 1);
        assert_eq!(stars_from_score(false, 115001, s), 2);
        assert_eq!(stars_from_score(false, 125000, s), 2);
        assert_eq!(stars_from_score(false, 125001, s), 3);
    }

    #[test]
    fn low_time_warning_once() {
        let mut m = mode_with_gates();
        m.time_left = 5.05;
        let k = KartState { is_player: true, race_time: 1.0, ..Default::default() };
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update(0.1, &[k.clone()], &mut fx, &mut ev); // 5.05 -> 4.95 (flag set next frame: check is before the decrement)
        assert!(!m.low_time_warning);
        m.update(0.1, &[k.clone()], &mut fx, &mut ev);
        assert!(m.low_time_warning);
        let n = ev.iter().filter(|e| matches!(e, ModeEvent::Message { tag } if tag == "ui_event:0x1f")).count();
        assert_eq!(n, 1);
        m.update(0.1, &[k], &mut fx, &mut ev);
        let n = ev.iter().filter(|e| matches!(e, ModeEvent::Message { tag } if tag == "ui_event:0x1f")).count();
        assert_eq!(n, 1);
    }

    struct Straight;
    impl SplineProbe for Straight {
        fn position_ahead(&self, from: f32, d: f32) -> Vec3 {
            Vec3::new(0.0, 0.0, from + d)
        }
        fn closest_spline_pos(&self, p: Vec3) -> f32 {
            p.z
        }
        fn left_width(&self, _: f32) -> f32 {
            -20.0
        }
        fn right_width(&self, _: f32) -> f32 {
            20.0
        }
        fn right_vector(&self, _: f32) -> Vec3 {
            Vec3::X
        }
        fn forward_vector(&self, _: f32) -> Vec3 {
            Vec3::Z
        }
        fn ground_below(&self, p: Vec3) -> Option<Vec3> {
            Some(Vec3::new(p.x, 0.0, p.z))
        }
    }

    #[test]
    fn tnt_layout_matches_original_arithmetic() {
        let def = SlalomGateDef {
            spline: "c".into(),
            fraction_along_spline: 0.0,
            lateral_offset_from_spline: 0.0,
            height_above_spline: 0.0,
            separation: 8.0,
            time_penalty: 5.0,
            cols: 1,
            rows: 3,
            col_spacing: 15.0,
            row_spacing: 8.0,
        };
        let car = SlalomCarProbe { spline_pos: 100.0, lateral_offset: 2.0, lookahead_param: 20.0 };
        let v = tnt_crate_positions(&def, &car, &Straight);
        assert_eq!(v.len(), 3);
        // base z = 100 + 20*1.5 = 130; col offset (0 - 0.5)*15 = -7.5; lateral 2 + (row-1.5)*8
        for (row, p) in v.iter().enumerate() {
            assert!((p.z - (130.0 - 7.5)).abs() < 1e-4);
            assert!((p.x - (2.0 + (row as f32 - 1.5) * 8.0)).abs() < 1e-4);
            assert!((p.y - 5.0).abs() < 1e-4);
        }
        // right-edge clamp: right_width 20 - 1.5*8 = 8 < lateral 15 -> lateral becomes 8
        let car2 = SlalomCarProbe { lateral_offset: 15.0, ..car };
        let v2 = tnt_crate_positions(&def, &car2, &Straight);
        assert!((v2[0].x - (8.0 + (0.0 - 1.5) * 8.0)).abs() < 1e-4);
    }

    #[test]
    fn missed_gate_with_world_spawns_named_tnt_and_uses_live_spline_pos() {
        let mut m = mode_with_gates();
        // stored spline pos is bogus (1e9) but the live query (Straight: closest pos = z) says the gate is at z = 100
        m.gates[0].spline_pos = 1.0e9;
        m.set_car_probe(SlalomCarProbe { spline_pos: 100.5, lateral_offset: 0.0, lookahead_param: 10.0 });
        let k = KartState { is_player: true, race_time: 1.0, pos: Vec3::new(50.0, 0.0, 100.0), ..Default::default() };
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update_with_world(0.1, &[k], Some(&Straight), &mut fx, &mut ev);
        assert!(m.gates[0].passed && m.missed_any);
        let spawns: Vec<_> = fx.iter().filter_map(|e| if let CarEffect::SpawnObject { kind, .. } = e { Some(kind.clone()) } else { None }).collect();
        assert_eq!(spawns.len() as i32, m.gates[0].def.cols * m.gates[0].def.rows);
        assert!(spawns.iter().all(|k| k == "smck_TNT_Box_3m"));
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Particle { name, .. } if name == "KingSlingSpawnIn")));
        assert_eq!(tnt_instance_name(2, 0, 1), "Slalom TnT Gate (2) Col (0) Row (1)");
    }

    #[test]
    fn gate_post_colours_alternate_starting_blue() {
        let p0 = SlalomGateDef::post_helpers(0);
        let p1 = SlalomGateDef::post_helpers(1);
        assert!(!p0.red && p1.red);
        assert_eq!(p0.left_helper, "slalom_post_blue_left");
        assert_eq!(p1.right_helper, "slalom_post_red_right");
        assert_eq!(p0.left_smackable.1, 0x6d);
        assert_eq!(p0.right_smackable.1, 0x71);
        assert_eq!(p1.left_smackable.1, 0x75);
        assert_eq!(p1.right_smackable.1, 0x79);
        assert!(!SlalomGateDef::post_helpers(2).red);
        assert_eq!(MAX_COINS_PER_GATE, 64);
    }
}
