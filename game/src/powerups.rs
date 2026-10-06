//! Shop power-ups, the in-race power-up effects, pickups, score counters and in-race challenges of Angry Birds Go! 2.9.1.
//!
//! Everything here is a plain-data port of the original C++ (`libABK291.so`, Ghidra export `libABK291_annotated.c`;
//! every function documents its original symbol + address). Standalone: depends only on `std`, `glam` and `roxmltree`;
//! the host (race / car sim) feeds the structs below and applies the returned [`CarEffect`] requests.
//!
//! Layout of this file
//! 1. [`PlayerPowerups`] -- `CPlayerInfo` power-up state machine (counts, chosen flags, error codes)
//! 2. [`DebugTweakables`] / [`GameplayTweakables`] -- the XML values the power-up code reads (+ dead data report)
//! 3. [`BodyworkDamage`] / [`AutoRepair`] -- `CCar::UpdatePowerups`
//! 4. [`PowerupVfx`] / [`PowerupFrame`] -- `CCar::IntegratePowerups` (+ SpeedBooster push input, KingSling, TargetCar)
//! 5. Pickups -- `CPickup*` classes, boost pad, coin / seed / fruit / gem values
//! 6. Score counters -- `CScoreCounter*` / `CScoreSystem`
//! 7. Challenges -- `CChallenge*` (parsed from `challenges.xml`, driven by [`ChallengeEvent`])
//!
//! Host contract / needs
//! * `CarEffect::Particle { name }` names carry `"<effect>/<instance>"` (`FindEffect(effect)` + `SpawnEffect(.., instance)`),
//!   e.g. `AutoRepair/RepairEffect`, `SpeedBoost/SpeedBoostEffect` (`SpeedBoostBranded/..` for sponsor campaigns). Repair
//!   bodywork re-appear bursts use the piece's own effect name.
//! * `CarEffect::Other` tags emitted (no dedicated variant exists): `RemoveSmackable` (vals[0] = piece index),
//!   `VoiceOnPowerUp` (vals[0] = character byte), `RemoveParticle:RepairEffect`, `RemoveParticle:SpeedBoostEffect`.
//! * `main.rs` must declare `mod modes;` (with `modes/mod.rs` containing `pub mod types;`) and `mod powerups;`.
//! * Challenge classes `AvoidObstacles`, `NearlyMissObstacles`, `TravelDistance`, `BeatScore`, `SpinAndWin` are parsed
//!   but not ported (see [`ChUnported`]); `Challenge::is_completed` returns `None` for them.
//! * Not ported on purpose: the in-race power-up popup (`GameUI::CPopupManager::PopupInGamePowerup @003dcb08`) is a
//!   rewarded-video offer, `smck_randompower_box.xgm` / `random_power_box.xgt` are only referenced as asset names (no
//!   decompiled code path), `CGame::SetupEnvironmentMarkup @0011db98` replaces some `pickup_coin` helpers with
//!   `pickup_megacoin` / `pickup_giftbox` / `pickup_gem` using the economy.xml `ProbOf*InRace` probabilities (metagame).
//!
//! Debug-tweakable indices: `CDebugManager::GetDebugFloat(i)` reads `float[i]` at Ghidra `0x00de0af8 + 4*i`,
//! `GetDebugInt(i)` reads `int[i]` at `0x00de0570 + 4*i` (`@002c6740` / `@002c672c`). The store address of every XML
//! key in `SetDebugTweakablesFromXML @002c7258` therefore gives its index (done with a script over the whole table;
//! e.g. `Boost_Push_Factor` -> 0x57 agrees with `carsim.rs`).
#![allow(dead_code)]

use crate::modes::types::*;

// ============================================================================================================
// 1. CPlayerInfo power-ups
// ============================================================================================================

/// `Type::EPowerup::Enum` (`GetPowerUpName @0014923c`: 0 `POWERUP_KINGSLING`, 1 `POWERUP_AUTOREPAIR`,
/// 2 `POWERUP_LEAFBLOWER` (the SpeedBooster; asset key `Powerup_SpeedBooster`), 3 `POWERUP_TARGETCAR` (asset key
/// `Powerup_PartnerCar`, the sponsored one: `IsSponsoredPowerUp @001492ec` == `idx == 3`)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EPowerup {
    KingSling = 0,
    AutoRepair = 1,
    SpeedBooster = 2,
    TargetCar = 3,
}
pub const NUM_POWERUPS: usize = 4;

impl EPowerup {
    /// Index -> enum (`< 4`, the bound every `CPlayerInfo` accessor tests).
    pub fn from_index(i: i32) -> Option<EPowerup> {
        match i {
            0 => Some(EPowerup::KingSling),
            1 => Some(EPowerup::AutoRepair),
            2 => Some(EPowerup::SpeedBooster),
            3 => Some(EPowerup::TargetCar),
            _ => None,
        }
    }
}

/// `GetPowerUpName(Type::EPowerup::Enum) @0014923c`: the *localisation key* handed to `CLoc::String`.
/// Out of range returns the empty string (`&DAT_00149294 + DAT_001492a4`, an empty literal).
pub fn power_up_loc_key(p: i32) -> &'static str {
    match p {
        0 => "POWERUP_KINGSLING",
        1 => "POWERUP_AUTOREPAIR",
        2 => "POWERUP_LEAFBLOWER",
        3 => "POWERUP_TARGETCAR",
        _ => "",
    }
}

/// `IsSponsoredPowerUp(Type::EPowerup::Enum) @001492ec`.
pub fn is_sponsored_power_up(p: i32) -> bool {
    p == 3
}

/// `CPlayerInfo::GetPowerupStringByEnum(EPowerup, char*) @0014d0e8`: asset key per power-up. Out of range leaves the
/// buffer untouched (here: `None`).
pub fn powerup_string_by_enum(p: i32) -> Option<&'static str> {
    match p {
        0 => Some("Powerup_KingSling"),
        1 => Some("Powerup_AutoRepair"),
        2 => Some("Powerup_SpeedBooster"),
        3 => Some("Powerup_PartnerCar"),
        _ => None,
    }
}

/// `CPlayerInfo::GetPowerupEnumByText(char const*) @0014d07c` (case-insensitive `strcasecmp`). NOTE the original has
/// no entry for the 4th power-up: "TargetCar" / "PartnerCar" returns -1.
pub fn powerup_enum_by_text(s: &str) -> i32 {
    if s.eq_ignore_ascii_case("KingSling") {
        0
    } else if s.eq_ignore_ascii_case("AutoRepair") {
        1
    } else if s.eq_ignore_ascii_case("SpeedBooster") {
        2
    } else {
        -1
    }
}

/// `CPlayerInfo::GetPowerupEnumByName(UNameTag) @0014cfe8`: compares the 4 byte `UNameTag` with four global tags
/// (`strncmp(tag_i, name, 4)` in order 0..3, -1 when none matches). The four tag strings sit behind GOT slots
/// (`DAT_0014d078 + 0x14d004 ..+0x14d010`) that are filled by the dynamic loader, so the text of the tags is not in the
/// file. UNRESOLVED: the caller must supply the tags (`tags[i]` = the 4 bytes the original compares against).
pub fn powerup_enum_by_name(tag: [u8; 4], tags: &[[u8; 4]; 4]) -> i32 {
    for (i, t) in tags.iter().enumerate() {
        if *t == tag {
            return i as i32;
        }
    }
    -1
}

/// Return codes of `CPlayerInfo::SetPowerUpActive @0014c4fc` (`undefined4`; 1 on success).
pub mod set_active_err {
    /// `0xfffffff9`: `idx >= 4`.
    pub const BAD_INDEX: i32 = -7;
    /// `0xfffffff8`: this slot is already chosen.
    pub const ALREADY_ACTIVE: i32 = -8;
    /// `0xfffffff6`: none in stock (`count < 1`).
    pub const NONE_IN_STOCK: i32 = -10;
    /// `0xfffffff7`: "all four chosen". UNREACHABLE in the original (needs every slot chosen but then the requested
    /// slot is already chosen and `ALREADY_ACTIVE` returns first); kept literally.
    pub const TOO_MANY: i32 = -9;
}

/// The power-up block of `CPlayerInfo`: `int count[4]` at +0x3b8, `int total_gained[4]` at +0x3c8 (lifetime counter
/// bumped by `AddPowerupCharges`), `int chosen[4]` at +0x3d8 (what `IsPowerUpActive` returns), `float duration[4]`
/// at +0x3e8 and the "has ever owned a power-up" flag at +0xa98.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerPowerups {
    pub count: [i32; NUM_POWERUPS],
    pub total_gained: [i32; NUM_POWERUPS],
    pub chosen: [bool; NUM_POWERUPS],
    /// `SetPowerUpActiveForDuration`'s timer (+0x3e8). Dead in 2.9.1: nothing calls the setter and nothing ticks it.
    pub duration: [f32; NUM_POWERUPS],
    pub has_powerups_flag: bool,
}

impl PlayerPowerups {
    /// `CPlayerInfo::GetPowerupCountByIndex @0014c2a8` (`idx < 4` signed compare, else 0).
    pub fn count_by_index(&self, idx: i32) -> i32 {
        if (0..4).contains(&idx) {
            self.count[idx as usize]
        } else {
            0
        }
    }

    /// `CPlayerInfo::GetPowerupChosenByIndex @0014c2bc`.
    pub fn chosen_by_index(&self, idx: i32) -> bool {
        (0..4).contains(&idx) && self.chosen[idx as usize]
    }

    /// `CPlayerInfo::AddPowerupCharges(EPowerup, int, ETypeOfGain, ...) @0014c2d0`. The analytics arguments are dropped.
    pub fn add_charges(&mut self, idx: i32, amount: i32) {
        if !self.has_powerups_flag {
            self.has_powerups_flag = true;
        }
        if (0..4).contains(&idx) {
            self.count[idx as usize] += amount;
            self.total_gained[idx as usize] += amount;
        }
    }

    /// `CPlayerInfo::IsPowerUpActive @0014c598` (the chosen flag; no bounds check in the original).
    pub fn is_active(&self, idx: i32) -> bool {
        self.chosen_by_index(idx)
    }

    /// `CPlayerInfo::UnSetPowerUp @0014c4ac`.
    pub fn unset(&mut self, idx: i32) {
        if (0..4).contains(&idx) {
            self.chosen[idx as usize] = false;
        }
    }

    /// `CPlayerInfo::SetPowerUpActiveForDuration(EPowerup, float) @0014c4bc`. No caller exists in 2.9.1 (dead API).
    pub fn set_active_for_duration(&mut self, idx: usize, seconds: f32) {
        let cur = self.duration[idx];
        if self.chosen[idx] && cur <= 0.0 {
            return;
        }
        self.chosen[idx] = true;
        self.duration[idx] = if cur < seconds { seconds } else { cur };
    }

    /// `CPlayerInfo::SetPowerUpActive(EPowerup, int force) @0014c4fc`. Returns 1 or one of [`set_active_err`].
    /// `force == false` runs the stock / duplicate checks; `true` skips them (used for the sponsored boost).
    pub fn set_active(&mut self, idx: i32, force: bool) -> i32 {
        if idx >= 4 || idx < 0 {
            return set_active_err::BAD_INDEX;
        }
        let i = idx as usize;
        if !force {
            if self.chosen[i] {
                return set_active_err::ALREADY_ACTIVE;
            }
            if self.count[i] < 1 {
                return set_active_err::NONE_IN_STOCK;
            }
            let mut n = self.chosen[0] as u8;
            if self.chosen[1] {
                n += 1;
            }
            if self.chosen[2] {
                n += 1;
            }
            if self.chosen[3] && n == 3 {
                return set_active_err::TOO_MANY;
            }
        }
        self.chosen[i] = true;
        1
    }

    /// `CPlayerInfo::ClearSelectedPowerUps @0014c5a4`. (Ghidra shows the last three stores as reads of a zeroed vector
    /// register: all four flags are cleared.)
    pub fn clear_selected(&mut self) {
        self.chosen = [false; NUM_POWERUPS];
    }

    /// `CPlayerInfo::ConsumePowerUp(EPowerup) [clone .part.71] @00149134`: take one charge. 0 when none left.
    fn consume_inner(&mut self, idx: usize) -> i32 {
        if self.count[idx] < 1 {
            return 0;
        }
        self.count[idx] -= 1;
        1
    }

    /// `CPlayerInfo::ConsumePowerUp(EPowerup) @0014c448`. `campaign_active` = `CInGameAdManager::IsCampaignActive`:
    /// while a sponsor campaign is on, the sponsored (index 3) power-up is free (returns 1 without consuming).
    pub fn consume(&mut self, idx: i32, campaign_active: bool) -> i32 {
        if idx > 3 || idx < 0 {
            return 0;
        }
        if idx != 3 || !campaign_active {
            return self.consume_inner(idx as usize);
        }
        1
    }

    /// `CPlayerInfo::ConsumeAllSelectedPowerUps @0014c308`: called when the slingshot has been fired. Consumes one
    /// charge of every chosen power-up in order 0,1,2,3; returns 0 as soon as one chosen power-up cannot be paid
    /// (earlier ones stay consumed), else 1. The campaign check only applies to index 3 (for 0..2 the original
    /// calls `IsCampaignActive` and ignores the answer).
    pub fn consume_all_selected(&mut self, campaign_active: bool) -> i32 {
        let mut r = 1;
        for i in 0..3usize {
            if self.chosen[i] {
                r = self.consume_inner(i);
                if r == 0 {
                    return 0;
                }
            }
        }
        if self.chosen[3] {
            if !campaign_active {
                r = self.consume_inner(3);
                if r == 0 {
                    r = 0;
                }
            } else {
                r = 1;
            }
        }
        r
    }

    /// The block in the in-game screen's game-begin handler (`@0030b7c4..0030c0b4`, `CXGSFE_InGameScreen`): with a
    /// sponsor campaign running and ad feature 6 active the SpeedBooster is switched on for free via
    /// `SetPowerUpActive(2, 1)` (force). Returns true when it was newly activated (the caller then plays the
    /// power-up UI sounds `OnPowerUpSelected(2)`, `OnEvent(0x26)` and bumps the `UsePowerUp` achievement counter).
    pub fn sponsored_speedbooster_on_game_begin(&mut self, game_32cc_set: bool, campaign_active: bool, ad_feature_6: bool) -> bool {
        if game_32cc_set && campaign_active && ad_feature_6 {
            return self.set_active(2, true) == 1;
        }
        false
    }
}

/// TargetCar ("partner car") power-up: `CGame::CreatePowerupCar @0011beac` builds a second car from the episode's
/// partner-car name (`EpisodeDefinition+0x48`) with a level fraction `(character_level - 1) / 11.0`
/// (`local_11c`; `GetDebugFloat(0x48)` overrides it when `>= 0`, UNRESOLVED name of float 0x48 -- it is the store at
/// the `0x48` slot which `SetDebugTweakablesFromXML` fills from no key, i.e. stays at its default), then
/// `CGame::SetCarAsPowerupCar @0011bbdc` swaps the player's car for it when the pre-race power-up button for index 3
/// finishes its animation (`SetupChosenPowerups @00357fcc`, `iVar5 == 3`).
pub fn partner_car_level_fraction(character_level: i32) -> f32 {
    (character_level - 1) as f32 / 11.0
}

// ============================================================================================================
// 2. XML values
// ============================================================================================================

/// The `GetDebugFloat/GetDebugInt` values the power-up, boost and KingSling code reads
/// (`GMISC:DebugTweakables.xml`; keys and indices resolved from `SetDebugTweakablesFromXML @002c7258`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugTweakables {
    /// float 0x51 `Autorepair_Start_Delay`: seconds of accumulated "damage monitoring" before the piece count is evaluated.
    pub autorepair_start_delay: f32,
    /// float 0x52 `Autorepair_VFX_Delay`: seconds between the repair trigger and the actual repair.
    pub autorepair_vfx_delay: f32,
    /// int 6 `Autorepair_Damaged_Pieces`: repair triggers when the damaged-piece count is strictly greater.
    pub autorepair_damaged_pieces: i32,
    /// float 0x53/0x54/0x55 `Drift_Angle_Start`, `Drift_Angle_Stop`, `Drift_Speed_Start` (car drift detection, `carsim.rs`).
    pub drift_angle_start: f32,
    pub drift_angle_stop: f32,
    pub drift_speed_start: f32,
    /// float 0x57 `Boost_Push_Factor` (SpeedBooster push, `CCar::Integrate`).
    pub boost_push_factor: f32,
    /// float 0x58..0x5b `Boost_Camera_Max_Distance / Start_Speed / End_Speed / Start_Delay` (`CCar::GetCamBehindMod`).
    pub boost_camera_max_distance: f32,
    pub boost_camera_start_speed: f32,
    pub boost_camera_end_speed: f32,
    pub boost_camera_start_delay: f32,
    /// float 0x5c `King_Sling_Force_Multiplier`: launch velocity multiplier with KingSling (`CCar::GetLaunchVelocity @00199c04`).
    pub king_sling_force_multiplier: f32,
    /// float 0x5d `King_Sling_Catchup_Disabled_Time`: floor of the catch-up-disabled timer (`game+0x3298`) with KingSling.
    pub king_sling_catchup_disabled_time: f32,
}

impl Default for DebugTweakables {
    /// Interface/helper (no original function): the shipped `debugtweakables.xml` `<Powerup>` section values.
    fn default() -> Self {
        DebugTweakables {
            autorepair_start_delay: 2.0,
            autorepair_vfx_delay: 1.0,
            autorepair_damaged_pieces: 1,
            drift_angle_start: 0.2,
            drift_angle_stop: 0.2,
            drift_speed_start: 15.0,
            boost_push_factor: 4.0,
            boost_camera_max_distance: 1.0,
            boost_camera_start_speed: 1.0,
            boost_camera_end_speed: 1.01,
            boost_camera_start_delay: 0.0,
            king_sling_force_multiplier: 1.4024,
            king_sling_catchup_disabled_time: 10.0,
        }
    }
}

fn xml_f32(doc: &roxmltree::Document, tag: &str) -> Option<f32> {
    doc.descendants().find(|n| n.has_tag_name(tag)).and_then(|n| n.text()).and_then(|t| t.trim().parse::<f32>().ok())
}
fn xml_i32(doc: &roxmltree::Document, tag: &str) -> Option<i32> {
    doc.descendants().find(|n| n.has_tag_name(tag)).and_then(|n| n.text()).and_then(|t| t.trim().parse::<i32>().ok())
}

fn sec_f32(sec: &roxmltree::Node, tag: &str) -> Option<f32> {
    sec.children().find(|n| n.is_element() && n.has_tag_name(tag)).and_then(|n| n.text()).and_then(|t| t.trim().parse::<f32>().ok())
}
fn sec_i32(sec: &roxmltree::Node, tag: &str) -> Option<i32> {
    sec.children().find(|n| n.is_element() && n.has_tag_name(tag)).and_then(|n| n.text()).and_then(|t| t.trim().parse::<i32>().ok())
}

impl DebugTweakables {
    /// Subset of `CDebugManager::SetDebugTweakablesFromXML(char const*) @002c7258` (file `GMISC:DebugTweakables.xml`,
    /// i.e. `assets292/xml/gameplay/misc/debugtweakables.xml`). All of these keys live in the `<Powerup>` section (the loader
    /// reads them from that child node; the file also has a stray `Boost_Camera_Start_Delay` under `<Gameplay>` that nothing
    /// reads). Missing keys keep their [`Default`].
    pub fn from_xml(xml: &str) -> Option<DebugTweakables> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let sec = doc.descendants().find(|n| n.is_element() && n.has_tag_name("Powerup"))?;
        let d = DebugTweakables::default();
        Some(DebugTweakables {
            autorepair_start_delay: sec_f32(&sec, "Autorepair_Start_Delay").unwrap_or(d.autorepair_start_delay),
            autorepair_vfx_delay: sec_f32(&sec, "Autorepair_VFX_Delay").unwrap_or(d.autorepair_vfx_delay),
            autorepair_damaged_pieces: sec_i32(&sec, "Autorepair_Damaged_Pieces").unwrap_or(d.autorepair_damaged_pieces),
            drift_angle_start: sec_f32(&sec, "Drift_Angle_Start").unwrap_or(d.drift_angle_start),
            drift_angle_stop: sec_f32(&sec, "Drift_Angle_Stop").unwrap_or(d.drift_angle_stop),
            drift_speed_start: sec_f32(&sec, "Drift_Speed_Start").unwrap_or(d.drift_speed_start),
            boost_push_factor: sec_f32(&sec, "Boost_Push_Factor").unwrap_or(d.boost_push_factor),
            boost_camera_max_distance: sec_f32(&sec, "Boost_Camera_Max_Distance").unwrap_or(d.boost_camera_max_distance),
            boost_camera_start_speed: sec_f32(&sec, "Boost_Camera_Start_Speed").unwrap_or(d.boost_camera_start_speed),
            boost_camera_end_speed: sec_f32(&sec, "Boost_Camera_End_Speed").unwrap_or(d.boost_camera_end_speed),
            boost_camera_start_delay: sec_f32(&sec, "Boost_Camera_Start_Delay").unwrap_or(d.boost_camera_start_delay),
            king_sling_force_multiplier: sec_f32(&sec, "King_Sling_Force_Multiplier").unwrap_or(d.king_sling_force_multiplier),
            king_sling_catchup_disabled_time: sec_f32(&sec, "King_Sling_Catchup_Disabled_Time").unwrap_or(d.king_sling_catchup_disabled_time),
        })
    }

    /// Reads `<ASSETS292>/xml/gameplay/misc/debugtweakables.xml`.
    pub fn load() -> Option<DebugTweakables> {
        let p = format!("{}/xml/gameplay/misc/debugtweakables.xml", ASSETS292);
        DebugTweakables::from_xml(&std::fs::read_to_string(p).ok()?)
    }
}

/// `CGameplayTweakables::Load() @001ef690`: the only values 2.9.1 takes from `GMISC:GameplayTweakables.xml` (the
/// file is NFS-era; every other key -- `RepPoints`, `Bounty`, `Pursuit`, `GridGame`, `Nitro`, `Modes`, `MissionMode`,
/// `InterceptorDamage` -- is never read: DEAD DATA). `PowerUpTweakables.xml` is opened (`GMISC:PowerUpTweakables.xml`)
/// but only its root's validity is tested; none of `Duration`/`Effect`/`Turbo`/`Rep`/`RepWithNitro`/`SoundwavePower`
/// (nor the strings `Soundwave`, `DeflectHeat`, `Jammer`, `DrainBoost`, `CruiseControl`) occur in the binary: DEAD DATA.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct GameplayTweakables {
    /// `<Damage>`: `Competitor` (1.1), `World` (0.4), `Smackable` (0.1), `MaxOneHit` (30.0).
    pub competitor_damage: f32,
    pub world_damage: f32,
    pub smackable_damage: f32,
    pub max_one_hit: f32,
    /// `<Crash>`: the 8 values stored in order (`MinorTriggerSnapVehicle`, `MinorTriggerSnapNoVehicle`,
    /// `MinorTriggerRotVel`, `MajorTriggerSnapVehicle`, `MajorTriggerSnapNoVehicle`, `MajorTriggerRotVel`,
    /// `DurationPlayer`, `DurationAI`).
    pub crash: [f32; 8],
    /// `<Other><TimeUntilAppRateReques>` x `DAT_001ef9f0` (= 3600.0, hours -> seconds).
    pub time_until_app_rate_request: f32,
}

impl GameplayTweakables {
    /// Parse `GameplayTweakables.xml` the way `CGameplayTweakables::Load @001ef690` does (see struct docs).
    pub fn from_xml(xml: &str) -> Option<GameplayTweakables> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let g = |t: &str| xml_f32(&doc, t).unwrap_or(0.0);
        Some(GameplayTweakables {
            competitor_damage: g("Competitor"),
            world_damage: g("World"),
            smackable_damage: g("Smackable"),
            max_one_hit: g("MaxOneHit"),
            crash: [
                g("MinorTriggerSnapVehicle"),
                g("MinorTriggerSnapNoVehicle"),
                g("MinorTriggerRotVel"),
                g("MajorTriggerSnapVehicle"),
                g("MajorTriggerSnapNoVehicle"),
                g("MajorTriggerRotVel"),
                g("DurationPlayer"),
                g("DurationAI"),
            ],
            time_until_app_rate_request: g("TimeUntilAppRateReques") * 3600.0,
        })
    }
    pub fn load() -> Option<GameplayTweakables> {
        let p = format!("{}/xml/gameplay/misc/gameplaytweakables.xml", ASSETS292);
        GameplayTweakables::from_xml(&std::fs::read_to_string(p).ok()?)
    }
}

/// `CPlayerInfo` start-up gifts (`Initial_Sling_Gift`, `Initial_Boost_Gift`, `Initial_Repair_Gift`,
/// `Initial_PartnerCar_Gift` in `SetDebugTweakablesFromXML`; the shipped xml spells the last one
/// `Initial_Partner_Gift`, so the original read finds no node and the value is its default -- UNRESOLVED which default).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct InitialGifts {
    pub sling: i32,
    pub boost: i32,
    pub repair: i32,
    pub partner: Option<i32>,
}
impl InitialGifts {
    pub fn from_xml(xml: &str) -> Option<InitialGifts> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        Some(InitialGifts {
            sling: xml_i32(&doc, "Initial_Sling_Gift").unwrap_or(0),
            boost: xml_i32(&doc, "Initial_Boost_Gift").unwrap_or(0),
            repair: xml_i32(&doc, "Initial_Repair_Gift").unwrap_or(0),
            partner: xml_i32(&doc, "Initial_PartnerCar_Gift"),
        })
    }
}

// ============================================================================================================
// 3. AutoRepair (CCar::UpdatePowerups)
// ============================================================================================================

/// Number of bodywork slots the car keeps per-piece damage for (`CCar+0x196c .. +0x1998`, `memset(this+0x199c,0,0x30)`).
pub const NUM_BODY_PARTS: usize = 12;

/// The per-car bodywork-damage block `UpdatePowerups` reads and resets.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyworkDamage {
    /// `CCar+0x196c[12]`: damage state per bodywork piece (0 intact .. 4 detached).
    pub state: [i32; NUM_BODY_PARTS],
    /// `CCar+0x199c[12]`: float per piece (hit accumulator, reset to the constant `DAT_001ab550` = 0.0 on repair).
    pub timer: [f32; NUM_BODY_PARTS],
    /// `CCar+0x19cc[12]`: particle handle per piece (reset to -1).
    pub vfx_handle: [i32; NUM_BODY_PARTS],
    /// `CCar+0x1a14[12]`: particle effect index of the piece (-1 = none).
    pub vfx_effect: [i32; NUM_BODY_PARTS],
    /// Parent index per piece (`bodywork_cfg + i*0x128 + 0x88`, -1 = root).
    pub parent: [i32; NUM_BODY_PARTS],
    /// Piece the damage walk starts from (`bodywork_cfg + 0xe20`; -1 = none).
    pub head: i32,
    /// Particle effect name per piece (`bodywork_cfg + i*0x128 + 0x11c`), used for the "piece re-appears" burst.
    pub effect_name: [String; NUM_BODY_PARTS],
    /// Whether the piece has a live smackable object (`CCar+0x193c[i] != null`).
    pub has_smackable: [bool; NUM_BODY_PARTS],
    /// `CCar+0x19fc..0x1a08`: the four tyre-damage values.
    pub wheel_damage: [f32; 4],
    /// `CCar+0x1b4c != 0`: tyre damage disabled (`GetWheelState` returns 0 then).
    pub tyres_immune: bool,
}

impl Default for BodyworkDamage {
    fn default() -> Self {
        BodyworkDamage {
            state: [0; NUM_BODY_PARTS],
            timer: [0.0; NUM_BODY_PARTS],
            vfx_handle: [-1; NUM_BODY_PARTS],
            vfx_effect: [-1; NUM_BODY_PARTS],
            parent: [-1; NUM_BODY_PARTS],
            head: -1,
            effect_name: Default::default(),
            has_smackable: [false; NUM_BODY_PARTS],
            wheel_damage: [0.0; 4],
            tyres_immune: false,
        }
    }
}

impl BodyworkDamage {
    /// The chain walk shared by both halves of `UpdatePowerups`: starting at `head`, follow `parent`; true when some
    /// piece on the way is fully detached (`state == 4`) -- then neither counting nor repairing happens.
    fn chain_has_detached(&self) -> bool {
        let mut i = self.head;
        if i < 0 {
            return false;
        }
        let mut guard = 0;
        while (i as usize) < NUM_BODY_PARTS {
            if self.state[i as usize] == 4 {
                return true;
            }
            i = self.parent[i as usize];
            if i < 0 {
                return false;
            }
            guard += 1;
            if guard > NUM_BODY_PARTS {
                return false;
            }
        }
        false
    }

    /// The damage tally of `UpdatePowerups @001ab0e4` (`CCar+0x1bb4`): number of pieces with `state > 2`, plus -- unless
    /// `tyres_immune` -- one per cross-pair of tyres (`(0,2) (0,3) (1,3) (1,2)`) where both have damage `> 2.0`.
    pub fn damaged_piece_count(&self) -> u32 {
        let mut n = 0u32;
        for s in self.state.iter() {
            if *s > 2 {
                n += 1;
            }
        }
        if !self.tyres_immune {
            let w = &self.wheel_damage;
            let m = |a: f32, b: f32| if a <= b { a } else { b };
            // original order: min(w0,w2), min(w0,w3), min(w1,w3), min(w1,w2), each `2.0 < min`
            if 2.0 < m(w[0], w[2]) {
                n += 1;
            }
            if 2.0 < m(w[0], w[3]) {
                n += 1;
            }
            if 2.0 < m(w[1], w[3]) {
                n += 1;
            }
            if 2.0 < m(w[1], w[2]) {
                n += 1;
            }
        }
        n
    }

    /// Reset block of the repair (`@001ab418`): wheel damage 0, per-piece timer/handle/state/smackable cleared.
    fn reset_all(&mut self) {
        self.wheel_damage = [0.0; 4];
        for i in 0..NUM_BODY_PARTS {
            self.timer[i] = 0.0;
            self.vfx_handle[i] = -1;
            self.state[i] = 0;
            self.has_smackable[i] = false;
        }
    }
}

/// Runtime state of the AutoRepair monitor (`CCar+0x1bac`, `+0x1bb0`, `+0x1bb4`, `+0x1bb8`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AutoRepair {
    /// `+0x1bac`: seconds since the monitor last reset.
    pub accum: f32,
    /// `+0x1bb0`: seconds since the repair was triggered.
    pub since_trigger: f32,
    /// `+0x1bb4`: last damaged-piece count.
    pub count: u32,
    /// `+0x1bb8`: repair armed (the green "repairing" VFX shows while true).
    pub repairing: bool,
}

impl AutoRepair {
    /// `CCar+0x1ba8..0x1bb8` reset in the car re-init (`@00183483`): all zero.
    pub fn reset(&mut self) {
        *self = AutoRepair::default();
    }

    /// `CCar::UpdatePowerups(float dt) @001ab0e4` (the same block is inlined into `CCar::Integrate @001abaa8`).
    ///
    /// `is_player` = `CCar+0x1af0 != 0`; `active` = `CPlayerInfo::IsPowerUpActive(1)`. Returns the effects the repair
    /// asks for: `CarEffect::Repair{fraction: 1.0}` plus one `Particle` per piece that was detached (the original
    /// calls `ABKSound::CKartController::OnBodyworkSpawnParticleEffect(name)`) and `Other{tag:"RemoveSmackable"}`
    /// per piece whose smackable the repair removes (`CSmackableManager::RemoveSmackable(ptr, 1)`).
    ///
    /// Faithful quirks: the "remove smackable" branch inside the `state == 4` case of the original is dead (the
    /// inner test `state != 4` can never hold there), so detached pieces keep their smackable pointer cleared
    /// without removal; the repair is skipped (but the trigger consumed) when any ancestor piece is detached.
    pub fn update(
        &mut self,
        dt: f32,
        car: CarId,
        is_player: bool,
        active: bool,
        body: &mut BodyworkDamage,
        tw: &DebugTweakables,
        effects: &mut Vec<CarEffect>,
    ) {
        if !is_player || !active {
            return;
        }
        let prev = self.accum;
        self.accum = dt + prev;
        self.since_trigger += dt;
        if !self.repairing {
            if body.chain_has_detached() {
                return;
            }
            if tw.autorepair_start_delay < dt + prev {
                self.count = body.damaged_piece_count();
                if self.count as i32 <= tw.autorepair_damaged_pieces {
                    self.accum = 0.0;
                    if !self.repairing {
                        return;
                    }
                } else {
                    self.since_trigger = 0.0;
                    self.repairing = true;
                }
            } else if !self.repairing {
                return;
            }
        }
        // repair armed: wait Autorepair_VFX_Delay
        if self.since_trigger <= tw.autorepair_vfx_delay {
            return;
        }
        if body.chain_has_detached() {
            self.repairing = false;
            return;
        }
        for i in 0..NUM_BODY_PARTS {
            if body.state[i] == 4 {
                if body.vfx_effect[i] != -1 {
                    effects.push(CarEffect::Particle { name: body.effect_name[i].clone(), car: Some(car), pos: Vec3::ZERO });
                }
            } else if body.has_smackable[i] {
                effects.push(CarEffect::Other { tag: "RemoveSmackable", car: Some(car), vals: [i as f32, 1.0, 0.0, 0.0], pos: Vec3::ZERO });
            }
        }
        body.reset_all();
        effects.push(CarEffect::Repair { car, fraction: 1.0 });
        self.repairing = false;
    }
}

// ============================================================================================================
// 4. IntegratePowerups
// ============================================================================================================

/// Sound / particle bookkeeping of `CCar::IntegratePowerups @001ab558` (fields `CCar+0x1b3c`, `0x1b40`, `0x1bbc`,
/// `0x1bc0`, `0x1c60`, `0x1c64` and the boost sound controllers `0x1930/0x1934`).
#[derive(Clone, Debug, PartialEq)]
pub struct PowerupVfx {
    /// `+0x1bbc`: live repair particle (handle != -1).
    pub repair_vfx: bool,
    /// `+0x1b3c`: live boost particle.
    pub boost_vfx: bool,
    /// Boost start sound created (`+0x1930`), loop sound created (`+0x1934`).
    pub boost_start_sound: bool,
    pub boost_loop_sound: bool,
    /// `+0x1c60`: ramp `min(1, ramp + rb[0x98]*5.0)` while boosting (unreset in this function; consumed by the renderer).
    pub boost_ramp: f32,
}

impl Default for PowerupVfx {
    fn default() -> Self {
        PowerupVfx { repair_vfx: false, boost_vfx: false, boost_start_sound: false, boost_loop_sound: false, boost_ramp: 0.0 }
    }
}

/// Inputs of one `IntegratePowerups` call.
#[derive(Clone, Copy, Debug)]
pub struct PowerupInput {
    pub car: CarId,
    /// `CCar+0x1af0 != 0`.
    pub is_player: bool,
    /// Character byte (`CCar+0x1a5c`) for `CVoiceController::OnPowerUp`.
    pub character: u8,
    /// `AutoRepair::repairing`.
    pub repairing: bool,
    /// The car's ability exists, is active and reports ability type 4 (`ability->vfunc 0x1c == 4`): the repair particle is dropped.
    pub ability_type4_active: bool,
    /// `CCar+0x1b60 != 0`: boost start suppressed.
    pub boost_suppressed: bool,
    /// `game+0x32cc != 0 && IsCampaignActive && IsAdFeatureActive(6)`: sponsored "SpeedBoostBranded" particle.
    pub branded: bool,
    /// The car's sound controller is the local one (`CCar+0x1928 == global`): only then do the power-up sounds play.
    pub local_audio: bool,
    /// `rb+0x98` (used for the boost ramp).
    pub rb_98: f32,
    /// The boost start sound has finished playing (host audio query; `ABKSound::Core::CController::IsPlaying`).
    pub boost_start_sound_playing: bool,
}

/// Output of [`integrate_powerups`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerupFrame {
    /// `CarInput.boost` for `carsim.rs` (`IsPowerUpActive(2)` on the human car; `CCar::Integrate @001abaa8`
    /// then pushes `dt * Boost_Push_Factor * clamp(1 - air_time) * mass`).
    pub boost: bool,
    /// `IsPowerUpActive(0)` on the human car: KingSling is armed (`CCar` launch multiplies by
    /// [`DebugTweakables::king_sling_force_multiplier`] and [`catchup_disabled_floor`] applies).
    pub king_sling: bool,
    pub effects: Vec<CarEffect>,
}

/// `CCar::IntegratePowerups() @001ab558`: which particle effects / sounds each active power-up requests.
///
/// * AutoRepair active (and the ability is not type 4): while `repairing` a particle named `AutoRepair` is spawned as
///   instance `RepairEffect` and follows the car; the first frame plays `ABY_powerup_autorepair` and the character's
///   voice line (`CVoiceController::OnPowerUp(character, car)`), both only for the local audio listener. The particle is
///   removed when `repairing` ends or when a type-4 ability becomes active.
/// * SpeedBooster active: particle `SpeedBoost` (or `SpeedBoostBranded` for sponsor campaigns) instance
///   `SpeedBoostEffect`; sound `ABY_powerup_boost_start`, then `ABY_powerup_boost_loop` once the start sound ended.
///   When the power-up (or the player) is gone: particle removed, `ABY_powerup_boost_stop`.
/// * KingSling has no per-frame particle here; its spawn burst is [`king_sling_spawn_in_effect`].
pub fn integrate_powerups(inp: &PowerupInput, pu: &PlayerPowerups, vfx: &mut PowerupVfx, tw: &DebugTweakables) -> PowerupFrame {
    let mut out = PowerupFrame::default();
    let car = inp.car;
    let _ = tw;
    out.king_sling = inp.is_player && pu.is_active(0);
    if !inp.is_player {
        stop_boost(inp, vfx, &mut out);
        return out;
    }
    // ---- AutoRepair ----
    if pu.is_active(1) {
        if !inp.ability_type4_active {
            if !vfx.repair_vfx {
                if inp.repairing {
                    vfx.repair_vfx = true;
                    out.effects.push(CarEffect::Particle { name: "AutoRepair/RepairEffect".to_string(), car: Some(car), pos: Vec3::ZERO });
                    if inp.local_audio {
                        out.effects.push(CarEffect::Sound { name: "ABY_powerup_autorepair".to_string(), car: Some(car) });
                        out.effects.push(CarEffect::Other { tag: "VoiceOnPowerUp", car: Some(car), vals: [inp.character as f32, 0.0, 0.0, 0.0], pos: Vec3::ZERO });
                    }
                }
            } else if !inp.repairing {
                vfx.repair_vfx = false;
                out.effects.push(CarEffect::Other { tag: "RemoveParticle:RepairEffect", car: Some(car), vals: [0.0; 4], pos: Vec3::ZERO });
            }
        } else if vfx.repair_vfx {
            vfx.repair_vfx = false;
            out.effects.push(CarEffect::Other { tag: "RemoveParticle:RepairEffect", car: Some(car), vals: [0.0; 4], pos: Vec3::ZERO });
        }
    }
    // ---- SpeedBooster ----
    if !pu.is_active(2) {
        stop_boost(inp, vfx, &mut out);
        return out;
    }
    out.boost = true;
    if !vfx.boost_vfx {
        if inp.boost_suppressed {
            return out;
        }
        vfx.boost_vfx = true;
        let name = if inp.branded { "SpeedBoostBranded/SpeedBoostEffect" } else { "SpeedBoost/SpeedBoostEffect" };
        out.effects.push(CarEffect::Particle { name: name.to_string(), car: Some(car), pos: Vec3::ZERO });
        if !vfx.boost_start_sound && !vfx.boost_loop_sound {
            vfx.boost_start_sound = true;
            out.effects.push(CarEffect::Sound { name: "ABY_powerup_boost_start".to_string(), car: Some(car) });
        }
    }
    // follow the car, ramp (+0x1c60) and the start -> loop sound hand-over
    let r = vfx.boost_ramp + inp.rb_98 * 5.0;
    vfx.boost_ramp = if r >= 1.0 { 1.0 } else { r };
    if vfx.boost_start_sound && !inp.boost_start_sound_playing {
        vfx.boost_start_sound = false;
        vfx.boost_loop_sound = true;
        out.effects.push(CarEffect::Sound { name: "ABY_powerup_boost_loop".to_string(), car: Some(car) });
    }
    out
}

/// The tail `LAB_001ab6ec` of `IntegratePowerups`: SpeedBoost particle removal + stop sound.
fn stop_boost(inp: &PowerupInput, vfx: &mut PowerupVfx, out: &mut PowerupFrame) {
    if !vfx.boost_vfx {
        return;
    }
    vfx.boost_vfx = false;
    vfx.boost_start_sound = false;
    vfx.boost_loop_sound = false;
    out.effects.push(CarEffect::Other { tag: "RemoveParticle:SpeedBoostEffect", car: Some(inp.car), vals: [0.0; 4], pos: Vec3::ZERO });
    out.effects.push(CarEffect::Sound { name: "ABY_powerup_boost_stop".to_string(), car: Some(inp.car) });
}

/// `CEnvObjectManager::EnableKingSlingForPlayer() @001e5140`: when KingSling is chosen the local player's launch
/// object gets the `KingSlingSpawnIn` particle (instance name equals the effect name) and a 0.1 s timer
/// (`this+0x4750 = 0x3dcccccd`).
pub fn king_sling_spawn_in_effect(pu: &PlayerPowerups, car: CarId) -> Option<CarEffect> {
    if pu.is_active(0) {
        Some(CarEffect::Particle { name: "KingSlingSpawnIn".to_string(), car: Some(car), pos: Vec3::ZERO })
    } else {
        None
    }
}

/// KingSling launch multiplier (`CCar::GetLaunchVelocity(int) @00199c04`): when the power-up is on (human car),
/// the launch velocity vector is scaled by `GetDebugFloat(0x5c)` before the lift is added.
pub fn king_sling_launch_scale(pu: &PlayerPowerups, is_player: bool, tw: &DebugTweakables) -> f32 {
    if is_player && pu.is_active(0) {
        tw.king_sling_force_multiplier
    } else {
        1.0
    }
}

/// Catch-up (rubber band) disabling after a launch (`CCar::SetInSlingshot @00199f64`, `game+0x3298`; the whole block is
/// inside `if (car+0x1af0 != 0)`, i.e. only for the human car): returns the
/// new value of the timer. With KingSling it is raised to at least `King_Sling_Catchup_Disabled_Time` (0x5d),
/// otherwise -- only when `0 <= t_a - t_b < 0.1` (`DAT_0019a0ec`, `just_launched`) -- to at least 5.0.
pub fn catchup_disabled_floor(current: f32, is_player: bool, pu: &PlayerPowerups, tw: &DebugTweakables, just_launched: bool) -> f32 {
    if !is_player {
        return current;
    }
    if pu.is_active(0) {
        if current <= tw.king_sling_catchup_disabled_time {
            tw.king_sling_catchup_disabled_time
        } else {
            current
        }
    } else if just_launched {
        if current <= 5.0 {
            5.0
        } else {
            current
        }
    } else {
        current
    }
}

/// The SpeedBooster branch of `CCar::GetCamBehindMod @001a4210`: extra camera distance
/// `Boost_Camera_Max_Distance * clamp01((speed - Start_Speed)/(End_Speed - Start_Speed)) * clamp01(boost_time / Start_Delay)`
/// (`speed` = `CCar+0x1ab4`, `boost_time` = `CCar+0x1c44`; the clamp floor constant `DAT_001a43e0` is 0.0).
pub fn boost_cam_behind_mod(speed: f32, boost_time: f32, tw: &DebugTweakables) -> f32 {
    let a = (speed - tw.boost_camera_start_speed) / (tw.boost_camera_end_speed - tw.boost_camera_start_speed);
    let a = if 0.0 <= a { if a > 1.0 { 1.0 } else { a } } else { 0.0 };
    let b = boost_time / tw.boost_camera_start_delay;
    let b = if 0.0 <= b { if b > 1.0 { 1.0 } else { b } } else { 0.0 };
    tw.boost_camera_max_distance * a * b
}

// ============================================================================================================
// 5. Pickups (CPickupObject family, CEnvObjectManager pickup dispatch)
// ============================================================================================================

/// Pickup classes by helper name (`CEnvObjectManager::GetPickupTypeFromHelperName @001e2000`: 13 table entries
/// `{ctor, GetName, ..}` of `StaticGetName` strings, compared with `strcasecmp`, then as case-insensitive prefix).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupKind {
    /// `pickup_coin` (`CPickupCoin`)
    Coin,
    /// `pickup_megacoin` (`CPickupMegaCoin`)
    MegaCoin,
    /// `pickup_gem` (`CPickupGem`)
    Gem,
    /// `pickup_special_item_marker` (`CPickupSpecialItemMarker`)
    SpecialItemMarker,
    /// `pickup_giftbox` (`CPickupGiftBox`)
    GiftBox,
    /// `pickup_seedrushtoken` (`CPickupSeedRushToken`): fruit / ice-cream / coin depending on the mode
    SeedRushToken,
    /// `pickup_seedrushtoken_snow` (`CPickupSeedRushTokenSnow`)
    SeedRushTokenSnow,
    /// `pickup_seedrushtoken_large` (`CPickupSeedRushTokenLarge`): +1 on the event state's fruit counter, no coin
    SeedRushTokenLarge,
    /// `boost_pad` (`CPickupBoost`, model `ENVOBJ:Boost_Pad.XGM`)
    Boost,
    /// `ai_hotspot_small|medium|large|extralarge` (`CPickupAIHotspot*`): AI steering hot spots, never picked
    AIHotspot(u8),
}

/// Helper names in table order (index = `GetPickupIndexFromHelperName` result).
pub const PICKUP_HELPER_NAMES: [&str; 13] = [
    "pickup_coin",
    "pickup_megacoin",
    "pickup_gem",
    "pickup_special_item_marker",
    "pickup_giftbox",
    "pickup_seedrushtoken",
    "pickup_seedrushtoken_snow",
    "pickup_seedrushtoken_large",
    "boost_pad",
    "ai_hotspot_small",
    "ai_hotspot_medium",
    "ai_hotspot_large",
    "ai_hotspot_extralarge",
];

fn pickup_kind_by_index(i: usize) -> PickupKind {
    match i {
        0 => PickupKind::Coin,
        1 => PickupKind::MegaCoin,
        2 => PickupKind::Gem,
        3 => PickupKind::SpecialItemMarker,
        4 => PickupKind::GiftBox,
        5 => PickupKind::SeedRushToken,
        6 => PickupKind::SeedRushTokenSnow,
        7 => PickupKind::SeedRushTokenLarge,
        8 => PickupKind::Boost,
        n => PickupKind::AIHotspot((n - 9) as u8),
    }
}

/// `CEnvObjectManager::GetPickupIndexFromHelperName @001e20e4`: exact (case-insensitive) match, else prefix match,
/// else -1.
pub fn pickup_index_from_helper_name(name: &str) -> i32 {
    for (i, n) in PICKUP_HELPER_NAMES.iter().enumerate() {
        if name.eq_ignore_ascii_case(n) {
            return i as i32;
        }
    }
    let lower = name.to_ascii_lowercase();
    for (i, n) in PICKUP_HELPER_NAMES.iter().enumerate() {
        if lower.starts_with(n) {
            return i as i32;
        }
    }
    -1
}

/// `CEnvObjectManager::GetPickupTypeFromHelperName @001e2000` (0 = no pickup in the original).
pub fn pickup_kind_from_helper_name(name: &str) -> Option<PickupKind> {
    let i = pickup_index_from_helper_name(name);
    if i < 0 {
        None
    } else {
        Some(pickup_kind_by_index(i as usize))
    }
}

/// Seconds a collected pickup keeps flying around the collecting car before it disappears
/// (`DAT_001e7a5c` == `DAT_001e8db4` == ... == 0.2 in `CPickupCoin::Update @001e7944`, the seed-rush token, gem,
/// gift box and mega coin `Update`s).
pub const PICKUP_COLLECT_TIME: f32 = 0.2;

/// Shape of the "fly to the car" offset: `1.65 + (sin(t * 5 * PI) + 1) * 0.5 * 1.35` (`DAT_001e7a68` = 1.65,
/// `DAT_001e7a64` = 1.35, `vmov.f32 s13,#5.0`, `DAT_001e7a60` = PI; the `sinf` argument was lost by Ghidra and
/// decoded from the machine code of `CPickupCoin::Update @001e7944`).
pub fn pickup_fly_offset(t: f32) -> f32 {
    1.65 + ((t * 5.0 * std::f32::consts::PI).sin() + 1.0) * 0.5 * 1.35
}

/// Position of a collected pickup while it animates: `car_pos + 0.5 * axis1 + pickup_fly_offset(t) * axis0`
/// (`axis0/axis1` = the first two rows of the car's `CXGSRigidBody::GetMatrix`).
pub fn pickup_fly_position(car_pos: Vec3, axis0: Vec3, axis1: Vec3, t: f32) -> Vec3 {
    car_pos + axis1 * 0.5 + axis0 * pickup_fly_offset(t)
}

/// What the pickup code needs to know about the car that touched it.
#[derive(Clone, Copy, Debug)]
pub struct PickupCar {
    pub id: CarId,
    /// `CCar+0x1af0 != 0` (has a `CPlayer`).
    pub is_player: bool,
    /// `CPlayer::IsLocalPlayer`.
    pub is_local_player: bool,
    /// `CGame::GetPlayerIndex(player)` (-1 if not a player).
    pub player_index: i32,
    /// `CCar+0x1af4 != 0` (the `CanBePicked` blocker).
    pub flag_1af4: bool,
    pub pos: Vec3,
    pub axis0: Vec3,
    pub axis1: Vec3,
}

/// Game-wide context for pickups.
#[derive(Clone, Copy, Debug)]
pub struct PickupCtx {
    /// `CPlayerInfo+0x420 != 0`: coin doubler (`CCar::AddCoin`: 1 -> 2, `AddCoins(n)`: n << 1).
    pub coin_doubler: bool,
    /// `CNetwork::GetMPGameState == 0` (no multiplayer game running; coins are hidden/ignored otherwise).
    pub mp_idle: bool,
    /// The global at `DAT_001e8c3c + 0x1e88ac`: `< 2` while a fruit rush (`0` FRUIT, `1` ICECREAM,
    /// `StringToFruitRushMode @000fa350`) is running; seed-rush tokens are fruit then, coins otherwise.
    pub fruit_rush_mode: u32,
    /// The first-ever gift box state (`DAT_001ecdc4 + 0x1eccfc`): false until the player opened the first gift box.
    pub gift_box_opened_before: bool,
}

/// Events a pickup raises.
#[derive(Clone, Debug, PartialEq)]
pub enum PickupEvent {
    /// `CCar::AddCoin`/`AddCoins` -> `CPlayerInfo::AddSoftCurrencyFromPickup(value)`; also `car+0x1c1c += value`.
    /// Achievement `PickUpCoins` and the challenge event `Pickup`.
    Coins { car: CarId, value: i32 },
    /// `CCar::AddGems(1)` -> `AddHardCurrencyFromPickup(1)`; `car+0x1c20 += 1`; sound `CGeneralController::OnEvent(3,1)`.
    Gem { car: CarId },
    /// Fruit / ice-cream counted into the event state (`TEventState+0x20 += 1`; achievement `PickUpFruit`,
    /// `CSeasonalContentManager::UpdateChallenges(3, 1.0)`). `kind` = token kind 0..5 (banana, melon, strawberry,
    /// ice wafer, ice lolly, ice cream); the particle `seed_token_particle_name(kind)` plays at the token.
    Fruit { car: CarId, kind: u8 },
    /// Large token: only `+1` on the event state's fruit counter (`CPickupSeedRushTokenLarge::OnCarInRadius`).
    FruitLarge { car: CarId },
    /// First gift box ever: pops the gift box screen (`CChallengeManager::Event`, sound `OnEvent(6,1)`).
    GiftBoxFirst { car: CarId },
    /// Boost pad touched: `CCar+0x1be0 = 1` (set in the original even for AI cars; `carsim.rs` detects pads by wheel
    /// material 0x18/0x22 itself) and a `Boost` challenge event for the player.
    BoostPad { car: CarId },
    /// A seed-rush token was passed without being touched (`NearlyMiss` challenge event, `gap` = how far).
    NearlyMiss { car: CarId, gap: f32 },
    /// Plays the collect sound `ABKSound::CGeneralController::OnEvent(id, 1)`.
    Sound { id: i32 },
}

/// Value of one `pickup_megacoin` (`CCar::AddCoins(*(int*)(DAT_001ee47c + 0x1ee42c))`). The global lives in `.data`
/// (Ghidra `0x00daf740`) with the link-time value 50; no writer was found, so 50 is the shipped value.
pub const MEGA_COIN_VALUE: i32 = 50;

/// `CCar::AddCoin() @001b0c38`: 1 coin (2 with the coin doubler).
pub fn coin_value(ctx: &PickupCtx) -> i32 {
    if ctx.coin_doubler {
        2
    } else {
        1
    }
}

/// `CCar::AddCoins(int) @001b0c84`: `n` coins (`n << 1` with the doubler).
pub fn coins_value(n: i32, ctx: &PickupCtx) -> i32 {
    if ctx.coin_doubler {
        n << 1
    } else {
        n
    }
}

/// `CPlayerInfo::AddSoftCurrencyFromPickup(int) @0014c618` on the decoded wallet (the original keeps it XOR-obfuscated
/// with `0x3e5ab9c`): `sum = wallet + amount`; if `wallet <= sum` the result is `min(sum, 999_999_999)`, otherwise
/// (negative amount) the wallet is unchanged.
pub fn add_soft_currency(wallet: i32, amount: i32) -> i32 {
    let sum = wallet.wrapping_add(amount);
    if wallet <= sum {
        if sum < 999_999_999 {
            sum
        } else {
            999_999_999
        }
    } else {
        wallet
    }
}

/// Runtime state of one placed pickup (`CPickupObject` + the fields every subclass adds: `+0x7c/0x80/0x84/0x88/0x8c`
/// for coin, gem, gift box, mega coin, large token; the plain seed-rush token uses `+0xa0/0xa4/0xac/0xb0` and a
/// "passed" flag `+0xa8`).
#[derive(Clone, Debug, PartialEq)]
pub struct Pickup {
    pub kind: PickupKind,
    pub pos: Vec3,
    /// Model bounding radius (`model+0xcc`): `IsInRadius` tests `|p - c| <= r + extent` (false when the model is not loaded).
    pub extent: f32,
    /// Visible / alive.
    pub active: bool,
    /// Already picked and playing the fly-away animation.
    pub picked: bool,
    /// Car that picked it.
    pub picker: Option<CarId>,
    pub picker_player: i32,
    /// Seconds since pick-up (`+0x8c` etc.).
    pub timer: f32,
    /// Seed-rush token only: kind 0..5 (`+0x94`), nearly-miss already reported (`+0xa8`).
    pub token_kind: u8,
    pub nearly_missed: bool,
    pub model_loaded: bool,
}

impl Pickup {
    pub fn new(kind: PickupKind, pos: Vec3, extent: f32) -> Pickup {
        Pickup { kind, pos, extent, active: true, picked: false, picker: None, picker_player: -1, timer: 0.0, token_kind: 0, nearly_missed: false, model_loaded: true }
    }

    /// `CPickup*::IsInRadius(CXGSVector32 const&, float, int)` (e.g. `CPickupCoin::IsInRadius @001e7380`).
    pub fn is_in_radius(&self, center: Vec3, radius: f32) -> bool {
        if !self.model_loaded {
            return false;
        }
        let r = radius + self.extent;
        (center - self.pos).length_squared() <= r * r
    }

    /// `CPickup*::CanBePicked(CCar*)` (coin @001e77b0, gem @001ec0e8, mega coin @001ee484, large token @001eae6c,
    /// gift box @001ecdd8, seed-rush token @001e83c0). `fruit_rush_blocks` is the global tested by the plain token
    /// (`DAT_001e8414 + 0x1e83e4`): a car with `flag_1af4` cannot pick it while it is set.
    pub fn can_be_picked(&self, car: Option<&PickupCar>, ctx: &PickupCtx, fruit_rush_blocks: bool) -> bool {
        let blocked = car.map(|c| c.flag_1af4).unwrap_or(false);
        match self.kind {
            PickupKind::Coin => !blocked && ctx.mp_idle && self.active && !self.picked,
            PickupKind::SeedRushToken | PickupKind::SeedRushTokenSnow => {
                if car.is_some() && blocked && fruit_rush_blocks {
                    return false;
                }
                self.active && !self.picked
            }
            PickupKind::Gem | PickupKind::MegaCoin | PickupKind::SeedRushTokenLarge | PickupKind::GiftBox => !blocked && self.active && !self.picked,
            PickupKind::Boost => true,
            _ => false,
        }
    }

    /// `CPickup*::Update(float)`: advance the collect animation; `picker` = (car position, matrix row 0, row 1) of
    /// the car that collected it. After 0.2 s the pickup is deactivated.
    pub fn update(&mut self, dt: f32, picker: Option<(Vec3, Vec3, Vec3)>) {
        self.timer += dt;
        if !self.picked {
            return;
        }
        if self.timer > PICKUP_COLLECT_TIME {
            self.active = false;
            return;
        }
        if let Some((car_pos, a0, a1)) = picker {
            self.pos = pickup_fly_position(car_pos, a0, a1, self.timer);
        }
    }

    /// `CPickup*::OnCarInRadius(CCar*, CXGSVector32 const&, float)`: coin @001e7708, boost pad @001e6cb8, seed-rush
    /// token @001e871c, large token @001eada8, gem @001ec050, gift box @001ecc8c, mega coin @001ee3b8.
    /// `gap` (token only) = `|car - token| - (radius + extent)`: positive means "close but not touching".
    /// `behind` (token only) = `dot(car - token, car_axis0) >= 0` (the token has been passed).
    pub fn on_car_in_radius(&mut self, car: &PickupCar, ctx: &PickupCtx, gap: f32, behind: bool) -> Vec<PickupEvent> {
        let mut ev = Vec::new();
        match self.kind {
            PickupKind::Coin => {
                self.timer = 0.0;
                self.picked = car.is_player;
                self.picker = Some(car.id);
                ev.push(PickupEvent::Coins { car: car.id, value: coin_value(ctx) });
                ev.push(PickupEvent::Sound { id: 2 });
            }
            PickupKind::Boost => {
                if car.is_player {
                    ev.push(PickupEvent::BoostPad { car: car.id });
                }
            }
            PickupKind::Gem => {
                self.timer = 0.0;
                self.picked = car.is_player;
                if car.is_player && car.is_local_player {
                    self.picker = Some(car.id);
                    ev.push(PickupEvent::Gem { car: car.id });
                    ev.push(PickupEvent::Sound { id: 3 });
                }
            }
            PickupKind::MegaCoin => {
                self.timer = 0.0;
                self.picked = car.player_index != -1;
                self.picker_player = car.player_index;
                if car.is_player {
                    ev.push(PickupEvent::Coins { car: car.id, value: coins_value(MEGA_COIN_VALUE, ctx) });
                    ev.push(PickupEvent::Sound { id: 2 });
                }
            }
            PickupKind::SeedRushTokenLarge => {
                self.timer = 0.0;
                self.picked = car.player_index != -1;
                self.picker_player = car.player_index;
                if car.is_player {
                    ev.push(PickupEvent::FruitLarge { car: car.id });
                }
            }
            PickupKind::GiftBox => {
                self.timer = 0.0;
                self.picked = car.player_index != -1;
                self.picker_player = car.player_index;
                if car.is_player {
                    if !ctx.gift_box_opened_before {
                        ev.push(PickupEvent::GiftBoxFirst { car: car.id });
                        ev.push(PickupEvent::Sound { id: 6 });
                    } else {
                        ev.push(PickupEvent::Coins { car: car.id, value: coin_value(ctx) });
                        ev.push(PickupEvent::Sound { id: 2 });
                    }
                }
            }
            PickupKind::SeedRushToken | PickupKind::SeedRushTokenSnow => {
                if gap > 0.0 {
                    // inside the (bigger) query radius but not touching: one NearlyMiss per token
                    if self.nearly_missed || !behind || !car.is_player {
                        return ev;
                    }
                    ev.push(PickupEvent::NearlyMiss { car: car.id, gap });
                    self.nearly_missed = true;
                    return ev;
                }
                self.timer = 0.0;
                self.picked = true;
                self.picker_player = car.player_index;
                if ctx.fruit_rush_mode < 2 {
                    if car.is_player {
                        ev.push(PickupEvent::Fruit { car: car.id, kind: self.token_kind });
                    }
                } else if car.is_player {
                    ev.push(PickupEvent::Coins { car: car.id, value: coin_value(ctx) });
                    ev.push(PickupEvent::Sound { id: 2 });
                }
            }
            _ => {}
        }
        ev
    }
}

/// Name of the particle a seed-rush token spawns when collected in a fruit rush (`@001e871c`, `switch(kind)`).
pub fn seed_token_particle_name(kind: u8) -> Option<&'static str> {
    match kind {
        0 => Some("BananaDestroyed"),
        1 => Some("MelonDestroyed"),
        2 => Some("StrawberryDestroyed"),
        3 => Some("IceWaferDestroyed"),
        4 => Some("IceLollyDestroyed"),
        5 => Some("IceCreamDestroyed"),
        _ => None,
    }
}

/// `CEnvObjectManager::InvokePickupsInRadius(CCar*, CXGSVector32 const&, float) @001e22a0` without the sorted-sweep
/// acceleration (pickups are kept sorted by `dot(pos, sweep_axis)` and only those within `DAT_001e24f8` of the car's
/// projection are visited): for every pickup that `can_be_picked` and `is_in_radius`, in list order, returns its index.
/// (The original also lets an active ability pick objects up first, `CheckPickupByAbility @001dd88c`:
/// UNRESOLVED -- the ability type table belongs to `abilities.rs`.)
pub fn pickups_in_radius(list: &[Pickup], car: &PickupCar, ctx: &PickupCtx, radius: f32) -> Vec<usize> {
    let mut v = Vec::new();
    for (i, p) in list.iter().enumerate() {
        if p.can_be_picked(Some(car), ctx, false) && p.is_in_radius(car.pos, radius) {
            v.push(i);
        }
    }
    v
}

// ============================================================================================================
// 6. Score counters (CScoreSystem / CScoreCounter*)
// ============================================================================================================

/// `EScoreCounterType` (the `StaticGetType` of every counter class, `@001f3a04..001f3b54`; index into the 15-slot
/// array `CScoreSystem+4`). `CScoreCounterGrind` named in `scoreconfig.xml` has no class in 2.9.1 (its XML node is
/// skipped by `CScoreSystem::Init @001f49cc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreCounterType {
    Base = 0,
    ImpactWithObject = 1,
    FinishingPosition = 2,
    FinishingTime = 3,
    FinishingFruit = 4,
    Jenga = 5,
    Bonus = 6,
    Drift = 7,
    InAir = 8,
    Drafting = 9,
    HitBoostPad = 10,
    DamageDone = 11,
    TopSpeed = 12,
    Acceleration = 13,
    Deaths = 14,
}

/// `CGame::GetGameMode()` values that matter to scoring (`CEventDefinitionManager::StringToGameMode @000fa1bc`:
/// `TIME_ATTACK` 6, `SEED_RUSH` 7; 10 is the Jenga mode that disables every counter except `Jenga`).
pub const GAME_MODE_TIME_ATTACK: i32 = 6;
pub const GAME_MODE_SEED_RUSH: i32 = 7;
pub const GAME_MODE_JENGA: i32 = 10;

/// `CScoreSystem::IsCounterAvailable(EScoreCounterType) @001f48b8`.
pub fn is_counter_available(ty: ScoreCounterType, game_mode: i32) -> bool {
    ty == ScoreCounterType::Jenga || game_mode != GAME_MODE_JENGA
}

/// `CMetagameManager::GetRaceMaxScore(int cc)`: `cc * 150` (`0x96`). Basis of every finishing counter.
pub fn race_max_score(race_cc: i32) -> i32 {
    race_cc * 0x96
}

/// The per-counter "points popup" fields every `CScoreCounter` carries (`+4` shown flag, `+8` last delta, `+0xc`
/// timestamp in ms): UI only (`CXGSFE_InGameScreen::UpdateScoreCounters @003001b4`). Always 0 in 2.9.1 since every
/// per-metre value in `scoreconfig.xml` is 0.0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScorePopup {
    pub show: bool,
    pub delta: i32,
    pub stamp_ms: u32,
}

/// `<ScoreSystem>` of `ScoreConfig.xml` (`XMLGLOBALPAK:ScoreConfig.xml`, `CScoreSystem::Init @001f49cc`): the
/// children are matched against the counter names; unknown ones (`ScoreCounterGrind`) are ignored.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreConfig {
    pub has_finishing_position: bool,
    pub has_jenga: bool,
    /// `ScoreCounterFinishingFruit`: `PercentForMinScore`, `PercentForMaxScore`, `MinFruitScore`, `MaxFruitScore`.
    pub fruit: Option<[f32; 4]>,
    /// `ScoreCounterFinishingTime`: `PercentOverForMaxScore`, `PercentOverForMinScore`, `MaxTimeScore`, `MinTimeScore`
    /// in the order of the object fields (+0x3fc, +0x400, +0x404, +0x408).
    pub time: Option<[f32; 4]>,
    /// `ScoreCounterDamageDone`: `ScorePerDamagePoint`, `PerCCPowerModifier`.
    pub damage_done: Option<[f32; 2]>,
    /// `ScoreCounterDeaths`: `MaxScoreDivisor`, `MultiplierPerDeath`.
    pub deaths: Option<[f32; 2]>,
    /// `ScoreCounterTopSpeed`: `SpeedThreshold`, `X`, `Y`, `Z`, then the five `ThemeModifiers`.
    pub top_speed: Option<([f32; 4], [f32; 5])>,
    /// `ScoreCounterAcceleration`: `X`, `Y`, `Z`, then the five `ThemeModifiers`.
    pub acceleration: Option<([f32; 3], [f32; 5])>,
    /// `ScoreCounterDrift` / `InAir` / `Drafting`: `ScorePerMeter`, `MinDistance` (drafting only reads the first).
    pub drift: Option<[f32; 2]>,
    pub in_air: Option<[f32; 2]>,
    pub drafting: Option<[f32; 2]>,
    /// `ScoreCounterHitBoostPad`: `Score` (int).
    pub hit_boost_pad: Option<i32>,
    /// `ScoreCounterImpactWithObject`: `MinImpactVelocity`, `MaxImpactVelocity` (floats; absent in the shipped xml),
    /// `MinImpactScore`, `MaxImpactScore` (ints).
    pub impact: Option<([f32; 2], [i32; 2])>,
}

/// Theme names in index order (`Seedway` = episode 1 .. `SubZero` = episode 5), the table at Ghidra `0x00dac080`.
pub const THEME_NAMES: [&str; 5] = ["Seedway", "RockyRoad", "Air", "Stunt", "SubZero"];

fn child_f32(n: &roxmltree::Node, tag: &str) -> Option<f32> {
    n.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).and_then(|t| t.trim().parse::<f32>().ok())
}
fn child_i32(n: &roxmltree::Node, tag: &str) -> Option<i32> {
    n.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).and_then(|t| t.trim().parse::<i32>().ok())
}

impl ScoreConfig {
    /// Parser for `scoreconfig.xml` mirroring each counter's `LoadProperties` (`@001f04d8`, `001f07ac`, `001f0bfc`,
    /// `001f0e44`, `001f13f8`, `001f19e4`, `001f1cd0`, `001f2110`, `001f246c`, `001f28e4`, `001f2f04`, `001f3908`).
    /// `strtod` / `GetFloat` read missing keys as 0 / keep the constructor default; here missing keys are 0.
    pub fn from_xml(xml: &str) -> Option<ScoreConfig> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let root = doc.root_element();
        let mut c = ScoreConfig {
            has_finishing_position: false,
            has_jenga: false,
            fruit: None,
            time: None,
            damage_done: None,
            deaths: None,
            top_speed: None,
            acceleration: None,
            drift: None,
            in_air: None,
            drafting: None,
            hit_boost_pad: None,
            impact: None,
        };
        for n in root.children().filter(|n| n.is_element()) {
            let f = |t: &str| child_f32(&n, t).unwrap_or(0.0);
            let theme = |n: &roxmltree::Node| -> [f32; 5] {
                let mut a = [0.0f32; 5];
                if let Some(tm) = n.children().find(|c| c.has_tag_name("ThemeModifiers")) {
                    for (i, name) in THEME_NAMES.iter().enumerate() {
                        a[i] = child_f32(&tm, name).unwrap_or(0.0);
                    }
                }
                a
            };
            match n.tag_name().name() {
                "ScoreCounterFinishingPosition" => c.has_finishing_position = true,
                "ScoreCounterJenga" => c.has_jenga = true,
                "ScoreCounterFinishingFruit" => c.fruit = Some([f("PercentForMinScore"), f("PercentForMaxScore"), f("MinFruitScore"), f("MaxFruitScore")]),
                "ScoreCounterFinishingTime" => c.time = Some([f("PercentOverForMaxScore"), f("PercentOverForMinScore"), f("MaxTimeScore"), f("MinTimeScore")]),
                "ScoreCounterDamageDone" => c.damage_done = Some([f("ScorePerDamagePoint"), f("PerCCPowerModifier")]),
                "ScoreCounterDeaths" => c.deaths = Some([f("MaxScoreDivisor"), f("MultiplierPerDeath")]),
                "ScoreCounterTopSpeed" => c.top_speed = Some(([f("SpeedThreshold"), f("X"), f("Y"), f("Z")], theme(&n))),
                "ScoreCounterAcceleration" => c.acceleration = Some(([f("X"), f("Y"), f("Z")], theme(&n))),
                "ScoreCounterDrift" => c.drift = Some([f("ScorePerMeter"), f("MinDistance")]),
                "ScoreCounterInAir" => c.in_air = Some([f("ScorePerMeter"), f("MinDistance")]),
                "ScoreCounterDrafting" => c.drafting = Some([f("ScorePerMeter"), f("MinDistance")]),
                "ScoreCounterHitBoostPad" => c.hit_boost_pad = Some(child_i32(&n, "Score").unwrap_or(0)),
                "ScoreCounterImpactWithObject" => {
                    c.impact = Some(([f("MinImpactVelocity"), f("MaxImpactVelocity")], [child_i32(&n, "MinImpactScore").unwrap_or(0), child_i32(&n, "MaxImpactScore").unwrap_or(0)]))
                }
                _ => {}
            }
        }
        Some(c)
    }

    /// Reads `<ASSETS292>/xml/xml/global/scoreconfig.xml`.
    pub fn load() -> Option<ScoreConfig> {
        let p = format!("{}/xml/xml/global/scoreconfig.xml", ASSETS292);
        ScoreConfig::from_xml(&std::fs::read_to_string(p).ok()?)
    }
}

/// `CScoreCounterDrift / InAir / Drafting` (`Update(CPlayer*, int active) @001f1474 / @001f2f80 / @001f0e88`):
/// distance travelled while `active`, scored per metre.
/// `start_*` = `+0x3fc..0x404`, `base` = `+0x408` (score when the run began), `running` = `+0x3f8`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceCounter {
    pub score: f32,
    pub running: bool,
    pub start: Vec3,
    pub base: f32,
    pub per_meter: f32,
    /// `+0x410`: runs shorter than this are not scored (Drift / InAir; Drafting has no minimum).
    pub min_distance: f32,
    pub use_min: bool,
    pub popup: ScorePopup,
}

impl DistanceCounter {
    /// Constructor defaults: Drift `@001f17a4` and InAir `@001f32b0` `ScorePerMeter = 20.0` (`0x41a00000`),
    /// `MinDistance = 1.0`; Drafting `@001f11a0` `ScorePerMeter = 20.0`.
    pub fn new(use_min: bool) -> DistanceCounter {
        DistanceCounter { score: 0.0, running: false, start: Vec3::ZERO, base: 0.0, per_meter: 20.0, min_distance: 1.0, use_min, popup: ScorePopup::default() }
    }
    /// `Reset @001f1364` (Drift): score 0 and not running.
    pub fn reset(&mut self) {
        self.score = 0.0;
        self.running = false;
    }
    /// `GetScore @001f1378`: `(int)(score + 0.5)`.
    pub fn get_score(&self) -> i32 {
        (self.score + 0.5) as i32
    }
    /// `Update`: `suspended` = `car+0x464 != 0 && car+0x468 >= 0` (the car is inside a launcher / scripted section:
    /// nothing counts); `now_ms` for the popup stamp.
    pub fn update(&mut self, active: bool, suspended: bool, pos: Vec3, now_ms: u32) {
        if suspended {
            return;
        }
        if !active {
            if !self.running {
                return;
            }
            self.running = false;
        } else if !self.running {
            self.start = pos;
            self.base = self.score;
            self.running = true;
        }
        let d = (pos - self.start).length();
        if self.use_min && d < self.min_distance {
            return;
        }
        self.score = self.base + d * self.per_meter;
        self.popup.delta = (self.score - self.base) as i32;
        self.popup.show = self.popup.delta > 0;
        self.popup.stamp_ms = now_ms;
    }
}

/// `CScoreCounterAcceleration` / `CScoreCounterTopSpeed` (`GetScore @001f0254 / @001f363c`):
/// `(X * t * pow(cc, Y) + cc * Z) * theme_modifier[theme]`, truncated; `t` = seconds spent in the state, `cc` = the
/// kart's cc (`CKartManager::GetKartCC`, -1 until the first update). The `pow` operand order was decoded from the
/// machine code (`pow((double)cc, Y)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerCounter {
    pub score: f32,
    pub time: f32,
    pub kart_cc: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub theme: [f32; 5],
    /// TopSpeed only: `SpeedThreshold` (default 1.2 = `0x3f99999a`, shipped 1.35).
    pub speed_threshold: f32,
}

impl PowerCounter {
    pub fn acceleration(cfg: ([f32; 3], [f32; 5])) -> PowerCounter {
        PowerCounter { score: 0.0, time: 0.0, kart_cc: -1, x: cfg.0[0], y: cfg.0[1], z: cfg.0[2], theme: cfg.1, speed_threshold: 0.0 }
    }
    pub fn top_speed(cfg: ([f32; 4], [f32; 5])) -> PowerCounter {
        PowerCounter { score: 0.0, time: 0.0, kart_cc: -1, x: cfg.0[1], y: cfg.0[2], z: cfg.0[3], theme: cfg.1, speed_threshold: cfg.0[0] }
    }
    /// `Reset`: score 0, time 0, cc -1.
    pub fn reset(&mut self) {
        self.score = 0.0;
        self.time = 0.0;
        self.kart_cc = -1;
    }
    /// `CScoreCounterAcceleration::Update(CPlayer*, float) @001f0444`: time accrues while the car is not drifting.
    pub fn update_acceleration(&mut self, dt: f32, suspended: bool, drifting: bool, kart_cc: i32) {
        if suspended {
            return;
        }
        if !drifting {
            self.time += dt;
        }
        if self.kart_cc < 0 {
            self.kart_cc = kart_cc;
        }
    }
    /// `CScoreCounterTopSpeed::Update(CPlayer*, float) @001f3854`: time accrues while
    /// `top_speed_of_kart * SpeedThreshold <= speed` (`car+0x1aa8 -> +0x438` x threshold vs `car+0x1ab4`).
    pub fn update_top_speed(&mut self, dt: f32, suspended: bool, kart_top_speed: f32, speed: f32, kart_cc: i32) {
        if suspended {
            return;
        }
        if kart_top_speed * self.speed_threshold <= speed {
            self.time += dt;
        }
        if self.kart_cc < 0 {
            self.kart_cc = kart_cc;
        }
    }
    /// `GetScore` for Acceleration (`theme == 5` -> 0) and TopSpeed (`cc == -1` -> 0).
    pub fn get_score(&mut self, theme: usize, is_top_speed: bool) -> i32 {
        if (!is_top_speed && theme == 5) || (is_top_speed && self.kart_cc == -1) || theme >= 5 {
            return 0;
        }
        let cc = self.kart_cc as f64;
        let a = (self.x * self.time) as f64 * cc.powf(self.y as f64);
        let s = ((a as f32) + (self.kart_cc as f32) * self.z) * self.theme[theme];
        self.score = s;
        s as i32
    }
}

/// `CScoreCounterDeaths` (`GetScore @001f0c40`): `(S / MaxScoreDivisor) * pow(MultiplierPerDeath, deaths)` with
/// `S = race_max_score(cc)`; `AddDeath @001f0d78` increments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeathsCounter {
    pub max_score_divisor: f32,
    pub multiplier_per_death: f32,
    pub deaths: i32,
    pub score: f32,
}
impl DeathsCounter {
    pub fn new(cfg: [f32; 2]) -> DeathsCounter {
        DeathsCounter { max_score_divisor: cfg[0], multiplier_per_death: cfg[1], deaths: 0, score: 0.0 }
    }
    pub fn reset(&mut self) {
        self.score = 0.0;
        self.deaths = 0;
    }
    pub fn add_death(&mut self) {
        self.deaths += 1;
    }
    pub fn get_score(&mut self, max_race_score: i32) -> i32 {
        let a = ((max_race_score as f32) / self.max_score_divisor) as f64;
        let s = (a * (self.multiplier_per_death as f64).powf(self.deaths as f64)) as f32;
        self.score = s;
        s as i32
    }
}

/// `CScoreCounterImpactWithObject::OnCollision @001f29a0`: collision impulse -> score. Uses the 16-entry
/// `ContactBuffer` (`@001992dc`) so the same body is not counted twice within 5 s.
#[derive(Clone, Debug, PartialEq)]
pub struct ImpactCounter {
    pub min_velocity: f32,
    pub max_velocity: f32,
    pub min_score: i32,
    pub max_score: i32,
    pub score: i32,
    pub contacts: ContactBuffer,
    pub popup: ScorePopup,
}

/// `ContactBuffer` (`@00199298` ctor, `@001992bc` Reset, `@001992dc` OnCollision): 16 `{body, time_ms}` slots (`0x80`
/// bytes) + count at `+0x80`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactBuffer {
    pub entries: Vec<(u32, u32)>,
}
impl ContactBuffer {
    pub fn reset(&mut self) {
        self.entries.clear();
    }
    /// Returns 2 for a null body, 1 when `body` was already recorded in the last 5000 ms, else 0 (and records it).
    /// Entries older than 5000 ms are dropped first (the buffer is time ordered); a full buffer drops its oldest entry.
    pub fn on_collision(&mut self, body: Option<u32>, now_ms: u32) -> u32 {
        let body = match body {
            None => return 2,
            Some(b) => b,
        };
        self.entries.retain(|e| now_ms.wrapping_sub(e.1) <= 5000);
        if self.entries.iter().any(|e| e.0 == body) {
            return 1;
        }
        if self.entries.len() == 16 {
            self.entries.remove(0);
        }
        self.entries.push((body, now_ms));
        0
    }
}

impl ImpactCounter {
    pub fn new(cfg: ([f32; 2], [i32; 2])) -> ImpactCounter {
        ImpactCounter { min_velocity: cfg.0[0], max_velocity: cfg.0[1], min_score: cfg.1[0], max_score: cfg.1[1], score: 0, contacts: ContactBuffer::default(), popup: ScorePopup::default() }
    }
    pub fn reset(&mut self) {
        self.score = 0;
        self.contacts.reset();
    }
    /// `rel_speed` = `dot(car_pos_velocity... )` as in the original: `dot(rb_car[0x10..0x18] - body[0x10..0x18],
    /// normal)` (relative velocity along the contact normal). Returns true when it scored.
    pub fn on_collision(&mut self, suspended: bool, body: Option<u32>, rel_speed: f32, now_ms: u32) -> bool {
        if suspended {
            return false;
        }
        if self.contacts.on_collision(body, now_ms) == 1 {
            return false;
        }
        if self.min_velocity < rel_speed {
            let v = if rel_speed < self.max_velocity { rel_speed } else { self.max_velocity };
            let t = (v - self.min_velocity) / (self.max_velocity - self.min_velocity);
            let pts = (self.min_score as f32 + t * (self.max_score as f32 - self.min_score as f32) + 0.5) as i32;
            self.score += pts;
            self.popup.delta += pts;
            self.popup.show = self.popup.delta > 0;
            self.popup.stamp_ms = now_ms;
            return pts > 0;
        }
        false
    }
}

/// All counters of one race (`CScoreSystem`, `+4` array, `+8` cached total (-1 = invalid), `+0xc` race cc,
/// `+0x10` max race score).
#[derive(Clone, Debug)]
pub struct ScoreSystem {
    pub cfg: ScoreConfig,
    pub race_cc: i32,
    pub max_race_score: i32,
    pub cached: i32,
    pub position: Option<(bool, i32)>,
    pub time: Option<(bool, i32)>,
    pub fruit: Option<(bool, i32)>,
    pub damage_done: Option<(f32, f32, f32, i32)>,
    pub deaths: Option<DeathsCounter>,
    pub top_speed: Option<PowerCounter>,
    pub acceleration: Option<PowerCounter>,
    pub drift: Option<DistanceCounter>,
    pub in_air: Option<DistanceCounter>,
    pub drafting: Option<DistanceCounter>,
    /// HitBoostPad: (score, per_hit, last_active, popup)
    pub hit_boost_pad: Option<(i32, i32, bool, ScorePopup)>,
    pub impact: Option<ImpactCounter>,
    pub jenga: Option<f32>,
}

impl ScoreSystem {
    /// `CScoreSystem::Init() @001f49cc` + `CScoreSystem()` ctor `@001f41b8`: instantiates the counters named in the
    /// config.
    pub fn new(cfg: ScoreConfig) -> ScoreSystem {
        ScoreSystem {
            race_cc: 0,
            max_race_score: 0,
            cached: -1,
            position: if cfg.has_finishing_position { Some((false, 0)) } else { None },
            time: cfg.time.map(|_| (false, 0)),
            fruit: cfg.fruit.map(|_| (false, 0)),
            damage_done: cfg.damage_done.map(|c| (c[0], c[1], 0.0, 0)),
            deaths: cfg.deaths.map(DeathsCounter::new),
            top_speed: cfg.top_speed.map(PowerCounter::top_speed),
            acceleration: cfg.acceleration.map(PowerCounter::acceleration),
            drift: cfg.drift.map(|c| {
                let mut d = DistanceCounter::new(true);
                d.per_meter = c[0];
                d.min_distance = c[1];
                d
            }),
            in_air: cfg.in_air.map(|c| {
                let mut d = DistanceCounter::new(true);
                d.per_meter = c[0];
                d.min_distance = c[1];
                d
            }),
            drafting: cfg.drafting.map(|c| {
                let mut d = DistanceCounter::new(false);
                d.per_meter = c[0];
                d
            }),
            hit_boost_pad: cfg.hit_boost_pad.map(|s| (0, s, false, ScorePopup::default())),
            impact: cfg.impact.map(ImpactCounter::new),
            jenga: if cfg.has_jenga { Some(0.0) } else { None },
            cfg,
        }
    }

    /// `CScoreSystem::SetRaceCC(int) @001f4258`.
    pub fn set_race_cc(&mut self, cc: i32) {
        self.race_cc = cc;
        self.max_race_score = race_max_score(cc);
    }

    /// `CScoreSystem::Reset() @001f4214`: every counter reset, cache invalidated.
    pub fn reset(&mut self) {
        self.cached = -1;
        if let Some(p) = &mut self.position {
            *p = (false, 0);
        }
        if let Some(p) = &mut self.time {
            *p = (false, 0);
        }
        if let Some(p) = &mut self.fruit {
            *p = (false, 0);
        }
        if let Some(d) = &mut self.damage_done {
            d.2 = 0.0;
            d.3 = 0;
        }
        if let Some(d) = &mut self.deaths {
            d.reset();
        }
        if let Some(d) = &mut self.top_speed {
            d.reset();
        }
        if let Some(d) = &mut self.acceleration {
            d.reset();
        }
        for d in [&mut self.drift, &mut self.in_air, &mut self.drafting].into_iter().flatten() {
            d.reset();
        }
        if let Some(h) = &mut self.hit_boost_pad {
            h.0 = 0;
            h.2 = false;
        }
        if let Some(i) = &mut self.impact {
            i.reset();
        }
        if let Some(j) = &mut self.jenga {
            *j = 0.0;
        }
    }

    /// `CScoreSystem::InvalidateScore() @001f4468`.
    pub fn invalidate(&mut self) {
        self.cached = -1;
    }

    /// `CScoreCounterFinishingPosition::SetPosition(int) @001f1d48`: once, `trunc((9 - pos) / 8 * S)`
    /// (`VectorSignedFixedToFloat(9 - pos, 0x20, 3)`).
    pub fn set_finishing_position(&mut self, pos: i32) {
        let s = self.max_race_score;
        if let Some(p) = &mut self.position {
            if p.0 {
                return;
            }
            p.0 = true;
            p.1 = (((9 - pos) as f32 / 8.0) * s as f32) as i32;
        }
    }

    /// `CScoreCounterFinishingTime::SetTime() @001f230c`: only in `TIME_ATTACK` (6), once. `percent_over` =
    /// `-race_data[0x24] / race_data[0x20]`. FAITHFUL QUIRK (machine code of `001f2354..001f23bc`): the final
    /// interpolation `S*Max + t * (S*Max - S*Max)` subtracts the same product from itself (`MinTimeScore` is never
    /// read), so the result is always `trunc(S * MaxTimeScore)` regardless of the time.
    pub fn set_finishing_time(&mut self, game_mode: i32, percent_over: f32) {
        let s = self.max_race_score as f32;
        let cfg = match self.cfg.time {
            Some(c) => c,
            None => return,
        };
        if let Some(t) = &mut self.time {
            if t.0 || game_mode != GAME_MODE_TIME_ATTACK {
                return;
            }
            let lo = cfg[0];
            let hi = cfg[1];
            // clamp(percent_over, lo, hi) as in the original (value unused by the quirk below)
            let mut v = percent_over;
            if !(lo > v) {
                v = if hi >= v { v } else { hi };
            } else {
                v = lo;
            }
            let _ = v;
            t.0 = true;
            let max = s * cfg[2];
            t.1 = (max + (v / hi) * (max - max)) as i32;
        }
    }

    /// `CScoreCounterFinishingFruit::SetFruitPercent(float) @001f1bcc`: only in `SEED_RUSH` (7), once:
    /// `t = clamp((pct - PercentForMin) / (PercentForMax - PercentForMin), 0, 1)`,
    /// `score = trunc(S*Min + t * (S*Max - S*Min))`.
    pub fn set_fruit_percent(&mut self, game_mode: i32, pct: f32) {
        let s = self.max_race_score as f32;
        let cfg = match self.cfg.fruit {
            Some(c) => c,
            None => return,
        };
        if let Some(f) = &mut self.fruit {
            if f.0 || game_mode != GAME_MODE_SEED_RUSH {
                return;
            }
            let mut t = (pct - cfg[0]) / (cfg[1] - cfg[0]);
            if t < 0.0 {
                t = 0.0;
            } else if t > 1.0 {
                t = 1.0;
            }
            f.0 = true;
            f.1 = (s * cfg[2] + t * (s * cfg[3] - s * cfg[2])) as i32;
        }
    }

    /// `CScoreCounterDamageDone::AddDamageDone(float) @001f0820`: `total += |dmg|`,
    /// `score = (int)(total * ScorePerDamagePoint * PerCCPowerModifier * race_cc)`. (`ScorePerDamagePoint` and the
    /// modifier are 0 in the shipped config, so it stays 0.)
    pub fn add_damage_done(&mut self, dmg: f32) {
        let cc = self.race_cc as f32;
        if let Some(d) = &mut self.damage_done {
            d.2 += dmg.abs();
            d.3 = (d.2 * d.0 * d.1 * cc) as i32;
        }
    }

    /// `CScoreCounterDeaths::AddDeath @001f0d78`.
    pub fn add_death(&mut self) {
        if let Some(d) = &mut self.deaths {
            d.add_death();
        }
    }

    /// `CScoreCounterHitBoostPad::Update(CPlayer*, int active) @001f24a0`: +`Score` on every rising edge of "a wheel
    /// is on a boost pad" (material 0x18 / 0x22).
    pub fn update_hit_boost_pad(&mut self, active: bool, suspended: bool, now_ms: u32) {
        if suspended {
            return;
        }
        if let Some(h) = &mut self.hit_boost_pad {
            if h.2 == active {
                return;
            }
            h.2 = active;
            if active {
                h.0 += h.1;
                h.3.delta += h.1;
                h.3.show = true;
                h.3.stamp_ms = now_ms;
            }
        }
    }

    /// `CScoreCounterJenga::AddScore(float) @001f3590`.
    pub fn add_jenga(&mut self, v: f32) {
        if let Some(j) = &mut self.jenga {
            *j += v;
        }
    }

    /// The per-frame feed `CPlayer::Process @00146bb8` gives the counters (only when `gate` =
    /// `car+0x1b5c == 0 && (player+500 == 0 || car+0x1bec != 0)`): InAir (`active` = no wheel on the ground),
    /// Drafting (`active` = `car+0x438 > 0.5`), TopSpeed, Drift (`active` = `car+0x4c8`), Acceleration.
    pub fn update_player(&mut self, f: &ScoreFrame) {
        if !f.gate {
            return;
        }
        let gm = f.game_mode;
        if is_counter_available(ScoreCounterType::InAir, gm) {
            if let Some(c) = &mut self.in_air {
                c.update(f.wheels_on_ground == 0, f.suspended, f.pos, f.now_ms);
            }
        }
        if is_counter_available(ScoreCounterType::Drafting, gm) {
            if let Some(c) = &mut self.drafting {
                c.update(f.draft > 0.5, f.suspended, f.pos, f.now_ms);
            }
        }
        if is_counter_available(ScoreCounterType::TopSpeed, gm) {
            if let Some(c) = &mut self.top_speed {
                c.update_top_speed(f.dt, f.suspended, f.kart_top_speed, f.speed, f.kart_cc);
            }
        }
        if is_counter_available(ScoreCounterType::Drift, gm) {
            if let Some(c) = &mut self.drift {
                c.update(f.drifting, f.suspended, f.pos, f.now_ms);
            }
        }
        if is_counter_available(ScoreCounterType::Acceleration, gm) {
            if let Some(c) = &mut self.acceleration {
                c.update_acceleration(f.dt, f.suspended, f.drifting, f.kart_cc);
            }
        }
    }

    /// `CScoreSystem::GetScore() @001f4288`: sum of every present counter whose `IsBonusScore()` is false (position,
    /// time and fruit are "bonus" counters), cached until `invalidate`/`reset`. `theme` = `CGame+0x2fc` (0..4, 5 = none).
    pub fn get_score(&mut self, theme: usize) -> i32 {
        if self.cached != -1 {
            return self.cached;
        }
        let max = self.max_race_score;
        let mut total = 0i32;
        if let Some(c) = &mut self.impact {
            total += c.score;
        }
        if let Some(j) = &self.jenga {
            total += *j as i32;
        }
        if let Some(c) = &self.drift {
            total += c.get_score();
        }
        if let Some(c) = &self.in_air {
            total += c.get_score();
        }
        if let Some(c) = &self.drafting {
            total += c.get_score();
        }
        if let Some(c) = &self.hit_boost_pad {
            total += c.0;
        }
        if let Some(c) = &self.damage_done {
            total += c.3;
        }
        if let Some(c) = &mut self.top_speed {
            total += c.get_score(theme, true);
        }
        if let Some(c) = &mut self.acceleration {
            total += c.get_score(theme, false);
        }
        if let Some(c) = &mut self.deaths {
            total += c.get_score(max);
        }
        self.cached = total;
        total
    }

    /// `CScoreSystem::GetBonusScore() @001f43bc`: the finishing counter of the mode (TIME_ATTACK -> time,
    /// SEED_RUSH -> fruit, else position), rounded up to the next 100: `((v + 99) / 100) * 100`.
    pub fn get_bonus_score(&self, game_mode: i32) -> i32 {
        let v = match game_mode {
            GAME_MODE_TIME_ATTACK => self.time.map(|t| t.1),
            GAME_MODE_SEED_RUSH => self.fruit.map(|t| t.1),
            _ => self.position.map(|t| t.1),
        };
        match v {
            Some(v) => ((v + 99) / 100) * 100,
            None => 0,
        }
    }

    /// `CScoreSystem::GetScoreWithStars(int stars) @001f48f4`: `stars * 10000 + GetScore()` (leaderboard value).
    pub fn get_score_with_stars(&mut self, stars: i32, theme: usize) -> i32 {
        stars * 10000 + self.get_score(theme)
    }
}

/// Per-frame input of [`ScoreSystem::update_player`].
#[derive(Clone, Copy, Debug)]
pub struct ScoreFrame {
    pub dt: f32,
    pub now_ms: u32,
    pub game_mode: i32,
    /// `car+0x1b5c == 0 && (player+500 == 0 || car+0x1bec != 0)`.
    pub gate: bool,
    /// `car+0x464 != 0 && car+0x468 >= 0`.
    pub suspended: bool,
    pub pos: Vec3,
    pub wheels_on_ground: u32,
    /// `car+0x438` (draft strength).
    pub draft: f32,
    /// `car+0x4c8 != 0` (drifting).
    pub drifting: bool,
    /// `car+0x1ab4` (speed) and `car+0x1aa8 -> +0x438` (the kart's top speed).
    pub speed: f32,
    pub kart_top_speed: f32,
    /// `CKartManager::GetKartCC(kart, level)`.
    pub kart_cc: i32,
}

// ============================================================================================================
// 7. Challenges (CChallengeManager / CChallenge*)
// ============================================================================================================
//
// Every challenge class of 2.9.1 is an object with an `OnEvent(CChallengeEvent const&)` virtual (`CChallengeManager::Event
// @001f6604` forwards each event to the up to three active challenges of the current set) and an `IsCompletedInternal()`
// virtual. Here each class is a struct whose fields carry the original `this+offset` in their doc comments; the event
// kinds are the 18 `CChallengeEvent*::GetId()` classes (resolved through their GOT slots, see `ChallengeEvent`).
// Flag byte `this+0x10`: bit0 = `Persistent` (progress survives races), bit1 = completed-and-locked, bit4/bit5 are
// manager bookkeeping (bit5 = instance owns its strings).

/// C `atoi` after `SkipWhiteSpaces` (leading integer, 0 when none).
pub fn c_atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    t[..i].parse::<i64>().unwrap_or(0) as i32
}

/// C `strtod` after `SkipWhiteSpaces` (longest numeric prefix, 0.0 when none), narrowed to `f32` like the original.
pub fn c_strtod(s: &str) -> f32 {
    let t = s.trim_start();
    let mut end = 0;
    let mut best = 0.0f64;
    for (i, _) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
        if let Ok(v) = t[..i].parse::<f64>() {
            best = v;
            end = i;
        }
    }
    let _ = end;
    best as f32
}

/// `StringPartialMatchNoCase(text, "true")`: the text starts with `true`, ignoring case.
pub fn c_is_true(s: &str) -> bool {
    s.trim_start().to_ascii_lowercase().starts_with("true")
}

/// What challenge code reads from the player's car (`CPlayer+0x28` = `CCar`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarSnap {
    /// `CCar+0x1b60 != 0`: this car has finished the race.
    pub finished: bool,
    /// `CCar+0x1b80`: finishing race time.
    pub race_time: f32,
    /// `CCar+0x1a9c`: live race position (1 = leading).
    pub position: i32,
    /// `CCar+0x1af8`: final race position.
    pub final_position: i32,
    /// `CCar+0x4c8 != 0`: drifting.
    pub drifting: bool,
    /// `CCar::GetNumWheelsOnGround()`.
    pub wheels_on_ground: i32,
    /// `CCar+0x464 != 0 && CCar+0x468 >= 0`: inside a launcher / scripted section.
    pub suspended: bool,
    /// `rb+0x38..0x40`.
    pub pos: Vec3,
    /// `CCar::GetRaceTotalSplineDist()`.
    pub spline_dist: f32,
    /// `CCar+0x1bec` (bit 0 = the launched car has touched down / started moving).
    pub launch_state: u32,
    /// `CCar+0x1b08`: accumulated damage.
    pub damage: f32,
    /// `dot(spline tangent at the car, car matrix row 0)`; > 0 means driving the right way.
    pub facing_spline_dot: f32,
    /// `CCar::GetVisualDamage(bodywork_head)` (4 = detached).
    pub head_visual_damage: i32,
    /// `|CCar+0x4f8|`: steering input magnitude.
    pub steer_abs: f32,
    /// `CCar+0x438`: draft / slipstream strength (> 0.5 = in the slipstream).
    pub draft: f32,
    /// Car matrix row 0 (forward axis, `CXGSRigidBody::GetMatrix` first row).
    pub forward: Vec3,
}

/// Payload of a `CChallengeEventHit` (`CCar::CollisionCallback @001a0cf8`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitInfo {
    /// `event+8`: the other rigid body (used as identity for de-duplication); `0` = none.
    pub other_id: u32,
    /// `GetPhysicalObject() != null` (body `+0x304`).
    pub has_object: bool,
    /// Physical object `vfunc 8`: 1 = opponent car, 0 = smackable.
    pub is_opponent: bool,
    pub is_smackable: bool,
    /// Smackable type index (`object+0x454`) and its display name (`CSmackableManager::GetSmackableDisplayName`).
    pub smackable_type: i32,
    pub smackable_name: String,
    /// `event+4` car was in the air (`GetNumWheelsOnGround < 1` test of the `InAir` filter).
    pub attacker_wheels_on_ground: Option<i32>,
    /// `event+0x24` (always 0 at the only creation site).
    pub flag_24: bool,
    /// `GetPhysicalObject()` as an identity + kind (for the classes that track the object itself).
    pub object: Option<PhysObj>,
}

/// `CChallengeEvent*` kinds (ids resolved from `CChallengeEvent*::GetId` GOT slots `0x00daa6f0..0x00daa7f8`:
/// base `0xdaa6f0`, Update `f4`, Launch `f8`, Reset `704`, Hit `760`, Respawn `764`, Ability `774`, Destroy `78c`,
/// Boost `7cc`, Pickup `7d0`, NearlyMiss `7d4`, Read `7dc`, Activated `7e0`, Finalize `7e4`, RaceStart `7ec`,
/// RaceQuit `7f0`, RaceRestart `7f4`, RaceFinish `7f8`). `Read` (XML parse) is [`Challenge::from_xml`].
#[derive(Clone, Debug)]
pub enum ChallengeEvent {
    Reset,
    Activated,
    Finalize,
    RaceStart,
    RaceFinish { car: CarSnap },
    RaceQuit { car: CarSnap },
    RaceRestart,
    /// Per-frame (`CPlayer::Process @00146bb8`).
    Update { dt: f32, car: CarSnap },
    /// The player's slingshot launch; `others` = every other car.
    Launch { car: CarSnap, others: Vec<CarSnap> },
    Hit { hit: HitInfo },
    Respawn,
    /// `event+8` = ability id of the character, `event+0xc` = 1 on use / 0 on end.
    Ability { ability: i32, used: bool },
    /// Boost pad touched (`CPickupBoost::OnCarInRadius`); `pad_id` identifies the pad object.
    Boost { pad_id: u32, car: CarSnap },
    /// A pickup was collected; `kind` is the pickup's type.
    /// `car` = the collecting player's car; `token_kind` = seed-rush token kind 0..5 (only meaningful for tokens).
    Pickup { kind: PickupKind, token_kind: u8, car: CarSnap },
    /// `token_kind` is `Some` when the missed object is a seed-rush token.
    NearlyMiss { gap: f32, token_kind: Option<u8> },
    /// `CChallengeEventDestroy`: `object` = the destroyed physical object (`None` = a one-hit destruction without object,
    /// judged by `amount >= 0.9`); `drifting_player` = the local player's car is drifting.
    Destroy { object: Option<PhysObj>, amount: f32, drifting_player: bool },
}

/// Game state the challenge code reads besides the car.
#[derive(Clone, Debug)]
pub struct ChallengeEnv<'a> {
    pub powerups: &'a PlayerPowerups,
    /// `CScoreSystem::GetScore()`.
    pub score: i32,
    /// `CEventDefinitionManager::GetStarsFromScore(score)`.
    pub stars: i32,
    /// `CKartManager::GetKartName(selected kart)`.
    pub kart_name: &'a str,
    /// `game+0x31b8`: number of cars in the race.
    pub num_cars: i32,
    /// `game+0x2ec`: the player's character id (short).
    pub player_character: i16,
    /// Game-mode data `vfunc 0x44` (time left of a timed mode; 0 for modes without one).
    pub mode_time_left: f32,
    /// Positions of every other car (excluding the player), for the "landed near an opponent" test.
    pub others_pos: &'a [Vec3],
    /// Global fruit-rush mode (`< 2` = a fruit rush is running; see [`PickupCtx::fruit_rush_mode`]).
    pub fruit_rush_mode: u32,
    /// Number of plain `pickup_seedrushtoken` objects on the track (`CEnvObjectManager+0x44f0` list scan at race start).
    pub num_seed_tokens: i32,
    /// `CGame::GetGameMode()`.
    pub game_mode: i32,
    /// Fruit-rush bar full (`car->mode_data+0x28 >= 1.0`).
    pub fruit_bar_full: bool,
    /// Names of the destroyable track items (instance exists and `item+0x40 < 1`), scanned at race start by `ChDestroy`.
    pub destroyable_items: &'a [&'a str],
    /// `game+0x2f4`: the selected kart id; `CCar::GetNumOfBrokenWheels` of the player car.
    pub kart_id: i32,
    pub broken_wheels: i32,
}

/// Which counter an event bumped, for the HUD ticks (`CChallenge::GetScaleAnimTime` etc. are UI; not ported).
fn child_text<'a>(n: &roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == tag).and_then(|c| c.text())
}
fn rd_i32(n: &roxmltree::Node, tag: &str, dst: &mut i32) {
    if let Some(t) = child_text(n, tag) {
        *dst = c_atoi(t);
    }
}
fn rd_f32(n: &roxmltree::Node, tag: &str, dst: &mut f32) {
    if let Some(t) = child_text(n, tag) {
        *dst = c_strtod(t);
    }
}
fn rd_bool(n: &roxmltree::Node, tag: &str, dst: &mut bool) {
    if let Some(t) = child_text(n, tag) {
        *dst = c_is_true(t);
    }
}

/// Completed + restore helper shared by the "RaceFinish without finishing" branches.
fn car_finished(car: &CarSnap) -> bool {
    car.finished
}

// ---- CChallengeRaceTime @0020512c / OnEvent @001fee08 --------------------------------------------------------
/// `<ChallengeRaceTime><Time/><CountTimeLeft/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChRaceTime {
    /// `+0x14` (1.0 once the race was finished, else 0).
    pub done: f32,
    /// `+0x18` result: race time, or the time left when `count_time_left`.
    pub result: f32,
    /// `+0x1c` `Time`.
    pub limit: f32,
    /// `+0x20` `CountTimeLeft`.
    pub count_time_left: bool,
}
impl ChRaceTime {
    pub fn read(n: &roxmltree::Node) -> ChRaceTime {
        let mut c = ChRaceTime::default();
        rd_f32(n, "Time", &mut c.limit);
        rd_bool(n, "CountTimeLeft", &mut c.count_time_left);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::RaceStart | ChallengeEvent::Reset => self.done = 0.0,
            ChallengeEvent::RaceFinish { car } => {
                if !car_finished(car) {
                    return;
                }
                self.done = 1.0;
                self.result = if self.count_time_left { env.mode_time_left } else { car.race_time };
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001f9740`.
    pub fn is_completed(&self) -> bool {
        let (a, b) = if self.count_time_left { (self.result, self.limit) } else { (self.limit, self.result) };
        if self.done == 0.0 {
            false
        } else {
            a >= b
        }
    }
}

// ---- CChallengeScore @002052e8 / OnEvent @001fefc0 ------------------------------------------------------------
/// `<ChallengeScore><Score/><Finish/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChScore {
    /// `+0x14` score at race start (only refreshed for non-persistent ones).
    pub start_score: i32,
    /// `+0x18` finished flag.
    pub finished: bool,
    /// `+0x1c` `Score` target.
    pub target: i32,
    /// `+0x20` `Finish`: the race must be finished.
    pub need_finish: bool,
}
impl ChScore {
    pub fn read(n: &roxmltree::Node) -> ChScore {
        let mut c = ChScore::default();
        rd_i32(n, "Score", &mut c.target);
        rd_bool(n, "Finish", &mut c.need_finish);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.start_score = env.score;
                }
                self.finished = false;
            }
            ChallengeEvent::RaceFinish { car } => self.finished = car_finished(car),
            ChallengeEvent::Activated | ChallengeEvent::Reset => {
                self.finished = false;
                self.start_score = env.score;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001f9e00`.
    pub fn is_completed(&self, env: &ChallengeEnv) -> bool {
        if self.need_finish && !self.finished {
            return false;
        }
        self.target <= env.score
    }
}

// ---- CChallengeCollectCoins @00204a14 / OnEvent @00200bd8 ------------------------------------------------------
/// `<ChallengeCollectCoins><Amount/><DoNotPick/><InAir/><Drifting/><Maximum/><Less/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChCollectCoins {
    /// `+0x14` coins counted, `+0x18` saved at race start.
    pub count: i32,
    pub saved: i32,
    /// `+0x1c` bit0 = in air now, bit1 = "no coin taken" at finish, bit2 = drifting now, bit3 = race finished.
    pub bits: u8,
    pub amount: i32,
    pub do_not_pick: bool,
    pub in_air: bool,
    pub drifting: bool,
    pub maximum: bool,
    pub less: bool,
}
impl ChCollectCoins {
    pub fn read(n: &roxmltree::Node) -> ChCollectCoins {
        let mut c = ChCollectCoins::default();
        rd_i32(n, "Amount", &mut c.amount);
        rd_bool(n, "DoNotPick", &mut c.do_not_pick);
        rd_bool(n, "InAir", &mut c.in_air);
        rd_bool(n, "Drifting", &mut c.drifting);
        rd_bool(n, "Maximum", &mut c.maximum);
        rd_bool(n, "Less", &mut c.less);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool) {
        match ev {
            ChallengeEvent::Update { car, .. } => {
                if car.suspended {
                    return;
                }
                let air = car.wheels_on_ground == 0;
                self.bits = (self.bits & 0xfa) | (air as u8) | ((car.drifting as u8) << 2);
            }
            ChallengeEvent::Pickup { kind, .. } => {
                if self.in_air && (self.bits & 1) == 0 {
                    return;
                }
                if self.drifting && (self.bits & 4) == 0 {
                    return;
                }
                // only `pickup_coin` (type slot 0xdaa664) counts
                if *kind == PickupKind::Coin {
                    self.count += 1;
                }
            }
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.count = 0;
                }
                self.saved = self.count;
                self.bits &= 0xf0;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car_finished(car) {
                    self.bits = (self.bits & 0xfd) | (((self.count == self.saved) as u8) << 1) | 8;
                } else {
                    self.count = self.saved;
                }
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => self.count = self.saved,
            ChallengeEvent::Reset => self.count = 0,
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fa3e4`.
    pub fn is_completed(&self) -> bool {
        if self.do_not_pick {
            return ((self.bits & 3) >> 1) != 0;
        }
        if !self.maximum {
            if !self.less {
                return self.amount <= self.count;
            }
            return (self.bits & 8) != 0 && self.count < self.amount;
        }
        (self.bits & 8) != 0 && self.count <= self.amount
    }
}

// ---- CChallengeDrift @00205910 / OnEvent @0020200c ------------------------------------------------------------
/// `<ChallengeDrift><Distance/><Continuous/><FinishLineDrift/><Count/><MostlyDrifting/>`
#[derive(Clone, Debug, PartialEq)]
pub struct ChDrift {
    /// `+0x14` drift distance (m), `+0x18` saved.
    pub distance: f32,
    pub saved_distance: f32,
    /// `+0x1c` spline distance at the start of the current drift (-1 = not drifting).
    pub start_dist: f32,
    /// `+0x20` drift count, `+0x24` saved.
    pub count: i32,
    pub saved_count: i32,
    /// `+0x28` drifting at the finish line, `+0x2c` race finished, `+0x30` race time, `+0x34` time spent drifting.
    pub drift_at_finish: bool,
    pub finished: bool,
    pub total_time: f32,
    pub drift_time: f32,
    pub target_distance: f32,
    /// `Continuous` (default true, ctor `@00205910`).
    pub continuous: bool,
    pub finish_line_drift: bool,
    pub target_count: i32,
    pub mostly_drifting: bool,
}
impl Default for ChDrift {
    fn default() -> Self {
        ChDrift {
            distance: 0.0,
            saved_distance: 0.0,
            start_dist: -1.0,
            count: 0,
            saved_count: 0,
            drift_at_finish: false,
            finished: false,
            total_time: 0.0,
            drift_time: 0.0,
            target_distance: 0.0,
            continuous: true,
            finish_line_drift: false,
            target_count: 0,
            mostly_drifting: false,
        }
    }
}
impl ChDrift {
    pub fn read(n: &roxmltree::Node) -> ChDrift {
        let mut c = ChDrift::default();
        rd_f32(n, "Distance", &mut c.target_distance);
        rd_bool(n, "Continuous", &mut c.continuous);
        rd_bool(n, "FinishLineDrift", &mut c.finish_line_drift);
        rd_i32(n, "Count", &mut c.target_count);
        rd_bool(n, "MostlyDrifting", &mut c.mostly_drifting);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                if !self.finished {
                    self.total_time += dt;
                }
                if !car.drifting {
                    if self.start_dist >= 0.0 {
                        let d = car.spline_dist - self.start_dist;
                        if !self.continuous {
                            self.distance += d;
                        } else if self.distance < d {
                            self.distance = d;
                        }
                        self.start_dist = -1.0;
                    }
                } else {
                    if self.start_dist < 0.0 {
                        self.count += 1;
                        self.start_dist = car.spline_dist;
                    }
                    self.drift_time += dt;
                }
            }
            ChallengeEvent::RaceStart => {
                self.start_dist = -1.0;
                if !persistent {
                    self.count = 0;
                    self.distance = 0.0;
                }
                self.saved_count = self.count;
                self.saved_distance = self.distance;
                self.drift_at_finish = false;
                self.finished = false;
                self.total_time = 0.0;
                self.drift_time = 0.0;
            }
            ChallengeEvent::RaceFinish { car } => {
                self.finished = true;
                if !car_finished(car) {
                    self.distance = self.saved_distance;
                    self.count = self.saved_count;
                }
                if self.finish_line_drift {
                    self.drift_at_finish = car.drifting;
                }
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => {
                self.distance = self.saved_distance;
                self.count = self.saved_count;
            }
            ChallengeEvent::Reset => {
                self.count = 0;
                self.drift_at_finish = false;
                self.finished = false;
                self.distance = 0.0;
                self.start_dist = -1.0;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fa670`.
    pub fn is_completed(&self) -> bool {
        if self.mostly_drifting {
            if self.total_time <= 0.0 {
                return false;
            }
            if self.finish_line_drift && !self.drift_at_finish {
                return false;
            }
            return 0.5 < self.drift_time / self.total_time;
        }
        if self.finish_line_drift {
            return self.drift_at_finish;
        }
        if self.target_count < 1 {
            return self.target_distance <= self.distance;
        }
        self.target_count <= self.count
    }
}

// ---- CChallengeUseBoostPad @00205848 / OnEvent @00200f98 ----------------------------------------------------------
/// `<ChallengeUseBoostPad><Count/><Overtake/><MaximumLimit/><ExactCount/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChUseBoostPad {
    /// `+0x14` pads used, `+0x18` saved; `+0x20` places gained inside boost windows, `+0x24` saved.
    pub count: i32,
    pub saved_count: i32,
    pub overtakes: i32,
    pub saved_overtakes: i32,
    /// `+0x1c` race position when the pad was hit.
    pub position_at_hit: i32,
    /// `+0x28` seconds left of the 1 s boost window, `+0x2c` finished, `+0x30` last pad id.
    pub window: f32,
    pub finished: bool,
    pub last_pad: u32,
    pub target_count: i32,
    pub target_overtake: i32,
    pub maximum_limit: bool,
    pub exact_count: bool,
}
impl ChUseBoostPad {
    pub fn read(n: &roxmltree::Node) -> ChUseBoostPad {
        let mut c = ChUseBoostPad::default();
        rd_i32(n, "Count", &mut c.target_count);
        rd_i32(n, "Overtake", &mut c.target_overtake);
        rd_bool(n, "MaximumLimit", &mut c.maximum_limit);
        rd_bool(n, "ExactCount", &mut c.exact_count);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                if self.window <= 0.0 {
                    return;
                }
                if *dt <= self.window {
                    self.window -= dt;
                    return;
                }
                if 0 < self.target_overtake && self.overtakes < self.target_overtake {
                    let gained = self.position_at_hit - car.position;
                    let mut o = self.overtakes;
                    if -1 < gained {
                        o += gained;
                    }
                    self.overtakes = o;
                }
                self.window = 0.0;
            }
            ChallengeEvent::Boost { pad_id, car } => {
                if self.window <= 0.0 && *pad_id != self.last_pad {
                    self.count += 1;
                    self.position_at_hit = car.position;
                    self.last_pad = *pad_id;
                }
                self.window = 1.0;
            }
            ChallengeEvent::RaceStart => {
                self.window = 0.0;
                if !persistent {
                    self.count = 0;
                    self.overtakes = 0;
                }
                self.saved_count = self.count;
                self.saved_overtakes = self.overtakes;
                self.last_pad = 0;
                self.finished = false;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car_finished(car) {
                    self.finished = true;
                    return;
                }
                self.count = self.saved_count;
                self.overtakes = self.saved_overtakes;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => {
                self.count = self.saved_count;
                self.overtakes = self.saved_overtakes;
            }
            ChallengeEvent::Reset => {
                self.count = 0;
                self.overtakes = 0;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fa5d4`.
    pub fn is_completed(&self) -> bool {
        if self.target_overtake >= 1 {
            return self.overtakes >= self.target_overtake;
        }
        if self.exact_count {
            return self.finished && self.count == self.target_count;
        }
        if self.maximum_limit {
            return self.finished && self.count <= self.target_count;
        }
        self.target_count <= self.count
    }
}

// ---- CChallengeUsePowerUp @00205780 / OnEvent @0020375c ---------------------------------------------------------
/// `<ChallengeUsePowerUp><Count/><Ability/><UseInRow/><UseDifferent/><Maximum/><UseAllDiffRaces/>`
#[derive(Clone, Debug, PartialEq)]
pub struct ChUsePowerUp {
    /// `+0x14` count, `+0x18` saved, `+0x1c` races in which all four were used.
    pub count: i32,
    pub saved: i32,
    pub all_four_races: i32,
    /// `+0x20..0x2c`: power-up already counted (UseDifferent).
    pub used: [bool; 4],
    pub target: i32,
    /// `+0x38` specific power-up (-1 = any; resolved by `CPlayerInfo::GetPowerupEnumByText(Ability)`).
    pub powerup: i32,
    pub use_in_row: bool,
    pub use_different: bool,
    pub maximum: bool,
    pub use_all_diff_races: bool,
    /// `+0x4c` race finished.
    pub finished: bool,
}
impl Default for ChUsePowerUp {
    fn default() -> Self {
        ChUsePowerUp { count: 0, saved: 0, all_four_races: 0, used: [false; 4], target: 0, powerup: -1, use_in_row: false, use_different: false, maximum: false, use_all_diff_races: false, finished: false }
    }
}
impl ChUsePowerUp {
    pub fn read(n: &roxmltree::Node) -> ChUsePowerUp {
        let mut c = ChUsePowerUp::default();
        rd_i32(n, "Count", &mut c.target);
        // `strncpy(local, GetText("Ability"), 0x1f)` then GetPowerupEnumByText (-1 when absent / unknown)
        let name = child_text(n, "Ability").unwrap_or("");
        c.powerup = powerup_enum_by_text(name);
        rd_bool(n, "UseInRow", &mut c.use_in_row);
        rd_bool(n, "UseDifferent", &mut c.use_different);
        rd_bool(n, "Maximum", &mut c.maximum);
        rd_bool(n, "UseAllDiffRaces", &mut c.use_all_diff_races);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, env: &ChallengeEnv) {
        let pu = env.powerups;
        match ev {
            ChallengeEvent::Launch { .. } => {
                if self.use_all_diff_races {
                    let n = (0..4).filter(|i| pu.is_active(*i)).count();
                    if n != 4 {
                        return;
                    }
                    self.all_four_races += 1;
                    return;
                }
                if !self.use_different {
                    let any = if self.powerup == -1 { (0..4).any(|i| pu.is_active(i)) } else { pu.is_active(self.powerup) };
                    if any {
                        self.count += 1;
                        return;
                    }
                    if !self.use_in_row {
                        return;
                    }
                    self.count = 0;
                    return;
                }
                let mut fresh = 0;
                for i in (0..4usize).rev() {
                    if pu.is_active(i as i32) && !self.used[i] {
                        self.used[i] = true;
                        fresh += 1;
                    }
                }
                if !self.use_in_row || fresh != 0 {
                    if !persistent {
                        self.count += fresh;
                        return;
                    }
                    if fresh == 0 {
                        return;
                    }
                    self.count += 1;
                    return;
                }
                self.count = 0;
            }
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.count = 0;
                    self.used = [false; 4];
                }
                self.saved = self.count;
                self.finished = false;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car_finished(car) {
                    self.finished = true;
                    return;
                }
                self.count = self.saved;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => self.count = self.saved,
            ChallengeEvent::Reset => {
                self.count = 0;
                self.saved = 0;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fa564`.
    pub fn is_completed(&self) -> bool {
        if self.use_all_diff_races {
            return self.target <= self.all_four_races;
        }
        if !self.maximum {
            return self.target <= self.count;
        }
        if self.finished {
            return self.count <= self.target;
        }
        false
    }
}

// ---- CChallengeUseAbility @002056a8 / OnEvent @001fe8dc -----------------------------------------------------------
/// `<ChallengeUseAbility><Ability/><Count/><UseDifferent/><DoNotUse/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChUseAbility {
    /// `+0x14` uses, `+0x18` saved; `+0x1c..0x2c` the abilities seen (UseDifferent); `+0x30` race finished.
    pub count: i32,
    pub saved: i32,
    pub seen: [i32; 5],
    pub finished: bool,
    /// `+0x34` ability id wanted (0 = any); `CBaseAbility::GetBirdAbilityFromString` of `Ability`.
    pub ability: i32,
    pub target: i32,
    pub use_different: bool,
    pub do_not_use: bool,
}
impl ChUseAbility {
    /// `resolve` = `CBaseAbility::GetBirdAbilityFromString` (abilities.rs).
    pub fn read(n: &roxmltree::Node, resolve: &dyn Fn(&str) -> i32) -> ChUseAbility {
        let mut c = ChUseAbility::default();
        c.ability = resolve(child_text(n, "Ability").unwrap_or(""));
        rd_i32(n, "Count", &mut c.target);
        rd_bool(n, "UseDifferent", &mut c.use_different);
        rd_bool(n, "DoNotUse", &mut c.do_not_use);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool) {
        match ev {
            ChallengeEvent::Ability { ability, used } => {
                if !*used {
                    return;
                }
                if !self.use_different {
                    if self.ability == 0 || self.ability == *ability {
                        self.count += 1;
                    }
                } else {
                    let c = self.count;
                    let id = *ability;
                    if c < 5
                        && (c < 1
                            || (self.seen[0] != id
                                && (c == 1 || (self.seen[1] != id && (c == 2 || (self.seen[2] != id && (c != 4 || self.seen[3] != id)))))))
                    {
                        self.count = c + 1;
                        self.seen[c as usize] = id;
                    }
                }
            }
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.count = 0;
                }
                self.finished = false;
                self.saved = self.count;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car_finished(car) {
                    self.finished = true;
                } else {
                    self.count = self.saved;
                }
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => {
                self.finished = false;
                self.count = self.saved;
            }
            ChallengeEvent::Reset => {
                self.count = 0;
                self.saved = 0;
                self.finished = false;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fa520`.
    pub fn is_completed(&self) -> bool {
        if !self.do_not_use {
            return self.target <= self.count;
        }
        self.finished && self.count == 0
    }
}

// ---- CChallengeWinWithKart @00206144 / OnEvent @001fe6cc -----------------------------------------------------------
/// `<ChallengeWinWithKart><Count/><Stars/><Kart/>`
#[derive(Clone, Debug, PartialEq)]
pub struct ChWinWithKart {
    pub count: i32,
    pub won_now: bool,
    pub kart_matches: bool,
    /// `+0x20` `Count` (default 1), `+0x24` `Stars`, `+0x28` `Kart` (32 bytes).
    pub target: i32,
    pub stars: i32,
    pub kart: String,
}
impl Default for ChWinWithKart {
    fn default() -> Self {
        ChWinWithKart { count: 0, won_now: false, kart_matches: false, target: 1, stars: 0, kart: String::new() }
    }
}
fn name32(s: &str) -> String {
    s.chars().take(0x20).collect()
}
impl ChWinWithKart {
    pub fn read(n: &roxmltree::Node) -> ChWinWithKart {
        let mut c = ChWinWithKart::default();
        rd_i32(n, "Count", &mut c.target);
        rd_i32(n, "Stars", &mut c.stars);
        if let Some(t) = child_text(n, "Kart") {
            c.kart = name32(t);
        }
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::RaceStart => {
                self.won_now = false;
                self.kart_matches = name32(env.kart_name) == self.kart;
            }
            ChallengeEvent::RaceFinish { car } if self.kart_matches => {
                let mut won = ((car.final_position - 1) as u32) < 3;
                if self.stars > 0 && env.stars < self.stars {
                    won = false;
                }
                self.won_now = won;
                if won {
                    self.count += 1;
                }
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fa724`.
    pub fn is_completed(&self) -> bool {
        self.kart_matches && self.won_now && self.target <= self.count
    }
}

// ---- CChallengeWinWithDifferentKarts @00206268 / OnEvent @001fec20 ----------------------------------------------
/// `<ChallengeWinWithDifferentKarts><NumKarts/><Stars/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChWinWithDifferentKarts {
    /// `+0x14` different karts that won, `+0x18..` the names (5 x 32 bytes).
    pub count: i32,
    pub names: Vec<String>,
    pub num_karts: i32,
    pub stars: i32,
}
impl ChWinWithDifferentKarts {
    pub fn read(n: &roxmltree::Node) -> ChWinWithDifferentKarts {
        let mut c = ChWinWithDifferentKarts::default();
        rd_i32(n, "NumKarts", &mut c.num_karts);
        rd_i32(n, "Stars", &mut c.stars);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv) {
        if let ChallengeEvent::RaceFinish { car } = ev {
            if !(((car.final_position - 1) as u32) < 3) {
                return;
            }
            if !((self.stars < 1 || self.stars <= env.stars) && 0 < self.num_karts) {
                return;
            }
            let cur = name32(env.kart_name);
            let mut i = 0usize;
            if !self.names.is_empty() {
                loop {
                    let eq = self.names.get(i).map(|s| *s == cur).unwrap_or(false);
                    i += 1;
                    if eq {
                        return;
                    }
                    if self.num_karts <= i as i32 {
                        return;
                    }
                    if i >= self.names.len() {
                        break;
                    }
                }
            }
            self.count += 1;
            if i < self.names.len() {
                self.names[i] = cur;
            } else {
                self.names.push(cur);
            }
        }
    }
    /// `IsCompletedInternal @001f9898`.
    pub fn is_completed(&self) -> bool {
        self.count == self.num_karts
    }
}

// ---- CChallengeJump @002051e8 / OnEvent @00201310 ---------------------------------------------------------------
/// `<ChallengeJump><Length/><Count/><Duration/><MostlyInAir/>`
#[derive(Clone, Debug, PartialEq)]
pub struct ChJump {
    /// `+0x14` longest jump (m), `+0x18` last jump; `+0x1c..0x24` take-off position.
    pub longest: f32,
    pub last: f32,
    pub takeoff: Vec3,
    /// `+0x28` jumps longer than `Length`, `+0x2c` saved.
    pub jumps: i32,
    pub saved_jumps: i32,
    /// `+0x30` current air time, `+0x34` longest air time, `+0x38` saved.
    pub air_time: f32,
    pub best_air_time: f32,
    pub saved_air_time: f32,
    /// `+0x3c` bit0 airborne, bit1 launched, bit2 landed after launch, bit3 saw airborne, bit4 finished.
    pub bits: u8,
    /// `+0x40` race time since launch, `+0x44` time spent in the air.
    pub time_since_launch: f32,
    pub time_in_air: f32,
    pub length: f32,
    pub count: i32,
    pub duration: f32,
    pub mostly_in_air: bool,
}
impl Default for ChJump {
    fn default() -> Self {
        ChJump {
            longest: 0.0,
            last: 0.0,
            takeoff: Vec3::ZERO,
            jumps: 0,
            saved_jumps: 0,
            air_time: 0.0,
            best_air_time: 0.0,
            saved_air_time: 0.0,
            bits: 0,
            time_since_launch: 0.0,
            time_in_air: 0.0,
            length: -1.0,
            count: 0,
            duration: 0.0,
            mostly_in_air: false,
        }
    }
}
impl ChJump {
    pub fn read(n: &roxmltree::Node) -> ChJump {
        let mut c = ChJump::default();
        rd_f32(n, "Length", &mut c.length);
        rd_i32(n, "Count", &mut c.count);
        rd_f32(n, "Duration", &mut c.duration);
        rd_bool(n, "MostlyInAir", &mut c.mostly_in_air);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                if self.bits & 2 == 0 {
                    return;
                }
                if self.bits & 4 == 0 {
                    // wait for the launched car to touch down once
                    if self.bits & 8 != 0 {
                        self.bits = (self.bits & 0xfb) | (((car.wheels_on_ground > 0) as u8) << 2);
                        return;
                    }
                    self.bits = (self.bits & 0xf7) | (((car.wheels_on_ground == 0) as u8) << 3);
                    return;
                }
                if car.wheels_on_ground < 1 {
                    if self.bits & 1 == 0 {
                        self.takeoff = car.pos;
                    }
                    self.bits |= 1;
                } else {
                    if self.bits & 1 != 0 {
                        let d = (car.pos - self.takeoff).length();
                        self.last = d;
                        if self.length < d {
                            self.jumps += 1;
                        }
                        if self.longest <= d {
                            self.longest = d;
                        }
                    }
                    self.bits &= 0xfe;
                }
                let airborne = self.bits & 1 != 0;
                if !airborne || self.duration <= 0.0 {
                    if !self.mostly_in_air {
                        return;
                    }
                    if self.bits & 0x10 != 0 {
                        return;
                    }
                    self.time_since_launch += dt;
                    if !airborne {
                        return;
                    }
                } else {
                    if self.bits & 0x10 != 0 {
                        return;
                    }
                    if self.count < 1 {
                        self.air_time += dt;
                        if self.best_air_time <= self.air_time {
                            self.best_air_time = self.air_time;
                        }
                    } else if self.duration <= self.air_time {
                        self.air_time = 0.0;
                        self.jumps += 1;
                    }
                    if !self.mostly_in_air {
                        return;
                    }
                    self.time_since_launch += dt;
                }
                self.time_in_air += dt;
            }
            ChallengeEvent::Launch { .. } => self.bits |= 2,
            ChallengeEvent::RaceStart => {
                let mut sa = 0.0;
                if !persistent {
                    self.jumps = 0;
                } else {
                    sa = self.air_time;
                }
                if !persistent {
                    self.air_time = 0.0;
                }
                self.saved_jumps = self.jumps;
                self.saved_air_time = if persistent { sa } else { 0.0 };
                self.bits &= 0xe0;
                self.longest = 0.0;
                self.last = 0.0;
                self.best_air_time = 0.0;
                self.time_since_launch = 0.0;
                self.time_in_air = 0.0;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car_finished(car) {
                    self.bits |= 0x10;
                    return;
                }
                self.restore();
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => self.restore(),
            ChallengeEvent::Reset => {
                self.longest = 0.0;
                self.jumps = 0;
                self.best_air_time = 0.0;
                self.air_time = 0.0;
            }
            _ => {}
        }
    }
    fn restore(&mut self) {
        self.longest = 0.0;
        self.jumps = self.saved_jumps;
        self.best_air_time = 0.0;
        self.air_time = self.saved_air_time;
    }
    /// `IsCompletedInternal @001fb574` (`persistent` = flag bit0).
    pub fn is_completed(&self, persistent: bool) -> bool {
        if self.mostly_in_air {
            if self.bits & 0x10 == 0 {
                return false;
            }
            return self.time_since_launch * 0.5 < self.time_in_air;
        }
        if self.count < 1 {
            if self.duration <= 0.0 {
                return self.length <= self.longest;
            }
            let t = if !persistent { self.best_air_time } else { self.air_time };
            return self.duration <= t;
        }
        self.count <= self.jumps
    }
}

// ---- CChallengeLaunch @002059f4 / OnEvent @002024b8 -------------------------------------------------------------
/// `<ChallengeLaunch><Position/><Count/><Distance/><TestLandingPosition/><LandBehindOpponent/><InRow/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChLaunch {
    /// `+0x14` position at launch (or landing), `+0x18` count of good launches, `+0x1c` saved.
    pub position: i32,
    pub good: i32,
    pub saved_good: i32,
    /// `+0x20` distance of the launch, `+0x24..0x2c` launch position.
    pub distance: f32,
    pub launch_pos: Vec3,
    /// `+0x30` launched, `+0x34` landed, `+0x38` "landed behind an opponent" achieved.
    pub launched: bool,
    pub landed: bool,
    pub behind_ok: bool,
    pub max_position: i32,
    pub count: i32,
    pub min_distance: f32,
    pub test_landing_position: bool,
    pub land_behind_opponent: bool,
    pub in_row: bool,
}
impl ChLaunch {
    pub fn read(n: &roxmltree::Node) -> ChLaunch {
        let mut c = ChLaunch::default();
        rd_i32(n, "Position", &mut c.max_position);
        rd_i32(n, "Count", &mut c.count);
        rd_f32(n, "Distance", &mut c.min_distance);
        rd_bool(n, "TestLandingPosition", &mut c.test_landing_position);
        rd_bool(n, "LandBehindOpponent", &mut c.land_behind_opponent);
        rd_bool(n, "InRow", &mut c.in_row);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::Update { car, .. } => {
                if !(self.launched && !self.landed) {
                    return;
                }
                let land = car.launch_state != 0;
                self.landed = land;
                if !land {
                    return;
                }
                self.distance = (car.pos - self.launch_pos).length();
                if !self.test_landing_position {
                    if self.land_behind_opponent {
                        for op in env.others_pos.iter() {
                            let d = *op - car.pos;
                            let d2 = d.length_squared();
                            if 16.0 <= d2 || (1.0 / d2.sqrt()) * d.dot(car.forward) < 0.707 {
                                continue;
                            }
                            self.behind_ok = true;
                            break;
                        }
                    }
                } else {
                    let pos = car.position;
                    if pos <= self.max_position {
                        self.good += 1;
                    }
                    if self.in_row && pos != self.position && self.position != 0 {
                        self.good = self.saved_good;
                    }
                    self.position = pos;
                }
            }
            ChallengeEvent::Launch { car, others } => {
                if env.num_cars > 1 && !self.test_landing_position {
                    let mut n = 1;
                    for o in others.iter() {
                        if !o.suspended {
                            n += 1;
                        }
                    }
                    self.position = n;
                    if n <= self.max_position {
                        self.good += 1;
                    }
                }
                if self.in_row && self.position > 1 {
                    self.good = 0;
                }
                self.launched = true;
                self.launch_pos = car.pos;
            }
            ChallengeEvent::RaceStart => {
                self.position = 0;
                self.launched = false;
                self.distance = 0.0;
                self.saved_good = self.good;
                self.landed = false;
                self.behind_ok = false;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car_finished(car) {
                    return;
                }
                self.distance = 0.0;
                self.good = self.saved_good;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => {
                self.distance = 0.0;
                self.good = self.saved_good;
            }
            ChallengeEvent::Reset => {
                self.good = 0;
                self.launched = false;
                self.landed = false;
                self.distance = 0.0;
                self.behind_ok = false;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001f9784`.
    pub fn is_completed(&self) -> bool {
        if !self.launched {
            return false;
        }
        if self.land_behind_opponent {
            return self.behind_ok;
        }
        if self.min_distance <= 0.0 {
            if self.count < 1 {
                if !self.test_landing_position {
                    return self.position <= self.max_position;
                }
                return self.landed && self.position <= self.max_position;
            }
            return self.count <= self.good;
        }
        self.landed && self.min_distance <= self.distance
    }
}

fn partial_match_nocase(a: &str, b: &str) -> bool {
    a.to_ascii_lowercase().starts_with(&b.to_ascii_lowercase())
}

// ---- CChallengeHit @002053fc / OnEvent @00207e00 ----------------------------------------------------------------
/// `<ChallengeHit><Hits/><BodiesToHit/><UseAbility/><LaunchHit/><FinishRace/><FinishFirst/><InAir/><SmackableType/>`
/// plus child elements = smackable helper names (at most 6, resolved with `CSmackableManager::GetSmackableTypeFromHelperName`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChHit {
    /// `+0x14` hits, `+0x18` saved.
    pub count: i32,
    pub saved: i32,
    /// `+0x1c..0x6b`: 10 `{time_ms @+0x1c+8i, body id @+0x20+8i}` slots of recently hit bodies.
    pub slots: [(u32, u32); 10],
    /// `+0x6c` number of different bodies hit.
    pub distinct: i16,
    /// `+0x70` ability active, `+0x74` launched, `+0x78` landed (`car+0x1bec`), `+0x7c` running, `+0x80` finished.
    pub ability: bool,
    pub launched: bool,
    pub landed: u32,
    pub running: bool,
    pub finished: bool,
    /// `+0x84` final position.
    pub final_position: i16,
    pub hits: i32,
    pub bodies_to_hit: i32,
    pub use_ability: bool,
    pub launch_hit: bool,
    pub finish_race: bool,
    pub finish_first: bool,
    pub in_air: bool,
    /// `+0xa4` `SmackableType` filter text; `+0xc4` smackable type ids (`+0xdc` count).
    pub smackable_filter: String,
    pub types: Vec<i32>,
}
impl ChHit {
    pub fn read(n: &roxmltree::Node, resolve_smackable: &dyn Fn(&str) -> u32) -> ChHit {
        let mut c = ChHit::default();
        rd_i32(n, "Hits", &mut c.hits);
        rd_i32(n, "BodiesToHit", &mut c.bodies_to_hit);
        rd_bool(n, "UseAbility", &mut c.use_ability);
        rd_bool(n, "LaunchHit", &mut c.launch_hit);
        rd_bool(n, "FinishRace", &mut c.finish_race);
        rd_bool(n, "FinishFirst", &mut c.finish_first);
        rd_bool(n, "InAir", &mut c.in_air);
        if let Some(t) = child_text(n, "SmackableType") {
            c.smackable_filter = name32(t);
        }
        // `GetFirstChild` + siblings: every element child (the loop reads each node's own text)
        for ch in n.children().filter(|x| x.is_element()) {
            if c.types.len() >= 6 {
                break;
            }
            let id = resolve_smackable(ch.text().unwrap_or(""));
            if id < 0x7e {
                c.types.push(id as i32);
            }
        }
        c
    }

    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, now_ms: u32) {
        match ev {
            ChallengeEvent::Update { car, .. } => {
                if self.running && self.launched && self.landed == 0 {
                    self.landed = car.launch_state;
                }
            }
            ChallengeEvent::Hit { hit } => {
                if !(self.running && self.launched) {
                    return;
                }
                if self.use_ability && !(hit.flag_24 || self.ability) {
                    return;
                }
                if self.launch_hit && self.landed != 0 {
                    return;
                }
                if self.in_air {
                    if let Some(w) = hit.attacker_wheels_on_ground {
                        if w >= 1 {
                            return;
                        }
                    }
                }
                if !hit.has_object {
                    return;
                }
                if self.types.is_empty() {
                    if self.smackable_filter.is_empty() {
                        if !hit.is_opponent {
                            return;
                        }
                    } else {
                        if hit.is_opponent {
                            return;
                        }
                        if !partial_match_nocase(&hit.smackable_name, &self.smackable_filter) {
                            return;
                        }
                    }
                } else {
                    if hit.is_opponent {
                        return;
                    }
                    if !self.types.contains(&hit.smackable_type) {
                        return;
                    }
                }
                self.count += 1;
                // find the body in the recent list, else take an empty / the oldest slot
                let mut pick: Option<usize> = None;
                for i in (0..10).rev() {
                    let (t, id) = self.slots[i];
                    if id != 0 {
                        if id == hit.other_id {
                            self.slots[i].0 = now_ms;
                            return;
                        }
                        if let Some(p) = pick {
                            if self.slots[p].1 != 0 && t < self.slots[p].0 {
                                pick = Some(i);
                            }
                        } else {
                            pick = Some(i);
                        }
                    } else {
                        pick = Some(i);
                    }
                }
                if let Some(p) = pick {
                    self.slots[p] = (now_ms, hit.other_id);
                }
                self.distinct += 1;
            }
            ChallengeEvent::Ability { used, .. } => self.ability = *used,
            ChallengeEvent::Launch { .. } => self.launched = true,
            ChallengeEvent::RaceStart => {
                self.running = true;
                if !persistent {
                    self.count = 0;
                }
                self.distinct = 0;
                self.ability = false;
                self.saved = self.count;
                self.launched = false;
                self.landed = 0;
                self.finished = false;
                self.slots = [(0, 0); 10];
            }
            ChallengeEvent::RaceFinish { car } => {
                if !car.finished {
                    self.distinct = 0;
                    self.count = self.saved;
                } else {
                    self.finished = true;
                    self.final_position = car.final_position as i16;
                }
                self.running = false;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => {
                self.distinct = 0;
                self.running = false;
                self.count = self.saved;
            }
            ChallengeEvent::Reset => {
                self.count = 0;
                self.distinct = 0;
                self.launched = false;
                self.landed = 0;
                self.running = false;
            }
            _ => {}
        }
    }

    /// `IsCompletedInternal @001fa488`.
    pub fn is_completed(&self) -> bool {
        if self.finish_race && !self.finished {
            return false;
        }
        let n = self.bodies_to_hit;
        if !self.finish_first {
            if 0 < n {
                return n <= self.distinct as i32;
            }
        } else if 0 < n {
            if self.final_position != 1 {
                return false;
            }
            return 0 < self.distinct;
        }
        if self.hits < 1 {
            return false;
        }
        self.hits <= self.count
    }
}

// ---- CChallengeRacePosition @00204f74 / OnEvent @00203bf0 -----------------------------------------------------------
/// `<ChallengeRacePosition>` (the "finish in position / win N in a row" family used by 32 challenges).
#[derive(Clone, Debug, PartialEq)]
pub struct ChRacePosition {
    /// `+0x14` successes (wins in sequence), `+0x18` saved, `+0x1c` previous final position (sequence rule).
    pub progress: i32,
    pub saved: i32,
    pub last_position: i32,
    /// `+0x20` worst race position seen after the launch.
    pub max_pos: i32,
    /// `+0x24`: bit0 ability used, bit1 launched, bit2 race running, bit3 hit, bit4 hit by an opponent,
    /// bit5 launch position captured, bit6 ability active now, bit7 boost pad used. `+0x25` bit0 respawned.
    pub bits: u8,
    pub respawned: bool,
    /// `+0x28` seconds from race start until the launch.
    pub time_to_launch: f32,
    /// `+0x2c..0x31` the (up to 3) character ids that achieved it, `+0x34` count.
    pub chars: [i16; 3],
    pub num_chars: i32,
    /// `+0x38` position at the launch.
    pub launched_position: i32,
    pub position: i32,
    /// `Counter` (`+0x40`, ctor default 1).
    pub counter: i32,
    pub max_position: i32,
    pub do_not_use_ability: bool,
    pub do_not_use_powerup: bool,
    pub do_not_be_hit: bool,
    pub do_not_use_boost: bool,
    pub in_sequence: bool,
    pub heavily_damaged: bool,
    pub no_damage: bool,
    pub reversed: bool,
    pub detached: bool,
    pub in_air: bool,
    pub use_ability_in_finish: bool,
    pub respawned_req: bool,
    pub do_not_respawn: bool,
    pub launch_delay: f32,
    pub unique_characters: i32,
    pub launched_position_req: i32,
    pub num_powerup_used: i32,
}
impl Default for ChRacePosition {
    fn default() -> Self {
        ChRacePosition {
            progress: 0,
            saved: 0,
            last_position: 0,
            max_pos: 0,
            bits: 0,
            respawned: false,
            time_to_launch: 0.0,
            chars: [0; 3],
            num_chars: 0,
            launched_position: 0,
            position: 0,
            counter: 1,
            max_position: 0,
            do_not_use_ability: false,
            do_not_use_powerup: false,
            do_not_be_hit: false,
            do_not_use_boost: false,
            in_sequence: false,
            heavily_damaged: false,
            no_damage: false,
            reversed: false,
            detached: false,
            in_air: false,
            use_ability_in_finish: false,
            respawned_req: false,
            do_not_respawn: false,
            launch_delay: 0.0,
            unique_characters: 0,
            launched_position_req: 0,
            num_powerup_used: 0,
        }
    }
}
/// `DAT_002042f0` (= 70.0): a car counts as heavily damaged at or above this `CCar+0x1b08` value.
pub const HEAVY_DAMAGE: f32 = 70.0;
impl ChRacePosition {
    pub fn read(n: &roxmltree::Node) -> ChRacePosition {
        let mut c = ChRacePosition::default();
        rd_i32(n, "Position", &mut c.position);
        rd_i32(n, "Counter", &mut c.counter);
        rd_bool(n, "DoNotUseAbility", &mut c.do_not_use_ability);
        rd_bool(n, "DoNotUsePowerUp", &mut c.do_not_use_powerup);
        rd_bool(n, "DoNotBeHit", &mut c.do_not_be_hit);
        rd_bool(n, "DoNotUseBoost", &mut c.do_not_use_boost);
        rd_bool(n, "InSequence", &mut c.in_sequence);
        rd_f32(n, "LaunchDelay", &mut c.launch_delay);
        rd_i32(n, "UniqueCharacters", &mut c.unique_characters);
        rd_i32(n, "MaxPosition", &mut c.max_position);
        rd_bool(n, "HeavilyDamaged", &mut c.heavily_damaged);
        rd_bool(n, "NoDamage", &mut c.no_damage);
        rd_bool(n, "Reversed", &mut c.reversed);
        rd_bool(n, "Detached", &mut c.detached);
        rd_bool(n, "InAir", &mut c.in_air);
        rd_bool(n, "Respawned", &mut c.respawned_req);
        rd_bool(n, "DoNotRespawn", &mut c.do_not_respawn);
        rd_i32(n, "LaunchedPosition", &mut c.launched_position_req);
        rd_bool(n, "UseAbilityInFinish", &mut c.use_ability_in_finish);
        rd_i32(n, "NumPowerUpUsed", &mut c.num_powerup_used);
        c
    }

    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                if self.bits & 4 == 0 {
                    return;
                }
                if self.bits & 2 == 0 {
                    self.time_to_launch += dt;
                    return;
                }
                if self.bits & 0x20 == 0 {
                    let landed = car.launch_state & 1;
                    self.bits = (self.bits & 0xdf) | ((landed as u8) << 5);
                    if landed == 0 {
                        return;
                    }
                    self.launched_position = car.position;
                    return;
                }
                if self.max_position < 1 {
                    return;
                }
                if self.max_pos < car.position {
                    self.max_pos = car.position;
                }
            }
            ChallengeEvent::Launch { car, .. } => {
                self.launched_position = car.position;
                self.bits |= 2;
            }
            ChallengeEvent::Hit { hit } => {
                self.bits |= 8;
                if hit.has_object && hit.is_opponent {
                    self.bits |= 0x10;
                }
            }
            ChallengeEvent::Boost { .. } => {
                self.bits |= 0x80;
                if self.do_not_use_boost && self.in_sequence {
                    self.progress = 0;
                }
            }
            ChallengeEvent::RaceStart => {
                self.max_pos = 0;
                self.launched_position = 0;
                self.bits = 4;
                self.time_to_launch = 0.0;
                self.respawned = false;
                self.saved = self.progress;
            }
            ChallengeEvent::RaceQuit { car } => {
                // not in a sequence: restore; in a sequence: 0 unless the car had finished (`@00204130`)
                if !self.in_sequence {
                    self.progress = self.saved;
                } else if !car.finished {
                    self.progress = 0;
                }
                self.bits &= 0xfb;
            }
            ChallengeEvent::RaceRestart => {
                self.progress = self.saved;
                self.bits &= 0xfb;
            }
            ChallengeEvent::Ability { used, .. } => {
                if *used {
                    self.bits |= 0x41;
                } else {
                    self.bits &= 0xbf;
                }
            }
            ChallengeEvent::Respawn => self.respawned = true,
            ChallengeEvent::Reset => {
                self.progress = 0;
                self.last_position = 0;
                self.num_chars = 0;
                self.bits &= 0xf1;
                self.time_to_launch = 0.0;
            }
            ChallengeEvent::RaceFinish { car } => self.on_finish(car, env),
            _ => {}
        }
    }

    /// The `RaceFinish` branch of `OnEvent @00203bf0` (every "do not ..." gate, then the position test).
    fn on_finish(&mut self, car: &CarSnap, env: &ChallengeEnv) {
        let prev_bits = self.bits;
        self.bits &= 0xfb;
        let pu = env.powerups;
        if self.do_not_use_ability && (prev_bits & 1) != 0 {
            return;
        }
        if self.do_not_use_powerup && (0..4).any(|i| pu.is_active(i)) {
            return;
        }
        if 0 < self.num_powerup_used {
            let n = (0..4).filter(|i| pu.is_active(*i)).count() as i32;
            if n < self.num_powerup_used {
                return;
            }
        }
        if self.use_ability_in_finish && (self.bits & 0x40) == 0 {
            return;
        }
        if self.do_not_be_hit && (self.bits & 0x10) != 0 {
            return;
        }
        if self.do_not_use_boost && (self.bits & 0x80) != 0 {
            return;
        }
        if 0.0 < self.launch_delay && self.time_to_launch < self.launch_delay {
            return;
        }
        if 0 < self.max_position && self.max_position < self.max_pos {
            return;
        }
        let mut lp = self.launched_position_req;
        if 0 < lp && 0 < self.launched_position {
            if env.num_cars <= lp {
                lp = env.num_cars;
            }
            if self.launched_position < lp {
                return;
            }
        }
        if !self.respawned_req {
            if self.do_not_respawn && self.respawned {
                return;
            }
        } else {
            if !self.respawned {
                return;
            }
            if self.do_not_respawn {
                return;
            }
        }
        if !car.finished {
            return;
        }
        if self.reversed && 0.0 < car.facing_spline_dot {
            return;
        }
        if self.detached && car.head_visual_damage != 4 {
            return;
        }
        if self.in_air && car.wheels_on_ground > 0 {
            return;
        }
        if self.heavily_damaged && car.damage < HEAVY_DAMAGE {
            return;
        }
        if self.no_damage && 10.0 < car.damage {
            return;
        }
        let pos = car.final_position;
        let target = self.position;
        if pos <= target {
            let skip = self.in_sequence && self.last_position != 0 && pos > self.last_position;
            if !skip {
                self.progress += 1;
                if self.num_chars < 3 {
                    let ch = env.player_character;
                    let known = self.chars[..self.num_chars as usize].contains(&ch);
                    if !known {
                        self.chars[self.num_chars as usize] = ch;
                        self.num_chars += 1;
                    }
                }
            }
            self.last_position = pos;
            return;
        }
        if self.in_sequence {
            self.progress = 0;
        }
        self.last_position = pos;
    }

    /// `IsCompletedInternal @001f96e8`.
    pub fn is_completed(&self) -> bool {
        if self.progress < self.counter {
            return false;
        }
        if self.unique_characters == 0 {
            return true;
        }
        self.unique_characters <= self.num_chars
    }
}

// ---- CChallengeOvertake @00205abc / OnEvent @00202a88 ------------------------------------------------------------
/// `<ChallengeOvertake><Count/><Time/><NoTurning/><UseBoost/><InAir/><Drifting/><CountOvertakes/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChOvertake {
    /// `+0x14` current streak, `+0x18` best / total, `+0x1c` last position, `+0x20` countdown of `Time`.
    pub current: i32,
    pub best: i32,
    pub last_position: i32,
    pub window: f32,
    /// `+0x24` launched, `+0x28` started (`car+0x1bec` seen), `+0x2c` boost window.
    pub launched: bool,
    pub started: u32,
    pub boost_window: f32,
    pub count: i32,
    pub time: f32,
    pub no_turning: bool,
    pub use_boost: bool,
    pub in_air: bool,
    pub drifting: bool,
    pub count_overtakes: bool,
}
/// `DAT_00202f64` (0.001): steering magnitude above which `NoTurning` fails.
pub const NO_TURNING_EPS: f32 = 0.001;
/// Seconds an overtake counts after a boost pad (`0x3f333333`).
pub const OVERTAKE_BOOST_WINDOW: f32 = 0.7;
impl ChOvertake {
    pub fn read(n: &roxmltree::Node) -> ChOvertake {
        let mut c = ChOvertake::default();
        rd_i32(n, "Count", &mut c.count);
        rd_f32(n, "Time", &mut c.time);
        rd_bool(n, "NoTurning", &mut c.no_turning);
        rd_bool(n, "UseBoost", &mut c.use_boost);
        rd_bool(n, "InAir", &mut c.in_air);
        rd_bool(n, "Drifting", &mut c.drifting);
        rd_bool(n, "CountOvertakes", &mut c.count_overtakes);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                if self.started == 0 {
                    self.started = car.launch_state;
                    return;
                }
                if !self.launched {
                    return;
                }
                if self.use_boost {
                    let t = self.boost_window;
                    if t <= 0.0 {
                        return;
                    }
                    if t < *dt {
                        self.boost_window = 0.0;
                        let n = self.last_position - car.position;
                        self.current = n;
                        if self.best < n {
                            self.best = n;
                        }
                        return;
                    }
                    self.boost_window = t - dt;
                    return;
                }
                if self.count_overtakes {
                    let last = self.last_position;
                    if last != 0 && car.position < last {
                        self.best += last - car.position;
                    }
                    self.last_position = car.position;
                    return;
                }
                let t = self.time;
                let pos = car.position;
                let go = if t <= 0.0 {
                    true
                } else {
                    self.window -= dt;
                    0.0 < self.window
                };
                let cur;
                if go {
                    let last = self.last_position;
                    if last != 0 {
                        if pos < last {
                            self.current += last - pos;
                        } else if last < pos {
                            let mut v = (last - pos) + self.current;
                            if v < 1 {
                                v = 0;
                                self.window = t;
                            }
                            self.current = v;
                        }
                        if self.no_turning && NO_TURNING_EPS < car.steer_abs {
                            self.current = 0;
                        }
                        if self.in_air && car.wheels_on_ground > 0 {
                            self.current = 0;
                        }
                        if self.drifting && !car.drifting {
                            self.current = 0;
                        }
                        self.last_position = pos;
                        if self.best < self.current {
                            self.best = self.current;
                        }
                        return;
                    }
                    cur = self.current;
                } else {
                    self.current = 0;
                    self.last_position = 0;
                    cur = 0;
                }
                self.window = t;
                self.last_position = pos;
                if self.best < cur {
                    self.best = cur;
                }
            }
            ChallengeEvent::Boost { car, .. } => {
                if !self.use_boost {
                    return;
                }
                if self.boost_window <= 0.0 {
                    self.current = 0;
                    self.last_position = car.position;
                }
                self.boost_window = OVERTAKE_BOOST_WINDOW;
            }
            ChallengeEvent::Launch { .. } => self.launched = true,
            ChallengeEvent::RaceStart | ChallengeEvent::RaceRestart => {
                self.current = 0;
                self.best = 0;
                self.last_position = 0;
                if let ChallengeEvent::RaceStart = ev {
                    self.boost_window = 0.0;
                }
                self.launched = false;
                self.started = 0;
            }
            ChallengeEvent::RaceQuit { .. } => self.best = 0,
            _ => {}
        }
    }
    /// `IsCompletedInternal @001f9848`.
    pub fn is_completed(&self) -> bool {
        self.count <= self.best
    }
}

// ---- CChallengePositionInTime @0020508c / OnEvent @002007f4 -----------------------------------------------------
/// `<ChallengePositionInTime><StartPosition/><EndPosition/><Time/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChPositionInTime {
    /// `+0x14` seconds left, `+0x18` bit0 reached the start position, bit1 done, bit2 multi-car race, bit3 launched,
    /// bit4 airborne seen.
    pub timer: f32,
    pub bits: u8,
    pub start_position: i32,
    pub end_position: i32,
    pub time: f32,
}
impl ChPositionInTime {
    pub fn read(n: &roxmltree::Node) -> ChPositionInTime {
        let mut c = ChPositionInTime::default();
        rd_i32(n, "StartPosition", &mut c.start_position);
        rd_i32(n, "EndPosition", &mut c.end_position);
        rd_f32(n, "Time", &mut c.time);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                let b = self.bits;
                if (b & 0xc) != 0xc {
                    return;
                }
                if (b & 0x10) == 0 {
                    self.bits = (b & 0xef) | (((car.wheels_on_ground > 0) as u8) << 4);
                    return;
                }
                if (b & 2) != 0 {
                    return;
                }
                let mut p = self.start_position;
                if p < 1 || env.num_cars < p {
                    p = env.num_cars;
                }
                if (b & 1) == 0 {
                    if p <= car.position {
                        self.bits = (self.bits & 0xfd) | 1;
                        self.timer = self.time;
                    }
                    return;
                }
                let pos = car.position;
                let bvar = self.end_position < p;
                if p < pos || (p == pos && bvar) {
                    self.timer = self.time;
                    return;
                }
                if self.end_position < pos || !bvar {
                    if *dt < self.timer {
                        self.timer -= dt;
                        return;
                    }
                    if bvar {
                        self.bits &= 0xfe;
                        return;
                    }
                }
                self.bits |= 2;
            }
            ChallengeEvent::RaceStart => {
                self.timer = 0.0;
                self.bits = (self.bits & 0xe0) | (((env.num_cars > 1) as u8) << 2);
            }
            ChallengeEvent::RaceFinish { car } => {
                if !car.finished {
                    self.timer = 0.0;
                }
                self.bits &= 0xfb;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => self.timer = 0.0,
            ChallengeEvent::Launch { .. } => self.bits |= 8,
            ChallengeEvent::Reset => {
                self.timer = 0.0;
                self.bits &= 0xf8;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001f9728`.
    pub fn is_completed(&self) -> bool {
        (self.bits & 3) == 3
    }
}

// ---- CChallengeSlipstream @00205b68 / OnEvent @001ffbe0 ------------------------------------------------------------
/// `<ChallengeSlipstream><Time/><Continuous/>`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChSlipstream {
    /// `+0x14` seconds drafted, `+0x18` saved.
    pub time: f32,
    pub saved: f32,
    pub target: f32,
    pub continuous: bool,
}
impl ChSlipstream {
    pub fn read(n: &roxmltree::Node) -> ChSlipstream {
        let mut c = ChSlipstream::default();
        rd_f32(n, "Time", &mut c.target);
        rd_bool(n, "Continuous", &mut c.continuous);
        c
    }
    /// `locked` = flag bit1 of the base (already completed and latched).
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, locked: bool, draft: f32) {
        match ev {
            ChallengeEvent::Update { dt, .. } => {
                if locked {
                    return;
                }
                if self.target <= self.time {
                    return;
                }
                if 0.5 < draft {
                    self.time += dt;
                    return;
                }
                if !self.continuous {
                    return;
                }
                self.time = self.saved;
            }
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.saved = 0.0;
                    self.time = 0.0;
                    return;
                }
                self.time = self.saved;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car.finished {
                    return;
                }
                self.time = self.saved;
            }
            ChallengeEvent::RaceQuit { .. } => self.time = self.saved,
            ChallengeEvent::RaceRestart => {
                self.time = 0.0;
                self.saved = 0.0;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001f9860`.
    pub fn is_completed(&self) -> bool {
        self.target <= self.time
    }
}


// ---- CChallengeCollectFruit @00204b84 / OnEvent @00206378 -----------------------------------------------------------
/// Fruit order of the per-type counters / `MaxFruit` index: Banana 0, Melon 1, Strawberry 2, IceWafer 3, IceLolly 4, IceCream 5.
pub const FRUIT_NAMES: [&str; 6] = ["Banana", "Melon", "Strawberry", "IceWafer", "IceLolly", "IceCream"];

/// `<ChallengeCollectFruit>` (80 challenges): collect seed-rush tokens under various conditions.
#[derive(Clone, Debug, PartialEq)]
pub struct ChCollectFruit {
    /// `+0x14` fruit counted now, `+0x18` best count of the race, `+0x1c` saved at race start.
    pub count: i32,
    pub best: i32,
    pub saved: i32,
    /// `+0x20` kind of the last fruit (-1 none), `+0x24` number of plain tokens on the track.
    pub last_kind: i32,
    pub total_tokens: i32,
    /// `+0x28` (0.5 s after a pick-up; unused elsewhere), `+0x2c` timer since the first fruit (-1 = not started).
    pub pick_flash: f32,
    pub timer: f32,
    /// `+0x30..0x47`: which fruit types count (`Banana`.. `IceCream` child elements); `+0x48` bit0 = any filter given.
    pub filter: [bool; 6],
    pub has_filter: bool,
    /// `+0x4a..0x55`: fruit collected this race per type (shorts).
    pub collected: [i16; 6],
    /// `+0x56`: bit0 launched, bit1 touched the ground after the launch, bit2 race finished, bit3 ability active,
    /// bit4 inside the boost window, bit5 Timer goal reached, bit6 fruit bar full.
    pub bits: u8,
    /// `+0x58` boost window left, `+0x5c` seconds until the fruit bar was full.
    pub boost_window: f32,
    pub time_to_full: f32,
    pub amount: i32,
    pub timer_limit: f32,
    pub no_steering: bool,
    pub single_no_steering: bool,
    pub drifting: bool,
    pub single_drift: bool,
    pub in_row: bool,
    pub in_air: bool,
    pub single_in_air: bool,
    pub use_ability: bool,
    pub nearly_miss: bool,
    pub maximum: bool,
    pub less: bool,
    pub boosting: bool,
    pub percentage: bool,
    pub full_fruit_bar: bool,
    /// `+0xa0` `MaxFruit` (-1 none).
    pub max_fruit: i32,
}
impl Default for ChCollectFruit {
    fn default() -> Self {
        ChCollectFruit {
            count: 0,
            best: 0,
            saved: 0,
            last_kind: -1,
            total_tokens: 0,
            pick_flash: 0.0,
            timer: -1.0,
            filter: [false; 6],
            has_filter: false,
            collected: [0; 6],
            bits: 0,
            boost_window: 0.0,
            time_to_full: 0.0,
            amount: 0,
            timer_limit: 0.0,
            no_steering: false,
            single_no_steering: false,
            drifting: false,
            single_drift: false,
            in_row: false,
            in_air: false,
            single_in_air: false,
            use_ability: false,
            nearly_miss: false,
            maximum: false,
            less: false,
            boosting: false,
            percentage: false,
            full_fruit_bar: false,
            max_fruit: -1,
        }
    }
}
impl ChCollectFruit {
    pub fn read(n: &roxmltree::Node) -> ChCollectFruit {
        let mut c = ChCollectFruit::default();
        rd_i32(n, "Amount", &mut c.amount);
        rd_f32(n, "Timer", &mut c.timer_limit);
        rd_bool(n, "NoSteering", &mut c.no_steering);
        rd_bool(n, "SingleNoSteering", &mut c.single_no_steering);
        rd_bool(n, "Drifting", &mut c.drifting);
        rd_bool(n, "SingleDrift", &mut c.single_drift);
        rd_bool(n, "InRow", &mut c.in_row);
        rd_bool(n, "InAir", &mut c.in_air);
        rd_bool(n, "SingleInAir", &mut c.single_in_air);
        rd_bool(n, "UseAbility", &mut c.use_ability);
        rd_bool(n, "NearlyMiss", &mut c.nearly_miss);
        rd_bool(n, "Maximum", &mut c.maximum);
        rd_bool(n, "Less", &mut c.less);
        rd_bool(n, "Boosting", &mut c.boosting);
        rd_bool(n, "Percentage", &mut c.percentage);
        rd_bool(n, "FullFruitBar", &mut c.full_fruit_bar);
        // every element child whose text names a fruit enables that type (GetFirstChild/GetNextSibling loop)
        for ch in n.children().filter(|x| x.is_element()) {
            let t = ch.text().unwrap_or("");
            for (i, name) in FRUIT_NAMES.iter().enumerate() {
                if t.eq_ignore_ascii_case(name) {
                    c.filter[i] = true;
                    c.has_filter = true;
                    break;
                }
            }
        }
        if let Some(t) = child_text(n, "MaxFruit") {
            for (i, name) in FRUIT_NAMES.iter().enumerate() {
                if t.eq_ignore_ascii_case(name) {
                    c.max_fruit = i as i32;
                    break;
                }
            }
        }
        c
    }

    fn token_gate(&self, car: &CarSnap) -> bool {
        // conditions every `Pickup` must pass (`@002063a0..`)
        if self.use_ability && (self.bits & 8) == 0 {
            return false;
        }
        if self.drifting && !car.drifting {
            return false;
        }
        if self.in_air && car.wheels_on_ground > 0 {
            return false;
        }
        if self.no_steering && NO_TURNING_EPS < car.steer_abs {
            return false;
        }
        if self.boosting && (self.bits & 0x10) == 0 {
            return false;
        }
        true
    }

    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::Update { dt, car } => {
                let mut b = self.bits;
                if (b & 3) == 1 {
                    let ground = car.wheels_on_ground > 0;
                    b = (b & !2) | ((ground as u8) << 1);
                    self.bits = b;
                }
                if (b & 3) == 3 && self.full_fruit_bar && (b & 0x40) == 0 {
                    if env.game_mode == GAME_MODE_SEED_RUSH {
                        let full = env.fruit_bar_full;
                        self.bits = (self.bits & 0xbf) | ((full as u8) << 6);
                    }
                    self.time_to_full += dt;
                }
                self.boost_window -= dt;
                if self.boost_window <= 0.0 {
                    self.boost_window = 0.0;
                    self.bits &= 0xef;
                }
                let mut t = self.timer;
                let running = t != -1.0;
                if running {
                    t += dt;
                    self.timer = t;
                }
                let expired = self.timer_limit < t;
                if expired {
                    self.timer = -1.0;
                    self.count = 0;
                } else if self.count >= self.amount {
                    self.bits |= 0x20;
                }
            }
            ChallengeEvent::Pickup { kind, token_kind, car } => {
                if self.nearly_miss {
                    return;
                }
                // only seed-rush tokens count (type slot 0xdaa668)
                if *kind != PickupKind::SeedRushToken {
                    return;
                }
                if !self.token_gate(car) {
                    return;
                }
                if env.fruit_rush_mode > 1 {
                    return;
                }
                let k = (*token_kind as usize).min(5);
                if !self.has_filter || ((!self.in_row || (k as i32 == self.last_kind || self.last_kind == -1)) && self.filter[k]) {
                    self.count += 1;
                    self.collected[k] += 1;
                    if self.timer_limit != 0.0 && self.timer == -1.0 {
                        self.timer = 0.0;
                    }
                }
                self.last_kind = k as i32;
                self.pick_flash = 0.5;
                if self.best < self.count {
                    self.best = self.count;
                }
            }
            ChallengeEvent::NearlyMiss { token_kind, .. } => {
                if !self.nearly_miss {
                    return;
                }
                if self.use_ability && (self.bits & 8) == 0 {
                    return;
                }
                let kind = match token_kind {
                    Some(k) => *k as usize,
                    None => return,
                };
                if self.has_filter && !self.filter[kind.min(5)] {
                    return;
                }
                self.count += 1;
                if self.best < self.count {
                    self.best = self.count;
                }
            }
            ChallengeEvent::Ability { used, .. } => self.bits = (self.bits & 0xf7) | (((*used) as u8) << 3),
            ChallengeEvent::Boost { .. } => {
                self.boost_window = OVERTAKE_BOOST_WINDOW;
                self.bits |= 0x10;
            }
            ChallengeEvent::Launch { .. } => self.bits |= 1,
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.count = 0;
                }
                self.pick_flash = 0.0;
                self.time_to_full = 0.0;
                self.saved = self.count;
                self.best = self.count;
                self.last_kind = -1;
                self.bits &= 0xb0;
                self.total_tokens = env.num_seed_tokens;
                self.collected = [0; 6];
            }
            ChallengeEvent::RaceFinish { car } => {
                if car.finished {
                    self.bits |= 4;
                    return;
                }
                self.count = self.saved;
                self.best = self.saved;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => {
                self.count = self.saved;
                self.best = self.saved;
            }
            ChallengeEvent::Reset => {
                self.count = 0;
                self.best = 0;
            }
            _ => {}
        }
    }

    /// `IsCompletedInternal @001fd72c`.
    pub fn is_completed(&self) -> bool {
        if self.full_fruit_bar {
            if self.bits & 0x40 == 0 {
                return false;
            }
            return self.time_to_full <= self.timer_limit;
        }
        if self.maximum {
            if self.bits & 4 == 0 {
                return false;
            }
            return self.best <= self.amount;
        }
        if self.less {
            if self.bits & 4 == 0 {
                return false;
            }
            return self.best < self.amount;
        }
        if self.percentage {
            if self.total_tokens < 1 {
                return false;
            }
            let pct = if self.max_fruit == -1 {
                (self.best * 100) / self.total_tokens
            } else {
                let idx = self.max_fruit as usize;
                let mut m: i16 = 0;
                for j in 0..6 {
                    if j != idx && self.collected[j] > m {
                        m = self.collected[j];
                    }
                }
                if m == 0 {
                    if self.collected[idx] == 0 {
                        0
                    } else {
                        100
                    }
                } else {
                    (self.collected[idx] as i32 * 100) / m as i32
                }
            };
            return self.amount <= pct;
        }
        if self.timer_limit != 0.0 {
            return (self.bits & 0x20) != 0;
        }
        if self.max_fruit == -1 {
            return self.amount <= self.best;
        }
        if self.bits & 4 == 0 {
            return false;
        }
        let idx = self.max_fruit as usize;
        for j in 0..6 {
            if j != idx && self.collected[idx] <= self.collected[j] {
                return false;
            }
        }
        true
    }
}

// ---- CChallengeGet3Stars @00205c54 / OnEvent @001ffe54 -------------------------------------------------------------
/// `<ChallengeGet3Stars><NumCharacters/><NumKarts/><NoSpecialAbility/><NoPowerUp/><HeavilyDamaged/><Count/><Respawns/>
/// <Row/><WithKart/><KartName/>`
#[derive(Clone, Debug, PartialEq)]
pub struct ChGet3Stars {
    /// `+0x14..0x1c` characters that got 3 stars (-1 = empty), `+0x24..0x2c` karts (same).
    pub chars: [i32; 3],
    pub karts: [i32; 3],
    /// `+0x3c` number of 3-star races counted, `+0x40` respawns this race, `+0x44` WithKart success flag,
    /// `+0x48` a power-up was used at the launch, `+0x4c` NoPowerUp success flag, `+0x5c` an ability was used.
    pub count3: i32,
    pub respawns: i32,
    pub with_kart_ok: bool,
    pub powerup_used: bool,
    pub no_powerup_ok: bool,
    pub ability_used: bool,
    pub num_characters: i32,
    pub num_karts: i32,
    pub count: i32,
    pub respawns_required: i32,
    pub no_special_ability: bool,
    pub no_powerup: bool,
    pub heavily_damaged: bool,
    pub row: bool,
    pub with_kart: bool,
    pub kart_name: String,
}
impl Default for ChGet3Stars {
    fn default() -> Self {
        ChGet3Stars {
            chars: [-1; 3],
            karts: [-1; 3],
            count3: 0,
            respawns: 0,
            with_kart_ok: false,
            powerup_used: false,
            no_powerup_ok: false,
            ability_used: false,
            num_characters: 0,
            num_karts: 0,
            count: 0,
            respawns_required: 0,
            no_special_ability: false,
            no_powerup: false,
            heavily_damaged: false,
            row: false,
            with_kart: false,
            kart_name: String::new(),
        }
    }
}
fn filled(list: &[i32; 3], want: i32) -> i32 {
    // `GetNumCharsCompleted @00205cec` / `GetNumKartsCompleted @00205d4c`
    if want < 1 || list[0] == -1 {
        return 0;
    }
    if want == 1 {
        return 1;
    }
    if list[1] == -1 {
        return 1;
    }
    if want == 2 {
        return 2;
    }
    if list[2] == -1 {
        2
    } else {
        3
    }
}
fn register_slot(list: &mut [i32; 3], want: i32, id: i32) {
    // the nested slot search of `@002002e8`
    if want != 0 && 0 < want && id != list[0] {
        let slot;
        if list[0] == -1 {
            slot = 0;
        } else {
            if want < 2 || id == list[1] {
                return;
            }
            if list[1] == -1 {
                slot = 1;
            } else {
                if want < 3 || id == list[2] || list[2] != -1 {
                    return;
                }
                slot = 2;
            }
        }
        list[slot] = id;
    }
}
impl ChGet3Stars {
    pub fn read(n: &roxmltree::Node) -> ChGet3Stars {
        let mut c = ChGet3Stars::default();
        rd_i32(n, "NumCharacters", &mut c.num_characters);
        rd_i32(n, "NumKarts", &mut c.num_karts);
        rd_bool(n, "NoSpecialAbility", &mut c.no_special_ability);
        rd_bool(n, "NoPowerUp", &mut c.no_powerup);
        rd_bool(n, "HeavilyDamaged", &mut c.heavily_damaged);
        rd_i32(n, "Count", &mut c.count);
        rd_i32(n, "Respawns", &mut c.respawns_required);
        rd_bool(n, "Row", &mut c.row);
        rd_bool(n, "WithKart", &mut c.with_kart);
        if let Some(t) = child_text(n, "KartName") {
            c.kart_name = name32(t);
        }
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, env: &ChallengeEnv) {
        let kart_id = env.kart_id;
        let broken_wheels = env.broken_wheels;
        match ev {
            ChallengeEvent::RaceStart => {
                if !persistent {
                    self.count3 = 0;
                }
                self.powerup_used = false;
                self.ability_used = false;
                self.respawns = 0;
            }
            ChallengeEvent::Ability { used, .. } => {
                if *used {
                    self.ability_used = true;
                }
            }
            ChallengeEvent::Launch { .. } => {
                if (0..4).any(|i| env.powerups.is_active(i)) {
                    self.powerup_used = true;
                }
            }
            ChallengeEvent::Respawn => self.respawns += 1,
            ChallengeEvent::RaceFinish { car } => {
                if self.ability_used && self.no_special_ability {
                    return;
                }
                if self.heavily_damaged && broken_wheels > 0 {
                    return;
                }
                if 0 < self.respawns_required && self.respawns < self.respawns_required {
                    return;
                }
                if !car.finished {
                    return;
                }
                if env.stars != 3 {
                    if !self.row {
                        return;
                    }
                    self.count3 = 0;
                    return;
                }
                register_slot(&mut self.chars, self.num_characters, env.player_character as i32);
                if self.num_karts != 0 {
                    // `@002002e8`: register the kart id, then return (no count)
                    let want = self.num_karts;
                    if want < 1 {
                        return;
                    }
                    if kart_id == self.karts[0] {
                        return;
                    }
                    let slot;
                    if self.karts[0] == -1 {
                        slot = 0;
                    } else {
                        if want < 2 || kart_id == self.karts[1] {
                            return;
                        }
                        if self.karts[1] == -1 {
                            slot = 1;
                        } else {
                            if want < 3 || kart_id == self.karts[2] || self.karts[2] != -1 {
                                return;
                            }
                            slot = 2;
                        }
                    }
                    self.karts[slot] = kart_id;
                    return;
                }
                if !self.with_kart {
                    if self.no_powerup {
                        self.no_powerup_ok = !self.powerup_used;
                        return;
                    }
                } else {
                    if name32(env.kart_name) != self.kart_name {
                        return;
                    }
                    if self.count == 0 {
                        self.with_kart_ok = true;
                        return;
                    }
                }
                self.count3 += 1;
            }
            _ => {}
        }
    }
    /// `IsCompletedInternal @001fb840`.
    pub fn is_completed(&self) -> bool {
        if self.with_kart {
            if self.count == 0 {
                return self.with_kart_ok;
            }
            return self.count <= self.count3;
        }
        if self.no_powerup {
            return self.no_powerup_ok;
        }
        if self.num_characters != 0 {
            return self.num_characters == filled(&self.chars, self.num_characters);
        }
        if self.num_karts == 0 {
            return self.count <= self.count3;
        }
        filled(&self.karts, self.num_karts) == self.num_karts
    }
}

// ---- CChallengeDestroy @00205504 / OnEvent @001ff14c -----------------------------------------------------------------
/// The object a `Hit` / `Destroy` event refers to (`CChallengeEventHit::GetPhysicalObject` /
/// `CChallengeEventDestroy+4`): identity + kind + smackable display name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhysObj {
    pub id: u32,
    /// `vfunc 8`: 1 = opponent car, 0 = smackable.
    pub is_opponent: bool,
    pub is_smackable: bool,
    pub smackable_name: String,
}

/// `<ChallengeDestroy><ObjectsDestroyed/><OneHit/><InAir/><Drifting/><SmackableType/><Percent/><TimeLimit/>`
#[derive(Clone, Debug, PartialEq)]
pub struct ChDestroy {
    /// `+0x14..0x73`: up to 8 recently hit objects `{id, time left (1 s), was in the air}`, `+0x74` count.
    pub hit: Vec<(u32, f32, bool)>,
    /// `+0x78` destroyed, `+0x7c` saved, `+0x90` total destroyable items, `+0x9c` seconds since the first destroy,
    /// `+0xa0` best (shortest) time for `ObjectsDestroyed` within `TimeLimit`.
    pub destroyed: i32,
    pub saved: i32,
    pub total: i32,
    pub timer: f32,
    pub best_time: f32,
    pub objects_destroyed: i32,
    pub one_hit: bool,
    pub in_air: bool,
    pub drifting: bool,
    pub percent: i32,
    pub time_limit: f32,
    pub smackable_filter: String,
}
impl Default for ChDestroy {
    fn default() -> Self {
        ChDestroy {
            hit: Vec::new(),
            destroyed: 0,
            saved: 0,
            total: 0,
            timer: 0.0,
            best_time: f32::INFINITY,
            objects_destroyed: 0,
            one_hit: false,
            in_air: false,
            drifting: false,
            percent: 0,
            time_limit: 0.0,
            smackable_filter: String::new(),
        }
    }
}
impl ChDestroy {
    pub fn read(n: &roxmltree::Node) -> ChDestroy {
        let mut c = ChDestroy::default();
        rd_i32(n, "ObjectsDestroyed", &mut c.objects_destroyed);
        rd_bool(n, "OneHit", &mut c.one_hit);
        rd_bool(n, "InAir", &mut c.in_air);
        rd_bool(n, "Drifting", &mut c.drifting);
        if let Some(t) = child_text(n, "SmackableType") {
            c.smackable_filter = name32(t);
        }
        rd_i32(n, "Percent", &mut c.percent);
        rd_f32(n, "TimeLimit", &mut c.time_limit);
        c
    }
    pub fn on_event(&mut self, ev: &ChallengeEvent, persistent: bool, env: &ChallengeEnv) {
        match ev {
            ChallengeEvent::Update { dt, .. } => {
                let mut i = self.hit.len();
                while i != 0 {
                    let idx = i - 1;
                    self.hit[idx].1 -= dt;
                    if self.hit[idx].1 <= 0.0 {
                        self.hit.remove(idx);
                    }
                    i -= 1;
                }
                let before = self.timer;
                self.timer = before + dt;
                if 0.0 < self.time_limit && self.time_limit < before + dt {
                    self.destroyed = 0;
                }
            }
            ChallengeEvent::Hit { hit } => {
                let obj = match &hit.object {
                    Some(o) => o,
                    None => return,
                };
                if !obj.is_opponent && !obj.is_smackable {
                    return;
                }
                // a body already tracked: refresh its timer
                for e in self.hit.iter_mut().rev().take(8) {
                    if e.0 == obj.id {
                        e.1 = 1.0;
                        return;
                    }
                }
                if self.hit.len() > 7 {
                    return;
                }
                self.hit.push((obj.id, 1.0, hit.attacker_wheels_on_ground.map(|w| w == 0).unwrap_or(false)));
            }
            ChallengeEvent::Destroy { object, amount, drifting_player } => {
                let o = match object {
                    None => {
                        if self.one_hit && 0.9 <= *amount {
                            self.destroyed += 1;
                        }
                        return;
                    }
                    Some(o) => o,
                };
                // the destroyed object must be one of the recently hit ones
                let pos = match self.hit.iter().rposition(|e| e.0 == o.id) {
                    Some(p) => p,
                    None => return,
                };
                let was_air = self.hit[pos].2;
                let matches = if self.smackable_filter.is_empty() {
                    o.is_opponent
                } else if o.is_smackable {
                    partial_match_nocase(&o.smackable_name, &self.smackable_filter)
                } else {
                    false
                };
                let mut count = self.destroyed;
                if !self.in_air || was_air {
                    if (!self.drifting || *drifting_player) && matches {
                        if 0.0 < self.time_limit && self.destroyed == 0 {
                            self.timer = 0.0;
                        }
                        self.destroyed += 1;
                        count = self.destroyed;
                    }
                }
                if self.objects_destroyed <= count && 0.0 < self.time_limit {
                    self.destroyed = 0;
                    let t = self.timer;
                    self.timer = 0.0;
                    if t < self.best_time {
                        self.best_time = t;
                    }
                }
                self.hit.remove(pos);
            }
            ChallengeEvent::RaceStart => {
                self.hit.clear();
                if !persistent {
                    self.destroyed = 0;
                }
                self.saved = self.destroyed;
                self.total = env
                    .destroyable_items
                    .iter()
                    .filter(|n| self.smackable_filter.is_empty() || partial_match_nocase(n, &self.smackable_filter))
                    .count() as i32;
                self.timer = 0.0;
                self.best_time = f32::INFINITY;
            }
            ChallengeEvent::RaceFinish { car } => {
                if car.finished {
                    return;
                }
                self.destroyed = self.saved;
            }
            ChallengeEvent::RaceQuit { .. } | ChallengeEvent::RaceRestart => self.destroyed = self.saved,
            ChallengeEvent::Reset => {
                self.destroyed = 0;
                self.hit.clear();
            }
            _ => {}
        }
    }
    /// `GetDestroyedPercent @002055f8`.
    pub fn destroyed_percent(&self) -> f32 {
        if self.total != 0 {
            (self.destroyed as f32 * 100.0) / self.total as f32
        } else {
            0.0
        }
    }
    /// `IsCompletedInternal @001fb7ac`.
    pub fn is_completed(&self) -> bool {
        if self.time_limit > 0.0 {
            return self.best_time <= self.time_limit;
        }
        if self.objects_destroyed == 0 {
            return self.percent as f32 <= self.destroyed_percent();
        }
        self.objects_destroyed <= self.destroyed
    }
}
// ---- classes whose update logic is not ported ----------------------------------------------------------------------
/// A challenge class whose XML properties are parsed (every child element as a name/value pair) but whose
/// `OnEvent` / `IsCompletedInternal` are NOT ported: `CChallengeSpinAndWin @00206304`, `CChallengeNearlyMissObstacles @00206098`, `CChallengeAvoidObstacles @00205e20`,
/// `CChallengeTravelDistance @00204e30`, `CChallengeBeatScore @002061dc`.
/// UNRESOLVED, per class: `AvoidObstacles` (42) and `NearlyMissObstacles` (23) test every track item against the spline
/// (`CEventDefinitionManager::GetTrackItemData`, `CSpline::GetLateralOffset`, collision-box extents); `TravelDistance` (18)
/// reads wheel `PhysMaterial_GetWearRate`; `BeatScore` (3) is driven from outside (friend / own best score);
/// `SpinAndWin` (2) uses an `acos` whose arguments Ghidra lost. The lead should leave these out of the active sets or
/// treat them as never completing. Decomp: AvoidObstacles `OnEvent @00202008`, NearlyMiss `@00201814`, TravelDistance
/// `@002072c4`, SpinAndWin `@002004b8`, BeatScore `@001f9894`.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChUnported {
    pub class: String,
    pub props: Vec<(String, String)>,
}

/// One concrete challenge class.
#[derive(Clone, Debug, PartialEq)]
pub enum ChallengeKind {
    RaceTime(ChRaceTime),
    Score(ChScore),
    CollectCoins(ChCollectCoins),
    Drift(ChDrift),
    UseBoostPad(ChUseBoostPad),
    UsePowerUp(ChUsePowerUp),
    UseAbility(ChUseAbility),
    WinWithKart(ChWinWithKart),
    WinWithDifferentKarts(ChWinWithDifferentKarts),
    Jump(ChJump),
    Launch(ChLaunch),
    Hit(ChHit),
    RacePosition(ChRacePosition),
    Overtake(ChOvertake),
    PositionInTime(ChPositionInTime),
    Slipstream(ChSlipstream),
    CollectFruit(ChCollectFruit),
    Get3Stars(ChGet3Stars),
    Destroy(ChDestroy),
    Unported(ChUnported),
}

/// A challenge definition + its runtime state (`CChallenge`: `+4` name, `+8` category, `+0xc` localisation key of the
/// description, `+0x10` flags: bit0 `Persistent`).
#[derive(Clone, Debug, PartialEq)]
pub struct Challenge {
    pub name: String,
    pub category: String,
    /// `Description="MISSION_FINISH_RACE"` (a `CLoc::String` key).
    pub description: String,
    pub persistent: bool,
    /// Flag bit1: completed and latched (set by the manager once `is_completed()` was seen true).
    pub locked: bool,
    pub kind: ChallengeKind,
}

/// External lookups the parser needs (they live in other systems).
pub struct ChallengeResolvers<'a> {
    /// `CBaseAbility::GetBirdAbilityFromString(name)` (abilities.rs).
    pub ability_by_name: &'a dyn Fn(&str) -> i32,
    /// `CSmackableManager::GetSmackableTypeFromHelperName(name)`; a value `>= 0x7e` means unknown.
    pub smackable_by_helper: &'a dyn Fn(&str) -> u32,
}

impl Challenge {
    /// `CChallengeManager::Init(char const*) @001f5404..` (template part): one `<ChallengeXxx Name= Category=
    /// Persistent= Description=>` element with the class-specific children read by that class's `OnEvent(Read)`.
    pub fn from_xml(n: &roxmltree::Node, res: &ChallengeResolvers) -> Option<Challenge> {
        let class = n.tag_name().name();
        let kind = match class {
            "ChallengeRaceTime" => ChallengeKind::RaceTime(ChRaceTime::read(n)),
            "ChallengeScore" => ChallengeKind::Score(ChScore::read(n)),
            "ChallengeCollectCoins" => ChallengeKind::CollectCoins(ChCollectCoins::read(n)),
            "ChallengeDrift" => ChallengeKind::Drift(ChDrift::read(n)),
            "ChallengeUseBoostPad" => ChallengeKind::UseBoostPad(ChUseBoostPad::read(n)),
            "ChallengeUsePowerUp" => ChallengeKind::UsePowerUp(ChUsePowerUp::read(n)),
            "ChallengeUseAbility" => ChallengeKind::UseAbility(ChUseAbility::read(n, res.ability_by_name)),
            "ChallengeWinWithKart" => ChallengeKind::WinWithKart(ChWinWithKart::read(n)),
            "ChallengeWinWithDifferentKarts" => ChallengeKind::WinWithDifferentKarts(ChWinWithDifferentKarts::read(n)),
            "ChallengeJump" => ChallengeKind::Jump(ChJump::read(n)),
            "ChallengeLaunch" => ChallengeKind::Launch(ChLaunch::read(n)),
            "ChallengeHit" => ChallengeKind::Hit(ChHit::read(n, res.smackable_by_helper)),
            "ChallengeRacePosition" => ChallengeKind::RacePosition(ChRacePosition::read(n)),
            "ChallengeOvertake" => ChallengeKind::Overtake(ChOvertake::read(n)),
            "ChallengePositionInTime" => ChallengeKind::PositionInTime(ChPositionInTime::read(n)),
            "ChallengeSlipstream" => ChallengeKind::Slipstream(ChSlipstream::read(n)),
            "ChallengeCollectFruit" => ChallengeKind::CollectFruit(ChCollectFruit::read(n)),
            "ChallengeGet3Stars" => ChallengeKind::Get3Stars(ChGet3Stars::read(n)),
            "ChallengeDestroy" => ChallengeKind::Destroy(ChDestroy::read(n)),
            "ChallengeSpinAndWin" | "ChallengeNearlyMissObstacles" | "ChallengeAvoidObstacles"
            | "ChallengeTravelDistance" | "ChallengeBeatScore" => {
                let mut props = Vec::new();
                for ch in n.children().filter(|c| c.is_element()) {
                    props.push((ch.tag_name().name().to_string(), ch.text().unwrap_or("").trim().to_string()));
                }
                ChallengeKind::Unported(ChUnported { class: class.to_string(), props })
            }
            _ => return None,
        };
        Some(Challenge {
            name: n.attribute("Name")?.to_string(),
            category: n.attribute("Category").unwrap_or("").to_string(),
            description: n.attribute("Description").unwrap_or("").to_string(),
            persistent: n.attribute("Persistent").map(c_is_true).unwrap_or(false),
            locked: false,
            kind,
        })
    }

    /// `CChallenge::OnEvent(CChallengeEvent const&)` dispatch (`vtable+0xc`).
    pub fn on_event(&mut self, ev: &ChallengeEvent, env: &ChallengeEnv, now_ms: u32) {
        let p = self.persistent;
        match &mut self.kind {
            ChallengeKind::RaceTime(c) => c.on_event(ev, env),
            ChallengeKind::Score(c) => c.on_event(ev, p, env),
            ChallengeKind::CollectCoins(c) => c.on_event(ev, p),
            ChallengeKind::Drift(c) => c.on_event(ev, p),
            ChallengeKind::UseBoostPad(c) => c.on_event(ev, p),
            ChallengeKind::UsePowerUp(c) => c.on_event(ev, p, env),
            ChallengeKind::UseAbility(c) => c.on_event(ev, p),
            ChallengeKind::WinWithKart(c) => c.on_event(ev, env),
            ChallengeKind::WinWithDifferentKarts(c) => c.on_event(ev, env),
            ChallengeKind::Jump(c) => c.on_event(ev, p),
            ChallengeKind::Launch(c) => c.on_event(ev, env),
            ChallengeKind::Hit(c) => c.on_event(ev, p, now_ms),
            ChallengeKind::RacePosition(c) => c.on_event(ev, env),
            ChallengeKind::Overtake(c) => c.on_event(ev),
            ChallengeKind::PositionInTime(c) => c.on_event(ev, env),
            ChallengeKind::Slipstream(c) => {
                let draft = if let ChallengeEvent::Update { car, .. } = ev { car.draft } else { 0.0 };
                let locked = self.locked;
                c.on_event(ev, p, locked, draft)
            }
            ChallengeKind::CollectFruit(c) => c.on_event(ev, p, env),
            ChallengeKind::Get3Stars(c) => c.on_event(ev, p, env),
            ChallengeKind::Destroy(c) => c.on_event(ev, p, env),
            ChallengeKind::Unported(_) => {}
        }
    }

    /// `CChallenge::IsCompleted()` -> `IsCompletedInternal()`. `None` for the unported classes.
    pub fn is_completed(&self, env: &ChallengeEnv) -> Option<bool> {
        Some(match &self.kind {
            ChallengeKind::RaceTime(c) => c.is_completed(),
            ChallengeKind::Score(c) => c.is_completed(env),
            ChallengeKind::CollectCoins(c) => c.is_completed(),
            ChallengeKind::Drift(c) => c.is_completed(),
            ChallengeKind::UseBoostPad(c) => c.is_completed(),
            ChallengeKind::UsePowerUp(c) => c.is_completed(),
            ChallengeKind::UseAbility(c) => c.is_completed(),
            ChallengeKind::WinWithKart(c) => c.is_completed(),
            ChallengeKind::WinWithDifferentKarts(c) => c.is_completed(),
            ChallengeKind::Jump(c) => c.is_completed(self.persistent),
            ChallengeKind::Launch(c) => c.is_completed(),
            ChallengeKind::Hit(c) => c.is_completed(),
            ChallengeKind::RacePosition(c) => c.is_completed(),
            ChallengeKind::Overtake(c) => c.is_completed(),
            ChallengeKind::PositionInTime(c) => c.is_completed(),
            ChallengeKind::Slipstream(c) => c.is_completed(),
            ChallengeKind::CollectFruit(c) => c.is_completed(),
            ChallengeKind::Get3Stars(c) => c.is_completed(),
            ChallengeKind::Destroy(c) => c.is_completed(),
            ChallengeKind::Unported(_) => return None,
        })
    }
}

/// One `<Event Filename=...>` of an `<Episode>`: the challenge names the event offers
/// (`SChallengeEvent`: list at `+0x64`, `StartLoop/StartSilver/StartGolden` at `+0x58/+0x5c/+0x60`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventChallenges {
    pub filename: String,
    pub start_loop: Option<String>,
    pub start_silver: Option<String>,
    pub start_golden: Option<String>,
    /// Challenge names in list order (`CChallengeList`: a plain vector, duplicates allowed).
    pub names: Vec<String>,
}

/// `<Episode Name="episode_main_01">`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpisodeChallenges {
    pub name: String,
    pub events: Vec<EventChallenges>,
}

/// `CChallengeManager` data (`GMISC:Challenges.xml`, `CChallengeManager::Init @001f7ae8`..): the challenge templates and
/// the per-episode / per-event lists, built for one platform (the Android build ignores `Platform="..."` children
/// whose platform is not `Android`).
#[derive(Clone, Debug, Default)]
pub struct ChallengeManager {
    pub version: String,
    pub templates: Vec<Challenge>,
    pub episodes: Vec<EpisodeChallenges>,
}

impl ChallengeManager {
    /// `CChallengeManager::Init(char const* file)`: parse templates and episodes. `platform` is `"Android"` for the
    /// shipped build (a child with a different `Platform` is skipped, one without is always applied).
    pub fn from_xml(xml: &str, platform: &str, res: &ChallengeResolvers) -> Option<ChallengeManager> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let root = doc.root_element();
        let mut m = ChallengeManager { version: root.attribute("Version").unwrap_or("").to_string(), ..Default::default() };
        let plat_ok = |n: &roxmltree::Node| n.attribute("Platform").map(|p| p.eq_ignore_ascii_case(platform)).unwrap_or(true);
        if let Some(chs) = root.children().find(|c| c.is_element() && c.tag_name().name() == "Challenges") {
            for n in chs.children().filter(|c| c.is_element()) {
                if let Some(c) = Challenge::from_xml(&n, res) {
                    m.templates.push(c);
                }
            }
        }
        let exists = |m: &ChallengeManager, name: &str| m.templates.iter().any(|t| t.name.eq_ignore_ascii_case(name));
        for ep in root.children().filter(|c| c.is_element() && c.tag_name().name() == "Episode") {
            let mut e = EpisodeChallenges { name: ep.attribute("Name").unwrap_or("").to_string(), events: Vec::new() };
            for ev in ep.children().filter(|c| c.is_element() && c.tag_name().name() == "Event") {
                let filename = match ev.attribute("Filename") {
                    Some(f) => f.to_string(),
                    None => continue,
                };
                let mut entry = EventChallenges { filename: filename.clone(), ..Default::default() };
                // FromEpisode + FromEvent: copy the lists of an event parsed earlier
                if let (Some(fe), Some(fv)) = (ev.attribute("FromEpisode"), ev.attribute("FromEvent")) {
                    let src = if fe.eq_ignore_ascii_case(&e.name) { e.events.iter().find(|x| x.filename.eq_ignore_ascii_case(fv)) } else { m.episodes.iter().find(|x| x.name.eq_ignore_ascii_case(fe)).and_then(|x| x.events.iter().find(|y| y.filename.eq_ignore_ascii_case(fv))) };
                    if let Some(s) = src {
                        entry.names = s.names.clone();
                        entry.start_loop = s.start_loop.clone();
                        entry.start_silver = s.start_silver.clone();
                        entry.start_golden = s.start_golden.clone();
                    }
                }
                if let Some(v) = ev.attribute("StartLoop") {
                    entry.start_loop = Some(v.to_string());
                }
                if let Some(v) = ev.attribute("StartSilver") {
                    entry.start_silver = Some(v.to_string());
                }
                if let Some(v) = ev.attribute("StartGolden") {
                    entry.start_golden = Some(v.to_string());
                }
                for ch in ev.children().filter(|c| c.is_element()) {
                    let tag = ch.tag_name().name();
                    if tag.eq_ignore_ascii_case("Challenge") {
                        if plat_ok(&ch) {
                            if let Some(nm) = ch.attribute("Name") {
                                if exists(&m, nm) {
                                    entry.names.push(nm.to_string());
                                }
                            }
                        }
                    } else if tag.eq_ignore_ascii_case("Replace") {
                        if let (Some(old), Some(new)) = (ch.attribute("Old"), ch.attribute("New")) {
                            if plat_ok(&ch) && exists(&m, new) {
                                // `FindChallenge` returns the LAST occurrence
                                if let Some(i) = entry.names.iter().rposition(|x| x.eq_ignore_ascii_case(old)) {
                                    entry.names[i] = new.to_string();
                                }
                            }
                        }
                    } else if tag.eq_ignore_ascii_case("Remove") {
                        if plat_ok(&ch) {
                            if let Some(nm) = ch.attribute("Name") {
                                if let Some(i) = entry.names.iter().rposition(|x| x.eq_ignore_ascii_case(nm)) {
                                    entry.names.remove(i);
                                }
                            }
                        }
                    }
                }
                e.events.push(entry);
            }
            m.episodes.push(e);
        }
        Some(m)
    }

    /// Reads `<ASSETS292>/xml/gameplay/misc/challenges.xml` for `platform`.
    pub fn load(platform: &str, res: &ChallengeResolvers) -> Option<ChallengeManager> {
        let p = format!("{}/xml/gameplay/misc/challenges.xml", ASSETS292);
        ChallengeManager::from_xml(&std::fs::read_to_string(p).ok()?, platform, res)
    }

    /// `CChallengeManager::FindChallengeTemplate` + `CChallenge::CopyInstance`: a fresh runtime copy of a template.
    pub fn instantiate(&self, name: &str) -> Option<Challenge> {
        self.templates.iter().find(|t| t.name.eq_ignore_ascii_case(name)).cloned()
    }

    /// The challenge names of `episode`/`event` (e.g. `"episode_main_01"`, `"EventDef_Episode01_Event00_StageXX.xml"`).
    pub fn event_challenges(&self, episode: &str, event: &str) -> Option<&EventChallenges> {
        self.episodes.iter().find(|e| e.name.eq_ignore_ascii_case(episode))?.events.iter().find(|v| v.filename.eq_ignore_ascii_case(event))
    }

    /// `CChallengeManager::Event(const CChallengeEvent&) @001f6604`: forwards `ev` to the (up to three) active
    /// challenges, last slot first (`for i in 2..=0`). The original suppresses all events in game mode 10 and while
    /// the manager's pause flag (`+0x34`) is set, except the event whose id equals `DAT_001f6ef8`.
    pub fn dispatch(active: &mut [Challenge], ev: &ChallengeEvent, env: &ChallengeEnv, now_ms: u32) {
        for c in active.iter_mut().take(3).rev() {
            c.on_event(ev, env, now_ms);
        }
    }
}

// ============================================================================================================
// Tests (run against the real assets292 data; each one skips when the data is missing)
// ============================================================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn have_assets() -> bool {
        std::path::Path::new(ASSETS292).join("xml/gameplay/misc/challenges.xml").exists()
    }

    #[test]
    fn powerup_state_machine() {
        let mut p = PlayerPowerups::default();
        assert_eq!(p.set_active(0, false), set_active_err::NONE_IN_STOCK);
        assert_eq!(p.set_active(4, false), set_active_err::BAD_INDEX);
        p.add_charges(0, 2);
        p.add_charges(3, 1);
        assert!(p.has_powerups_flag);
        assert_eq!(p.count_by_index(0), 2);
        assert_eq!(p.total_gained[0], 2);
        assert_eq!(p.set_active(0, false), 1);
        assert_eq!(p.set_active(0, false), set_active_err::ALREADY_ACTIVE);
        assert!(p.is_active(0));
        // sponsored slot: free while a campaign is active
        assert_eq!(p.set_active(3, false), 1);
        assert_eq!(p.consume_all_selected(true), 1);
        assert_eq!(p.count_by_index(0), 1);
        assert_eq!(p.count_by_index(3), 1);
        assert_eq!(p.consume_all_selected(false), 1);
        assert_eq!(p.count_by_index(0), 0);
        assert_eq!(p.count_by_index(3), 0);
        // nothing left: a chosen power-up cannot be paid
        assert_eq!(p.consume_all_selected(false), 0);
        p.clear_selected();
        assert!(!p.is_active(0));
        // forced activation skips the stock check (sponsored speed booster)
        assert!(p.sponsored_speedbooster_on_game_begin(true, true, true));
        assert!(p.is_active(2));
        assert_eq!(p.consume(7, false), 0);
        assert_eq!(p.consume(3, true), 1);
        assert_eq!(powerup_enum_by_text("speedbooster"), 2);
        assert_eq!(powerup_enum_by_text("TargetCar"), -1);
        assert_eq!(powerup_string_by_enum(3), Some("Powerup_PartnerCar"));
        assert!(is_sponsored_power_up(3));
        assert_eq!(power_up_loc_key(2), "POWERUP_LEAFBLOWER");
        assert!((partner_car_level_fraction(12) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn debug_tweakables_match_the_xml() {
        if !have_assets() {
            return;
        }
        let t = DebugTweakables::load().expect("debugtweakables.xml");
        assert_eq!(t.autorepair_start_delay, 2.0);
        assert_eq!(t.autorepair_vfx_delay, 1.0);
        assert_eq!(t.autorepair_damaged_pieces, 1);
        assert_eq!(t.boost_push_factor, 4.0);
        assert!((t.king_sling_force_multiplier - 1.4024).abs() < 1e-6);
        assert_eq!(t.king_sling_catchup_disabled_time, 10.0);
        // `<Powerup>` section values (the stray 2.00 Boost_Camera_Start_Delay under `<Gameplay>` is not read)
        assert_eq!((t.drift_angle_start, t.drift_angle_stop, t.drift_speed_start), (0.2, 0.2, 15.0));
        assert_eq!((t.boost_camera_max_distance, t.boost_camera_start_speed, t.boost_camera_end_speed, t.boost_camera_start_delay), (1.0, 1.0, 1.01, 0.0));
        assert_eq!(t, DebugTweakables::default(), "Default == shipped xml");
        let g = GameplayTweakables::load().expect("gameplaytweakables.xml");
        assert!((g.competitor_damage - 1.1).abs() < 1e-6);
        assert_eq!(g.max_one_hit, 30.0);
        assert_eq!(g.crash[6], 0.8);
        assert_eq!(g.crash[7], 2.4);
        let txt = std::fs::read_to_string(format!("{}/xml/gameplay/misc/debugtweakables.xml", ASSETS292)).unwrap();
        let gifts = InitialGifts::from_xml(&txt).unwrap();
        assert_eq!(gifts.sling, 1);
        assert_eq!(gifts.partner, None, "xml spells the key Initial_Partner_Gift, the binary reads Initial_PartnerCar_Gift");
    }

    #[test]
    fn auto_repair_triggers_after_the_start_delay_and_repairs_after_the_vfx_delay() {
        let tw = DebugTweakables::default();
        let mut ar = AutoRepair::default();
        let mut body = BodyworkDamage::default();
        body.state[3] = 3;
        body.state[5] = 3; // two pieces > 2: count 2 > Autorepair_Damaged_Pieces (1)
        body.has_smackable[3] = true;
        body.wheel_damage = [5.0; 4];
        assert_eq!(body.damaged_piece_count(), 2 + 4);
        let mut fx = Vec::new();
        // armed as soon as the accumulated time exceeds Autorepair_Start_Delay (2.0 s)
        let mut steps = 0;
        while !ar.repairing && steps < 40 {
            ar.update(0.1, 0, true, true, &mut body, &tw, &mut fx);
            steps += 1;
        }
        assert!((20..=21).contains(&steps), "armed after ~2.0 s, took {steps} steps");
        assert!(fx.is_empty());
        for _ in 0..8 {
            ar.update(0.1, 0, true, true, &mut body, &tw, &mut fx);
        }
        assert!(ar.repairing, "VFX delay 1.0 s not over yet");
        for _ in 0..3 {
            ar.update(0.1, 0, true, true, &mut body, &tw, &mut fx);
        }
        assert!(!ar.repairing);
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Repair { fraction, .. } if *fraction == 1.0)));
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Other { tag: "RemoveSmackable", .. })));
        assert_eq!(body.state[3], 0);
        assert_eq!(body.wheel_damage, [0.0; 4]);
        // a detached ancestor blocks counting
        let mut b2 = BodyworkDamage::default();
        b2.head = 0;
        b2.state[0] = 4;
        b2.state[1] = 3;
        let mut ar2 = AutoRepair::default();
        for _ in 0..40 {
            ar2.update(0.1, 0, true, true, &mut b2, &tw, &mut fx);
        }
        assert!(!ar2.repairing);
        // not the player: no state change
        let mut ar3 = AutoRepair::default();
        ar3.update(5.0, 0, false, true, &mut body, &tw, &mut fx);
        assert_eq!(ar3.accum, 0.0);
    }

    #[test]
    fn integrate_powerups_requests_the_boost_vfx_and_sounds() {
        let tw = DebugTweakables::default();
        let mut pu = PlayerPowerups::default();
        pu.add_charges(2, 1);
        pu.set_active(2, false);
        let inp = PowerupInput {
            car: 1,
            is_player: true,
            character: 0,
            repairing: false,
            ability_type4_active: false,
            boost_suppressed: false,
            branded: false,
            local_audio: true,
            rb_98: 0.1,
            boost_start_sound_playing: true,
        };
        let mut vfx = PowerupVfx::default();
        let f = integrate_powerups(&inp, &pu, &mut vfx, &tw);
        assert!(f.boost);
        assert!(vfx.boost_vfx);
        assert!(f.effects.iter().any(|e| matches!(e, CarEffect::Particle { name, .. } if name.starts_with("SpeedBoost/"))));
        assert!(f.effects.iter().any(|e| matches!(e, CarEffect::Sound { name, .. } if name == "ABY_powerup_boost_start")));
        assert!((vfx.boost_ramp - 0.5).abs() < 1e-6);
        let mut inp2 = inp;
        inp2.boost_start_sound_playing = false;
        let f2 = integrate_powerups(&inp2, &pu, &mut vfx, &tw);
        assert!(f2.effects.iter().any(|e| matches!(e, CarEffect::Sound { name, .. } if name == "ABY_powerup_boost_loop")));
        pu.unset(2);
        let f3 = integrate_powerups(&inp2, &pu, &mut vfx, &tw);
        assert!(!f3.boost);
        assert!(f3.effects.iter().any(|e| matches!(e, CarEffect::Sound { name, .. } if name == "ABY_powerup_boost_stop")));
        // repair vfx
        pu.add_charges(1, 1);
        pu.set_active(1, false);
        let mut inp3 = inp;
        inp3.repairing = true;
        let f4 = integrate_powerups(&inp3, &pu, &mut vfx, &tw);
        assert!(f4.effects.iter().any(|e| matches!(e, CarEffect::Sound { name, .. } if name == "ABY_powerup_autorepair")));
        assert!(vfx.repair_vfx);
        // KingSling
        pu.add_charges(0, 1);
        pu.set_active(0, false);
        assert!((king_sling_launch_scale(&pu, true, &tw) - 1.4024).abs() < 1e-6);
        assert_eq!(catchup_disabled_floor(0.0, true, &pu, &tw, false), 10.0);
        pu.unset(0);
        assert_eq!(catchup_disabled_floor(0.0, true, &pu, &tw, true), 5.0);
        assert_eq!(catchup_disabled_floor(0.0, true, &pu, &tw, false), 0.0);
        assert_eq!(catchup_disabled_floor(1.0, false, &pu, &tw, true), 1.0, "AI cars are untouched");
    }

    #[test]
    fn boost_camera_ramps() {
        let tw = DebugTweakables { boost_camera_max_distance: 2.0, boost_camera_start_speed: 10.0, boost_camera_end_speed: 30.0, boost_camera_start_delay: 1.0, ..Default::default() };
        assert_eq!(boost_cam_behind_mod(5.0, 5.0, &tw), 0.0);
        assert!((boost_cam_behind_mod(20.0, 0.5, &tw) - 0.5).abs() < 1e-6);
        assert!((boost_cam_behind_mod(99.0, 9.0, &tw) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn pickups() {
        assert_eq!(pickup_kind_from_helper_name("pickup_coin"), Some(PickupKind::Coin));
        assert_eq!(pickup_kind_from_helper_name("PICKUP_COIN_03"), Some(PickupKind::Coin));
        assert_eq!(pickup_kind_from_helper_name("pickup_seedrushtoken_large"), Some(PickupKind::SeedRushTokenLarge));
        assert_eq!(pickup_kind_from_helper_name("boost_pad"), Some(PickupKind::Boost));
        assert_eq!(pickup_kind_from_helper_name("ai_hotspot_large"), Some(PickupKind::AIHotspot(2)));
        assert_eq!(pickup_kind_from_helper_name("nothing"), None);
        let ctx = PickupCtx { coin_doubler: false, mp_idle: true, fruit_rush_mode: 5, gift_box_opened_before: true };
        let car = PickupCar { id: 0, is_player: true, is_local_player: true, player_index: 0, flag_1af4: false, pos: Vec3::ZERO, axis0: Vec3::X, axis1: Vec3::Y };
        let mut coin = Pickup::new(PickupKind::Coin, Vec3::new(1.0, 0.0, 0.0), 0.5);
        assert!(coin.can_be_picked(Some(&car), &ctx, false));
        assert!(coin.is_in_radius(Vec3::ZERO, 0.6));
        assert!(!coin.is_in_radius(Vec3::ZERO, 0.4));
        let ev = coin.on_car_in_radius(&car, &ctx, -1.0, false);
        assert_eq!(ev[0], PickupEvent::Coins { car: 0, value: 1 });
        assert!(!coin.can_be_picked(Some(&car), &ctx, false));
        coin.update(0.15, Some((Vec3::ZERO, Vec3::X, Vec3::Y)));
        assert!(coin.active);
        coin.update(0.1, None);
        assert!(!coin.active, "gone after 0.2 s");
        let dbl = PickupCtx { coin_doubler: true, ..ctx };
        assert_eq!(coin_value(&dbl), 2);
        assert_eq!(coins_value(MEGA_COIN_VALUE, &dbl), 100);
        assert_eq!(add_soft_currency(999_999_990, 50), 999_999_999);
        assert_eq!(add_soft_currency(10, -5), 10);
        assert_eq!(add_soft_currency(10, 5), 15);
        // fruit rush token
        let fruit = PickupCtx { fruit_rush_mode: 0, ..ctx };
        let mut tok = Pickup::new(PickupKind::SeedRushToken, Vec3::ZERO, 0.5);
        tok.token_kind = 2;
        let ev = tok.on_car_in_radius(&car, &fruit, -0.1, false);
        assert_eq!(ev, vec![PickupEvent::Fruit { car: 0, kind: 2 }]);
        assert_eq!(seed_token_particle_name(2), Some("StrawberryDestroyed"));
        // nearly miss once
        let mut tok2 = Pickup::new(PickupKind::SeedRushToken, Vec3::ZERO, 0.5);
        assert_eq!(tok2.on_car_in_radius(&car, &fruit, 0.4, true).len(), 1);
        assert!(tok2.on_car_in_radius(&car, &fruit, 0.4, true).is_empty());
        assert!((pickup_fly_offset(0.0) - 1.65 - 0.675).abs() < 1e-4);
        assert!((pickup_fly_offset(0.1) - (1.65 + 1.35)).abs() < 1e-4);
        // boost pad
        let mut pad = Pickup::new(PickupKind::Boost, Vec3::ZERO, 1.0);
        assert_eq!(pad.on_car_in_radius(&car, &ctx, 0.0, false), vec![PickupEvent::BoostPad { car: 0 }]);
    }

    #[test]
    fn score_system_matches_the_formulas_and_config() {
        if !have_assets() {
            return;
        }
        let cfg = ScoreConfig::load().expect("scoreconfig.xml");
        assert_eq!(cfg.top_speed.unwrap().0, [1.35, 150.0, 0.1, 50.0]);
        assert_eq!(cfg.top_speed.unwrap().1, [0.9, 1.0, 1.1, 0.35, 0.5]);
        assert_eq!(cfg.acceleration.unwrap().0, [0.5, 1.0, 35.0]);
        assert_eq!(cfg.deaths.unwrap(), [1.5, 0.5]);
        assert_eq!(cfg.time.unwrap(), [0.0, 0.35, 1.0, 0.125]);
        assert_eq!(cfg.fruit.unwrap(), [0.0, 1.0, 0.125, 1.0]);
        assert_eq!(cfg.hit_boost_pad, Some(0));
        let mut s = ScoreSystem::new(cfg);
        s.set_race_cc(100);
        assert_eq!(s.max_race_score, 15000);
        // finishing position: (9 - pos) / 8 * S
        s.set_finishing_position(1);
        assert_eq!(s.position.unwrap().1, 15000);
        s.set_finishing_position(5); // once only
        assert_eq!(s.position.unwrap().1, 15000);
        assert_eq!(s.get_bonus_score(4), 15000);
        // finishing time quirk: always S * MaxTimeScore
        s.set_finishing_time(GAME_MODE_TIME_ATTACK, 0.2);
        assert_eq!(s.time.unwrap().1, 15000);
        assert_eq!(s.get_bonus_score(GAME_MODE_TIME_ATTACK), 15000);
        // fruit: half way between min (0.125 S) and max (1.0 S)
        s.set_fruit_percent(GAME_MODE_SEED_RUSH, 0.5);
        assert_eq!(s.fruit.unwrap().1, (15000.0f32 * 0.125 + 0.5 * (15000.0 - 15000.0 * 0.125)) as i32);
        // deaths: (S / 1.5) * 0.5^deaths
        s.add_death();
        s.add_death();
        let theme = 0;
        // top speed / acceleration need a kart cc
        let frame = ScoreFrame {
            dt: 10.0,
            now_ms: 0,
            game_mode: 4,
            gate: true,
            suspended: false,
            pos: Vec3::ZERO,
            wheels_on_ground: 4,
            draft: 0.0,
            drifting: false,
            speed: 100.0,
            kart_top_speed: 50.0,
            kart_cc: 100,
        };
        s.update_player(&frame);
        let total = s.get_score(theme);
        // acceleration: (0.5 * 10 * 100^1 + 100 * 35) * 1.0 = 4000 ; top speed: (150 * 10 * 100^0.1 + 100*50) * 0.9 ; deaths: 10000 * 0.25 = 2500
        let accel = (0.5f32 * 10.0) * 100.0 + 100.0 * 35.0;
        assert!((accel - 4000.0).abs() < 1.0);
        let top = ((150.0f32 * 10.0) as f64 * 100f64.powf(0.1f32 as f64)) as f32 + 100.0 * 50.0;
        let expect = (accel * 1.0) as i32 + (top * 0.9) as i32 + 2500;
        assert!((total - expect).abs() <= 2, "total {total} vs {expect}");
        // cached until invalidated
        assert_eq!(s.get_score(theme), total);
        s.invalidate();
        s.reset();
        // after a reset deaths = 0 -> S / 1.5; Acceleration has no `cc == -1` guard in the original: (0 * pow(-1, 1) + -1 * 35) = -35
        assert_eq!(s.get_score(theme), 10000 - 35);
        assert_eq!(race_max_score(30), 4500);
        assert!(is_counter_available(ScoreCounterType::Jenga, GAME_MODE_JENGA));
        assert!(!is_counter_available(ScoreCounterType::Drift, GAME_MODE_JENGA));
        // contact buffer
        let mut cb = ContactBuffer::default();
        assert_eq!(cb.on_collision(None, 0), 2);
        assert_eq!(cb.on_collision(Some(7), 0), 0);
        assert_eq!(cb.on_collision(Some(7), 100), 1);
        assert_eq!(cb.on_collision(Some(7), 6000), 0, "older than 5 s is forgotten");
    }

    #[test]
    fn distance_counter_needs_the_minimum_run() {
        let mut d = DistanceCounter::new(true);
        d.per_meter = 2.0;
        d.min_distance = 5.0;
        d.update(true, false, Vec3::ZERO, 0);
        d.update(true, false, Vec3::new(3.0, 0.0, 0.0), 0);
        assert_eq!(d.get_score(), 0, "run shorter than MinDistance does not score");
        d.update(true, false, Vec3::new(10.0, 0.0, 0.0), 0);
        assert_eq!(d.get_score(), 20);
        d.update(false, false, Vec3::new(12.0, 0.0, 0.0), 0);
        assert_eq!(d.get_score(), 24);
    }

    fn resolvers<'a>() -> ChallengeResolvers<'a> {
        ChallengeResolvers { ability_by_name: &|s: &str| if s.is_empty() { 0 } else { 1 + (s.len() as i32 % 7) }, smackable_by_helper: &|_s: &str| 0x7e }
    }

    #[test]
    fn challenges_xml_parses_and_events_are_listed() {
        if !have_assets() {
            return;
        }
        let m = ChallengeManager::load("Android", &resolvers()).expect("challenges.xml");
        assert_eq!(m.version, "8");
        let doc_text = std::fs::read_to_string(format!("{}/xml/gameplay/misc/challenges.xml", ASSETS292)).unwrap();
        let doc = roxmltree::Document::parse(&doc_text).unwrap();
        let n_templates = doc.descendants().find(|n| n.has_tag_name("Challenges")).unwrap().children().filter(|c| c.is_element()).count();
        assert_eq!(m.templates.len(), n_templates, "every class element must have a parser");
        let ev = m.event_challenges("episode_main_01", "EventDef_Episode01_Event00_StageXX.xml").unwrap();
        assert_eq!(ev.start_loop.as_deref(), Some("Win5xInSeq"));
        assert_eq!(&ev.names[..3], &["FinishMax3rd".to_string(), "Get40Coins1Race".to_string(), "Hit1Opp1Race".to_string()]);
        // FromEvent copy
        let copy = m.event_challenges("episode_main_01", "EventDef_Episode01_Event04_StageXX.xml").unwrap();
        assert!(!copy.names.is_empty());
        let r = m.instantiate("Win3xInSeq").unwrap();
        assert!(r.persistent);
        match r.kind {
            ChallengeKind::RacePosition(ref c) => {
                assert_eq!(c.position, 1);
                assert_eq!(c.counter, 3);
                assert!(c.in_sequence);
            }
            _ => panic!("Win3xInSeq must be a RacePosition"),
        }
        let d = m.instantiate("DriftInRace").unwrap();
        match d.kind {
            ChallengeKind::Drift(ref c) => {
                assert_eq!(c.target_count, 1);
                assert!(c.continuous, "Continuous defaults to true");
            }
            _ => panic!(),
        }
    }

    fn env<'a>(pu: &'a PlayerPowerups, others: &'a [Vec3]) -> ChallengeEnv<'a> {
        ChallengeEnv { powerups: pu, score: 0, stars: 3, kart_name: "kart", num_cars: 8, player_character: 2, mode_time_left: 0.0, others_pos: others, fruit_rush_mode: 5, num_seed_tokens: 0, game_mode: 4, fruit_bar_full: false, destroyable_items: &[], kart_id: 0, broken_wheels: 0 }
    }

    #[test]
    fn win_in_sequence_counts_consecutive_wins_and_resets_on_a_loss() {
        if !have_assets() {
            return;
        }
        let m = ChallengeManager::load("Android", &resolvers()).unwrap();
        let mut c = m.instantiate("Win2xInSeq").unwrap();
        let pu = PlayerPowerups::default();
        let e = env(&pu, &[]);
        let finish = |pos: i32| ChallengeEvent::RaceFinish { car: CarSnap { finished: true, final_position: pos, ..Default::default() } };
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        c.on_event(&finish(1), &e, 0);
        assert_eq!(c.is_completed(&e), Some(false));
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        c.on_event(&finish(4), &e, 0);
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        c.on_event(&finish(1), &e, 0);
        assert_eq!(c.is_completed(&e), Some(false), "the loss reset the streak");
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        c.on_event(&finish(1), &e, 0);
        assert_eq!(c.is_completed(&e), Some(true));
    }

    #[test]
    fn drift_challenge_measures_spline_distance_and_count() {
        if !have_assets() {
            return;
        }
        let m = ChallengeManager::load("Android", &resolvers()).unwrap();
        let pu = PlayerPowerups::default();
        let e = env(&pu, &[]);
        let mut c = m.instantiate("Drift10").unwrap();
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        let upd = |drifting: bool, d: f32| ChallengeEvent::Update { dt: 0.1, car: CarSnap { drifting, spline_dist: d, ..Default::default() } };
        c.on_event(&upd(true, 100.0), &e, 0);
        c.on_event(&upd(true, 105.0), &e, 0);
        c.on_event(&upd(false, 108.0), &e, 0);
        assert_eq!(c.is_completed(&e), Some(false), "8 m < 10 m");
        c.on_event(&upd(true, 200.0), &e, 0);
        c.on_event(&upd(false, 212.0), &e, 0);
        assert_eq!(c.is_completed(&e), Some(true), "Continuous: best single drift 12 m >= 10");
        // count based
        let mut c2 = m.instantiate("Drift2x1Race").unwrap();
        c2.on_event(&ChallengeEvent::RaceStart, &e, 0);
        for k in 0..2 {
            c2.on_event(&upd(true, 10.0 * k as f32), &e, 0);
            c2.on_event(&upd(false, 10.0 * k as f32 + 1.0), &e, 0);
        }
        assert_eq!(c2.is_completed(&e), Some(true));
    }

    #[test]
    fn use_boost_pad_and_powerup_and_ability_challenges() {
        if !have_assets() {
            return;
        }
        let m = ChallengeManager::load("Android", &resolvers()).unwrap();
        let mut pu = PlayerPowerups::default();
        pu.add_charges(2, 1);
        pu.set_active(2, false);
        let e = env(&pu, &[]);
        // boost pad x3 (distinct pads, each > 1 s apart)
        let mut c = m.instantiate("UseBoost3x").unwrap();
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        for pad in 1..=3u32 {
            c.on_event(&ChallengeEvent::Boost { pad_id: pad, car: CarSnap::default() }, &e, 0);
            c.on_event(&ChallengeEvent::Update { dt: 1.5, car: CarSnap::default() }, &e, 0);
        }
        assert_eq!(c.is_completed(&e), Some(true));
        // any power-up used at the launch
        let mut u = m.instantiate("UseAnyPowerup").unwrap();
        u.on_event(&ChallengeEvent::RaceStart, &e, 0);
        u.on_event(&ChallengeEvent::Launch { car: CarSnap::default(), others: vec![] }, &e, 0);
        assert_eq!(u.is_completed(&e), Some(true));
        // WinNoPowerUp (RacePosition with DoNotUsePowerUp): fails when a power-up is active
        let mut w = m.instantiate("WinNoPowerUp").unwrap();
        w.on_event(&ChallengeEvent::RaceStart, &e, 0);
        w.on_event(&ChallengeEvent::RaceFinish { car: CarSnap { finished: true, final_position: 1, ..Default::default() } }, &e, 0);
        assert_eq!(w.is_completed(&e), Some(false));
        let none = PlayerPowerups::default();
        let e2 = env(&none, &[]);
        let mut w2 = m.instantiate("WinNoPowerUp").unwrap();
        w2.on_event(&ChallengeEvent::RaceStart, &e2, 0);
        w2.on_event(&ChallengeEvent::RaceFinish { car: CarSnap { finished: true, final_position: 1, ..Default::default() } }, &e2, 0);
        assert_eq!(w2.is_completed(&e2), Some(true));
        // Use different abilities
        let mut a = m.instantiate("UseDiffAbility").unwrap();
        a.on_event(&ChallengeEvent::RaceStart, &e, 0);
        for id in [3, 3, 4, 5] {
            a.on_event(&ChallengeEvent::Ability { ability: id, used: true }, &e, 0);
        }
        assert_eq!(a.is_completed(&e), Some(true), "3 different abilities used (the repeated 3 is ignored)");
    }

    #[test]
    fn coins_hits_and_position_challenges() {
        if !have_assets() {
            return;
        }
        let m = ChallengeManager::load("Android", &resolvers()).unwrap();
        let pu = PlayerPowerups::default();
        let e = env(&pu, &[]);
        let mut c = m.instantiate("Get5Coins1Race").unwrap();
        c.on_event(&ChallengeEvent::RaceStart, &e, 0);
        for _ in 0..5 {
            c.on_event(&ChallengeEvent::Pickup { kind: PickupKind::Coin, token_kind: 0, car: CarSnap::default() }, &e, 0);
        }
        c.on_event(&ChallengeEvent::Pickup { kind: PickupKind::Gem, token_kind: 0, car: CarSnap::default() }, &e, 0);
        assert_eq!(c.is_completed(&e), Some(true));
        let mut h = m.instantiate("Hit2Opp1Race").unwrap();
        h.on_event(&ChallengeEvent::RaceStart, &e, 0);
        h.on_event(&ChallengeEvent::Launch { car: CarSnap::default(), others: vec![] }, &e, 0);
        let hit = |id: u32| ChallengeEvent::Hit { hit: HitInfo { other_id: id, has_object: true, is_opponent: true, ..Default::default() } };
        h.on_event(&hit(11), &e, 1);
        assert_eq!(h.is_completed(&e), Some(false));
        h.on_event(&hit(12), &e, 2);
        assert_eq!(h.is_completed(&e), Some(true));
        let mut f = m.instantiate("FinishMax3rd").unwrap();
        f.on_event(&ChallengeEvent::RaceStart, &e, 0);
        f.on_event(&ChallengeEvent::RaceFinish { car: CarSnap { finished: true, final_position: 4, ..Default::default() } }, &e, 0);
        assert_eq!(f.is_completed(&e), Some(false));
        f.on_event(&ChallengeEvent::RaceStart, &e, 0);
        f.on_event(&ChallengeEvent::RaceFinish { car: CarSnap { finished: true, final_position: 3, ..Default::default() } }, &e, 0);
        assert_eq!(f.is_completed(&e), Some(true));
        let mut t = m.instantiate("FinishIn50").unwrap();
        t.on_event(&ChallengeEvent::RaceStart, &e, 0);
        t.on_event(&ChallengeEvent::RaceFinish { car: CarSnap { finished: true, race_time: 49.0, ..Default::default() } }, &e, 0);
        assert_eq!(t.is_completed(&e), Some(true));
        let mut s = m.instantiate("Score90k").unwrap();
        let mut e3 = env(&pu, &[]);
        e3.score = 95_000;
        s.on_event(&ChallengeEvent::RaceStart, &e3, 0);
        assert_eq!(s.is_completed(&e3), Some(false), "needs Finish");
        s.on_event(&ChallengeEvent::RaceFinish { car: CarSnap { finished: true, ..Default::default() } }, &e3, 0);
        assert_eq!(s.is_completed(&e3), Some(true));
        let mut j = m.instantiate("Jump20").unwrap();
        j.on_event(&ChallengeEvent::RaceStart, &e, 0);
        j.on_event(&ChallengeEvent::Launch { car: CarSnap::default(), others: vec![] }, &e, 0);
        let upd = |w: i32, x: f32| ChallengeEvent::Update { dt: 0.1, car: CarSnap { wheels_on_ground: w, pos: Vec3::new(x, 0.0, 0.0), ..Default::default() } };
        j.on_event(&upd(0, 0.0), &e, 0); // airborne (bit3)
        j.on_event(&upd(4, 0.0), &e, 0); // landed once (bit2)
        j.on_event(&upd(0, 0.0), &e, 0); // jump starts
        j.on_event(&upd(4, 25.0), &e, 0); // lands 25 m away
        assert_eq!(j.is_completed(&e), Some(true));
    }

    #[test]
    fn fruit_get3stars_destroy_launch_overtake_slipstream() {
        if !have_assets() {
            return;
        }
        let m = ChallengeManager::load("Android", &resolvers()).unwrap();
        let pu = PlayerPowerups::default();
        let mut e = env(&pu, &[]);
        e.fruit_rush_mode = 0;
        e.num_seed_tokens = 20;
        e.game_mode = GAME_MODE_SEED_RUSH;
        let car = CarSnap::default();
        // ---- collect fruit: first plain "CollectXFruit" style challenge
        let name = m
            .templates
            .iter()
            .find(|t| matches!(&t.kind, ChallengeKind::CollectFruit(c) if c.amount == 10 && !c.has_filter && c.timer_limit == 0.0 && !c.maximum && !c.less && !c.percentage && !c.full_fruit_bar && !c.nearly_miss && !c.drifting && !c.in_air && !c.no_steering && !c.boosting && !c.use_ability && c.max_fruit == -1))
            .map(|t| t.name.clone());
        if let Some(name) = name {
            let mut c = m.instantiate(&name).unwrap();
            c.on_event(&ChallengeEvent::RaceStart, &e, 0);
            for k in 0..10u8 {
                assert_eq!(c.is_completed(&e), Some(false));
                c.on_event(&ChallengeEvent::Pickup { kind: PickupKind::SeedRushToken, token_kind: k % 3, car: car.clone() }, &e, 0);
            }
            assert_eq!(c.is_completed(&e), Some(true), "{name}");
        }
        // fruit pick-ups are ignored outside fruit rush
        let mut e2 = env(&pu, &[]);
        e2.fruit_rush_mode = 5;
        let mut cf = ChCollectFruit { amount: 1, ..Default::default() };
        cf.on_event(&ChallengeEvent::Pickup { kind: PickupKind::SeedRushToken, token_kind: 0, car: car.clone() }, false, &e2);
        assert_eq!(cf.count, 0);
        // type filter + "Maximum" + percentage
        let mut cf2 = ChCollectFruit { amount: 100, percentage: true, ..Default::default() };
        cf2.total_tokens = 10;
        cf2.best = 5;
        assert!(!cf2.is_completed());
        cf2.best = 10;
        assert!(cf2.is_completed());
        // ---- Get3Stars: needs 3 stars (env.stars = 3) and finishing
        let mut g = ChGet3Stars { count: 2, ..Default::default() };
        let fin = ChallengeEvent::RaceFinish { car: CarSnap { finished: true, ..Default::default() } };
        g.on_event(&ChallengeEvent::RaceStart, true, &e);
        g.on_event(&fin, true, &e);
        assert!(!g.is_completed());
        g.on_event(&ChallengeEvent::RaceStart, true, &e);
        g.on_event(&fin, true, &e);
        assert!(g.is_completed());
        let mut e1 = env(&pu, &[]);
        e1.stars = 2;
        g.row = true;
        g.on_event(&fin, true, &e1);
        assert_eq!(g.count3, 0, "a non-3-star finish breaks the row");
        // ---- Destroy: hit then destroy
        let mut d = ChDestroy { objects_destroyed: 2, ..Default::default() };
        d.on_event(&ChallengeEvent::RaceStart, false, &e);
        let obj = |id: u32| PhysObj { id, is_opponent: true, is_smackable: false, smackable_name: String::new() };
        for id in [1u32, 2] {
            d.on_event(&ChallengeEvent::Hit { hit: HitInfo { object: Some(obj(id)), ..Default::default() } }, false, &e);
            d.on_event(&ChallengeEvent::Destroy { object: Some(obj(id)), amount: 1.0, drifting_player: false }, false, &e);
        }
        assert!(d.is_completed());
        // the tracked bodies expire after 1 s
        let mut d2 = ChDestroy { objects_destroyed: 1, ..Default::default() };
        d2.on_event(&ChallengeEvent::Hit { hit: HitInfo { object: Some(obj(5)), ..Default::default() } }, false, &e);
        d2.on_event(&ChallengeEvent::Update { dt: 1.1, car: car.clone() }, false, &e);
        d2.on_event(&ChallengeEvent::Destroy { object: Some(obj(5)), amount: 1.0, drifting_player: false }, false, &e);
        assert!(!d2.is_completed());
        // ---- Launch: "LaunchFirst" = start in position 1
        let mut l = m.instantiate("LaunchFirst").unwrap();
        l.on_event(&ChallengeEvent::RaceStart, &e, 0);
        l.on_event(&ChallengeEvent::Launch { car: car.clone(), others: vec![CarSnap { suspended: false, ..Default::default() }] }, &e, 0);
        assert_eq!(l.is_completed(&e), Some(false), "one other car already launched ahead: position 2");
        let mut l2 = m.instantiate("LaunchFirst").unwrap();
        l2.on_event(&ChallengeEvent::RaceStart, &e, 0);
        l2.on_event(&ChallengeEvent::Launch { car: car.clone(), others: vec![CarSnap { suspended: true, ..Default::default() }] }, &e, 0);
        assert_eq!(l2.is_completed(&e), Some(true));
        // ---- Overtake: count places gained
        let mut o = ChOvertake { count: 2, ..Default::default() };
        o.on_event(&ChallengeEvent::RaceStart);
        o.on_event(&ChallengeEvent::Launch { car: car.clone(), others: vec![] });
        let up = |pos: i32| ChallengeEvent::Update { dt: 0.1, car: CarSnap { position: pos, launch_state: 1, ..Default::default() } };
        o.on_event(&up(5)); // first update only latches `started`
        o.on_event(&up(5));
        o.on_event(&up(4));
        o.on_event(&up(3));
        assert!(o.is_completed());
        // ---- Slipstream: needs `Time` seconds of draft > 0.5, continuous resets
        let mut s = ChSlipstream { target: 1.0, continuous: true, ..Default::default() };
        let dr = |draft: f32| ChallengeEvent::Update { dt: 0.5, car: CarSnap { draft, ..Default::default() } };
        s.on_event(&dr(0.9), false, false, 0.9);
        s.on_event(&dr(0.0), false, false, 0.0);
        assert_eq!(s.time, 0.0);
        s.on_event(&dr(0.9), false, false, 0.9);
        s.on_event(&dr(0.9), false, false, 0.9);
        assert!(s.is_completed());
        // ---- PositionInTime: be in position <= 2 within 3 s of being 5th
        let mut p = ChPositionInTime { start_position: 5, end_position: 2, time: 3.0, ..Default::default() };
        p.on_event(&ChallengeEvent::RaceStart, &e);
        p.on_event(&ChallengeEvent::Launch { car: car.clone(), others: vec![] }, &e);
        let pit = |w: i32, pos: i32, dt: f32| ChallengeEvent::Update { dt, car: CarSnap { wheels_on_ground: w, position: pos, ..Default::default() } };
        p.on_event(&pit(4, 8, 0.1), &e); // saw the ground (bit4)
        p.on_event(&pit(4, 5, 0.1), &e); // reached start position
        p.on_event(&pit(4, 4, 1.0), &e);
        p.on_event(&pit(4, 3, 1.0), &e);
        p.on_event(&pit(4, 2, 1.0), &e);
        p.on_event(&pit(4, 2, 1.0), &e);
        assert!(p.is_completed());
    }
}
