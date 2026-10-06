//! Multiplayer race modes TMR (id 9, `kartcount=6`), QMR (id 8, `kartcount=4`) and LMR (id 0xe, `kartcount=2`),
//! ported from the Angry Birds Go 2.9.1 decompile, plus `mprank_config.xml`.
//!
//! # What the 2.9.1 binary actually contains (honest summary)
//! * `LMR` = **L**ocal **M**ultiplayer **R**ace, a real class: `CGameModeLocalMultiplayerRace` (vtable `0xd91b70`,
//!   data `CGameModeLocalMultiplayerRaceData`). Cars are the connected network players (`CGameModeManager::InitialiseCars`
//!   branch for mode `0xe`: `CNetwork::GetConnectedPlayers` human cars, AI removed). Overridden virtuals:
//!   `InitialiseMode @ 00183620`, `InitialiseCarData @ 00183550`, `Update @ 00183520`,
//!   `CheckGameOverCondition @ 0018341c`, `GetEventCompleted @ 0018334c`. See [`LmrRules`].
//! * `QMR` (id 8, loc key `QUICK_RACE_EVENT_NAME`) and `TMR` (id 9, `TEAM_RACE_EVENT_NAME`) have **no game mode object**:
//!   `CGameMode::CreateGameMode @ 0017d1b4` returns NULL for 8 and 9 (and `CGameModeManager::InitialiseMode` would call
//!   through that NULL). They were the server-hosted online quick / tournament-team races; the only code left for them is
//!   the AI set-up in `CGameModeManager::InitialiseCars` (for ids 8, 9, 0xe the AI skill base is the constant
//!   `DAT_001849fc` instead of the `level` based formula) and the eventdef reader (`kartcount`). Everything that decided
//!   their results (matchmaking, rank/points, leaderboards) was server side and is not in the binary.
//!   // UNRESOLVED: server-only. [`MultiplayerRaceMode`] therefore applies the LMR rule to all three as the only MP result
//!   // rule in the binary.
//! * `mprank_config.xml` (`SPRank_Config`, `TBMRank_Config`, `SBMRank_Config`) is shipped in the 2.9.2 data, but neither
//!   the 2.9.1 nor the 2.9.2 `libABK.so` contains any of those strings (searched case-insensitively for `mprank`,
//!   `Rank_Config`, `TBMRank`, `SBMRank`), i.e. no consumer exists in the available binaries.
//!   // UNRESOLVED: who reads it. The data is parsed ([`MpRankConfig`]); the mapping TBM <-> TMR (6 places) and
//!   // SBM <-> QMR (4 places) is inferred from the number of entries (6 / 4) matching `kartcount`, it is not stated anywhere.
//!
//! # Naming / related data checked
//! * LMR is "Local" multiplayer (not "league"); TMR maps to loc key `TEAM_RACE_EVENT_NAME` (not "tournament"); QMR and LMR both use
//!   `QUICK_RACE_EVENT_NAME`.
//! * Tournaments (`xml/tournament/tournamenttypes.xml`, `tournament.xml`) are single-player score tournaments over
//!   `gameMode="Race"`(10), `"TimeAttack"`(4) and `"FruitRush"`(3) sub types with gem / token prizes per leaderboard rank; they
//!   contain no TMR/QMR/LMR and no place-points table.
//! * Daily races (`dailyraces.xml`, the 42 `eventdef_der_*.xml`) are all `<GameMode name="RACE"/>`.
//! * `meta/data.rs` (other agent) also parses `mprank_config.xml`; keep a single parser (this one is a standalone copy).
//!
//! Standalone: std + glam; eventdefs use a tolerant scanner (the shipped files can contain duplicate attributes),
//! `mprank_config.xml` is clean xml and uses roxmltree.
#![allow(dead_code)]

use super::types::*;

pub const MODE_QMR: u32 = 8;
pub const MODE_TMR: u32 = 9;
pub const MODE_LMR: u32 = 0xe;

/// `CGame+0x3244`-style constant of LMR: none. The LMR network wait timer initial value is the static global at
/// `0xdad6ec` (read by `CGameModeLocalMultiplayerRace::InitialiseMode`): `VectorSignedToFloat(3)` = 3.0 s.
/// UNRESOLVED: the global is a plain `.data` int with initial value 3; whether the network code overwrites it at runtime
/// is not visible in the decompile.
pub const LMR_RACE_TIMES_WAIT_SECONDS: f32 = 3.0;
/// `NetworkMessage_Send(0xf)` sent by the host when everybody finished.
pub const LMR_HOST_DONE_MESSAGE: u32 = 0xf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MpKind {
    Qmr,
    Tmr,
    Lmr,
}

impl MpKind {
    pub fn mode_id(self) -> u32 {
        match self {
            MpKind::Qmr => MODE_QMR,
            MpKind::Tmr => MODE_TMR,
            MpKind::Lmr => MODE_LMR,
        }
    }
    pub fn from_name(s: &str) -> Option<MpKind> {
        match s {
            "QMR" => Some(MpKind::Qmr),
            "TMR" => Some(MpKind::Tmr),
            "LMR" => Some(MpKind::Lmr),
            _ => None,
        }
    }
    /// `CGameMode::GetGameModeName @ 0017d4ac`: 8 and 0xe -> `QUICK_RACE_EVENT_NAME`, 9 -> `TEAM_RACE_EVENT_NAME`.
    pub fn name_loc_key(self) -> &'static str {
        match self {
            MpKind::Qmr | MpKind::Lmr => "QUICK_RACE_EVENT_NAME",
            MpKind::Tmr => "TEAM_RACE_EVENT_NAME",
        }
    }
    /// `kartcount=` of every shipped eventdef of that mode.
    pub fn default_kart_count(self) -> u32 {
        match self {
            MpKind::Qmr => 4,
            MpKind::Tmr => 6,
            MpKind::Lmr => 2,
        }
    }
    /// `CGameModeManager::CreateGameMode` really builds a mode object (only LMR).
    pub fn has_original_game_mode_object(self) -> bool {
        matches!(self, MpKind::Lmr)
    }
}

// ------------------------------------------------------------------------------------------------------------
// Eventdef
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

/// AI weighting of one `<Spline name= min_ai_weighting= max_ai_weighting=/>`.
#[derive(Clone, Debug, PartialEq)]
pub struct SplineWeight {
    pub name: String,
    pub min_ai_weighting: f32,
    pub max_ai_weighting: f32,
}

/// The mode specific header values of a TMR / QMR / LMR eventdef (`ReadEventDefinitionFromXML @ 00101478`).
#[derive(Clone, Debug, PartialEq)]
pub struct MpEventDef {
    pub kind: MpKind,
    /// `<GameMode kartcount=>` (`manager+0x23c`)
    pub kart_count: u32,
    pub environment: String,
    pub difficulty_level: f32,
    /// `level_index=` (`manager+0x288`, default 0)
    pub level_index: i32,
    /// `tier=` (LMR files only)
    pub tier: Option<i32>,
    pub stars: [i32; 3],
    pub splines: Vec<SplineWeight>,
    /// `<DifficultyAdjust><VeryEasy>..<Impossible>` (`manager+0x590..0x5a0`)
    pub difficulty_adjust: [f32; 5],
    pub track_items: usize,
}

impl MpEventDef {
    pub fn from_xml(xml: &str) -> Result<MpEventDef, String> {
        let tags = scan_tags(xml);
        let gm = tags.iter().find(|t| t.name == "GameMode").ok_or("no <GameMode>")?;
        let name = gm.get("name").ok_or("GameMode has no name")?;
        let kind = MpKind::from_name(name).ok_or_else(|| format!("GameMode {name} is not QMR/TMR/LMR"))?;
        // `kartcount` is only written when the attribute exists; the manager default is whatever it held before
        // (UNRESOLVED), the shipped files always carry it. Fall back to the mode's usual count.
        let kart_count = gm.get("kartcount").and_then(|v| v.trim().parse().ok()).unwrap_or(kind.default_kart_count());
        let diff = tags.iter().find(|t| t.name == "Difficulty");
        let mut stars = [0i32; 3];
        if let Some(s) = tags.iter().find(|t| t.name == "Stars") {
            for (i, n) in ["Star1", "Star2", "Star3"].iter().enumerate() {
                if let Some(v) = s.get(n) {
                    stars[i] = v.trim().parse().unwrap_or(0);
                }
            }
            if !(stars[0] == 0 && stars[1] == 0 && stars[2] == 0) {
                if stars[1] < stars[0] {
                    stars[1] = stars[0];
                }
                if stars[2] < stars[1] {
                    stars[2] = stars[1];
                }
            }
        }
        let mut difficulty_adjust = [0.0f32; 5];
        // <DifficultyAdjust><VeryEasy>0.0</VeryEasy>... : plain text children, read by hand
        if let Some(p) = xml.find("<DifficultyAdjust>") {
            let end = xml[p..].find("</DifficultyAdjust>").map(|e| p + e).unwrap_or(xml.len());
            let body = &xml[p..end];
            for (i, n) in ["VeryEasy", "Easy", "Medium", "Hard", "Impossible"].iter().enumerate() {
                let open = format!("<{n}>");
                if let Some(a) = body.find(&open) {
                    let vs = a + open.len();
                    if let Some(b) = body[vs..].find('<') {
                        difficulty_adjust[i] = body[vs..vs + b].trim().parse().unwrap_or(0.0);
                    }
                }
            }
        }
        Ok(MpEventDef {
            kind,
            kart_count,
            environment: tags.iter().find(|t| t.name == "Environment").and_then(|t| t.get("pathname")).unwrap_or("").to_string(),
            difficulty_level: diff.and_then(|d| d.get("level")).and_then(|v| v.parse().ok()).unwrap_or(0.5),
            level_index: diff.and_then(|d| d.get("level_index")).and_then(|v| v.parse().ok()).unwrap_or(0),
            tier: diff.and_then(|d| d.get("tier")).and_then(|v| v.parse().ok()),
            stars,
            splines: tags
                .iter()
                .filter(|t| t.name == "Spline")
                .map(|t| SplineWeight {
                    name: t.get("name").unwrap_or("").to_string(),
                    min_ai_weighting: t.get("min_ai_weighting").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                    max_ai_weighting: t.get("max_ai_weighting").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                })
                .collect(),
            difficulty_adjust,
            track_items: tags.iter().filter(|t| t.name == "TrackItem").count(),
        })
    }
}

// ------------------------------------------------------------------------------------------------------------
// mprank_config.xml
// ------------------------------------------------------------------------------------------------------------

/// `mprank_config.xml`: `SPRank_Config` (rank name -> minimum points), `TBMRank_Config` (6 places) and `SBMRank_Config`
/// (4 places) points awarded per finishing place.
#[derive(Clone, Debug, PartialEq)]
pub struct MpRankConfig {
    /// `(rank name, minimum value)` in file order (ranks "1".."25")
    pub sp_ranks: Vec<(String, u32)>,
    /// points for places 1..=6 (First..Sixth)
    pub tbm_points: Vec<u32>,
    /// points for places 1..=4 (First..Fourth)
    pub sbm_points: Vec<u32>,
}

impl MpRankConfig {
    pub fn from_xml(xml: &str) -> Result<MpRankConfig, String> {
        // the file is a forest (several root elements); wrap it so roxmltree accepts it
        let wrapped = format!("<root>{xml}</root>");
        let doc = roxmltree::Document::parse(&wrapped).map_err(|e| e.to_string())?;
        let section = |name: &str| doc.descendants().find(|n| n.has_tag_name(name));
        let sp_ranks = section("SPRank_Config")
            .map(|s| {
                s.children()
                    .filter(|c| c.has_tag_name("Rank"))
                    .map(|c| (c.attribute("name").unwrap_or("").to_string(), c.attribute("value").and_then(|v| v.parse().ok()).unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default();
        let places = |sec: &str, names: &[&str]| -> Vec<u32> {
            section(sec)
                .map(|s| {
                    names
                        .iter()
                        .filter_map(|n| s.children().find(|c| c.has_tag_name(*n)))
                        .map(|c| c.attribute("value").and_then(|v| v.parse().ok()).unwrap_or(0))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(MpRankConfig {
            sp_ranks,
            tbm_points: places("TBMRank_Config", &["First", "Second", "Third", "Fourth", "Fifth", "Sixth"]),
            sbm_points: places("SBMRank_Config", &["First", "Second", "Third", "Fourth"]),
        })
    }

    /// Highest rank whose minimum value is `<= points` (rank names are numbers as strings).
    pub fn sp_rank_for_points(&self, points: u32) -> Option<&str> {
        self.sp_ranks.iter().filter(|(_, v)| *v <= points).last().map(|(n, _)| n.as_str())
    }

    /// Points for finishing `place` (1-based) in a race of `kind`. TMR uses the 6 place table, QMR the 4 place table
    /// (mapping inferred, see the module docs); LMR has none.
    pub fn points_for_place(&self, kind: MpKind, place: usize) -> Option<u32> {
        let t = match kind {
            MpKind::Tmr => &self.tbm_points,
            MpKind::Qmr => &self.sbm_points,
            MpKind::Lmr => return None,
        };
        place.checked_sub(1).and_then(|i| t.get(i)).copied()
    }
}

// ------------------------------------------------------------------------------------------------------------
// LMR rules (CGameModeLocalMultiplayerRace)
// ------------------------------------------------------------------------------------------------------------

/// Inputs of `CGameModeLocalMultiplayerRace::CheckGameOverCondition @ 0018341c`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LmrNet {
    /// `CNetwork::GetMPGameState != 0` (a network race is in progress; 0 -> the check returns "over" immediately)
    pub mp_game_active: bool,
    /// `CGame::GetLocalNetworkAllPlayersFinished`
    pub all_players_finished: bool,
    /// `CNetwork::IsHost`
    pub is_host: bool,
    /// `CNetwork::CheckPlayerConnectionState(net, 0)` (0 = the client still waits for the host's race times)
    pub connection_state: i32,
    /// `CGame::GetLocalNetworkRaceTimesReceived`
    pub race_times_received: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LmrGameOver {
    pub over: bool,
    /// host only: `NetworkMessage_Send(0xf)`
    pub send_message: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct LmrRules {
    /// `this+0x24`: set by a client once everybody finished (starts the wait timer)
    pub waiting_for_times: bool,
    /// `this+0x28`: countdown of the wait (starts at [`LMR_RACE_TIMES_WAIT_SECONDS`])
    pub wait_timer: f32,
}

impl Default for LmrRules {
    fn default() -> Self {
        LmrRules { waiting_for_times: false, wait_timer: LMR_RACE_TIMES_WAIT_SECONDS }
    }
}

impl LmrRules {
    /// `Update @ 00183520`: base update, then `if (this+0x24 != 0) this+0x28 -= dt`.
    pub fn tick(&mut self, dt: f32) {
        if self.waiting_for_times {
            self.wait_timer -= dt;
        }
    }

    /// `CheckGameOverCondition @ 0018341c`.
    /// * no network race -> over;
    /// * not everybody finished -> not over;
    /// * host -> sends message 0xf, over;
    /// * client -> starts the wait timer; over as soon as the host's race times arrived, or the connection state is
    ///   non-zero (lost), or the 3 s timer ran out.
    pub fn check_game_over(&mut self, net: &LmrNet) -> LmrGameOver {
        if !net.mp_game_active {
            return LmrGameOver { over: true, send_message: None };
        }
        if !net.all_players_finished {
            return LmrGameOver { over: false, send_message: None };
        }
        if net.is_host {
            return LmrGameOver { over: true, send_message: Some(LMR_HOST_DONE_MESSAGE) };
        }
        self.waiting_for_times = true;
        let mut waiting = if net.connection_state == 0 { !net.race_times_received } else { false };
        if self.wait_timer <= 0.0 {
            waiting = false;
        }
        LmrGameOver { over: !waiting, send_message: None }
    }

    /// `GetEventCompleted @ 0018334c`: finishing position (`data+0x10`) `< 3`.
    pub fn event_completed(data_position: i32) -> bool {
        data_position < 3
    }
}

// ------------------------------------------------------------------------------------------------------------
// Mode wrapper
// ------------------------------------------------------------------------------------------------------------

/// Offline / single-device handling of the three modes: normal race rules (`CGameMode::ProcessUpdate` finish handling)
/// with the LMR result rule (`position < 3` completes the event) and, for TMR/QMR, the placement points of
/// [`MpRankConfig`].
#[derive(Clone, Debug)]
pub struct MultiplayerRaceMode {
    pub def: MpEventDef,
    pub rank: Option<MpRankConfig>,
    pub lmr: LmrRules,
    pub data_state: u8,
    /// `data+0x10` current position, `data+0x14` best position
    pub data_position: i32,
    pub data_best_position: i32,
    /// cars in finishing order
    pub finish_order: Vec<CarId>,
    pub clock: f32,
    pub score_total: i64,
    pub local_car: CarId,
}

pub mod data_state {
    pub const RUNNING: u8 = 1;
    pub const COMPLETED: u8 = 2;
    pub const FAILED: u8 = 3;
}

impl MultiplayerRaceMode {
    pub fn new(def: MpEventDef, rank: Option<MpRankConfig>, local_car: CarId) -> MultiplayerRaceMode {
        MultiplayerRaceMode {
            def,
            rank,
            lmr: LmrRules::default(),
            data_state: data_state::RUNNING,
            data_position: 0,
            data_best_position: 1000,
            finish_order: Vec::new(),
            clock: 0.0,
            score_total: 0,
            local_car,
        }
    }

    pub fn from_eventdef(xml: &str, rank: Option<MpRankConfig>, local_car: CarId) -> Result<MultiplayerRaceMode, String> {
        Ok(MultiplayerRaceMode::new(MpEventDef::from_xml(xml)?, rank, local_car))
    }

    /// Finishing place (1-based) of the local car once it finished.
    pub fn local_place(&self) -> Option<usize> {
        self.finish_order.iter().position(|&c| c == self.local_car).map(|i| i + 1)
    }

    /// Points of the finished local car (TMR / QMR only).
    pub fn points_awarded(&self) -> Option<u32> {
        let p = self.local_place()?;
        self.rank.as_ref()?.points_for_place(self.def.kind, p)
    }
}

impl GameModeRules for MultiplayerRaceMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::Other
    }

    fn update(&mut self, dt: f32, karts: &[KartState], _effects: &mut Vec<CarEffect>, _events: &mut Vec<ModeEvent>) {
        self.clock += dt;
        self.lmr.tick(dt);
        if let Some(k) = karts.iter().find(|k| k.id == self.local_car) {
            self.data_position = k.race_position as i32;
            if self.data_position <= self.data_best_position {
                self.data_best_position = self.data_position;
            }
        }
    }

    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        match input {
            ModeInput::KartFinished { car, .. } => {
                if !self.finish_order.contains(car) {
                    self.finish_order.push(*car);
                }
                if *car == self.local_car && self.data_state == data_state::RUNNING {
                    // `ProcessUpdate`: state = GetEventCompleted ? 2 : 3, using the position stored in the car data
                    let place = self.local_place().unwrap_or(0) as i32;
                    self.data_position = place;
                    self.data_state = if LmrRules::event_completed(place) { data_state::COMPLETED } else { data_state::FAILED };
                    events.push(ModeEvent::Finished { won: self.data_state == data_state::COMPLETED, score: self.score_total, stars: self.stars() });
                }
            }
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

    /// `GetStarsFromScore`: 3 / 2 stars above `Star3` / `Star2`, otherwise 1 once finished (0 while running / lost).
    fn stars(&self) -> u8 {
        if self.state() != ModeState::Won {
            return 0;
        }
        let s = self.def.stars;
        if self.score_total > s[2] as i64 {
            3
        } else if self.score_total > s[1] as i64 {
            2
        } else {
            1
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

    const DIR: &str = "xml/gameplay/eventdef_other";

    fn load(rel: &str) -> Option<String> {
        let p = Path::new(ASSETS292).join(rel);
        if !p.exists() {
            eprintln!("skip: {} missing", p.display());
            return None;
        }
        std::fs::read_to_string(p).ok()
    }

    #[test]
    fn parse_all_real_mp_eventdefs() {
        let dir = Path::new(ASSETS292).join(DIR);
        if !dir.exists() {
            return;
        }
        let mut n = [0usize; 3];
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let kind = if name.starts_with("eventdef_tmr_") {
                MpKind::Tmr
            } else if name.starts_with("eventdef_qmr_") {
                MpKind::Qmr
            } else if name.starts_with("eventdef_lmr_") {
                MpKind::Lmr
            } else {
                continue;
            };
            let x = std::fs::read_to_string(&p).unwrap();
            let d = MpEventDef::from_xml(&x).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(d.kind, kind, "{name}");
            assert_eq!(d.kart_count, kind.default_kart_count(), "{name}");
            assert!(d.environment.starts_with("environments\\"), "{name}");
            assert!(d.stars[0] > 0 && d.stars[0] <= d.stars[1] && d.stars[1] <= d.stars[2], "{name}");
            n[kind as usize] += 1;
        }
        // 14 files of each mode
        assert_eq!(n, [14, 14, 14]);
    }

    #[test]
    fn specific_values_of_first_files() {
        let Some(x) = load(&format!("{DIR}/eventdef_tmr_00_00.xml")) else { return };
        let d = MpEventDef::from_xml(&x).unwrap();
        assert_eq!((d.kind, d.kart_count, d.level_index), (MpKind::Tmr, 6, 13));
        assert_eq!(d.stars, [60000, 85000, 95000]);
        assert_eq!(d.splines.len(), 2);
        assert_eq!(d.splines[1].max_ai_weighting, 0.8);
        assert_eq!(d.difficulty_adjust, [0.0; 5]);
        let x = load(&format!("{DIR}/eventdef_qmr_00_00.xml")).unwrap();
        let d = MpEventDef::from_xml(&x).unwrap();
        assert_eq!((d.kind, d.kart_count, d.level_index), (MpKind::Qmr, 4, 1));
        assert_eq!(d.difficulty_adjust, [0.0, 0.0, 0.05, 0.1, 0.3]);
        let x = load(&format!("{DIR}/eventdef_lmr_00_00.xml")).unwrap();
        let d = MpEventDef::from_xml(&x).unwrap();
        assert_eq!((d.kind, d.kart_count, d.tier), (MpKind::Lmr, 2, Some(0)));
        assert_eq!(d.stars, [60000, 90000, 100000]);
        assert_eq!(d.difficulty_adjust, [0.0, 0.05, 0.1, 0.15, 0.3]);
    }

    #[test]
    fn parse_mprank_config() {
        let Some(x) = load("xml/gameplay/misc/mprank_config.xml") else { return };
        let c = MpRankConfig::from_xml(&x).unwrap();
        assert_eq!(c.sp_ranks.len(), 25);
        assert_eq!(c.sp_ranks[0], ("1".to_string(), 0));
        assert_eq!(c.sp_ranks[24], ("25".to_string(), 10000));
        assert_eq!(c.tbm_points, vec![8, 6, 4, 2, 1, 0]);
        assert_eq!(c.sbm_points, vec![10, 6, 4, 2]);
        assert_eq!(c.sp_rank_for_points(0), Some("1"));
        assert_eq!(c.sp_rank_for_points(47), Some("2"));
        assert_eq!(c.sp_rank_for_points(48), Some("3"));
        assert_eq!(c.sp_rank_for_points(99999), Some("25"));
        assert_eq!(c.points_for_place(MpKind::Tmr, 1), Some(8));
        assert_eq!(c.points_for_place(MpKind::Tmr, 6), Some(0));
        assert_eq!(c.points_for_place(MpKind::Qmr, 1), Some(10));
        assert_eq!(c.points_for_place(MpKind::Qmr, 5), None);
        assert_eq!(c.points_for_place(MpKind::Lmr, 1), None);
        assert_eq!(c.points_for_place(MpKind::Tmr, 0), None);
    }

    #[test]
    fn lmr_game_over_logic() {
        let mut r = LmrRules::default();
        let mut net = LmrNet { mp_game_active: false, all_players_finished: false, is_host: false, connection_state: 0, race_times_received: false };
        assert!(r.check_game_over(&net).over); // no network race
        net.mp_game_active = true;
        assert!(!r.check_game_over(&net).over); // somebody still racing
        net.all_players_finished = true;
        net.is_host = true;
        assert_eq!(r.check_game_over(&net), LmrGameOver { over: true, send_message: Some(0xf) });
        // client: waits for the race times
        net.is_host = false;
        let g = r.check_game_over(&net);
        assert!(!g.over && r.waiting_for_times);
        net.race_times_received = true;
        assert!(r.check_game_over(&net).over);
        // times never arrive: the 3 s timer ends the wait
        let mut r = LmrRules::default();
        net.race_times_received = false;
        assert!(!r.check_game_over(&net).over);
        for _ in 0..31 {
            r.tick(0.1);
        }
        assert!(r.check_game_over(&net).over);
        // connection lost -> do not wait
        let mut r = LmrRules::default();
        net.connection_state = 1;
        assert!(r.check_game_over(&net).over);
    }

    #[test]
    fn lmr_event_completed_is_top_two() {
        assert!(LmrRules::event_completed(1));
        assert!(LmrRules::event_completed(2));
        assert!(!LmrRules::event_completed(3));
    }

    #[test]
    fn offline_race_places_and_points() {
        let Some(x) = load(&format!("{DIR}/eventdef_tmr_00_00.xml")) else { return };
        let rank = load("xml/gameplay/misc/mprank_config.xml").map(|r| MpRankConfig::from_xml(&r).unwrap());
        let mut m = MultiplayerRaceMode::from_eventdef(&x, rank.clone(), 0).unwrap();
        let mut ev = Vec::new();
        m.on_input(&ModeInput::KartFinished { car: 3, time: 50.0 }, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 5, time: 51.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Running);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 52.0 }, &mut ev);
        assert_eq!(m.local_place(), Some(3));
        assert_eq!(m.state(), ModeState::Lost);
        if rank.is_some() {
            assert_eq!(m.points_awarded(), Some(4));
        }
        // second place still completes (position < 3)
        let mut m2 = MultiplayerRaceMode::from_eventdef(&x, rank, 0).unwrap();
        m2.on_input(&ModeInput::KartFinished { car: 3, time: 50.0 }, &mut ev);
        m2.on_input(&ModeInput::KartFinished { car: 0, time: 51.0 }, &mut ev);
        assert_eq!(m2.state(), ModeState::Won);
        assert_eq!(m2.stars(), 1);
        m2.score_total = 90000;
        assert_eq!(m2.stars(), 2);
    }

    #[test]
    fn rejects_non_mp_modes() {
        assert!(MpEventDef::from_xml("<EventDefinition><GameMode name=\"RACE\"/></EventDefinition>").is_err());
    }

    #[test]
    fn dead_modes_flagged() {
        assert!(!MpKind::Qmr.has_original_game_mode_object());
        assert!(!MpKind::Tmr.has_original_game_mode_object());
        assert!(MpKind::Lmr.has_original_game_mode_object());
        assert_eq!(MpKind::Tmr.name_loc_key(), "TEAM_RACE_EVENT_NAME");
        assert_eq!(MpKind::Lmr.name_loc_key(), "QUICK_RACE_EVENT_NAME");
    }
}
