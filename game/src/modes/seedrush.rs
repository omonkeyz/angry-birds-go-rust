//! SEED_RUSH (a.k.a. Fruit Rush, `fruit_rush_mode="FRUIT"`) and its ICECREAM variant ("Ice Splat").
//!
//! Ported from `CGameModeSeedRush` (ctor `@ 0017f3f8`, `InitialiseMode @ 0017f3cc`, `InitialiseCarData @ 0017f1cc`,
//! `Update @ 0017f1a8`, `GetEventCompleted @ 0017ead4`, `GetBonusCoins @ 0017eacc`, `GetAICharacter @ 0017f450`,
//! `CheckGameOverCondition @ 0017e940`), `CGameModeSeedRushData` (ctor `@ 0017f044`, `Reset @ 0017ef28`, `Update @ 0017eec0`),
//! the pickups `CPickupSeedRushToken::OnCarInRadius @ 001e871c` / `UpdateCoinTransformation @ 001ea244` /
//! `ShowCoins @ 001ea964` / `ShowFruit @ 001ea9d0` and `CPickupSeedRushTokenLarge::OnCarInRadius @ 001eada8`.
//!
//! What the original does (everything below is read from the decompile):
//! * Mode id 7 (`CGameMode::CreateGameMode` case 7). It is a RACE with a different rule set: the race still ends when a car
//!   crosses the finish line (`CGameMode::ProcessUpdate @ 0017caac`; SeedRush does not override the finish check).
//! * Per car `CGameModeSeedRushData` = `CGameModeData` + `+0x20` collected count, `+0x24` threshold
//!   (`CEventDefinitionManager::GetTokenThreshold(tier)` = eventdef `seed_amount_<tier>`), `+0x28` = collected/threshold.
//! * WIN = `1.0 <= collected/threshold` evaluated when the car crosses the finish (reaching the threshold does NOT end the race).
//! * Only cars driven by a human (`car+0x1af0 != 0`, a `CPlayer`) count seeds; AI karts never collect.
//! * After the threshold is reached the pickups turn into coins (`CCar::AddCoin`): see [`SeedRushMode::coin_state`].
//! * Bonus coins at finish: always 0 (`GetBonusCoins @ 0017eacc`).
//! * Score: bonus counter = `CScoreCounterFinishingFruit` (type 4), set once at the finish of the local player.
//! * ICECREAM: `fruit_rush_mode` is stored by `CGameModeManager::SetFruitRushMode @ 00183854`; the only readers visible in
//!   the decompile are the localisation lookups (`CGameMode::GetGameModeName @ 0017d4ac` -> "MODE_ICE_SPLAT",
//!   `GetGameModeDesc` -> "GAMEMODE_DESC_ICE_SPLAT") and the per-pickup particle choice (Banana/Melon/Strawberry vs
//!   IceWafer/IceLolly/IceCream). Scoring and win rules are identical.
#![allow(dead_code)]

use super::eventdef::*;
use super::types::*;

/// `CGameModeSeedRushData` (+0x20 / +0x24 / +0x28 on top of [`CarModeData`]).
#[derive(Clone, Debug, PartialEq)]
pub struct SeedRushCarData {
    pub base: CarModeData,
    /// `+0x20` fruit collected
    pub collected: i32,
    /// `+0x24` `GetTokenThreshold(tier)`
    pub threshold: i32,
    /// `+0x28` collected / threshold (refreshed by [`SeedRushCarData::update`] only)
    pub fraction: f32,
}

impl SeedRushCarData {
    /// `CGameModeSeedRushData::CGameModeSeedRushData @ 0017f044` / `Reset @ 0017ef28`.
    pub fn new(threshold: i32) -> Self {
        SeedRushCarData { base: CarModeData::default(), collected: 0, threshold, fraction: 0.0 }
    }

    /// `CGameModeSeedRushData::Update @ 0017eec0`: base update, then `fraction = (float)collected / (float)threshold`
    /// (plain float division like the original; the debug text "Fruits Collected: %i / %i" is a HUD sprite).
    pub fn update(&mut self, car_position: i32, clock_positive: bool, dt: f32) {
        self.base.update_base(car_position, clock_positive, dt);
        self.fraction = self.collected as f32 / self.threshold as f32;
    }

    /// `CGameModeSeedRush::GetEventCompleted @ 0017ead4`
    pub fn event_completed(&self) -> bool {
        1.0 <= self.fraction
    }
}

/// What the HUD shows (`CGameModeSeedRushData` text, `CXGSFE_SeedRushSubScreen @ 00346dc8` result line
/// "Bonus Coins: +%d Fruit: %d/%d").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeedRushHud {
    pub collected: i32,
    pub threshold: i32,
    pub fraction: f32,
    /// 0 fruit, 1 target just reached, 2/3 coins
    pub coin_state: u32,
    pub bonus_coins: i32,
}

#[derive(Clone, Debug)]
pub struct SeedRushMode {
    pub header: EventHeader,
    pub params: ModeParams,
    pub tier: DifficultyTier,
    /// `GetTokenThreshold(tier)`
    pub threshold: i32,
    pub cars: Vec<SeedRushCarData>,
    /// `CGameMode+0x1c`: seconds the mode has been updated
    pub mode_elapsed: f32,
    /// the static "coin transformation" state of `CPickupSeedRushToken` (`DAT_001ea8e8`): 0 = fruit, 1 = threshold reached
    /// (pickups still count as fruit), 2 = coins (pickups give `AddCoin`), 3 = transformation finished.
    pub coin_state: u32,
    coin_timer1: f32,
    coin_timer2: f32,
    is_player: Vec<bool>,
    /// coins collected per car (`CCar::AddCoin` calls requested)
    pub coins: Vec<i32>,
    coin_fx: Vec<CarEffect>,
    local_car: Option<CarId>,
    totals: ScoreTotals,
    fruit_counter_set: bool,
    /// `CScoreCounterFinishingPosition` raw value (forced to position 8 in modes 6/7); not a bonus counter in this mode
    pub position_counter_raw: Option<i32>,
    mode_state: ModeState,
    stars: u8,
}

/// `CPickupSeedRushToken::UpdateCoinTransformation` stage 1 length (`DAT_001ea954`), seconds.
pub const COIN_STAGE1_SECONDS: f32 = 0.3;
/// stage 2 length (`DAT_001ea950`), seconds.
pub const COIN_STAGE2_SECONDS: f32 = 0.6;

impl SeedRushMode {
    /// Build from a parsed eventdef header. `CGameModeSeedRush::CGameModeSeedRush @ 0017f3f8` +
    /// `InitialiseCarData @ 0017f1cc` (one data object per car, threshold from the tier).
    pub fn new(header: &EventHeader, params: ModeParams) -> SeedRushMode {
        let tier = params.tier();
        let threshold = token_threshold(header, tier);
        let n = params.kart_count.max(1);
        let mut is_player = vec![false; n];
        is_player[0] = true; // the host calls set_player() with the real flags; car 0 is the local player by default
        SeedRushMode {
            header: header.clone(),
            tier,
            threshold,
            cars: (0..n).map(|_| SeedRushCarData::new(threshold)).collect(),
            mode_elapsed: 0.0,
            coin_state: 0, // CPickupSeedRushToken::ShowFruit @ 001ea9d0
            coin_timer1: 0.0,
            coin_timer2: 0.0,
            is_player,
            coins: vec![0; n],
            coin_fx: Vec::new(),
            local_car: Some(0),
            totals: ScoreTotals { base_score: params.base_score, bonus_raw: 0 },
            fruit_counter_set: false,
            position_counter_raw: None,
            mode_state: ModeState::Running,
            stars: 0,
            params,
        }
    }

    pub fn from_xml(xml: &str, params: ModeParams) -> Option<SeedRushMode> {
        let h = EventHeader::from_xml(xml)?;
        if h.mode_id != mode_id::SEED_RUSH {
            return None;
        }
        Some(SeedRushMode::new(&h, params))
    }

    /// Load `xml/gameplay/<rel>` from the shipped assets.
    pub fn load(rel: &str, params: ModeParams) -> Option<SeedRushMode> {
        let s = std::fs::read_to_string(format!("{}/xml/gameplay/{}", ASSETS292, rel)).ok()?;
        SeedRushMode::from_xml(&s, params)
    }

    pub fn fruit_rush_mode(&self) -> FruitRushMode {
        self.header.fruit_rush_mode
    }

    /// `CGameMode::GetGameModeName @ 0017d4ac` localisation key for case 7.
    pub fn name_key(&self, challenge: bool) -> &'static str {
        match (self.header.fruit_rush_mode, challenge) {
            (FruitRushMode::Fruit, false) => "MODE_FRUIT_RUSH",
            (FruitRushMode::Fruit, true) => "MODE_FRUIT_RUSH_CHALLENGE",
            (FruitRushMode::IceCream, false) => "MODE_ICE_SPLAT",
            (FruitRushMode::IceCream, true) => "MODE_ICE_SPLAT_CHALLENGE",
        }
    }
    // UNRESOLVED: the plain-fruit non-challenge key is built from a PC-relative literal (`DAT_0017d624 + DAT_0017d670`)
    // that was not decoded; "MODE_FRUIT_RUSH" is inferred from its siblings ("MODE_FRUIT_RUSH_CHALLENGE").

    /// `CGameMode::GetGameModeDesc @ 0017d674` key for case 7.
    pub fn desc_key(&self) -> &'static str {
        match self.header.fruit_rush_mode {
            FruitRushMode::Fruit => "GAMEMODE_DESC_FRUIT_RUSH", // UNRESOLVED: literal not decoded, inferred from "GAMEMODE_DESC_ICE_SPLAT"
            FruitRushMode::IceCream => "GAMEMODE_DESC_ICE_SPLAT",
        }
    }

    /// Declare which karts are driven by a human (`car+0x1af0 != 0`); only those count seeds.
    pub fn set_player(&mut self, car: CarId, is_player: bool) {
        if car >= self.cars.len() {
            self.grow(car + 1);
        }
        self.is_player[car] = is_player;
        if is_player && self.local_car.is_none() {
            self.local_car = Some(car);
        }
    }

    fn grow(&mut self, n: usize) {
        while self.cars.len() < n {
            self.cars.push(SeedRushCarData::new(self.threshold));
            self.is_player.push(false);
            self.coins.push(0);
        }
    }

    pub fn hud(&self) -> SeedRushHud {
        let c = self.local_car.and_then(|i| self.cars.get(i));
        SeedRushHud {
            collected: c.map(|c| c.collected).unwrap_or(0),
            threshold: self.threshold,
            fraction: c.map(|c| c.fraction).unwrap_or(0.0),
            coin_state: self.coin_state,
            bonus_coins: c.map(|c| c.base.bonus_coins).unwrap_or(0),
        }
    }

    /// `CPickupSeedRushToken::UpdateCoinTransformation(float) @ 001ea244`.
    fn update_coin_transformation(&mut self, dt: f32) {
        if self.coin_state == 1 {
            self.coin_timer1 += dt;
            if COIN_STAGE1_SECONDS < self.coin_timer1 {
                self.coin_state = 2;
            }
        } else if self.coin_state == 2 {
            self.coin_timer2 += dt;
            if self.coin_timer2 >= COIN_STAGE2_SECONDS {
                self.coin_state = 3;
            }
        }
    }

    /// `CPickupSeedRushToken::OnCarInRadius @ 001e871c` (the part after the radius test). `large` = `CPickupSeedRushTokenLarge`
    /// (`@ 001eada8`), which only increments the count.
    pub fn token_pickup(&mut self, car: CarId, large: bool, events: &mut Vec<ModeEvent>, effects: &mut Vec<CarEffect>) {
        if car >= self.cars.len() || !self.is_player[car] {
            return; // `if (car+0x1af0 != 0)` gate: AI karts never collect
        }
        if large {
            self.cars[car].collected += 1;
            events.push(ModeEvent::Other { tag: "FruitCollected", value: self.cars[car].collected as f32 });
            return;
        }
        if self.coin_state < 2 {
            self.cars[car].collected += 1;
            events.push(ModeEvent::Other { tag: "FruitCollected", value: self.cars[car].collected as f32 });
            // CAchievementsManager "PickUpFruit", CSeasonalContentManager::UpdateChallenges(3, 1.0), challenge event
            events.push(ModeEvent::Other { tag: "Achievement:PickUpFruit", value: 1.0 });
            if self.cars[car].threshold <= self.cars[car].collected && self.coin_state == 0 {
                // ShowCoins-equivalent block: state = 1, UI sound 0x22
                self.coin_state = 1;
                self.coin_timer1 = 0.0;
                self.coin_timer2 = 0.0;
                events.push(ModeEvent::Other { tag: "UiSoundEvent", value: 0x22 as f32 });
            }
        } else {
            self.coins[car] += 1;
            effects.push(CarEffect::Other { tag: "AddCoin", car: Some(car), vals: [1.0, 0.0, 0.0, 0.0], pos: Vec3::ZERO });
            events.push(ModeEvent::Other { tag: "Achievement:PickUpCoins", value: 1.0 });
        }
    }

    /// The finish handling of `CGameMode::ProcessUpdate @ 0017caac` for one car, with SeedRush's virtuals.
    fn finish(&mut self, car: CarId, events: &mut Vec<ModeEvent>) {
        if car >= self.cars.len() || self.cars[car].base.state != CarModeState::Running {
            return;
        }
        let is_player = self.is_player[car];
        let won = self.cars[car].event_completed(); // GetEventCompleted @ 0017ead4
        let pos = self.cars[car].base.position;
        let bonus_coins = 0; // GetBonusCoins @ 0017eacc
        let star_completion = apply_finish(&mut self.cars[car].base, won, is_player, self.params.event_index, pos, bonus_coins);
        if star_completion {
            events.push(ModeEvent::Other { tag: "AddCurrentEventStarCompletion", value: 1.0 });
        }
        let local = is_player && self.local_car == Some(car);
        if local {
            // fill final positions of the other cars that have none yet (ProcessUpdate, local player branch)
            for c in self.cars.iter_mut() {
                if c.base.final_position == 0 {
                    c.base.final_position = c.base.position;
                }
            }
            // CCar::SetFinishLineCrossed @ 001aa158: position counter (8 in modes 6/7), fruit counter (mode 7)
            self.position_counter_raw = Some(finishing_position_score(8, self.params.event_cc));
            if !self.fruit_counter_set {
                self.fruit_counter_set = true;
                self.totals.bonus_raw =
                    finishing_fruit_score(self.cars[car].fraction, self.params.event_cc, &self.params.score_cfg.fruit);
            }
            self.totals.base_score = self.params.base_score;
            let total = self.totals.total();
            self.stars = stars_from_total(total, &self.header.stars);
            self.mode_state = if won { ModeState::Won } else { ModeState::Lost };
            events.push(ModeEvent::ScoreChanged { car, score: total as i64 });
            events.push(ModeEvent::Finished { won, score: total as i64, stars: self.stars });
        }
    }

    /// `CGameModeSeedRush::GetAICharacter(int) @ 0017f450`: among the first 0x10 character names pick the LAST one called
    /// "MinionPig"; `None` => fall back to `CGameMode::GetAICharacter` (`GetRandomNonDuplicateCharacter(2, param == 0)`).
    pub fn ai_character(character_names: &[&str]) -> Option<usize> {
        let mut found = None;
        for (i, n) in character_names.iter().take(0x10).enumerate() {
            if *n == "MinionPig" {
                found = Some(i);
            }
        }
        found
    }

    /// `GetBonusCoins` for the finished car (always 0 in this mode).
    pub fn bonus_coins(&self, _car: CarId) -> i32 {
        0
    }
}

impl GameModeRules for SeedRushMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::SeedRush
    }

    /// `CGameModeSeedRush::Update @ 0017f1a8`: `CPickupSeedRushToken::UpdateCoinTransformation(dt)` then
    /// `CGameMode::Update` -> `ProcessUpdate` (mode clock + per-car `CGameModeSeedRushData::Update`).
    fn update(&mut self, dt: f32, karts: &[KartState], effects: &mut Vec<CarEffect>, _events: &mut Vec<ModeEvent>) {
        effects.append(&mut self.coin_fx);
        self.update_coin_transformation(dt);
        self.mode_elapsed += dt;
        if let Some(m) = karts.iter().map(|k| k.id).max() {
            self.grow(m + 1);
        }
        for k in karts {
            self.is_player[k.id] = k.is_player;
            if k.is_player && self.local_car.is_none() {
                self.local_car = Some(k.id);
            }
            let d = &mut self.cars[k.id];
            d.update(k.race_position as i32, k.race_time > 0.0, dt);
        }
    }

    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        match input {
            ModeInput::SeedCollected { car, count } => {
                let mut fx = Vec::new();
                for _ in 0..*count {
                    self.token_pickup(*car, false, events, &mut fx);
                }
                // coins requested by pickups after the target are returned through `take_coin_effects`
                self.pending_effects(fx);
            }
            ModeInput::KartFinished { car, .. } => self.finish(*car, events),
            ModeInput::Other { tag: "seed_rush_token_large", car: Some(car), .. } => {
                let mut fx = Vec::new();
                self.token_pickup(*car, true, events, &mut fx);
            }
            _ => {}
        }
    }

    fn state(&self) -> ModeState {
        self.mode_state
    }

    /// `GetScore() + GetBonusScore()` of the local player (`CResultsScreen+0x164`), 0 bonus until the finish.
    fn score(&self) -> i64 {
        self.totals.total() as i64
    }

    fn stars(&self) -> u8 {
        self.stars
    }
}

impl SeedRushMode {
    fn pending_effects(&mut self, fx: Vec<CarEffect>) {
        self.coin_fx.extend(fx);
    }

    /// Coin requests (`CCar::AddCoin`) produced by pickups since the last `update` (also drained into `update`'s effects).
    pub fn take_coin_effects(&mut self) -> Vec<CarEffect> {
        std::mem::take(&mut self.coin_fx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(cc: i32, tier: DifficultyTier) -> ModeParams {
        ModeParams {
            kart_count: 2,
            kart_cc: 50,
            event_cc: cc,
            event_index: 3,
            score_cfg: ScoreConfig::load().unwrap_or_default(),
            tier_override: Some(tier),
            ..ModeParams::default()
        }
    }

    fn kart(id: usize, player: bool, pos: u32) -> KartState {
        KartState { id, is_player: player, race_position: pos, race_time: 1.0, ..KartState::default() }
    }

    #[test]
    fn real_eventdef_seed_rush() {
        let Some(h) = load_header("eventdef_episode01/eventdef_episode01_event03_stage00.xml") else { return };
        assert_eq!(h.kind(), GameModeKind::SeedRush);
        assert_eq!(h.kartcount, 2);
        assert_eq!(h.fruit_rush_mode, FruitRushMode::Fruit);
        assert_eq!(h.stars, [21500, 23500, 24500]);
        // tier-indexed thresholds
        assert_eq!(token_threshold(&h, DifficultyTier::VeryEasy), 90);
        assert_eq!(token_threshold(&h, DifficultyTier::Medium), 100);
        assert_eq!(token_threshold(&h, DifficultyTier::Impossible), 110);
        let m = SeedRushMode::load("eventdef_episode01/eventdef_episode01_event03_stage00.xml", params(55, DifficultyTier::Medium)).unwrap();
        assert_eq!(m.threshold, 100);
        assert_eq!(m.name_key(false), "MODE_FRUIT_RUSH");
    }

    #[test]
    fn real_eventdef_icecream() {
        let Some(h) = load_header("eventdef_episode04/eventdef_episode04_event03_stage00.xml") else { return };
        assert_eq!(h.fruit_rush_mode, FruitRushMode::IceCream);
        assert_eq!(h.seed_amount, [90, 100, 110, 120, 130]);
        assert_eq!(h.stars, [9500, 14500, 16500]);
        let m = SeedRushMode::new(&h, params(55, DifficultyTier::Hard));
        assert_eq!(m.threshold, 120);
        assert_eq!(m.name_key(false), "MODE_ICE_SPLAT");
        assert_eq!(m.desc_key(), "GAMEMODE_DESC_ICE_SPLAT");
    }

    #[test]
    fn win_requires_threshold_at_finish_and_ai_never_collects() {
        let mut h = EventHeader::default();
        h.seed_amount = [4, 4, 4, 4, 4];
        h.stars = [1000, 2000, 9000];
        let mut m = SeedRushMode::new(&h, params(50, DifficultyTier::Medium));
        let karts = [kart(0, true, 1), kart(1, false, 2)];
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update(0.016, &karts, &mut fx, &mut ev);
        m.on_input(&ModeInput::SeedCollected { car: 1, count: 10 }, &mut ev); // AI: ignored
        assert_eq!(m.cars[1].collected, 0);
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 3 }, &mut ev);
        m.update(0.016, &karts, &mut fx, &mut ev);
        // 3/4 at the finish: lost
        let mut m2 = m.clone();
        m2.on_input(&ModeInput::KartFinished { car: 0, time: 40.0 }, &mut ev);
        assert_eq!(m2.state(), ModeState::Lost);
        // 4/4: won (threshold reached before the finish does not end the race)
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 1 }, &mut ev);
        assert_eq!(m.coin_state, 1);
        assert_eq!(m.state(), ModeState::Running);
        m.update(0.016, &karts, &mut fx, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 40.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Won);
        assert!(ev.iter().any(|e| matches!(e, ModeEvent::Finished { won: true, .. })));
    }

    #[test]
    fn coin_transformation_timing_and_coins() {
        let mut h = EventHeader::default();
        h.seed_amount = [1, 1, 1, 1, 1];
        let mut m = SeedRushMode::new(&h, params(50, DifficultyTier::Medium));
        let karts = [kart(0, true, 1)];
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 1 }, &mut ev);
        assert_eq!(m.coin_state, 1);
        m.update(0.25, &karts, &mut fx, &mut ev);
        assert_eq!(m.coin_state, 1);
        m.update(0.06, &karts, &mut fx, &mut ev); // 0.31 > 0.3
        assert_eq!(m.coin_state, 2);
        // pickups are now coins: fruit count stays 1
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 2 }, &mut ev);
        assert_eq!(m.cars[0].collected, 1);
        assert_eq!(m.coins[0], 2);
        assert_eq!(m.take_coin_effects().len(), 2);
        m.update(0.6, &karts, &mut fx, &mut ev);
        assert_eq!(m.coin_state, 3);
    }

    #[test]
    fn score_and_stars_worked_example() {
        // cc 55 -> race max 8250; fraction 1.0 -> fruit counter 8250*1.0 -> bonus rounds up to 8300.
        let mut h = EventHeader::default();
        h.seed_amount = [10, 10, 10, 10, 10];
        h.stars = [21500, 23500, 24500];
        let mut p = params(55, DifficultyTier::Medium);
        p.base_score = 13500; // non-bonus counters supplied by the host
        let mut m = SeedRushMode::new(&h, p);
        let karts = [kart(0, true, 1)];
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 10 }, &mut ev);
        m.update(0.016, &karts, &mut fx, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 30.0 }, &mut ev);
        assert_eq!(m.totals.bonus_raw, 8250);
        assert_eq!(m.score(), 13500 + 8300);
        assert_eq!(m.stars(), 1); // 21800 >= 21500, < 23500
        // half the fruit: 8250*(0.125 + 0.875*0.5) = 4640 -> 4700
        assert_eq!(finishing_fruit_score(0.5, 55, &FruitCounterCfg { min_score: 0.125, ..FruitCounterCfg::default() }), 4640);
        assert_eq!(round_bonus_score(4640), 4700);
    }

    #[test]
    fn ai_character_prefers_last_minion_pig() {
        let n = ["red", "MinionPig", "blue", "MinionPig", "x"];
        assert_eq!(SeedRushMode::ai_character(&n), Some(3));
        assert_eq!(SeedRushMode::ai_character(&["red"]), None);
    }
}
