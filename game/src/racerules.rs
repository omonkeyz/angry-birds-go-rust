//! Race rules of Angry Birds Go! 2.9.1: game modes, event definitions, race splines + start/finish lines, per-kart race
//! progress, ranking, finish handling per mode, score / stars / rewards.
//!
//! Standalone: depends only on `std`, `glam`, `roxmltree` and (for `RaceTrack::from_stm`) `abgtool::stm`. No dependency on
//! `drive.rs` / `game.rs` / `carsim.rs`.
//!
//! Sources (Ghidra addresses of `libABK291.so`, file offset = address - 0x10000; decompile `libABK291_annotated.c`; where the
//! ARM VFP decompile lost arguments the headless-Ghidra disassembly listing was used, marked "(listing)"):
//!
//! * `CEventDefinitionManager::StringToGameMode @000fa1bc` (+ clone @000a3f90), `ReadEventDefinition @00101xxx` (the
//!   `<Difficulty>/<Stars>/<Spline>/<DifficultyAdjust>` reader, defaults and the star monotonic fix-up),
//!   `GetTimerDuration @000fee20`, `GetTokenThreshold @000fee2c`, `GetSplineOverride @000fa3f8`, `GetStarsFromScore @000fee38`
//! * `CMetagameManager::GetDifficultyAdjust @0012df90`, `GetRaceMaxScore @0012df04`, `CalculateScoreFromCC @0012df14`
//! * `CGame::CalcDifficultyAdjustEnum @0011b800`, `CalculateRacePositions @001191e4` (+ `TCarSortData_Comparator @00111f48`)
//! * `CGame::SetupEnvironmentSplines @0011caf8`, `SetupEnvironmentMarkup @0011db98` (start/finish/trackend/smash helper -> spline
//!   distances; the helper globals were resolved with Ghidra symbol names: `s_pFinishHelper` -> spline+0x34/+0x38,
//!   `s_pStartHelper` -> +0x3c, `s_pTrackEndHelper` -> +0x40, `s_pSmashHelper` -> +0x44) (listing)
//! * `CSpline::CSpline @0018cbb0`, `GetPosition/GetHeight/GetOffset/Lookahead/GetSafePos/GetLeftWidth/GetRightWidth/
//!   GetMinLeftWidth/GetLateralOffset/GetInfo/GetSlope/GetSignedDistanceAlongSplineFromRacePos/GetClosestNode/
//!   GetClosestSplinePos/GetNewPos/GetSplinePosFromDistance` (0018d4c0 .. 0018fc68)
//! * `CCar::GetRaceTotalSplineDist @001a43e8`, `GetRaceTotalSplineDistFromStart @001a444c`, `SetFinishLineCrossed @001aa158`,
//!   `SetRaceCompleted @001aa0a4`, `Respawn @001a5694` (grid placement, listing)
//! * `CGameMode::ProcessUpdate @0017caac`, `CheckFinishLineCrossed @0017c684`, `UpdateGameEnd @0017c808`,
//!   `CGameModeData::Update @0017c450` and the per-mode `GetEventCompleted / GetBonusCoins / Update / CheckGameOverCondition`
//!   (Race @00185094, TimeAttack @0018698c, Versus @00186de8, SeedRush @0017ead4, BossBattle @0017d9ec, Intro @0017f658, Slalom @001854a4, LMR @0018334c)
//! * `CGameModeManager::Update @00184c98`, `CheckGameOverCondition @00184f08`, `InitialiseCars @00183a08` (AI count / skill)
//! * `CGame::Process @00115cb0` (game state machine: 5 grid / slingshot, 8 racing, 9 game end) (listing)
//! * `CScoreSystem` + `CScoreCounter*` (scoring: `GetScore @001f4288`, `GetBonusScore @001f43bc`, position / time / fruit / deaths /
//!   top-speed / acceleration formulas) (listing), `scoreconfig.xml`, `economy.xml`, `eventdefinitiondata.xml`.
//!
//! # Facts established (they contradict the task text, see the report)
//! * Stars are decided from the SCORE (`CScoreSystem::GetScore() + GetBonusScore()`, not milliseconds). The results screen
//!   (`CResultsScreen @004351bc`) uses `score >= Star1/2/3` (non-strict) and stores them (`SetCurrentEventStars`) only for a successful
//!   event; `GetStarsFromScore @000fee38` (strict `>`, floor 1) is the challenge-code variant. Score = top-speed + acceleration +
//!   deaths counters + the finishing counter `ceil100((9-position)/8 * campaignCC*150)` (position / time / fruit, `GetBonusScore`).
//!   Data check over the 55 campaign events: the tutorial events C01/C02 have thresholds 0.5/0.75/1.0 x cc*150 (finishing bonus alone
//!   decides), later events 1.0-2.2 x cc*150 (the extra counters matter).
//! * There are no laps. Ranking sorts on the float spline parameter (`car+0x1a8c` = node index + fraction), descending.
//! * Finish: `dist(car) > spline+0x38` (or the car is past the plane of the closest node to `s_pFinishHelper`).
//!
//! # UNRESOLVED (also marked in code)
//! * `CGameModeSlalom::Update @00185a04` (gate scoring / timer bonus) and BossBattle abilities / health: not ported (rules treat them
//!   as Race / TimeAttack respectively; the caller can `force_end`).
//! * The multiplayer countdown 3-2-1-GO (`ms_fCountDownTime`, HUD) and the intro camera fly-by timing (game state 6).
//! * Score counters whose inputs are physics telemetry (`TopSpeed`/`Acceleration` time buckets, deaths): the formulas are ported,
//!   the stats are supplied by the caller (`ScoreInputs`); which `CCar` flag `+0x4c8` gates the acceleration timer is unknown.
//! * `CCar::Update` spline-id switching between parallel race splines is approximated (see `SplineTracker::update`).
//! * Grid: asymmetric-corridor branch of `Respawn` (`0x1a6278 / 0x1a663c`) not dumped; clearance test uses the supplied radii.
//! * `tracktimes.xml` (time-attack target/ghost times) does not exist in the shipped data; `TIME_ATTACK` uses `timer_*` from the
//!   eventdef (`CGameModeTimeAttackData`), no ghost.
#![allow(dead_code, clippy::too_many_arguments, clippy::needless_range_loop)]

use glam::Vec3;
use std::path::Path;

// =============================================================================================================
// Small helpers (C semantics)
// =============================================================================================================

/// C `atoi`: leading integer, 0 when there is none.
pub fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let mut end = 0;
    for (i, c) in t.char_indices() {
        if (i == 0 && (c == '-' || c == '+')) || c.is_ascii_digit() {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    t[..end].parse::<i32>().unwrap_or(0)
}

/// C `strtod`: longest numeric prefix (the shipped files contain values like `1.0f`).
pub fn atof(s: &str) -> f32 {
    let t = s.trim();
    let mut end = 0;
    let b = t.as_bytes();
    let mut seen_dot = false;
    let mut seen_exp = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_digit() || ((c == '-' || c == '+') && (i == 0 || b[i - 1] == b'e' || b[i - 1] == b'E')) {
            end = i + 1;
        } else if c == '.' && !seen_dot && !seen_exp {
            seen_dot = true;
            end = i + 1;
        } else if (c == 'e' || c == 'E') && !seen_exp && end > 0 {
            seen_exp = true;
        } else {
            break;
        }
        i += 1;
    }
    t[..end].trim_end_matches(['e', 'E']).parse::<f32>().unwrap_or(0.0)
}

fn attr_f32(n: roxmltree::Node, name: &str) -> Option<f32> {
    n.attribute(name).map(atof)
}
fn attr_i32(n: roxmltree::Node, name: &str) -> Option<i32> {
    n.attribute(name).map(atoi)
}
fn child<'a, 'b>(n: roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}
fn child_f32(n: roxmltree::Node, name: &str) -> Option<f32> {
    child(n, name).and_then(|c| c.text()).map(atof)
}

/// Minimal tolerant XML tree. The original reader (`CXGSXmlReader`) accepts what roxmltree rejects and what the shipped event
/// definitions contain: attribute names that start with a digit (`<Stars 1="1000" 2="20000" 3="25000"/>` in the VERSUS / INTRO dummies)
/// and attributes repeated on one element (`<TrackItem spline=.. spline=..>`). Lookups return the FIRST attribute of a name.
#[derive(Clone, Debug, Default)]
pub struct XNode {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
    pub text: String,
}

impl XNode {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn child(&self, name: &str) -> Option<&XNode> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn attr_f32(&self, name: &str) -> Option<f32> {
        self.attr(name).map(atof)
    }
    pub fn attr_i32(&self, name: &str) -> Option<i32> {
        self.attr(name).map(atoi)
    }
    pub fn child_f32(&self, name: &str) -> Option<f32> {
        self.child(name).map(|c| atof(c.text.trim()))
    }

    pub fn parse(xml: &str) -> Result<XNode, String> {
        fn unescape(s: &str) -> String {
            if !s.contains('&') {
                return s.to_string();
            }
            s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
        }
        let b = xml.as_bytes();
        let mut i = 0usize;
        let mut stack: Vec<XNode> = Vec::new();
        let mut root: Option<XNode> = None;
        while i < b.len() {
            if b[i] != b'<' {
                let j = xml[i..].find('<').map_or(b.len(), |k| i + k);
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&unescape(&xml[i..j]));
                }
                i = j;
                continue;
            }
            if xml[i..].starts_with("<!--") {
                i = xml[i..].find("-->").map(|k| i + k + 3).ok_or("unterminated comment")?;
                continue;
            }
            if xml[i..].starts_with("<?") || xml[i..].starts_with("<!") {
                i = xml[i..].find('>').map(|k| i + k + 1).ok_or("unterminated declaration")?;
                continue;
            }
            if xml[i..].starts_with("</") {
                let end = xml[i..].find('>').map(|k| i + k).ok_or("unterminated end tag")?;
                let node = stack.pop().ok_or("unbalanced end tag")?;
                i = end + 1;
                match stack.last_mut() {
                    Some(p) => p.children.push(node),
                    None => root = Some(node),
                }
                continue;
            }
            // start tag
            i += 1;
            let ns = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
                i += 1;
            }
            let mut node = XNode { name: xml[ns..i].to_string(), ..Default::default() };
            let mut self_closing = false;
            loop {
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= b.len() {
                    return Err("unterminated start tag".into());
                }
                if b[i] == b'>' {
                    i += 1;
                    break;
                }
                if b[i] == b'/' {
                    self_closing = true;
                    i += 1;
                    continue;
                }
                let ks = i;
                while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
                    i += 1;
                }
                let key = xml[ks..i].to_string();
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < b.len() && b[i] == b'=' {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                        let q = b[i];
                        i += 1;
                        let vs = i;
                        while i < b.len() && b[i] != q {
                            i += 1;
                        }
                        node.attrs.push((key, unescape(&xml[vs..i.min(b.len())])));
                        i += 1;
                    } else {
                        let vs = i;
                        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
                            i += 1;
                        }
                        node.attrs.push((key, xml[vs..i].to_string()));
                    }
                } else {
                    node.attrs.push((key, String::new()));
                }
            }
            if self_closing {
                match stack.last_mut() {
                    Some(p) => p.children.push(node),
                    None => root = Some(node),
                }
            } else {
                stack.push(node);
            }
        }
        root.ok_or_else(|| "no root element".to_string())
    }
}

/// `StringPartialMatchNoCase @002c0474`: `b` is a (case-insensitive) PREFIX of `a`.
pub fn partial_match(a: &str, b: &str) -> bool {
    a.len() >= b.len() && a.as_bytes()[..b.len()].eq_ignore_ascii_case(b.as_bytes())
}

/// Deterministic RNG (the original uses `ms_pDefaultThreadsafeRNG`; only the distribution matters). xorshift32.
#[derive(Clone, Debug)]
pub struct Rng(pub u32);
impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(if seed == 0 { 0x9e37_79b9 } else { seed })
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// [0,1) (the original multiplies a u32 by `s_iRandomOneOverNumber` = 2^-32).
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() as f64 * (1.0 / 4294967296.0)) as f32
    }
    /// `rng->vtbl+0x1c(min, max)` = `min + unit * (max - min)`.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + self.unit() * (max - min)
    }
}

// =============================================================================================================
// Game modes
// =============================================================================================================

/// `EGameMode` (`StringToGameMode @000fa1bc`, `CGameMode::CreateGameMode @0017d1b4`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GameMode {
    Intro = 0,
    Intro2 = 1,
    Intro3 = 2,
    Race = 4,
    Versus = 5,
    TimeAttack = 6,
    SeedRush = 7,
    /// quick match (online); `CreateGameMode` returns null for it
    Qmr = 8,
    /// tournament match (online); `CreateGameMode` returns null
    Tmr = 9,
    Jenga = 10,
    BossBattle = 11,
    BossFruitRush = 12,
    Slalom = 13,
    /// local (same-device) multiplayer race
    Lmr = 14,
    Unknown = 15,
}

impl GameMode {
    /// `CEventDefinitionManager::StringToGameMode`.
    pub fn from_name(s: &str) -> GameMode {
        match s {
            "TIME_ATTACK" => GameMode::TimeAttack,
            "VERSUS" => GameMode::Versus,
            "RACE" => GameMode::Race,
            "INTRO" => GameMode::Intro,
            "INTRO2" => GameMode::Intro2,
            "INTRO3" => GameMode::Intro3,
            "SEED_RUSH" => GameMode::SeedRush,
            "BOSS_BATTLE" => GameMode::BossBattle,
            "BOSS_FRUIT_RUSH" => GameMode::BossFruitRush,
            "SLALOM" => GameMode::Slalom,
            "QMR" => GameMode::Qmr,
            "TMR" => GameMode::Tmr,
            "LMR" => GameMode::Lmr,
            "JENGA" => GameMode::Jenga,
            _ => GameMode::Unknown,
        }
    }
    pub fn id(self) -> i32 {
        self as i32
    }
    /// `CGameMode::CreateGameMode` returns a mode object (false for QMR / TMR / unknown: they are network modes).
    pub fn creatable_offline(self) -> bool {
        !matches!(self, GameMode::Qmr | GameMode::Tmr | GameMode::Unknown)
    }
    /// Modes whose finish is by crossing the finish line (every mode without a per-car `CheckGameOverCondition` override).
    pub fn finishes_on_line(self) -> bool {
        !matches!(self, GameMode::Jenga | GameMode::Unknown)
    }
    /// Modes with a countdown timer in the per-car data (`CGameModeTimeAttackData`, `CGameModeSlalomData`).
    pub fn has_timer(self) -> bool {
        matches!(self, GameMode::TimeAttack | GameMode::Slalom)
    }
}

/// `EDifficultyAdjust`: 0 VeryEasy .. 4 Impossible.
pub const DIFFICULTY_NAMES: [&str; 5] = ["VeryEasy", "Easy", "Medium", "Hard", "Impossible"];

// =============================================================================================================
// Event definition (eventdef_*.xml)
// =============================================================================================================

#[derive(Clone, Debug, PartialEq)]
pub struct SplineWeight {
    pub name: String,
    pub min_ai_weighting: f32,
    pub max_ai_weighting: f32,
}

/// `<Difficulty ...>` with the defaults of the reader (`ReadEventDefinition`).
#[derive(Clone, Debug)]
pub struct Difficulty {
    /// `level` (default 0.5, `this+0x248`)
    pub level: f32,
    /// `level_index` (default 0)
    pub level_index: i32,
    /// `ai_upgrade` (default -1)
    pub ai_upgrade: i32,
    /// `timer_veryeasy .. timer_impossible` defaults 60, 47.5, 35, 22.5, 10 (`this+0x250..0x260`)
    pub timer: [f32; 5],
    /// `bosslevel` (default 1, `this+0x264`)
    pub boss_level: i32,
    /// `seed_amount_*` defaults 60, 47, 35, 22, 10 (`this+0x268..0x278`)
    pub seed_amount: [i32; 5],
    pub tier: Option<i32>,
    pub boss_bonus: Option<f32>,
}
impl Default for Difficulty {
    fn default() -> Self {
        Difficulty {
            level: 0.5,
            level_index: 0,
            ai_upgrade: -1,
            timer: [60.0, 47.5, 35.0, 22.5, 10.0],
            boss_level: 1,
            seed_amount: [60, 47, 35, 22, 10],
            tier: None,
            boss_bonus: None,
        }
    }
}

/// `<TrackItem ...>` (only what the AI / rules need; `items.rs` parses the full list itself).
#[derive(Clone, Debug)]
pub struct TrackItemRef {
    pub helper_name: String,
    pub spline: String,
    pub fraction_along_spline: f32,
    pub lateral_offset: f32,
}

#[derive(Clone, Debug)]
pub struct EventDef {
    pub tag: String,
    pub title: String,
    pub description: String,
    /// `environments\theme003\tracks\run000`
    pub env_path: String,
    pub theme: i32,
    pub run: i32,
    pub config: i32,
    pub character: String,
    pub mode_name: String,
    pub mode: GameMode,
    /// `kartcount` (`this+0x23c`, -1 = absent)
    pub kart_count: Option<i32>,
    /// `fruit_rush_mode="ICECREAM"` -> true (`this+0x244`)
    pub fruit_rush_icecream: bool,
    pub height_above_spline: Option<f32>,
    pub difficulty: Difficulty,
    /// `<Stars Star1 Star2 Star3>` after the reader's monotonic fix-up (`this+0x27c..0x284`)
    pub stars: [i32; 3],
    pub splines: Vec<SplineWeight>,
    /// `<DifficultyAdjust>` defaults = debug floats 0xa9..0xad = 0, 0.25, 0.5, 0.75, 1.0 (`this+0x590..0x5a0`)
    pub difficulty_adjust: [f32; 5],
    pub track_item_count: usize,
    pub hotspots: Vec<TrackItemRef>,
}

impl EventDef {
    pub fn parse(xml: &str) -> Result<EventDef, String> {
        let root = XNode::parse(xml).map_err(|e| format!("eventdef xml: {e}"))?;
        let mut ev = EventDef {
            tag: String::new(),
            title: String::new(),
            description: String::new(),
            env_path: String::new(),
            theme: 0,
            run: 0,
            config: 0,
            character: String::new(),
            mode_name: String::new(),
            mode: GameMode::Unknown,
            kart_count: None,
            fruit_rush_icecream: false,
            height_above_spline: None,
            difficulty: Difficulty::default(),
            stars: [0; 3],
            splines: Vec::new(),
            difficulty_adjust: [0.0, 0.25, 0.5, 0.75, 1.0],
            track_item_count: 0,
            hotspots: Vec::new(),
        };
        if let Some(n) = root.child("Name") {
            ev.tag = n.attr("tag").unwrap_or("").to_string();
            ev.title = n.attr("title").unwrap_or("").to_string();
            ev.description = n.attr("description").unwrap_or("").to_string();
        }
        if let Some(n) = root.child("Environment") {
            ev.env_path = n.attr("pathname").unwrap_or("").to_string();
            ev.config = n.attr_i32("config").unwrap_or(0);
            // sscanf("environments\\theme%d\\tracks\\run%d")
            let p = ev.env_path.replace('/', "\\");
            if let Some(rest) = p.strip_prefix("environments\\theme") {
                let mut it = rest.splitn(2, '\\');
                ev.theme = atoi(it.next().unwrap_or("0"));
                if let Some(r) = it.next().and_then(|r| r.strip_prefix("tracks\\run")) {
                    ev.run = atoi(r);
                }
            }
        }
        if let Some(n) = root.child("Character") {
            ev.character = n.attr("name").unwrap_or("").to_string();
        }
        if let Some(n) = root.child("GameMode") {
            ev.mode_name = n.attr("name").unwrap_or("").to_string();
            ev.mode = GameMode::from_name(&ev.mode_name);
            ev.kart_count = n.attr_i32("kartcount");
            // strcmp FRUIT -> 0, ICECREAM -> 1, anything else leaves the default 0
            ev.fruit_rush_icecream = n.attr("fruit_rush_mode") == Some("ICECREAM");
            ev.height_above_spline = n.attr_f32("heightabovespline");
        }
        if let Some(n) = root.child("Difficulty") {
            let d = &mut ev.difficulty;
            if let Some(v) = n.attr_f32("level") {
                d.level = v;
            }
            if let Some(v) = n.attr_i32("level_index") {
                d.level_index = v;
            }
            if let Some(v) = n.attr_i32("ai_upgrade") {
                d.ai_upgrade = v;
            }
            for (i, k) in ["timer_veryeasy", "timer_easy", "timer_medium", "timer_hard", "timer_impossible"].iter().enumerate() {
                if let Some(v) = n.attr_f32(k) {
                    d.timer[i] = v;
                }
            }
            if let Some(v) = n.attr_i32("bosslevel") {
                d.boss_level = v;
            }
            for (i, k) in ["seed_amount_veryeasy", "seed_amount_easy", "seed_amount_medium", "seed_amount_hard", "seed_amount_impossible"]
                .iter()
                .enumerate()
            {
                if let Some(v) = n.attr_i32(k) {
                    d.seed_amount[i] = v;
                }
            }
            d.tier = n.attr_i32("tier");
            d.boss_bonus = n.attr_f32("boss_bonus");
        }
        if let Some(n) = root.child("Stars") {
            // `Star1/Star2/Star3`; an eventdef that uses `1=` `2=` `3=` (VERSUS / INTRO dummies) keeps 0 (the reader never looks at them)
            let a = n.attr_i32("Star1").unwrap_or(0);
            let mut b = n.attr_i32("Star2").unwrap_or(0);
            let mut c = n.attr_i32("Star3").unwrap_or(0);
            // monotonic fix-up after the reads (`ReadEventDefinition`): if (a==0&&b==0) { if c==0 skip } else if b<a { b=a }; if c<b { c=b }
            if !(a == 0 && b == 0 && c == 0) {
                if !(a == 0 && b == 0) && b < a {
                    b = a;
                }
                if c < b {
                    c = b;
                }
            }
            ev.stars = [a, b, c];
        }
        for n in &root.children {
            match n.name.as_str() {
                "Spline" => ev.splines.push(SplineWeight {
                    name: n.attr("name").unwrap_or("").to_string(),
                    min_ai_weighting: n.attr_f32("min_ai_weighting").unwrap_or(0.0),
                    max_ai_weighting: n.attr_f32("max_ai_weighting").unwrap_or(0.0),
                }),
                "DifficultyAdjust" => {
                    for (i, k) in DIFFICULTY_NAMES.iter().enumerate() {
                        if let Some(v) = n.child_f32(k) {
                            ev.difficulty_adjust[i] = v;
                        }
                    }
                }
                "TrackItem" => {
                    ev.track_item_count += 1;
                    let h = n.attr("helpername").unwrap_or("");
                    if h.starts_with("ai_hotspot") {
                        ev.hotspots.push(TrackItemRef {
                            helper_name: h.to_string(),
                            spline: n.attr("spline").unwrap_or("").to_string(),
                            fraction_along_spline: n.attr_f32("fractionalongspline").unwrap_or(0.0),
                            lateral_offset: n.attr_f32("lateraloffsetfromspline").unwrap_or(0.0),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(ev)
    }

    /// `CEventDefinitionManager::GetTimerDuration(EDifficultyAdjust) @000fee20`.
    pub fn timer_duration(&self, adj: usize) -> f32 {
        self.difficulty.timer[adj.min(4)]
    }
    /// `GetTokenThreshold @000fee2c`: fruits to collect in SEED_RUSH.
    pub fn token_threshold(&self, adj: usize) -> i32 {
        self.difficulty.seed_amount[adj.min(4)]
    }
    /// `GetSplineOverride @000fa3f8` (first case-insensitive match).
    pub fn spline_override(&self, name: &str) -> Option<&SplineWeight> {
        self.splines.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }
    /// `CGame::CalcDifficultyAdjustFloat @0011b984` = `<DifficultyAdjust>[enum]`.
    pub fn difficulty_adjust_value(&self, adj: usize) -> f32 {
        self.difficulty_adjust[adj.min(4)]
    }
    /// Stars of the results screen (`CResultsScreen @004351bc` -> `CPlayerInfo::SetCurrentEventStars(this+0x204)`): NON-strict
    /// comparisons `score >= Star1 -> 1`, `>= Star2 -> 2`, `>= Star3 -> 3` (0 below Star1); the stars are only stored for a SUCCESSFUL
    /// event (`this+0x14c` = player state 2 or 5, not 3 / 6). `score` = `GetScore() + GetBonusScore()`.
    pub fn results_stars(&self, score: i32, success: bool) -> u8 {
        if !success {
            return 0;
        }
        if score >= self.stars[2] {
            3
        } else if score >= self.stars[1] {
            2
        } else if score >= self.stars[0] {
            1
        } else {
            0
        }
    }

    /// `CEventDefinitionManager::GetStarsFromScore @000fee38` (STRICT `>`; used by the challenge code, `CChallengeGet3Stars` /
    /// `CChallengeScore`, not by the results screen). `finished = false` is the "local player state == 1 (still racing)" case (0 stars).
    /// Once finished it returns 1 for every score <= Star2 (both Star1 branches return 1; listing-confirmed floor).
    pub fn stars_from_score(&self, score: i32, finished: bool) -> u8 {
        if !finished {
            return 0;
        }
        if self.stars[2] < score {
            3
        } else if self.stars[1] < score {
            2
        } else {
            1
        }
    }
    /// `GetStarThresholdScores @000feec0`.
    pub fn star_thresholds(&self) -> [i32; 3] {
        self.stars
    }
}

// =============================================================================================================
// Economy (economy.xml): difficulty adjust, AI skill, score-from-CC; scoreconfig.xml; eventdefinitiondata.xml
// =============================================================================================================

#[derive(Clone, Debug)]
pub struct Economy {
    /// `<Score fMultiplier fAddition fOne/Two/ThreeStarMultiplier>` -> `CalculateScoreFromCC @0012df14` (daily race / tournament only)
    pub score_multiplier: f32,
    pub score_addition: f32,
    pub star_multipliers: [f32; 3],
    /// `<DifficultyAdjust><Difficulty relativeCC>` VeryEasy..Extreme (`CMetagameManager+0x4708..0x4718`)
    pub relative_cc: [i32; 5],
    /// `<AISkill><Race/TimeAttack/FruitRush/BossBattle min max>`
    pub ai_skill_race: (f32, f32),
    pub ai_skill_time_attack: (f32, f32),
    pub ai_skill_fruit_rush: (f32, f32),
    pub ai_skill_boss_battle: (f32, f32),
    /// `SkillVariance` (defaults -0.05 / 0.05, `+0x4688/+0x468c`)
    pub skill_variance: (f32, f32),
    /// `SkillBase` per difficulty (`+0x4690 + 4*adj`)
    pub skill_base: [f32; 5],
    /// `SplineSwitching minCooldown maxCooldown` (defaults 3.0 / 10.0, `+0x4680/+0x4684`)
    pub spline_switch_cooldown: (f32, f32),
    /// `BossCatchupDifficulty` as an enum (`+0x46b4`, default 0): boss AI rubber-bands when `adj >= this`
    pub boss_catchup_difficulty: usize,
    /// `AICatchupDifficulty` (`+0x46b8`, default 4)
    pub ai_catchup_difficulty: usize,
    /// `<PlayerSkill><SpeedBoost speedBoost ccMultiplierMin ccMultiplierMax boostApplySpeed>` (`+0x46a4..0x46b0`)
    pub speed_boost: f32,
    pub cc_multiplier: (f32, f32),
}

impl Default for Economy {
    fn default() -> Self {
        Economy {
            score_multiplier: 150.0,
            score_addition: 0.0,
            star_multipliers: [1.0, 1.4, 1.9],
            relative_cc: [4, 0, -2, -4, -8],
            ai_skill_race: (0.0, 1.0),
            ai_skill_time_attack: (0.0, 1.0),
            ai_skill_fruit_rush: (0.0, 1.0),
            ai_skill_boss_battle: (0.0, 1.0),
            skill_variance: (-0.05, 0.05),
            skill_base: [0.0; 5],
            spline_switch_cooldown: (3.0, 10.0),
            boss_catchup_difficulty: 0,
            ai_catchup_difficulty: 4,
            speed_boost: 0.0,
            cc_multiplier: (1.0, 1.0),
        }
    }
}

fn difficulty_enum_from_trackselect(s: &str) -> Option<usize> {
    match s {
        "TRACKSELECT_VERYEASY" => Some(0),
        "TRACKSELECT_EASY" => Some(1),
        "TRACKSELECT_MEDIUM" => Some(2),
        "TRACKSELECT_HARD" => Some(3),
        "TRACKSELECT_EXTREME" => Some(4),
        _ => None,
    }
}

impl Economy {
    pub fn parse(xml: &str) -> Result<Economy, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| format!("economy xml: {e}"))?;
        let root = doc.root_element();
        let mut e = Economy::default();
        if let Some(n) = child(root, "Score") {
            e.score_multiplier = attr_f32(n, "fMultiplier").unwrap_or(e.score_multiplier);
            e.score_addition = attr_f32(n, "fAddition").unwrap_or(e.score_addition);
            e.star_multipliers = [
                attr_f32(n, "fOneStarMultiplier").unwrap_or(1.0),
                attr_f32(n, "fTwoStarMultiplier").unwrap_or(1.4),
                attr_f32(n, "fThreeStarMultiplier").unwrap_or(1.9),
            ];
        }
        if let Some(n) = child(root, "DifficultyAdjust") {
            for d in n.children().filter(|c| c.is_element() && c.tag_name().name() == "Difficulty") {
                if let (Some(i), Some(v)) = (d.attribute("value").and_then(difficulty_enum_from_trackselect), attr_i32(d, "relativeCC")) {
                    e.relative_cc[i] = v;
                }
            }
        }
        if let Some(n) = child(root, "PlayerSkill").and_then(|p| child(p, "SpeedBoost")) {
            e.speed_boost = attr_f32(n, "speedBoost").unwrap_or(0.0);
            e.cc_multiplier = (attr_f32(n, "ccMultiplierMin").unwrap_or(1.0), attr_f32(n, "ccMultiplierMax").unwrap_or(1.0));
        }
        if let Some(a) = child(root, "AISkill") {
            let mm = |name: &str, def: (f32, f32)| {
                child(a, name).map(|n| (attr_f32(n, "min").unwrap_or(def.0), attr_f32(n, "max").unwrap_or(def.1))).unwrap_or(def)
            };
            e.ai_skill_race = mm("Race", e.ai_skill_race);
            e.ai_skill_time_attack = mm("TimeAttack", e.ai_skill_time_attack);
            e.ai_skill_fruit_rush = mm("FruitRush", e.ai_skill_fruit_rush);
            e.ai_skill_boss_battle = mm("BossBattle", e.ai_skill_boss_battle);
            e.skill_variance = mm("SkillVariance", e.skill_variance);
            if let Some(b) = child(a, "SkillBase") {
                for d in b.children().filter(|c| c.is_element() && c.tag_name().name() == "Difficulty") {
                    if let (Some(i), Some(v)) = (d.attribute("value").and_then(difficulty_enum_from_trackselect), attr_f32(d, "skillBase")) {
                        e.skill_base[i] = v;
                    }
                }
            }
            if let Some(n) = child(a, "SplineSwitching") {
                e.spline_switch_cooldown = (attr_f32(n, "minCooldown").unwrap_or(3.0), attr_f32(n, "maxCooldown").unwrap_or(10.0));
            }
            if let Some(v) = child(a, "BossCatchupDifficulty").and_then(|n| n.attribute("value")).and_then(difficulty_enum_from_trackselect) {
                e.boss_catchup_difficulty = v;
            }
            if let Some(v) = child(a, "AICatchupDifficulty").and_then(|n| n.attribute("value")).and_then(difficulty_enum_from_trackselect) {
                e.ai_catchup_difficulty = v;
            }
        }
        Ok(e)
    }

    /// `CMetagameManager::GetDifficultyAdjust(eventCC, kartCC) @0012df90` -> enum 0..4.
    pub fn difficulty_adjust_enum(&self, event_cc: i32, kart_cc: i32) -> usize {
        let t = &self.relative_cc;
        if event_cc <= kart_cc - t[0] {
            0
        } else if event_cc <= kart_cc - t[1] {
            1
        } else if event_cc <= kart_cc - t[2] {
            2
        } else if kart_cc - t[3] < event_cc {
            4
        } else {
            3
        }
    }

    /// `CMetagameManager::GetRaceMaxScore @0012df04` = `cc * 150` (integer constant 0x96 in the code; economy.xml fMultiplier is 150).
    pub fn race_max_score(&self, race_cc: i32) -> i32 {
        race_cc * 0x96
    }

    /// `CMetagameManager::CalculateScoreFromCC @0012df14`: star thresholds used by daily races / tournaments (campaign events use the
    /// eventdef `<Stars>`, see `CResultsScreen @004351bc`).
    pub fn score_from_cc(&self, cc: i32) -> [i32; 3] {
        let c = cc as f32;
        [
            (self.score_addition + c * self.score_multiplier * self.star_multipliers[0]) as i32,
            (self.score_addition + c * self.score_multiplier * self.star_multipliers[1]) as i32,
            (self.score_addition + c * self.score_multiplier * self.star_multipliers[2]) as i32,
        ]
    }

    /// `(min,max)` clamp of the AI skill for a mode (`CampaignEvent aiSkillMin/aiSkillMax` default to these).
    pub fn ai_skill_range(&self, mode: GameMode) -> (f32, f32) {
        match mode {
            GameMode::TimeAttack => self.ai_skill_time_attack,
            GameMode::SeedRush | GameMode::BossFruitRush => self.ai_skill_fruit_rush,
            GameMode::BossBattle => self.ai_skill_boss_battle,
            _ => self.ai_skill_race,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// scoreconfig.xml
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ScoreConfig {
    pub fruit_pct_for_min: f32,
    pub fruit_pct_for_max: f32,
    pub min_fruit_score: f32,
    pub max_fruit_score: f32,
    pub time_pct_over_for_max: f32,
    pub time_pct_over_for_min: f32,
    pub min_time_score: f32,
    pub max_time_score: f32,
    pub deaths_max_score_divisor: f32,
    pub deaths_multiplier_per_death: f32,
    pub top_speed_threshold: f32,
    pub top_speed_xyz: [f32; 3],
    /// Seedway, RockyRoad, Air, Stunt, SubZero
    pub top_speed_theme: [f32; 5],
    pub accel_xyz: [f32; 3],
    pub accel_theme: [f32; 5],
}

impl Default for ScoreConfig {
    fn default() -> Self {
        ScoreConfig {
            fruit_pct_for_min: 0.0,
            fruit_pct_for_max: 1.0,
            min_fruit_score: 0.125,
            max_fruit_score: 1.0,
            time_pct_over_for_max: 0.0,
            time_pct_over_for_min: 0.35,
            min_time_score: 0.125,
            max_time_score: 1.0,
            deaths_max_score_divisor: 1.5,
            deaths_multiplier_per_death: 0.5,
            top_speed_threshold: 1.35,
            top_speed_xyz: [150.0, 0.1, 50.0],
            top_speed_theme: [0.9, 1.0, 1.1, 0.35, 0.5],
            accel_xyz: [0.5, 1.0, 35.0],
            accel_theme: [1.0, 0.3, 0.45, 0.3, 0.35],
        }
    }
}

impl ScoreConfig {
    pub fn parse(xml: &str) -> Result<ScoreConfig, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| format!("scoreconfig xml: {e}"))?;
        let root = doc.root_element();
        let mut c = ScoreConfig::default();
        let theme = |n: roxmltree::Node, fallback: [f32; 5]| -> [f32; 5] {
            let mut t = fallback;
            if let Some(m) = child(n, "ThemeModifiers") {
                for (i, k) in ["Seedway", "RockyRoad", "Air", "Stunt", "SubZero"].iter().enumerate() {
                    if let Some(v) = child_f32(m, k) {
                        t[i] = v;
                    }
                }
            }
            t
        };
        if let Some(n) = child(root, "ScoreCounterFinishingFruit") {
            c.fruit_pct_for_min = child_f32(n, "PercentForMinScore").unwrap_or(c.fruit_pct_for_min);
            c.fruit_pct_for_max = child_f32(n, "PercentForMaxScore").unwrap_or(c.fruit_pct_for_max);
            c.min_fruit_score = child_f32(n, "MinFruitScore").unwrap_or(c.min_fruit_score);
            c.max_fruit_score = child_f32(n, "MaxFruitScore").unwrap_or(c.max_fruit_score);
        }
        if let Some(n) = child(root, "ScoreCounterFinishingTime") {
            c.time_pct_over_for_max = child_f32(n, "PercentOverForMaxScore").unwrap_or(c.time_pct_over_for_max);
            c.time_pct_over_for_min = child_f32(n, "PercentOverForMinScore").unwrap_or(c.time_pct_over_for_min);
            c.min_time_score = child_f32(n, "MinTimeScore").unwrap_or(c.min_time_score);
            c.max_time_score = child_f32(n, "MaxTimeScore").unwrap_or(c.max_time_score);
        }
        if let Some(n) = child(root, "ScoreCounterDeaths") {
            c.deaths_max_score_divisor = child_f32(n, "MaxScoreDivisor").unwrap_or(c.deaths_max_score_divisor);
            c.deaths_multiplier_per_death = child_f32(n, "MultiplierPerDeath").unwrap_or(c.deaths_multiplier_per_death);
        }
        if let Some(n) = child(root, "ScoreCounterTopSpeed") {
            c.top_speed_threshold = child_f32(n, "SpeedThreshold").unwrap_or(c.top_speed_threshold);
            c.top_speed_xyz = [
                child_f32(n, "X").unwrap_or(c.top_speed_xyz[0]),
                child_f32(n, "Y").unwrap_or(c.top_speed_xyz[1]),
                child_f32(n, "Z").unwrap_or(c.top_speed_xyz[2]),
            ];
            c.top_speed_theme = theme(n, c.top_speed_theme);
        }
        if let Some(n) = child(root, "ScoreCounterAcceleration") {
            c.accel_xyz = [
                child_f32(n, "X").unwrap_or(c.accel_xyz[0]),
                child_f32(n, "Y").unwrap_or(c.accel_xyz[1]),
                child_f32(n, "Z").unwrap_or(c.accel_xyz[2]),
            ];
            c.accel_theme = theme(n, c.accel_theme);
        }
        Ok(c)
    }
}

/// Telemetry the score counters need (supplied by the car code; the formulas are the original ones).
#[derive(Clone, Debug, Default)]
pub struct ScoreInputs {
    /// `CKartManager::GetKartCC` of the player's kart (counters read it lazily; -1 = unset -> counter scores 0)
    pub kart_cc: i32,
    /// `CScoreCounterTopSpeed::Update`: seconds with `speed >= m_fMinDesiredSpeed * SpeedThreshold` (`CCar+0x1ab4`)
    pub top_speed_seconds: f32,
    /// `CScoreCounterAcceleration::Update`: seconds with `CCar+0x4c8 == 0` while not in the slingshot
    // UNRESOLVED: the meaning of CCar+0x4c8 (probably a "braking / ability" flag); the caller decides what counts.
    pub accel_seconds: f32,
    /// `CScoreCounterDeaths::AddDeath` count (`CCar::Respawn` of the local player)
    pub deaths: i32,
    /// theme / episode index 0..4 (`CGame+0x2fc`); 5 = no top speed / accel score
    pub episode: i32,
    /// `CScoreCounterBonus::AddScore` (boss battle)
    pub bonus: i32,
    /// SEED_RUSH: fruit fraction at the finish (`CScoreCounterFinishingFruit::SetFruitPercent`)
    pub fruit_fraction: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ScoreBreakdown {
    pub top_speed: i32,
    pub acceleration: i32,
    pub deaths: i32,
    /// `GetScore()`: sum of the non-bonus counters
    pub performance: i32,
    /// finishing position / time / fruit counter (`IsBonusScore`)
    pub finishing: i32,
    /// `GetBonusScore @001f43bc` = `ceil(finishing / 100) * 100`
    pub bonus_rounded: i32,
    pub total: i32,
}

impl ScoreConfig {
    /// `CScoreCounterFinishingPosition::SetPosition @001f1d48` (listing: fixed-point /8): `(9 - pos) / 8 * scale`.
    pub fn position_score(&self, scale: i32, position: i32) -> i32 {
        ((9 - position) as f32 / 8.0 * scale as f32) as i32
    }
    /// `CScoreCounterFinishingFruit::SetFruitPercent @001f1bcc` (listing).
    pub fn fruit_score(&self, scale: i32, fraction: f32) -> i32 {
        let t = ((fraction - self.fruit_pct_for_min) / (self.fruit_pct_for_max - self.fruit_pct_for_min)).clamp(0.0, 1.0);
        let s = scale as f32;
        (s * self.min_fruit_score + t * (s * self.max_fruit_score - s * self.min_fruit_score)) as i32
    }
    /// `CScoreCounterFinishingTime::SetTime @001f230c` (listing). NOTE the original blends `scale*MaxTimeScore` with
    /// `scale*MaxTimeScore` (the compiled expression is `x + t*(x - x)`), so the result is always `scale * MaxTimeScore`.
    pub fn time_score(&self, scale: i32, _remaining: f32, _duration: f32) -> i32 {
        let x = scale as f32 * self.max_time_score;
        (x + 0.0 * (x - x)) as i32
    }
    /// `CScoreCounterDeaths::GetScore @001f0c40` (listing): `scale / MaxScoreDivisor * MultiplierPerDeath ^ deaths`.
    pub fn deaths_score(&self, scale: i32, deaths: i32) -> i32 {
        let a = scale as f32 / self.deaths_max_score_divisor;
        (a as f64 * (self.deaths_multiplier_per_death as f64).powi(deaths.max(0))) as i32
    }
    /// `CScoreCounterTopSpeed::GetScore @001f363c` (listing): `(X*t*cc^Y + cc*Z) * theme[ep]`.
    pub fn top_speed_score(&self, inp: &ScoreInputs) -> i32 {
        if inp.kart_cc < 0 || !(0..5).contains(&inp.episode) {
            return 0;
        }
        let [x, y, z] = self.top_speed_xyz;
        let cc = inp.kart_cc as f64;
        let s = ((x * inp.top_speed_seconds) as f64 * cc.powf(y as f64)) as f32 + inp.kart_cc as f32 * z;
        (s * self.top_speed_theme[inp.episode as usize]) as i32
    }
    /// `CScoreCounter::GetScore @001f0254` (acceleration; listing): `(X*t*cc^Y + cc*Z) * theme[ep]`, 0 for episode 5.
    pub fn acceleration_score(&self, inp: &ScoreInputs) -> i32 {
        if !(0..5).contains(&inp.episode) {
            return 0;
        }
        let [x, y, z] = self.accel_xyz;
        let cc = inp.kart_cc as f64;
        let s = ((x * inp.accel_seconds) as f64 * cc.powf(y as f64)) as f32 + inp.kart_cc as f32 * z;
        (s * self.accel_theme[inp.episode as usize]) as i32
    }

    /// Full score of one finished event: `GetScore() + GetBonusScore()` (what `CResultsScreen` compares with the Star thresholds).
    /// `time_remaining/timer_duration` are only used by TIME_ATTACK (no effect, see `time_score`).
    pub fn total_score(
        &self,
        mode: GameMode,
        race_cc: i32,
        position: i32,
        time_remaining: f32,
        timer_duration: f32,
        inp: &ScoreInputs,
    ) -> ScoreBreakdown {
        let scale = race_cc * 0x96; // GetRaceMaxScore
        let mut b = ScoreBreakdown::default();
        b.top_speed = self.top_speed_score(inp);
        b.acceleration = self.acceleration_score(inp);
        b.deaths = self.deaths_score(scale, inp.deaths);
        b.performance = b.top_speed + b.acceleration + b.deaths;
        // GetBonusScore @001f43bc: counter 3 for TIME_ATTACK, 4 for SEED_RUSH, 2 (position) otherwise; rounded UP to 100
        b.finishing = match mode {
            GameMode::TimeAttack => self.time_score(scale, time_remaining, timer_duration),
            GameMode::SeedRush => self.fruit_score(scale, inp.fruit_fraction),
            _ => self.position_score(scale, position),
        };
        b.bonus_rounded = ((b.finishing + 99) / 100) * 100;
        b.total = b.performance + b.bonus_rounded + inp.bonus;
        b
    }
}

// ---------------------------------------------------------------------------------------------------------------
// eventdefinitiondata.xml: campaign events, rewards, event index table
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Reward {
    /// 1 = OneStar, 2 = TwoStar, 3 = ThreeStar, 4 = other (always)
    pub tier: u8,
    pub kind: String,
    pub sub_type: String,
    pub quantity: i32,
}

#[derive(Clone, Debug)]
pub struct CampaignEvent {
    pub tag: String,
    pub event_index: i32,
    pub campaign_cc: i32,
    pub energy_cost: i32,
    pub ai_skill_min: Option<f32>,
    pub ai_skill_max: Option<f32>,
    pub hidden: bool,
    pub disable_catchup: bool,
    pub rewards: Vec<Reward>,
}

#[derive(Clone, Debug)]
pub struct EventDataRow {
    pub index: i32,
    pub episode: String,
    pub game_mode: String,
    pub tier: i32,
    pub event: i32,
    pub stage: i32,
    pub energy_cost: i32,
}

#[derive(Clone, Debug, Default)]
pub struct EventTable {
    pub default_xp_reward: i32,
    pub default_coin_reward: i32,
    pub campaign: Vec<CampaignEvent>,
    pub events: Vec<EventDataRow>,
}

/// Episode names in eventdef folder order (`eventdef_episode00 .. 04`): themes 002..006.
pub const EPISODE_NAMES: [&str; 5] = ["Seedway", "RockyRoad", "Air", "Stunt", "SubZero"];

impl EventTable {
    pub fn parse(xml: &str) -> Result<EventTable, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| format!("eventdefinitiondata xml: {e}"))?;
        let root = doc.root_element();
        let mut t = EventTable::default();
        if let Some(c) = child(root, "Campaign") {
            t.default_xp_reward = attr_i32(c, "defaultXPReward").unwrap_or(0);
            t.default_coin_reward = attr_i32(c, "defaultCoinReward").unwrap_or(0);
            for e in c.children().filter(|n| n.is_element() && n.tag_name().name() == "CampaignEvent") {
                let mut ce = CampaignEvent {
                    tag: e.attribute("tag").unwrap_or("").to_string(),
                    event_index: attr_i32(e, "eventIndex").unwrap_or(-1),
                    campaign_cc: attr_i32(e, "campaignCC").unwrap_or(0),
                    energy_cost: attr_i32(e, "energyCost").unwrap_or(1),
                    ai_skill_min: attr_f32(e, "aiSkillMin"),
                    ai_skill_max: attr_f32(e, "aiSkillMax"),
                    hidden: e.attribute("hidden").map(|v| v == "true" || v == "1").unwrap_or(false),
                    disable_catchup: e.attribute("disableCatchup").map(|v| v == "true" || v == "1").unwrap_or(false),
                    rewards: Vec::new(),
                };
                for r in e.children().filter(|n| n.is_element() && n.tag_name().name() == "Reward") {
                    let tier = match r.attribute("RewardType").unwrap_or("") {
                        s if s.eq_ignore_ascii_case("OneStar") => 1,
                        s if s.eq_ignore_ascii_case("TwoStar") => 2,
                        s if s.eq_ignore_ascii_case("ThreeStar") => 3,
                        _ => 4,
                    };
                    ce.rewards.push(Reward {
                        tier,
                        kind: r.attribute("Type").unwrap_or("").to_string(),
                        sub_type: r.attribute("SubType").unwrap_or("").to_string(),
                        quantity: attr_i32(r, "Quantity").unwrap_or(0),
                    });
                }
                t.campaign.push(ce);
            }
        }
        if let Some(d) = child(root, "EventData") {
            for e in d.children().filter(|n| n.is_element() && n.tag_name().name() == "Event") {
                t.events.push(EventDataRow {
                    index: attr_i32(e, "index").unwrap_or(-1),
                    episode: e.attribute("episode").unwrap_or("").to_string(),
                    game_mode: e.attribute("gameMode").unwrap_or("").to_string(),
                    tier: attr_i32(e, "tier").unwrap_or(0),
                    event: attr_i32(e, "event").unwrap_or(0),
                    stage: attr_i32(e, "stage").unwrap_or(0),
                    energy_cost: attr_i32(e, "energyCost").unwrap_or(1),
                });
            }
        }
        Ok(t)
    }

    /// `eventdef_episode01_event03_stage02(.xml)` -> the `<Event index>` row (episode folder number -> `EPISODE_NAMES`).
    pub fn event_index_for_file(&self, stem: &str) -> Option<i32> {
        let s = stem.trim_end_matches(".xml");
        let rest = s.strip_prefix("eventdef_episode")?;
        let (ep, rest) = rest.split_once("_event")?;
        let (ev, st) = rest.split_once("_stage")?;
        let ep = atoi(ep) as usize;
        let name = EPISODE_NAMES.get(ep)?;
        let (ev, st) = (atoi(ev), atoi(st));
        self.events.iter().find(|r| r.episode == *name && r.event == ev && r.stage == st).map(|r| r.index)
    }

    /// `CEventDefinitionManager::GetCampaignData`: the `<CampaignEvent eventIndex>` of an event index.
    pub fn campaign_event(&self, event_index: i32) -> Option<&CampaignEvent> {
        self.campaign.iter().find(|c| c.event_index == event_index)
    }

    /// Rewards earned when going from `stars_before` to `stars_now` stars (each star tier pays once; tier 4 = always on the
    /// first completion).
    pub fn rewards_for(&self, event_index: i32, stars_before: u8, stars_now: u8) -> Vec<Reward> {
        let Some(ce) = self.campaign_event(event_index) else { return Vec::new() };
        ce.rewards
            .iter()
            .filter(|r| match r.tier {
                1..=3 => r.tier > stars_before && r.tier <= stars_now,
                _ => stars_before == 0 && stars_now > 0,
            })
            .cloned()
            .collect()
    }

    /// `CampaignEvent aiSkillMin/aiSkillMax` with the economy defaults per mode (`ReadEventDefinition`/campaign reader @00102xxx);
    /// events outside the campaign use 0..1 (`CGame+0x4b8/0x4bc` reset values).
    pub fn ai_skill_range(&self, event_index: Option<i32>, mode: GameMode, eco: &Economy) -> (f32, f32) {
        match event_index.and_then(|i| self.campaign_event(i)) {
            Some(ce) => {
                let d = eco.ai_skill_range(mode);
                (ce.ai_skill_min.unwrap_or(d.0), ce.ai_skill_max.unwrap_or(d.1))
            }
            None => (0.0, 1.0),
        }
    }
}

/// All configuration files the rules need, loaded from the `assets292` folder.
#[derive(Clone, Debug)]
pub struct RulesConfig {
    pub economy: Economy,
    pub score: ScoreConfig,
    pub events: EventTable,
}

impl RulesConfig {
    pub fn load(assets: &Path) -> Result<RulesConfig, String> {
        let rd = |p: &str| std::fs::read_to_string(assets.join(p)).map_err(|e| format!("{p}: {e}"));
        Ok(RulesConfig {
            economy: Economy::parse(&rd("xml_gameplay/misc/economy.xml")?)?,
            score: ScoreConfig::parse(&rd("xml/xml/global/scoreconfig.xml")?)?,
            events: EventTable::parse(&rd("xml/xml/global/eventdefinitiondata.xml")?)?,
        })
    }
}

// =============================================================================================================
// CSpline (race splines)
// =============================================================================================================

/// One node of `CSpline` (0x3c bytes in the original).
#[derive(Clone, Debug)]
pub struct SplineNode {
    pub pos: Vec3,
    /// source record floats [0..3] (surface up vector), `GetUpVector @0018fc6c`
    pub up: Vec3,
    /// source record [4] / [5]: corridor half widths (`+0x10` / `+0x14` of the source record)
    pub width_left: f32,
    pub width_right: f32,
    /// source record [6] as int = physics material (`GetPhysMaterial @0018f1b0`)
    pub phys_material: i32,
    /// `+0x10..0x18` = up x dir ("right", positive lateral offset)
    pub side: Vec3,
    /// `+0x1c..0x24` unit direction to the next node
    pub dir: Vec3,
    /// `+0x28` length of the segment to the next node (the last node repeats the previous segment)
    pub seg_len: f32,
    /// `+0x2c`
    pub inv_len: f32,
    /// `+0x30` turn angle at this node (previous direction -> this direction)
    pub curvature: f32,
    /// `+0x34` cumulative distance from node 0
    pub cum_dist: f32,
    /// `+0x38` "radius": window length / |sum of turn angles| over +-15 m, capped at 50000
    pub radius: f32,
}

/// `CSpline::+0x10` type by name.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SplineKind {
    Race = 0,
    /// `race_*` with both AI weightings 0 (`SetupEnvironmentSplines` sets type 1)
    Disabled = 1,
    CamPath = 2,
    CamTrack = 3,
    Other = 4,
}

#[derive(Clone, Debug)]
pub struct RaceSpline {
    pub name: String,
    /// number parsed from the first digit in the name (`CSpline+4`)
    pub number: i32,
    pub kind: SplineKind,
    /// `+0x20`: name starts with `main_`
    pub is_main: bool,
    /// `+0x18`: |last - first|^2 < 2500
    pub is_loop: bool,
    /// `+0x1c` total length
    pub length: f32,
    /// `+0x2c` / `+0x30` AI weightings (default 1.0; track.xml, then the eventdef)
    pub min_ai_weighting: f32,
    pub max_ai_weighting: f32,
    /// `+0x34` float spline position of the finish line, `+0x38` its distance (`SetupEnvironmentMarkup`)
    pub finish_pos: f32,
    pub finish_dist: f32,
    /// `+0x3c` distance of the start line, `+0x40` track end, `+0x44` smash line (optional)
    pub start_dist: f32,
    pub trackend_dist: f32,
    pub smash_dist: Option<f32>,
    pub nodes: Vec<SplineNode>,
}

const SHARPNESS_CAP: f32 = 50000.0; // DAT_0018d078
const LOOP_DIST2: f32 = 2500.0; // DAT_0018d070

impl RaceSpline {
    pub fn kind_from_name(name: &str) -> SplineKind {
        if partial_match(name, "race_") || partial_match(name, "main_race_") {
            SplineKind::Race
        } else if partial_match(name, "camposition_path") {
            SplineKind::CamPath
        } else if partial_match(name, "camposition_track") {
            SplineKind::CamTrack
        } else {
            SplineKind::Other
        }
    }

    /// `CSpline::CSpline(int,int,int) @0018cbb0` (copy-from-environment constructor).
    pub fn new(name: &str, points: &[[f32; 3]], extra: &[[f32; 7]]) -> RaceSpline {
        let n = points.len();
        let digits: String = name.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        let mut nodes: Vec<SplineNode> = (0..n)
            .map(|i| {
                let e = extra.get(i).copied().unwrap_or([0.0, 1.0, 0.0, 0.0, 20.0, 20.0, 0.0]);
                SplineNode {
                    pos: Vec3::from(points[i]),
                    up: Vec3::new(e[0], e[1], e[2]),
                    width_left: e[4],
                    width_right: e[5],
                    phys_material: e[6].to_bits() as i32,
                    side: Vec3::ZERO,
                    dir: Vec3::Z,
                    seg_len: 0.0,
                    inv_len: 0.0,
                    curvature: 0.0,
                    cum_dist: 0.0,
                    radius: SHARPNESS_CAP,
                }
            })
            .collect();
        let is_loop = n >= 2 && (nodes[n - 1].pos - nodes[0].pos).length_squared() < LOOP_DIST2;
        let mut total = 0.0f32;
        if n >= 2 {
            for i in 0..n {
                // the last node reuses the final segment (n-2 -> n-1)
                let (a, b) = if i + 1 < n { (i, i + 1) } else { (n - 2, n - 1) };
                let d = nodes[b].pos - nodes[a].pos;
                let len = d.length();
                let inv = 1.0 / len;
                let dir = d * inv;
                let up = nodes[i].up;
                nodes[i].seg_len = len;
                nodes[i].inv_len = inv;
                nodes[i].dir = dir;
                // side = up x dir
                nodes[i].side = Vec3::new(up.y * dir.z - dir.y * up.z, dir.x * up.z - dir.z * up.x, dir.y * up.x - up.y * dir.x);
                nodes[i].cum_dist = total;
                if i + 1 < n {
                    total += len;
                }
            }
            // `+0x1c` = total (the cumulative sum of the first n-1 segments; the last node's `cum_dist` is the total too)
            nodes[n - 1].cum_dist = total;
            // curvature: atan2(dot(prev.dir, dir), dot(prev.dir, side)) - pi/2
            for i in 0..n {
                let prev = nodes[(i + n - 1) % n].dir;
                let c = prev.dot(nodes[i].dir) as f64;
                let s = prev.dot(nodes[i].side) as f64;
                nodes[i].curvature = (c.atan2(s) - 1.570_796_370_506_286_6) as f32;
            }
            // radius over +-15 m (listing @0018cf0c..0018d03c)
            let curv: Vec<f32> = nodes.iter().map(|x| x.curvature).collect();
            let seg: Vec<f32> = nodes.iter().map(|x| x.seg_len).collect();
            for i in 0..n {
                let mut len_acc = seg[i];
                let mut c = curv[i];
                let mut rem = 15.0 - seg[i];
                let mut last = seg[i];
                let mut j = i;
                while rem > 0.0 {
                    j += 1;
                    if j >= n {
                        j = 0;
                    }
                    last = seg[j];
                    c += curv[j];
                    len_acc += last;
                    rem -= last;
                }
                len_acc -= last * 0.5;
                let mut p = (i + n - 1) % n;
                let mut last_b = seg[p];
                len_acc += last_b;
                let mut rem = 15.0 - last_b;
                while rem > 0.0 {
                    c += curv[p];
                    p = if p == 0 { n - 1 } else { p - 1 };
                    last_b = seg[p];
                    rem -= last_b;
                    len_acc += last_b;
                }
                nodes[i].radius = if c == 0.0 { SHARPNESS_CAP } else { ((len_acc - last_b * 0.5) / c.abs()).min(SHARPNESS_CAP) };
            }
        }
        let kind = Self::kind_from_name(name);
        let mut s = RaceSpline {
            name: name.to_string(),
            number: atoi(&digits),
            kind,
            is_main: partial_match(name, "main_"),
            is_loop,
            length: total,
            min_ai_weighting: 1.0,
            max_ai_weighting: 1.0,
            finish_pos: 0.0,
            finish_dist: 0.0,
            start_dist: 0.0,
            trackend_dist: 0.0,
            smash_dist: None,
            nodes,
        };
        // default markup (no helpers): finish 5 m before the end, track end 50 m after the finish (`SetupEnvironmentMarkup`)
        s.finish_dist = s.length - 5.0;
        s.finish_pos = s.pos_from_distance(s.finish_dist);
        s.trackend_dist = s.finish_dist + 50.0;
        s
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    fn next_index(&self, i: usize) -> usize {
        if i + 1 < self.nodes.len() {
            i + 1
        } else {
            0
        }
    }

    /// `CSpline::GetPosition(float) @0018d4c0`
    pub fn position(&self, pos: f32) -> Vec3 {
        let i = (pos as usize).min(self.nodes.len() - 1);
        let f = pos - i as f32;
        let a = self.nodes[i].pos;
        let b = self.nodes[self.next_index(i)].pos;
        a + (b - a) * f
    }

    /// `GetHeight @0018d544` (y of `position`).
    pub fn height(&self, pos: f32) -> f32 {
        self.position(pos).y
    }

    /// `GetUpVector @0018fc6c`
    pub fn up_vector(&self, node: usize) -> Vec3 {
        self.nodes[node].up
    }

    /// `GetOffset(p, node) @0018d468`: projection of `p` on the node's segment as a fraction (>= 0 = at/after the node's plane).
    pub fn offset(&self, p: Vec3, node: usize) -> f32 {
        let n = &self.nodes[node];
        (p - n.pos).dot(n.dir) * n.inv_len
    }

    /// `GetClosestNode @0018f1cc`
    pub fn closest_node(&self, p: Vec3) -> usize {
        let mut best = 0;
        let mut bd = (self.nodes[0].pos - p).length_squared();
        for (i, n) in self.nodes.iter().enumerate().skip(1) {
            let d = (n.pos - p).length_squared();
            if d < bd {
                bd = d;
                best = i;
            }
        }
        best
    }

    /// `GetClosestSplinePos @0018f280`: (float position, squared distance to the closest node).
    pub fn closest_pos(&self, p: Vec3) -> (f32, f32) {
        let node = self.closest_node(p);
        let d2 = (self.nodes[node].pos - p).length_squared();
        let n = self.nodes.len() as i32;
        let mut i = node as i32;
        let mut went_back = false;
        let mut went_fwd = false;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 4 * n + 8 {
                return (i.clamp(0, n - 1) as f32, d2);
            }
            let t = self.offset(p, i as usize);
            if t >= 0.9999 {
                if went_back {
                    return (i as f32 + 0.9999, d2);
                }
                i += 1;
                if i < n {
                    went_fwd = true;
                } else if !self.is_loop {
                    return ((n - 2) as f32 + 0.999, d2);
                } else {
                    i -= n;
                    went_fwd = true;
                }
                continue;
            }
            if t < 0.0 {
                if went_fwd {
                    return (i as f32, d2);
                }
                i -= 1;
                if i < 0 {
                    if !self.is_loop {
                        return (0.0, d2);
                    }
                    went_back = true;
                    i += n;
                } else {
                    went_back = true;
                }
                continue;
            }
            return (t + i as f32, d2);
        }
    }

    /// `CSpline::GetNewPos @0018d598` (unchecked variant, `param_5 = 0`, used by `CCar::Update` first): walk from `node` (the
    /// integer part of the previous position) along the nodes until `p` projects inside a segment.
    /// Returns (position, lap delta -1/0/+1).
    pub fn new_pos(&self, node: usize, p: Vec3) -> (f32, i32) {
        let n = self.nodes.len() as i32;
        let mut i = node as i32;
        let mut lap = 0;
        let mut moved_back = false;
        let mut moved_fwd = false;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 2 * n + 8 {
                return (i.clamp(0, n - 1) as f32, lap);
            }
            let t = self.offset(p, i as usize);
            if t >= 0.9999 {
                if moved_back {
                    return (i as f32 + 0.9999, lap);
                }
                i += 1;
                if i < n {
                    moved_fwd = true;
                } else if !self.is_loop {
                    return ((n - 2) as f32 + 0.999, lap);
                } else {
                    lap = 1;
                    i -= n;
                    moved_fwd = true;
                }
                continue;
            }
            if t >= 0.0 {
                return (t + i as f32, lap);
            }
            if moved_fwd {
                return (i as f32, lap);
            }
            i -= 1;
            if i < 0 {
                if !self.is_loop {
                    return (0.0, lap);
                }
                lap = -1;
                i += n;
            }
            moved_back = true;
        }
    }

    /// `CSpline::Lookahead(dist, pos) @0018dab4`: advance (or go back) `dist` metres from `pos`. Returns (position, lap delta).
    /// The original wraps to node 0 past the last node whatever `is_loop` says; so does this.
    pub fn lookahead(&self, pos: f32, dist: f32) -> (f32, i32) {
        let n = self.nodes.len();
        let mut i = (pos.max(0.0) as usize).min(n - 1);
        let mut laps = 0;
        let mut seg = self.nodes[i].seg_len;
        let mut d = dist + (pos - i as f32) * seg;
        if dist < 0.0 {
            let mut guard = 0;
            while d < 0.0 && guard < n * 4 + 8 {
                if i == 0 {
                    i = n - 1;
                    laps -= 1;
                } else {
                    i -= 1;
                }
                seg = self.nodes[i].seg_len;
                d += seg;
                guard += 1;
            }
        } else if d >= seg - 0.001 {
            let mut guard = 0;
            loop {
                i += 1;
                d -= seg;
                if i >= n {
                    i = 0;
                    laps += 1;
                }
                seg = self.nodes[i].seg_len;
                guard += 1;
                if !(seg - 0.001 < d) || guard > n * 4 + 8 {
                    break;
                }
            }
        }
        let f = d / seg;
        let f = if f < 0.0 { 0.0 } else { f.min(0.999) };
        (f + i as f32, laps)
    }

    /// `GetLeftWidth @0018dfb0` (min of the two bracketing nodes).
    pub fn left_width(&self, pos: f32) -> f32 {
        let i = pos as usize;
        self.nodes[i].width_left.min(self.nodes[self.next_index(i)].width_left)
    }
    /// `GetRightWidth @0018e008`
    pub fn right_width(&self, pos: f32) -> f32 {
        let i = pos as usize;
        self.nodes[i].width_right.min(self.nodes[self.next_index(i)].width_right)
    }
    fn min_over(&self, a: f32, b: f32, f: impl Fn(&SplineNode) -> f32) -> f32 {
        let n = self.nodes.len();
        let (a, b) = ((a as usize).min(n - 1), (b as usize).min(n - 1));
        let mut m = f(&self.nodes[a]).min(f(&self.nodes[self.next_index(a)])).min(f(&self.nodes[b])).min(f(&self.nodes[self.next_index(b)]));
        let mut i = a;
        let mut guard = 0;
        while i != b && guard < n + 2 {
            i = self.next_index(i);
            m = m.min(f(&self.nodes[i]));
            guard += 1;
        }
        m
    }
    /// `GetMinLeftWidth(a,b) @0018e0a8`
    pub fn min_left_width(&self, a: f32, b: f32) -> f32 {
        self.min_over(a, b, |n| n.width_left)
    }
    /// `GetMinRightWidth(a,b) @0018e19c`
    pub fn min_right_width(&self, a: f32, b: f32) -> f32 {
        self.min_over(a, b, |n| n.width_right)
    }

    /// `GetLateralOffset(pos, p) @0018e308`: signed distance of `p` from the line along `side` (positive = right).
    pub fn lateral_offset(&self, pos: f32, p: Vec3) -> f32 {
        let i = (pos as usize).min(self.nodes.len() - 1);
        let f = pos - i as f32;
        let a = &self.nodes[i];
        let on = a.pos + (self.nodes[self.next_index(i)].pos - a.pos) * f;
        (p - on).dot(a.side)
    }

    /// `GetInfo(pos, lateral) @0018e3ac`: point on the line at `lateral` metres from it, and the node direction.
    pub fn info(&self, pos: f32, lateral: f32) -> (Vec3, Vec3) {
        let n = self.nodes.len();
        let i = (pos as usize).min(n - 1);
        let f = pos - i as f32;
        let j = if i + 1 < n {
            i + 1
        } else if self.is_loop {
            0
        } else {
            n - 1
        };
        let (a, b) = (&self.nodes[i], &self.nodes[j]);
        let point = a.pos + (b.pos - a.pos) * f + a.side * lateral + (b.side * lateral - a.side * lateral) * f;
        (point, a.dir)
    }

    /// `GetSlope(node) @0018e5a4`
    pub fn slope(&self, node: usize) -> f32 {
        if node + 1 < self.nodes.len() {
            (self.nodes[node + 1].pos.y - self.nodes[node].pos.y) / self.nodes[node].seg_len
        } else if !self.is_loop {
            0.0
        } else {
            (self.nodes[0].pos.y - self.nodes[node].pos.y) / self.nodes[node].seg_len
        }
    }

    /// distance from node 0 at a float position (`GetRaceTotalSplineDist @001a43e8`: `cum[i] + frac * seg[i]`)
    pub fn dist_at(&self, pos: f32) -> f32 {
        let i = (pos as usize).min(self.nodes.len() - 1);
        self.nodes[i].cum_dist + (pos - i as f32) * self.nodes[i].seg_len
    }

    /// `GetSplinePosFromDistance @0018fb80`
    pub fn pos_from_distance(&self, dist: f32) -> f32 {
        let n = self.nodes.len();
        // binary search: first node with cum_dist > dist
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.nodes[mid].cum_dist <= dist {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return 0.0;
        }
        let a = &self.nodes[lo - 1];
        let f = ((dist - a.cum_dist) * a.inv_len).clamp(0.0, 1.0);
        (lo - 1) as f32 + f
    }

    /// `GetSignedDistanceAlongSplineFromRacePos(a, b) @0018f0a8`: metres from `a` to `b` along the line (wraps by `length`).
    pub fn signed_distance_from_race_pos(&self, a: f32, b: f32) -> f32 {
        let n = self.nodes.len() as f32;
        let (mut a, mut b) = (a, b);
        let mut laps = 0i32;
        while a < 0.0 {
            laps += 1;
            a += n;
        }
        while a > n {
            laps -= 1;
            a -= n;
        }
        while b < 0.0 {
            laps -= 1;
            b += n;
        }
        while b > n {
            laps += 1;
            b -= n;
        }
        (self.dist_at(b.min(n - 1.0)) - self.dist_at(a.min(n - 1.0))) + laps as f32 * self.length
    }

    /// `GetSignedDistanceAlongSpline(a, b) @0018f014`: positions -> distance, wrapped into +-length/2.
    pub fn signed_distance(&self, a: f32, b: f32) -> f32 {
        let mut d = self.dist_at(b) - self.dist_at(a);
        if d > self.length * 0.5 {
            d -= self.length;
        } else if d < -self.length * 0.5 {
            d += self.length;
        }
        d
    }

    pub fn is_race_line(&self) -> bool {
        self.kind == SplineKind::Race
    }
}

// =============================================================================================================
// Track = splines + helpers
// =============================================================================================================

/// `<Spline name min_ai_weighting max_ai_weighting>` of `track.xml` (`SetupEnvironmentSplines`).
pub fn parse_track_xml_weights(xml: &str) -> Vec<SplineWeight> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };
    doc.root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Spline")
        .map(|n| SplineWeight {
            name: n.attribute("name").unwrap_or("").to_string(),
            min_ai_weighting: attr_f32(n, "min_ai_weighting").unwrap_or(0.0),
            max_ai_weighting: attr_f32(n, "max_ai_weighting").unwrap_or(0.0),
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct RaceTrack {
    /// after `SetupEnvironmentSplines`: all splines except `bird_*` / `DragSpline*` (max 0x40), race lines with a different
    /// node count than the first race line removed. Index = spline id.
    pub splines: Vec<RaceSpline>,
    pub start_helper: Option<Vec3>,
    pub finish_helper: Option<Vec3>,
    pub trackend_helper: Option<Vec3>,
    pub smash_helper: Option<Vec3>,
    pub splittime_helpers: Vec<Vec3>,
}

impl RaceTrack {
    /// Builds the track from raw spline / helper data (`SetupEnvironmentSplines @0011caf8` + the spline part of
    /// `SetupEnvironmentMarkup @0011db98`).
    pub fn from_parts(
        spline_defs: &[(String, Vec<[f32; 3]>, Vec<[f32; 7]>)],
        helpers: &[(String, Vec3)],
        track_xml_weights: &[SplineWeight],
        event: Option<&EventDef>,
    ) -> RaceTrack {
        let mut t = RaceTrack::default();
        for (name, pts, extra) in spline_defs {
            if partial_match(name, "bird_") || partial_match(name, "DragSpline") || t.splines.len() >= 0x40 || pts.len() < 2 {
                continue;
            }
            t.splines.push(RaceSpline::new(name, pts, extra));
        }
        // weights: track.xml (all matching names), then the eventdef override (first match)
        for s in t.splines.iter_mut() {
            for w in track_xml_weights.iter().filter(|w| w.name.eq_ignore_ascii_case(&s.name)) {
                s.min_ai_weighting = w.min_ai_weighting;
                s.max_ai_weighting = w.max_ai_weighting;
            }
        }
        if let Some(ev) = event {
            for s in t.splines.iter_mut() {
                if let Some(w) = ev.spline_override(&s.name) {
                    s.min_ai_weighting = w.min_ai_weighting;
                    s.max_ai_weighting = w.max_ai_weighting;
                }
            }
        }
        // both weightings 0 -> type 1 (not a race line); race lines with a different node count than the first one are deleted
        for s in t.splines.iter_mut() {
            if s.min_ai_weighting == 0.0 && s.max_ai_weighting == 0.0 && s.kind == SplineKind::Race {
                s.kind = SplineKind::Disabled;
            }
        }
        let mut first_count: Option<usize> = None;
        let mut keep = Vec::with_capacity(t.splines.len());
        for s in t.splines.drain(..) {
            if s.kind == SplineKind::Race {
                match first_count {
                    None => first_count = Some(s.count()),
                    Some(c) if c != s.count() => continue,
                    _ => {}
                }
            }
            keep.push(s);
        }
        t.splines = keep;
        // helpers (the env loader keeps the LAST match of each kind)
        for (name, p) in helpers {
            if partial_match(name, "finishline") || partial_match(name, "spline_finishline") {
                t.finish_helper = Some(*p);
            } else if partial_match(name, "startline") || partial_match(name, "spline_startline") {
                t.start_helper = Some(*p);
            } else if partial_match(name, "trackendline") || partial_match(name, "spline_trackendline") {
                t.trackend_helper = Some(*p);
            } else if partial_match(name, "spline_smashline") {
                t.smash_helper = Some(*p);
            } else if partial_match(name, "spline_splittime") {
                t.splittime_helpers.push(*p);
            }
        }
        // per race spline markup distances (SetupEnvironmentMarkup, listing @0011e550..)
        let (fh, sh, th, mh) = (t.finish_helper, t.start_helper, t.trackend_helper, t.smash_helper);
        for s in t.splines.iter_mut().filter(|s| s.kind == SplineKind::Race) {
            if let Some(p) = fh {
                let (pos, _) = s.closest_pos(p);
                s.finish_pos = pos;
                s.finish_dist = s.dist_at(pos);
            } else {
                s.finish_dist = s.length - 5.0;
                s.finish_pos = s.pos_from_distance(s.finish_dist);
            }
            s.start_dist = match sh {
                Some(p) => {
                    let (pos, _) = s.closest_pos(p);
                    s.dist_at(pos)
                }
                None => s.nodes[0].cum_dist,
            };
            s.trackend_dist = match th {
                Some(p) => {
                    let (pos, _) = s.closest_pos(p);
                    s.dist_at(pos)
                }
                None => s.finish_dist + 50.0,
            };
            s.smash_dist = mh.map(|p| {
                let (pos, _) = s.closest_pos(p);
                s.dist_at(pos)
            });
        }
        t
    }

    /// From a parsed `track.stm` (`abgtool::stm::parse(..).pvs` or `parse_pvs_only`) and the `track.xml` text.
    pub fn from_stm(pvs: &abgtool::stm::Pvs, track_xml: &str, event: Option<&EventDef>) -> RaceTrack {
        let defs: Vec<(String, Vec<[f32; 3]>, Vec<[f32; 7]>)> =
            pvs.splines.iter().map(|s| (s.name.clone(), s.points.clone(), s.extra.clone())).collect();
        let helpers: Vec<(String, Vec3)> =
            pvs.helpers.iter().map(|h| (h.name.clone(), Vec3::new(h.matrix[12], h.matrix[13], h.matrix[14]))).collect();
        RaceTrack::from_parts(&defs, &helpers, &parse_track_xml_weights(track_xml), event)
    }

    /// First race line (the one `CGame::AddAI` falls back to / progress is measured on).
    pub fn main_line(&self) -> Option<usize> {
        self.splines.iter().position(|s| s.kind == SplineKind::Race)
    }
    pub fn race_lines(&self) -> Vec<usize> {
        (0..self.splines.len()).filter(|&i| self.splines[i].kind == SplineKind::Race).collect()
    }

    /// `CGame::AddAI @00114720` spline choice: weights `min + (max-min)*skill` over race lines, pick the first line whose running
    /// sum reaches `r = rand(0, sum)`; -1 when none.
    pub fn pick_ai_spline(&self, skill: f32, rng: &mut Rng) -> i32 {
        let w = |s: &RaceSpline| s.min_ai_weighting + (s.max_ai_weighting - s.min_ai_weighting) * skill;
        let total: f32 = self.splines.iter().filter(|s| s.kind == SplineKind::Race).map(w).sum();
        let r = rng.range(0.0, total);
        let mut acc = 0.0;
        for (i, s) in self.splines.iter().enumerate() {
            if s.kind == SplineKind::Race {
                acc += w(s);
                if r <= acc {
                    return i as i32;
                }
            }
        }
        -1
    }

    // ---- starting grid ----------------------------------------------------------------------------------

    /// Grid slot placement = `CCar::Respawn(slot)` (`slot >= 1`, 1-based: the local player is slot 1) on spline `spline_id`.
    ///
    /// Row 0 starts at `Lookahead(0.0, +30 m)` on an open spline (`-5.5 m` on a looped one); every further row is `-4 m` along the
    /// line (listing @0x1a5800/@0x1a5878). Odd slots take the left half of a row (`lateral = -s`), even slots the right (`+s`), with
    /// `s = 1.5` when the corridor is >= 6 m wide and `width/4` (3..6 m); rows narrower than 3 m are skipped. A candidate is skipped
    /// when it overlaps an already placed kart (`dist^2 < (r_self - 0.2 + r_other)^2`). Returns `(spline position, lateral, point, dir)`.
    // UNRESOLVED: the asymmetric-corridor branch (0x1a6278) is approximated by using the centre of the corridor as the lateral origin.
    pub fn grid_slot(&self, spline_id: usize, slot: usize, radius: f32, placed: &[(Vec3, f32)]) -> Option<(f32, f32, Vec3, Vec3)> {
        let s = self.splines.get(spline_id)?;
        let first = if s.is_loop { -5.5 } else { 30.0 };
        let (mut pos, _) = s.lookahead(0.0, first);
        let mut counter = 1usize;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 1000 {
                return None;
            }
            let (wl, wr) = (s.left_width(pos), s.right_width(pos));
            let width = wl + wr;
            if width >= 3.0 {
                let half = if width < 6.0 { width * 0.5 * 0.5 } else { 1.5 };
                let limit = if width < 6.0 { width * 0.5 } else { 3.0 };
                let centre = if limit > wl || limit > wr { width * 0.5 - wl } else { 0.0 };
                let left_side = slot == counter;
                let right_side = slot == counter + 1;
                if left_side || right_side {
                    let lateral = if left_side { centre - half } else { centre + half };
                    let (pt, dir) = s.info(pos, lateral);
                    let clear = placed.iter().all(|(pp, pr)| (*pp - pt).length_squared() >= (radius - 0.2 + pr) * (radius - 0.2 + pr));
                    if clear {
                        return Some((pos, lateral, pt, dir));
                    }
                    // blocked: restart the row search from this slot's own counter
                    counter = slot;
                } else {
                    counter += 2;
                }
            }
            let (np, _) = s.lookahead(pos, -4.0);
            if !s.is_loop && np >= pos {
                return None; // ran off the start of an open line
            }
            pos = np;
        }
    }
}

// =============================================================================================================
// Per-kart spline progress (CCar+0x1a88 spline id, +0x1a8c position)
// =============================================================================================================

#[derive(Clone, Debug)]
pub struct SplineTracker {
    pub spline_id: usize,
    /// `CCar+0x1a8c`: node index + fraction
    pub pos: f32,
    /// `CCar+0x1a94`: position of the previous frame (`CGameModeManager::Update` copies 1a8c -> 1a94)
    pub prev_pos: f32,
    /// `CCar+0x1a98`: lateral offset from the line
    pub lateral: f32,
}

impl SplineTracker {
    pub fn new(spline_id: usize, pos: f32) -> SplineTracker {
        SplineTracker { spline_id, pos, prev_pos: pos, lateral: 0.0 }
    }

    /// `CCar::Update` progress tracking (`GetNewPos` from the previous node + corridor check, other parallel race lines are tried
    /// when the kart left the corridor of the current one).
    // UNRESOLVED: the exact height / corridor thresholds of the line switch (`CCar::Update @001a6cd8`, ~lines 194335..194600) were not
    // ported one for one; the rule here is "stay on the current line unless another line of the same group has |lateral| smaller
    // than half the current corridor and a height within [-10, 5] of the kart".
    pub fn update(&mut self, track: &RaceTrack, p: Vec3) {
        self.prev_pos = self.pos;
        let Some(s) = track.splines.get(self.spline_id) else { return };
        let (np, _lap) = s.new_pos((self.pos as usize).min(s.count() - 1), p);
        let lat = s.lateral_offset(np, p);
        let in_corridor = lat >= -s.left_width(np) && lat <= s.right_width(np);
        if in_corridor {
            self.pos = np;
            self.lateral = lat;
            return;
        }
        let mut best: Option<(usize, f32, f32)> = None;
        for (i, o) in track.splines.iter().enumerate() {
            if i == self.spline_id || o.kind != SplineKind::Race || o.count() != s.count() {
                continue;
            }
            let (op, _) = o.new_pos((self.pos as usize).min(o.count() - 1), p);
            let ol = o.lateral_offset(op, p);
            let dy = o.height(op) - p.y;
            if ol >= -o.left_width(op) && ol <= o.right_width(op) && dy > -10.0 && dy < 5.0 && best.map_or(true, |b| ol.abs() < b.2.abs()) {
                best = Some((i, op, ol));
            }
        }
        match best {
            Some((i, op, ol)) => {
                self.spline_id = i;
                self.pos = op;
                self.lateral = ol;
            }
            None => {
                self.pos = np;
                self.lateral = lat;
            }
        }
    }

    /// `GetRaceTotalSplineDist @001a43e8`
    pub fn distance(&self, track: &RaceTrack) -> f32 {
        track.splines.get(self.spline_id).map_or(0.0, |s| s.dist_at(self.pos))
    }
    /// `GetRaceTotalSplineDistFromStart @001a444c`
    pub fn distance_from_start(&self, track: &RaceTrack) -> f32 {
        track.splines.get(self.spline_id).map_or(0.0, |s| s.dist_at(self.pos) - s.start_dist)
    }
    /// fraction of the line (`pos / spline+0x34`, clamped to 0..1) the rubber banding uses
    pub fn progress_fraction(&self, track: &RaceTrack) -> f32 {
        track.splines.get(self.spline_id).map_or(0.0, |s| (self.pos / s.finish_pos).clamp(0.0, 1.0))
    }
}

// =============================================================================================================
// Race state
// =============================================================================================================

/// What the caller tells the rules about each kart each frame (all positions in world / spline space).
#[derive(Clone, Debug)]
pub struct KartObs {
    pub id: usize,
    pub pos: Vec3,
    pub vel: Vec3,
    pub forward: Vec3,
    pub speed: f32,
    /// human controlled (`CCar+0x1af0 != 0` CPlayer)
    pub is_player: bool,
    /// the local human (`CPlayer::IsLocalPlayer`)
    pub is_local: bool,
    /// `CCar+0x1ae8`: the kart is running (physics active, not yet finished/removed)
    pub running: bool,
    /// `CCar+0x1b7c > 0`: the race clock runs for this kart (it left the slingshot)
    pub launched: bool,
    /// `CCar+0x4d4 != 0` (kart destroyed / dnf flag read by `CheckGameOverCondition`)
    pub destroyed: bool,
    /// `CCar+0x464 != 0 && +0x468 >= 0`: in the slingshot
    pub in_slingshot: bool,
    /// SEED_RUSH: fruits collected so far
    pub fruit: i32,
    /// kart bounding radius (`CCarSpec` sphere `+0xcc`), used by the grid clearance test
    pub radius: f32,
}
impl KartObs {
    pub fn new(id: usize, pos: Vec3) -> KartObs {
        KartObs {
            id,
            pos,
            vel: Vec3::ZERO,
            forward: Vec3::Z,
            speed: 0.0,
            is_player: false,
            is_local: false,
            running: true,
            launched: true,
            destroyed: false,
            in_slingshot: false,
            fruit: 0,
            radius: 1.2,
        }
    }
}

/// Per-kart race state (`CGameModeData`): `+4 state`, `+8 time`, `+0xc finish position`, `+0x10 current position`,
/// `+0x14 best position`; TIME_ATTACK `+0x20 duration / +0x24 remaining`; SEED_RUSH `+0x20 collected / +0x24 target / +0x28 fraction`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CarState {
    Racing = 1,
    /// finished, `GetEventCompleted` true
    Success = 2,
    /// finished, `GetEventCompleted` false
    Failed = 3,
    /// forced by a boss battle (`piVar13[1] == 5`): win
    ForcedWin = 5,
    /// forced by a boss battle (`== 6`): loss
    ForcedLoss = 6,
}

#[derive(Clone, Debug)]
pub struct CarRace {
    pub id: usize,
    pub tracker: SplineTracker,
    pub state: CarState,
    /// `CGameModeData+8` seconds the race clock ran for this kart
    pub time_s: f32,
    /// `CGameModeData+0xc`; 0 until the kart finishes
    pub finish_pos: i32,
    /// `CGameModeData+0x10` (position at the last update) / `+0x14` best (lowest) position seen
    pub cur_pos: i32,
    pub best_pos: i32,
    /// `CCar+0x1a9c` current race position (1 = leading)
    pub position: i32,
    /// `CCar+0x1b5c` race completed / `+0x1b60` finish line crossed
    pub race_completed: bool,
    pub finish_crossed: bool,
    /// `CCar+0x1b80` mode clock (seconds, `CGameMode+0x1c += dt`, listing @0017cac0..) at completion
    pub finish_time_s: f32,
    /// TIME_ATTACK / SLALOM timer: `+0x20` duration, `+0x24` remaining (seconds; may go negative)
    pub timer_duration: f32,
    pub timer_remaining: f32,
    pub timer_warned: bool,
    /// SEED_RUSH
    pub fruit_collected: i32,
    pub fruit_target: i32,
    /// `+0x1ae8`
    pub running: bool,
    /// `+0x4d4`
    pub destroyed: bool,
    pub bonus_coins: i32,
    pub is_player: bool,
    pub is_local: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// before the grid: game state 6 camera fly-by (timing UNRESOLVED, the caller advances it)
    Intro,
    /// `CGame` state 5: karts held on the grid / in the slingshots, AI frozen (`CRaceAI::Process` 0x18b868)
    Grid,
    /// state 8: race clock runs
    Racing,
    /// state 9: all local players done, stragglers finish (`CGameMode::UpdateGameEnd`)
    Finishing,
    Results,
}

#[derive(Clone, Debug)]
pub enum RaceEvent {
    PhaseChanged(Phase),
    /// a kart crossed the line (`SetFinishLineCrossed` + `SetRaceCompleted`)
    Finished { id: usize, position: i32, time_s: f32, state: CarState },
    /// TIME_ATTACK timer dropped below 5 s (`OnEvent(0x1f)` warning sound)
    TimerWarning { id: usize },
    /// `CGameModeManager::CheckGameOverCondition` became true
    GameOver,
}

#[derive(Clone, Debug)]
pub struct ResultRow {
    pub id: usize,
    pub position: i32,
    pub time_s: Option<f32>,
    pub state: CarState,
    pub is_player: bool,
    pub bonus_coins: i32,
}

#[derive(Clone, Debug, Default)]
pub struct RaceResults {
    pub rows: Vec<ResultRow>,
    /// the local player's row, score, stars, rewards
    pub player: Option<PlayerResult>,
}

#[derive(Clone, Debug)]
pub struct PlayerResult {
    pub id: usize,
    pub position: i32,
    pub success: bool,
    pub time_s: Option<f32>,
    pub score: ScoreBreakdown,
    pub stars: u8,
    pub rewards: Vec<Reward>,
    pub bonus_coins: i32,
}

pub struct RaceState {
    pub event: EventDef,
    pub track: RaceTrack,
    pub cfg: RulesConfig,
    pub mode: GameMode,
    pub phase: Phase,
    /// `CGameMode+0x1c`: race clock in seconds (`ProcessUpdate @0017caac` adds the frame dt)
    pub clock_s: f32,
    /// `CGame+0xd4`: seconds in the current phase
    pub phase_time: f32,
    pub cars: Vec<CarRace>,
    pub difficulty_adjust: usize,
    pub race_cc: i32,
    pub event_index: Option<i32>,
    pub stars_before: u8,
    pub score_inputs: ScoreInputs,
    results: Option<RaceResults>,
    game_over: bool,
}

impl RaceState {
    /// `RaceState::new(eventdef_xml, track data)`: `kart_count` = number of karts (1 player + AI); `adj` = `EDifficultyAdjust` the
    /// game computed with `RulesConfig.economy.difficulty_adjust_enum(event_cc, kart_cc)`.
    pub fn new(eventdef_xml: &str, track: RaceTrack, cfg: RulesConfig, kart_count: usize, adj: usize) -> Result<RaceState, String> {
        let event = EventDef::parse(eventdef_xml)?;
        Ok(RaceState::from_event(event, track, cfg, kart_count, adj))
    }

    pub fn from_event(event: EventDef, track: RaceTrack, cfg: RulesConfig, kart_count: usize, adj: usize) -> RaceState {
        let mode = event.mode;
        let event_theme = event.theme;
        let main = track.main_line().unwrap_or(0);
        let mut cars = Vec::with_capacity(kart_count);
        for id in 0..kart_count {
            let mut c = CarRace {
                id,
                tracker: SplineTracker::new(main, 0.0),
                state: CarState::Racing,
                time_s: 0.0,
                finish_pos: 0,
                cur_pos: 0,
                best_pos: 1000,
                position: id as i32 + 1,
                race_completed: false,
                finish_crossed: false,
                finish_time_s: 0.0,
                timer_duration: 0.0,
                timer_remaining: 0.0,
                timer_warned: false,
                fruit_collected: 0,
                fruit_target: 0,
                running: true,
                destroyed: false,
                bonus_coins: 0,
                is_player: false,
                is_local: false,
            };
            if mode.has_timer() {
                // CGameModeTimeAttackData::Reset: duration = GetTimerDuration(CalcDifficultyAdjustEnum)
                c.timer_duration = event.timer_duration(adj);
                c.timer_remaining = c.timer_duration;
            }
            if matches!(mode, GameMode::SeedRush | GameMode::BossFruitRush) {
                c.fruit_target = event.token_threshold(adj);
            }
            cars.push(c);
        }
        RaceState {
            event,
            track,
            cfg,
            mode,
            phase: Phase::Intro,
            clock_s: 0.0,
            phase_time: 0.0,
            cars,
            difficulty_adjust: adj,
            race_cc: 0,
            event_index: None,
            stars_before: 0,
            score_inputs: ScoreInputs { episode: (event_theme - 2).clamp(0, 5), ..ScoreInputs::default() },
            results: None,
            game_over: false,
        }
    }

    pub fn set_phase(&mut self, p: Phase, ev: &mut Vec<RaceEvent>) {
        if self.phase != p {
            self.phase = p;
            self.phase_time = 0.0;
            ev.push(RaceEvent::PhaseChanged(p));
        }
    }

    /// Intro -> Grid (`CGame` state 5: `CGameModeManager::StartGame`).
    pub fn begin_grid(&mut self) -> Vec<RaceEvent> {
        let mut ev = Vec::new();
        self.set_phase(Phase::Grid, &mut ev);
        ev
    }
    /// Grid -> Racing: called when the player releases the slingshot (single player) or the countdown reaches GO.
    pub fn go(&mut self) -> Vec<RaceEvent> {
        let mut ev = Vec::new();
        self.set_phase(Phase::Racing, &mut ev);
        ev
    }

    /// Grid slot positions for `kart_count` karts on `spline_id` (slot 1 = player, 2.. = AI in order), one `grid_slot` each.
    pub fn grid(&self, spline_id: usize, radii: &[f32]) -> Vec<Option<(f32, f32, Vec3, Vec3)>> {
        let mut placed: Vec<(Vec3, f32)> = Vec::new();
        let mut out = Vec::new();
        for (i, &r) in radii.iter().enumerate() {
            let g = self.track.grid_slot(spline_id, i + 1, r, &placed);
            if let Some((_, _, pt, _)) = g {
                placed.push((pt, r));
            }
            out.push(g);
        }
        out
    }

    /// Place the trackers at the given world positions (after `grid`).
    pub fn reset_trackers(&mut self, positions: &[Vec3]) {
        for (c, p) in self.cars.iter_mut().zip(positions) {
            let s = &self.track.splines[c.tracker.spline_id];
            let (pos, _) = s.closest_pos(*p);
            c.tracker = SplineTracker::new(c.tracker.spline_id, pos);
        }
    }

    pub fn set_ai_spline(&mut self, car: usize, spline_id: usize) {
        if let Some(c) = self.cars.get_mut(car) {
            c.tracker.spline_id = spline_id;
        }
    }

    /// `CGame::CalculateRacePositions @001191e4`: karts that are running and not completed are sorted by spline position
    /// (descending); they get ranks starting at `(number of excluded karts) + 1`.
    pub fn calculate_positions(&mut self) {
        let excluded = self.cars.iter().filter(|c| !c.running || c.race_completed).count();
        let mut list: Vec<(usize, f32)> =
            self.cars.iter().enumerate().filter(|(_, c)| c.running && !c.race_completed).map(|(i, c)| (i, c.tracker.pos)).collect();
        // TCarSortData_Comparator @00111f48: a goes first when its parameter is larger
        list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (k, (i, _)) in list.iter().enumerate() {
            self.cars[*i].position = (excluded + k + 1) as i32;
        }
    }

    /// `CGameMode::CheckFinishLineCrossed @0017c684` / the check inside `ProcessUpdate`.
    pub fn finish_line_crossed(&self, car: &CarRace, p: Vec3) -> bool {
        let Some(s) = self.track.splines.get(car.tracker.spline_id) else { return false };
        if let Some(h) = self.track.finish_helper {
            let node = s.closest_node(h);
            if s.offset(p, node) >= 0.0 {
                return true;
            }
        }
        s.dist_at(car.tracker.pos) > s.finish_dist
    }

    /// `GetEventCompleted(CCar*)` per mode.
    pub fn event_completed(&self, car: &CarRace) -> bool {
        match self.mode {
            GameMode::Race => car.cur_pos < 4,
            GameMode::Lmr => car.cur_pos < 3,
            GameMode::Versus | GameMode::BossBattle | GameMode::Intro | GameMode::Intro2 | GameMode::Intro3 => car.cur_pos == 1,
            GameMode::TimeAttack | GameMode::Slalom => car.timer_remaining >= 0.0,
            GameMode::SeedRush | GameMode::BossFruitRush => {
                car.fruit_target > 0 && car.fruit_collected as f32 / car.fruit_target as f32 >= 1.0
            }
            _ => car.cur_pos < 4,
        }
    }

    /// `GetBonusCoins(CCar*)`: TIME_ATTACK / SLALOM `max(0, (int)remaining) * 10`, else 0.
    pub fn bonus_coins(&self, car: &CarRace) -> i32 {
        match self.mode {
            GameMode::TimeAttack | GameMode::Slalom => (car.timer_remaining as i32).max(0) * 10,
            _ => 0,
        }
    }

    /// `CGameModeManager::CheckGameOverCondition @00184f08`: every human kart is destroyed, not racing-and-not-finished, or not running.
    pub fn check_game_over(&self) -> bool {
        let humans: Vec<&CarRace> = self.cars.iter().filter(|c| c.is_player).collect();
        if humans.is_empty() {
            return true;
        }
        humans.iter().all(|c| {
            // done = `4d4 != 0 || (!finished && state != 1) || !running`; a finished car only counts once it stopped running
            c.destroyed || (!c.finish_crossed && c.state != CarState::Racing) || !c.running
        })
    }

    /// One frame. `karts` are the karts in `id` order (`karts[i].id == i`).
    pub fn update(&mut self, dt: f32, karts: &[KartObs]) -> Vec<RaceEvent> {
        let mut ev = Vec::new();
        self.phase_time += dt;
        // sync observable flags
        for k in karts {
            if let Some(c) = self.cars.get_mut(k.id) {
                c.is_player = k.is_player;
                c.is_local = k.is_local;
                c.running = k.running;
                c.destroyed = k.destroyed;
                if matches!(self.mode, GameMode::SeedRush | GameMode::BossFruitRush) {
                    c.fruit_collected = k.fruit;
                }
            }
        }
        // progress along the spline (tracker owns the incremental position), then positions
        for k in karts {
            if let Some(c) = self.cars.get_mut(k.id) {
                if c.running && !c.race_completed {
                    c.tracker.update(&self.track, k.pos);
                }
            }
        }
        if self.phase == Phase::Intro || self.phase == Phase::Results {
            return ev;
        }
        self.calculate_positions();

        if self.phase == Phase::Racing {
            self.clock_s += dt;
        }
        // CGameMode::ProcessUpdate per kart
        for k in karts {
            let idx = k.id;
            if idx >= self.cars.len() {
                continue;
            }
            // CGameModeData::Update (always)
            {
                let c = &mut self.cars[idx];
                c.cur_pos = c.position;
                c.best_pos = c.best_pos.min(c.cur_pos);
                if c.state == CarState::Racing && k.launched && self.phase == Phase::Racing {
                    c.time_s += dt;
                    if self.mode.has_timer() {
                        // CGameModeTimeAttackData::Update @00186ab8
                        if !c.timer_warned && c.timer_remaining < 5.0 {
                            c.timer_warned = true;
                            ev.push(RaceEvent::TimerWarning { id: idx });
                        }
                        c.timer_remaining -= dt;
                    }
                }
            }
            if !self.cars[idx].running || self.cars[idx].race_completed {
                continue;
            }
            if self.cars[idx].state == CarState::Racing && self.phase != Phase::Grid {
                if self.finish_line_crossed(&self.cars[idx], k.pos) {
                    self.complete_car(idx, &mut ev);
                }
            } else if matches!(self.cars[idx].state, CarState::ForcedWin | CarState::ForcedLoss) {
                self.complete_car(idx, &mut ev);
            }
        }
        // game over (CGameModeManager::Update returns 1 -> CGame state 9)
        // CGameModeManager::Update returns 1 -> CGame state 9; the results exist from this moment, stragglers keep finishing
        // (`CGameMode::UpdateGameEnd`) and refresh them. The caller moves to `Results` with `show_results()` when its end-of-race
        // camera / UI timing is over (UNRESOLVED: the original delays come from the finish camera + fireworks).
        if self.phase == Phase::Racing && self.check_game_over() {
            ev.push(RaceEvent::GameOver);
            self.game_over = true;
            self.set_phase(Phase::Finishing, &mut ev);
            self.finalize();
        } else if self.phase == Phase::Finishing && ev.iter().any(|e| matches!(e, RaceEvent::Finished { .. })) {
            self.finalize();
        }
        ev
    }

    fn complete_car(&mut self, idx: usize, ev: &mut Vec<RaceEvent>) {
        let success = self.event_completed(&self.cars[idx]);
        let bonus = self.bonus_coins(&self.cars[idx]);
        let clock = self.clock_s;
        let c = &mut self.cars[idx];
        c.finish_crossed = true;
        if !matches!(c.state, CarState::ForcedWin | CarState::ForcedLoss) {
            c.state = if success { CarState::Success } else { CarState::Failed };
        }
        c.finish_pos = c.position;
        c.bonus_coins = bonus;
        // SetRaceCompleted: stores the mode clock and the position
        c.race_completed = true;
        c.finish_time_s = clock;
        c.running = false;
        ev.push(RaceEvent::Finished { id: idx, position: c.finish_pos, time_s: clock, state: c.state });
        // the local player finishing fills in the positions of everybody still racing (ProcessUpdate @0017cf28..)
        if c.is_local {
            for o in self.cars.iter_mut() {
                if o.finish_pos == 0 {
                    o.finish_pos = o.position;
                }
            }
        }
    }

    /// External end of a kart's event (boss battle win / loss etc.): `piVar13[1] = 5 / 6`.
    pub fn force_end(&mut self, car: usize, win: bool) {
        if let Some(c) = self.cars.get_mut(car) {
            c.state = if win { CarState::ForcedWin } else { CarState::ForcedLoss };
        }
    }

    fn finalize(&mut self) {
        // positions for the unfinished: current position (data+0xc is only set when the local player finished)
        let mut rows: Vec<ResultRow> = self
            .cars
            .iter()
            .map(|c| ResultRow {
                id: c.id,
                position: if c.finish_pos > 0 { c.finish_pos } else { c.position },
                time_s: if c.race_completed { Some(c.finish_time_s) } else { None },
                state: c.state,
                is_player: c.is_player,
                bonus_coins: c.bonus_coins,
            })
            .collect();
        rows.sort_by_key(|r| r.position);
        let mut res = RaceResults { rows, player: None };
        if let Some(c) = self.cars.iter().find(|c| c.is_local).or_else(|| self.cars.iter().find(|c| c.is_player)) {
            let success = matches!(c.state, CarState::Success | CarState::ForcedWin);
            let pos = if c.finish_pos > 0 { c.finish_pos } else { c.position };
            let mut inp = self.score_inputs.clone();
            if matches!(self.mode, GameMode::SeedRush | GameMode::BossFruitRush) && c.fruit_target > 0 {
                inp.fruit_fraction = c.fruit_collected as f32 / c.fruit_target as f32;
            }
            let score = self.cfg.score.total_score(self.mode, self.race_cc, pos, c.timer_remaining, c.timer_duration, &inp);
            // stars are only stored for a successful event (`CResultsScreen+0x14c`), thresholds compared with `>=`
            let finished = c.race_completed;
            let _ = finished;
            let stars = self.event.results_stars(score.total, success);
            let rewards = match self.event_index {
                Some(i) if success => self.cfg.events.rewards_for(i, self.stars_before, stars),
                _ => Vec::new(),
            };
            res.player = Some(PlayerResult {
                id: c.id,
                position: pos,
                success,
                time_s: if c.race_completed { Some(c.finish_time_s) } else { None },
                score,
                stars,
                rewards,
                bonus_coins: c.bonus_coins,
            });
        }
        self.results = Some(res);
    }

    /// `Finishing` -> `Results`.
    pub fn show_results(&mut self) -> Vec<RaceEvent> {
        let mut ev = Vec::new();
        if self.results.is_none() {
            self.finalize();
        }
        self.set_phase(Phase::Results, &mut ev);
        ev
    }

    /// Binds the event to the campaign tables: `eventdef_episode01_event03_stage02(.xml)` -> `<Event index>` -> `CampaignEvent`
    /// (`campaignCC` = `CScoreSystem::SetRaceCC`, rewards). Returns false when the file is not a campaign event.
    pub fn bind_campaign_event(&mut self, file_stem: &str) -> bool {
        let Some(idx) = self.cfg.events.event_index_for_file(file_stem) else { return false };
        self.event_index = Some(idx);
        if let Some(ce) = self.cfg.events.campaign_event(idx) {
            self.race_cc = ce.campaign_cc;
        }
        true
    }

    pub fn results(&self) -> Option<&RaceResults> {
        self.results.as_ref()
    }

    pub fn is_game_over(&self) -> bool {
        self.game_over
    }
}

// =============================================================================================================
// Tests
// =============================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn line(n: usize, step: f32) -> (String, Vec<[f32; 3]>, Vec<[f32; 7]>) {
        let pts: Vec<[f32; 3]> = (0..n).map(|i| [0.0, 0.0, i as f32 * step]).collect();
        let ex: Vec<[f32; 7]> = (0..n).map(|_| [0.0, 1.0, 0.0, 0.0, 10.0, 10.0, 0.0]).collect();
        ("race_001".to_string(), pts, ex)
    }

    fn track(n: usize, with_finish: bool) -> RaceTrack {
        let l = line(n, 10.0);
        let mut helpers = vec![("spline_startline".to_string(), Vec3::new(0.0, 0.0, 30.0))];
        if with_finish {
            helpers.push(("spline_finishline".to_string(), Vec3::new(0.0, 0.0, (n as f32 - 5.0) * 10.0)));
        }
        RaceTrack::from_parts(&[l], &helpers, &[], None)
    }

    fn cfg() -> RulesConfig {
        RulesConfig { economy: Economy::default(), score: ScoreConfig::default(), events: EventTable::default() }
    }

    const EV_RACE: &str = r#"<EventDefinition><Name tag="0000" title="t" description="d"/><Environment pathname="environments\theme003\tracks\run000" config="1"/><Character name="red"/><GameMode name="RACE" kartcount="5"/><Difficulty level="0.325" level_index="37" boss_bonus="0.0" tier="0"/><Stars Star1="14000" Star2="16000" Star3="17000"/><Spline name="race_001" min_ai_weighting="0.5" max_ai_weighting="0.5"/><DifficultyAdjust><VeryEasy>0.0</VeryEasy><Easy>0.025</Easy><Medium>0.065</Medium><Hard>0.1</Hard><Impossible>0.2</Impossible></DifficultyAdjust></EventDefinition>"#;

    fn obs(id: usize, z: f32) -> KartObs {
        let mut k = KartObs::new(id, Vec3::new(0.0, 0.5, z));
        k.is_player = id == 0;
        k.is_local = id == 0;
        k
    }

    #[test]
    fn eventdef_parse_defaults_and_stars() {
        let ev = EventDef::parse(EV_RACE).unwrap();
        assert_eq!(ev.mode, GameMode::Race);
        assert_eq!(ev.kart_count, Some(5));
        assert_eq!((ev.theme, ev.run), (3, 0));
        assert_eq!(ev.stars, [14000, 16000, 17000]);
        assert_eq!(ev.difficulty.timer, [60.0, 47.5, 35.0, 22.5, 10.0]);
        assert_eq!(ev.difficulty.seed_amount, [60, 47, 35, 22, 10]);
        assert!((ev.difficulty_adjust[3] - 0.1).abs() < 1e-6);
        // the VERSUS/INTRO dummies use `1=` `2=` `3=`: never read, thresholds stay 0
        let v = EventDef::parse(r#"<EventDefinition><GameMode name="VERSUS" kartcount="2"/><Stars 1="1000" 2="20000" 3="25000"/></EventDefinition>"#)
            .unwrap();
        assert_eq!(v.stars, [0, 0, 0]);
        assert_eq!(v.mode, GameMode::Versus);
        // monotonic fix-up
        let m = EventDef::parse(r#"<EventDefinition><GameMode name="RACE"/><Stars Star1="500" Star2="100" Star3="50"/></EventDefinition>"#).unwrap();
        assert_eq!(m.stars, [500, 500, 500]);
    }

    #[test]
    fn game_mode_names() {
        assert_eq!(GameMode::from_name("SEED_RUSH"), GameMode::SeedRush);
        assert_eq!(GameMode::from_name("TMR").id(), 9);
        assert_eq!(GameMode::from_name("LMR").id(), 14);
        assert_eq!(GameMode::from_name("JENGA").id(), 10);
        assert_eq!(GameMode::from_name("BOSS_BATTLE").id(), 11);
        assert!(!GameMode::Qmr.creatable_offline());
    }

    #[test]
    fn star_thresholds_and_floor() {
        let ev = EventDef::parse(EV_RACE).unwrap();
        assert_eq!(ev.stars_from_score(17001, true), 3);
        assert_eq!(ev.stars_from_score(17000, true), 2);
        assert_eq!(ev.stars_from_score(16001, true), 2);
        assert_eq!(ev.stars_from_score(16000, true), 1);
        assert_eq!(ev.stars_from_score(14001, true), 1);
        // the original floors a finished car at 1 star: Star1 is never an extra step
        assert_eq!(ev.stars_from_score(0, true), 1);
        assert_eq!(ev.stars_from_score(99999, false), 0);
    }

    #[test]
    fn results_star_rule_is_non_strict_and_needs_success() {
        // C01 thresholds 4100 / 6200 / 8300 = ceil100 of the finishing bonus of 3rd / 1st place
        let ev = EventDef::parse(r#"<EventDefinition><GameMode name="RACE" kartcount="3"/><Stars Star1="4100" Star2="6200" Star3="8300"/></EventDefinition>"#).unwrap();
        for (score, stars) in [(8300, 3), (8299, 2), (6200, 2), (6199, 1), (4100, 1), (4099, 0)] {
            assert_eq!(ev.results_stars(score, true), stars, "score {score}");
        }
        // a failed event stores no stars (CResultsScreen+0x14c = player state 2 or 5)
        assert_eq!(ev.results_stars(99999, false), 0);
        // the challenge-code variant is strict: 8300 is only 2 stars there
        assert_eq!(ev.stars_from_score(8300, true), 2);
    }

    #[test]
    fn score_formulas() {
        let c = ScoreConfig::default();
        // C01: campaignCC 55 -> scale 8250; 3 karts: 1st place = 8/8, 3rd = 6/8
        assert_eq!(c.position_score(8250, 1), 8250);
        assert_eq!(c.position_score(8250, 2), 7218);
        assert_eq!(c.position_score(8250, 3), 6187);
        // deaths: scale / 1.5 * 0.5^deaths
        assert_eq!(c.deaths_score(8250, 0), 5500);
        assert_eq!(c.deaths_score(8250, 1), 2750);
        // time score has the original (x - x) term: always scale * MaxTimeScore
        assert_eq!(c.time_score(8250, -50.0, 37.0), 8250);
        assert_eq!(c.time_score(8250, 20.0, 37.0), 8250);
        // fruit: 12.5% .. 100%
        assert_eq!(c.fruit_score(8000, 0.0), 1000);
        assert_eq!(c.fruit_score(8000, 1.0), 8000);
        let inp = ScoreInputs { kart_cc: 100, top_speed_seconds: 10.0, accel_seconds: 4.0, deaths: 0, episode: 0, ..Default::default() };
        // (150*10*100^0.1 + 100*50) * 0.9 = (2377.37 + 5000) * 0.9
        let ts = c.top_speed_score(&inp);
        assert!((ts - 6639).abs() <= 1, "{ts}");
        // (0.5*4*100^1 + 100*35) * 1.0 = 3700
        assert_eq!(c.acceleration_score(&inp), 3700);
        let b = c.total_score(GameMode::Race, 55, 1, 0.0, 0.0, &inp);
        assert_eq!(b.finishing, 8250);
        assert_eq!(b.bonus_rounded, 8300); // GetBonusScore rounds UP to 100
        assert_eq!(b.total, b.performance + 8300);
    }

    #[test]
    fn calc_difficulty_adjust_thresholds() {
        let e = Economy::default(); // relativeCC 4,0,-2,-4,-8
        assert_eq!(e.difficulty_adjust_enum(10, 100), 0);
        assert_eq!(e.difficulty_adjust_enum(96, 100), 0);
        assert_eq!(e.difficulty_adjust_enum(97, 100), 1);
        assert_eq!(e.difficulty_adjust_enum(100, 100), 1);
        assert_eq!(e.difficulty_adjust_enum(102, 100), 2);
        assert_eq!(e.difficulty_adjust_enum(103, 100), 3);
        assert_eq!(e.difficulty_adjust_enum(104, 100), 3);
        assert_eq!(e.difficulty_adjust_enum(105, 100), 4);
        assert_eq!(e.race_max_score(55), 8250);
    }

    #[test]
    fn spline_geometry() {
        let t = track(50, true);
        let s = &t.splines[0];
        assert_eq!(s.kind, SplineKind::Race);
        assert!(!s.is_loop);
        assert!((s.length - 490.0).abs() < 1e-3);
        assert!((s.dist_at(2.5) - 25.0).abs() < 1e-4);
        assert!((s.pos_from_distance(25.0) - 2.5).abs() < 1e-4);
        let (p, _) = s.lookahead(2.5, 10.0);
        assert!((p - 3.5).abs() < 1e-4, "{p}");
        let (p, _) = s.lookahead(3.5, -10.0);
        assert!((p - 2.5).abs() < 1e-4, "{p}");
        // lateral offset: side = up x dir = (1,0,0) for dir +Z, up +Y -> positive = +x
        assert!((s.lateral_offset(2.0, Vec3::new(3.0, 0.0, 20.0)) - 3.0).abs() < 1e-5);
        // closest pos / new pos agree
        let (cp, _) = s.closest_pos(Vec3::new(1.0, 0.0, 25.0));
        assert!((cp - 2.5).abs() < 1e-4);
        let (np, _) = s.new_pos(2, Vec3::new(1.0, 0.0, 25.0));
        assert!((np - 2.5).abs() < 1e-4);
        // markup from the helpers: start at z=30 -> dist 30, finish at z=450 -> 450, track end = finish + 50
        assert!((s.start_dist - 30.0).abs() < 1.0);
        assert!((s.finish_dist - 450.0).abs() < 1.0);
        assert!((s.trackend_dist - 500.0).abs() < 1.5 || s.trackend_dist >= s.finish_dist);
        // curvature of a straight line is 0, radius is the cap
        assert!(s.nodes[10].curvature.abs() < 1e-5);
        assert_eq!(s.nodes[10].radius, SHARPNESS_CAP);
    }

    #[test]
    fn corner_radius_and_curvature() {
        // quarter circle of radius 100
        let n = 40;
        let pts: Vec<[f32; 3]> = (0..n)
            .map(|i| {
                let a = i as f32 / (n - 1) as f32 * std::f32::consts::FRAC_PI_2;
                [100.0 * (1.0 - a.cos()), 0.0, 100.0 * a.sin()]
            })
            .collect();
        let ex: Vec<[f32; 7]> = (0..n).map(|_| [0.0, 1.0, 0.0, 0.0, 10.0, 10.0, 0.0]).collect();
        let s = RaceSpline::new("race_001", &pts, &ex);
        let r = s.nodes[20].radius;
        assert!((r - 100.0).abs() < 15.0, "radius {r}");
        assert!(s.nodes[20].curvature.abs() > 0.01);
    }

    #[test]
    fn finish_crossing_by_distance_and_helper() {
        // with a finish helper the plane test fires; without it the distance test does
        let t = track(50, true);
        let mut st = RaceState::from_event(EventDef::parse(EV_RACE).unwrap(), t, cfg(), 2, 2);
        st.begin_grid();
        st.go();
        let mut karts = vec![obs(0, 100.0), obs(1, 50.0)];
        let ev = st.update(0.016, &karts);
        assert!(!ev.iter().any(|e| matches!(e, RaceEvent::Finished { .. })));
        karts[0].pos.z = 455.0; // behind the helper plane (450) is z<450; 455 is past it
        karts[0].pos.z = 452.0;
        let ev = st.update(0.016, &karts);
        assert!(ev.iter().any(|e| matches!(e, RaceEvent::Finished { id: 0, .. })), "{ev:?}");

        // no helper: pure distance (finish at length - 5 = 485)
        let t2 = track(50, false);
        let mut st2 = RaceState::from_event(EventDef::parse(EV_RACE).unwrap(), t2, cfg(), 1, 2);
        st2.begin_grid();
        st2.go();
        let mut k = vec![obs(0, 400.0)];
        st2.update(0.016, &k);
        assert!(!st2.cars[0].race_completed);
        k[0].pos.z = 487.0;
        let ev = st2.update(0.016, &k);
        assert!(ev.iter().any(|e| matches!(e, RaceEvent::Finished { id: 0, .. })));
        assert!(st2.is_game_over());
    }

    #[test]
    fn ranking_with_finished_cars() {
        let t = track(50, true);
        let mut st = RaceState::from_event(EventDef::parse(EV_RACE).unwrap(), t, cfg(), 4, 2);
        st.begin_grid();
        st.go();
        let mut karts = vec![obs(0, 100.0), obs(1, 200.0), obs(2, 150.0), obs(3, 50.0)];
        st.update(0.016, &karts);
        assert_eq!(
            st.cars.iter().map(|c| c.position).collect::<Vec<_>>(),
            vec![3, 1, 2, 4],
            "sorted by spline position, descending"
        );
        // kart 1 finishes: it keeps rank 1, the others are ranked after the excluded count
        karts[1].pos.z = 455.0;
        st.update(0.016, &karts);
        assert!(st.cars[1].race_completed);
        assert_eq!(st.cars[1].finish_pos, 1);
        // an inactive (not running) kart also takes a leading slot, the rest start after it
        karts[3].running = false;
        st.update(0.016, &karts);
        let pos: Vec<i32> = st.cars.iter().map(|c| c.position).collect();
        assert_eq!(pos[0], 4, "{pos:?}"); // 2 excluded (finished + inactive) -> ranks start at 3: kart2 (150) = 3, kart0 (100) = 4
        assert_eq!(pos[2], 3);
    }

    #[test]
    fn mode_success_rules() {
        let t = track(50, false);
        let mut st = RaceState::from_event(EventDef::parse(EV_RACE).unwrap(), t.clone(), cfg(), 1, 2);
        // RACE: positions 1..3 succeed, 4th fails (`GetEventCompleted`: position < 4)
        for (p, ok) in [(1, true), (3, true), (4, false)] {
            st.cars[0].cur_pos = p;
            assert_eq!(st.event_completed(&st.cars[0]), ok);
        }
        let ta = EventDef::parse(r#"<EventDefinition><GameMode name="TIME_ATTACK"/><Difficulty timer_medium="35.00"/></EventDefinition>"#).unwrap();
        let mut st = RaceState::from_event(ta, t.clone(), cfg(), 1, 2);
        assert_eq!(st.cars[0].timer_duration, 35.0);
        st.cars[0].timer_remaining = 3.7;
        assert!(st.event_completed(&st.cars[0]));
        assert_eq!(st.bonus_coins(&st.cars[0]), 30);
        st.cars[0].timer_remaining = -0.1;
        assert!(!st.event_completed(&st.cars[0]));
        assert_eq!(st.bonus_coins(&st.cars[0]), 0);
        let sr = EventDef::parse(r#"<EventDefinition><GameMode name="SEED_RUSH" kartcount="2"/><Difficulty seed_amount_medium="145"/></EventDefinition>"#).unwrap();
        let mut st = RaceState::from_event(sr, t, cfg(), 1, 2);
        assert_eq!(st.cars[0].fruit_target, 145);
        st.cars[0].fruit_collected = 144;
        assert!(!st.event_completed(&st.cars[0]));
        st.cars[0].fruit_collected = 145;
        assert!(st.event_completed(&st.cars[0]));
    }

    #[test]
    fn timer_counts_down_only_while_launched() {
        let t = track(50, false);
        let ta = EventDef::parse(r#"<EventDefinition><GameMode name="TIME_ATTACK"/><Difficulty timer_medium="6.0"/></EventDefinition>"#).unwrap();
        let mut st = RaceState::from_event(ta, t, cfg(), 1, 2);
        st.begin_grid();
        st.go();
        let mut k = vec![obs(0, 100.0)];
        k[0].launched = false;
        st.update(1.0, &k);
        assert_eq!(st.cars[0].timer_remaining, 6.0);
        k[0].launched = true;
        st.update(1.5, &k);
        assert!((st.cars[0].timer_remaining - 4.5).abs() < 1e-5);
        // the < 5 s warning is checked BEFORE the decrement (CGameModeTimeAttackData::Update @00186ab8), so it fires one frame later
        let ev = st.update(0.1, &k);
        assert!(ev.iter().any(|e| matches!(e, RaceEvent::TimerWarning { .. })));
        assert!(st.cars[0].timer_warned);
    }

    #[test]
    fn grid_rows_and_slots() {
        let t = track(50, false);
        let st = RaceState::from_event(EventDef::parse(EV_RACE).unwrap(), t, cfg(), 4, 2);
        let g = st.grid(0, &[1.2, 1.2, 1.2, 1.2]);
        let p: Vec<Vec3> = g.iter().map(|x| x.unwrap().2).collect();
        // open line: first row at +30 m, odd slots left (-1.5 along side = -x), even right
        assert!((p[0].z - 30.0).abs() < 1e-3, "{:?}", p[0]);
        assert!((p[0].x + 1.5).abs() < 1e-3);
        assert!((p[1].x - 1.5).abs() < 1e-3);
        assert!((p[1].z - 30.0).abs() < 1e-3);
        // second row 4 m behind
        assert!((p[2].z - 26.0).abs() < 1e-3);
        assert!((p[3].z - 26.0).abs() < 1e-3);
    }

    #[test]
    fn ai_spline_pick_weights() {
        let a = line(20, 10.0);
        let mut b = line(20, 10.0);
        b.0 = "race_002".to_string();
        let w = vec![
            SplineWeight { name: "race_001".into(), min_ai_weighting: 1.0, max_ai_weighting: 0.8 },
            SplineWeight { name: "race_002".into(), min_ai_weighting: 0.2, max_ai_weighting: 1.0 },
        ];
        let t = RaceTrack::from_parts(&[a, b], &[], &w, None);
        let mut rng = Rng::new(7);
        let (mut c1, mut c2) = (0, 0);
        for _ in 0..4000 {
            match t.pick_ai_spline(0.0, &mut rng) {
                0 => c1 += 1,
                1 => c2 += 1,
                _ => {}
            }
        }
        // skill 0 -> weights 1.0 : 0.2 -> ~83 % on the first line
        let f = c1 as f32 / (c1 + c2) as f32;
        assert!((f - 0.833).abs() < 0.05, "{f}");
    }

    #[test]
    fn race_spline_count_filter_and_zero_weights() {
        let a = line(20, 10.0);
        let mut b = line(21, 10.0);
        b.0 = "race_002".into();
        let mut c = line(20, 10.0);
        c.0 = "race_003".into();
        let w = vec![SplineWeight { name: "race_003".into(), min_ai_weighting: 0.0, max_ai_weighting: 0.0 }];
        let t = RaceTrack::from_parts(&[a, b, c], &[], &w, None);
        let names: Vec<&str> = t.splines.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["race_001", "race_003"], "race_002 has a different node count and is deleted");
        assert_eq!(t.splines[1].kind, SplineKind::Disabled);
        assert_eq!(t.race_lines(), vec![0]);
    }

    #[test]
    fn results_stars_and_rewards_flow() {
        let Some(a) = assets() else { return };
        let Ok(cfg) = RulesConfig::load(&a) else { return };
        let ev_path = a.join("xml_gameplay/eventdef_episode00/eventdef_episode00_event04_stage00.xml");
        let ev = EventDef::parse(&std::fs::read_to_string(ev_path).unwrap()).unwrap();
        assert_eq!(ev.kart_count, Some(3));
        let mut st = RaceState::from_event(ev, track(50, false), cfg, 3, 2);
        assert!(st.bind_campaign_event("eventdef_episode00_event04_stage00"));
        assert_eq!(st.race_cc, 55);
        assert_eq!(st.event_index, Some(49));
        st.begin_grid();
        st.go();
        // karts 1 and 2 already left the race (not running: they hold the two leading ranks), the player crosses the line 3rd
        let mut k = vec![obs(0, 400.0), obs(1, 450.0), obs(2, 430.0)];
        k[1].running = false;
        k[2].running = false;
        st.update(0.016, &k);
        k[0].pos.z = 487.0;
        let ev = st.update(0.016, &k);
        assert!(ev.iter().any(|e| matches!(e, RaceEvent::GameOver)));
        assert_eq!(st.phase, Phase::Finishing);
        let r = st.results().unwrap().player.clone().unwrap();
        assert_eq!(r.position, 3);
        assert!(r.success); // position < 4
        // 3rd of 3: (9-3)/8 * 8250 = 6187 -> rounded up to 6200, plus the deaths counter 8250/1.5 = 5500 (0 deaths): > Star3 (8300)
        assert_eq!(r.score.finishing, 6187);
        assert_eq!(r.score.bonus_rounded, 6200);
        assert_eq!(r.score.deaths, 5500);
        assert_eq!(r.stars, 3);
        // with 3 deaths the deaths counter is 5500 * 0.5^3 = 687 -> 6887: > Star2 (6200), < Star3 -> 2 stars
        st.score_inputs.deaths = 3;
        st.finalize();
        assert_eq!(st.results().unwrap().player.as_ref().unwrap().stars, 2);
        // rewards: first completion with 2 stars pays the OneStar + TwoStar rewards of C01
        let r = st.results().unwrap().player.clone().unwrap();
        assert!(r.rewards.iter().any(|x| x.tier == 1 && x.sub_type == "Coins" && x.quantity == 200));
        assert!(r.rewards.iter().any(|x| x.tier == 2 && x.quantity == 300));
        assert!(!r.rewards.iter().any(|x| x.tier == 3));
        st.show_results();
        assert_eq!(st.phase, Phase::Results);
    }

    // ---------------- real data (skipped when the assets are not present) ----------------

    fn assets() -> Option<std::path::PathBuf> {
        if let Some(v) = std::env::var_os("ABG_ASSETS292") {
            return Some(std::path::PathBuf::from(v));
        }
        let m = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets292");
        if m.is_dir() {
            return Some(m);
        }
        let mut p = std::env::current_dir().ok()?;
        for _ in 0..4 {
            if p.join("assets292").is_dir() {
                return Some(p.join("assets292"));
            }
            if !p.pop() {
                break;
            }
        }
        None
    }

    #[test]
    fn real_eventdefs_all_parse() {
        let Some(a) = assets() else { return };
        let mut n = 0;
        let mut bad = 0;
        let mut modes: HashMap<GameMode, usize> = HashMap::new();
        for dir in std::fs::read_dir(a.join("xml_gameplay")).unwrap().flatten() {
            let name = dir.file_name().to_string_lossy().to_string();
            if !name.starts_with("eventdef_") {
                continue;
            }
            for f in std::fs::read_dir(dir.path()).unwrap().flatten() {
                let txt = std::fs::read_to_string(f.path()).unwrap();
                let ev = EventDef::parse(&txt).unwrap_or_else(|e| panic!("{:?}: {e}", f.path()));
                if ev.mode == GameMode::Unknown {
                    // 4 shipped files (episode00 event07 stage00/02, cobaltplateau_2, telepods_1) came out of the XOX2 decoder with
                    // their attribute names / values permuted (see PORT_STATUS): they have no <GameMode name=..>
                    assert!(!txt.contains("<GameMode name="), "{:?} {}", f.path(), ev.mode_name);
                    bad += 1;
                    continue;
                }
                *modes.entry(ev.mode).or_default() += 1;
                n += 1;
            }
        }
        assert!(n >= 600, "{n}");
        assert!(bad <= 4, "{bad} eventdefs without a game mode");
        assert!(modes[&GameMode::Race] > 100 && modes[&GameMode::TimeAttack] > 100 && modes[&GameMode::SeedRush] > 100);
    }

    #[test]
    fn real_reward_lookup() {
        let Some(a) = assets() else { return };
        let Ok(cfg) = RulesConfig::load(&a) else { return };
        // eventdef_episode00_event04_stage00 -> Seedway Race tier 1 event 4 stage 0 = Event index 49 = CampaignEvent C01 (cc 55)
        let idx = cfg.events.event_index_for_file("eventdef_episode00_event04_stage00.xml").unwrap();
        assert_eq!(idx, 49);
        let ce = cfg.events.campaign_event(idx).unwrap();
        assert_eq!((ce.tag.as_str(), ce.campaign_cc, ce.energy_cost), ("C01", 55, 1));
        let r1 = cfg.events.rewards_for(idx, 0, 1);
        assert!(r1.iter().any(|r| r.kind == "Currency" && r.sub_type == "Coins" && r.quantity == 200));
        assert!(r1.iter().any(|r| r.sub_type == "XP" && r.quantity == 30));
        let r23 = cfg.events.rewards_for(idx, 1, 3);
        assert!(r23.iter().any(|r| r.tier == 2 && r.quantity == 300));
        assert!(r23.iter().any(|r| r.tier == 3 && r.quantity == 500));
        assert!(cfg.events.rewards_for(idx, 3, 3).is_empty());
        // economy
        assert_eq!(cfg.economy.relative_cc, [4, 0, -2, -4, -8]);
        assert_eq!(cfg.economy.skill_base, [0.3, 0.4, 0.5, 0.55, 0.6]);
        assert_eq!(cfg.economy.ai_skill_race, (0.0, 0.6));
        assert_eq!(cfg.economy.boss_catchup_difficulty, 2);
        assert_eq!(cfg.economy.ai_catchup_difficulty, 3);
        assert_eq!(cfg.economy.spline_switch_cooldown, (3.0, 10.0));
        // C05 has explicit aiSkillMax 0.13
        let c05 = cfg.events.campaign.iter().find(|c| c.tag == "C05").unwrap();
        assert_eq!(c05.ai_skill_max, Some(0.13));
        // scoreconfig
        assert_eq!(cfg.score.top_speed_xyz, [150.0, 0.1, 50.0]);
        assert_eq!(cfg.score.accel_theme[1], 0.3);
        assert_eq!(cfg.score.time_pct_over_for_min, 0.35);
        // star check on the real event: 1st of 3 with scale 55*150 = 8250 + bonus rounding
        let ev = EventDef::parse(&std::fs::read_to_string(a.join("xml_gameplay/eventdef_episode00/eventdef_episode00_event04_stage00.xml")).unwrap())
            .unwrap();
        assert_eq!(ev.stars, [4100, 6200, 8300]);
        assert_eq!(cfg.score.position_score(cfg.economy.race_max_score(55), 1), 8250);
    }

    #[test]
    fn real_tracks_race_lines() {
        let Some(a) = assets() else { return };
        let mut seen = 0;
        for theme in std::fs::read_dir(a.join("tracks")).unwrap().flatten() {
            for run in std::fs::read_dir(theme.path()).unwrap().flatten() {
                let stm = run.path().join("track.stm");
                let Ok(data) = std::fs::read(&stm) else { continue };
                let Ok(pvs) = abgtool::stm::parse_pvs_only(&data) else { continue };
                let xml_path = a
                    .join("xml_tracks")
                    .join(theme.file_name())
                    .join(run.file_name())
                    .join("track.xml");
                let xml = std::fs::read_to_string(&xml_path).unwrap_or_default();
                let t = RaceTrack::from_stm(&pvs, &xml, None);
                let lines = t.race_lines();
                assert!(!lines.is_empty(), "{:?}", stm);
                // all surviving race lines share the node count (SetupEnvironmentSplines deletes the others)
                let c = t.splines[lines[0]].count();
                assert!(lines.iter().all(|&i| t.splines[i].count() == c));
                let s = &t.splines[lines[0]];
                eprintln!("{:?}: {} race lines, {} nodes, length {:.1}, loop {}, start {:.1} finish {:.1} trackend {:.1} helpers s/f/t/m {:?}/{:?}/{:?}/{:?}", stm.strip_prefix(&a).unwrap_or(&stm), lines.len(), c, s.length, s.is_loop, s.start_dist, s.finish_dist, s.trackend_dist, t.start_helper.is_some(), t.finish_helper.is_some(), t.trackend_helper.is_some(), t.smash_helper.is_some());
                assert!(s.length > 100.0, "{:?} length {}", stm, s.length);
                assert!(s.finish_dist <= s.length + 0.01 && s.finish_dist > 0.0);
                seen += 1;
            }
        }
        assert!(seen >= 1);
    }
}
