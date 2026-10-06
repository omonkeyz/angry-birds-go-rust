//! BOSS_BATTLE ("Champion Chase") and BOSS_FRUIT_RUSH ("Champion Fruit Splat") game modes.
//!
//! Port of `CGameModeBossBattle` / `CGameModeBossBattleData` (2.9.1 `libABK291.so`, 0017d860..0017dd60 and the
//! vtable at d917f8), `CGameModeBossFruitRush` (0017e084..0017e5cc), `CRaceAI::SetAsBoss` (0018c92c),
//! `CRaceAI::UpdateAbilityAI` boss branch (`[clone .part.50]` @00187164, dispatched from @0018a58c),
//! `CCar::LoadBossAbilities` (001b0d60), `CBaseAbility::GetBossAbilityCount/Type` (001bb57c / 001bb664),
//! `CBaseAbility::CreateBossAbility` (001bb92c), `CBaseAbility::InitBoss` (001bbee0).
//!
//! # What the original game mode really is (resolved from the decompile)
//! * A boss battle is an ordinary **race against AI karts**. Every AI car gets `CRaceAI::SetAsBoss(bosslevel)`
//!   in `InitialiseCarData` (0017dbe8): the boss file `CHARSPEC:Boss_%03d.xml` supplies the kart, the pilot, the
//!   AI ability range/cooldowns and the boss weapon (`<Ability>`).
//! * There is **no boss health, no hit counter and no phases**. The vtable of `CGameModeBossBattle` (d917f8) points
//!   `OnWeaponFire` (+0x34) and `OnCarCollision` (+0x38) at the base no-ops 0017c38c / 0017c390, and the mode never
//!   reads projectile or damage events. `ProjectileHit`, `Smashed` and `CarHitCar` are therefore ignored here.
//!   (RESOLVED: none in the original. No `ModeEvent::BossHealth` is ever emitted.)
//! * **Win** = the player's race position is 1 (`GetEventCompleted` @0017d9ec: `data+0x10 == 1`), evaluated when
//!   the player crosses the finish line (`CGameMode::ProcessUpdate` @0017caac sets data state 2 = completed, else 3).
//!   The boss finishing first does not end the mode, it just makes the player's position 2.
//! * **Game over** (`CheckGameOverCondition` @0017d860): every human player has finished.
//! * One-time **"Boss Destroyed" bonus** (`Update` @0017db10 / FruitRush @0017e2b4): the first time any AI car's
//!   `car+0x4a0` (pilot-detach countdown latch, -1.0 at reset, 2.0 on detach, stored as 0.0 on expiry) reads exactly
//!   0.0, score counter 6 (`CScoreCounterBonus`) gets +10000.0 (`mov r1,#0x4000; movt r1,#0x461c`).
//!   UNRESOLVED: `LoadBossAbilities` sets `car+0x1b4c = 1`, and that flag gates both detach paths found
//!   (`CCar::AddImpactDamage` @001a070c returns early; `IntegrateSteering` @001974d4 checks `1b4c == 0`), so whether
//!   the bonus can ever fire for a boss in the shipped game is not established. The host feeds the latch through
//!   `ModeInput::Other { tag: "PilotDetachTimer" }`.
//! * `CGameModeBossBattleData::Update` @0017dacc and `::Reset` @0017dad0 are 4-byte thunks into `CGameModeData`
//!   (@0017c450 / @0017c3a8), so the per-car data is exactly `CGameModeData`: state (+4), race time (+8), final
//!   position (+0xc), current position (+0x10), best position (+0x14, init 1000), bonus coins (+0x18).
//!
//! Standalone: `std` + `glam` (through `types`) + `roxmltree`.
#![allow(dead_code)]

use super::types::*;
use roxmltree::{Document, Node};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------------------------------------------------
// Constants (each from the decompile or the shipped data)
// ------------------------------------------------------------------------------------------------------------

/// `CGameModeBossBattle::Update` @0017db10 (`mov r1,#0x4000; movt r1,#0x461c` = 0x461c4000) and
/// `CGameModeBossFruitRush::Update` @0017e2b4 (same constant): amount added to score counter 6.
pub const BOSS_DESTROYED_BONUS: f32 = 10000.0;
/// `CScoreCounterBonus::GetType` @001f05bc.
pub const SCORE_COUNTER_BONUS: u32 = 6;
/// `CRaceAI` ctor-time defaults @~00189fxx (the function writing `this+0x1c8/0x1cc/0x1d0`, decompile line 167891,
/// enclosing function = `CRaceAI::CRaceAI`): MinCooldown 5.0 (0x40a00000), MaxCooldown 10.0 (0x41200000),
/// MaxRange 10.0 (0x41200000). `LoadBossAbilities` only overwrites a value when the xml attribute is non-zero.
pub const DEFAULT_AI_MIN_COOLDOWN: f32 = 5.0;
pub const DEFAULT_AI_MAX_COOLDOWN: f32 = 10.0;
pub const DEFAULT_AI_MAX_RANGE: f32 = 10.0;
/// `CGameModeBossBattle::InitialiseCarData` @0017dbe8 / FruitRush @0017e4c4 write 0x40a00000 (5.0) into
/// `CGame+0x3244`. UNRESOLVED: field meaning unknown, emitted as `CarEffect::Other { tag: "GameField3244" }`.
pub const GAME_FIELD_3244_VALUE: f32 = 5.0;
/// Highest `<Ability>` count `LoadBossAbilities` can store (`CCar+0x1c30..0x1c40`, count at `+0x1c40`).
pub const MAX_BOSS_ABILITIES: usize = 4;
/// `CGameModeData::CGameModeData` @0017ca38 (and `Reset` @0017c3a8): best-position initial value 1000 (0x3e8).
pub const INITIAL_BEST_POSITION: i32 = 1000;
/// Game-mode ids (`StringToGameMode` @~000fdxxx: "BOSS_BATTLE" -> 0xb, "BOSS_FRUIT_RUSH" -> 0xc).
pub const GAME_MODE_ID_BOSS_BATTLE: u32 = 0xb;
pub const GAME_MODE_ID_BOSS_FRUIT_RUSH: u32 = 0xc;

/// Character bytes for which `CCar::TriggerBossAbility` @0019fb74 plays `ABY_battle_puff`
/// (`uVar3 - 3 < 3 || uVar3 == 1 || uVar3 == 8` = bytes 1, 3, 4, 5, 8).
pub fn plays_battle_puff(character: u8) -> bool {
    matches!(character, 1 | 3 | 4 | 5 | 8)
}

// ------------------------------------------------------------------------------------------------------------
// Boss ability types
// ------------------------------------------------------------------------------------------------------------

/// Ability ids returned by `CBaseAbility::GetBossAbilityType` @001bb664 (and `GetBirdAbilityFromString` @001ba870).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u32)]
pub enum AbilityType {
    Unknown = 0,
    SpeedBoost = 1,
    Bomb = 2,
    MinionDefence = 3,
    StellaDefence = 4,
    StellaBossAbility = 5,
    ObjectSpawn = 6,
    RedSpeedBoost = 7,
    BlueSpeedBoost = 8,
    TerenceRage = 9,
    OvertakeSpeedBoost = 10,
    KingPigAbility = 0xb,
    BubblesInflateAbility = 0xc,
    BlueBossAbility = 0xd,
    MoustacheAbility = 0xe,
    HalAbility = 0xf,
    MatildaAbility = 0x10,
    BubblesBossAbility = 0x11,
    MoustacheBossAbility = 0x12,
    HalBossAbility = 0x13,
    ChuckBossAbility = 0x14,
    KingPigBossAbility = 0x15,
    MatildaBossAbility = 0x16,
}

impl AbilityType {
    /// `CBaseAbility::GetBossAbilityType` @001bb664 + `GetBirdAbilityFromString` @001ba870 (exact strcmp chains).
    pub fn from_type_string(s: &str) -> AbilityType {
        use AbilityType::*;
        match s {
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
            "KingPigAbility" => KingPigAbility,
            "KingPigBossAbility" => KingPigBossAbility,
            "BubblesInflateAbility" => BubblesInflateAbility,
            "BlueBossAbility" => BlueBossAbility,
            "BubblesBossAbility" => BubblesBossAbility,
            "MoustacheAbility" => MoustacheAbility,
            "MoustacheBossAbility" => MoustacheBossAbility,
            "HalAbility" => HalAbility,
            "HalBossAbility" => HalBossAbility,
            "MatildaAbility" => MatildaAbility,
            "MatildaBossAbility" => MatildaBossAbility,
            "ChuckBossAbility" => ChuckBossAbility,
            _ => Unknown,
        }
    }

    /// `CBaseAbility::CreateBossAbility` @001bb92c: the switch on `type - 5` returns an object only for
    /// Stella(5), ObjectSpawn(6), Overtake(10), BlueBoss(0xd), BubblesBoss(0x11), MoustacheBoss(0x12), HalBoss(0x13),
    /// ChuckBoss(0x14), KingPigBoss(0x15), MatildaBoss(0x16); every other case returns null.
    pub fn creates_ability(self) -> bool {
        use AbilityType::*;
        matches!(
            self,
            StellaBossAbility
                | ObjectSpawn
                | OvertakeSpeedBoost
                | BlueBossAbility
                | BubblesBossAbility
                | MoustacheBossAbility
                | HalBossAbility
                | ChuckBossAbility
                | KingPigBossAbility
                | MatildaBossAbility
        )
    }

    /// Original class created for the type (`CreateBossAbility` cases).
    pub fn class_name(self) -> Option<&'static str> {
        use AbilityType::*;
        Some(match self {
            StellaBossAbility => "CStellaBossAbility",
            ObjectSpawn => "CObjectSpawnAbility",
            OvertakeSpeedBoost => "COvertakeSpeedAbility",
            BlueBossAbility => "CBlueBossAbility",
            BubblesBossAbility => "CBubblesBossAbility",
            MoustacheBossAbility => "CMoustacheBossAbility",
            HalBossAbility => "CHalBossAbility",
            ChuckBossAbility => "CChuckBossAbility",
            KingPigBossAbility => "CKingPigBossAbility",
            MatildaBossAbility => "CMatildaBossAbility",
            _ => return None,
        })
    }

    /// Does `CanTriggerAtDistance` (vtable +0x7c) also require the boss to be ahead along the track (`delta > 0`)?
    /// ObjectSpawn @001cd910, Stella @001cff48, Blue @001bc8cc, Bubbles @001beb98: yes. Chuck @001c18b4 and Matilda
    /// @001c6c60: range only. Hal/KingPig/Moustache derive from `CObjectSpawnAbility` (constructors call
    /// `CObjectSpawnAbility::CObjectSpawnAbility`) and define no own `CanTriggerAtDistance`, so they inherit the
    /// ObjectSpawn rule (vtable slot not dumped: UNRESOLVED-minor).
    pub fn needs_boss_ahead(self) -> bool {
        use AbilityType::*;
        !matches!(self, ChuckBossAbility | MatildaBossAbility)
    }
}

/// One `<Ability name="Close" type="...">` of a boss file, with the raw child keys (the ability classes read them
/// in their `LoadAbilityValuesFromXML`; the follow-up ability port consumes `params`).
#[derive(Clone, Debug)]
pub struct BossAbilityDef {
    pub name: String,
    pub type_string: String,
    pub kind: AbilityType,
    /// All child elements in file order: (tag, text).
    pub params: Vec<(String, String)>,
    /// `<MinCooldown>` / `<MaxCooldown>` (post-trigger delay, `GetCooldown`).
    pub min_cooldown: f32,
    pub max_cooldown: f32,
    /// `<MinDistance>` / `<MaxDistance>` squared, as the classes store them (`this+0x98` / `+0x9c`).
    pub min_dist_sq: f32,
    pub max_dist_sq: f32,
    /// `<ReleaseTime>` (ObjectSpawn family: added to the cooldown after each trigger, `this+0x164 = this+0xd0`).
    pub release_time: f32,
}

impl BossAbilityDef {
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    pub fn param_f32(&self, key: &str) -> Option<f32> {
        self.param(key).and_then(|v| v.trim().parse::<f32>().ok())
    }
    /// `CreateBossAbility` returns an object for this ability.
    pub fn creates_ability(&self) -> bool {
        self.kind.creates_ability()
    }

    /// `CanTriggerAtDistance(distSq, deltaS)` (vtable +0x7c): `dist_sq` = squared distance boss -> local player,
    /// `delta_s` = boss spline position minus player spline position (positive = boss ahead).
    /// Source: ObjectSpawn @001cd910 (`maxSq <= d -> false; return 0 < delta && minSq <= d`), Chuck @001c18b4 and
    /// Matilda @001c6c60 (`maxSq <= d -> false; return minSq <= d`).
    pub fn can_trigger_at_distance(&self, dist_sq: f32, delta_s: f32) -> bool {
        if self.max_dist_sq <= dist_sq {
            return false;
        }
        if self.kind.needs_boss_ahead() {
            0.0 < delta_s && self.min_dist_sq <= dist_sq
        } else {
            self.min_dist_sq <= dist_sq
        }
    }

    /// `GetCooldown()` (vtable +0x80): `lerp(MinCooldown, MaxCooldown, rng01) + extra` with the RNG vtable
    /// `Float(min,max)` (default implementation `min + r * (max - min)`, @0018xxxx RNG slot 0x1c). `extra`:
    /// ObjectSpawn @001cdf50 adds `this+0x164` (= `ReleaseTime` stored by `TriggerAbility`); Blue @001bcb04 (+0x13c),
    /// Bubbles @001bed84 (+0xec), Stella @001d06c4 (+0xdc) add a field written by their `TriggerAbility`
    /// (UNRESOLVED: assumed to hold ReleaseTime, not verified field by field); Moustache @001ccc94 adds the largest
    /// pending rocket-launch delay (host supplied); Chuck @001c1968 and Matilda @001c6d44 add nothing.
    pub fn cooldown(&self, rng01: f32, extra: f32) -> f32 {
        self.min_cooldown + rng01 * (self.max_cooldown - self.min_cooldown) + extra
    }

    /// Cooldown extra the scheduler adds after a trigger (see [`BossAbilityDef::cooldown`]).
    fn extra_after_trigger(&self, pending_rocket_delay: f32) -> f32 {
        use AbilityType::*;
        match self.kind {
            ChuckBossAbility | MatildaBossAbility => 0.0,
            MoustacheBossAbility => pending_rocket_delay,
            _ => self.release_time,
        }
    }
}

// ------------------------------------------------------------------------------------------------------------
// Boss definition (boss_NNN.xml)
// ------------------------------------------------------------------------------------------------------------

/// `<BodyworkSpec .../>` attributes kept raw (consumed by the damage module).
pub type Attrs = BTreeMap<String, String>;

/// One `CHARSPEC:Boss_%03d.xml` file (`assets292\xml\characters\charxml\boss_NNN.xml`).
#[derive(Clone, Debug)]
pub struct BossDef {
    /// `NNN` of the file name = the eventdef `bosslevel`.
    pub id: u32,
    /// `<Name>` text (always "Boss").
    pub name: String,
    pub bodywork: Attrs,
    /// `<Kart m_sNameStringID m_iUpgradeLevel/>` (`CGameModeBossBattle::GetAIKart` @0017dee0).
    pub kart_name_string_id: String,
    pub kart_upgrade_level: i32,
    /// `<Pilot Name=/>` (`GetAICharacter` @0017dd60 compares it with the 16 character names).
    pub pilot_name: String,
    /// `<AbilityAttributes MaxRange MinCooldown MaxCooldown/>` after the "override only when non-zero" rule of
    /// `CCar::LoadBossAbilities` @001b0d60, starting from the `CRaceAI` defaults 10 / 5 / 10.
    pub max_range: f32,
    pub min_cooldown: f32,
    pub max_cooldown: f32,
    /// Raw attribute values (0.0 when absent), before the override rule.
    pub raw_max_range: f32,
    pub raw_min_cooldown: f32,
    pub raw_max_cooldown: f32,
    /// `<Ability>` children in order (`CountElement("Ability")` = `abilities.len()`).
    pub abilities: Vec<BossAbilityDef>,
}

fn attr_f32(n: &Node, key: &str) -> f32 {
    n.attribute(key).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0)
}

fn child<'a, 'b>(n: &Node<'a, 'b>, tag: &str) -> Option<Node<'a, 'b>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == tag)
}

impl BossDef {
    /// Parse a boss file. `GetFirstChild` takes a hidden element-name argument in the decompile (name lost in the
    /// listing), so elements are looked up by name: `Kart`, `Pilot`, `AbilityAttributes`, `Ability`.
    pub fn from_xml(id: u32, xml: &str) -> Result<BossDef, String> {
        let doc = Document::parse(xml).map_err(|e| format!("boss_{id:03}: {e}"))?;
        let root = doc.root_element();
        if root.tag_name().name() != "Character" {
            return Err(format!("boss_{id:03}: root is <{}>", root.tag_name().name()));
        }
        let name = child(&root, "Name").and_then(|n| n.text()).unwrap_or("").to_string();
        let mut bodywork = Attrs::new();
        if let Some(b) = child(&root, "BodyworkSpec") {
            for a in b.attributes() {
                bodywork.insert(a.name().to_string(), a.value().to_string());
            }
        }
        let (kart_name_string_id, kart_upgrade_level) = match child(&root, "Kart") {
            Some(k) => (
                k.attribute("m_sNameStringID").unwrap_or("").to_string(),
                // `atoi` in GetAIKart
                k.attribute("m_iUpgradeLevel").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0),
            ),
            None => (String::new(), 0),
        };
        let pilot_name = child(&root, "Pilot").and_then(|p| p.attribute("Name")).unwrap_or("").to_string();

        // CCar::LoadBossAbilities @001b0d60: start from the CRaceAI defaults, override only when non-zero.
        let (raw_max_range, raw_min_cooldown, raw_max_cooldown) = match child(&root, "AbilityAttributes") {
            Some(a) => (attr_f32(&a, "MaxRange"), attr_f32(&a, "MinCooldown"), attr_f32(&a, "MaxCooldown")),
            None => (0.0, 0.0, 0.0),
        };
        let max_range = if raw_max_range != 0.0 { raw_max_range } else { DEFAULT_AI_MAX_RANGE };
        let min_cooldown = if raw_min_cooldown != 0.0 { raw_min_cooldown } else { DEFAULT_AI_MIN_COOLDOWN };
        let max_cooldown = if raw_max_cooldown != 0.0 { raw_max_cooldown } else { DEFAULT_AI_MAX_COOLDOWN };

        let mut abilities = Vec::new();
        for a in root.children().filter(|c| c.is_element() && c.tag_name().name() == "Ability") {
            let type_string = a.attribute("type").unwrap_or("").to_string();
            let kind = AbilityType::from_type_string(&type_string);
            let mut params = Vec::new();
            for c in a.children().filter(|c| c.is_element()) {
                params.push((c.tag_name().name().to_string(), c.text().unwrap_or("").trim().to_string()));
            }
            let f = |k: &str| -> f32 {
                params.iter().find(|(kk, _)| kk == k).and_then(|(_, v)| v.parse::<f32>().ok()).unwrap_or(0.0)
            };
            let (mn, mx) = (f("MinDistance"), f("MaxDistance"));
            abilities.push(BossAbilityDef {
                name: a.attribute("name").unwrap_or("").to_string(),
                type_string,
                kind,
                min_cooldown: f("MinCooldown"),
                max_cooldown: f("MaxCooldown"),
                min_dist_sq: mn * mn,
                max_dist_sq: mx * mx,
                release_time: f("ReleaseTime"),
                params,
            });
        }
        Ok(BossDef {
            id,
            name,
            bodywork,
            kart_name_string_id,
            kart_upgrade_level,
            pilot_name,
            max_range,
            min_cooldown,
            max_cooldown,
            raw_max_range,
            raw_min_cooldown,
            raw_max_cooldown,
            abilities,
        })
    }

    /// Path of `boss_NNN.xml` below the asset root (`assets292`).
    pub fn path(assets_root: &Path, id: u32) -> PathBuf {
        assets_root.join("xml").join("characters").join("charxml").join(format!("boss_{id:03}.xml"))
    }

    pub fn load(assets_root: &Path, id: u32) -> Result<BossDef, String> {
        let p = BossDef::path(assets_root, id);
        let xml = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        BossDef::from_xml(id, &xml)
    }

    /// Ids of every `boss_NNN.xml` present (the shipped data has 33).
    pub fn list_ids(assets_root: &Path) -> Vec<u32> {
        let dir = assets_root.join("xml").join("characters").join("charxml");
        let mut ids = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if let Some(num) = n.strip_prefix("boss_").and_then(|r| r.strip_suffix(".xml")) {
                    if let Ok(v) = num.parse::<u32>() {
                        ids.push(v);
                    }
                }
            }
        }
        ids.sort_unstable();
        ids
    }

    pub fn load_all(assets_root: &Path) -> Vec<BossDef> {
        BossDef::list_ids(assets_root).into_iter().filter_map(|i| BossDef::load(assets_root, i).ok()).collect()
    }

    /// `CBaseAbility::GetBossAbilityCount` @001bb57c.
    pub fn ability_count(&self) -> usize {
        self.abilities.len()
    }

    /// `CBaseAbility::GetBossAbilityType(id, index)` @001bb664 (0 when out of range / unknown).
    pub fn ability_type(&self, index: usize) -> AbilityType {
        self.abilities.get(index).map(|a| a.kind).unwrap_or(AbilityType::Unknown)
    }

    /// `CGameModeBossBattle::GetAICharacter` @0017dd60 (FruitRush @0017e61c): index of the character whose name
    /// equals `<Pilot Name>`, scanning the 16 character names and keeping the **last** match. `None` = the original
    /// falls back to `CGameMode::GetAICharacter` (@0017c75c, random non-duplicate character): the caller decides.
    pub fn ai_character_index<S: AsRef<str>>(&self, names: &[S]) -> Option<usize> {
        let mut found = None;
        for (i, n) in names.iter().take(16).enumerate() {
            if n.as_ref() == self.pilot_name {
                found = Some(i);
            }
        }
        found
    }

    /// `CGameModeBossBattle::GetAIKart` @0017dee0: index into the kart table whose (`m_sNameStringID`, upgrade
    /// level) equal this boss' `<Kart>`; -1 when none (the original also returns 0 when the file cannot be read).
    pub fn ai_kart_index<S: AsRef<str>>(&self, karts: &[(S, i32)]) -> i32 {
        for (i, (n, lvl)) in karts.iter().enumerate() {
            if *lvl == self.kart_upgrade_level && n.as_ref() == self.kart_name_string_id {
                return i as i32;
            }
        }
        -1
    }
}

/// Names of `char_001..char_016.xml` in index order (`CCharacterManager::GetCharacterName`), for
/// [`BossDef::ai_character_index`].
pub fn load_character_names(assets_root: &Path) -> Vec<String> {
    let mut v = Vec::new();
    for i in 1..=16 {
        let p = assets_root.join("xml").join("characters").join("charxml").join(format!("char_{i:03}.xml"));
        let name = std::fs::read_to_string(&p)
            .ok()
            .and_then(|x| {
                let d = Document::parse(&x).ok()?;
                let r = d.root_element();
                child(&r, "Name").and_then(|n| n.text()).map(|s| s.to_string())
            })
            .unwrap_or_default();
        v.push(name);
    }
    v
}

// ------------------------------------------------------------------------------------------------------------
// Event definition subset
// ------------------------------------------------------------------------------------------------------------

/// The parts of an `eventdef_*.xml` with `<GameMode name="BOSS_BATTLE"/>` that this mode needs (the full parser is
/// `CEventDefinitionManager::ReadEventDefinition` @~00101xxx, owned by `eventdef.rs`).
#[derive(Clone, Debug, Default)]
pub struct BossEventDef {
    pub title: String,
    pub environment_path: String,
    pub environment_config: i32,
    pub character: String,
    /// "BOSS_BATTLE" or "BOSS_FRUIT_RUSH".
    pub game_mode: String,
    /// `<Difficulty level=>`.
    pub difficulty_level: f32,
    /// `<Difficulty bosslevel=>` = the `boss_NNN.xml` id; the parser stores 1 first (`this+0x264 = 1`) and only
    /// overwrites it when the attribute exists.
    pub bosslevel: u32,
    pub level_index: Option<i32>,
    pub ai_upgrade: Option<i32>,
    /// `<Stars Star1 Star2 Star3>` (parser @~00101f00: `Star2 = max(Star2, Star1)`, `Star3 = max(Star3, Star2)`;
    /// all three attributes missing = left as is). `None` when the element is absent (5 of the 39 files).
    /// UNRESOLVED: the default stored in the definition before parsing (missing `<Stars>`) is not read here.
    pub stars: Option<[i32; 3]>,
    pub splines: Vec<(String, f32, f32)>,
    pub track_items: usize,
    /// `<Difficulty seed_rush_amount=>`, `<Difficulty boss_bonus=>`: present in the 2.9.2 data, no matching string
    /// in the 2.9.1 decompile (UNRESOLVED: unread by the shipped 2.9.1 code).
    pub seed_rush_amount: Option<String>,
    pub boss_bonus: Option<String>,
}

impl BossEventDef {
    pub fn from_xml(xml: &str) -> Result<BossEventDef, String> {
        let doc = Document::parse(xml).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        let mut e = BossEventDef { bosslevel: 1, ..Default::default() };
        if let Some(n) = child(&root, "Name") {
            e.title = n.attribute("title").unwrap_or("").to_string();
        }
        if let Some(n) = child(&root, "Environment") {
            e.environment_path = n.attribute("pathname").unwrap_or("").to_string();
            // `config` default 1 (decomp: `*(param_2+0x5c) = 1` when absent)
            e.environment_config = n.attribute("config").and_then(|v| v.trim().parse().ok()).unwrap_or(1);
        }
        if let Some(n) = child(&root, "Character") {
            e.character = n.attribute("name").unwrap_or("").to_string();
        }
        if let Some(n) = child(&root, "GameMode") {
            e.game_mode = n.attribute("name").unwrap_or("").to_string();
        }
        if let Some(n) = child(&root, "Difficulty") {
            e.difficulty_level = n.attribute("level").and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
            if let Some(v) = n.attribute("bosslevel") {
                e.bosslevel = v.trim().parse::<i64>().unwrap_or(0) as u32; // atoi
            }
            e.level_index = n.attribute("level_index").and_then(|v| v.trim().parse().ok());
            e.ai_upgrade = n.attribute("ai_upgrade").and_then(|v| v.trim().parse().ok());
            e.seed_rush_amount = n.attribute("seed_rush_amount").map(|s| s.to_string());
            e.boss_bonus = n.attribute("boss_bonus").map(|s| s.to_string());
        }
        if let Some(n) = child(&root, "Stars") {
            let g = |k: &str| n.attribute(k).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
            let (s1, mut s2, mut s3) = (g("Star1"), g("Star2"), g("Star3"));
            if !(s1 == 0 && s2 == 0 && s3 == 0) {
                if !(s1 == 0 && s2 == 0) && s2 < s1 {
                    s2 = s1;
                }
                if s3 < s2 {
                    s3 = s2;
                }
            }
            e.stars = Some([s1, s2, s3]);
        }
        for n in root.children().filter(|c| c.is_element()) {
            match n.tag_name().name() {
                "Spline" => e.splines.push((
                    n.attribute("name").unwrap_or("").to_string(),
                    n.attribute("min_ai_weighting").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                    n.attribute("max_ai_weighting").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                )),
                "TrackItem" => e.track_items += 1,
                _ => {}
            }
        }
        Ok(e)
    }

    pub fn is_boss_mode(&self) -> bool {
        self.game_mode == "BOSS_BATTLE" || self.game_mode == "BOSS_FRUIT_RUSH"
    }

    pub fn mode_kind(&self) -> GameModeKind {
        if self.game_mode == "BOSS_FRUIT_RUSH" {
            GameModeKind::BossFruitRush
        } else {
            GameModeKind::BossBattle
        }
    }
}

/// Every eventdef under `xml/gameplay/eventdef_*` whose `<GameMode>` is BOSS_BATTLE / BOSS_FRUIT_RUSH.
pub fn find_boss_eventdefs(assets_root: &Path) -> Vec<(PathBuf, BossEventDef)> {
    let mut out = Vec::new();
    let base = assets_root.join("xml").join("gameplay");
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(&base) {
        Ok(rd) => rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => return out,
    };
    dirs.sort();
    for d in dirs {
        let is_ev = d.file_name().map(|n| n.to_string_lossy().starts_with("eventdef_")).unwrap_or(false);
        if !is_ev {
            continue;
        }
        let mut files: Vec<PathBuf> =
            std::fs::read_dir(&d).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        for f in files {
            if f.extension().map(|x| x == "xml").unwrap_or(false) {
                if let Ok(xml) = std::fs::read_to_string(&f) {
                    if xml.contains("BOSS_BATTLE") || xml.contains("BOSS_FRUIT_RUSH") {
                        if let Ok(ev) = BossEventDef::from_xml(&xml) {
                            if ev.is_boss_mode() {
                                out.push((f, ev));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------
// Data-driven pieces from other files
// ------------------------------------------------------------------------------------------------------------

/// `gameplay/misc/debugtweakables.xml` `<BossBattle>` values the boss AI reads: `CDebugManager::GetDebugFloat`
/// slots 0x44 (`Ability_Distance`) and 0x45 (`Initial_Ability_Delay`), verified from the loader @~002c7560 and the
/// slot base (DAT_002c6754 + 0x2c674c = 0xde0af8, slot = (addr - base) / 4). `Ability_Delay` and `Crown_Swap_Delay`
/// exist in the 2.9.2 xml but are not read by the 2.9.1 loader. Ram_* (0x3f..0x43) belong to `CRaceAI::UpdateRamAI`
/// (@0018ab24, raceai.rs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BossAiTweaks {
    /// Slot 0x44. `< 0` means "use the boss file's MaxRange" (`UpdateAbilityAI`: `if (v < 0) v = this+0x1d0`).
    pub ability_distance: f32,
    /// Slot 0x45: value both ability timers start from (`CRaceAI::Reset` @001874d0: `+0x1c4 = +0x1d4 = slot 0x45`).
    pub initial_ability_delay: f32,
}

impl Default for BossAiTweaks {
    /// Shipped values (`debugtweakables.xml`: Ability_Distance -1.0, Initial_Ability_Delay 4.0).
    fn default() -> Self {
        BossAiTweaks { ability_distance: -1.0, initial_ability_delay: 4.0 }
    }
}

impl BossAiTweaks {
    pub fn from_xml(xml: &str) -> Option<BossAiTweaks> {
        let doc = Document::parse(xml).ok()?;
        let root = doc.root_element();
        let bb = child(&root, "BossBattle")?;
        let f = |k: &str| child(&bb, k).and_then(|n| n.text()).and_then(|t| t.trim().parse::<f32>().ok());
        Some(BossAiTweaks { ability_distance: f("Ability_Distance")?, initial_ability_delay: f("Initial_Ability_Delay")? })
    }
}

/// `characterlevelling.xml` `<Rewards><BossBattle><Reward position= xp=/>`: XP by finishing position
/// (`ReadCharacterLevellingDataFromXML` @0013194c stores `xp` at `this[(position + 0x81) * 4]`; unlisted positions
/// stay at the manager's zero initial value, UNRESOLVED: initialiser not read).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BossXpRewards {
    pub by_position: Vec<(u32, u32)>,
}

impl BossXpRewards {
    pub fn from_characterlevelling_xml(xml: &str) -> Option<BossXpRewards> {
        // The file has several top-level elements (`<Levels>`, `<Rewards>`): the game's reader wraps documents in
        // one throw-away element (see PORT_STATUS "Update 2"), so do the same.
        let wrapped = format!("<root>{}</root>", xml.trim_start_matches('\u{feff}'));
        let doc = Document::parse(&wrapped).ok()?;
        let rewards = child(&doc.root_element(), "Rewards")?;
        let bb = child(&rewards, "BossBattle")?;
        let mut v = Vec::new();
        for r in bb.children().filter(|c| c.is_element() && c.tag_name().name() == "Reward") {
            let p = r.attribute("position")?.trim().parse().ok()?;
            let x = r.attribute("xp")?.trim().parse().ok()?;
            v.push((p, x));
        }
        Some(BossXpRewards { by_position: v })
    }
    pub fn xp_for_position(&self, position: u32) -> u32 {
        self.by_position.iter().find(|(p, _)| *p == position).map(|(_, x)| *x).unwrap_or(0)
    }
}

/// Intro passive message (`CPassiveMsgBossIntro::LayoutScreen` @00378958): localisation key by the number of boss
/// stages already completed (`TEventState::GetStagesCompleted`). Case 2 loads its string through
/// `DAT_00378d28` (UNRESOLVED: the key is not printed in the listing; the 2.9.1 localisation has
/// `BOSS_RACE_INTRO_ONCE = "Final race against ...!"`, which is what it must be).
pub fn intro_message_key(stages_completed: i32) -> &'static str {
    match stages_completed {
        1 => "BOSS_RACE_INTRO_TWICE",
        2 => "BOSS_RACE_INTRO_ONCE", // UNRESOLVED: inferred, see doc comment
        0 => "BOSS_RACE_INTRO_THREE",
        _ => "BOSS_RACE_INTRO_BOSS_UNLOCKED",
    }
}

/// Number of enabled tick boxes in that popup (`LayoutScreen`: 1 -> box 1; 2 -> boxes 1,2; other non-zero -> 1,2,3;
/// 0 -> none).
pub fn intro_tick_boxes(stages_completed: i32) -> u32 {
    match stages_completed {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 3,
    }
}

// ------------------------------------------------------------------------------------------------------------
// Boss AI ability scheduling
// ------------------------------------------------------------------------------------------------------------

/// What the scheduler asks the car to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossTrigger {
    /// `CCar::TriggerAbility` @0019fa48: the pilot's own bird ability (`char_NNN.xml`, `CCar+0x1c2c`).
    PilotAbility,
    /// `CCar::TriggerBossAbility(i)` @0019fb74: the `<Ability>` weapon of the boss file.
    BossAbility(usize),
}

/// Simple xorshift32 stand-in for the game RNG (`CXGSRandom`, vtable slots 0x10/0x18/0x1c). UNRESOLVED: the
/// original generator algorithm is not ported; the host should call [`BossAi::update`] with its own RNG.
#[derive(Clone, Debug)]
pub struct Xorshift32(pub u32);
impl Xorshift32 {
    pub fn next01(&mut self) -> f32 {
        let mut x = if self.0 == 0 { 0x9e3779b9 } else { self.0 };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Per-boss-car AI ability timers. `CRaceAI` fields: `+0x1a8` is-boss, `+0x1c4` pilot-ability timer, `+0x1d4`
/// boss-ability timer, `+0x1c8/0x1cc` cooldown range, `+0x1d0` max range.
#[derive(Clone, Debug)]
pub struct BossAi {
    pub def: BossDef,
    pub tweaks: BossAiTweaks,
    /// `CRaceAI+0x1c4`.
    pub pilot_timer: f32,
    /// `CRaceAI+0x1d4`.
    pub boss_timer: f32,
    /// Cooldown extra per ability (see [`BossAbilityDef::cooldown`]); 0 until the first trigger.
    extra: [f32; MAX_BOSS_ABILITIES],
    /// Host supplied: largest pending Moustache rocket launch delay (`CMoustacheBossAbility+0x2c4..0x2fc`).
    pub pending_rocket_delay: f32,
}

impl BossAi {
    /// `CRaceAI::Reset` @001874d0 (timers = DebugFloat 0x45) + `SetAsBoss` @0018c92c / `LoadBossAbilities`.
    pub fn new(def: BossDef, tweaks: BossAiTweaks) -> BossAi {
        BossAi {
            pilot_timer: tweaks.initial_ability_delay,
            boss_timer: tweaks.initial_ability_delay,
            extra: [0.0; MAX_BOSS_ABILITIES],
            pending_rocket_delay: 0.0,
            def,
            tweaks,
        }
    }

    /// Range actually used for the pilot ability: `UpdateAbilityAI` reads DebugFloat 0x44 and, if negative,
    /// `this+0x1d0` (boss MaxRange).
    pub fn effective_range(&self) -> f32 {
        if self.tweaks.ability_distance < 0.0 {
            self.def.max_range
        } else {
            self.tweaks.ability_distance
        }
    }

    /// `CRaceAI::UpdateAbilityAI(float)` boss branch (@00187164, called from @0018a58c when `this+0x1a8 != 0`).
    /// * `dist_sq`: squared distance between the boss and the **local player** car (chassis positions).
    /// * `delta_s`: boss spline position minus player spline position (`(node+0x34) + frac * (node+0x28)` each).
    /// * `rng01`: game RNG in [0, 1) (`Float(min,max)` default = `min + r * (max - min)`).
    /// * `abilities_enabled`: the gate read in `TriggerBossAbility` (`CGame+0x1c -> +0x20c -> +8 != 0`; UNRESOLVED
    ///   meaning, pass true).
    pub fn update(&mut self, dt: f32, dist_sq: f32, delta_s: f32, rng01: &mut dyn FnMut() -> f32, abilities_enabled: bool) -> Vec<BossTrigger> {
        let mut out = Vec::new();
        let range = self.effective_range();
        // pilot ability timer
        let t = self.pilot_timer - dt;
        self.pilot_timer = t;
        if t < 0.0 && dist_sq < range * range {
            out.push(BossTrigger::PilotAbility);
            let r = rng01();
            self.pilot_timer = self.def.min_cooldown + r * (self.def.max_cooldown - self.def.min_cooldown);
        }
        // boss ability timer
        let t2 = self.boss_timer - dt;
        self.boss_timer = t2;
        if t2 < 0.0 {
            for i in 0..self.def.abilities.len().min(MAX_BOSS_ABILITIES) {
                let a = &self.def.abilities[i];
                if !a.creates_ability() {
                    continue; // CreateBossAbility returned null -> GetBossAbility(i) == 0
                }
                if a.can_trigger_at_distance(dist_sq, delta_s) {
                    // TriggerBossAbility: CanTriggerAbility (+0x70, ability module) && abilities_enabled
                    if abilities_enabled {
                        out.push(BossTrigger::BossAbility(i));
                    }
                    let r = rng01();
                    self.extra[i] = a.extra_after_trigger(self.pending_rocket_delay);
                    self.boss_timer = a.cooldown(r, self.extra[i]);
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------------------------
// Per-car game mode data (CGameModeData)
// ------------------------------------------------------------------------------------------------------------

/// `CGameModeData+4`: 1 = racing, 2 = completed (event won), 3 = failed (`ProcessUpdate` @0017caac).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataState {
    Running = 1,
    Completed = 2,
    Failed = 3,
}

/// `CGameModeData` (Reset @0017c3a8 / ctor @0017ca38).
#[derive(Clone, Debug)]
pub struct CarModeData {
    pub state: DataState,
    /// +8: seconds raced (`Update` @0017c450 adds dt while state == 1 and `car+0x1b7c > 0`).
    pub time: f32,
    /// +0xc: position stored when the car finishes.
    pub final_position: i32,
    /// +0x10: current race position (`car+0x1a9c`).
    pub position: i32,
    /// +0x14: best (lowest) position seen, starts at 1000.
    pub best_position: i32,
    /// +0x18: bonus coins (`GetBonusCoins` is the base 0 for both boss modes).
    pub bonus_coins: i32,
    /// `CGameModeSeedRushData` (+0x20 / +0x24): fruit splatted / fruit target (BossFruitRush only).
    pub fruit_collected: u32,
    pub fruit_target: u32,
}

impl Default for CarModeData {
    fn default() -> Self {
        CarModeData {
            state: DataState::Running,
            time: 0.0,
            final_position: 0,
            position: 0,
            best_position: INITIAL_BEST_POSITION,
            bonus_coins: 0,
            fruit_collected: 0,
            fruit_target: 0,
        }
    }
}

impl CarModeData {
    /// `CGameModeSeedRushData::Update` @0017eec0: `+0x28 = collected / target` (float division).
    pub fn fruit_fraction(&self) -> f32 {
        self.fruit_collected as f32 / self.fruit_target as f32
    }
}

// ------------------------------------------------------------------------------------------------------------
// The mode
// ------------------------------------------------------------------------------------------------------------

/// `CGameModeBossBattle` (mode 0xb) / `CGameModeBossFruitRush` (mode 0xc).
pub struct BossBattle {
    kind: GameModeKind,
    pub boss: BossDef,
    pub event: BossEventDef,
    pub tweaks: BossAiTweaks,
    cars: BTreeMap<CarId, CarModeData>,
    ai: BTreeMap<CarId, BossAi>,
    /// Latest `car+0x4a0` readings fed by the host (default -1.0 = never detached).
    detach_timer: BTreeMap<CarId, f32>,
    /// `this+0x24`: the Boss Destroyed bonus was already awarded.
    bonus_awarded: bool,
    /// `CScoreCounterBonus+0x3f4`.
    bonus_counter: f32,
    /// Sum of every other `CScoreSystem` counter, supplied by the host (`ModeInput::Other { tag: "BaseScore" }`).
    base_score: i64,
    fruit_target: u32,
    initialised: bool,
    state: ModeState,
    rng: Xorshift32,
    pub abilities_enabled: bool,
}

impl BossBattle {
    pub fn new(boss: BossDef, event: BossEventDef) -> BossBattle {
        BossBattle {
            kind: event.mode_kind(),
            boss,
            event,
            tweaks: BossAiTweaks::default(),
            cars: BTreeMap::new(),
            ai: BTreeMap::new(),
            detach_timer: BTreeMap::new(),
            bonus_awarded: false,
            bonus_counter: 0.0,
            base_score: 0,
            fruit_target: 0,
            initialised: false,
            state: ModeState::Running,
            rng: Xorshift32(0x1234_5678),
            abilities_enabled: true,
        }
    }

    /// Convenience: load the boss file named by the eventdef's `bosslevel` from the asset root.
    pub fn from_eventdef(assets_root: &Path, event: BossEventDef) -> Result<BossBattle, String> {
        let boss = BossDef::load(assets_root, event.bosslevel)?;
        let mut b = BossBattle::new(boss, event);
        if let Ok(x) = std::fs::read_to_string(assets_root.join("xml/gameplay/misc/debugtweakables.xml")) {
            if let Some(t) = BossAiTweaks::from_xml(&x) {
                b.tweaks = t;
            }
        }
        Ok(b)
    }

    pub fn set_rng_seed(&mut self, seed: u32) {
        self.rng = Xorshift32(seed);
    }

    /// Fruit target per car (BossFruitRush; `CEventDefinitionManager::GetTokenThreshold(difficulty)`).
    pub fn set_fruit_target(&mut self, target: u32) {
        self.fruit_target = target;
        for d in self.cars.values_mut() {
            d.fruit_target = target;
        }
    }

    /// `InitialiseCarData` @0017dbe8 (FruitRush @0017e4c4): one `CGameModeData` per car (FruitRush: a
    /// `CGameModeSeedRushData`), `SetAsBoss(bosslevel)` on every AI car, then `CGame+0x3244 = 5.0`.
    pub fn initialise_car_data(&mut self, karts: &[KartState], effects: &mut Vec<CarEffect>) {
        self.cars.clear();
        self.ai.clear();
        for k in karts {
            let mut d = CarModeData::default();
            d.fruit_target = self.fruit_target;
            self.cars.insert(k.id, d);
            if !k.is_player {
                // CRaceAI::SetAsBoss @0018c92c: this+0x1a8 = 1; CCar::LoadBossAbilities(bosslevel, ...)
                self.ai.insert(k.id, BossAi::new(self.boss.clone(), self.tweaks));
                effects.push(CarEffect::Other {
                    tag: "SetAsBoss",
                    car: Some(k.id),
                    vals: [self.event.bosslevel as f32, self.boss.ability_count() as f32, 0.0, 0.0],
                    pos: Vec3::ZERO,
                });
            }
        }
        effects.push(CarEffect::Other {
            tag: "GameField3244", // UNRESOLVED meaning of CGame+0x3244
            car: None,
            vals: [GAME_FIELD_3244_VALUE, 0.0, 0.0, 0.0],
            pos: Vec3::ZERO,
        });
        self.initialised = true;
    }

    pub fn car_data(&self, car: CarId) -> Option<&CarModeData> {
        self.cars.get(&car)
    }

    pub fn boss_ai(&self, car: CarId) -> Option<&BossAi> {
        self.ai.get(&car)
    }

    pub fn bonus_awarded(&self) -> bool {
        self.bonus_awarded
    }

    /// `GetEventCompleted` (BossBattle @0017d9ec: position == 1; FruitRush @0017e210: position == 1 and
    /// `fruit fraction >= 1.0`).
    pub fn event_completed(&self, d: &CarModeData) -> bool {
        if d.position != 1 {
            return false;
        }
        match self.kind {
            GameModeKind::BossFruitRush => 1.0 <= d.fruit_fraction(),
            _ => true,
        }
    }

    /// `GetBonusCoins` (base @0017c37c, not overridden): 0.
    pub fn bonus_coins(&self) -> i32 {
        0
    }

    fn local_player(&self) -> Option<CarId> {
        // lowest-id human; the host normally has exactly one
        self.cars.keys().copied().find(|id| !self.ai.contains_key(id))
    }

    /// `CheckGameOverCondition` @0017d860 (FruitRush @0017e084): true when every human car is done. The original
    /// per-car test is `(car+0x4d4 == 0 && (car+0x1b60 != 0 || state == 1)) ? !running(car+0x1ae8) : true`;
    /// UNRESOLVED fields +0x4d4 / +0x1b60 (treated as 0): a car counts as done once its data state left 1. With no
    /// human players the original returns true immediately.
    pub fn check_game_over(&self) -> bool {
        let mut humans = self.cars.iter().filter(|(id, _)| !self.ai.contains_key(id)).peekable();
        if humans.peek().is_none() {
            return true;
        }
        humans.all(|(_, d)| d.state != DataState::Running)
    }

    /// `CEventDefinitionManager::GetStarsFromScore` @000fee38: 0 while the local player's data state is 1, then
    /// `1 + (score > Star2) + (score > Star3)` (strict compares; Star1 never raises the result; a finished
    /// race therefore always yields at least 1).
    pub fn stars_from_score(&self, score: i64) -> u8 {
        let Some(p) = self.local_player() else { return 0 };
        if self.cars[&p].state == DataState::Running {
            return 0;
        }
        let [_s1, s2, s3] = self.event.stars.unwrap_or([0, 0, 0]);
        if (s3 as i64) < score {
            3
        } else if (s2 as i64) < score {
            2
        } else {
            1
        }
    }

    /// `CScoreSystem::GetScore` @001f4288 = sum of all counters: host supplied sum + the bonus counter.
    pub fn total_score(&self) -> i64 {
        self.base_score + self.bonus_counter as i64
    }
}

impl GameModeRules for BossBattle {
    fn kind(&self) -> GameModeKind {
        self.kind
    }

    /// `CGameModeBossBattle::Update` @0017db10 (FruitRush @0017e2b4) then `CGameMode::Update` ->
    /// `ProcessUpdate` @0017caac (per-car data update) plus the boss AI ability scheduling.
    fn update(&mut self, dt: f32, karts: &[KartState], effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>) {
        if !self.initialised {
            self.initialise_car_data(karts, effects);
        }

        // Boss Destroyed bonus: any AI car whose `+0x4a0` reads exactly 0.0 (once; this+0x24).
        if !self.bonus_awarded {
            let hit = karts.iter().any(|k| {
                !k.is_player && self.detach_timer.get(&k.id).copied().unwrap_or(-1.0) == 0.0
            });
            if hit {
                self.bonus_awarded = true;
                self.bonus_counter += BOSS_DESTROYED_BONUS; // CScoreCounterBonus::AddScore (counter 6)
                if let Some(p) = self.local_player() {
                    events.push(ModeEvent::ScoreChanged { car: p, score: self.total_score() });
                }
            }
        }

        // CGameModeData::Update for every racing car.
        for k in karts {
            if let Some(d) = self.cars.get_mut(&k.id) {
                if d.state != DataState::Running {
                    continue;
                }
                d.position = k.race_position as i32;
                if d.position < d.best_position {
                    d.best_position = d.position;
                }
                if k.race_time > 0.0 {
                    d.time += dt; // `0.0 < car+0x1b7c`
                }
                if self.kind == GameModeKind::BossFruitRush {
                    d.fruit_target = self.fruit_target;
                }
            }
        }

        // Boss AI (CRaceAI::UpdateAbilityAI boss branch) against the local player.
        if let Some(pid) = self.local_player() {
            if let Some(player) = karts.iter().find(|k| k.id == pid) {
                let rng = &mut self.rng;
                let mut draw = || rng.next01();
                for k in karts.iter().filter(|k| !k.is_player) {
                    let Some(ai) = self.ai.get_mut(&k.id) else { continue };
                    let d = k.pos - player.pos;
                    let dist_sq = d.x * d.x + d.y * d.y + d.z * d.z;
                    let delta_s = k.spline_distance - player.spline_distance;
                    for t in ai.update(dt, dist_sq, delta_s, &mut draw, self.abilities_enabled) {
                        match t {
                            BossTrigger::PilotAbility => effects.push(CarEffect::Other {
                                tag: "TriggerAbility",
                                car: Some(k.id),
                                vals: [0.0; 4],
                                pos: Vec3::ZERO,
                            }),
                            BossTrigger::BossAbility(i) => {
                                effects.push(CarEffect::Other {
                                    tag: "TriggerBossAbility",
                                    car: Some(k.id),
                                    vals: [i as f32, ai.def.abilities[i].kind as u32 as f32, 0.0, 0.0],
                                    pos: Vec3::ZERO,
                                });
                                if plays_battle_puff(k.character) {
                                    effects.push(CarEffect::Sound { name: "ABY_battle_puff".to_string(), car: Some(k.id) });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Mode inputs. `ProjectileHit`, `Smashed`, `CarHitCar`: no-ops (the BossBattle vtable keeps the base
    /// `OnWeaponFire` @0017c38c / `OnCarCollision` @0017c390 and the mode has no health). `KartFinished` runs the
    /// finish branch of `ProcessUpdate` @0017caac. `SeedCollected` feeds the BossFruitRush fruit counter
    /// (`CGameModeSeedRushData+0x20`).
    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        match input {
            ModeInput::KartFinished { car, .. } => {
                let completed = match self.cars.get(car) {
                    Some(d) if d.state == DataState::Running => self.event_completed(d),
                    _ => return,
                };
                let is_ai = self.ai.contains_key(car);
                if let Some(d) = self.cars.get_mut(car) {
                    d.state = if completed { DataState::Completed } else { DataState::Failed };
                    d.final_position = d.position; // piVar13[3] = car+0x1a9c
                }
                if !is_ai && self.check_game_over() {
                    let p = self.local_player().unwrap_or(*car);
                    let won = self.cars[&p].state == DataState::Completed;
                    self.state = if won { ModeState::Won } else { ModeState::Lost };
                    let score = self.total_score();
                    let stars = self.stars();
                    events.push(ModeEvent::Finished { won, score, stars });
                }
            }
            ModeInput::SeedCollected { car, count } => {
                if self.kind == GameModeKind::BossFruitRush && !self.ai.contains_key(car) {
                    if let Some(d) = self.cars.get_mut(car) {
                        d.fruit_collected += *count;
                    }
                }
            }
            ModeInput::Other { tag, car, value } => match *tag {
                // `car+0x4a0` of an AI car (pilot detach countdown latch).
                "PilotDetachTimer" => {
                    if let Some(c) = car {
                        self.detach_timer.insert(*c, *value);
                    }
                }
                // Sum of the other score counters (CScoreSystem counters except the bonus counter).
                "BaseScore" => self.base_score = *value as i64,
                _ => {}
            },
            ModeInput::Smashed { .. } | ModeInput::CarHitCar { .. } | ModeInput::ProjectileHit { .. } => {}
            ModeInput::GatePassed { .. } => {}
        }
    }

    fn state(&self) -> ModeState {
        self.state
    }

    fn score(&self) -> i64 {
        self.total_score()
    }

    /// Stars credited: `AddCurrentEventStarCompletion` is only called for state 2 (won) in `ProcessUpdate`, so a
    /// loss credits none; a win gets `GetStarsFromScore(score)`.
    fn stars(&self) -> u8 {
        if self.state == ModeState::Won {
            self.stars_from_score(self.total_score())
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

    fn root() -> Option<PathBuf> {
        let p = PathBuf::from(ASSETS292);
        if p.join("xml").join("characters").join("charxml").join("boss_001.xml").exists() {
            Some(p)
        } else {
            None
        }
    }

    fn kart(id: usize, player: bool, pos: u32, p: Vec3, spline: f32) -> KartState {
        KartState { id, is_player: player, race_position: pos, pos: p, spline_distance: spline, race_time: 1.0, ..Default::default() }
    }

    fn bb(stars: Option<[i32; 3]>, kind: &str) -> BossBattle {
        let boss = BossDef::from_xml(
            1,
            r#"<Character><Name>Boss</Name><Kart m_sNameStringID="KART_BLACK" m_iUpgradeLevel="3"/><Pilot Name="Black"/>
<AbilityAttributes MaxRange="8.0" MinCooldown="25.0" MaxCooldown="34.0"/>
<Ability name="Close" type="ObjectSpawn"><ObjectType>bombs_bomb</ObjectType><MinCooldown>2.5</MinCooldown><MaxCooldown>3.0</MaxCooldown>
<MinDistance>1.0</MinDistance><MaxDistance>55.0</MaxDistance><ReleaseTime>1.475</ReleaseTime></Ability></Character>"#,
        )
        .unwrap();
        let mut ev = BossEventDef { bosslevel: 1, game_mode: kind.to_string(), stars, ..Default::default() };
        ev.stars = stars;
        BossBattle::new(boss, ev)
    }

    #[test]
    fn ability_type_mapping() {
        use AbilityType::*;
        assert_eq!(AbilityType::from_type_string("ObjectSpawn"), ObjectSpawn);
        assert_eq!(AbilityType::from_type_string("StellaBossAbility"), StellaBossAbility);
        assert_eq!(AbilityType::from_type_string("ChuckBossAbility") as u32, 0x14);
        assert_eq!(AbilityType::from_type_string("KingPigBossAbility") as u32, 0x15);
        assert_eq!(AbilityType::from_type_string("nonsense"), Unknown);
        assert!(!RedSpeedBoost.creates_ability()); // CreateBossAbility case 2 -> null
        assert!(!MoustacheAbility.creates_ability());
        assert!(MoustacheBossAbility.creates_ability());
        assert_eq!(OvertakeSpeedBoost.class_name(), Some("COvertakeSpeedAbility"));
    }

    #[test]
    fn can_trigger_and_cooldown() {
        let b = bb(None, "BOSS_BATTLE");
        let a = &b.boss.abilities[0];
        // 1..55 m, boss ahead
        assert!(a.can_trigger_at_distance(10.0 * 10.0, 5.0));
        assert!(!a.can_trigger_at_distance(10.0 * 10.0, -5.0)); // boss behind
        assert!(!a.can_trigger_at_distance(0.5 * 0.5, 5.0)); // too close
        assert!(!a.can_trigger_at_distance(60.0 * 60.0, 5.0)); // too far
        assert!((a.cooldown(0.0, 0.0) - 2.5).abs() < 1e-6);
        assert!((a.cooldown(1.0, 1.475) - 4.475).abs() < 1e-5);
    }

    #[test]
    fn scheduler_timers() {
        let mut b = bb(None, "BOSS_BATTLE");
        let def = b.boss.clone();
        assert_eq!(def.max_range, 8.0);
        let mut ai = BossAi::new(def, BossAiTweaks::default());
        let mut r = || 0.5f32;
        // before the 4.0 s initial delay nothing fires
        assert!(ai.update(3.9, 4.0, 3.0, &mut r, true).is_empty());
        // pilot ability: timer below 0 and inside MaxRange (8) -> fires and re-arms with lerp(25,34,.5)
        let t = ai.update(0.2, 4.0, 3.0, &mut r, true);
        assert!(t.contains(&BossTrigger::PilotAbility));
        assert!(t.contains(&BossTrigger::BossAbility(0)));
        assert!((ai.pilot_timer - 29.5).abs() < 1e-4);
        // boss timer = lerp(2.5,3.0,.5) + ReleaseTime
        assert!((ai.boss_timer - (2.75 + 1.475)).abs() < 1e-4);
        // outside MaxRange the pilot ability does not fire
        let mut ai2 = BossAi::new(b.boss.clone(), BossAiTweaks::default());
        let t2 = ai2.update(5.0, 9.0 * 9.0, -1.0, &mut r, true);
        assert!(t2.is_empty());
        // DebugFloat(0x44) >= 0 overrides MaxRange
        let mut tw = BossAiTweaks::default();
        tw.ability_distance = 20.0;
        let mut ai3 = BossAi::new(b.boss.clone(), tw);
        assert!(ai3.update(5.0, 15.0 * 15.0, -1.0, &mut r, true).contains(&BossTrigger::PilotAbility));
        b.abilities_enabled = false;
    }

    #[test]
    fn win_lose_and_game_over() {
        let mut m = bb(Some([100, 200, 300]), "BOSS_BATTLE");
        let mut fx = Vec::new();
        let mut ev = Vec::new();
        let ks = |pp: u32, bp: u32| vec![kart(0, true, pp, Vec3::ZERO, 10.0), kart(1, false, bp, Vec3::new(0.0, 0.0, 5.0), 15.0)];
        m.update(0.1, &ks(1, 2), &mut fx, &mut ev);
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Other { tag: "SetAsBoss", car: Some(1), .. })));
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Other { tag: "GameField3244", .. })));
        // boss finishing first does not end the mode
        m.on_input(&ModeInput::KartFinished { car: 1, time: 50.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Running);
        // player finishes while still ranked 2 -> lose
        m.update(0.1, &ks(2, 1), &mut fx, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 60.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Lost);
        assert_eq!(m.stars(), 0);
        assert!(matches!(ev.last(), Some(ModeEvent::Finished { won: false, .. })));

        let mut m = bb(Some([100, 200, 300]), "BOSS_BATTLE");
        let mut ev = Vec::new();
        m.update(0.1, &ks(1, 2), &mut fx, &mut ev);
        assert!(!m.check_game_over());
        m.on_input(&ModeInput::KartFinished { car: 0, time: 40.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Won);
        assert!(m.check_game_over());
        // score 0 -> 1 star minimum on a win
        assert_eq!(m.stars(), 1);
        m.on_input(&ModeInput::Other { tag: "BaseScore", car: None, value: 250.0 }, &mut ev);
        assert_eq!(m.stars(), 2);
        m.on_input(&ModeInput::Other { tag: "BaseScore", car: None, value: 301.0 }, &mut ev);
        assert_eq!(m.stars(), 3);
        // combat events change nothing
        let before = m.score();
        m.on_input(&ModeInput::ProjectileHit { owner: 1, victim: 0, kind: "x".into() }, &mut ev);
        m.on_input(&ModeInput::CarHitCar { attacker: 0, victim: 1, speed: 20.0 }, &mut ev);
        assert_eq!(m.score(), before);
    }

    #[test]
    fn boss_destroyed_bonus_once() {
        let mut m = bb(Some([1, 2, 3]), "BOSS_BATTLE");
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let ks = vec![kart(0, true, 1, Vec3::ZERO, 0.0), kart(1, false, 2, Vec3::ZERO, 0.0)];
        m.update(0.1, &ks, &mut fx, &mut ev);
        assert!(!m.bonus_awarded());
        m.on_input(&ModeInput::Other { tag: "PilotDetachTimer", car: Some(1), value: 2.0 }, &mut ev);
        m.update(0.1, &ks, &mut fx, &mut ev);
        assert!(!m.bonus_awarded());
        m.on_input(&ModeInput::Other { tag: "PilotDetachTimer", car: Some(1), value: 0.0 }, &mut ev);
        m.update(0.1, &ks, &mut fx, &mut ev);
        assert!(m.bonus_awarded());
        assert_eq!(m.score(), 10000);
        m.update(0.1, &ks, &mut fx, &mut ev);
        assert_eq!(m.score(), 10000);
    }

    #[test]
    fn fruit_rush_rule() {
        let mut m = bb(Some([1, 2, 3]), "BOSS_FRUIT_RUSH");
        assert_eq!(m.kind(), GameModeKind::BossFruitRush);
        m.set_fruit_target(10);
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        let ks = vec![kart(0, true, 1, Vec3::ZERO, 0.0), kart(1, false, 2, Vec3::ZERO, 0.0)];
        m.update(0.1, &ks, &mut fx, &mut ev);
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 9 }, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 30.0 }, &mut ev);
        // first place but only 9/10 fruit -> not completed
        assert_eq!(m.state(), ModeState::Lost);

        let mut m = bb(Some([1, 2, 3]), "BOSS_FRUIT_RUSH");
        m.set_fruit_target(10);
        m.update(0.1, &ks, &mut fx, &mut ev);
        m.on_input(&ModeInput::SeedCollected { car: 0, count: 10 }, &mut ev);
        m.on_input(&ModeInput::KartFinished { car: 0, time: 30.0 }, &mut ev);
        assert_eq!(m.state(), ModeState::Won);
    }

    #[test]
    fn stars_fixup_and_intro() {
        let ev = BossEventDef::from_xml(
            r#"<EventDefinition><Name title="t"/><GameMode name="BOSS_BATTLE"/><Difficulty level="0.1" bosslevel="7"/><Stars Star1="500" Star2="100" Star3="200"/></EventDefinition>"#,
        )
        .unwrap();
        assert_eq!(ev.bosslevel, 7);
        assert_eq!(ev.stars, Some([500, 500, 500]));
        let ev = BossEventDef::from_xml(r#"<EventDefinition><GameMode name="BOSS_BATTLE"/><Difficulty level="0.1"/></EventDefinition>"#).unwrap();
        assert_eq!(ev.bosslevel, 1);
        assert_eq!(ev.stars, None);
        assert_eq!(intro_message_key(0), "BOSS_RACE_INTRO_THREE");
        assert_eq!(intro_message_key(1), "BOSS_RACE_INTRO_TWICE");
        assert_eq!(intro_message_key(3), "BOSS_RACE_INTRO_BOSS_UNLOCKED");
        assert_eq!(intro_tick_boxes(2), 2);
        assert!(plays_battle_puff(8) && plays_battle_puff(1) && !plays_battle_puff(2));
    }

    #[test]
    fn parse_all_bosses() {
        let Some(r) = root() else { return };
        let ids = BossDef::list_ids(&r);
        assert_eq!(ids.len(), 33);
        assert_eq!(ids, (1..=33).collect::<Vec<u32>>());
        let names = load_character_names(&r);
        assert_eq!(names.len(), 16);
        assert_eq!(names[1], "Black");
        for id in ids {
            let b = BossDef::load(&r, id).unwrap();
            assert_eq!(b.name, "Boss");
            assert_eq!(b.ability_count(), 1, "boss {id}");
            assert!(b.abilities[0].creates_ability(), "boss {id}: {}", b.abilities[0].type_string);
            assert!(b.abilities[0].kind != AbilityType::Unknown);
            assert!(b.abilities[0].max_dist_sq > b.abilities[0].min_dist_sq);
            assert!(b.max_range > 0.0 && b.min_cooldown > 0.0 && b.max_cooldown >= b.min_cooldown);
            assert!(b.ai_character_index(&names).is_some(), "boss {id} pilot {}", b.pilot_name);
            assert!(!b.kart_name_string_id.is_empty() && b.kart_upgrade_level >= 3);
        }
    }

    #[test]
    fn parse_all_boss_eventdefs() {
        let Some(r) = root() else { return };
        let evs = find_boss_eventdefs(&r);
        assert_eq!(evs.len(), 39);
        let mut no_stars = 0;
        for (p, e) in &evs {
            assert_eq!(e.game_mode, "BOSS_BATTLE", "{}", p.display());
            assert!((1..=33).contains(&e.bosslevel), "{}: bosslevel {}", p.display(), e.bosslevel);
            assert!(BossDef::path(&r, e.bosslevel).exists());
            match e.stars {
                Some([a, b, c]) => assert!(a <= b && b <= c, "{}", p.display()),
                None => no_stars += 1,
            }
            assert!(!e.environment_path.is_empty());
            // the mode builds from it
            let m = BossBattle::from_eventdef(&r, e.clone()).unwrap();
            assert_eq!(m.kind(), GameModeKind::BossBattle);
        }
        assert_eq!(no_stars, 5);
    }

    #[test]
    fn data_files() {
        let Some(r) = root() else { return };
        let dbg = std::fs::read_to_string(r.join("xml/gameplay/misc/debugtweakables.xml")).unwrap();
        let t = BossAiTweaks::from_xml(&dbg).unwrap();
        assert_eq!(t, BossAiTweaks::default());
        let cl = std::fs::read_to_string(r.join("xml/gameplay/misc/characterlevelling.xml")).unwrap();
        let x = BossXpRewards::from_characterlevelling_xml(&cl).unwrap();
        assert_eq!(x.xp_for_position(1), 50);
        assert_eq!(x.xp_for_position(2), 10);
        assert_eq!(x.xp_for_position(3), 0);
    }

    /// Prints the boss table (run with `--nocapture`); also asserts it is consistent.
    #[test]
    fn print_boss_table() {
        let Some(r) = root() else { return };
        let evs = find_boss_eventdefs(&r);
        for b in BossDef::load_all(&r) {
            let users: Vec<String> = evs
                .iter()
                .filter(|(_, e)| e.bosslevel == b.id)
                .map(|(p, _)| p.file_stem().unwrap().to_string_lossy().replace("eventdef_", ""))
                .collect();
            let a = &b.abilities[0];
            println!(
                "boss_{:03} | {} L{} | pilot {} | range {} cd {}-{} | {} {:?} obj={} dist {}..{} cd {}-{} rel {} | used by {}",
                b.id,
                b.kart_name_string_id,
                b.kart_upgrade_level,
                b.pilot_name,
                b.max_range,
                b.min_cooldown,
                b.max_cooldown,
                a.type_string,
                a.kind.class_name(),
                a.param("ObjectType").or(a.param("ObjectType1")).unwrap_or("-"),
                a.min_dist_sq.sqrt(),
                a.max_dist_sq.sqrt(),
                a.min_cooldown,
                a.max_cooldown,
                a.release_time,
                if users.is_empty() { "-".to_string() } else { users.join(",") }
            );
        }
    }
}
