//! VERSUS: a two-kart head-to-head race (`<GameMode name="VERSUS" kartcount="2"/>`).
//!
//! Ported from `CGameModeVersus` (ctor `@ 00186f14`, `InitialiseMode @ 00186f70`, `GetEventCompleted @ 00186de8`,
//! `GetAICharacter @ 00186f88`) and `CGameModeVersusData` (ctor `@ 00186ec8`, an empty subclass of `CGameModeRaceData`);
//! the base behaviour is `CGameModeRace` (ctor `@ 001852b0`, `InitialiseCarData @ 001851bc`,
//! `CGameModeRaceData::Update @ 00185174`, `CheckGameOverCondition @ 00184f08`) and `CGameMode::ProcessUpdate @ 0017caac`.
//!
//! What the original does:
//! * Mode id 5. It is a RACE (derived from `CGameModeRace`): the car finishes when it crosses the finish line. The only
//!   rule changes are `GetEventCompleted` (WIN = `data+0x10 == 1`, i.e. the car's race position is 1; a plain race accepts
//!   position `< 4`, `CGameModeRace::GetEventCompleted @ 00185094`) and the AI character choice.
//! * Score: bonus counter = `CScoreCounterFinishingPosition` (type 2) = `(9 - finalPosition) / 8 * raceMaxScore`
//!   (the final position `car+0x1af8` is the position at the moment of crossing the line), rounded up to 100.
//! * Star thresholds: the 14 `eventdef_versus_NN_NN.xml` files use `<Stars Star1= Star2= Star3=>` (60000..160000);
//!   the 3 `eventdef_versusNN.xml` files use `<Stars 1= 2= 3=>` which the original never reads, so their thresholds are
//!   all 0 and, with the results-screen comparison `Star <= total`, ANY finish gives 3 stars. Ported as is.
//! * Rank points: none in this mode. (`mprank_config.xml` `SBMRank_Config` / `TBMRank_Config` belong to the multiplayer
//!   `CGameModeLocalMultiplayerRace`, mode ids 8/9/14.)
#![allow(dead_code)]

use super::eventdef::*;
use super::types::*;

/// `CGameMode::GetAICharacter(int)` base is `GetRandomNonDuplicateCharacter(2, param == 0)`; Versus overrides `param == 0`:
/// it keeps drawing `GetRandomNonDuplicateCharacter(2, 0)` until the character's name is neither "MinionPig" nor the
/// character name of the episode's first non-boss stage (`stage+100`, scanned with `mode - 0xb > 2`, i.e. not BOSS_BATTLE
/// (0xb) / BOSS_FRUIT_RUSH (0xc) / SLALOM (0xd)). This is the acceptance predicate of that loop.
pub fn versus_ai_character_ok(candidate_name: &str, first_non_boss_stage_character: &str) -> bool {
    candidate_name != "MinionPig" && candidate_name != first_non_boss_stage_character
}

/// `CGameModeVersus::GetEventCompleted @ 00186de8`: `data+0x10 == 1`.
pub fn versus_event_completed(data: &CarModeData) -> bool {
    data.position == 1
}

#[derive(Clone, Debug)]
pub struct VersusMode {
    pub header: EventHeader,
    pub params: ModeParams,
    /// per-car `CGameModeVersusData` (= `CGameModeRaceData` = `CGameModeData`)
    pub cars: Vec<CarModeData>,
    /// `CGameMode+0x1c`
    pub mode_elapsed: f32,
    is_player: Vec<bool>,
    local_car: Option<CarId>,
    totals: ScoreTotals,
    position_counter_set: bool,
    mode_state: ModeState,
    stars: u8,
    /// final race position (`car+0x1af8`) of the local player once finished
    pub final_position: Option<i32>,
}

impl VersusMode {
    /// `CGameModeVersus::CGameModeVersus @ 00186f14` (mode id 5) + `CGameModeRace::InitialiseCarData @ 001851bc`.
    /// `kart_count` should be the eventdef `kartcount` (2).
    pub fn new(header: &EventHeader, mut params: ModeParams) -> VersusMode {
        if header.kartcount > 0 {
            params.kart_count = header.kartcount as usize;
        }
        let n = params.kart_count.max(1);
        let mut is_player = vec![false; n];
        is_player[0] = true;
        VersusMode {
            header: header.clone(),
            cars: (0..n).map(|_| CarModeData::default()).collect(),
            mode_elapsed: 0.0,
            is_player,
            local_car: Some(0),
            totals: ScoreTotals { base_score: params.base_score, bonus_raw: 0 },
            position_counter_set: false,
            mode_state: ModeState::Running,
            stars: 0,
            final_position: None,
            params,
        }
    }

    pub fn from_xml(xml: &str, params: ModeParams) -> Option<VersusMode> {
        let h = EventHeader::from_xml(xml)?;
        if h.mode_id != mode_id::VERSUS {
            return None;
        }
        Some(VersusMode::new(&h, params))
    }

    pub fn load(rel: &str, params: ModeParams) -> Option<VersusMode> {
        let s = std::fs::read_to_string(format!("{}/xml/gameplay/{}", ASSETS292, rel)).ok()?;
        VersusMode::from_xml(&s, params)
    }

    pub fn set_player(&mut self, car: CarId, is_player: bool) {
        self.grow(car + 1);
        self.is_player[car] = is_player;
        if is_player && self.local_car.is_none() {
            self.local_car = Some(car);
        }
    }

    fn grow(&mut self, n: usize) {
        while self.cars.len() < n {
            self.cars.push(CarModeData::default());
            self.is_player.push(false);
        }
    }

    /// `GetGameModeName` case 5 / `GetGameModeDesc` case 5 localisation keys.
    pub fn name_key() -> &'static str {
        "MODE_VERSUS"
    }
    pub fn desc_key() -> &'static str {
        "MODE_VERSUS_DESC"
    }

    fn finish(&mut self, car: CarId, events: &mut Vec<ModeEvent>) {
        if car >= self.cars.len() || self.cars[car].state != CarModeState::Running {
            return;
        }
        let is_player = self.is_player[car];
        let won = versus_event_completed(&self.cars[car]);
        let pos = self.cars[car].position;
        let star_completion = apply_finish(&mut self.cars[car], won, is_player, self.params.event_index, pos, 0);
        if star_completion {
            events.push(ModeEvent::Other { tag: "AddCurrentEventStarCompletion", value: 1.0 });
        }
        if is_player && self.local_car == Some(car) {
            for c in self.cars.iter_mut() {
                if c.final_position == 0 {
                    c.final_position = c.position;
                }
            }
            self.final_position = Some(pos);
            // CCar::SetFinishLineCrossed @ 001aa158: position counter uses car+0x1af8 (mode is not 6/7)
            if !self.position_counter_set {
                self.position_counter_set = true;
                self.totals.bonus_raw = finishing_position_score(pos, self.params.event_cc);
            }
            self.totals.base_score = self.params.base_score;
            let total = self.totals.total();
            self.stars = stars_from_total(total, &self.header.stars);
            self.mode_state = if won { ModeState::Won } else { ModeState::Lost };
            events.push(ModeEvent::ScoreChanged { car, score: total as i64 });
            events.push(ModeEvent::Finished { won, score: total as i64, stars: self.stars });
        }
    }
}

impl GameModeRules for VersusMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::Versus
    }

    /// `CGameModeRace::Update @ 001851b8` -> `CGameMode::ProcessUpdate`: mode clock + `CGameModeRaceData::Update` per car
    /// (current / best position, elapsed time while the race clock runs).
    fn update(&mut self, dt: f32, karts: &[KartState], _effects: &mut Vec<CarEffect>, _events: &mut Vec<ModeEvent>) {
        self.mode_elapsed += dt;
        if let Some(m) = karts.iter().map(|k| k.id).max() {
            self.grow(m + 1);
        }
        for k in karts {
            self.is_player[k.id] = k.is_player;
            if k.is_player && self.local_car.is_none() {
                self.local_car = Some(k.id);
            }
            self.cars[k.id].update_base(k.race_position as i32, k.race_time > 0.0, dt);
        }
    }

    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        if let ModeInput::KartFinished { car, .. } = input {
            self.finish(*car, events);
        }
    }

    fn state(&self) -> ModeState {
        self.mode_state
    }

    fn score(&self) -> i64 {
        self.totals.total() as i64
    }

    fn stars(&self) -> u8 {
        self.stars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(cc: i32) -> ModeParams {
        ModeParams {
            kart_count: 2,
            event_cc: cc,
            event_index: 1,
            score_cfg: ScoreConfig::load().unwrap_or_default(),
            ..ModeParams::default()
        }
    }

    fn karts(p0: u32, p1: u32) -> [KartState; 2] {
        [
            KartState { id: 0, is_player: true, race_position: p0, race_time: 1.0, ..KartState::default() },
            KartState { id: 1, is_player: false, race_position: p1, race_time: 1.0, ..KartState::default() },
        ]
    }

    #[test]
    fn real_eventdef_versus_underscore_stars() {
        let rel = "eventdef_other/eventdef_versus_01_01.xml";
        let Some(h) = load_header(rel) else { return };
        assert_eq!(h.kind(), GameModeKind::Versus);
        assert_eq!(h.kartcount, 2);
        assert_eq!(h.stars, [60000, 95000, 105000]);
        assert_eq!(h.level_index, 25);
        let m = VersusMode::load(rel, params(55)).unwrap();
        assert_eq!(m.cars.len(), 2);
    }

    #[test]
    fn real_eventdef_versus00_unread_star_attributes_are_zero() {
        let rel = "eventdef_other/eventdef_versus00.xml";
        let Some(_) = load_header(rel) else { return };
        let mut m = VersusMode::load(rel, params(55)).unwrap();
        assert_eq!(m.header.stars, [0, 0, 0]); // `<Stars 1= 2= 3=>` is not read by the original
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update(0.016, &karts(2, 1), &mut fx, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 50.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Lost); // 2nd place: not position 1
        assert_eq!(m.stars(), 3); // 0 <= total for every threshold -> 3 stars (original behaviour)
    }

    #[test]
    fn win_only_in_first_place_and_position_score() {
        let mut h = EventHeader::default();
        h.kartcount = 2;
        h.stars = [60000, 95000, 105000];
        let mut m = VersusMode::new(&h, params(55));
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update(0.016, &karts(1, 2), &mut fx, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 1, time: 49.0 }, &mut ev); // AI finishes: no player result
        assert_eq!(m.state(), ModeState::Running);
        assert_eq!(m.cars[1].state, CarModeState::Failed); // AI in 2nd place
        m.on_input(&ModeInput::KartFinished { car: 0, time: 50.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Won);
        // 1st place: 8/8 * 8250 = 8250 -> 8300 (no base score supplied)
        assert_eq!(m.score(), 8300);
        assert_eq!(m.stars(), 0); // 8300 < 60000
        assert_eq!(m.final_position, Some(1));
        // 2nd place: 7/8 * 8250 = 7218
        assert_eq!(finishing_position_score(2, 55), 7218);
    }

    #[test]
    fn ai_character_rule() {
        assert!(!versus_ai_character_ok("MinionPig", "red"));
        assert!(!versus_ai_character_ok("red", "red"));
        assert!(versus_ai_character_ok("blue", "red"));
    }
}
