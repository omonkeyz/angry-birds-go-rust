//! TIME_ATTACK: race the track against a countdown that depends on the difficulty tier.
//!
//! Ported from `CGameModeTimeAttack` (ctor `@ 00186d98`, `InitialiseMode @ 00186d74`, `InitialiseCarData @ 00186c54`,
//! `Update @ 00186c50`, `GetEventCompleted @ 0018698c`, `GetBonusCoins @ 00186968`, `GetTimeLeft @ 00186a74`,
//! `CheckGameOverCondition @ 001867dc`) and `CGameModeTimeAttackData` (ctor `@ 00186bc4`, `Reset @ 00186b64`,
//! `Update @ 00186ab8`).
//!
//! What the original does:
//! * Mode id 6. Still a RACE: the car finishes when it crosses the finish line (`CGameMode::ProcessUpdate @ 0017caac`; the
//!   finish check virtual is not overridden). There is NO ghost, no recorded run and no medal/target logic in 2.9.1
//!   (no "ghost" symbol exists in the decompile apart from the ability-AI helper `CBaseAbility::ShouldGhostUseAI`).
//!   `tracktimes.xml` is loaded into per-stage records (`+0xc`) by the eventdef manager but is not read by this mode;
//!   it is NOT the star criterion (see [`super::eventdef::load_track_times`]).
//! * Countdown: `timeLeft` starts at `GetTimerDuration(tier)` (eventdef `timer_<tier>`), decreases by `dt` while the state
//!   is running and `car+0x1b7c > 0` (the race clock has started), is never clamped and keeps going below 0; the race
//!   continues and the car can still finish.
//! * Low-time warning: the first frame in which `timeLeft < 5.0` (checked before the decrement) fires UI sound event 0x1f;
//!   while the state is not running UI sound event 0x20 (timer loop stop) fires every frame.
//! * WIN = `0.0 <= timeLeft` at the finish line (state 2) else state 3. Bonus coins = `max(0, (int)timeLeft) * 10`.
//! * Score: bonus counter = `CScoreCounterFinishingTime` (type 3), HIGHER IS BETTER. Stars come from the points, not from
//!   the raw time (`Star1..3` = 9400/14100/18800 style point thresholds). (types.rs says time modes return milliseconds;
//!   that does not hold for 2.9.1: [`TimeAttackMode::score`] returns points, [`TimeAttackMode::time_left`] the seconds.)
#![allow(dead_code)]

use super::eventdef::*;
use super::types::*;

/// `CGameModeTimeAttackData::Update @ 00186ab8`: the low-time threshold (`5.0`, the literal compared with `timeLeft`).
pub const LOW_TIME_WARNING_SECONDS: f32 = 5.0;
/// `ABKSound::CUIController::OnEvent(0x1f)` (low-time warning) and `(0x20)` (timer stop).
pub const UI_EVENT_LOW_TIME: u32 = 0x1f;
pub const UI_EVENT_TIMER_STOP: u32 = 0x20;
/// `*(CGame+0x3244) = 5.0f` written by `InitialiseCarData` of RACE / SEED_RUSH / TIME_ATTACK (UNRESOLVED: field meaning).
pub const GAME_3244_AFTER_INIT: f32 = 5.0;

/// `CGameModeTimeAttackData`: `+0x20` duration, `+0x24` time left, `+0x28` = 0, `+0x2c` warning-sound-given flag.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeAttackCarData {
    pub base: CarModeData,
    /// `+0x20`
    pub duration: f32,
    /// `+0x24`
    pub time_left: f32,
    /// `+0x28` (never changed by this class)
    pub unknown_28: i32,
    /// `+0x2c`
    pub warned: bool,
}

impl TimeAttackCarData {
    /// ctor `@ 00186bc4` / `Reset @ 00186b64`
    pub fn new(duration: f32) -> Self {
        TimeAttackCarData { base: CarModeData::default(), duration, time_left: duration, unknown_28: 0, warned: false }
    }

    /// `CGameModeTimeAttackData::Update @ 00186ab8`. Returns the UI sound event fired this frame, if any.
    pub fn update(&mut self, car_position: i32, clock_positive: bool, dt: f32) -> Option<u32> {
        let mut sound = None;
        if self.base.state == CarModeState::Running {
            if clock_positive {
                if !self.warned && self.time_left < LOW_TIME_WARNING_SECONDS {
                    sound = Some(UI_EVENT_LOW_TIME);
                    self.warned = true;
                }
                self.time_left -= dt;
            }
        } else {
            sound = Some(UI_EVENT_TIMER_STOP);
        }
        self.base.update_base(car_position, clock_positive, dt);
        sound
    }

    /// `CGameModeTimeAttack::GetEventCompleted @ 0018698c`
    pub fn event_completed(&self) -> bool {
        0.0 <= self.time_left
    }

    /// `CGameModeTimeAttack::GetBonusCoins @ 00186968`: `((int)t & ~((int)t >> 31)) * 10`
    pub fn bonus_coins(&self) -> i32 {
        (self.time_left as i32).max(0) * 10
    }
}

#[derive(Clone, Debug)]
pub struct TimeAttackMode {
    pub header: EventHeader,
    pub params: ModeParams,
    pub tier: DifficultyTier,
    /// `GetTimerDuration(tier)`
    pub duration: f32,
    pub cars: Vec<TimeAttackCarData>,
    /// `CGameMode+0x1c`
    pub mode_elapsed: f32,
    local_car: Option<CarId>,
    is_player: Vec<bool>,
    totals: ScoreTotals,
    time_counter_set: bool,
    pub position_counter_raw: Option<i32>,
    mode_state: ModeState,
    stars: u8,
}

impl TimeAttackMode {
    /// `CGameModeTimeAttack::CGameModeTimeAttack @ 00186d98` + `InitialiseCarData @ 00186c54`.
    pub fn new(header: &EventHeader, params: ModeParams) -> TimeAttackMode {
        let tier = params.tier();
        let duration = timer_duration(header, tier);
        let n = params.kart_count.max(1);
        let mut is_player = vec![false; n];
        is_player[0] = true;
        TimeAttackMode {
            header: header.clone(),
            tier,
            duration,
            cars: (0..n).map(|_| TimeAttackCarData::new(duration)).collect(),
            mode_elapsed: 0.0,
            local_car: Some(0),
            is_player,
            totals: ScoreTotals { base_score: params.base_score, bonus_raw: 0 },
            time_counter_set: false,
            position_counter_raw: None,
            mode_state: ModeState::Running,
            stars: 0,
            params,
        }
    }

    pub fn from_xml(xml: &str, params: ModeParams) -> Option<TimeAttackMode> {
        let h = EventHeader::from_xml(xml)?;
        if h.mode_id != mode_id::TIME_ATTACK {
            return None;
        }
        Some(TimeAttackMode::new(&h, params))
    }

    pub fn load(rel: &str, params: ModeParams) -> Option<TimeAttackMode> {
        let s = std::fs::read_to_string(format!("{}/xml/gameplay/{}", ASSETS292, rel)).ok()?;
        TimeAttackMode::from_xml(&s, params)
    }

    /// `CGameModeTimeAttack::GetTimeLeft @ 00186a74` (the local player's `+0x24`).
    pub fn time_left(&self) -> f32 {
        self.local_car.and_then(|i| self.cars.get(i)).map(|c| c.time_left).unwrap_or(0.0)
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
            self.cars.push(TimeAttackCarData::new(self.duration));
            self.is_player.push(false);
        }
    }

    /// `CGameMode::GetGameModeName @ 0017d4ac` case 6 localisation keys.
    pub fn name_key(challenge: bool) -> &'static str {
        if challenge {
            "MODE_TIME_ATTACK_CHALLENGE"
        } else {
            "MODE_TIME_ATTACK"
        }
    }

    /// `GetGameModeDesc` case 6.
    pub fn desc_key() -> &'static str {
        "GAMEMODE_DESC_TIMEATTACK"
    }

    fn finish(&mut self, car: CarId, events: &mut Vec<ModeEvent>) {
        if car >= self.cars.len() || self.cars[car].base.state != CarModeState::Running {
            return;
        }
        let is_player = self.is_player[car];
        let won = self.cars[car].event_completed();
        let pos = self.cars[car].base.position;
        let bonus = self.cars[car].bonus_coins();
        let star_completion = apply_finish(&mut self.cars[car].base, won, is_player, self.params.event_index, pos, bonus);
        if star_completion {
            events.push(ModeEvent::Other { tag: "AddCurrentEventStarCompletion", value: 1.0 });
        }
        if is_player && self.local_car == Some(car) {
            for c in self.cars.iter_mut() {
                if c.base.final_position == 0 {
                    c.base.final_position = c.base.position;
                }
            }
            // CCar::SetFinishLineCrossed @ 001aa158
            self.position_counter_raw = Some(finishing_position_score(8, self.params.event_cc));
            if !self.time_counter_set {
                self.time_counter_set = true;
                let c = &self.cars[car];
                self.totals.bonus_raw =
                    finishing_time_score(c.time_left, c.duration, self.params.event_cc, &self.params.score_cfg.time);
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

impl GameModeRules for TimeAttackMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::TimeAttack
    }

    /// `CGameModeTimeAttack::Update @ 00186c50` -> `CGameMode::ProcessUpdate`: mode clock, per-car data update. Emits the
    /// HUD countdown (`CXGSFE_TimeAttackTimerDisplay`) as `ModeEvent::Timer` for the local player and the UI sound events
    /// 0x1f / 0x20 as `ModeEvent::Other { tag: "UiSoundEvent" }`.
    fn update(&mut self, dt: f32, karts: &[KartState], _effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>) {
        self.mode_elapsed += dt;
        if let Some(m) = karts.iter().map(|k| k.id).max() {
            self.grow(m + 1);
        }
        for k in karts {
            self.is_player[k.id] = k.is_player;
            if k.is_player && self.local_car.is_none() {
                self.local_car = Some(k.id);
            }
            let local = self.local_car == Some(k.id);
            let s = self.cars[k.id].update(k.race_position as i32, k.race_time > 0.0, dt);
            if local {
                if let Some(code) = s {
                    events.push(ModeEvent::Other { tag: "UiSoundEvent", value: code as f32 });
                }
            }
        }
        if self.mode_state == ModeState::Running {
            events.push(ModeEvent::Timer { remaining: self.time_left() });
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

    /// Points (`GetScore() + GetBonusScore()`), the quantity the stars are compared against. Higher is better.
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

    fn params(cc: i32, tier: DifficultyTier) -> ModeParams {
        ModeParams {
            kart_count: 1,
            kart_cc: 50,
            event_cc: cc,
            event_index: 2,
            score_cfg: ScoreConfig::load().unwrap_or_default(),
            tier_override: Some(tier),
            ..ModeParams::default()
        }
    }

    fn kart(clock: f32) -> KartState {
        KartState { id: 0, is_player: true, race_position: 1, race_time: clock, ..KartState::default() }
    }

    const REAL: &str = "eventdef_episode01/eventdef_episode01_event01_stage00.xml";

    #[test]
    fn real_eventdef_time_attack() {
        let Some(h) = load_header(REAL) else { return };
        assert_eq!(h.kind(), GameModeKind::TimeAttack);
        assert_eq!(h.timer, [49.0, 48.0, 47.0, 46.0, 45.0]);
        assert_eq!(h.stars, [9400, 14100, 18800]);
        assert!((h.difficulty_adjust[2] - 0.2).abs() < 1e-6);
        let m = TimeAttackMode::load(REAL, params(55, DifficultyTier::VeryEasy)).unwrap();
        assert_eq!(m.duration, 49.0);
        let m = TimeAttackMode::load(REAL, params(55, DifficultyTier::Impossible)).unwrap();
        assert_eq!(m.duration, 45.0);
        assert_eq!(m.time_left(), 45.0);
    }

    #[test]
    fn countdown_runs_only_with_race_clock_and_never_clamps() {
        let mut h = EventHeader::default();
        h.timer = [10.0; 5];
        let mut m = TimeAttackMode::new(&h, params(50, DifficultyTier::Medium));
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update(1.0, &[kart(0.0)], &mut fx, &mut ev); // clock not started
        assert_eq!(m.time_left(), 10.0);
        for _ in 0..12 {
            m.update(1.0, &[kart(1.0)], &mut fx, &mut ev);
        }
        assert_eq!(m.time_left(), -2.0);
        assert_eq!(m.state(), ModeState::Running); // the race goes on
        // warning fired exactly once, at the first frame with timeLeft < 5
        let warns = ev.iter().filter(|e| matches!(e, ModeEvent::Other { tag: "UiSoundEvent", value } if *value == 31.0)).count();
        assert_eq!(warns, 1);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 12.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Lost); // timeLeft < 0 at the finish
        assert_eq!(m.cars[0].base.bonus_coins, 0);
    }

    #[test]
    fn win_with_time_left_gives_bonus_coins_and_points() {
        let mut h = EventHeader::default();
        h.timer = [47.0; 5];
        h.stars = [9400, 14100, 18800];
        let mut p = params(55, DifficultyTier::Medium);
        p.base_score = 6000;
        let mut m = TimeAttackMode::new(&h, p);
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        for _ in 0..37 {
            m.update(1.0, &[kart(1.0)], &mut fx, &mut ev);
        }
        assert_eq!(m.time_left(), 10.0);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 37.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Won);
        assert_eq!(m.cars[0].base.bonus_coins, 100);
        // finishing with time left: over < 0 -> clamped to 0 -> full MaxTimeScore: 8250 -> bonus 8300
        assert_eq!(m.score(), 6000 + 8300);
        assert_eq!(m.stars(), 2); // 14300 >= 14100, < 18800
    }

    #[test]
    fn time_score_curve() {
        let cfg = ScoreConfig::load().unwrap_or_default().time;
        if ScoreConfig::load().is_none() {
            return;
        }
        assert_eq!(cfg.percent_over_for_min, 0.35);
        let max = race_max_score(55);
        assert_eq!(max, 8250);
        // on time or early: full
        assert_eq!(finishing_time_score(5.0, 47.0, 55, &cfg), 8250);
        // 17.5% over: halfway between max (1.0) and min (0.125)
        assert_eq!(finishing_time_score(-47.0 * 0.175, 47.0, 55, &cfg), 4640);
        // 35% or more over: MinTimeScore
        assert_eq!(finishing_time_score(-47.0, 47.0, 55, &cfg), 1031);
    }

    #[test]
    fn stars_compare_points_higher_is_better() {
        let s = [9400, 14100, 18800];
        assert_eq!(stars_from_total(9399, &s), 0);
        assert_eq!(stars_from_total(9400, &s), 1);
        assert_eq!(stars_from_total(14100, &s), 2);
        assert_eq!(stars_from_total(18800, &s), 3);
        // GetStarsFromScore @ 000fee38 is strict and never below 1 once finished
        assert_eq!(stars_from_score_strict(9400, &s, false), 1);
        assert_eq!(stars_from_score_strict(14100, &s, false), 1);
        assert_eq!(stars_from_score_strict(14101, &s, false), 2);
        assert_eq!(stars_from_score_strict(20000, &s, true), 0);
    }
}
