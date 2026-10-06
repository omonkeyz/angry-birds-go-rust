//! Bird ability framework: `CBaseAbility` (the base class of every character ability), its factories, the car-side
//! plumbing in `CCar` (`TriggerAbility`, `CanTriggerAbility`, charged fraction, boss ability slots) and the metagame
//! cost logic (`CMetagameManager::CanUseAbility`, `GetBirdAbilityCostInRace`, `GetBirdAbilityCostPreRace`).
//!
//! Standalone: `std` + `glam` + `roxmltree` only. The module never mutates a car; it returns [`CarEffect`] requests
//! (see `modes/types.rs`) that the host applies to the car sim / world / camera / audio.
//!
//! # Extending (phase 2)
//! 1. Create `abilities/<name>.rs` (the stub files already exist and are declared below).
//! 2. `struct XAbility { base: AbilityBase, ..own fields.. }`, `impl Ability for XAbility` - implement only the slots the
//!    original class overrides (the vtable slot offsets are quoted on every trait method). Everything else falls back
//!    to the base-class behaviour through the default methods. When an override has to call the base implementation
//!    (the original does `CBaseAbility::Foo(this)`), call the matching free function `base_foo(self, ...)`.
//! 3. Add a match arm to [`create_ability_by_enum`] (bird ability) or [`create_boss_ability_by_enum`] (boss ability).
//!
//! # Lifecycle (per car, per frame - mirrors `CCar::Update @001a6cd8` / `CCar::Integrate @001abaa8`)
//! * `CCar::Integrate` (every physics step): `ability.on_integrate(kart, out)` for the main and every boss ability.
//! * `CCar::Update` (every frame): `ability.always_update(dt)`; then `if ability.is_active() { ability.update(dt, ..) }`.
//!   [`tick_ability`] does exactly that pair, [`CarAbilitySlot::tick`] does it for all abilities of a car.
//! * Player presses the button: [`CarAbilitySlot::trigger`] (= `CCar::TriggerAbility @0019fa48`).
//!
//! # Per-tick host data
//! The original reads many things off the `CCar` / `CGame`. Those that are not in `KartState` travel in
//! [`AbilityEnv`], which the host refreshes with `ability.base_mut().env = ..` (or [`Ability::set_env`]) each frame.

#![allow(dead_code)]

use crate::modes::types::*;
use glam::Vec3;
use std::collections::HashMap;

#[path = "abilities/speed.rs"]
pub mod speed;
#[path = "abilities/bomb.rs"]
pub mod bomb;
#[path = "abilities/terence.rs"]
pub mod terence;
#[path = "abilities/hal.rs"]
pub mod hal;
#[path = "abilities/kingpig.rs"]
pub mod kingpig;
#[path = "abilities/matilda.rs"]
pub mod matilda;
#[path = "abilities/moustache.rs"]
pub mod moustache;
#[path = "abilities/objectspawn.rs"]
pub mod objectspawn;
#[path = "abilities/stella.rs"]
pub mod stella;
#[path = "abilities/bubbles.rs"]
pub mod bubbles;
#[path = "abilities/minion.rs"]
pub mod minion;
#[path = "abilities/boss.rs"]
pub mod boss;

// =====================================================================================================================
// Constants resolved from the binary / shipped data
// =====================================================================================================================

/// `CDebugManager::GetDebugFloat(0x98)` = `Ability_Holdoff_Time`: the default reuse delay of every ability
/// (`CBaseAbility::CBaseAbility @001ba3f4` writes it to +0x14 and +0x84; `CCar::CCar` seeds `CCar+0x1c4c` with it).
/// Source: `debugtweakables.xml` `<Misc><Ability_Holdoff_Time>5.0` (the tag is stored at 0xde0d58 = float index 0x98 by
/// `CDebugManager::SetDebugTweakablesFromXML @002c7258`) and `CDebugManager::SetDefaults @002c69cc` (also 5.0).
pub const DEBUG_FLOAT_ABILITY_HOLDOFF_TIME: f32 = 5.0;

/// `CGame+0x3298` is floored to this when a player triggers a speed ability (`CRedSpeedAbility::TriggerAbility @001cf744`,
/// `CBlueSpeedAbility::TriggerAbility @001be2d4`, `CSpeedAbility::TriggerAbility @001cf98c`: `max(old, 2.0)`).
/// `CGame::Process` counts it down by dt and the AI rubber-band code reads it (`CCar::...@0018b0b0` tests `> 0`).
pub const GAME_ABILITY_GRACE_TIME: f32 = 2.0;

/// `CCar::GetAbilityChargedFraction @001aac70`: constant `DAT_001aadd8` (0.05, file offset 0x19add8).
pub const CHARGED_FRACTION_ZERO_THRESHOLD: f32 = 0.05;

/// Default `ChargeCount` when the xml has none and the ability is Blue/Moustache/Minion (`LoadAbilityValuesFromXML @001bc09c`
/// loads 3.0 at 0x1bc6a8..0x1bc6ac, else 1.0 = `GetChargesPerPurchase`).
pub const CHARGES_PER_PURCHASE_MULTI: i32 = 3;

/// Default of `AIActivationTime minTime/maxTime` when the node exists but an attribute does not parse (0x41200000).
pub const AI_ACTIVATION_TIME_DEFAULT: f32 = 10.0;

/// Debug booleans read by the ability code, with their SHIPPED values (`GetDebugBool` index -> tweakable):
/// 0x16 `Ability_Activation_Cam_Enabled`=false, 0x58 `Buy_Abilities_In_Race`=true, 0x57 `Buy_Abilities_Pre_Race`=false
/// (all from `debugtweakables.xml`, mapped through `SetDebugTweakablesFromXML @002c7258` store addresses 0xde0688,
/// 0xde0790, 0xde078c). 0x25, 0x29, 0x56, 0x77 are not set by any tweakable and `SetDefaults` leaves them 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugBools {
    /// 0x16 (slow-mo ability camera; `TriggerPlayerAbility` debug branch - not ported, shipped value is false).
    pub ability_cam: bool,
    /// 0x25 (`IsAbilityDebugEnabled`).
    pub b25: bool,
    /// 0x29 (`IsAbilityVFXEnabled`; also lets `CCar::TriggerAbility` ignore `CanTriggerAbility`).
    pub b29: bool,
    /// 0x56 (`CCar::TriggerAbility`: AI cars do not trigger when set).
    pub b56: bool,
    /// 0x57 `Buy_Abilities_Pre_Race`.
    pub buy_pre_race: bool,
    /// 0x58 `Buy_Abilities_In_Race`.
    pub buy_in_race: bool,
    /// 0x77 (`CCar::CanTriggerAbility`, `CGame::IsMultipleAbilitiesEnabled`).
    pub b77: bool,
}

impl Default for DebugBools {
    fn default() -> Self {
        DebugBools { ability_cam: false, b25: false, b29: false, b56: false, buy_pre_race: false, buy_in_race: true, b77: false }
    }
}

impl DebugBools {
    /// `CBaseAbility::CanRetrigger @001bb16c`: `b29 || b25 || b57 || b58`. TRUE in the shipped configuration, so the
    /// "spent" latch of Red / Yellow (`FinishAbility` sets +0x98 only when this is false) is never set; charges alone
    /// limit how often an ability can be used.
    pub fn can_retrigger(&self) -> bool {
        self.b29 || self.b25 || self.buy_pre_race || self.buy_in_race
    }
}

// =====================================================================================================================
// EBirdAbility
// =====================================================================================================================

/// `EBirdAbility` (the enum behind `CBaseAbility::GetBirdAbilityFromString @001ba870` + its `.part.18` clone @000a4114,
/// `CreateAbility @001ba9e8`, `CreateBossAbility @001bb92c`). The value is `GetId()` (vtable slot 0x1c) of the class.
/// 0 in the original means "none" (represented by `Option::None`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BirdAbility {
    /// "SpeedBoost" - `CSpeedAbility` (Yellow/Senna/SennaHelmet).
    SpeedBoost = 1,
    /// "Bomb" - `CBombAbility`.
    Bomb = 2,
    /// "MinionDefence" - `CMinionDefenceAbility`.
    MinionDefence = 3,
    /// "StellaDefence" - `CStellaDefenceAbility`.
    StellaDefence = 4,
    /// "StellaBossAbility" - `CStellaBossAbility` (boss factory only).
    StellaBossAbility = 5,
    /// "ObjectSpawn" - `CObjectSpawnAbility` (boss factory only).
    ObjectSpawn = 6,
    /// "RedSpeedBoost" - `CRedSpeedAbility`.
    RedSpeedBoost = 7,
    /// "BlueSpeedBoost" - `CBlueSpeedAbility`.
    BlueSpeedBoost = 8,
    /// "TerenceRage" - `CTerenceRageAbility`.
    TerenceRage = 9,
    /// "OvertakeSpeedBoost" - `COvertakeSpeedAbility` (boss factory only).
    OvertakeSpeedBoost = 10,
    /// "KingPigAbility" - `CKingPigAbility`.
    KingPig = 11,
    /// "BubblesInflateAbility" - `CBubblesInflateAbility`.
    BubblesInflate = 12,
    /// "BlueBossAbility" - `CBlueBossAbility` (boss factory only).
    BlueBoss = 13,
    /// "MoustacheAbility" - `CMoustacheAbility`.
    Moustache = 14,
    /// "HalAbility" - `CHalAbility`.
    Hal = 15,
    /// "MatildaAbility" - `CMatildaAbility`.
    Matilda = 16,
    /// "BubblesBossAbility" - `CBubblesBossAbility` (boss factory only).
    BubblesBoss = 17,
    /// "MoustacheBossAbility" - `CMoustacheBossAbility` (boss factory only).
    MoustacheBoss = 18,
    /// "HalBossAbility" - `CHalBossAbility` (boss factory only).
    HalBoss = 19,
    /// "ChuckBossAbility" - `CChuckBossAbility` (both factories).
    ChuckBoss = 20,
    /// "KingPigBossAbility" - `CKingPigBossAbility` (both factories).
    KingPigBoss = 21,
    /// "MatildaBossAbility" - `CMatildaBossAbility` (both factories).
    MatildaBoss = 22,
}

impl BirdAbility {
    /// Enum value as stored by the original (`GetId()` result).
    pub fn id(self) -> i32 {
        self as i32
    }

    /// Reverse of [`BirdAbility::id`]; 0 / unknown -> `None`.
    pub fn from_id(id: i32) -> Option<BirdAbility> {
        use BirdAbility::*;
        Some(match id {
            1 => SpeedBoost,
            2 => Bomb,
            3 => MinionDefence,
            4 => StellaDefence,
            5 => StellaBossAbility,
            6 => ObjectSpawn,
            7 => RedSpeedBoost,
            8 => BlueSpeedBoost,
            9 => TerenceRage,
            10 => OvertakeSpeedBoost,
            11 => KingPig,
            12 => BubblesInflate,
            13 => BlueBoss,
            14 => Moustache,
            15 => Hal,
            16 => Matilda,
            17 => BubblesBoss,
            18 => MoustacheBoss,
            19 => HalBoss,
            20 => ChuckBoss,
            21 => KingPigBoss,
            22 => MatildaBoss,
            _ => return None,
        })
    }

    /// `CBaseAbility::GetBirdAbilityFromString(char const*) @001ba870` (+ `.part.18` clone @000a4114 for the names from
    /// "KingPigAbility" on). Also the string -> enum step of `GetBossAbilityType @001bb664` (the `type` attribute).
    /// Returns `None` where the original returns 0 (unknown name).
    pub fn from_name(name: &str) -> Option<BirdAbility> {
        use BirdAbility::*;
        Some(match name {
            "Bomb" => Bomb,
            "SpeedBoost" => SpeedBoost,
            "MinionDefence" => MinionDefence,
            "StellaDefence" => StellaDefence,
            "StellaBossAbility" => StellaBossAbility,
            "RedSpeedBoost" => RedSpeedBoost,
            "BlueSpeedBoost" => BlueSpeedBoost,
            "TerenceRage" => TerenceRage,
            "ObjectSpawn" => ObjectSpawn,
            "OvertakeSpeedBoost" => OvertakeSpeedBoost,
            // .part.18 clone @000a4114
            "KingPigAbility" => KingPig,
            "KingPigBossAbility" => KingPigBoss,
            "BubblesInflateAbility" => BubblesInflate,
            "BlueBossAbility" => BlueBoss,
            "BubblesBossAbility" => BubblesBoss,
            "MoustacheAbility" => Moustache,
            "MoustacheBossAbility" => MoustacheBoss,
            "HalAbility" => Hal,
            "HalBossAbility" => HalBoss,
            "MatildaAbility" => Matilda,
            "MatildaBossAbility" => MatildaBoss,
            "ChuckBossAbility" => ChuckBoss,
            _ => return None,
        })
    }

    /// `CBaseAbility::GetChargesPerPurchase(EBirdAbility) @001baf14`: 3 for Blue (8), Moustache (0xe) and Minion (3),
    /// else 1.
    pub fn charges_per_purchase(self) -> i32 {
        match self {
            BirdAbility::BlueSpeedBoost | BirdAbility::Moustache | BirdAbility::MinionDefence => CHARGES_PER_PURCHASE_MULTI,
            _ => 1,
        }
    }

    /// `CBaseAbility::ShouldGhostUseAI(EBirdAbility) @001badf8`: true for Bomb(2), Minion(3), Stella(4), Terence(9),
    /// Moustache(0xe), Hal(0xf), Matilda(0x10); false for everything else.
    pub fn should_ghost_use_ai(self) -> bool {
        use BirdAbility::*;
        matches!(self, Bomb | MinionDefence | StellaDefence | TerenceRage | Moustache | Hal | Matilda)
    }
}

// =====================================================================================================================
// Parameters parsed from the character / boss xml `<Ability>` node
// =====================================================================================================================

/// One `<Name><MinLevel>a</MinLevel><MaxLevel>b</MaxLevel></Name>` child; each side is `None` when the element is absent
/// or does not parse (the original then uses the call-site default, see [`AbilityParams::float_for_level`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RawLevel {
    pub min: Option<f32>,
    pub max: Option<f32>,
}

/// Name of the xml effect tags of a character (`CCharacterManager::GetCharacterInfo(id)+0x10c / +0x18c / +0x20c`):
/// `<AbilityEffectName>`, `<AbilityEndEffectName>`, `<AbilityDustEffectName>`. Empty string = none.
/// The +0x10c/+0x18c/+0x20c <-> tag mapping is inferred from the string order of the character loader and from the xml
/// (`CharacterAbilityEndEffect` is the spawn name used with the +0x18c effect in `StopEffects @001b9b90`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AbilityEffectNames {
    pub start: String,
    pub end: String,
    pub dust: String,
}

/// Decoded `<Ability name=".." type="..">` node (character `char_NNN.xml` or one `<Ability>` of `boss_NNN.xml`).
#[derive(Clone, Debug, Default)]
pub struct AbilityParams {
    /// `name` attribute (character xml: the class selector, e.g. "RedSpeedBoost"; boss xml: a label like "Close").
    pub name: String,
    /// `type` attribute (boss xml only, e.g. "ObjectSpawn" - `GetBossAbilityType @001bb664`).
    pub ty: String,
    levels: HashMap<String, RawLevel>,
    texts: HashMap<String, String>,
    attrs: HashMap<String, String>,
    /// `<AIActivationTime minTime= maxTime=/>` present: `(min, max)` with unparsable attributes = 10.0.
    pub ai_activation: Option<(f32, f32)>,
    /// `<ReuseDelays>` present.
    pub has_reuse_delays: bool,
    /// `<ReuseDelays><Default value=/>` parsed value (None = absent / unparsable: the original keeps +0x14).
    pub reuse_default: Option<f32>,
    /// `<Delay number= value=/>` entries (entries whose `number` does not parse are skipped, a bad `value` is 0.0).
    pub reuse_delays: Vec<(i32, f32)>,
    /// Effect names of the surrounding `<Character>` (only set by [`AbilityParams::from_character_xml`]).
    pub effects: AbilityEffectNames,
}

fn parse_f32(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok()
}

fn elem_children<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element())
}

fn child_named<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    elem_children(n).find(|c| c.tag_name().name() == name)
}

impl AbilityParams {
    /// Decode an `<Ability>` element (the node `CBaseAbility::LoadAbilityValuesFromXML @001bc09c` receives).
    pub fn from_node(node: roxmltree::Node) -> AbilityParams {
        let mut p = AbilityParams::default();
        p.name = node.attribute("name").unwrap_or("").to_string();
        p.ty = node.attribute("type").unwrap_or("").to_string();
        for a in node.attributes() {
            p.attrs.insert(a.name().to_string(), a.value().to_string());
        }
        for c in elem_children(node) {
            let tag = c.tag_name().name().to_string();
            match tag.as_str() {
                "AIActivationTime" => {
                    let min = c.attribute("minTime").and_then(parse_f32).unwrap_or(AI_ACTIVATION_TIME_DEFAULT);
                    let max = c.attribute("maxTime").and_then(parse_f32).unwrap_or(AI_ACTIVATION_TIME_DEFAULT);
                    p.ai_activation = Some((min, max));
                }
                "ReuseDelays" => {
                    p.has_reuse_delays = true;
                    if let Some(d) = child_named(c, "Default") {
                        p.reuse_default = d.attribute("value").and_then(parse_f32);
                    }
                    for d in elem_children(c).filter(|d| d.tag_name().name() == "Delay") {
                        if let Some(num) = d.attribute("number").and_then(|s| s.trim().parse::<i32>().ok()) {
                            let v = d.attribute("value").and_then(parse_f32).unwrap_or(0.0);
                            p.reuse_delays.push((num, v));
                        }
                    }
                }
                _ => {
                    let min_n = child_named(c, "MinLevel");
                    let max_n = child_named(c, "MaxLevel");
                    if min_n.is_some() || max_n.is_some() {
                        let rd = |n: Option<roxmltree::Node>| n.and_then(|n| n.text()).and_then(parse_f32);
                        p.levels.insert(tag, RawLevel { min: rd(min_n), max: rd(max_n) });
                    } else {
                        p.texts.insert(tag, c.text().unwrap_or("").trim().to_string());
                    }
                }
            }
        }
        p
    }

    /// Parse a whole character xml (`CHARSPEC:Char_%03d.xml`, `CBaseAbility::Init @001bbd9c`): the first `<Ability>`
    /// child of the root, plus the character's effect names. `None` when there is no ability node (e.g. `char_015.xml`).
    pub fn from_character_xml(xml: &str) -> Option<AbilityParams> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let root = doc.root_element();
        let node = child_named(root, "Ability")?;
        let mut p = AbilityParams::from_node(node);
        let txt = |n: &str| child_named(root, n).and_then(|c| c.text()).unwrap_or("").trim().to_string();
        p.effects = AbilityEffectNames { start: txt("AbilityEffectName"), end: txt("AbilityEndEffectName"), dust: txt("AbilityDustEffectName") };
        Some(p)
    }

    /// Parse the `index`-th `<Ability>` of a boss xml (`CBaseAbility::InitBoss @001bbee0`; `GetBossAbilityCount @001bb57c`
    /// counts the `Ability` elements).
    pub fn from_boss_xml(xml: &str, index: usize) -> Option<AbilityParams> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let node = elem_children(doc.root_element()).filter(|c| c.tag_name().name() == "Ability").nth(index)?;
        Some(AbilityParams::from_node(node))
    }

    /// Number of `<Ability>` elements (`CBaseAbility::GetBossAbilityCount @001bb57c`).
    pub fn boss_ability_count(xml: &str) -> usize {
        match roxmltree::Document::parse(xml) {
            Ok(doc) => elem_children(doc.root_element()).filter(|c| c.tag_name().name() == "Ability").count(),
            Err(_) => 0,
        }
    }

    /// The raw min/max pair of a leveled child (None when the element has no MinLevel/MaxLevel).
    pub fn raw_level(&self, key: &str) -> Option<RawLevel> {
        self.levels.get(key).copied()
    }

    /// The shared-type view of a leveled child: missing sides are 0 (the integer loaders' default).
    pub fn level_value(&self, key: &str) -> Option<LevelValue> {
        self.levels.get(key).map(|r| LevelValue { min: r.min.unwrap_or(0.0), max: r.max.unwrap_or(0.0) })
    }

    /// `CBaseAbility::GetAbilityFloatForLevel @001bbc18` (and the inlined copies in `LoadAbilityValuesFromXML`):
    /// `min + (max - min) * clamp(level, 0, 1)` where `MinLevel` / `MaxLevel` default to `default` when missing
    /// (the call sites pass 0.0, except `EffectDuration` which passes the already loaded `Duration`) and `level` is the
    /// car's ability level fraction `CCar+0x1a60` (negative -> 0.0, > 1 -> 1.0; `DAT_001bbcc8` = 0.0).
    pub fn float_for_level(&self, key: &str, default: f32, level: f32) -> f32 {
        let r = self.levels.get(key).copied().unwrap_or_default();
        let min = r.min.unwrap_or(default);
        let max = r.max.unwrap_or(default);
        min + (max - min) * clamp_level(level)
    }

    /// `CBaseAbility::GetAbilityIntForLevel @001bbcd4`: `(int)ceilf(min + (max - min) * clamp(level, 0, 1))` with
    /// `CXmlUtil::GetInteger` for both sides (missing / non-integer text = 0).
    pub fn int_for_level(&self, key: &str, level: f32) -> i32 {
        let r = self.levels.get(key).copied().unwrap_or_default();
        let to_i = |v: Option<f32>| v.map(|f| f.trunc()).unwrap_or(0.0);
        let (min, max) = (to_i(r.min), to_i(r.max));
        (min + (max - min) * clamp_level(level)).ceil() as i32
    }

    /// `CXmlUtil::GetFloat/GetFloatOrDefault(node, key[, default])`: child element text as a float.
    pub fn scalar_f32(&self, key: &str, default: f32) -> f32 {
        self.texts.get(key).and_then(|s| parse_f32(s)).unwrap_or(default)
    }

    /// Child element text (e.g. boss `<ObjectType>`).
    pub fn scalar_str(&self, key: &str) -> Option<&str> {
        self.texts.get(key).map(|s| s.as_str())
    }

    /// Attribute of the `<Ability>` element itself (`name`, `type`, ...).
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }
}

/// Level fraction clamp used by every `*ForLevel` loader: `< 0 -> 0.0` (`DAT_001bbcc8` = 0.0), `> 1 -> 1.0`.
pub fn clamp_level(level: f32) -> f32 {
    if level < 0.0 {
        0.0
    } else if level > 1.0 {
        1.0
    } else {
        level
    }
}

// =====================================================================================================================
// Host-supplied context
// =====================================================================================================================

/// Things the original reads off `CCar` / `CGame` that are not in [`KartState`]. The host refreshes this each frame
/// (`ability.set_env(..)`).
#[derive(Clone, Debug)]
pub struct AbilityEnv {
    /// `CXGSRigidBody+0x98` = the fixed physics step. Ability forces are `rb+0x98 * strength` along body +Z
    /// (an impulse in carsim's convention). UNRESOLVED (same as `CarSim::base_dt`): 1/60 is the `CWheel::CWheel` default.
    pub physics_dt: f32,
    /// `CPlayer::IsLocalPlayer(*(CCar+0x1af0))`.
    pub is_local_player: bool,
    /// `CCar::IsPilotDetached @?` (the bird is out of the kart): abilities cannot trigger.
    pub pilot_detached: bool,
    /// `*(*(CCar+0x1af4)+0x1a8) != 0`: a per-driver flag that lets non-player drivers use the ability
    /// (`CBaseAbility::CanTriggerAbility @001b9d14`). UNRESOLVED: the object at `CCar+0x1af4` was not identified.
    pub driver_ability_flag: bool,
    /// `CGame::GetCurrentSlowMoTimeMultiplier @0011ace0` (1.0 = no slow-mo).
    pub slowmo_multiplier: f32,
    /// `*(*(CCar+0x548)+0xe1c)`: number of attached bodywork part bodies whose time scale is also changed by `CSpeedAbility`.
    pub part_bodies: u32,
    /// `(*(CCar+0x1af4)+0x12c, +0x130)`: AI speed factors that `COvertakeSpeedAbility` overrides.
    pub ai_boost_factors: (f32, f32),
    /// Shipped debug booleans ([`DebugBools::default`]).
    pub debug: DebugBools,
    /// Effect names of the car's character.
    pub effects: AbilityEffectNames,
    // ---- world data for phase 2 (host refreshes every frame; see `abilityrun.rs`) ----
    /// Every OTHER car of the race (`CGame` car list `+0x3144..`, count `+0x31b8`, minus this car).
    pub others: Vec<OtherCar>,
    /// `CGame+0x32b0 == 3` (team game mode): cars of the same team are spared by area abilities.
    pub team_mode: bool,
    /// This car's team id (`CCar::GetTeamID`; every car is its own team in a normal race = its id).
    pub team: i32,
    /// Seconds since the race started (host race clock).
    pub race_time: f32,
    /// Ground below this car: (y, normal), `None` when there is none.
    pub ground_below: Option<(f32, Vec3)>,
    /// Host random source state (xorshift32); abilities draw with [`AbilityEnv::rand01`]. The host reseeds it per car.
    pub rng: u32,
    /// `CCar+0x1af0 != 0` seen from the game (a player owns the car) - same as `KartState::is_player`.
    /// Number of players in the race (`CGame+0x31c4`).
    pub num_players: i32,
    /// Racing line of this car: the spline direction under the car (unit), used by boss weapons that aim along the track.
    pub track_dir: Option<Vec3>,
    /// `CCar+0x4ac` penalty / `+0x1a8c` race-progress value some AI-chance functions compare (host supplies 0 / position-ish).
    pub progress_value: f32,
    /// (added by the KingPig port) `CSpline::GetPosition(CSpline::Lookahead(spline, d, .., car+0x1a8c))` and
    /// `CSpline::GetUpVectorInterpolated` at that point: the racing-line point `d` metres ahead of this car, where `d` is
    /// `Ability::spline_lookahead_request()` of the car's ability. `None` = the host has no spline.
    pub spline_ahead: Option<(Vec3, Vec3)>,
    /// (added by the Hal port) the car's race spline (`CCar::GetSpline`), the car's fractional spline index (`CCar+0x1a8c`) and its
    /// signed lateral offset from the spline in metres (`CCar+0x1a98`): `CHalAbility` moves its boomerang along this spline.
    pub car_spline: Option<std::sync::Arc<crate::items::spline::CSpline>>,
    pub car_spline_index: f32,
    pub car_spline_lateral: f32,
}

/// Another car as an ability sees it. Offsets quoted from the loops in `CBombAbility::FinishAbility @001be6fc` etc.
#[derive(Clone, Debug)]
pub struct OtherCar {
    pub id: CarId,
    pub pos: Vec3,
    pub vel: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub team: i32,
    pub is_player: bool,
    /// `CCar+0x42c <= 0.0` in the area loops: the car may be hit (the "non-collide / invulnerable" timer has run out).
    pub hittable: bool,
    /// `CCar+0x1a8c` race value (progress), `CCar+0x4ac` penalty timer.
    pub progress_value: f32,
    pub penalty: f32,
    /// Distance along the race spline (m).
    pub spline_distance: f32,
    pub race_position: u32,
    /// The other car's own ability currently grants immunity (shields, rage): `ImmuneToSpin / Damage / Explosions`.
    pub immune_spin: bool,
    pub immune_damage: bool,
    pub immune_explosions: bool,
}

impl AbilityEnv {
    /// Uniform float in [0,1) from the host rng state (xorshift32, advances the state).
    pub fn rand01(&mut self) -> f32 {
        let mut x = if self.rng == 0 { 0x9e3779b9 } else { self.rng };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Requests an ability makes of the world (smackable manager, shields, models). The host (`abilityrun.rs`) executes them in
/// order each frame after `tick`/`integrate`. The ability picks the `handle` (use [`AbilityBase::alloc_handle`]); the host maps it to
/// the object it creates and reports back through [`WorldEvent`]. Add variants when a class needs something else and document them
/// (original function + exact semantics) - the host `abilityrun::apply_request` must then handle them.
#[derive(Clone, Debug)]
pub enum WorldRequest {
    /// `CSmackableManager::AddSmackable(type, matrix, 0, 0)`: a loose smackable body. `forward/up` define its frame (right = up x forward is
    /// the item-frame convention of `items`: X = normal x tangent, Y = up, Z = forward). `scale` != 1 scales the model (boss spawn `InitialScale`
    /// grows to 1). `gravity=false` = no gravity (`CXGSRigidBody::SetGravity`). `ignore_owner_explode` = `CCar::SetupToIgnoreExplodeForce`.
    /// `explode_power` overrides the smackable's explosion strength (`smackable+0x1168`) when > 0. `sleep`: body asleep until `UpdateSmackable`.
    SpawnSmackable {
        handle: u32,
        type_id: u32,
        pos: Vec3,
        forward: Vec3,
        up: Vec3,
        vel: Vec3,
        ang_vel: Vec3,
        scale: f32,
        owner: CarId,
        gravity: bool,
        ignore_owner_explode: bool,
        explode_power: f32,
        sleep: bool,
    },
    /// Change an existing smackable (None = keep). `pos`/`vel` replace the body's; `forward/up` re-orient; `gravity` toggles; `sleep` false wakes it.
    UpdateSmackable { handle: u32, pos: Option<Vec3>, vel: Option<Vec3>, ang_vel: Option<Vec3>, forward_up: Option<(Vec3, Vec3)>, scale: Option<f32>, gravity: Option<bool>, sleep: Option<bool> },
    /// Remove the smackable; `shatter` = `CSmackable::Shatter` (breaks into fragments + effect) else silent removal (`RemoveRigidBody`).
    RemoveSmackable { handle: u32, shatter: bool },
    /// A spherical shield collision object (`CMinionDefenceShield`, `CStellaDefenceShield`, `CBubblesBall`): follows `pos`, blocks / bounces
    /// other cars' bodies and projectiles while `enabled`; `model` is the visual (xgm under `pak/`, empty = none), drawn at `pos` with `radius`.
    SetShield { handle: u32, owner: CarId, pos: Vec3, radius: f32, enabled: bool, model: String },
    /// Remove a shield object.
    RemoveShield { handle: u32 },
    /// A purely visual model instance (`CXGSAssetManager::LoadModel` + manual render in `OnCarRender`): pos + forward/up + uniform scale. Host draws it until removed.
    SetVisual { handle: u32, model: String, pos: Vec3, forward: Vec3, up: Vec3, scale: f32 },
    RemoveVisual { handle: u32 },
}

/// What the world tells an ability back (the callbacks the original registers: `ObjectCollisionCallback`,
/// `RegisterCallbackOnSmashed` callbacks, `ShieldCollision`).
#[derive(Clone, Debug)]
pub enum WorldEvent {
    /// `ObjectCollisionCallback(CSmackable*, ctx)`: smackable `handle` collided with car `car` (None = world / another body) at `pos`; `vel` = the
    /// smackable's velocity, `impulse` = the hit magnitude.
    SmackableHit { handle: u32, car: Option<CarId>, pos: Vec3, vel: Vec3, impulse: f32 },
    /// The smackable was smashed (`RegisterCallbackOnSmashed` callback): `pos` = where.
    SmackableSmashed { handle: u32, pos: Vec3 },
    /// The smackable touched the ground (first contact after being airborne).
    SmackableGround { handle: u32, pos: Vec3, normal: Vec3, speed: f32 },
    /// `ShieldCollision`: a car / object of `other` pushed into the shield `handle` with `impulse`.
    ShieldHit { handle: u32, other: Option<CarId>, pos: Vec3, impulse: f32 },
}

impl Default for AbilityEnv {
    fn default() -> Self {
        AbilityEnv {
            physics_dt: 1.0 / 60.0,
            is_local_player: false,
            pilot_detached: false,
            driver_ability_flag: false,
            slowmo_multiplier: 1.0,
            part_bodies: 0,
            ai_boost_factors: (1.0, 1.0),
            debug: DebugBools::default(),
            effects: AbilityEffectNames::default(),
            others: Vec::new(),
            team_mode: false,
            team: 0,
            race_time: 0.0,
            ground_below: None,
            rng: 0x1234_5678,
            num_players: 1,
            track_dir: None,
            progress_value: 0.0,
            spline_ahead: None,
            car_spline: None,
            car_spline_index: 0.0,
            car_spline_lateral: 0.0,
        }
    }
}

// =====================================================================================================================
// CBaseAbility data
// =====================================================================================================================

/// The data members of `CBaseAbility` (`CBaseAbility::CBaseAbility(CCar*) @001ba3f4`, size 0x98, derived classes start
/// at 0x98). Offsets in the comments; the meaning of every field was resolved from `LoadAbilityValuesFromXML`'s xml tag
/// names and from its users.
#[derive(Clone, Debug)]
pub struct AbilityBase {
    /// +0x04 time remaining of the ability (set to `GetDuration()` on trigger; 0 = "unset", -1 after finish).
    pub time_remaining: f32,
    /// +0x0c ability running.
    pub active: bool,
    /// +0x10 `<Duration>` (level-interpolated; `GetDuration()` returns 1.0 when it is <= 0).
    pub duration: f32,
    /// +0x14 default reuse delay (`<ReuseDelays><Default>`, else `Ability_Holdoff_Time`).
    pub reuse_delay_default: f32,
    /// +0x18 `<BoostStrength>`: force (N) of the generic boost applied in `OnCarIntegrate` while `boost_time > 0`.
    pub boost_strength: f32,
    /// +0x1c `<BoostDuration>`: seconds the generic boost lasts after trigger.
    pub boost_duration: f32,
    /// +0x20 boost seconds left (decremented in `OnCarAlwaysUpdate`; survives `FinishAbility` inside `OnCarUpdate`,
    /// which saves/restores it around the finish call).
    pub boost_time: f32,
    /// +0x24 / +0x28 particle effect definition indices (-1 = none) - not modelled (the host spawns by name).
    /// +0x2c start-effect instance alive (`!= -1`).
    pub fx_alive: bool,
    /// +0x30 end-effect instance alive (`!= -1`).
    pub end_fx_alive: bool,
    /// +0x34 remaining charges (`<ChargeCount>`; the ctor default is 1).
    pub charges: i32,
    /// +0x38 times triggered this race (player path only: `TriggerPlayerAbility` increments it).
    pub uses: i32,
    /// +0x3c visual-effect timer (set from `effect_duration` on trigger, counts down in `OnCarUpdate`).
    pub effect_timer: f32,
    /// +0x40 `<EffectDuration>` (defaults to `Duration`).
    pub effect_duration: f32,
    /// +0x44 the ability has been triggered at least once (`IsActive` requires it).
    pub triggered: bool,
    /// +0x5c `<CamBehindMod>` (extra camera-behind distance at the start of the ability; ctor -1.0 = "none").
    pub cam_behind_mod: f32,
    /// +0x60 boss ability (created through `CreateBossAbility`; the ctor of `COvertakeSpeedAbility` etc. sets 1).
    pub is_boss: bool,
    /// +0x64 play the pilot animation (ctor 1).
    pub play_anim: bool,
    /// +0x68 / +0x6c / +0x70 immunity flags consulted by `ImmuneToSpin / ImmuneToDamage / ImmuneToExplosions` while active
    /// (set by the derived classes that grant them).
    pub immune_spin: bool,
    pub immune_damage: bool,
    pub immune_explosions: bool,
    /// +0x74 dust particle instance alive; +0x78 dust enabled (`EnableDustEffect`).
    pub dust_fx_alive: bool,
    pub dust_on: bool,
    /// +0x7c / +0x80 `<AIActivationTime minTime maxTime>` (0 until the node is read).
    pub ai_min_time: f32,
    pub ai_max_time: f32,
    /// +0x84 reuse delay in force now (`GetReuseDelay()`): the `<Delay number=uses+1>` entry, else `reuse_delay_default`.
    pub reuse_delay: f32,
    /// +0x88/+0x8c `<ReuseDelays><Delay number= value=>` table.
    pub reuse_table: Vec<(i32, f32)>,
    /// Ability level fraction (`CCar+0x1a60` = `CModSpec+0xc`), clamped when used. UNRESOLVED producer: the
    /// `CModSpec` ctor zeroes it and no setter was found in the decompile, so the host must supply the value.
    pub level: f32,
    /// Host data.
    pub env: AbilityEnv,
    /// World requests queued by the ability since the host last drained them ([`Ability::drain_requests`]).
    pub requests: Vec<WorldRequest>,
    /// Handle counter for [`AbilityBase::alloc_handle`].
    pub next_handle: u32,
}

impl AbilityBase {
    /// `CBaseAbility::CBaseAbility(CCar*) @001ba3f4` field defaults (`level` = `CCar+0x1a60`).
    pub fn new(level: f32) -> AbilityBase {
        AbilityBase {
            time_remaining: 0.0,
            active: false,
            duration: 0.0,
            reuse_delay_default: DEBUG_FLOAT_ABILITY_HOLDOFF_TIME,
            boost_strength: 0.0,
            boost_duration: 0.0,
            boost_time: 0.0,
            fx_alive: false,
            end_fx_alive: false,
            charges: 1,
            uses: 0,
            effect_timer: 0.0,
            effect_duration: 0.0,
            triggered: false,
            cam_behind_mod: -1.0,
            is_boss: false,
            play_anim: true,
            immune_spin: false,
            immune_damage: false,
            immune_explosions: false,
            dust_fx_alive: false,
            dust_on: false,
            ai_min_time: 0.0,
            ai_max_time: 0.0,
            reuse_delay: DEBUG_FLOAT_ABILITY_HOLDOFF_TIME,
            reuse_table: Vec::new(),
            level,
            env: AbilityEnv::default(),
            requests: Vec::new(),
            next_handle: 1,
        }
    }

    /// `CBaseAbility::IsActive @001bb24c`: `triggered && (active || effect_timer > 0)`.
    pub fn is_active(&self) -> bool {
        self.triggered && (self.active || self.effect_timer > 0.0)
    }

    /// `CBaseAbility::GetDuration @00196b78` (default slot 0x88).
    pub fn default_duration(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            self.duration
        }
    }

    /// A process-unique object handle for [`WorldRequest`]s (global atomic counter; `car` is ignored, kept for clarity at call sites).
    pub fn alloc_handle(&mut self, _car: CarId) -> u32 {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Queue a world request.
    pub fn request(&mut self, r: WorldRequest) {
        self.requests.push(r);
    }

    /// `CBaseAbility::UpdateReuseDelay @001bb3e0`: the table entry whose number is `uses + 1`, else the default.
    pub fn update_reuse_delay(&mut self) {
        let want = self.uses + 1;
        self.reuse_delay = self.reuse_table.iter().find(|(n, _)| *n == want).map(|(_, v)| *v).unwrap_or(self.reuse_delay_default);
    }
}

// =====================================================================================================================
// The vtable
// =====================================================================================================================

/// `CBaseAbility` as a trait. Slot offsets are the original vtable offsets (`CBaseAbility` vtable @00d91f98+8).
/// Every method has the base-class behaviour as default, so a derived class implements only what it overrides.
/// Effects/sounds that the original triggers inline are returned as `CarEffect`s (`Other{tag}` where `types.rs` has no
/// dedicated variant; the tag is the original function name).
pub trait Ability {
    fn base(&self) -> &AbilityBase;
    fn base_mut(&mut self) -> &mut AbilityBase;

    /// slot 0x1c `GetId()`: the `EBirdAbility` value.
    fn id(&self) -> BirdAbility;
    /// slot 0x18 `GetAbilityName()`.
    fn name(&self) -> &str;

    /// Host refresh of the per-frame data.
    fn set_env(&mut self, env: AbilityEnv) {
        self.base_mut().env = env;
    }

    /// slot 0x14 `LoadAbilityValuesFromXML(node)`. Derived classes read their own tags, then call [`base_load_values`].
    fn load_values(&mut self, p: &AbilityParams) {
        base_load_values(self, p)
    }

    /// slot 0x20 `TriggerPlayerAbility @001b953c`.
    fn trigger_player(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_trigger_player(self, kart, out)
    }
    /// slot 0x24 `TriggerAbility @001b96d8`.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_trigger(self, kart, out)
    }
    /// slot 0x28 `TriggerAbilityEffects @001b9a44`.
    fn trigger_effects(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_trigger_effects(self, kart, out)
    }
    /// slot 0x2c `StopEffects @001b9b90`.
    fn stop_effects(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_stop_effects(self, kart, out)
    }
    /// slot 0x3c `OnCarIntegrate @001b9de4` (every physics step).
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_on_integrate(self, kart, out)
    }
    /// slot 0x40 `OnCarAlwaysUpdate @001b9398` (every frame, active or not).
    fn always_update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_always_update(self, dt, kart, out)
    }
    /// slot 0x44 `OnCarUpdate @001b987c` (every frame while `is_active()`).
    fn update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_update(self, dt, kart, out)
    }
    /// slot 0x4c `OnCarCollision`: base returns 1.0 (`0x3f800000`).
    fn on_collision(&mut self) -> f32 {
        1.0
    }
    /// (added by the KingPig port) `OnCarCollision(.., CXGSRigidBody* other, ..)` with the data the original's overrides read:
    /// `hit_world` = the collision partner pointer is null (static world). Default: [`Ability::on_collision`].
    fn on_collision_ex(&mut self, _hit_world: bool, _kart: &KartState, _out: &mut Vec<CarEffect>) -> f32 {
        self.on_collision()
    }
    /// slot 0x50 `OnCarImpactDamage(pos, damage)`: base does nothing (the caller keeps `damage`).
    fn on_impact_damage(&mut self, damage: f32) -> f32 {
        damage
    }
    /// slot 0x54 `OnCarSetSteering(steer)`: base leaves the steering unchanged (called from `CCar::SetSteering`
    /// only while the ability is active).
    fn on_set_steering(&self, steer: f32) -> f32 {
        steer
    }
    /// slot 0x58 `OnCarCamBehindMod @001b996c`: `input` is the camera-behind value computed by `CCar::GetCamBehindMod`.
    fn on_cam_behind_mod(&self, input: f32) -> f32 {
        base_cam_behind_mod(self.base(), self.duration(), input)
    }
    /// slot 0x5c `OnPlayerAbilityPurchase @00196e70`: charges = `GetChargesPerPurchase(GetId())`.
    fn on_player_purchase(&mut self) {
        let c = self.id().charges_per_purchase();
        self.base_mut().charges = c;
    }
    /// slot 0x6c `GetReuseDelay @001b91f4`.
    fn reuse_delay(&self) -> f32 {
        self.base().reuse_delay
    }
    /// slot 0x70 `CanTriggerAbility @001b9d14`.
    fn can_trigger(&self, kart: &KartState) -> bool {
        base_can_trigger(self.base(), kart)
    }
    /// slot 0x74 `GetAbilityCharges @001870a4`.
    fn charges(&self) -> i32 {
        self.base().charges
    }
    /// slot 0x78 `GetUsesThisRace @00196b70`.
    fn uses_this_race(&self) -> i32 {
        self.base().uses
    }
    /// slot 0x7c `CanTriggerAtDistance(a, b)`: base true (AI trigger gating of the boss abilities).
    fn can_trigger_at_distance(&mut self, _a: f32, _b: f32) -> bool {
        true
    }
    /// slot 0x80 `GetCooldown`: base 0.
    fn cooldown(&self) -> f32 {
        0.0
    }
    /// slot 0x84 `CalcCurrentAITriggerChance`: base 0.
    fn ai_trigger_chance(&self) -> f32 {
        0.0
    }
    /// slot 0x88 `GetDuration @00196b78`.
    fn duration(&self) -> f32 {
        self.base().default_duration()
    }
    /// slot 0x8c `ImmuneToSpin @00196b94`: `active && immune_spin`.
    fn immune_to_spin(&self) -> bool {
        self.base().active && self.base().immune_spin
    }
    /// slot 0x90 `ImmuneToDamage @00196bb4`.
    fn immune_to_damage(&self) -> bool {
        self.base().active && self.base().immune_damage
    }
    /// slot 0x94 `ImmuneToExplosions @001b917c`.
    fn immune_to_explosions(&self) -> bool {
        self.base().active && self.base().immune_explosions
    }
    /// slot 0x98 `FinishAbility @001b945c`.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_finish(self, kart, out)
    }
    /// slot 0x9c `CanTriggerEffects @001b919c`: base true.
    fn can_trigger_effects(&self) -> bool {
        true
    }
    /// (added by the KingPig port) Host: when `Some(d)`, fill `env.spline_ahead` with the racing-line point `d` metres ahead
    /// of the car before calling `on_integrate` (`CSpline::Lookahead` input). Default: no spline needed.
    fn spline_lookahead_request(&self) -> Option<f32> {
        None
    }
    /// Host: take the queued world requests (spawn / move / remove smackables, shields, visuals).
    fn drain_requests(&mut self) -> Vec<WorldRequest> {
        std::mem::take(&mut self.base_mut().requests)
    }
    /// Host: a world callback for an object / shield this ability created (the original's registered callbacks).
    fn on_world_event(&mut self, _ev: &WorldEvent, _kart: &KartState, _out: &mut Vec<CarEffect>) {}
    /// Host: this car was hit by `damage` of bodywork damage / an explosion from `source`; return the damage that still applies
    /// (shields absorb). Called before the host applies `CarEffect::Damage` to this car. Default = unchanged.
    fn absorb_damage(&mut self, damage: f32, _from_explosion: bool, _kart: &KartState, _out: &mut Vec<CarEffect>) -> f32 {
        damage
    }
    /// `CBaseAbility::OnCarRender`: extra models the ability draws this frame (shields, bubbles, boomerangs that are not smackables).
    fn render_visuals(&self) -> Vec<WorldRequest> {
        Vec::new()
    }
    /// `CBaseAbility::IsActive @001bb24c` (non-virtual).
    fn is_active(&self) -> bool {
        self.base().is_active()
    }
}

/// `CCar::Update`'s per-ability pair: `OnCarAlwaysUpdate(dt)` always, `OnCarUpdate(dt)` only when `IsActive()`
/// (`CCar::Update @001a6cd8`, call sites at 0x1a72cc..).
pub fn tick_ability(a: &mut dyn Ability, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
    a.always_update(dt, kart, out);
    if a.is_active() {
        a.update(dt, kart, out);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CBaseAbility method bodies (free functions so derived classes can call the "base" version)
// ---------------------------------------------------------------------------------------------------------------------

/// `CBaseAbility::LoadAbilityValuesFromXML(CXGSXmlReaderNode const&) @001bc09c`.
/// xml tag -> field: `Duration`->+0x10, `EffectDuration`->+0x40 (min/max default to Duration), `BoostStrength`->+0x18,
/// `BoostDuration`->+0x1c, `CamBehindMod`->+0x5c (all default 0), `ChargeCount`->+0x34 (float truncated; default
/// `GetChargesPerPurchase(GetId())` = 3.0 for ids 8/0xe/3 else 1.0), `ReuseDelays/Default@value`->+0x14,
/// `ReuseDelays/Delay@number,@value`->table +0x88, `AIActivationTime@minTime,@maxTime`->+0x7c/+0x80.
pub fn base_load_values<A: Ability + ?Sized>(a: &mut A, p: &AbilityParams) {
    let id = a.id();
    let b = a.base_mut();
    let lvl = b.level;
    b.duration = p.float_for_level("Duration", 0.0, lvl);
    b.effect_duration = p.float_for_level("EffectDuration", b.duration, lvl);
    b.boost_strength = p.float_for_level("BoostStrength", 0.0, lvl);
    b.boost_duration = p.float_for_level("BoostDuration", 0.0, lvl);
    b.cam_behind_mod = p.float_for_level("CamBehindMod", 0.0, lvl);
    let default_charges = id.charges_per_purchase() as f32;
    b.charges = p.scalar_f32("ChargeCount", default_charges) as i32;
    if p.has_reuse_delays {
        if let Some(v) = p.reuse_default {
            b.reuse_delay_default = v;
        }
        b.reuse_table = p.reuse_delays.clone();
    }
    if let Some((mn, mx)) = p.ai_activation {
        b.ai_min_time = mn;
        b.ai_max_time = mx;
    }
    b.update_reuse_delay();
}

/// `CBaseAbility::TriggerAbility() @001b96d8` (the AI path of `CCar::TriggerAbility`, and the inner step of
/// `TriggerPlayerAbility`).
pub fn base_trigger<A: Ability + ?Sized>(a: &mut A, kart: &KartState, out: &mut Vec<CarEffect>) {
    // challenge-manager event (player only) is bookkeeping for the challenge system - not modelled
    let dur = a.duration(); // slot 0x88, computed before the flags change
    {
        let b = a.base_mut();
        b.active = true;
        b.triggered = true;
        if b.charges > 0 {
            b.charges -= 1;
        }
        b.time_remaining = dur;
        b.boost_time = b.boost_duration;
        b.effect_timer = b.effect_duration;
        // non-player drivers: pilot animation state 0xb
        if b.play_anim && !kart.is_player {
            out.push(CarEffect::Other { tag: "CPilotAnimationHandler::SetAnimState", car: Some(kart.id), vals: [11.0, 0.0, 0.0, 0.0], pos: kart.pos });
        }
    }
    if a.can_trigger_effects() {
        a.trigger_effects(kart, out);
    }
    let is_boss = a.base().is_boss;
    out.push(CarEffect::Other { tag: "ABKSound::CVoiceController::OnAbilityTriggered", car: Some(kart.id), vals: [kart.character as f32, 0.0, 0.0, 0.0], pos: kart.pos });
    if kart.character != 1 {
        out.push(CarEffect::Other {
            tag: "ABKSound::CAbilityController::OnAbilityStart",
            car: Some(kart.id),
            vals: [kart.character as f32, if is_boss { 1.0 } else { 0.0 }, 0.0, 0.0],
            pos: kart.pos,
        });
    }
}

/// `CBaseAbility::TriggerPlayerAbility() @001b953c`. The debug branch (`GetDebugBool(0x16)`, ability slow-mo camera,
/// shipped value false) is not ported. When the ability has charges it triggers, bumps `uses` and loads the reuse delay
/// of the NEXT use (`Delay number == uses_before + 2`, else the default).
pub fn base_trigger_player<A: Ability + ?Sized>(a: &mut A, kart: &KartState, out: &mut Vec<CarEffect>) {
    if a.base().play_anim {
        out.push(CarEffect::Other { tag: "CPilotAnimationHandler::SetAnimState", car: Some(kart.id), vals: [11.0, 0.0, 0.0, 0.0], pos: kart.pos });
    }
    if a.base().charges != 0 {
        a.trigger(kart, out); // slot 0x24
        let b = a.base_mut();
        let before = b.uses;
        b.uses = before + 1;
        let want = before + 2;
        b.reuse_delay = b.reuse_table.iter().find(|(n, _)| *n == want).map(|(_, v)| *v).unwrap_or(b.reuse_delay_default);
    }
}

/// `CBaseAbility::TriggerAbilityEffects() @001b9a44`: (re)spawns the character's ability particle effect on the car.
/// Only non-boss abilities of characters that have an `AbilityEffectName`.
pub fn base_trigger_effects<A: Ability + ?Sized>(a: &mut A, kart: &KartState, out: &mut Vec<CarEffect>) {
    let b = a.base_mut();
    if b.is_boss {
        return;
    }
    if b.env.effects.start.is_empty() {
        return;
    }
    b.fx_alive = true;
    out.push(CarEffect::Particle { name: b.env.effects.start.clone(), car: Some(kart.id), pos: kart.pos });
}

/// `CBaseAbility::StopEffects() @001b9b90`: removes the start effect (and sets `effect_timer = -1`), then spawns the
/// character's end effect when it has one.
pub fn base_stop_effects<A: Ability + ?Sized>(a: &mut A, kart: &KartState, out: &mut Vec<CarEffect>) {
    let b = a.base_mut();
    if !b.fx_alive {
        return; // `if (this+0x2c != -1)`
    }
    b.fx_alive = false;
    b.effect_timer = -1.0;
    out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(ability)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
    if !b.is_boss && !b.env.effects.end.is_empty() {
        b.end_fx_alive = true;
        out.push(CarEffect::Particle { name: b.env.effects.end.clone(), car: Some(kart.id), pos: kart.pos });
    }
}

/// `CBaseAbility::FinishAbility() @001b945c`.
pub fn base_finish<A: Ability + ?Sized>(a: &mut A, kart: &KartState, out: &mut Vec<CarEffect>) {
    let b = a.base_mut();
    if b.active {
        out.push(CarEffect::Other {
            tag: "ABKSound::CAbilityController::OnAbilityEnd",
            car: Some(kart.id),
            vals: [kart.character as f32, if b.is_boss { 1.0 } else { 0.0 }, 0.0, 0.0],
            pos: kart.pos,
        });
    }
    b.active = false;
    b.time_remaining = -1.0;
    b.boost_time = -1.0;
    // (player: CChallengeManager::Event - not modelled) pilot animation state 0xb only when `play_anim` is false
    if !b.play_anim {
        out.push(CarEffect::Other { tag: "CPilotAnimationHandler::SetAnimState", car: Some(kart.id), vals: [11.0, 0.0, 0.0, 0.0], pos: kart.pos });
    }
}

/// `CBaseAbility::OnCarIntegrate() @001b9de4`: the generic boost (`+Z force = rb+0x98 * BoostStrength` while the boost
/// timer is positive) and the dust trail (on while ground contact and active, never for boss abilities).
pub fn base_on_integrate<A: Ability + ?Sized>(a: &mut A, kart: &KartState, out: &mut Vec<CarEffect>) {
    let b = a.base_mut();
    if b.boost_time > 0.0 {
        out.push(CarEffect::BodyForce { car: kart.id, force_local: Vec3::new(0.0, 0.0, b.env.physics_dt * b.boost_strength), at_local: Vec3::ZERO });
    }
    // dust trail state machine
    let dust_off = !b.triggered || (!b.active && b.effect_timer <= 0.0) || b.is_boss;
    if dust_off {
        if b.dust_on {
            if !b.env.effects.dust.is_empty() && b.dust_fx_alive {
                out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(dust)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
                b.dust_fx_alive = false;
            }
            b.dust_on = false;
        }
        return;
    }
    if kart.wheels_on_ground < 1 {
        if b.dust_on {
            if !b.env.effects.dust.is_empty() && b.dust_fx_alive {
                out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(dust)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
                b.dust_fx_alive = false;
            }
            b.dust_on = false;
        }
    } else if !b.dust_on {
        if !b.env.effects.dust.is_empty() {
            if b.dust_fx_alive {
                out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(dust)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
            }
            out.push(CarEffect::Particle { name: b.env.effects.dust.clone(), car: Some(kart.id), pos: kart.pos });
            b.dust_fx_alive = true;
        }
        b.dust_on = true;
    }
}

/// `CBaseAbility::OnCarAlwaysUpdate(dt) @001b9398`: counts the boost timer down. (The slow-mo ability camera
/// bookkeeping `+0x48/+0x58` only runs with debug bool 0x16, shipped false - not ported.)
pub fn base_always_update<A: Ability + ?Sized>(a: &mut A, dt: f32, _kart: &KartState, _out: &mut Vec<CarEffect>) {
    let b = a.base_mut();
    if b.boost_time > 0.0 {
        b.boost_time -= dt;
    }
}

/// `CBaseAbility::OnCarUpdate(dt) @001b987c`: effect timer -> `StopEffects`, then the ability timer (initialised to
/// `GetDuration()` when it is exactly 0, otherwise `-= dt`); when it reaches 0 the ability finishes - and `boost_time`
/// survives the finish (it is saved and restored around the call). The trailing
/// `ABKSound::CAbilityController::OnAbilityUpdate` sound-controller tick carries no data and is not emitted.
pub fn base_update<A: Ability + ?Sized>(a: &mut A, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
    {
        let b = a.base_mut();
        if b.effect_timer > 0.0 {
            b.effect_timer -= dt;
        }
    }
    if a.base().effect_timer <= 0.0 {
        a.stop_effects(kart, out); // slot 0x2c
    }
    let t = if a.base().time_remaining == 0.0 {
        // (default `GetDuration` inlined: Duration, or 1.0 when <= 0; overridden GetDuration is called otherwise)
        let d = a.duration();
        a.base_mut().time_remaining = d;
        d
    } else {
        let t = a.base().time_remaining - dt;
        a.base_mut().time_remaining = t;
        t
    };
    if t <= 0.0 {
        let saved = a.base().boost_time;
        a.finish(kart, out); // slot 0x98
        a.base_mut().boost_time = saved;
    }
}

/// `CBaseAbility::CanTriggerAbility() @001b9d14`: false when the pilot is detached; players may trigger when one of the
/// debug bools {0x29, 0x25, 0x57, 0x58} is set (0x58 `Buy_Abilities_In_Race` is TRUE in the shipped data) and charges
/// remain; non-player drivers with the driver ability flag may always trigger; otherwise only boss abilities.
pub fn base_can_trigger(b: &AbilityBase, kart: &KartState) -> bool {
    if b.env.pilot_detached {
        return false;
    }
    if kart.is_player {
        let d = &b.env.debug;
        if (d.b29 || d.b25 || d.buy_pre_race || d.buy_in_race) && b.charges != 0 {
            return true;
        }
    }
    if b.env.driver_ability_flag {
        return true;
    }
    b.is_boss
}

/// `CBaseAbility::OnCarCamBehindMod(float) @001b996c`: when `CamBehindMod >= 0` the camera-behind value is replaced by
/// `(1 - (D - max(remaining, 0)) / D) * CamBehindMod` (D = `GetDuration()`), i.e. it decays from `CamBehindMod` to 0 over
/// the ability; otherwise `input` is returned.
pub fn base_cam_behind_mod(b: &AbilityBase, duration: f32, input: f32) -> f32 {
    if b.cam_behind_mod >= 0.0 {
        let rem = if b.time_remaining <= 0.0 { 0.0 } else { b.time_remaining };
        (1.0 - (duration - rem) / duration) * b.cam_behind_mod
    } else {
        input
    }
}

// =====================================================================================================================
// Factories
// =====================================================================================================================

/// `CBaseAbility::CreateAbility(EBirdAbility, CCar*) @001ba9e8` + `Init @001bbd9c` (the xml node is `params`; `level` is
/// `CCar+0x1a60`). Returns `None` for ids the original maps to a null (5, 6, 10, 13, 17..19) and for classes that are
/// not ported yet (phase 2 adds their arms here).
pub fn create_ability_by_enum(id: BirdAbility, params: &AbilityParams, level: f32) -> Option<Box<dyn Ability>> {
    use BirdAbility::*;
    let mut a: Box<dyn Ability> = match id {
        SpeedBoost => Box::new(speed::SpeedAbility::new(level)),
        RedSpeedBoost => Box::new(speed::RedSpeedAbility::new(level)),
        BlueSpeedBoost => Box::new(speed::BlueSpeedAbility::new(level)),
        Bomb => Box::new(bomb::BombAbility::new(level)),
        MinionDefence => Box::new(minion::MinionDefenceAbility::new(level)),
        StellaDefence => Box::new(stella::StellaDefenceAbility::new(level)),
        TerenceRage => Box::new(terence::TerenceRageAbility::new(level)),
        KingPig => Box::new(kingpig::KingPigAbility::new(level)),
        BubblesInflate => Box::new(bubbles::BubblesInflateAbility::new(level)),
        Moustache => Box::new(moustache::MoustacheAbility::new(level)),
        Hal => Box::new(hal::HalAbility::new(level)),
        Matilda => Box::new(matilda::MatildaAbility::new(level)),
        ChuckBoss => Box::new(boss::ChuckBossAbility::new(level)),
        KingPigBoss => Box::new(kingpig::KingPigBossAbility::new(level)),
        MatildaBoss => Box::new(matilda::MatildaBossAbility::new(level)),
        // (done) Bomb, MinionDefence, StellaDefence, TerenceRage, KingPig, BubblesInflate,
        // Moustache, Hal, Matilda, ChuckBoss, KingPigBoss, MatildaBoss.
        // The original returns null for StellaBossAbility(5), ObjectSpawn(6), OvertakeSpeedBoost(10), BlueBoss(13),
        // BubblesBoss(17), MoustacheBoss(18), HalBoss(19): those only exist through the boss factory.
        _ => return None,
    };
    a.load_values(params);
    Some(a)
}

/// Brief-level entry point: `GetBirdAbilityFromString(name)` then [`create_ability_by_enum`].
pub fn create_ability(name: &str, params: &AbilityParams, level: f32) -> Option<Box<dyn Ability>> {
    create_ability_by_enum(BirdAbility::from_name(name)?, params, level)
}

/// `CBaseAbility::CreateBossAbility(int boss, CCar*, int index) @001bb92c` + `InitBoss @001bbee0`: `ty` is the `type`
/// attribute of the `index`-th `<Ability>` (`GetBossAbilityType @001bb664`). Boss abilities have `is_boss = true`.
pub fn create_boss_ability_by_enum(id: BirdAbility, params: &AbilityParams, level: f32) -> Option<Box<dyn Ability>> {
    use BirdAbility::*;
    let mut a: Box<dyn Ability> = match id {
        OvertakeSpeedBoost => Box::new(speed::OvertakeSpeedAbility::new(level)),
        StellaBossAbility => Box::new(stella::StellaBossAbility::new(level)),
        ObjectSpawn => Box::new(objectspawn::ObjectSpawnAbility::new(level)),
        BlueBoss => Box::new(boss::BlueBossAbility::new(level)),
        BubblesBoss => Box::new(bubbles::BubblesBossAbility::new(level)),
        MoustacheBoss => Box::new(moustache::MoustacheBossAbility::new(level)),
        HalBoss => Box::new(hal::HalBossAbility::new(level)),
        ChuckBoss => Box::new(boss::ChuckBossAbility::new(level)),
        KingPigBoss => Box::new(kingpig::KingPigBossAbility::new(level)),
        MatildaBoss => Box::new(matilda::MatildaBossAbility::new(level)),
        // (done) StellaBossAbility(5), ObjectSpawn(6), BlueBoss(13), BubblesBoss(17), MoustacheBoss(18), HalBoss(19),
        // ChuckBoss(20), KingPigBoss(21), MatildaBoss(22). The original returns null for 7, 8, 9, 11, 12, 14, 15, 16.
        _ => return None,
    };
    a.load_values(params);
    Some(a)
}

/// Boss factory by the xml `type` string.
pub fn create_boss_ability(ty: &str, params: &AbilityParams, level: f32) -> Option<Box<dyn Ability>> {
    create_boss_ability_by_enum(BirdAbility::from_name(ty)?, params, level)
}

// =====================================================================================================================
// CCar side
// =====================================================================================================================

/// Race / game state the `CCar` ability functions consult (`CGame`, `CMetagameManager`, `CNetwork`).
#[derive(Clone, Debug)]
pub struct RaceAbilityCtx<'a> {
    /// `CGame+0xc8` (game state; `CanTriggerAbility` requires `!= 6 || aux == 8`).
    pub game_state: i32,
    /// `CGame+0xcc`.
    pub game_state_aux: i32,
    /// `*(*(*(CGame+0x1c)+0x20c)+8) != 0`: the current event's mode allows abilities.
    pub event_allows_abilities: bool,
    /// `CGame::IsMultipleAbilitiesEnabled @001196ac`: `!(mode == 0xe && !dbg77) && CGame+0x31c4 < 2`.
    pub multiple_abilities_enabled: bool,
    /// `CGame+0x31c4`: number of players.
    pub num_players: i32,
    /// `CNetwork::GetMPGameState() != 0`.
    pub mp_game_active: bool,
    /// Debug bool 0x77.
    pub dbg77: bool,
    /// Economy numbers for `CMetagameManager::CanUseAbility`.
    pub economy: &'a AbilityEconomy,
}

/// The car's ability slots: `CCar+0x1c2c` main ability, `+0x1c30..+0x1c3c` up to 4 boss abilities (count `+0x1c40`),
/// the ability clock `+0x1c44`, time since trigger `+0x1c48`, ready time `+0x1c4c`, cached charged fraction `+0x1c50`.
pub struct CarAbilitySlot {
    pub ability: Option<Box<dyn Ability>>,
    pub boss: Vec<Box<dyn Ability>>,
    /// +0x1c44: advanced by dt in `CCar::Update @001a6cd8` (the gate `iVar24 == 0 || car+0x468 == -1` was not resolved:
    /// the host calls [`CarAbilitySlot::tick`] on every frame the car updates).
    pub clock: f32,
    /// +0x1c48 seconds since the last trigger.
    pub since_trigger: f32,
    /// +0x1c4c ability usable when `ready_time < clock`.
    pub ready_time: f32,
    /// +0x1c50 last charged fraction.
    pub charged_fraction_cache: f32,
    /// +0x448 set by `TriggerAbility` / `TriggerBossAbility` (consumed by the car code - UNRESOLVED consumer).
    pub triggered_flag: bool,
}

impl CarAbilitySlot {
    /// `CCar::CCar @001a8588` ability part: `CreateAbility(character ability)`, `clock = since = 0`,
    /// `ready_time = ability.GetReuseDelay()` (`GetDebugFloat(0x98)` when there is no ability).
    pub fn new(ability: Option<Box<dyn Ability>>) -> CarAbilitySlot {
        let ready = ability.as_ref().map(|a| a.reuse_delay()).unwrap_or(DEBUG_FLOAT_ABILITY_HOLDOFF_TIME);
        CarAbilitySlot { ability, boss: Vec::new(), clock: 0.0, since_trigger: 0.0, ready_time: ready, charged_fraction_cache: 0.0, triggered_flag: false }
    }

    /// `CCar::LoadBossAbilities @001b0d60` adds up to four.
    pub fn push_boss(&mut self, a: Box<dyn Ability>) -> bool {
        if self.boss.len() >= 4 {
            return false;
        }
        self.boss.push(a);
        true
    }

    /// Ability part of `CCar::Update`: clocks + `OnCarAlwaysUpdate`/`OnCarUpdate` of every ability.
    pub fn tick(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.clock += dt;
        self.since_trigger += dt;
        if let Some(a) = self.ability.as_mut() {
            tick_ability(a.as_mut(), dt, kart, out);
        }
        for a in self.boss.iter_mut() {
            tick_ability(a.as_mut(), dt, kart, out);
        }
    }

    /// Host: all world requests queued by the main and boss abilities since the last call.
    pub fn drain_requests(&mut self) -> Vec<WorldRequest> {
        let mut v = Vec::new();
        if let Some(a) = self.ability.as_mut() {
            v.extend(a.drain_requests());
        }
        for a in self.boss.iter_mut() {
            v.extend(a.drain_requests());
        }
        v
    }

    /// Host: deliver a world callback to every ability of the car (each ignores handles it does not own).
    pub fn deliver(&mut self, ev: &WorldEvent, kart: &KartState, out: &mut Vec<CarEffect>) {
        if let Some(a) = self.ability.as_mut() {
            a.on_world_event(ev, kart, out);
        }
        for a in self.boss.iter_mut() {
            a.on_world_event(ev, kart, out);
        }
    }

    /// Host: push the per-frame environment into every ability of the car.
    pub fn set_env(&mut self, env: &AbilityEnv) {
        if let Some(a) = self.ability.as_mut() {
            let rng = a.base().env.rng;
            let mut e = env.clone();
            e.rng = rng;
            e.effects = a.base().env.effects.clone();
            e.is_local_player = a.base().env.is_local_player;
            a.set_env(e);
        }
        for a in self.boss.iter_mut() {
            let rng = a.base().env.rng;
            let mut e = env.clone();
            e.rng = rng;
            a.set_env(e);
        }
    }

    /// Damage that survives the shields of the car's abilities (`absorb_damage` of every ability).
    pub fn absorb_damage(&mut self, damage: f32, from_explosion: bool, kart: &KartState, out: &mut Vec<CarEffect>) -> f32 {
        let mut d = damage;
        if let Some(a) = self.ability.as_mut() {
            d = a.absorb_damage(d, from_explosion, kart, out);
        }
        for a in self.boss.iter_mut() {
            d = a.absorb_damage(d, from_explosion, kart, out);
        }
        d
    }

    /// `ImmuneToSpin` / `ImmuneToDamage` / `ImmuneToExplosions` of the car's abilities.
    pub fn immunities(&self) -> (bool, bool, bool) {
        let mut r = (false, false, false);
        let mut f = |a: &dyn Ability| {
            r.0 |= a.immune_to_spin();
            r.1 |= a.immune_to_damage();
            r.2 |= a.immune_to_explosions();
        };
        if let Some(a) = self.ability.as_ref() {
            f(a.as_ref());
        }
        for a in self.boss.iter() {
            f(a.as_ref());
        }
        r
    }

    /// Visual models the car's abilities want drawn this frame.
    pub fn visuals(&self) -> Vec<WorldRequest> {
        let mut v = Vec::new();
        if let Some(a) = self.ability.as_ref() {
            v.extend(a.render_visuals());
        }
        for a in self.boss.iter() {
            v.extend(a.render_visuals());
        }
        v
    }

    /// Ability part of `CCar::Integrate`: `OnCarIntegrate` of the main and every boss ability.
    pub fn integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if let Some(a) = self.ability.as_mut() {
            a.on_integrate(kart, out);
        }
        for a in self.boss.iter_mut() {
            a.on_integrate(kart, out);
        }
    }

    /// `CCar::SetInSlingshot @00199f64` tail: `ready_time = clock + GetReuseDelay()`.
    pub fn reset_ready_time(&mut self) {
        if let Some(a) = self.ability.as_ref() {
            self.ready_time = self.clock + a.reuse_delay();
        }
    }

    /// `CCar::IsAbilityActive @001aaa80`.
    pub fn is_active(&self) -> bool {
        self.ability.as_ref().map(|a| a.is_active()).unwrap_or(false)
    }

    /// `CCar::GetAbilityCharges @001aade8`.
    pub fn charges(&self) -> i32 {
        self.ability.as_ref().map(|a| a.charges()).unwrap_or(0)
    }

    /// `CCar::GetAbilityUsesThisRace @001aae74`.
    pub fn uses_this_race(&self) -> i32 {
        self.ability.as_ref().map(|a| a.uses_this_race()).unwrap_or(0)
    }

    /// `CCar::OnPlayerAbilityPurchase @001aae20`.
    pub fn on_player_purchase(&mut self) {
        if let Some(a) = self.ability.as_mut() {
            a.on_player_purchase();
        }
    }

    /// `CCar::CanTriggerAbility() @001aaaa4`.
    pub fn can_trigger(&self, kart: &KartState, ctx: &RaceAbilityCtx) -> bool {
        let a = match self.ability.as_ref() {
            Some(a) => a,
            None => return false,
        };
        if !a.can_trigger(kart) {
            return false;
        }
        if !((ctx.game_state != 6 || ctx.game_state_aux == 8) && ctx.event_allows_abilities && self.ready_time < self.clock) {
            return false;
        }
        if ctx.multiple_abilities_enabled && !can_use_ability(a.uses_this_race(), a.id(), ctx.economy) {
            return false;
        }
        let uses = a.uses_this_race();
        let cpp = a.id().charges_per_purchase();
        if uses <= cpp {
            return true;
        }
        if ctx.dbg77 || !ctx.mp_game_active {
            // second test of the original: uses <= cpp (false here) else `num_players < 2`
            return ctx.num_players < 2;
        }
        false
    }

    /// `CCar::TriggerAbility() @0019fa48`. Players go through `TriggerPlayerAbility` (charge check, `uses++`, next reuse
    /// delay); AI cars call `TriggerAbility` directly (no charge check) unless debug bool 0x56. In both cases the next
    /// allowed time is `clock + GetDuration() + GetReuseDelay()` (even when a player had 0 charges).
    /// Returns true when the trigger branch was entered.
    pub fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) -> bool {
        let clock = self.clock;
        let ready = self.ready_time;
        let a = match self.ability.as_mut() {
            Some(a) => a,
            None => return false,
        };
        let dbg29 = a.base().env.debug.b29;
        let dbg56 = a.base().env.debug.b56;
        if !((a.can_trigger(kart) || dbg29) && ready < clock) {
            return false;
        }
        self.triggered_flag = true;
        if kart.is_player {
            a.trigger_player(kart, out); // slot 0x20
        } else if !dbg56 {
            a.trigger(kart, out); // slot 0x24
        }
        self.since_trigger = 0.0;
        self.ready_time = clock + a.duration() + a.reuse_delay();
        true
    }

    /// `CCar::GetAbilityChargedFraction() @001aac70` (HUD ability bar).
    pub fn charged_fraction(&mut self, multiple_abilities_enabled: bool) -> f32 {
        let (uses, cpp, active, dur, remaining, reuse) = match self.ability.as_ref() {
            Some(a) => (a.uses_this_race(), a.id().charges_per_purchase(), a.is_active(), a.duration(), a.base().time_remaining, a.reuse_delay()),
            None => (0, 1, false, 1.0, 0.0, 0.0),
        };
        if cpp <= uses && !multiple_abilities_enabled && self.charged_fraction_cache < CHARGED_FRACTION_ZERO_THRESHOLD {
            self.charged_fraction_cache = 0.0;
            return 0.0;
        }
        if self.ability.is_some() {
            if active {
                let f = remaining / dur;
                self.charged_fraction_cache = f;
                return f;
            }
            let f = (reuse - (self.ready_time - self.clock)) / reuse;
            self.charged_fraction_cache = f;
            return f;
        }
        self.charged_fraction_cache
    }

    /// `CCar::CanTriggerBossAbility(int) @001ab0b4`.
    pub fn can_trigger_boss(&self, idx: usize, kart: &KartState) -> bool {
        self.boss.get(idx).map(|a| a.can_trigger(kart)).unwrap_or(false)
    }

    /// `CCar::TriggerBossAbility(int) @0019fb74`: needs `CanTriggerAbility` and `event_allows_abilities`; plays the
    /// "ABY_battle_puff" sound for characters 1, 3, 4, 5, 8. Boss abilities use slot 0x24 directly.
    pub fn trigger_boss(&mut self, idx: usize, kart: &KartState, event_allows_abilities: bool, out: &mut Vec<CarEffect>) -> bool {
        let a = match self.boss.get_mut(idx) {
            Some(a) => a,
            None => return false,
        };
        if !(a.can_trigger(kart) && event_allows_abilities) {
            return false;
        }
        a.trigger(kart, out);
        let c = kart.character as u32;
        if c.wrapping_sub(3) < 3 || c == 1 || c == 8 {
            out.push(CarEffect::Sound { name: "ABY_battle_puff".to_string(), car: Some(kart.id) });
        }
        true
    }

    /// `CCar::SetFinishLineCrossed @001aa158` / `SetRaceCompleted @001aa0a4`: finish the running ability and stop its
    /// effects.
    pub fn finish_running(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if let Some(a) = self.ability.as_mut() {
            if a.is_active() {
                a.finish(kart, out);
                a.stop_effects(kart, out);
            }
        }
    }
}

// =====================================================================================================================
// CMetagameManager (economy.xml)
// =====================================================================================================================

/// The economy numbers behind `CMetagameManager::CanUseAbility @0012d7ec`, `GetBirdAbilityCostInRace @0012d86c` and
/// `GetBirdAbilityCostPreRace @0012d964`, from `economy.xml` (`ReadEconomyDataFromXML @0012e058`):
/// `<BirdAbilityCost><CostBeforeRace>` -> +0x644 (default 2), `<MaxAbility>` -> +0x648 (default 10),
/// `<RaceAbilityCosts MaxUses=><Cost value=/>...` -> +0x46cc (default -1) and the table +0x46d0/+0x46d4.
#[derive(Clone, Debug)]
pub struct AbilityEconomy {
    pub cost_before_race: i32,
    pub max_ability: i32,
    pub max_uses: i32,
    pub costs: Vec<u32>,
}

impl Default for AbilityEconomy {
    fn default() -> Self {
        AbilityEconomy { cost_before_race: 2, max_ability: 10, max_uses: -1, costs: Vec::new() }
    }
}

impl AbilityEconomy {
    /// Parse `economy.xml` (the `BirdAbilityCost` and `RaceAbilityCosts` elements anywhere in the document).
    pub fn from_economy_xml(xml: &str) -> AbilityEconomy {
        let mut e = AbilityEconomy::default();
        let doc = match roxmltree::Document::parse(xml) {
            Ok(d) => d,
            Err(_) => return e,
        };
        let geti = |n: roxmltree::Node, name: &str, d: i32| child_named(n, name).and_then(|c| c.text()).and_then(|t| t.trim().parse::<i32>().ok()).unwrap_or(d);
        for n in doc.descendants().filter(|n| n.is_element()) {
            match n.tag_name().name() {
                "BirdAbilityCost" => {
                    e.max_ability = geti(n, "MaxAbility", 10);
                    e.cost_before_race = geti(n, "CostBeforeRace", 2);
                }
                "RaceAbilityCosts" => {
                    e.max_uses = n.attribute("MaxUses").and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(-1);
                    e.costs = elem_children(n)
                        .filter(|c| c.tag_name().name() == "Cost")
                        .map(|c| c.attribute("value").and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0))
                        .collect();
                }
                _ => {}
            }
        }
        e
    }
}

/// `CMetagameManager::CanUseAbility(CCar*) @0012d7ec`: true when the ability has not been used yet; otherwise
/// `false` when `uses / GetChargesPerPurchase > MaxUses` (and `MaxUses > 0`), else true iff a cost table exists.
pub fn can_use_ability(uses_this_race: i32, id: BirdAbility, eco: &AbilityEconomy) -> bool {
    if uses_this_race == 0 {
        return true;
    }
    let n = uses_this_race / id.charges_per_purchase();
    if n > eco.max_uses && eco.max_uses > 0 {
        return false;
    }
    !eco.costs.is_empty()
}

/// `CMetagameManager::GetBirdAbilityCostInRace(CCar*) @0012d86c`: `None` = the original's `-1` (no ability / not
/// purchasable), `Some(0)` = free (no complete purchase used yet), else the table cost
/// `costs[min(n, count) - 1]` with `n = uses / GetChargesPerPurchase`.
pub fn bird_ability_cost_in_race(ability: Option<(i32, BirdAbility)>, eco: &AbilityEconomy) -> Option<u32> {
    let (uses, id) = ability?;
    let n = uses / id.charges_per_purchase();
    if n == 0 {
        return Some(0);
    }
    if n > eco.max_uses && eco.max_uses > 0 {
        return None;
    }
    if eco.costs.is_empty() {
        return None;
    }
    let idx = (n as usize).min(eco.costs.len()) - 1;
    Some(eco.costs[idx])
}

/// `CMetagameManager::GetBirdAbilityCostPreRace() @0012d964` (`+0x644`).
pub fn bird_ability_cost_pre_race(eco: &AbilityEconomy) -> i32 {
    eco.cost_before_race
}

#[cfg(test)]
#[path = "abilities/tests.rs"]
mod tests;
