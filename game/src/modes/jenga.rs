//! JENGA game mode (`<GameMode name="JENGA"/>`, `EGameMode` id 10), ported from the Angry Birds Go 2.9.1 decompile.
//!
//! Original classes: `CGameModeJenga` (vtable `0xd91ae0`), per-car data `CGameModeData` (the local player's car only),
//! score counter `CScoreCounterJenga` (`EScoreCounterType` 5), HUD/flow `GameUI::CJengaScreen`.
//!
//! # What the mode is
//! A "block tower smash" bonus event (eventdefs `eventdef_jenga_1..3.xml`, episodes `episode_jenga_00..02`, `kart_type="OFFR"`):
//! the player is slung down a short ramp straight into a tower made of stone / glass / wood blocks, TNT and pirate pigs.
//! The tower region is delimited by two env helpers (`trophy_helper_01`/`trophy_helper_02`, see [`TowerBounds`]).
//! The player gets [`TRIES`] (4) launches. A launch ends when the pilot is thrown off, or the kart is destroyed
//! (leaves the spline corridor), or the physics settle. After every launch the mode counts the pigs
//! (types `0x32`/`0x33`, `smck_PiratePig1_3m`/`2_3m`) that were smashed or fell out of the tower region; if any pig is
//! still in the tower and tries remain, the kart is respawned for another launch; if all pigs are gone the event is WON
//! (data state 2, `OnJengaWin`), if the tries run out with pigs left it is LOST (state 3, `OnJengaLose`).
//!
//! # Scoring (the only active score counter in this mode, `CScoreSystem::IsCounterAvailable(5)`)
//! * every smackable of one of the 12 tower types that is smashed (`OnSmashedCallback @ 00180380`) adds its points
//!   immediately ([`points_for_type`], table at `0xcec978` indexed by `type - 0x2d`);
//! * at game over (`CheckGameOverCondition @ 001823e4`) every block of those types that is *outside* the tower region
//!   (fallen) adds its points once more (`fallen[type] * points`).
//! The eventdef `<Stars Star1= Star2= Star3=>` (60000/80000/90000) are compared with that total
//! (`CEventDefinitionManager::GetStarsFromScore @ 000fee38`).
//!
//! # What differs from a RACE (overridden `CGameMode` virtuals)
//! slot 2 `InitialiseMode @ 00182e0c`, 3 `InitialiseCarData @ 001832b0` (only car 0 gets a data object),
//! 4 `InitialiseGrid @ 00183320` (base + `CPlayer::SetSlingshotEnabled(player, 1)`), 5 `Update @ 001809d0`,
//! 7 `CheckGameOverCondition @ 001823e4`, 13 `OnWeaponFire @ 001802f8`, 14 `OnCarCollision @ 001802b0`;
//! `OnQuitEvent @ 001802e0` (`ResetSlowMo`) and `OnFinishEvent @ 00180798` (remove kart trail) are reached through the
//! non-vtable callers. `GetEventCompleted` is *not* overridden (base returns 0): the mode writes the result state
//! (2 = win, 3 = lose) itself inside `CheckGameOverCondition`, so a host must not derive win/lose from
//! `GetEventCompleted` for Jenga.
//!
//! # No timer, access and entry flow
//! There is **no clock** in this mode: the only limit is the [`TRIES`] launches (the per-car data is a plain `CGameModeData`,
//! no countdown is read or written anywhere in `CGameModeJenga`). The event is reached from the settings screen (`CSettingsScreen::OnJenga @
//! 0042e298` -> state `jengaScreen`, needs `CPlayerInfo::CheckConnectivity`) which opens `GameUI::CJengaScreen`
//! (`uijengascreen.xml`: upsell text `JENGA_UPSELL`, title `JENGASCREEN_TITLE`, buttons Play (`PlayJengaSelected` ->
//! `OnPlayJenga @ 0043ae80`), Buy (`BuyJengaSelected` -> `OnBuyJenga @ 0043b444` -> `CShopManager::AttemptPurchase(jenga product)`, price label +
//! no-connection icon) and Enter code (`EnterCodeSelected`, the code entry screen); `CAnalyticsManager::PurchaseJenga @ 002352c0`,
//! `JengaIAPUnlock @ 002409c8` and `JengaCodeUnlock @ 00241110` are only the analytics reports of those unlocks). Access is gated on `CPlayerInfo::IsJengaUnlocked` (`CPlayerInfo+0xa10`, set by `SetJengaUnlocked @ 0014ccb0`, also
//! unlocked by a bundle item, the code, or save migration `bJengaUnlocked`). `OnPlayJenga` hard-codes the episode name
//! `"episode_jenga_00"` (= `EventDef_Jenga_1.xml`), unlocks the kart named by `MakeNameTag(..)` if the player does not own it,
//! selects it, calls `CGame::SetJenga(1)` and `RequestStateChange_FrontendToGameplayLoading`. `episode_jenga_01` and
//! `episode_jenga_02` (`EventDef_Jenga_2/3.xml`) exist in `episode_config.xml` but nothing in the 2.9.1 binary names them:
//! `CEventDefinitionManager::GetRandomEpisodeIndex @ 000fef34(1, ..)` can pick a random jenga episode (it filters the episodes
//! whose first event is mode 10) but has no caller in the decompile. Achievement tracker `PlayJenga`.
//!
//! UNRESOLVED items are marked `// UNRESOLVED:` below.
#![allow(dead_code)]

use super::types::*;

/// `EGameMode` id (`StringToGameMode .part.23 @ 000a3f90`, `CGameModeJenga::CGameModeJenga`: `this+0xc = 10`).
pub const GAME_MODE_ID: u32 = 10;
/// Launches per event (`CGameModeJenga::InitialiseMode`: `this+0x24 = 4`).
pub const TRIES: i32 = 4;
/// Smackable types that count (`GetSmackableTypeFromEnvObjectType` result), in the original table order
/// (`CGameModeJenga+0x28` array of `{total, smashed, fallen}` 12-byte entries; id list at `0xceca18`).
pub const BLOCK_TYPES: [i32; 12] = [0x2e, 0x2f, 0x2d, 0x51, 0x50, 0x53, 0x52, 0x4f, 0x4e, 0x32, 0x33, 0x30];
/// Smackable type names (`CSmackableManager` type table at `0xdad720`, 0x40-byte records, name at +0).
pub const BLOCK_TYPE_NAMES: [&str; 12] = [
    "smck_block_glass_3m_sq",
    "smck_block_wood_3m_sq",
    "smck_block_stone_3m_sq",
    "smck_block_stone_3X6_w",
    "smck_block_stone_3X6_h",
    "smck_block_wood_3X6_w",
    "smck_block_wood_3X6_h",
    "smck_block_glass_3X6_w",
    "smck_block_glass_3X6_h",
    "smck_PiratePig1_3m",
    "smck_PiratePig2_3m",
    "smck_TNT_Box_3m",
];
/// The two pig types (`type - 0x32 < 2` tests in the original).
pub const PIG_TYPES: [i32; 2] = [0x32, 0x33];

/// Points of one block of smackable type `t` (`float[0x27]` table at `0xcec978`, index `t - 0x2d`, 0 outside the table):
/// stone/glass/wood 3 m cubes 500, `smck_TNT_Box_3m` 8000, pirate pigs 5000, the six 3x6 blocks 1000.
pub fn points_for_type(t: i32) -> f32 {
    match t {
        0x2d | 0x2e | 0x2f => 500.0,
        0x30 => 8000.0,
        0x32 | 0x33 => 5000.0,
        0x4e..=0x53 => 1000.0,
        _ => 0.0,
    }
}

/// Index of a smackable type in [`BLOCK_TYPES`] (the original's if/else chain), `None` when the type is not tracked.
pub fn block_index(t: i32) -> Option<usize> {
    BLOCK_TYPES.iter().position(|&x| x == t)
}

/// Smackable type id from its model/type name (case-insensitive; accepts `smck_PiratePig1_3m` and `smck_piratepig1_3m.xgm`).
pub fn type_id_from_name(name: &str) -> Option<i32> {
    let n = name.trim_end_matches(".xgm").to_ascii_lowercase();
    BLOCK_TYPE_NAMES.iter().position(|b| b.to_ascii_lowercase() == n).map(|i| BLOCK_TYPES[i])
}

/// Result state values of `CGameModeData+4` written by `CheckGameOverCondition`.
pub mod data_state {
    pub const RUNNING: u8 = 1;
    pub const WON: u8 = 2;
    pub const LOST: u8 = 3;
}

/// Slow-motion parameters of `CGame::EnterSlowMo(scale, hold, ramp_in, ramp_out) @ 0011acb8`
/// (stored at `CGame+0x3278/0x3284/0x3280/0x3288`; `GetCurrentSlowMoTimeMultiplier @ 0011ace0` ramps `1 -> scale` over
/// `ramp_in`, holds `hold` seconds, then ramps back over `ramp_out`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlowMo {
    pub scale: f32,
    pub hold: f32,
    pub ramp_in: f32,
    pub ramp_out: f32,
}
/// Entering slow motion when the kart crosses plane 1 (`Update`: `scale 0.35`, hold forever (`0x7f800000` = +inf), ramp in 0.1 s).
pub const SLOWMO_ENTER: SlowMo = SlowMo { scale: 0.35, hold: f32::INFINITY, ramp_in: 0.1, ramp_out: 0.0 };
/// Leaving slow motion 0.5 s after plane 2 (`scale 1.0`, ramp 0.1 s).
pub const SLOWMO_LEAVE: SlowMo = SlowMo { scale: 1.0, hold: f32::INFINITY, ramp_in: 0.1, ramp_out: 0.0 };

/// Distance (race total spline distance) after which the decor swap happens (`DAT_00180f18` = 40.0).
pub const SCENERY_SWAP_DIST: f32 = 40.0;
/// `OnWeaponFire` camera shake threshold (`DAT_00180378` = 40.0) and the env-object type that triggers it
/// (`matildaegg_whole` smackable type `0x5b`).
pub const WEAPON_SHAKE_MIN_DIST: f32 = 40.0;
pub const WEAPON_SHAKE_TYPE: i32 = 0x5b;
/// Seconds after plane 2 before slow motion is left (`0.5`) and the settle window (`2.0`).
pub const SLOWMO_LEAVE_DELAY: f32 = 0.5;
pub const SETTLE_TIME: f32 = 2.0;
/// Seconds after the kart was destroyed before the try is judged (`this+0x9c >= 1.0`).
pub const DESTROYED_JUDGE_DELAY: f32 = 1.0;
/// Env-object type ids toggled when the decor swaps (`Update`/`InitialiseMode` switch statements): ids `0x41,0x42` are
/// hidden at the start and shown after [`SCENERY_SWAP_DIST`]; ids `0x5f..=0x63` the other way round.
pub const SCENERY_SHOW_AFTER: [i32; 2] = [0x41, 0x42];
pub const SCENERY_HIDE_AFTER: [i32; 5] = [0x5f, 0x60, 0x61, 0x62, 0x63];
/// Effect names / xml files (`InitialiseMode`: loaded from `EFFECTPAK:xml/`).
pub const FX_TRAIL: &str = "kart_trail.xml";
pub const FX_BOOST: &str = "jenga_boost.xml";
pub const FX_SCORE_GREEN_5: &str = "score_green_5.xml";
pub const FX_KART_DESTROYED: &str = "Destructible/KartDestroyedBits.xml";
pub const FX_JET_STREAM_NAME: &str = "JengaJetStreamEffect";
pub const FX_KART_DESTRUCTION_NAME: &str = "KartDestructionEffect";
pub const FX_GREEN_PIG_SCORE_NAME: &str = "JengaGreenPigScoreEffect";
pub const SND_PIG_POP: &str = "ABY_jenga_breakables_minion_pop";

// ------------------------------------------------------------------------------------------------------------
// Eventdef
// ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct JengaEventDef {
    /// `<Environment pathname=...>` e.g. `environments\theme002\tracks\run002`
    pub environment: String,
    pub character: String,
    pub difficulty_level: f32,
    pub stars: [i32; 3],
}

fn tag_attr<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    // tolerant: first `<tag ...>` occurrence, first `attr="..."` inside it (shipped data may carry duplicates)
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(p) = xml[from..].find(&open) {
        let s = from + p;
        let after = s + open.len();
        match xml.as_bytes().get(after) {
            Some(c) if c.is_ascii_whitespace() || *c == b'/' || *c == b'>' => {
                let e = xml[s..].find('>').map(|e| s + e).unwrap_or(xml.len());
                let body = &xml[s..e];
                let key = format!("{attr}=\"");
                let k = body.find(&key)?;
                let vs = k + key.len();
                let ve = body[vs..].find('"')?;
                return Some(&body[vs..vs + ve]);
            }
            _ => from = after,
        }
    }
    None
}

impl JengaEventDef {
    pub fn from_xml(xml: &str) -> Result<JengaEventDef, String> {
        match tag_attr(xml, "GameMode", "name") {
            Some("JENGA") => {}
            Some(o) => return Err(format!("GameMode is {o}, not JENGA")),
            None => return Err("no <GameMode name=...>".into()),
        }
        let mut stars = [0i32; 3];
        for (i, n) in ["Star1", "Star2", "Star3"].iter().enumerate() {
            if let Some(v) = tag_attr(xml, "Stars", n) {
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
        Ok(JengaEventDef {
            environment: tag_attr(xml, "Environment", "pathname").unwrap_or("").to_string(),
            character: tag_attr(xml, "Character", "name").unwrap_or("").to_string(),
            difficulty_level: tag_attr(xml, "Difficulty", "level").and_then(|v| v.parse().ok()).unwrap_or(0.5),
            stars,
        })
    }
}

/// `GetStarsFromScore @ 000fee38`: 0 while running, 3 / 2 for `score > Star3 / Star2`, else 1.
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
// Tower region and block bookkeeping
// ------------------------------------------------------------------------------------------------------------

/// Axis aligned box of the tower: the bounding box of the env helpers named `trophy_helper_01` and `trophy_helper_02`
/// (`CGameModeJenga::CGameModeJenga @ 0018147c`: only the first helper matching each name pattern is used;
/// `StringPartialMatchNoCase`). Also holds the two trigger planes derived from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TowerBounds {
    pub min: Vec3,
    pub max: Vec3,
    /// `this+0x48..0x50` centre and `this+0x54..0x5c` half extents
    pub centre: Vec3,
    pub half: Vec3,
    /// plane 1 point (`this+0x64..0x6c` = min + (0, 0, 25)) and plane 2 point (`this+0x70..0x78` = min + (0, 0, 10));
    /// both use the normal `(this+0x7c, 0x80, 0x84) = (0, 0, 1)`.
    pub plane1: Vec3,
    pub plane2: Vec3,
    pub plane_normal: Vec3,
}

impl TowerBounds {
    /// Initial values `0x501502f9` / `0xd01502f9` (+-9.99e9) of min / max.
    pub fn from_helpers(helpers: &[(&str, Vec3)]) -> TowerBounds {
        let mut min = Vec3::splat(f32::from_bits(0x501502f9));
        let mut max = Vec3::splat(f32::from_bits(0xd01502f9));
        for pat in ["trophy_helper_01", "trophy_helper_02"] {
            if let Some((_, p)) = helpers.iter().find(|(n, _)| n.to_ascii_lowercase().contains(pat)) {
                min = min.min(*p);
                max = max.max(*p);
            }
        }
        let centre = (min + max) * 0.5;
        let half = max - centre;
        // DAT_001818c0 = 0.0 (x/y offsets of the planes and the x/y of the normal), z offsets 25.0 / 10.0, normal z = 1.0
        TowerBounds {
            min,
            max,
            centre,
            half,
            plane1: Vec3::new(min.x, min.y, min.z + 25.0),
            plane2: Vec3::new(min.x, min.y, min.z + 10.0),
            plane_normal: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    /// Squared distance from `p` to the box (0 inside): the `fVar5` computed in `IsSmackableFallen` & co.
    pub fn dist2(&self, p: Vec3) -> f32 {
        let mut d = 0.0;
        for (v, lo, hi) in [(p.x, self.min.x, self.max.x), (p.y, self.min.y, self.max.y), (p.z, self.min.z, self.max.z)] {
            if v < lo {
                d += (v - lo) * (v - lo);
            } else if v > hi {
                d += (v - hi) * (v - hi);
            }
        }
        d
    }
}

/// Radius used by `IsSmackableFallen`: half the diagonal of the model bounding box.
pub fn block_radius(model_min: Vec3, model_max: Vec3) -> f32 {
    ((model_max - model_min) * 0.5).length()
}

/// One tower smackable as seen by the mode (`CEnvObject` with a smackable of one of the [`BLOCK_TYPES`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JengaBlock {
    pub type_id: i32,
    /// world position (`(*smackable->vtable[7])(&pos)`)
    pub pos: Vec3,
    /// [`block_radius`] of its model
    pub radius: f32,
    /// `smackable->vtable[10]() != 0` (still simulated / not asleep); only used by the settle check
    pub active: bool,
}

/// `IsSmackableFallen @ 001819b0`: the block's bounding sphere is completely outside the tower box
/// (`radius^2 <= dist2`).
pub fn is_fallen(b: &JengaBlock, tower: &TowerBounds) -> bool {
    b.radius * b.radius <= tower.dist2(b.pos)
}

/// `{total, smashed, fallen}` per tracked type (12-byte records at `CGameModeJenga+0x28`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockCount {
    pub total: i32,
    pub smashed: i32,
    pub fallen: i32,
}

/// What the host tells the mode every frame about the local player's kart. `race_total_dist` is
/// `CCar::GetRaceTotalSplineDist`, `spline_end_dist` is `CSpline+0x38` (the point after which the kart has left the ramp).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JengaCar {
    pub pos: Vec3,
    pub race_total_dist: f32,
    pub spline_end_dist: f32,
    /// `CCar+0x464 != 0 && CCar+0x468 != -1`: the kart is still sitting in the slingshot
    pub in_slingshot: bool,
    /// `CPlayer+0x234` (UNRESOLVED name; the jet stream starts when it is > 0.25)
    pub launch_power: f32,
    /// `CSpline::CheckOutOfSplineCorridor(...)` for the kart
    pub out_of_corridor: bool,
    /// `CCar::IsPilotDetached`
    pub pilot_detached: bool,
    /// `CCar+0x4a0` (UNRESOLVED name: timer tested `< 2.0` together with `pilot_detached`)
    pub pilot_timer: f32,
    /// `CCar+0x488` input of the launch blend (UNRESOLVED name), see [`JengaMode::launch_blend`]
    pub car_488: f32,
    /// `CPlayer+0x234` input of the launch blend (the same value as `launch_power`; kept separate because the original
    /// reads it as `pfVar2[0x8d]` in both places)
    pub player_234: f32,
}

impl Default for JengaCar {
    /// `spline_end_dist` defaults to +inf-ish so a host that never reports it does not make the kart look "past the ramp".
    fn default() -> Self {
        JengaCar {
            pos: Vec3::ZERO,
            race_total_dist: 0.0,
            spline_end_dist: f32::MAX,
            in_slingshot: false,
            launch_power: 0.0,
            out_of_corridor: false,
            pilot_detached: false,
            pilot_timer: 0.0,
            car_488: 0.0,
            player_234: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JengaGameOver {
    /// `CheckGameOverCondition` returned 0 and nothing happened
    NotOver,
    /// returned 0 after respawning the kart for the next launch (`tries` already decremented)
    Retry,
    /// returned 1; `won` = no pig left in the tower
    Over { won: bool },
}

#[derive(Clone, Debug)]
pub struct JengaMode {
    pub def: JengaEventDef,
    pub tower: TowerBounds,
    /// `this+0x24`: launches left
    pub tries: i32,
    /// `this+0x1c` mode clock
    pub clock: f32,
    /// `this+0x8c` kart is past the end of the spline (flying into the tower)
    pub past_end: bool,
    /// `this+0x90` plane 1 crossed (slow motion entered)
    pub plane1_crossed: bool,
    /// `this+0x94` slow motion left again
    pub slowmo_left: bool,
    /// `this+0x98` decor swap done
    pub scenery_swapped: bool,
    /// `this+0x88` seconds since plane 2 (-1 = not reached)
    pub t_plane2: f32,
    /// `this+0x9c` seconds since the kart was destroyed (-1 = alive)
    pub t_destroyed: f32,
    /// a particle effect (jet stream / trail) is alive (`this+0xa8 != -1`)
    pub effect_alive: bool,
    pub counts: [BlockCount; 12],
    /// `CScoreCounterJenga+0x3f4`
    pub score: f32,
    pub data_state: u8,
    /// UNRESOLVED: `**(DAT_001828b0 + 0x182808)`: a global switch that makes `CheckGameOverCondition` end the event
    /// after 5 s of game time even if the tower has not settled. Its owner is not named in the decompile (looks like a
    /// debug / fast-forward switch). Default off, which is the normal game.
    pub force_end_flag: bool,
    /// `blocks` handed in by the host for the trait based `update`
    pub blocks: Vec<JengaBlock>,
    pub car_extra: JengaCar,
    /// effects produced by `on_input` (which has no effect list); flushed by the next `update`
    pub pending_effects: Vec<CarEffect>,
}

impl JengaMode {
    /// `CGameModeJenga::CGameModeJenga @ 0018147c` + `InitialiseMode @ 00182e0c` (without the world scan: call
    /// [`Self::initialise_counts`] with the tower blocks afterwards).
    pub fn new(def: JengaEventDef, tower: TowerBounds) -> JengaMode {
        JengaMode {
            def,
            tower,
            tries: TRIES,
            clock: 0.0,
            past_end: false,
            plane1_crossed: false,
            slowmo_left: false,
            scenery_swapped: false,
            t_plane2: -1.0,
            t_destroyed: -1.0,
            effect_alive: false,
            counts: [BlockCount::default(); 12],
            score: 0.0,
            data_state: data_state::RUNNING,
            force_end_flag: false,
            blocks: Vec::new(),
            car_extra: JengaCar::default(),
            pending_effects: Vec::new(),
        }
    }

    /// `InitialiseMode @ 00182e0c`: zero `{total, smashed}` and count the tower objects per type into `total`.
    pub fn initialise_counts(&mut self, blocks: &[JengaBlock]) {
        for c in &mut self.counts {
            c.total = 0;
            c.smashed = 0;
        }
        for b in blocks {
            if let Some(i) = block_index(b.type_id) {
                self.counts[i].total += 1;
            }
        }
    }

    /// `ResetBeforeNextTry @ 00181924` (flags only; the kart reactivation is the `Retry` effect).
    pub fn reset_before_next_try(&mut self) {
        self.past_end = false;
        self.plane1_crossed = false;
        self.slowmo_left = false;
        self.scenery_swapped = false;
        self.t_plane2 = -1.0;
        self.t_destroyed = -1.0;
        self.effect_alive = false;
    }

    /// `CountFallenSmackables @ 00181b28`: recompute `fallen` per type.
    pub fn count_fallen(&mut self, blocks: &[JengaBlock]) {
        for c in &mut self.counts {
            c.fallen = 0;
        }
        for b in blocks {
            if let Some(i) = block_index(b.type_id) {
                if is_fallen(b, &self.tower) {
                    self.counts[i].fallen += 1;
                }
            }
        }
    }

    /// Some pig is neither smashed nor fallen out of the tower (`smashed + fallen < total` for a pig type).
    pub fn pigs_remaining(&self) -> bool {
        PIG_TYPES.iter().any(|&t| {
            let c = &self.counts[block_index(t).unwrap()];
            c.smashed + c.fallen < c.total
        })
    }

    /// `AreSmackablesDone @ 00181e20` (the inverse of [`Self::pigs_remaining`] after a fresh count).
    pub fn are_smackables_done(&mut self, blocks: &[JengaBlock]) -> bool {
        self.count_fallen(blocks);
        !self.pigs_remaining()
    }

    /// `AreSmackablesSteady @ 00182194`: no still-active tower block intersects the tower region.
    pub fn are_smackables_steady(&self, blocks: &[JengaBlock]) -> bool {
        !blocks.iter().any(|b| {
            block_index(b.type_id).is_some() && b.active && self.tower.dist2(b.pos) < b.radius * b.radius
        })
    }

    /// `OnSmashedCallback @ 00180380`: a tower smackable was smashed. Returns the points added.
    pub fn on_block_smashed(&mut self, type_id: i32, effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>, at: Vec3) -> f32 {
        let Some(i) = block_index(type_id) else { return 0.0 };
        self.counts[i].smashed += 1;
        let pts = points_for_type(type_id);
        self.score += pts;
        events.push(ModeEvent::ScoreChanged { car: 0, score: self.score as i64 });
        if PIG_TYPES.contains(&type_id) {
            events.push(ModeEvent::Other { tag: "jenga_pig_hit_voice", value: type_id as f32 });
            effects.push(CarEffect::Particle { name: FX_GREEN_PIG_SCORE_NAME.to_string(), car: None, pos: at });
            effects.push(CarEffect::Sound { name: SND_PIG_POP.to_string(), car: None });
        }
        pts
    }

    /// `OnCarCollision @ 001802b0`: after the kart left the ramp any collision throws the pilot off. Returns true when
    /// the host must call `CCar::DetachPilot`.
    pub fn on_car_collision(&self, pilot_detached: bool) -> bool {
        self.past_end && !pilot_detached
    }

    /// `OnWeaponFire @ 001802f8`: camera shake when a `matildaegg_whole` (type 0x5b) is fired beyond 40 m before the
    /// kart left the ramp.
    pub fn on_weapon_fire(&self, object_type: i32, race_total_dist: f32) -> bool {
        object_type == WEAPON_SHAKE_TYPE && race_total_dist > WEAPON_SHAKE_MIN_DIST && !self.past_end
    }

    /// Smooth launch blend of `Update @ 001809d0` (`DAT_00180f08/0c` = 70 / 57): returns
    /// `(car+0x48c, player+0xf8)` given `car+0x488` and `player+0x234`.
    /// `t = (dist - 70) / 57`; `w = t < 0 ? 0 : t > 1 ? 1 : t*t`;
    /// `car[0x48c] = (1 - w) * car[0x488]`;
    /// `player[0xf8] = w * (a + (1 - a) * 0.5) + (1 - w) * (a + (1 - a) * 1.4)` with `a = player[0x234]`.
    /// UNRESOLVED: the meaning of these four fields (they scale the launch force / camera; names not in the decompile).
    pub fn launch_blend(dist: f32, car_488: f32, player_234: f32) -> (f32, f32) {
        let t = (dist - 70.0) / 57.0;
        let w = if t < 0.0 {
            0.0
        } else if t > 1.0 {
            1.0
        } else {
            t * t
        };
        let a = player_234;
        (
            (1.0 - w) * car_488,
            w * (a + (1.0 - a) * 0.5) + (a + (1.0 - a) * 1.4) * (1.0 - w),
        )
    }

    fn dot_plane(&self, plane: Vec3, p: Vec3) -> f32 {
        (p - plane).dot(self.tower.plane_normal)
    }

    /// Per-frame logic of `CGameModeJenga::Update @ 001809d0` (the parts that are game rules; effects/camera numbers are
    /// emitted as requests).
    pub fn update_car(&mut self, dt: f32, car: &JengaCar, effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>) {
        self.clock += dt;
        if car.spline_end_dist < car.race_total_dist {
            self.past_end = true;
        }
        // launch blend: the original writes `car+0x48c` and `player+0xf8` every frame
        let (c48c, pf8) = Self::launch_blend(car.race_total_dist, car.car_488, car.player_234);
        effects.push(CarEffect::Other { tag: "JengaLaunchBlend", car: Some(0), vals: [c48c, pf8, 0.0, 0.0], pos: Vec3::ZERO });
        // jet stream / trail effect
        if !self.effect_alive && self.t_destroyed == -1.0 {
            let boost = !self.past_end;
            let start = if boost { !car.in_slingshot && car.launch_power > 0.25 } else { true };
            if start {
                self.effect_alive = true;
                events.push(ModeEvent::Message {
                    tag: format!("jenga_effect:{}:{}", FX_JET_STREAM_NAME, if boost { FX_BOOST } else { FX_TRAIL }),
                });
            }
        }
        // plane 1: enter slow motion
        if !self.plane1_crossed && self.dot_plane(self.tower.plane1, car.pos) < 0.0 {
            self.plane1_crossed = true;
            effects.push(slowmo_effect(SLOWMO_ENTER));
        }
        // plane 2: start the settle timer, leave slow motion 0.5 s later
        if self.t_plane2 < 0.0 {
            if self.dot_plane(self.tower.plane2, car.pos) < 0.0 {
                self.t_plane2 = 0.0;
            }
        } else {
            self.t_plane2 += dt;
            if !self.slowmo_left && self.t_plane2 >= SLOWMO_LEAVE_DELAY {
                // UNRESOLVED: with the global switch `force_end_flag` set the original keeps slow motion until the tower
                // settles or 3 s passed; the default (flag clear) path is implemented.
                self.slowmo_left = true;
                effects.push(slowmo_effect(SLOWMO_LEAVE));
            }
        }
        // decor swap beyond 40 m
        if !self.scenery_swapped && SCENERY_SWAP_DIST < car.race_total_dist {
            self.scenery_swapped = true;
            events.push(ModeEvent::Message { tag: "jenga_scenery_swap".to_string() });
        }
        // kart destroyed: leaves the spline corridor before the end of the ramp
        if self.t_destroyed == -1.0 {
            if !self.past_end && car.out_of_corridor {
                self.t_destroyed = 0.0;
                self.effect_alive = false;
                effects.push(CarEffect::Particle { name: FX_KART_DESTRUCTION_NAME.to_string(), car: Some(0), pos: car.pos });
                effects.push(CarEffect::Other { tag: "JengaKartDestroyed", car: Some(0), vals: [0.0; 4], pos: car.pos });
                events.push(ModeEvent::Other { tag: "jenga_player_pop_voice", value: 0.0 });
            }
        } else {
            self.t_destroyed += dt;
        }
        if self.t_destroyed >= 0.0 {
            events.push(ModeEvent::Other { tag: "jenga_player_f8_zero", value: 0.0 });
        }
    }

    /// `CheckGameOverCondition @ 001823e4`. `game_time` is `CGame+0xd4` (only used with `force_end_flag`).
    pub fn check_game_over(
        &mut self,
        car: &JengaCar,
        blocks: &[JengaBlock],
        game_time: f32,
        effects: &mut Vec<CarEffect>,
        events: &mut Vec<ModeEvent>,
    ) -> JengaGameOver {
        // the host polls this every frame (slot 7); once the event is decided keep answering "over" without side effects
        if self.data_state != data_state::RUNNING {
            return JengaGameOver::Over { won: self.data_state == data_state::WON };
        }
        let mut tries_after: Option<i32> = None;
        // comparisons exactly as the compiled flag tests: `t_destroyed <= 1.0` waits, `t_plane2 > 2.0` settles, `game_time > 5.0`
        if self.t_destroyed <= DESTROYED_JUDGE_DELAY {
            if self.t_plane2 == -1.0 && car.pilot_detached && car.pilot_timer < SETTLE_TIME {
                tries_after = Some(self.tries - 1);
            } else if !(self.t_plane2 > SETTLE_TIME) || !self.are_smackables_steady(blocks) {
                if !self.force_end_flag || !(game_time > 5.0) {
                    return JengaGameOver::NotOver;
                }
                tries_after = Some(0);
            } else {
                tries_after = Some(self.tries - 1);
            }
        }
        let tries = tries_after.unwrap_or(self.tries - 1);
        self.tries = tries;
        if tries != 0 {
            self.count_fallen(blocks);
            if self.pigs_remaining() {
                // next launch: respawn the kart (`car+0x1a8c = 0`, `Respawn(-1)`, `ReInit`, camera type 0, `CGame+0x1e0 = 0`)
                self.reset_before_next_try();
                // a stale host-side "pilot detached" flag would burn the next try at once: clear our copy, the host must
                // clear its own on respawn
                self.car_extra.pilot_detached = false;
                self.car_extra.pilot_timer = 0.0;
                effects.push(CarEffect::Other { tag: "JengaRespawnKart", car: Some(0), vals: [self.tries as f32, 0.0, 0.0, 0.0], pos: Vec3::ZERO });
                return JengaGameOver::Retry;
            }
        }
        // finished: judge
        self.count_fallen(blocks);
        let won = !self.pigs_remaining();
        self.data_state = if won { data_state::WON } else { data_state::LOST };
        events.push(ModeEvent::Other { tag: if won { "jenga_win_music" } else { "jenga_lose_music" }, value: 0.0 });
        for (i, c) in self.counts.iter().enumerate() {
            self.score += c.fallen as f32 * points_for_type(BLOCK_TYPES[i]);
        }
        events.push(ModeEvent::Finished { won, score: self.score as i64, stars: if won { stars_from_score(false, self.score as i64, self.def.stars) } else { 0 } });
        JengaGameOver::Over { won }
    }
}

fn slowmo_effect(s: SlowMo) -> CarEffect {
    // CarEffect has no hold/ramp fields: `Other` carries [scale, hold, ramp_in, ramp_out]
    CarEffect::Other { tag: "EnterSlowMo", car: None, vals: [s.scale, s.hold, s.ramp_in, s.ramp_out], pos: Vec3::ZERO }
}

impl GameModeRules for JengaMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::Jenga
    }

    fn update(&mut self, dt: f32, karts: &[KartState], effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>) {
        effects.append(&mut self.pending_effects);
        if let Some(k) = karts.iter().find(|k| k.is_player).or_else(|| karts.first()) {
            let mut car = self.car_extra;
            car.pos = k.pos;
            car.race_total_dist = k.spline_distance;
            self.update_car(dt, &car, effects, events);
        }
    }

    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        match input {
            ModeInput::Smashed { object, .. } => {
                if let Some(t) = type_id_from_name(object) {
                    let mut fx = Vec::new();
                    self.on_block_smashed(t, &mut fx, events, Vec3::ZERO);
                    self.pending_effects.extend(fx);
                }
            }
            ModeInput::Other { tag: "jenga_spline_end_dist", value, .. } => self.car_extra.spline_end_dist = *value,
            ModeInput::Other { tag: "jenga_launch_power", value, .. } => self.car_extra.launch_power = *value,
            ModeInput::Other { tag: "jenga_out_of_corridor", value, .. } => self.car_extra.out_of_corridor = *value != 0.0,
            ModeInput::Other { tag: "jenga_pilot_detached", value, .. } => self.car_extra.pilot_detached = *value != 0.0,
            ModeInput::Other { tag: "jenga_pilot_timer", value, .. } => self.car_extra.pilot_timer = *value,
            _ => {}
        }
    }

    fn state(&self) -> ModeState {
        match self.data_state {
            data_state::WON => ModeState::Won,
            data_state::LOST => ModeState::Lost,
            _ => ModeState::Running,
        }
    }

    fn score(&self) -> i64 {
        self.score as i64
    }

    fn stars(&self) -> u8 {
        if self.state() == ModeState::Won {
            stars_from_score(false, self.score as i64, self.def.stars)
        } else {
            0
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

    fn load(rel: &str) -> Option<String> {
        let p = Path::new(ASSETS292).join(rel);
        if !p.exists() {
            eprintln!("skip: {} missing", p.display());
            return None;
        }
        std::fs::read_to_string(p).ok()
    }

    #[test]
    fn parse_real_jenga_eventdefs() {
        for i in 1..=3 {
            let Some(x) = load(&format!("xml/gameplay/eventdef_other/eventdef_jenga_{i}.xml")) else { return };
            let d = JengaEventDef::from_xml(&x).unwrap();
            assert_eq!(d.stars, [60000, 80000, 90000]);
            assert!(d.environment.contains("theme002"));
            assert!(d.environment.ends_with(["run002", "run004", "run005"][i - 1]));
            assert_eq!(d.character, "red");
        }
        assert!(JengaEventDef::from_xml("<EventDefinition><GameMode name=\"RACE\"/></EventDefinition>").is_err());
    }

    #[test]
    fn episode_config_lists_jenga_events() {
        let Some(x) = load("xml/gameplay/misc/episode_config.xml") else { return };
        for (n, f) in [(0, "EventDef_Jenga_1.xml"), (1, "EventDef_Jenga_2.xml"), (2, "EventDef_Jenga_3.xml")] {
            assert!(x.contains(&format!("episode_jenga_0{n}")));
            assert!(x.contains(f));
        }
    }

    #[test]
    fn points_table_matches_binary_dump() {
        // float[0x27] at 0xcec978: 500 500 500 8000 | 0 5000 5000 0 | 0 x24 | 0 1000 1000 1000 | 1000 1000 1000
        let mut table = [0.0f32; 0x27];
        for t in 0x2d..0x2d + 0x27 {
            table[(t - 0x2d) as usize] = points_for_type(t);
        }
        assert_eq!(&table[..8], &[500.0, 500.0, 500.0, 8000.0, 0.0, 5000.0, 5000.0, 0.0]);
        assert!(table[8..32].iter().all(|&v| v == 0.0));
        assert_eq!(&table[32..], &[0.0, 1000.0, 1000.0, 1000.0, 1000.0, 1000.0, 1000.0]);
        assert_eq!(points_for_type(0x7d), 0.0);
        assert_eq!(points_for_type(0x2c), 0.0);
    }

    #[test]
    fn type_names_and_indices() {
        for (i, &t) in BLOCK_TYPES.iter().enumerate() {
            assert_eq!(block_index(t), Some(i));
            assert_eq!(type_id_from_name(BLOCK_TYPE_NAMES[i]), Some(t));
        }
        assert_eq!(type_id_from_name("smck_piratepig1_3m.xgm"), Some(0x32));
        assert_eq!(type_id_from_name("smck_tnt_box_3m"), Some(0x30));
        assert_eq!(type_id_from_name("nonsense"), None);
    }

    fn tower() -> TowerBounds {
        TowerBounds::from_helpers(&[
            ("trophy_helper_01", Vec3::new(-10.0, 0.0, -100.0)),
            ("junk", Vec3::new(500.0, 500.0, 500.0)),
            ("Trophy_Helper_02", Vec3::new(10.0, 30.0, -80.0)),
        ])
    }

    #[test]
    fn tower_bounds_from_helpers() {
        let t = tower();
        assert_eq!(t.min, Vec3::new(-10.0, 0.0, -100.0));
        assert_eq!(t.max, Vec3::new(10.0, 30.0, -80.0));
        assert_eq!(t.centre, Vec3::new(0.0, 15.0, -90.0));
        assert_eq!(t.half, Vec3::new(10.0, 15.0, 10.0));
        assert_eq!(t.plane1.z, -75.0);
        assert_eq!(t.plane2.z, -90.0);
        assert_eq!(t.dist2(Vec3::new(0.0, 10.0, -90.0)), 0.0);
        assert_eq!(t.dist2(Vec3::new(13.0, 10.0, -90.0)), 9.0);
    }

    fn blocks_with_pigs(n_pigs: usize, fallen: usize) -> Vec<JengaBlock> {
        let t = tower();
        let mut v = Vec::new();
        for i in 0..n_pigs {
            let pos = if i < fallen { Vec3::new(100.0, 0.0, -90.0) } else { t.centre };
            v.push(JengaBlock { type_id: 0x32, pos, radius: 2.0, active: false });
        }
        v.push(JengaBlock { type_id: 0x2e, pos: t.centre, radius: 2.0, active: false });
        v.push(JengaBlock { type_id: 0x30, pos: Vec3::new(0.0, 0.0, 200.0), radius: 2.0, active: false });
        v
    }

    fn mode() -> JengaMode {
        let def = JengaEventDef { environment: "x".into(), character: "red".into(), difficulty_level: 0.1, stars: [60000, 80000, 90000] };
        JengaMode::new(def, tower())
    }

    #[test]
    fn fallen_and_totals() {
        let mut m = mode();
        let b = blocks_with_pigs(3, 1);
        m.initialise_counts(&b);
        assert_eq!(m.counts[block_index(0x32).unwrap()].total, 3);
        assert_eq!(m.counts[block_index(0x2e).unwrap()].total, 1);
        m.count_fallen(&b);
        assert_eq!(m.counts[block_index(0x32).unwrap()].fallen, 1);
        assert_eq!(m.counts[block_index(0x30).unwrap()].fallen, 1);
        assert!(m.pigs_remaining());
    }

    #[test]
    fn smash_scoring() {
        let mut m = mode();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        assert_eq!(m.on_block_smashed(0x2e, &mut fx, &mut ev, Vec3::ZERO), 500.0);
        assert_eq!(m.on_block_smashed(0x32, &mut fx, &mut ev, Vec3::ZERO), 5000.0);
        assert_eq!(m.on_block_smashed(0x30, &mut fx, &mut ev, Vec3::ZERO), 8000.0);
        assert_eq!(m.on_block_smashed(0x7d, &mut fx, &mut ev, Vec3::ZERO), 0.0); // not a tower type
        assert_eq!(m.score, 13500.0);
        assert_eq!(fx.len(), 2); // only the pig spawns effect + sound
        assert_eq!(m.counts[block_index(0x32).unwrap()].smashed, 1);
    }

    #[test]
    fn win_when_all_pigs_gone_and_final_scoring() {
        let mut m = mode();
        let b = blocks_with_pigs(2, 2); // both pigs fell out
        m.initialise_counts(&b);
        m.tries = 3;
        m.t_plane2 = 2.5;
        let car = JengaCar::default();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let r = m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev);
        assert_eq!(r, JengaGameOver::Over { won: true });
        assert_eq!(m.state(), ModeState::Won);
        // fallen: 2 pigs (5000) + TNT box (8000, outside the tower) = 18000; glass cube still in the tower
        assert_eq!(m.score(), 18000);
        assert_eq!(m.stars(), 1);
    }

    #[test]
    fn retry_then_lose_when_tries_exhausted() {
        let mut m = mode();
        let b = blocks_with_pigs(2, 0);
        m.initialise_counts(&b);
        m.t_plane2 = 2.5;
        let car = JengaCar::default();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        for expect in [3, 2, 1] {
            let r = m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev);
            assert_eq!(r, JengaGameOver::Retry);
            assert_eq!(m.tries, expect);
            assert_eq!(m.t_plane2, -1.0); // flags reset for the next launch
            m.t_plane2 = 2.5;
        }
        let r = m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev);
        assert_eq!(r, JengaGameOver::Over { won: false });
        assert_eq!(m.state(), ModeState::Lost);
        assert_eq!(m.stars(), 0);
    }

    #[test]
    fn not_over_while_unsettled() {
        let mut m = mode();
        let mut b = blocks_with_pigs(1, 0);
        b[0].active = true; // moving block inside the tower
        m.initialise_counts(&b);
        let car = JengaCar::default();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.t_plane2 = 1.0;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::NotOver);
        m.t_plane2 = 2.5; // timer done but the block is still moving inside the box
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::NotOver);
        b[0].active = false;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Retry);
    }

    #[test]
    fn pilot_detach_and_kart_destroyed_end_a_try() {
        let mut m = mode();
        let b = blocks_with_pigs(1, 0);
        m.initialise_counts(&b);
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        // pilot thrown off before plane 2 and detach timer < 2 -> try ends immediately
        let car = JengaCar { pilot_detached: true, pilot_timer: 0.5, ..Default::default() };
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Retry);
        assert_eq!(m.tries, 3);
        // kart destroyed >= 1 s ago -> judged regardless of settling
        m.t_destroyed = 1.2;
        assert_eq!(m.check_game_over(&JengaCar::default(), &b, 0.0, &mut fx, &mut ev), JengaGameOver::Retry);
        assert_eq!(m.tries, 2);
    }

    #[test]
    fn update_planes_and_slowmo() {
        let mut m = mode();
        let t = m.tower;
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let far = JengaCar { pos: Vec3::new(0.0, 5.0, t.min.z + 40.0), race_total_dist: 10.0, spline_end_dist: 100.0, ..Default::default() };
        m.update_car(0.1, &far, &mut fx, &mut ev);
        assert!(!m.plane1_crossed);
        let p1 = JengaCar { pos: Vec3::new(0.0, 5.0, t.min.z + 24.0), race_total_dist: 50.0, ..far };
        m.update_car(0.1, &p1, &mut fx, &mut ev);
        assert!(m.plane1_crossed);
        let slow: Vec<_> = fx.iter().filter(|e| matches!(e, CarEffect::Other { tag: "EnterSlowMo", .. })).cloned().collect();
        assert!(matches!(slow[0], CarEffect::Other { vals, .. } if vals[0] == 0.35 && vals[1].is_infinite() && vals[2] == 0.1));
        let p2 = JengaCar { pos: Vec3::new(0.0, 5.0, t.min.z + 9.0), race_total_dist: 120.0, spline_end_dist: 100.0, ..far };
        m.update_car(0.1, &p2, &mut fx, &mut ev);
        assert!(m.past_end && m.scenery_swapped);
        assert_eq!(m.t_plane2, 0.0);
        for _ in 0..5 {
            m.update_car(0.1, &p2, &mut fx, &mut ev);
        }
        assert!(m.slowmo_left);
        let slow: Vec<_> = fx.iter().filter(|e| matches!(e, CarEffect::Other { tag: "EnterSlowMo", .. })).cloned().collect();
        assert_eq!(slow.len(), 2);
        assert!(matches!(slow[1], CarEffect::Other { vals, .. } if vals[0] == 1.0));
        // collision after the end of the ramp detaches the pilot
        assert!(m.on_car_collision(false));
        assert!(!m.on_car_collision(true));
    }

    #[test]
    fn kart_destroyed_when_leaving_corridor() {
        let mut m = mode();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let car = JengaCar { out_of_corridor: true, race_total_dist: 10.0, spline_end_dist: 100.0, pos: Vec3::new(0.0, 0.0, 50.0), ..Default::default() };
        m.update_car(0.1, &car, &mut fx, &mut ev);
        assert_eq!(m.t_destroyed, 0.0);
        m.update_car(0.25, &car, &mut fx, &mut ev);
        assert!((m.t_destroyed - 0.25).abs() < 1e-6);
    }

    #[test]
    fn launch_blend_endpoints() {
        assert_eq!(JengaMode::launch_blend(0.0, 2.0, 0.5), (2.0, 0.5 + 0.5 * 1.4));
        let (a, b) = JengaMode::launch_blend(200.0, 2.0, 0.5);
        assert_eq!(a, 0.0);
        assert!((b - (0.5 + 0.25)).abs() < 1e-6);
    }

    #[test]
    fn weapon_fire_shake_rule() {
        let m = mode();
        assert!(m.on_weapon_fire(0x5b, 41.0));
        assert!(!m.on_weapon_fire(0x5b, 39.0));
        assert!(!m.on_weapon_fire(0x30, 100.0));
    }

    #[test]
    fn stars_thresholds() {
        let s = [60000, 80000, 90000];
        assert_eq!(stars_from_score(false, 60000, s), 1);
        assert_eq!(stars_from_score(false, 80001, s), 2);
        assert_eq!(stars_from_score(false, 90001, s), 3);
    }

    #[test]
    fn check_game_over_is_idempotent_after_the_event_ended() {
        let mut m = mode();
        let b = blocks_with_pigs(2, 2);
        m.initialise_counts(&b);
        m.t_plane2 = 2.5;
        let car = JengaCar::default();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let r1 = m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev);
        let (tries, score, nev) = (m.tries, m.score, ev.len());
        let r2 = m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev);
        let r3 = m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev);
        assert_eq!(r1, JengaGameOver::Over { won: true });
        assert_eq!(r2, r1);
        assert_eq!(r3, r1);
        assert_eq!((m.tries, m.score, ev.len()), (tries, score, nev));
        // a lost event keeps answering "over, lost"
        let mut l = mode();
        let b = blocks_with_pigs(2, 0);
        l.initialise_counts(&b);
        l.tries = 1;
        l.t_plane2 = 2.5;
        assert_eq!(l.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Over { won: false });
        assert_eq!(l.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Over { won: false });
        assert_eq!(l.tries, 0);
    }

    #[test]
    fn boundary_comparisons_match_flag_tests() {
        let mut m = mode();
        let b = blocks_with_pigs(1, 0);
        m.initialise_counts(&b);
        let car = JengaCar::default();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        // settle needs t_plane2 strictly > 2.0
        m.t_plane2 = 2.0;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::NotOver);
        m.t_plane2 = 2.0001;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Retry);
        // destroyed: exactly 1.0 still waits (needs the settle path), > 1.0 judges
        let mut m = mode();
        m.initialise_counts(&b);
        m.t_destroyed = 1.0;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::NotOver);
        m.t_destroyed = 1.0001;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Retry);
        // force-end switch: needs game_time strictly > 5.0
        let mut m = mode();
        m.initialise_counts(&b);
        m.force_end_flag = true;
        assert_eq!(m.check_game_over(&car, &b, 5.0, &mut fx, &mut ev), JengaGameOver::NotOver);
        assert_eq!(m.check_game_over(&car, &b, 5.5, &mut fx, &mut ev), JengaGameOver::Over { won: false });
        assert_eq!(m.tries, 0);
    }

    #[test]
    fn trait_update_path_does_not_think_the_kart_is_past_the_ramp() {
        let mut m = mode();
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let k = KartState { is_player: true, spline_distance: 12.0, pos: Vec3::new(0.0, 0.0, 500.0), ..Default::default() };
        for _ in 0..3 {
            m.update(0.1, &[k.clone()], &mut fx, &mut ev);
        }
        assert!(!m.past_end);
        assert!(!m.on_car_collision(false));
        // once the host reports the end of the spline the flag flips
        m.on_input(&ModeInput::Other { tag: "jenga_spline_end_dist", car: None, value: 10.0 }, &mut ev);
        m.update(0.1, &[k], &mut fx, &mut ev);
        assert!(m.past_end);
        assert!(m.on_car_collision(false));
        // every frame the launch blend request goes out
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Other { tag: "JengaLaunchBlend", .. })));
    }

    #[test]
    fn trait_smash_effects_are_queued_until_next_update() {
        let mut m = mode();
        let mut ev = Vec::new();
        m.on_input(&ModeInput::Smashed { car: 0, object: "smck_PiratePig1_3m".into() }, &mut ev);
        assert_eq!(m.score(), 5000);
        assert_eq!(m.pending_effects.len(), 2);
        let mut fx = Vec::new();
        let k = KartState { is_player: true, ..Default::default() };
        m.update(0.1, &[k], &mut fx, &mut ev);
        assert!(m.pending_effects.is_empty());
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Sound { name, .. } if name == SND_PIG_POP)));
    }

    #[test]
    fn retry_clears_stale_pilot_flag() {
        let mut m = mode();
        let b = blocks_with_pigs(1, 0);
        m.initialise_counts(&b);
        m.car_extra.pilot_detached = true;
        m.car_extra.pilot_timer = 0.5;
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let car = m.car_extra;
        assert_eq!(m.check_game_over(&car, &b, 0.0, &mut fx, &mut ev), JengaGameOver::Retry);
        assert!(!m.car_extra.pilot_detached);
    }
}
