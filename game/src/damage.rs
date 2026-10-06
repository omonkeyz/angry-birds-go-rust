//! Kart damage / bodywork / smash model - port of the damage half of `CCar` (libABK 2.9.1, `libABK291.so`).
//!
//! Standalone: `std` + `glam` + `roxmltree`; shared types from [`crate::modes::types`]. The module never touches the car
//! sim or the world: the host (lead) feeds it the quantities the original reads from the rigid bodies and applies the
//! returned [`DamageEvent`]s (convertible to [`CarEffect`] with [`events_to_car_effects`]).
//!
//! # What the original actually does (verified against the decompile AND the Thumb listing)
//! * `CCar+0x193c/0x196c/0x199c/0x19cc/0x1a14` = per-part (12 slots, `0xc` loops) smackable pointer / damage state /
//!   accumulated attach damage / loose-point index (-1 = all three springs) / particle-effect handle.
//!   Body part table = `CCar+0x548 -> CCarModel` (`+0xe1c` count, `+0xe20` pilot part index or -1, stride `0x128`,
//!   `+0x88` parent part index, `+0xd4..+0x11c` the `<BodyworkSpec>` values, see [`BodyworkSpec`]).
//! * `CCar+0x19fc/0x1a00/0x1a04/0x1a08` = four side-damage accumulators (front / rear / left / right). Wheel `i` shows
//!   `min(axle, side)` damage ([`Bodywork::wheel_state`]), the driver is ejected when three sides pass 10, the
//!   *AutoRepair* power-up counts wheels above 2.
//! * `CCar+0x1b08` = 0..80 hit-point meter (`CCar::AddDamage`), `+0x1a10` = damage "armour" ramp 0.25 -> 1 (+0.5/s).
//! * Bodywork parts are *always* physical rigid bodies (state 1/2) tied to the chassis by 3 spring/damper points; damage
//!   only *softens* the springs (`IntegrateVisualDamage`). In 2.9.1 the only part that is ever torn off by damage is the
//!   pilot (`AddImpactDamage` / `DetachPilot`). `fAttachBreakDamageThreshold` and `iMaxAttachLoosePoint` are parsed but
//!   never read (`CheckDetachState @0019d174` is a stub returning 0) - see the UNRESOLVED list below.
//!
//! # Verification level
//! * Thumb-listing confirmed: `AddImpactDamage` body (r1 world point passed to hooks, r2 float damage, r3 directional flag;
//!   side rule; pilot-eject rule), both call sites (`CollisionCallback`: `scaled - 0.3` NOT truncated, r3 = 1; Terence rage:
//!   `ability+0xa0`, r3 = 1), detached-thrust force/point, the `fDetachedThrust` string, the `Kill` constants (+inf).
//! * Decompile-only (Ghidra pseudo-C, constants read from the binary): `IntegrateVisualDamage` spring/damage math, the wear
//!   block in `Integrate`, AutoRepair sequencing, the stomp branch, `GetWheelState`, shake curve.
//! * The unit tests check internal consistency and the real XML values, not the original's runtime behaviour.
//!
//! # UNRESOLVED
//! * `// UNRESOLVED: BREAK` `fAttachBreakDamageThreshold` (`+0xf0`) and `iMaxAttachLoosePoint` (`+0xf4`) have no reader
//!   anywhere in the 2.9.1 decompile (grep of every `*0x128+0xf0/0xf4` form, `CheckDetachState` stub, loose-point array
//!   `CCar+0x19cc` only ever written -1). Damage state 3 is never written either. They are kept as parsed data only.
//! * `// UNRESOLVED: STATE12` the engine call `vtable+0x10` (`CCar::BreakBodywork @0019c920`, `CheckVisualDamage`,
//!   `RestoreDamageState`, `SetVisualDamage`) whose low bit chooses state 1 or 2 when a part body is created. Looks like
//!   a random bit; modelled by [`Bodywork::state12_rng`] (xorshift stand-in), the visual difference is not known.
//! * `// UNRESOLVED: PARTORDER` part order (= index into the 12 slots) comes from the model's `attach_<part>_<n>` helper
//!   nodes; the telepod `cargeom/<kart>/<part>.xml` files give specs but not the order or the attach-point geometry
//!   (needs the `.xgm` helpers). [`load_part_dir`] returns files sorted by name.
//! * `// UNRESOLVED: PARTMASS` `rb+0x8c` of a part's rigid body (used by the spring damage step) is assumed to be the
//!   part's `fMass`.
//! * `// UNRESOLVED: GAMEMODE` game-mode ids are the raw `CGame+0x34+200` ints; only their *use* is ported
//!   ([`mode_allows_hp_meter`] = 2|8, [`mode_allows_spring_damage`] = 5|8|9, [`Bodywork::check_visual_damage`] skips 5).
//! * `// UNRESOLVED: INVULN` there is no per-car "invulnerability timer" in `CCar` damage code. The respawn/ability
//!   immunity is (a) `CCar+0x42c` non-collideable timer (`CCar::CollisionEnabledCallback @00196e90`, `SetNonCollideableTimer
//!   @001aa608`; also skips Terence-rage hits), (b) the per-ability `OnCarImpactDamage` filters (vtable `+0x50`, ported as [`ImpactHook`]), (c) `CCar+0x1b50` (overtake ability: no wear damage). `CBaseAbility::ImmuneToDamage @00196bb4` (vtable `+0x90`, inlined as `ability+0xc && ability+0x6c` in `IntegrateVisualDamage`) only vetoes the *spring* damage of the main ability (`spring_damage_enabled(.., ability_veto, ..)`), NOT `AddImpactDamage`. The XML
//!   `ShieldInvulnTime` / `Invulnerability_Time` / `Invincible_Karts` keys have no reader in the car damage code
//!   (`Invulnerability_Time` is not even parsed by `CDebugManager::SetDebugTweakablesFromXML @002c7258`); the ability
//!   layer owns them. This module only exposes the hooks ([`ImpactHook`], `ability_veto`).
//! * `// UNRESOLVED: SPIN` spin-outs are triggered by abilities / smackable subtypes via `CCar::Spin360 @001b091c`
//!   (abilities.rs); nothing in the collision callback itself spins the car.
//! * `// UNRESOLVED: HP-CALLERS` no direct caller of `CCar::AddDamage @001a0500` (HP meter) exists in the decompile
//!   (vtable / network `Network_Receive_PlayerDamaged @00137d24` only), so what feeds it in 2.9.1 is unknown.
//!   `CarEffect::Repair{fraction}` has no original counterpart (only `FullRepair`); [`Bodywork::repair_fraction`] scales
//!   the accumulators for fractions < 1 and calls [`Bodywork::full_repair`] at >= 1.
//! * `// UNRESOLVED: GPT-DAMAGE` `CGameplayTweakables::Get{Competitor,Smackable,World}DamageMultiplier @001ef654..` are
//!   never called (NFS leftovers) - not used.
#![allow(dead_code)]

use crate::modes::types::*;
use glam::Quat;

// ------------------------------------------------------------------------------------------------------------
// Constants (each with the decompile address of the literal)
// ------------------------------------------------------------------------------------------------------------

/// Number of bodywork slots in the `CCar` arrays (loops run to `0xc`).
pub const MAX_PARTS: usize = 12;
/// `CCar::AddDamage` clamp: max `DAT_001a0588` = 80.0, min `DAT_001a058c` = 0.0.
pub const HP_METER_MAX: f32 = 80.0;
pub const HP_METER_MIN: f32 = 0.0;
/// `CCar::Integrate`: wheel sparks while the HP meter is above `DAT_001abf20` = 75; intensity `DAT_001abf24 + (hp-25)*10`
/// with `DAT_001abf24` = 250.
pub const HP_SPARKS_ABOVE: f32 = 75.0;
pub const HP_SPARKS_BASE: f32 = 250.0;
/// Initial / max / growth of the damage armour ramp `CCar+0x1a10` (`ReInit @001a3704`: 0x3e800000; `Integrate`: `+dt*0.5`, min 1.0).
pub const VULN_INIT: f32 = 0.25;
pub const VULN_RATE: f32 = 0.5;
pub const VULN_MAX: f32 = 1.0;
/// `CCar::CollisionCallback`: minimum (impact * ramp) that produces damage (`DAT_001a17fc` = 0.49) and the amount
/// subtracted before the float damage is passed on (`DAT_001a180c` = 0.3). NOT truncated: `AddImpactDamage(CXGSVector32 const&,
/// float, int)` receives the float in r2 (Thumb listing 001a17e8..001a17f0) and `r3 = 1` (directional).
pub const IMPACT_MIN: f32 = 0.49;
pub const IMPACT_SUB: f32 = 0.3;
/// Smackable type ids with special impact scaling in `CollisionCallback` (`+0x1150 == 0x31`: x10, `== 0x4d`: fixed `DAT_001a0ef4`=40).
pub const SMACKABLE_TYPE_X10: i32 = 0x31;
pub const SMACKABLE_X10: f32 = 10.0;
pub const SMACKABLE_TYPE_FIXED: i32 = 0x4d;
pub const SMACKABLE_FIXED_IMPACT: f32 = 40.0;
/// `dv > 1.5` (`vmov.f32 s14,0x3fc00000` @001a0f44) -> `CPilotAnimationHandler::SetAnimState(5)` (hurt).
pub const HURT_DV: f32 = 1.5;
/// The up component of the (halved) impulse is reduced by this fraction before the damage test (`*0.5` @001a0f8c).
pub const UP_REMOVE: f32 = 0.5;
/// Wheel / side damage thresholds (`GetWheelState @0019f090`): <=2 intact, <=10 level 1, <=20 level 2, >20 wheel gone.
pub const WHEEL_T1: f32 = 2.0;
pub const WHEEL_T2: f32 = 10.0;
pub const WHEEL_T3: f32 = 20.0;
/// `AddImpactDamage`: pilot is ejected when three sides exceed this (`vmov.f32 s12,0x41200000` @001a0848).
pub const PILOT_EJECT_SIDES: f32 = 10.0;
/// `CCar+0x4a0` pilot-detached timer value after ejection (`mov r1,#0x40000000`), -1.0 = not detached (`Reset`).
pub const PILOT_DETACH_TIMER: f32 = 2.0;
/// Pilot animation states set by the damage code (`CPilotAnimationHandler::SetAnimState`).
pub const PILOT_ANIM_HURT: i32 = 5;
pub const PILOT_ANIM_EJECTED: i32 = 6;
/// `CCar::GetImpactCamShakeMod @001a3f28`: clamp 2.0 -> returns `DAT_001a3f7c` = 100, else `v * DAT_001a3f78` = 50.
pub const SHAKE_CAP: f32 = 2.0;
pub const SHAKE_AT_CAP: f32 = 100.0;
pub const SHAKE_SCALE: f32 = 50.0;
/// `CCar::Update`: `shake -= dt / slowmo * 3.0`.
pub const SHAKE_DECAY: f32 = 3.0;
/// Car-on-car "stomp" penalty (`CollisionCallback`): impulse.up < `DAT_001a181c` = -100 while the other car is > 1.0 above
/// along our up axis; velocity scale `DAT_001a2030` = 0.85 (or 0.75 + total wreck when `CGame+0x31bc` >= 3 cars).
pub const STOMP_IMPULSE: f32 = -100.0;
pub const STOMP_HEIGHT: f32 = 1.0;
pub const STOMP_SPEED_SCALE: f32 = 0.85;
pub const STOMP_SPEED_SCALE_WRECK: f32 = 0.75;
pub const STOMP_WRECK_CARS: i32 = 3; // compared with `CGame+0x31bc` (NOT the car count, that is `+0x31b8`; semantic UNRESOLVED)
/// Spring tuning in `IntegrateVisualDamage @0019d17c`: damage->stiffness 0.5, ->damping 0.75 (min factors 0.5 / 0.25),
/// fixed step `DAT_0019d5b0` = 1/120 (host "fixed step" mode), vertical force removal `DAT_0019dd74` = 0.8, teleport-back
/// distance^2 `DAT_0019d5ac` = 100, grace age `DAT_0019d5a8` = 0.1 s, pilot fragility doubling, damage doubling 2.0, nuts&bolts
/// threshold 0.5 after `*0.25`.
pub const SPRING_STIFF_SOFTEN: f32 = 0.5;
pub const SPRING_DAMP_SOFTEN: f32 = 0.75;
pub const SPRING_FIXED_DT: f32 = 1.0 / 120.0;
pub const SPRING_UP_REMOVE: f32 = 0.8;
pub const SPRING_TELEPORT_DIST2: f32 = 100.0;
pub const SPRING_GRACE_AGE: f32 = 0.1;
pub const SPRING_DAMAGE_GAIN: f32 = 2.0;
pub const NUTS_FACTOR: f32 = 0.25;
pub const NUTS_MIN: f32 = 0.5;
/// Detached-part thrust (`Integrate @001ae8a0..001ae900`): local force `(0,0,dt*fDetachedThrust)` at `(0,0,-0.3)` (`DAT_001ae94c`).
pub const DETACHED_THRUST_AT_Z: f32 = -0.3;
/// Material ids in `CollisionCallback`: sparks for 0x12, 0x14, 0x15; "kill" material 0x1f (all damage = +inf when it does
/// not respawn).
pub const SPARK_MATERIALS: [u16; 3] = [0x12, 0x14, 0x15];
pub const KILL_MATERIAL: u16 = 0x1f;

/// `CDebugManager` autorepair tweakables: engine defaults (`SetDefaults @002c69cc`: floats 0x51/0x52 = 1.0, int 6 = 1)...
pub const AUTOREPAIR_DEFAULT_START_DELAY: f32 = 1.0;
pub const AUTOREPAIR_DEFAULT_VFX_DELAY: f32 = 1.0;
pub const AUTOREPAIR_DEFAULT_PIECES: i32 = 1;

/// Game-mode gates (raw `CGame+0x34+200` ids, see `UNRESOLVED: GAMEMODE`).
/// `CCar::AddDamage @001a0500`: only modes 2 and 8 keep the HP meter.
pub fn mode_allows_hp_meter(mode_id: i32) -> bool {
    mode_id == 2 || mode_id == 8
}
/// `CCar::IntegrateVisualDamage @0019d17c`: spring-force damage accrues only in modes 5, 8, 9.
pub fn mode_allows_spring_damage(mode_id: i32) -> bool {
    matches!(mode_id, 5 | 8 | 9)
}

// ------------------------------------------------------------------------------------------------------------
// BodyworkSpec
// ------------------------------------------------------------------------------------------------------------

/// One `<BodyworkSpec .../>` element - `CCarModel::ReadBodyworkSpec @001b1bec` (stride `0x128` entry, offsets quoted).
/// The same element is used for the per-part files `xml/cars/**/cargeom/<kart>/<part>.xml` and for the pilot
/// (`xml/characters/charxml/{char,boss}_NNN.xml`, the part named `pilot`).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyworkSpec {
    /// `fMass` +0xd4
    pub mass: f32,
    /// `fDrag` +0xd8
    pub drag: f32,
    /// `fDownForce` +0xdc (optional, default 0). A value < 0 marks wing-like parts that `Respawn(-1)` restores.
    pub down_force: f32,
    /// `fAttachStiffness` +0xe0
    pub attach_stiffness: f32,
    /// `fAttachDamping` +0xe4
    pub attach_damping: f32,
    /// `fAttachDamageThreshold` +0xe8: spring force below which no damage accrues (x dt x `CCar+0x534`).
    pub attach_damage_threshold: f32,
    /// `fAttachLooseDamageThreshold` +0xec: damage that fully softens the springs (x `CCar+0x534`).
    pub attach_loose_damage_threshold: f32,
    /// `fAttachBreakDamageThreshold` +0xf0 (parsed, never read by the game code - UNRESOLVED: BREAK).
    pub attach_break_damage_threshold: f32,
    /// `iMaxAttachLoosePoint` +0xf4 (optional, default 1; never read - UNRESOLVED: BREAK).
    pub max_attach_loose_point: i32,
    /// `fDetachedThrust` +0xf8 (optional, default 0): thrust along local +z applied to a detached (state 4) part.
    pub detached_thrust: f32,
    /// `sAudibleMaterial` +0xfc (32 bytes, default "Wood").
    pub audible_material: String,
    /// `sEffectSound` +0x11c (64 bytes, default empty).
    pub effect_sound: String,
    /// `cName` (present on pilot specs, ignored by `ReadBodyworkSpec`).
    pub name: Option<String>,
}

impl Default for BodyworkSpec {
    fn default() -> Self {
        BodyworkSpec {
            mass: 0.0,
            drag: 0.0,
            down_force: 0.0,
            attach_stiffness: 0.0,
            attach_damping: 0.0,
            attach_damage_threshold: 0.0,
            attach_loose_damage_threshold: 0.0,
            attach_break_damage_threshold: 0.0,
            max_attach_loose_point: 1,
            detached_thrust: 0.0,
            audible_material: "Wood".to_string(),
            effect_sound: String::new(),
            name: None,
        }
    }
}

fn attr_f32(n: roxmltree::Node, key: &str) -> Option<f32> {
    n.attribute(key).and_then(|s| s.trim().parse::<f64>().ok()).map(|v| v as f32)
}

impl BodyworkSpec {
    /// `ReadBodyworkSpec @001b1bec`: parse a `<BodyworkSpec>` node. Missing mandatory attributes are an error (the
    /// original would `strtod(NULL)`); `fDownForce`, `fDetachedThrust`, `iMaxAttachLoosePoint`, `sAudibleMaterial`,
    /// `sEffectSound` are optional with the original defaults.
    pub fn from_node(n: roxmltree::Node) -> Result<BodyworkSpec, String> {
        let req = |k: &str| attr_f32(n, k).ok_or_else(|| format!("BodyworkSpec: missing/invalid attribute {k}"));
        let mut s = BodyworkSpec {
            mass: req("fMass")?,
            drag: req("fDrag")?,
            attach_stiffness: req("fAttachStiffness")?,
            attach_damping: req("fAttachDamping")?,
            attach_damage_threshold: req("fAttachDamageThreshold")?,
            attach_loose_damage_threshold: req("fAttachLooseDamageThreshold")?,
            attach_break_damage_threshold: req("fAttachBreakDamageThreshold")?,
            ..BodyworkSpec::default()
        };
        s.down_force = attr_f32(n, "fDownForce").unwrap_or(0.0);
        s.detached_thrust = attr_f32(n, "fDetachedThrust").unwrap_or(0.0);
        if let Some(v) = n.attribute("iMaxAttachLoosePoint") {
            // `atoi`
            s.max_attach_loose_point = v.trim().parse::<i32>().unwrap_or(0);
        }
        if let Some(v) = n.attribute("sAudibleMaterial") {
            s.audible_material = v.chars().take(0x1f).collect();
        }
        if let Some(v) = n.attribute("sEffectSound") {
            s.effect_sound = v.chars().take(0x3f).collect();
        }
        s.name = n.attribute("cName").map(|v| v.to_string());
        Ok(s)
    }

    /// Parse the first `<BodyworkSpec>` element anywhere in `xml` (a part file, or a character file).
    pub fn from_xml(xml: &str) -> Result<BodyworkSpec, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
        let n = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "BodyworkSpec")
            .ok_or_else(|| "no <BodyworkSpec> element".to_string())?;
        BodyworkSpec::from_node(n)
    }

    pub fn from_file(path: &std::path::Path) -> Result<BodyworkSpec, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        BodyworkSpec::from_xml(&s)
    }

    /// `*(spec+0xdc) < 0.0` - parts restored by `Respawn(-1)` / `RestoreDamageState(1)` besides the pilot.
    pub fn is_wing(&self) -> bool {
        self.down_force < 0.0
    }
}

/// Every `*.xml` with a `<BodyworkSpec>` in a `cargeom/<kart>` directory: `(part name = file stem, spec)`, sorted by name.
/// (The game order comes from the model's `attach_<part>_<n>` helpers - UNRESOLVED: PARTORDER.)
pub fn load_part_dir(dir: &std::path::Path) -> Vec<(String, BodyworkSpec)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("xml") {
                continue;
            }
            if let Ok(spec) = BodyworkSpec::from_file(&p) {
                let name = p.file_stem().and_then(|x| x.to_str()).unwrap_or("").to_string();
                out.push((name, spec));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `CCarSpec::m_fFragility` (`+0x480`) of a `<CarSpec>` / `carxml/*_{min,max}.xml` document (blend with the character mods
/// is done in `carsim.rs`). Returns `None` when the attribute is absent.
pub fn read_fragility(xml: &str) -> Option<f32> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.descendants().find_map(|n| if n.is_element() { attr_f32(n, "m_fFragility") } else { None })
}

// ------------------------------------------------------------------------------------------------------------
// AutoRepair tweakables
// ------------------------------------------------------------------------------------------------------------

/// `Autorepair_Start_Delay`, `Autorepair_VFX_Delay`, `Autorepair_Damaged_Pieces` of `<DebugTweakables><Powerup>`
/// (`CDebugManager::SetDebugTweakablesFromXML @002c7258` writes them to debug floats 0x51, 0x52 and debug int 6, which
/// `CCar::Update @001a6cd8` / `UpdatePowerups @001ab0e4` read; indices verified from the destination addresses).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoRepairParams {
    /// Seconds without new damage before a repair may start (debug float 0x51).
    pub start_delay: f32,
    /// Seconds between the repair start (the VFX) and the actual repair (debug float 0x52).
    pub vfx_delay: f32,
    /// Repair starts when the damaged-piece count is strictly greater than this (debug int 6).
    pub damaged_pieces: i32,
}

impl Default for AutoRepairParams {
    /// Engine defaults before `DebugTweakables.xml` is loaded (`CDebugManager::SetDefaults`).
    fn default() -> Self {
        AutoRepairParams {
            start_delay: AUTOREPAIR_DEFAULT_START_DELAY,
            vfx_delay: AUTOREPAIR_DEFAULT_VFX_DELAY,
            damaged_pieces: AUTOREPAIR_DEFAULT_PIECES,
        }
    }
}

impl AutoRepairParams {
    /// Parse `<Powerup><Autorepair_*>` out of `debugtweakables.xml`; missing keys keep the engine defaults.
    pub fn from_debug_tweakables_xml(xml: &str) -> Result<AutoRepairParams, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
        let mut p = AutoRepairParams::default();
        for n in doc.descendants().filter(|n| n.is_element()) {
            let txt = n.text().map(|t| t.trim()).unwrap_or("");
            match n.tag_name().name() {
                "Autorepair_Start_Delay" => {
                    if let Ok(v) = txt.parse::<f32>() {
                        p.start_delay = v
                    }
                }
                "Autorepair_VFX_Delay" => {
                    if let Ok(v) = txt.parse::<f32>() {
                        p.vfx_delay = v
                    }
                }
                "Autorepair_Damaged_Pieces" => {
                    if let Ok(v) = txt.parse::<i32>() {
                        p.damaged_pieces = v
                    }
                }
                _ => {}
            }
        }
        Ok(p)
    }
}

// ------------------------------------------------------------------------------------------------------------
// State types
// ------------------------------------------------------------------------------------------------------------

/// `EDamageState` (`CCar+0x196c[part]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageState {
    /// 0: no physical body yet (`CheckVisualDamage` creates one).
    NoBody = 0,
    /// 1 / 2: body exists, tied to the chassis by springs (random variant, UNRESOLVED: STATE12).
    AttachedA = 1,
    AttachedB = 2,
    /// 3: never written by the 2.9.1 code (only compared `> 2`).
    Loose = 3,
    /// 4: torn off; the smackable lives on as debris (`IsAttached` = state < 4).
    Detached = 4,
}

impl DamageState {
    pub fn as_int(self) -> i32 {
        self as i32
    }
    pub fn from_int(v: i32) -> DamageState {
        match v {
            1 => DamageState::AttachedA,
            2 => DamageState::AttachedB,
            3 => DamageState::Loose,
            4 => DamageState::Detached,
            _ => DamageState::NoBody,
        }
    }
    /// Spring physics runs for states 1..=3 (`IntegrateVisualDamage`: `state - 1 < 3`).
    pub fn is_springy(self) -> bool {
        matches!(self, DamageState::AttachedA | DamageState::AttachedB | DamageState::Loose)
    }
}

/// Index into [`Bodywork::side_damage`] = `CCar+0x19fc/0x1a00/0x1a04/0x1a08`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front = 0,
    Rear = 1,
    /// local x <= 0 at the hit (`+0x1a04`)
    Left = 2,
    /// local x > 0 (`+0x1a08`)
    Right = 3,
}

/// Wheel `i` (EWheel order FL, FR, RR, RL) -> (axle side, lateral side) used by `GetWheelState @0019f090`,
/// `GetNumOfBrokenWheels @0019f16c`, `RenderOpaque` and the AutoRepair count.
pub const WHEEL_SIDES: [(Side, Side); 4] =
    [(Side::Front, Side::Left), (Side::Front, Side::Right), (Side::Rear, Side::Right), (Side::Rear, Side::Left)];

/// One bodywork part definition (an entry of the `CCarModel` part table).
#[derive(Clone, Debug)]
pub struct PartDef {
    pub name: String,
    pub spec: BodyworkSpec,
    /// `+0x88` parent part index (-1 = attached to the chassis).
    pub parent: Option<usize>,
    /// The part named `pilot` (`CCarModel+0xe20`).
    pub is_pilot: bool,
}

/// Everything the host should do / show. Convertible to [`CarEffect`] with [`events_to_car_effects`].
#[derive(Clone, Debug, PartialEq)]
pub enum DamageEvent {
    /// `CSmackableManager::AddBodyworkSmackable`: spawn the part's rigid body at its attach pose (state 1/2 attached).
    PartMaterialised { part: usize, state: DamageState },
    /// `CSmackableManager::RemoveSmackable(smackable, 1)`: delete the part body (repair).
    PartRemoved { part: usize },
    /// A part got state 4: the body keeps living as free debris (and receives [`DamageEvent::DetachedThrust`]).
    PartDetached { part: usize },
    /// The driver was ejected (`AddImpactDamage` 3-sides rule / `DetachPilot`).
    PilotEjected { part: usize },
    /// `CPilotAnimationHandler::SetAnimState(state)` (5 hurt, 6 ejected).
    PilotAnim { state: i32 },
    /// `CCar+0x4a0` ran out: spawn the character's `FeatherEffect` at the pilot part (`Integrate @001b0190`).
    PilotFeathers { part: usize },
    /// Wheel visual damage level changed (0 intact, 1, 2, 3 = wheel not rendered).
    WheelState { wheel: usize, from: u8, to: u8 },
    /// `ABKSound::CKartController::OnBodyworkSpawnParticleEffect(sEffectSound)` + effect respawn on repair.
    RepairEffect { part: usize, effect_sound: String },
    AutoRepairStarted,
    /// Everything was repaired (`FullRepair` / AutoRepair finish).
    Repaired,
    /// Spring-force damage burst: `CImpactEffectManager::Add(0, ..)` + `OnSpawnNutsAndBolts(intensity)`.
    NutsAndBolts { part: usize, intensity: i32 },
    /// `CGame::SpawnSparksAndDebris(0, ..)` at the contact (materials 0x12/0x14/0x15).
    Sparks { pos: Vec3, mag: f32 },
    /// HP-meter sparks at every wheel on the ground (`Integrate`: hp > 75): intensity `250 + (hp-25)*10`.
    HpSparks { intensity: f32 },
    /// Respawn material hit with `car+0x1b60 == 0` (`car+0x4d4 = 1`).
    RespawnRequested,
    /// Kill material 0x1f: every damage accumulator set to +inf.
    Wrecked,
    /// Credit `damage` to the *other* (human) car's `CScoreCounterDamageDone` (`AddDamageDone`).
    DamageDone { amount: f32 },
    /// Force on a detached part: local force `(0,0,dt*fDetachedThrust)` at local `(0,0,-0.3)` (`ApplyBodyForce`).
    DetachedThrust { part: usize, force_local: Vec3, at_local: Vec3 },
    /// HP meter changed (`CCar+0x1b08`).
    HpMeter { value: f32 },
}

/// Ability / power-up damage filter (vtable `+0x50` `OnCarImpactDamage(pos, dmg) -> dmg`): shields return 0, overtake
/// multiplies, minion/Stella/bubbles shields absorb. Implemented by `abilities.rs`.
pub type ImpactHook<'a> = &'a mut dyn FnMut(Vec3, f32) -> f32;

// ------------------------------------------------------------------------------------------------------------
// Pure functions
// ------------------------------------------------------------------------------------------------------------

/// `CCar::GetWheelState @0019f090` for already-combined `min(axle, side)` damage `v`: 0..=3.
pub fn wheel_state_for(v: f32) -> u8 {
    if v <= WHEEL_T1 {
        0
    } else if v <= WHEEL_T2 {
        1
    } else if v <= WHEEL_T3 {
        2
    } else {
        3
    }
}

/// `CCar::AddImpactDamage @001a070c`, hit position -> side: rotate `world_hit - car_pos` by the *inverse* of the chassis
/// quaternion (`rb+0x44,0x48,0x4c,0x50` = x,y,z,w; the Thumb listing @001a09f0..001a0a88 is exactly `q^-1 * v`).
/// `z > 0` -> front else rear; `x > 0` -> right (`+0x1a08`) else left (`+0x1a04`).
pub fn world_to_car_local(world_hit: Vec3, car_pos: Vec3, car_rot: Quat) -> Vec3 {
    car_rot.inverse() * (world_hit - car_pos)
}

/// Which two accumulators a directional hit at car-local `local` feeds.
pub fn sides_of_hit(local: Vec3) -> (Side, Side) {
    (if local.z > 0.0 { Side::Front } else { Side::Rear }, if local.x > 0.0 { Side::Right } else { Side::Left })
}

/// Pilot ejection test of `AddImpactDamage @001a0830..001a09ec`: front, left and right all > 10, or rear, left and right
/// all > 10. `side` = [front, rear, left, right].
pub fn pilot_eject_condition(side: &[f32; 4]) -> bool {
    let (f, r, l, rt) = (side[0], side[1], side[2], side[3]);
    (f.min(l) > PILOT_EJECT_SIDES && f.min(rt) > PILOT_EJECT_SIDES) || (r.min(l) > PILOT_EJECT_SIDES && r.min(rt) > PILOT_EJECT_SIDES)
}

/// `CCar::GetImpactCamShakeMod @001a3f28` (also clamps / zeroes the stored value). Feeds the camera shake.
pub fn impact_cam_shake_mod(shake: &mut f32) -> f32 {
    if *shake > SHAKE_CAP {
        *shake = SHAKE_CAP;
        SHAKE_AT_CAP
    } else if *shake >= 0.0 {
        *shake * SHAKE_SCALE
    } else {
        *shake = 0.0;
        0.0
    }
}

/// `CCar::AddDamage @001a0500` on the 0..80 hit-point meter. `allowed` = [`mode_allows_hp_meter`].
pub fn add_hp_damage(meter: f32, amount: f32, allowed: bool) -> f32 {
    if !allowed {
        return meter;
    }
    if amount > 0.0 {
        if meter >= HP_METER_MAX {
            return meter;
        }
        (meter + amount).min(HP_METER_MAX)
    } else {
        (meter + amount).max(HP_METER_MIN)
    }
}

/// Wheel sparks intensity of the HP meter (`Integrate @001abde8`): `Some(250 + (hp-25)*10)` when `hp > 75`.
pub fn hp_sparks_intensity(meter: f32) -> Option<f32> {
    if meter > HP_SPARKS_ABOVE {
        Some(HP_SPARKS_BASE + (meter - 25.0) * 10.0)
    } else {
        None
    }
}

/// What the car did in a collision callback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OtherBody {
    /// Static world / no owner (`param_8 == 0` or `GetObjectType() != 0` and not a car handled below).
    World,
    /// Another car (`GetObjectType() == 1`); `has_player` = `other+0x1af0 != 0`, `other_height_along_up` = (other pos - ours) . our up.
    Car { has_player: bool, height_above: f32 },
    /// A smackable (`GetObjectType() == 0`); `type_id` = `CSmackable+0x1150`.
    Smackable { type_id: i32 },
}

/// Inputs of `CCar::CollisionCallback @001a0cf8` (the damage part).
#[derive(Clone, Debug)]
pub struct CollisionInput {
    /// First `CXGSVector32` of the callback (impulse); the code halves it first.
    pub impulse: Vec3,
    /// Contact position (world) - second vector.
    pub hit_pos: Vec3,
    /// `CCarSpec::m_fMass` (`spec+0x44c`).
    pub mass: f32,
    /// Chassis up axis (collision object `+0x10,+0x14,+0x18`).
    pub up: Vec3,
    /// `EPhysMaterial` id of the surface/other body (`param_10`).
    pub material: u16,
    pub other: OtherBody,
    /// `PhysMaterial_GetRespawn(material) != 0`.
    pub material_respawns: bool,
    /// `CCar+0x1b60 != 0` blocks the respawn request (`else if (car+0xb60 == 0)`).
    pub respawn_locked: bool,
    /// `CCar+0xf4 != 0 || CCar+0x464 != 0`: callback returns 1.0 immediately.
    pub ignore: bool,
    /// `CGame+0x31bc` (number of cars) for the stomp rule.
    pub cgame_31bc: i32,
    /// This car has a human (`CCar+0x1af0 != 0`).
    pub self_is_player: bool,
    /// Pilot already torn off (`IsPilotDetached`): no shake record.
    pub pilot_detached: bool,
    /// `CBaseAbility::OnCarCollision` multiplier of the active ability (default 1.0).
    pub ability_collision_scale: f32,
}

impl CollisionInput {
    pub fn simple(impulse: Vec3, hit_pos: Vec3, mass: f32, up: Vec3) -> CollisionInput {
        CollisionInput {
            impulse,
            hit_pos,
            mass,
            up,
            material: 1,
            other: OtherBody::World,
            material_respawns: false,
            respawn_locked: false,
            ignore: false,
            cgame_31bc: 0,
            self_is_player: false,
            pilot_detached: false,
            ability_collision_scale: 1.0,
        }
    }
}

/// Result of [`collision_damage`] (all pure numbers).
#[derive(Clone, Debug, PartialEq)]
pub struct CollisionOutcome {
    /// |impulse*0.5| (`pCVar24`, sound / challenge / spark magnitude).
    pub mag: f32,
    /// |impulse*0.5| / mass (`fVar21` first value): shake record + hurt animation.
    pub dv: f32,
    /// Lateral impact / mass (up component reduced by [`UP_REMOVE`]).
    pub impact: f32,
    /// `impact (x type scaling) * armour ramp`.
    pub scaled: f32,
    /// `Some(scaled - 0.3)` when `scaled > 0.49`: the float handed to `AddImpactDamage` (r2) with directional flag r3 = 1.
    pub damage: Option<f32>,
    pub shake_candidate: f32,
    pub hurt_anim: bool,
    pub sparks: bool,
    pub respawn_request: bool,
    pub wreck: bool,
    /// Car-on-car stomp: multiply the velocity of the car selected by `stomp_target_self` by this (wreck everything when `stomp_wreck`).
    pub stomp_speed_scale: Option<f32>,
    pub stomp_wreck: bool,
    /// true: scale OUR velocity (this car is the human, or >= 3 cars); false: scale the OTHER car (this car is AI).
    pub stomp_target_self: bool,
    /// Credit for the other human car (`AddDamageDone`): `scaled - 0.3`.
    pub damage_done: Option<f32>,
    /// Return value of the callback (collision response scale), 1.0 unless an ability changes it.
    pub response_scale: f32,
}

/// `CCar::CollisionCallback @001a0cf8` damage maths (pure part). `ramp` = `CCar+0x1a10`.
pub fn collision_damage(c: &CollisionInput, ramp: f32) -> CollisionOutcome {
    let mut out = CollisionOutcome {
        mag: 0.0,
        dv: 0.0,
        impact: 0.0,
        scaled: 0.0,
        damage: None,
        shake_candidate: 0.0,
        hurt_anim: false,
        sparks: false,
        respawn_request: false,
        wreck: false,
        stomp_speed_scale: None,
        stomp_wreck: false,
        stomp_target_self: true,
        damage_done: None,
        response_scale: c.ability_collision_scale,
    };
    if c.ignore {
        out.response_scale = 1.0;
        return out;
    }
    // material handling (respawn / kill material)
    if !c.material_respawns {
        if c.material == KILL_MATERIAL {
            out.wreck = true;
        }
    } else if !c.respawn_locked {
        out.respawn_request = true;
    }
    let v = c.impulse * 0.5;
    out.mag = v.length();
    let mass = if c.mass != 0.0 { c.mass } else { 1.0 };
    out.dv = out.mag / mass;
    if !c.pilot_detached {
        out.shake_candidate = out.dv;
    }
    out.hurt_anim = out.dv > HURT_DV;
    // lateral impact: remove half of the up component
    let along = v.dot(c.up);
    let lat = v - c.up * (UP_REMOVE * along);
    out.impact = lat.length() / mass;
    let base = match c.other {
        OtherBody::Smackable { type_id } if type_id == SMACKABLE_TYPE_X10 => out.impact * SMACKABLE_X10,
        OtherBody::Smackable { type_id } if type_id == SMACKABLE_TYPE_FIXED => SMACKABLE_FIXED_IMPACT,
        _ => out.impact,
    };
    out.scaled = base * ramp;
    if out.scaled > IMPACT_MIN {
        out.damage = Some(out.scaled - IMPACT_SUB);
    }
    out.sparks = SPARK_MATERIALS.contains(&c.material);
    if let OtherBody::Car { has_player, height_above } = c.other {
        if has_player {
            out.damage_done = Some(out.scaled - IMPACT_SUB);
        }
        // stomp: impulse.up < -100 and the other car more than 1 m above us
        if along < STOMP_IMPULSE && height_above > STOMP_HEIGHT {
            if c.cgame_31bc < STOMP_WRECK_CARS {
                out.stomp_speed_scale = Some(STOMP_SPEED_SCALE);
                out.stomp_target_self = c.self_is_player;
            } else {
                out.stomp_speed_scale = Some(STOMP_SPEED_SCALE_WRECK);
                out.stomp_wreck = true;
            }
        }
    }
    out
}

/// Wear damage fed into all four side accumulators each frame while at least one wheel touches the ground
/// (`CCar::Integrate @001abaa8`, `_CarIntegrateCallback @001b0918`): `dt * fragility * speed / n_wheels_on_ground *
/// sum(PhysMaterial_GetWearRate(material, offroad_tyre))`. Returns `(damage_this_frame, stored wear factor car+0xa0c)`.
/// `skip` = `CCar+0x1b50 != 0` (overtake ability) -> no damage (the stored factor is still updated). `speed` is `CCar+0x1ab4`
/// (named by its uses: camera `*5.0`, speed gates `<= 1.5` / `> 10`; inferred, not symbol-confirmed).
pub fn wear_damage(dt: f32, fragility: f32, speed: f32, wheels_on_ground: u32, wear_sum: f32, skip: bool) -> (f32, f32) {
    if wheels_on_ground == 0 {
        return (0.0, 0.0);
    }
    let w = (speed / wheels_on_ground as f32) * wear_sum * fragility;
    (if skip { 0.0 } else { dt * w }, w)
}

/// Spring softening of a part (`IntegrateVisualDamage @0019d17c`): `(stiffness factor, damping factor)` from the part
/// damage `damage`, its loose threshold and `CCar+0x534` (`scale534`, 1.0 normally). The pilot part and parts with an
/// explicit loose point are never softened.
pub fn spring_softening(spec: &BodyworkSpec, damage: f32, scale534: f32, exempt: bool) -> (f32, f32) {
    if exempt {
        return (1.0, 1.0);
    }
    let r = damage / (spec.attach_loose_damage_threshold * scale534);
    if r < 0.0 {
        (1.0, 1.0)
    } else if r <= 1.0 {
        (1.0 - r * SPRING_STIFF_SOFTEN, 1.0 - r * SPRING_DAMP_SOFTEN)
    } else {
        (1.0 - SPRING_STIFF_SOFTEN, 1.0 - SPRING_DAMP_SOFTEN)
    }
}

/// One of the (up to three) attach springs: the force that is applied to the part (`ApplyWorldForce(part, F, point)`) and
/// negated on the host body. All terms are already multiplied by the step: `k = step*fAttachStiffness*soft_k`,
/// `c = step*fAttachDamping*soft_c` with `step` = the body's dt (or [`SPRING_FIXED_DT`] in the engine's fixed mode).
/// `host_vel`/`part_vel` are the velocities *at the attach points* (`v + omega x (p - com)`).
pub fn spring_point_force(k: f32, c: f32, host_pt: Vec3, part_pt: Vec3, host_vel: Vec3, part_vel: Vec3) -> Vec3 {
    (host_pt - part_pt) * k + (host_vel - part_vel) * c
}

/// Inputs of the part damage step at the end of `IntegrateVisualDamage` (only run when [`spring_damage_enabled`]).
#[derive(Clone, Copy, Debug)]
pub struct SpringDamageInput {
    /// Sum of the spring forces applied to the part this step (`fVar26, fVar20, fVar19`).
    pub force_sum: Vec3,
    /// Chassis up axis.
    pub up: Vec3,
    /// Host rigid-body step (`rb+0x98`).
    pub dt: f32,
    /// `CCar+0x1a10` ramp.
    pub ramp: f32,
    /// `CCarSpec::m_fFragility`.
    pub fragility: f32,
    pub is_pilot: bool,
    /// `CCar+0x534`.
    pub scale534: f32,
    /// Part body's `rb+0x8c` (assumed = bodywork fMass, UNRESOLVED: PARTMASS).
    pub part_mass: f32,
    /// `CCar+0xa0c` wear factor from [`wear_damage`].
    pub wear: f32,
    /// `CCar+0x1b50 != 0`: damage is not stored.
    pub no_damage: bool,
}

/// Result of [`spring_damage_step`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringDamage {
    /// Added to `CCar+0x199c[part]` (twice `fVar22`, 0 when `no_damage`).
    pub add_damage: f32,
    /// `Some(int(fVar22*0.25))`-style burst intensity (`OnSpawnNutsAndBolts`) when the burst exceeds 0.5.
    pub nuts: Option<i32>,
}

/// Damage accumulation of one part per physics step (`IntegrateVisualDamage @0019d17c` tail).
pub fn spring_damage_step(spec: &BodyworkSpec, i: &SpringDamageInput) -> SpringDamage {
    // lateral part of the force: remove SPRING_UP_REMOVE of the up component
    let along = i.force_sum.dot(i.up) * SPRING_UP_REMOVE;
    let f = i.force_sum - i.up * along;
    let mut armour = i.ramp;
    if i.is_pilot {
        armour *= i.fragility + i.fragility;
    }
    let thr = i.dt * spec.attach_damage_threshold * i.scale534;
    let f2 = armour * armour * f.length_squared();
    let mut d = i.dt * i.part_mass * i.wear;
    if thr * thr < f2 {
        d += f2.sqrt() - thr;
    }
    let add = if i.no_damage { 0.0 } else { d * SPRING_DAMAGE_GAIN };
    let burst = d * NUTS_FACTOR;
    SpringDamage { add_damage: add, nuts: if burst > NUTS_MIN { Some(burst as i32) } else { None } }
}

/// Gate of the spring damage (`pCVar12` in `IntegrateVisualDamage`): `age = CCar+0x460 > 0.1`, `CCar+0x464 == 0`, no
/// `CCar+0x1b50`, no ability veto (vtable `+0x90`), mode 5/8/9, and not a remote car in a network race.
pub fn spring_damage_enabled(age: f32, flag_464: bool, no_damage_1b50: bool, ability_veto: bool, mode_id: i32, remote_net_car: bool) -> bool {
    !flag_464 && age > SPRING_GRACE_AGE && !no_damage_1b50 && !ability_veto && mode_allows_spring_damage(mode_id) && !remote_net_car
}

// ------------------------------------------------------------------------------------------------------------
// Bodywork state machine
// ------------------------------------------------------------------------------------------------------------

/// Per-car damage state (the damage fields of `CCar`).
#[derive(Clone, Debug)]
pub struct Bodywork {
    pub parts: Vec<PartDef>,
    /// `CCarModel+0xe20`.
    pub pilot: Option<usize>,
    /// `CCar+0x196c[12]`.
    state: [DamageState; MAX_PARTS],
    /// `CCar+0x199c[12]` accumulated per-part attach damage.
    pub part_damage: [f32; MAX_PARTS],
    /// `CCar+0x19cc[12]` loose-point index (-1 = all three springs). Never set >= 0 in 2.9.1.
    pub loose_point: [i32; MAX_PARTS],
    /// `CCar+0x19fc, 0x1a00, 0x1a04, 0x1a08` = front, rear, left, right.
    pub side_damage: [f32; 4],
    /// `CCarSpec::m_fFragility` (`spec+0x480`).
    pub fragility: f32,
    /// `CCar+0x1b4c`: boss cars (`LoadBossAbilities`) - wheels never show damage, no pilot ejection.
    pub boss: bool,
    /// `CCar+0x1b50`: set by the overtake ability - no wear / spring damage.
    pub no_wear_damage: bool,
    /// `CCar+0x534`: 1.0 at `Reset @0019e09c`, rewritten by two abilities (`decomp lines 212849 and 213330, time-scale style abilities).
    pub scale534: f32,
    /// `CCar+0x1a10` armour ramp.
    pub ramp: f32,
    /// `CCar+0x1b08` 0..80 hit-point meter.
    pub hp_meter: f32,
    /// `CCar+0x4a0` pilot-detached timer (-1 = attached).
    pub pilot_timer: f32,
    /// `CCar+0x4dc` max recent impact dv (camera shake / rumble source).
    pub impact_shake: f32,
    /// `CCar+0xa0c` wear factor.
    pub wear: f32,
    /// AutoRepair state: `CCar+0x1bac` idle timer, `+0x1bb0` repair timer, `+0x1bb4` last count, `+0x1bb8` running.
    pub ar_idle: f32,
    pub ar_timer: f32,
    pub ar_count: usize,
    pub ar_running: bool,
    pub autorepair: AutoRepairParams,
    /// Host fills: the per-part effect handle != -1 (`CCar+0x1a14`) i.e. a looping effect is alive for the part.
    pub part_effect: [bool; MAX_PARTS],
    wheel_states: [u8; 4],
    /// Stand-in for the engine RNG bit that picks state 1 or 2 (UNRESOLVED: STATE12).
    pub state12_rng: u32,
}

impl Bodywork {
    /// New car: `CCar::CCar @001a8588` / `ResetCarSpec` (`memset 0` states, `0xff` loose points, ramp 0.25).
    pub fn new(parts: Vec<PartDef>, fragility: f32) -> Bodywork {
        let pilot = parts.iter().position(|p| p.is_pilot);
        let n = parts.len().min(MAX_PARTS);
        let mut parts = parts;
        parts.truncate(n);
        Bodywork {
            parts,
            pilot,
            state: [DamageState::NoBody; MAX_PARTS],
            part_damage: [0.0; MAX_PARTS],
            loose_point: [-1; MAX_PARTS],
            side_damage: [0.0; 4],
            fragility,
            boss: false,
            no_wear_damage: false,
            scale534: 1.0,
            ramp: VULN_INIT,
            hp_meter: 0.0,
            pilot_timer: -1.0,
            impact_shake: 0.0,
            wear: 0.0,
            ar_idle: 0.0,
            ar_timer: 0.0,
            ar_count: 0,
            ar_running: false,
            autorepair: AutoRepairParams::default(),
            part_effect: [false; MAX_PARTS],
            wheel_states: [0; 4],
            state12_rng: 0x9e37_79b9,
        }
    }

    pub fn num_parts(&self) -> usize {
        self.parts.len()
    }

    /// `CCar::GetVisualDamage(int) @0019cdf4`.
    pub fn visual_damage(&self, part: usize) -> DamageState {
        self.state.get(part).copied().unwrap_or(DamageState::NoBody)
    }

    /// Host-side override (`CCar::SetVisualDamage` writes the state directly after creating/removing the body).
    pub fn set_state_raw(&mut self, part: usize, s: DamageState) {
        if part < MAX_PARTS {
            self.state[part] = s;
        }
    }

    fn next_state12(&mut self) -> DamageState {
        // xorshift32 stand-in for the engine RNG call (UNRESOLVED: STATE12)
        let mut x = self.state12_rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state12_rng = x;
        if x & 1 != 0 {
            DamageState::AttachedA
        } else {
            DamageState::AttachedB
        }
    }

    /// `CCar::IsPilotDetached @0019ce08`: the pilot part or any of its parents is in state 4.
    pub fn is_pilot_detached(&self) -> bool {
        let mut cur = self.pilot;
        while let Some(i) = cur {
            if self.state[i] == DamageState::Detached {
                return true;
            }
            cur = self.parts.get(i).and_then(|p| p.parent);
        }
        false
    }

    /// `CCar::GetWheelState @0019f090`: 0 intact, 1, 2, 3 (wheel gone). Boss cars (`+0x1b4c`) always 0.
    pub fn wheel_state(&self, wheel: usize) -> u8 {
        if self.boss || wheel >= 4 {
            return 0;
        }
        let (a, b) = WHEEL_SIDES[wheel];
        wheel_state_for(self.side_damage[a as usize].min(self.side_damage[b as usize]))
    }

    /// `CCar::GetNumOfBrokenWheels @0019f16c`: wheels whose level is exactly 2 (10 < v <= 20). Used by the HUD /
    /// challenge conditions. (`num_wheels` = `CCarSpec+0x11c`.)
    pub fn num_broken_wheels(&self, num_wheels: usize) -> usize {
        if self.boss {
            return 0;
        }
        (0..num_wheels.min(4)).filter(|&w| self.wheel_state(w) == 2).count()
    }

    /// Emit [`DamageEvent::WheelState`] for every wheel whose level changed since the last call.
    pub fn poll_wheel_states(&mut self, ev: &mut Vec<DamageEvent>) {
        for w in 0..4 {
            let to = self.wheel_state(w);
            if to != self.wheel_states[w] {
                ev.push(DamageEvent::WheelState { wheel: w, from: self.wheel_states[w], to });
                self.wheel_states[w] = to;
            }
        }
    }

    /// AutoRepair "damaged pieces" count (`UpdatePowerups @001ab0e4` / `Update`): parts with state > 2 plus (non-boss)
    /// wheels whose `min(axle, side) > 2`.
    pub fn damaged_piece_count(&self) -> usize {
        let mut n = self.state.iter().filter(|s| s.as_int() > 2).count();
        if !self.boss {
            for (a, b) in WHEEL_SIDES {
                if self.side_damage[a as usize].min(self.side_damage[b as usize]) > WHEEL_T1 {
                    n += 1;
                }
            }
        }
        n
    }

    fn materialise(&mut self, part: usize, ev: &mut Vec<DamageEvent>) {
        let st = self.next_state12();
        self.state[part] = st;
        ev.push(DamageEvent::PartMaterialised { part, state: st });
    }

    /// `CCar::CheckVisualDamage @0019cb58`: create the physical body of every part that has none (state 0). Skipped
    /// in game mode 5.
    pub fn check_visual_damage(&mut self, mode_id: i32, ev: &mut Vec<DamageEvent>) {
        if mode_id == 5 {
            return;
        }
        for i in 0..self.parts.len() {
            if self.state[i] == DamageState::NoBody {
                self.materialise(i, ev);
            }
        }
    }

    fn clear_part_slot(&mut self, i: usize, remove_body: bool, ev: &mut Vec<DamageEvent>) {
        if remove_body && self.state[i] != DamageState::NoBody && self.state[i] != DamageState::Detached {
            ev.push(DamageEvent::PartRemoved { part: i });
        }
        self.part_damage[i] = 0.0; // DAT_001a36f0 = 0.0
        self.loose_point[i] = -1;
        self.state[i] = DamageState::NoBody;
    }

    /// `CCar::FullRepair @001aa858` (identical body to the AutoRepair finish in `UpdatePowerups`): clear the four side
    /// accumulators, delete the bodies of attached parts (detached ones stay as debris), reset all 12 slots. A part that
    /// was torn off with a live effect re-triggers its `sEffectSound` (`OnBodyworkSpawnParticleEffect`).
    pub fn full_repair(&mut self, ev: &mut Vec<DamageEvent>) {
        self.side_damage = [0.0; 4];
        for i in 0..MAX_PARTS {
            if self.state[i] == DamageState::Detached {
                if self.part_effect[i] {
                    if let Some(p) = self.parts.get(i) {
                        ev.push(DamageEvent::RepairEffect { part: i, effect_sound: p.spec.effect_sound.clone() });
                    }
                }
                // the debris body is kept: `smackable != 0 && state != 4` is false
            } else if self.state[i] != DamageState::NoBody {
                ev.push(DamageEvent::PartRemoved { part: i });
            }
            self.part_damage[i] = 0.0;
            self.loose_point[i] = -1;
            self.state[i] = DamageState::NoBody;
        }
        self.poll_wheel_states(ev);
        ev.push(DamageEvent::Repaired);
    }

    /// `CarEffect::Repair{fraction}` (no original counterpart): `>= 1` = [`Bodywork::full_repair`] (+ `check_visual_damage`),
    /// otherwise scale the side accumulators and part damage by `1 - fraction`.
    pub fn repair_fraction(&mut self, fraction: f32, mode_id: i32, ev: &mut Vec<DamageEvent>) {
        if fraction >= 1.0 {
            self.full_repair(ev);
            self.check_visual_damage(mode_id, ev);
        } else if fraction > 0.0 {
            let k = 1.0 - fraction;
            for s in &mut self.side_damage {
                *s *= k;
            }
            for d in &mut self.part_damage {
                *d *= k;
            }
            self.poll_wheel_states(ev);
        }
    }

    /// `CCar::RestoreDamageState(int) @001a3294` (and the identical block of `Respawn @001a5694`): clears the side
    /// accumulators; `full == true` (argument 0) resets every slot, otherwise only the pilot part and parts with
    /// `fDownForce < 0`. Then every state-0 part (all of them, or the same subset) gets a fresh body.
    pub fn restore_damage_state(&mut self, full: bool, ev: &mut Vec<DamageEvent>) {
        self.side_damage = [0.0; 4];
        let sel = |s: &Bodywork, i: usize| -> bool {
            full || s.pilot == Some(i) || s.parts.get(i).map(|p| p.spec.is_wing()).unwrap_or(false)
        };
        if full {
            for i in 0..MAX_PARTS {
                // smackable removed unless detached; slot reset without touching a detached debris body
                self.clear_part_slot(i, true, ev);
            }
        } else {
            for i in 0..MAX_PARTS {
                if sel(self, i) {
                    self.clear_part_slot(i, true, ev);
                }
            }
        }
        for i in 0..self.parts.len() {
            if sel(self, i) && self.state[i] == DamageState::NoBody {
                self.materialise(i, ev);
            }
        }
        self.poll_wheel_states(ev);
    }

    /// `CCar::BreakBodywork(int, uint) @0019c920`: tear off one part (creates its body first when it has none).
    pub fn break_bodywork(&mut self, part: usize, ev: &mut Vec<DamageEvent>) {
        if part >= self.parts.len() {
            return;
        }
        if self.state[part] == DamageState::NoBody {
            self.materialise(part, ev);
        }
        self.state[part] = DamageState::Detached;
        ev.push(DamageEvent::PartDetached { part });
    }

    /// `CCar::DetachPilot @001a2034` / the pilot branch of `AddImpactDamage` (`@001a08c0`): materialise if needed,
    /// state 4, `CCar+0x4a0 = 2.0`, pilot animation 6.
    pub fn detach_pilot(&mut self, ev: &mut Vec<DamageEvent>) {
        let Some(p) = self.pilot else { return };
        if self.state[p] == DamageState::Detached {
            return;
        }
        if self.state[p] == DamageState::NoBody {
            self.materialise(p, ev);
        }
        self.state[p] = DamageState::Detached;
        self.pilot_timer = PILOT_DETACH_TIMER;
        ev.push(DamageEvent::PartDetached { part: p });
        ev.push(DamageEvent::PilotEjected { part: p });
        ev.push(DamageEvent::PilotAnim { state: PILOT_ANIM_EJECTED });
    }

    /// `CCar::Kill @001a2310`: every side accumulator and the first part slots set to +inf (visual wreck; the pilot is
    /// ejected by the next hit through the normal rule).
    pub fn kill(&mut self) {
        self.side_damage = [f32::INFINITY; 4];
        for d in self.part_damage.iter_mut() {
            *d = f32::INFINITY;
        }
    }

    /// `CCar::AddDamageToBodyPart(uint, float) @001a0594`.
    pub fn add_damage_to_body_part(&mut self, part: usize, amount: f32) {
        if part < MAX_PARTS {
            self.part_damage[part] += amount;
        }
    }

    /// `CCar::AddDamageToBodyParts(float) @001a05b0`: adds to the first `num_parts` slots.
    pub fn add_damage_to_body_parts(&mut self, amount: f32) {
        for i in 0..self.parts.len().min(MAX_PARTS) {
            self.part_damage[i] += amount;
        }
    }

    /// `CCar::AddDamage(float) @001a0500` on the HP meter.
    pub fn add_hp(&mut self, amount: f32, mode_id: i32, ev: &mut Vec<DamageEvent>) {
        let n = add_hp_damage(self.hp_meter, amount, mode_allows_hp_meter(mode_id));
        if n != self.hp_meter {
            self.hp_meter = n;
            ev.push(DamageEvent::HpMeter { value: n });
        }
    }

    /// `CCar::AddImpactDamage(CXGSVector32 const&, float, int) @001a070c` (Thumb listing confirmed: r1 = world contact point,
    /// r2 = float damage, r3 = directional flag). `hit_world` is handed to the ability hooks unchanged (@001a0954 passes r1);
    /// the car-local position ([`world_to_car_local`] with the chassis pose) is only used to pick the two sides.
    /// `hooks` = active abilities' `OnCarImpactDamage` filters in the original order (main ability first). Returns the
    /// fragility-scaled damage that was added.
    pub fn add_impact_damage(&mut self, hit_world: Vec3, car_pos: Vec3, car_rot: Quat, damage: f32, directional: bool, hooks: &mut [ImpactHook], ev: &mut Vec<DamageEvent>) -> f32 {
        let car_local_hit = world_to_car_local(hit_world, car_pos, car_rot);
        let mut d = damage;
        for h in hooks.iter_mut() {
            d = h(hit_world, d);
        }
        let f = d * self.fragility;
        if f > 0.0 {
            self.ar_idle = 0.0; // CCar+0x1bac = 0
        }
        if !directional {
            for s in &mut self.side_damage {
                *s += f;
            }
        } else {
            let (a, b) = sides_of_hit(car_local_hit);
            self.side_damage[a as usize] += f;
            self.side_damage[b as usize] += f;
        }
        self.poll_wheel_states(ev);
        if self.boss {
            return f;
        }
        if pilot_eject_condition(&self.side_damage) && self.pilot.map(|p| self.state[p] != DamageState::Detached).unwrap_or(false) {
            self.detach_pilot(ev);
        }
        f
    }

    /// `CarEffect::Damage{amount}` from an ability/boss: `AddImpactDamage` with `hit_world` when known (directional) else
    /// non-directional (r3 = 0).
    pub fn apply_damage_effect(&mut self, amount: f32, hit_world: Option<(Vec3, Vec3, Quat)>, hooks: &mut [ImpactHook], ev: &mut Vec<DamageEvent>) -> f32 {
        match hit_world {
            Some((hit, pos, rot)) => self.add_impact_damage(hit, pos, rot, amount, true, hooks, ev),
            None => self.add_impact_damage(Vec3::ZERO, Vec3::ZERO, Quat::IDENTITY, amount, false, hooks, ev),
        }
    }

    /// Full damage part of `CCar::CollisionCallback @001a0cf8`: runs [`collision_damage`], applies the damage and records
    /// the camera shake. Returns the outcome (its `stomp_*` fields are for the host to apply to the car velocity,
    /// `response_scale` is the callback's return value).
    pub fn on_collision(&mut self, c: &CollisionInput, car_pos: Vec3, car_rot: Quat, hooks: &mut [ImpactHook], ev: &mut Vec<DamageEvent>) -> CollisionOutcome {
        let mut c2 = c.clone();
        c2.pilot_detached = self.is_pilot_detached();
        let out = collision_damage(&c2, self.ramp);
        if c.ignore {
            return out;
        }
        if out.wreck {
            self.kill();
            ev.push(DamageEvent::Wrecked);
        }
        if out.respawn_request {
            ev.push(DamageEvent::RespawnRequested);
        }
        if out.shake_candidate > self.impact_shake {
            self.impact_shake = out.shake_candidate;
        }
        if out.hurt_anim {
            ev.push(DamageEvent::PilotAnim { state: PILOT_ANIM_HURT });
        }
        if let Some(d) = out.damage {
            self.add_impact_damage(c.hit_pos, car_pos, car_rot, d, true, hooks, ev);
        }
        if let Some(a) = out.damage_done {
            ev.push(DamageEvent::DamageDone { amount: a });
        }
        if out.stomp_wreck {
            self.kill();
            ev.push(DamageEvent::Wrecked);
        }
        if out.sparks {
            ev.push(DamageEvent::Sparks { pos: c.hit_pos, mag: out.mag });
        }
        out
    }

    /// Per-frame wear of `Integrate`: adds to all four side accumulators and updates `CCar+0xa0c`; also the armour ramp
    /// (`+0xa10`, only while a wheel is on the ground). Returns the damage added.
    pub fn wear_step(&mut self, dt: f32, speed: f32, wheels_on_ground: u32, wear_sum: f32, ev: &mut Vec<DamageEvent>) -> f32 {
        let (d, w) = wear_damage(dt, self.fragility, speed, wheels_on_ground, wear_sum, self.no_wear_damage);
        if wheels_on_ground == 0 {
            self.wear = 0.0; // `CCar+0xa0c = 0` is written every frame before the grounded test
            return 0.0;
        }
        self.wear = w;
        for s in &mut self.side_damage {
            *s += d;
        }
        self.ramp = (self.ramp + dt * VULN_RATE).min(VULN_MAX);
        self.poll_wheel_states(ev);
        d
    }

    /// Apply a [`spring_damage_step`] result to part `part`.
    pub fn add_spring_damage(&mut self, part: usize, r: SpringDamage, ev: &mut Vec<DamageEvent>) {
        if part < MAX_PARTS {
            self.part_damage[part] += r.add_damage;
        }
        if let Some(i) = r.nuts {
            ev.push(DamageEvent::NutsAndBolts { part, intensity: i });
        }
    }

    /// Spring factors for `part` right now (stiffness factor, damping factor) - pilot and loose-point parts exempt.
    pub fn part_spring_softening(&self, part: usize) -> (f32, f32) {
        let exempt = self.pilot == Some(part) || self.loose_point.get(part).copied().unwrap_or(-1) >= 0;
        match self.parts.get(part) {
            Some(p) => spring_softening(&p.spec, self.part_damage[part], self.scale534, exempt),
            None => (1.0, 1.0),
        }
    }

    /// Per-frame housekeeping of `CCar::Update` / `Integrate`: shake decay (`dt/slowmo*3`), pilot-detached timer
    /// (feathers when it crosses 0), thrust on detached parts. `slowmo` = `CGame::GetCurrentSlowMoTimeMultiplier`.
    pub fn update(&mut self, dt: f32, slowmo: f32, ev: &mut Vec<DamageEvent>) {
        let sm = if slowmo != 0.0 { slowmo } else { 1.0 };
        self.impact_shake -= dt / sm * SHAKE_DECAY;
        if self.pilot_timer > 0.0 {
            self.pilot_timer -= dt;
            if self.pilot_timer < 0.0 {
                if let Some(p) = self.pilot {
                    ev.push(DamageEvent::PilotFeathers { part: p });
                }
            }
        }
        for i in 0..self.parts.len() {
            let t = self.parts[i].spec.detached_thrust;
            if t != 0.0 && self.state[i] == DamageState::Detached {
                ev.push(DamageEvent::DetachedThrust {
                    part: i,
                    force_local: Vec3::new(0.0, 0.0, dt * t),
                    at_local: Vec3::new(0.0, 0.0, DETACHED_THRUST_AT_Z),
                });
            }
        }
    }

    // ---- convenience wrappers returning event vectors ------------------------------------------------

    /// [`Bodywork::on_collision`] returning the events (name asked for in the task: `apply_impact`).
    pub fn apply_impact(&mut self, c: &CollisionInput, car_pos: Vec3, car_rot: Quat, hooks: &mut [ImpactHook]) -> (CollisionOutcome, Vec<DamageEvent>) {
        let mut ev = Vec::new();
        let o = self.on_collision(c, car_pos, car_rot, hooks, &mut ev);
        (o, ev)
    }

    /// [`Bodywork::update`] at normal speed, returning the events.
    pub fn tick(&mut self, dt: f32) -> Vec<DamageEvent> {
        let mut ev = Vec::new();
        self.update(dt, 1.0, &mut ev);
        ev
    }

    /// [`Bodywork::full_repair`] returning the events (task name: `repair`).
    pub fn repair(&mut self) -> Vec<DamageEvent> {
        let mut ev = Vec::new();
        self.full_repair(&mut ev);
        ev
    }

    /// `CCar::GetImpactCamShakeMod`.
    pub fn cam_shake_mod(&mut self) -> f32 {
        impact_cam_shake_mod(&mut self.impact_shake)
    }

    /// AutoRepair power-up (`CCar::Update @001a6cd8` block / `UpdatePowerups @001ab0e4`), call every frame. `active` =
    /// `CPlayerInfo::IsPowerUpActive(1)` and the car has a human driver. Waits `start_delay` seconds without damage
    /// (`ar_idle` is zeroed by every damaging hit), starts when more than `damaged_pieces` pieces are damaged, and after
    /// `vfx_delay` more seconds performs a [`full_repair`](Bodywork::full_repair) - unless the pilot is detached.
    pub fn autorepair_update(&mut self, dt: f32, active: bool, ev: &mut Vec<DamageEvent>) {
        if !active {
            return;
        }
        self.ar_idle += dt;
        self.ar_timer += dt;
        if !self.ar_running {
            if self.is_pilot_detached() {
                return;
            }
            if self.autorepair.start_delay < self.ar_idle {
                let n = self.damaged_piece_count();
                self.ar_count = n;
                if n as i64 > self.autorepair.damaged_pieces as i64 {
                    self.ar_timer = 0.0;
                    self.ar_running = true;
                    ev.push(DamageEvent::AutoRepairStarted);
                } else {
                    self.ar_idle = 0.0;
                }
            }
            if !self.ar_running {
                return;
            }
        }
        if self.ar_timer <= self.autorepair.vfx_delay {
            return;
        }
        if !self.is_pilot_detached() {
            // the original leaves the slots in state 0 here (CheckVisualDamage is only paired with FullRepair by game-mode code)
            self.full_repair(ev);
        }
        self.ar_running = false;
    }

    /// `Integrate` HP-meter wheel sparks request (every grounded wheel gets one burst per frame).
    pub fn hp_sparks(&self) -> Option<DamageEvent> {
        hp_sparks_intensity(self.hp_meter).map(|i| DamageEvent::HpSparks { intensity: i })
    }
}

/// Map [`DamageEvent`]s to the shared [`CarEffect`] requests the lead already handles; events without a counterpart
/// become `CarEffect::Other { tag, .. }` (tag = original function / field name). `pos` = car position.
pub fn events_to_car_effects(car: CarId, pos: Vec3, events: &[DamageEvent]) -> Vec<CarEffect> {
    let mut out = Vec::new();
    for e in events {
        match e {
            // NOT CarEffect::Repair: the host maps that onto `repair_fraction`, which would loop
            DamageEvent::Repaired => out.push(CarEffect::Other { tag: "CCar::FullRepair", car: Some(car), vals: [0.0; 4], pos }),
            DamageEvent::Sparks { pos: p, mag } => out.push(CarEffect::Other { tag: "CGame::SpawnSparksAndDebris(0)", car: Some(car), vals: [*mag, 0.0, 0.0, 0.0], pos: *p }),
            DamageEvent::RepairEffect { effect_sound, .. } => {
                out.push(CarEffect::Sound { name: effect_sound.clone(), car: Some(car) })
            }
            DamageEvent::PilotEjected { part } => {
                out.push(CarEffect::Other { tag: "CCar::DetachPilot", car: Some(car), vals: [*part as f32, 0.0, 0.0, 0.0], pos })
            }
            DamageEvent::PilotAnim { state } => {
                out.push(CarEffect::Other { tag: "CPilotAnimationHandler::SetAnimState", car: Some(car), vals: [*state as f32, 0.0, 0.0, 0.0], pos })
            }
            DamageEvent::RespawnRequested => out.push(CarEffect::Other { tag: "CCar+0x4d4 respawn", car: Some(car), vals: [1.0; 4], pos }),
            DamageEvent::Wrecked => out.push(CarEffect::Other { tag: "CCar::Kill", car: Some(car), vals: [0.0; 4], pos }),
            DamageEvent::NutsAndBolts { part, intensity } => {
                out.push(CarEffect::Other { tag: "OnSpawnNutsAndBolts", car: Some(car), vals: [*part as f32, *intensity as f32, 0.0, 0.0], pos })
            }
            DamageEvent::DetachedThrust { part, force_local, at_local } => {
                out.push(CarEffect::Other { tag: "ApplyBodyForce(part)", car: Some(car), vals: [*part as f32, force_local.z, at_local.z, 0.0], pos })
            }
            DamageEvent::HpSparks { intensity } => {
                out.push(CarEffect::Other { tag: "SpawnSparksAndDebris(hp)", car: Some(car), vals: [*intensity, 0.0, 0.0, 0.0], pos })
            }
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn assets() -> Option<PathBuf> {
        let p = PathBuf::from(ASSETS292);
        if p.join("xml").is_dir() {
            Some(p)
        } else {
            None
        }
    }

    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().and_then(|x| x.to_str()) == Some("xml") {
                    out.push(p);
                }
            }
        }
    }

    fn part(name: &str, pilot: bool, parent: Option<usize>) -> PartDef {
        PartDef {
            name: name.to_string(),
            spec: BodyworkSpec::from_xml(r#"<BodyworkSpec fMass="6.0" fDrag="0.1" fAttachStiffness="9000.0" fAttachDamping="250.0" fAttachDamageThreshold="3500.0" fAttachLooseDamageThreshold="33.0" fAttachBreakDamageThreshold="100.0" iMaxAttachLoosePoint="1" sAudibleMaterial="wood"/>"#).unwrap(),
            parent,
            is_pilot: pilot,
        }
    }

    fn car() -> Bodywork {
        Bodywork::new(vec![part("kartnose", false, None), part("pilot", true, None), part("sidepanelleft", false, None)], 0.5)
    }

    #[test]
    fn parse_part_spec_real() {
        let Some(a) = assets() else { return };
        let p = a.join("xml/cars/telepod/cargeom/kart_black_blackfriday/kartnose.xml");
        let s = BodyworkSpec::from_file(&p).unwrap();
        assert_eq!(s.mass, 6.0);
        assert_eq!(s.drag, 0.1);
        assert_eq!(s.attach_stiffness, 9000.0);
        assert_eq!(s.attach_damping, 250.0);
        assert_eq!(s.attach_damage_threshold, 3500.0);
        assert_eq!(s.attach_loose_damage_threshold, 33.0);
        assert_eq!(s.attach_break_damage_threshold, 100.0);
        assert_eq!(s.max_attach_loose_point, 1);
        assert_eq!(s.audible_material, "wood");
        assert_eq!(s.down_force, 0.0);
        assert_eq!(s.detached_thrust, 0.0);
        assert!(s.effect_sound.is_empty());
    }

    #[test]
    fn parse_every_bodywork_spec_in_assets() {
        let Some(a) = assets() else { return };
        let mut files = Vec::new();
        walk(&a.join("xml"), &mut files);
        let (mut n, mut thrust, mut wings, mut effects, mut cname) = (0, 0, 0, 0, 0);
        for f in files {
            let txt = match std::fs::read_to_string(&f) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if !txt.contains("<BodyworkSpec") {
                continue;
            }
            let s = BodyworkSpec::from_xml(&txt).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            assert!(s.mass > 0.0 && s.attach_stiffness > 0.0 && s.attach_damping > 0.0, "{}", f.display());
            assert!(s.attach_loose_damage_threshold > 0.0, "{}", f.display());
            assert!(s.attach_break_damage_threshold >= s.attach_loose_damage_threshold, "{}", f.display());
            n += 1;
            if s.detached_thrust != 0.0 {
                thrust += 1;
            }
            if s.is_wing() {
                wings += 1;
            }
            if !s.effect_sound.is_empty() {
                effects += 1;
            }
            if s.name.is_some() {
                cname += 1;
            }
        }
        assert_eq!(n, 471, "BodyworkSpec files in assets292/xml");
        assert_eq!(thrust, 20, "fDetachedThrust users");
        assert_eq!(effects, 12, "sEffectSound users");
        assert_eq!(cname, 10, "cName pilots");
        assert_eq!(wings, 24, "fDownForce < 0 parts");
    }

    #[test]
    fn character_pilot_spec() {
        let Some(a) = assets() else { return };
        let s = BodyworkSpec::from_file(&a.join("xml/characters/charxml/char_001.xml")).unwrap();
        assert_eq!(s.mass, 50.0);
        assert_eq!(s.attach_stiffness, 40000.0);
        assert_eq!(s.attach_damping, 2400.0);
        assert_eq!(s.attach_damage_threshold, 60000.0);
        assert_eq!(s.attach_loose_damage_threshold, 75.0);
        assert_eq!(s.attach_break_damage_threshold, 750.0);
        assert_eq!(s.audible_material, "Wood"); // default
    }

    #[test]
    fn fragility_from_carspec_and_carxml() {
        let Some(a) = assets() else { return };
        let base = std::fs::read_to_string(a.join("xml/cars/carspec/kart_base.xml")).unwrap();
        assert_eq!(read_fragility(&base), Some(0.5));
        let mx = std::fs::read_to_string(a.join("xml/cars/carxml/kart_red_max.xml")).unwrap();
        let mn = std::fs::read_to_string(a.join("xml/cars/carxml/kart_red_min.xml")).unwrap();
        assert_eq!(read_fragility(&mx), Some(0.0));
        assert_eq!(read_fragility(&mn), Some(10.0));
    }

    #[test]
    fn autorepair_tweakables_real() {
        let Some(a) = assets() else { return };
        let x = std::fs::read_to_string(a.join("xml_gameplay/misc/debugtweakables.xml")).unwrap();
        let p = AutoRepairParams::from_debug_tweakables_xml(&x).unwrap();
        assert_eq!(p.start_delay, 2.0);
        assert_eq!(p.vfx_delay, 1.0);
        assert_eq!(p.damaged_pieces, 1);
        assert_eq!(AutoRepairParams::default().start_delay, 1.0);
    }

    #[test]
    fn part_dir_loading() {
        let Some(a) = assets() else { return };
        let v = load_part_dir(&a.join("xml/cars/telepod/cargeom/kart_black_blackfriday"));
        let names: Vec<_> = v.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["canleft", "canright", "exhaustleft", "exhaustright", "kartnose", "sidepanelleft", "sidepanelright"]);
    }

    #[test]
    fn wheel_levels() {
        assert_eq!(wheel_state_for(0.0), 0);
        assert_eq!(wheel_state_for(2.0), 0);
        assert_eq!(wheel_state_for(2.01), 1);
        assert_eq!(wheel_state_for(10.0), 1);
        assert_eq!(wheel_state_for(10.01), 2);
        assert_eq!(wheel_state_for(20.0), 2);
        assert_eq!(wheel_state_for(20.01), 3);
        let mut b = car();
        b.side_damage = [5.0, 0.0, 3.0, 12.0];
        // FL = min(front 5, left 3) = 3 -> 1; FR = min(5, 12) = 5 -> 1; RR = min(rear 0, right) = 0; RL = 0
        assert_eq!([b.wheel_state(0), b.wheel_state(1), b.wheel_state(2), b.wheel_state(3)], [1, 1, 0, 0]);
        b.side_damage = [30.0, 15.0, 30.0, 30.0];
        assert_eq!([b.wheel_state(0), b.wheel_state(1), b.wheel_state(2), b.wheel_state(3)], [3, 3, 2, 2]);
        assert_eq!(b.num_broken_wheels(4), 2);
        b.boss = true;
        assert_eq!(b.wheel_state(0), 0);
    }

    #[test]
    fn directional_hit_feeds_two_sides() {
        let mut b = car();
        let mut ev = Vec::new();
        // hit at front-right: local z > 0, x > 0, damage 4 * fragility 0.5 = 2
        let f = b.add_impact_damage(Vec3::new(0.5, 0.0, 1.0), Vec3::ZERO, Quat::IDENTITY, 4.0, true, &mut [], &mut ev);
        assert_eq!(f, 2.0);
        assert_eq!(b.side_damage, [2.0, 0.0, 0.0, 2.0]);
        // rear-left
        b.add_impact_damage(Vec3::new(-0.5, 0.0, -1.0), Vec3::ZERO, Quat::IDENTITY, 2.0, true, &mut [], &mut ev);
        assert_eq!(b.side_damage, [2.0, 1.0, 1.0, 2.0]);
        // non directional -> all four
        b.add_impact_damage(Vec3::ZERO, Vec3::ZERO, Quat::IDENTITY, 2.0, false, &mut [], &mut ev);
        assert_eq!(b.side_damage, [3.0, 2.0, 2.0, 3.0]);
    }

    #[test]
    fn hooks_and_immunity() {
        let mut b = car();
        let mut ev = Vec::new();
        let mut zero = |_p: Vec3, _d: f32| 0.0;
        b.add_impact_damage(Vec3::Z, Vec3::ZERO, Quat::IDENTITY, 10.0, true, &mut [&mut zero], &mut ev);
        assert_eq!(b.side_damage, [0.0; 4]);
        let mut half = |_p: Vec3, d: f32| d * 0.5;
        b.add_impact_damage(Vec3::Z, Vec3::ZERO, Quat::IDENTITY, 10.0, true, &mut [&mut half], &mut ev);
        assert_eq!(b.side_damage[0], 2.5);
        // the hook receives the WORLD contact point (original passes r1 untouched)
        let seen = std::cell::Cell::new(Vec3::ZERO);
        let mut spy = |p: Vec3, d: f32| { seen.set(p); d };
        let q = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        b.add_impact_damage(Vec3::new(5.0, 0.0, 7.0), Vec3::new(5.0, 0.0, 6.0), q, 1.0, true, &mut [&mut spy], &mut ev);
        assert_eq!(seen.get(), Vec3::new(5.0, 0.0, 7.0));
    }

    #[test]
    fn pilot_ejection_rule() {
        assert!(!pilot_eject_condition(&[11.0, 0.0, 11.0, 10.0]));
        assert!(pilot_eject_condition(&[11.0, 0.0, 11.0, 10.01]));
        assert!(pilot_eject_condition(&[0.0, 11.0, 11.0, 11.0]));
        assert!(!pilot_eject_condition(&[11.0, 11.0, 0.0, 11.0]));
        let mut b = car();
        let mut ev = Vec::new();
        b.side_damage = [9.0, 0.0, 9.0, 9.0];
        // front-right hit adding 2.0 * 0.5... use 4.0 damage -> f = 2.0 on front+right: front 11, right 11, left 9 -> no
        b.add_impact_damage(Vec3::new(1.0, 0.0, 1.0), Vec3::ZERO, Quat::IDENTITY, 4.0, true, &mut [], &mut ev);
        assert!(!b.is_pilot_detached());
        b.add_impact_damage(Vec3::new(-1.0, 0.0, 1.0), Vec3::ZERO, Quat::IDENTITY, 4.0, true, &mut [], &mut ev);
        assert!(b.is_pilot_detached());
        assert_eq!(b.pilot_timer, 2.0);
        assert_eq!(b.visual_damage(1), DamageState::Detached);
        assert!(ev.contains(&DamageEvent::PilotAnim { state: 6 }));
        // the feathers appear when the 2 s timer runs out
        let mut ev2 = Vec::new();
        b.update(1.0, 1.0, &mut ev2);
        assert!(!ev2.iter().any(|e| matches!(e, DamageEvent::PilotFeathers { .. })));
        b.update(1.5, 1.0, &mut ev2);
        assert!(ev2.iter().any(|e| matches!(e, DamageEvent::PilotFeathers { part: 1 })));
        // boss cars never lose the pilot
        let mut bb = car();
        bb.boss = true;
        bb.side_damage = [50.0; 4];
        bb.add_impact_damage(Vec3::Z, Vec3::ZERO, Quat::IDENTITY, 4.0, true, &mut [], &mut ev);
        assert!(!bb.is_pilot_detached());
    }

    #[test]
    fn pilot_detached_through_parent_chain() {
        let mut b = Bodywork::new(vec![part("seat", false, None), part("pilot", true, Some(0))], 1.0);
        assert!(!b.is_pilot_detached());
        b.set_state_raw(0, DamageState::Detached);
        assert!(b.is_pilot_detached());
    }

    #[test]
    fn collision_numbers() {
        // impulse (0, 0, 1000) -> halved 500, mass 285 -> dv 1.754 > 1.5 (hurt), no up component
        let c = CollisionInput::simple(Vec3::new(0.0, 0.0, 1000.0), Vec3::ZERO, 285.0, Vec3::Y);
        let o = collision_damage(&c, 1.0);
        assert!((o.mag - 500.0).abs() < 1e-3);
        assert!((o.dv - 500.0 / 285.0).abs() < 1e-5);
        assert!(o.hurt_anim);
        assert!((o.impact - 500.0 / 285.0).abs() < 1e-5);
        assert!((o.damage.unwrap() - (500.0 / 285.0 - 0.3)).abs() < 1e-5);
        // armour ramp 0.25 scales it down: 0.4386 < 0.49 -> no damage
        let o = collision_damage(&c, 0.25);
        assert!(o.damage.is_none());
        // vertical impulse: up component loses half -> impact = 0.5 * dv
        let c = CollisionInput::simple(Vec3::new(0.0, 570.0, 0.0), Vec3::ZERO, 285.0, Vec3::Y);
        let o = collision_damage(&c, 1.0);
        assert!((o.dv - 1.0).abs() < 1e-5);
        assert!((o.impact - 0.5).abs() < 1e-5);
        // smackable special types
        let mut c = CollisionInput::simple(Vec3::new(0.0, 0.0, 285.0), Vec3::ZERO, 285.0, Vec3::Y);
        c.other = OtherBody::Smackable { type_id: 0x31 };
        let o = collision_damage(&c, 1.0);
        assert!((o.scaled - 5.0).abs() < 1e-4); // impact 0.5 * 10
        c.other = OtherBody::Smackable { type_id: 0x4d };
        let o = collision_damage(&c, 0.5);
        assert!((o.scaled - 20.0).abs() < 1e-4);
        assert!((o.damage.unwrap() - 19.7).abs() < 1e-4);
        // materials
        c.other = OtherBody::World;
        c.material = 0x14;
        assert!(collision_damage(&c, 1.0).sparks);
        c.material = 0x1f;
        assert!(collision_damage(&c, 1.0).wreck);
        c.material_respawns = true;
        let o = collision_damage(&c, 1.0);
        assert!(!o.wreck && o.respawn_request);
        c.respawn_locked = true;
        assert!(!collision_damage(&c, 1.0).respawn_request);
        // ignore flag
        c.ignore = true;
        assert!(collision_damage(&c, 1.0).damage.is_none());
    }

    #[test]
    fn stomp_rule() {
        let mut c = CollisionInput::simple(Vec3::new(0.0, -400.0, 0.0), Vec3::ZERO, 285.0, Vec3::Y);
        c.other = OtherBody::Car { has_player: true, height_above: 1.5 };
        c.cgame_31bc = 2;
        let o = collision_damage(&c, 1.0);
        assert_eq!(o.stomp_speed_scale, Some(0.85));
        assert!(!o.stomp_target_self); // AI car: the other (human) car is slowed
        assert!(!o.stomp_wreck);
        assert!(o.damage_done.is_some());
        c.cgame_31bc = 8;
        let o = collision_damage(&c, 1.0);
        assert_eq!(o.stomp_speed_scale, Some(0.75));
        assert!(o.stomp_wreck);
        c.other = OtherBody::Car { has_player: false, height_above: 0.5 };
        let o = collision_damage(&c, 1.0);
        assert!(o.stomp_speed_scale.is_none() && o.damage_done.is_none());
    }

    #[test]
    fn on_collision_end_to_end() {
        let mut b = car();
        let mut ev = Vec::new();
        b.ramp = 1.0;
        // head-on hit at the front, 600 impulse: halved 300 / 285 = 1.0526 lateral impact
        let mut c = CollisionInput::simple(Vec3::new(0.0, 0.0, 600.0), Vec3::new(0.0, 0.0, 1.2), 285.0, Vec3::Y);
        c.material = 0x12;
        let o = b.on_collision(&c, Vec3::ZERO, Quat::IDENTITY, &mut [], &mut ev);
        let d = o.damage.unwrap();
        assert!((d - (300.0 / 285.0 - 0.3)).abs() < 1e-4);
        // front + right? x == 0 -> not > 0 -> Left
        assert!((b.side_damage[0] - d * 0.5).abs() < 1e-5);
        assert!((b.side_damage[2] - d * 0.5).abs() < 1e-5);
        assert_eq!(b.side_damage[1], 0.0);
        assert!((b.impact_shake - 300.0 / 285.0).abs() < 1e-4);
        assert!(ev.iter().any(|e| matches!(e, DamageEvent::Sparks { .. })));
        // shake output: 1.0526*50
        let s = b.cam_shake_mod();
        assert!((s - 300.0 / 285.0 * 50.0).abs() < 1e-3);
        // wrecked by the kill material
        let mut c2 = c.clone();
        c2.material = 0x1f;
        b.on_collision(&c2, Vec3::ZERO, Quat::IDENTITY, &mut [], &mut ev);
        assert!(b.side_damage.iter().all(|v| v.is_infinite()));
        assert_eq!([b.wheel_state(0), b.wheel_state(1), b.wheel_state(2), b.wheel_state(3)], [3; 4]);
    }

    #[test]
    fn shake_curve_and_decay() {
        let mut s = 3.0;
        assert_eq!(impact_cam_shake_mod(&mut s), 100.0);
        assert_eq!(s, 2.0);
        let mut s = 1.0;
        assert_eq!(impact_cam_shake_mod(&mut s), 50.0);
        let mut s = -0.5;
        assert_eq!(impact_cam_shake_mod(&mut s), 0.0);
        assert_eq!(s, 0.0);
        let mut b = car();
        b.impact_shake = 1.5;
        let mut ev = Vec::new();
        b.update(0.1, 1.0, &mut ev);
        assert!((b.impact_shake - 1.2).abs() < 1e-5);
        b.update(0.1, 0.5, &mut ev); // slow motion: dt/slowmo
        assert!((b.impact_shake - 0.6).abs() < 1e-5);
    }

    #[test]
    fn hp_meter() {
        assert_eq!(add_hp_damage(10.0, 5.0, true), 15.0);
        assert_eq!(add_hp_damage(78.0, 5.0, true), 80.0);
        assert_eq!(add_hp_damage(80.0, 5.0, true), 80.0);
        assert_eq!(add_hp_damage(3.0, -5.0, true), 0.0);
        assert_eq!(add_hp_damage(10.0, 5.0, false), 10.0);
        assert!(mode_allows_hp_meter(2) && mode_allows_hp_meter(8) && !mode_allows_hp_meter(5));
        assert_eq!(hp_sparks_intensity(75.0), None);
        assert_eq!(hp_sparks_intensity(80.0), Some(250.0 + 55.0 * 10.0));
        let mut b = car();
        let mut ev = Vec::new();
        b.add_hp(30.0, 2, &mut ev);
        assert_eq!(b.hp_meter, 30.0);
        b.add_hp(30.0, 4, &mut ev);
        assert_eq!(b.hp_meter, 30.0);
    }

    #[test]
    fn wear_and_ramp() {
        // dt 0.1, fragility 0.5, speed 20, 4 wheels on the ground, wear sum 2.0 -> w = 20/4*2*0.5 = 5, d = 0.5
        assert_eq!(wear_damage(0.1, 0.5, 20.0, 4, 2.0, false), (0.5, 5.0));
        assert_eq!(wear_damage(0.1, 0.5, 20.0, 4, 2.0, true).0, 0.0);
        assert_eq!(wear_damage(0.1, 0.5, 20.0, 0, 2.0, false), (0.0, 0.0));
        let mut b = car();
        let mut ev = Vec::new();
        assert_eq!(b.ramp, 0.25);
        let d = b.wear_step(0.1, 20.0, 4, 2.0, &mut ev);
        assert!((d - 0.5).abs() < 1e-6);
        assert_eq!(b.side_damage, [0.5; 4]);
        assert!((b.ramp - 0.30).abs() < 1e-6);
        for _ in 0..30 {
            b.wear_step(0.1, 0.0, 4, 0.0, &mut ev);
        }
        assert_eq!(b.ramp, 1.0);
        // airborne: nothing changes
        let r = b.ramp;
        b.wear_step(1.0, 20.0, 0, 2.0, &mut ev);
        assert_eq!(b.ramp, r);
        assert_eq!(b.wear, 0.0); // airborne: the wear factor is cleared
    }

    #[test]
    fn spring_model() {
        let b = car();
        let s = &b.parts[0].spec;
        assert_eq!(spring_softening(s, 0.0, 1.0, false), (1.0, 1.0));
        let (k, c) = spring_softening(s, 16.5, 1.0, false); // r = 0.5
        assert!((k - 0.75).abs() < 1e-6 && (c - 0.625).abs() < 1e-6);
        assert_eq!(spring_softening(s, 1000.0, 1.0, false), (0.5, 0.25));
        assert_eq!(spring_softening(s, 1000.0, 1.0, true), (1.0, 1.0));
        assert_eq!(spring_softening(s, -5.0, 1.0, false), (1.0, 1.0));
        let f = spring_point_force(2.0, 0.5, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0), Vec3::ZERO);
        assert_eq!(f, Vec3::new(2.0, 1.0, 0.0));
        // below the threshold: no damage; above: excess
        let mut i = SpringDamageInput {
            force_sum: Vec3::new(0.0, 0.0, 10.0),
            up: Vec3::Y,
            dt: 0.01,
            ramp: 1.0,
            fragility: 0.5,
            is_pilot: false,
            scale534: 1.0,
            part_mass: 6.0,
            wear: 0.0,
            no_damage: false,
        };
        let r = spring_damage_step(s, &i);
        assert_eq!(r.add_damage, 0.0); // thr = 0.01*3500 = 35 > 10
        i.force_sum = Vec3::new(0.0, 0.0, 100.0);
        let r = spring_damage_step(s, &i);
        assert!((r.add_damage - 2.0 * (100.0 - 35.0)).abs() < 1e-3);
        assert_eq!(r.nuts, Some(16)); // (65 * 0.25) = 16.25
        i.no_damage = true;
        assert_eq!(spring_damage_step(s, &i).add_damage, 0.0);
        assert!(spring_damage_enabled(0.2, false, false, false, 5, false));
        assert!(!spring_damage_enabled(0.05, false, false, false, 5, false));
        assert!(!spring_damage_enabled(0.2, false, false, false, 3, false));
    }

    #[test]
    fn visual_damage_lifecycle() {
        let mut b = car();
        let mut ev = Vec::new();
        b.check_visual_damage(5, &mut ev); // mode 5 skips
        assert!(ev.is_empty());
        b.check_visual_damage(1, &mut ev);
        assert_eq!(ev.len(), 3);
        for i in 0..3 {
            assert!(matches!(b.visual_damage(i), DamageState::AttachedA | DamageState::AttachedB));
        }
        ev.clear();
        b.break_bodywork(0, &mut ev);
        assert_eq!(b.visual_damage(0), DamageState::Detached);
        b.part_damage[2] = 7.0;
        b.side_damage = [5.0, 5.0, 5.0, 5.0];
        ev.clear();
        b.full_repair(&mut ev);
        assert_eq!(b.side_damage, [0.0; 4]);
        assert_eq!(b.part_damage[2], 0.0);
        assert!((0..3).all(|i| b.visual_damage(i) == DamageState::NoBody));
        // part 0 was detached: no PartRemoved (debris stays), parts 1 and 2 removed
        let removed: Vec<_> = ev.iter().filter_map(|e| if let DamageEvent::PartRemoved { part } = e { Some(*part) } else { None }).collect();
        assert_eq!(removed, vec![1, 2]);
        assert!(ev.contains(&DamageEvent::Repaired));
    }

    #[test]
    fn restore_damage_state_subsets() {
        let mut wing = part("spoiler", false, None);
        wing.spec.down_force = -0.08;
        let mut b = Bodywork::new(vec![part("kartnose", false, None), wing, part("pilot", true, None)], 1.0);
        let mut ev = Vec::new();
        b.check_visual_damage(1, &mut ev);
        b.part_damage = [9.0; 12];
        b.side_damage = [3.0; 4];
        ev.clear();
        b.restore_damage_state(false, &mut ev); // Respawn(-1): pilot + wing only
        assert_eq!(b.part_damage[0], 9.0);
        assert_eq!(b.part_damage[1], 0.0);
        assert_eq!(b.part_damage[2], 0.0);
        assert_eq!(b.side_damage, [0.0; 4]);
        let made: Vec<_> = ev.iter().filter_map(|e| if let DamageEvent::PartMaterialised { part, .. } = e { Some(*part) } else { None }).collect();
        assert_eq!(made, vec![1, 2]);
        b.restore_damage_state(true, &mut ev);
        assert_eq!(b.part_damage[0], 0.0);
        assert!((0..3).all(|i| b.visual_damage(i) != DamageState::NoBody));
    }

    #[test]
    fn autorepair_flow() {
        let Some(a) = assets() else { return };
        let x = std::fs::read_to_string(a.join("xml_gameplay/misc/debugtweakables.xml")).unwrap();
        let params = AutoRepairParams::from_debug_tweakables_xml(&x).unwrap(); // 2.0 / 1.0 / 1
        let mut b = car();
        b.autorepair = params;
        let mut ev = Vec::new();
        b.check_visual_damage(1, &mut ev);
        // two wheels damaged > 2 -> count 2 > 1
        b.side_damage = [5.0, 0.0, 5.0, 5.0];
        assert_eq!(b.damaged_piece_count(), 2);
        // not active: nothing happens
        b.autorepair_update(10.0, false, &mut ev);
        assert!(!b.ar_running);
        // 1.5 s < start delay 2.0
        b.autorepair_update(1.5, true, &mut ev);
        assert!(!b.ar_running);
        b.autorepair_update(0.6, true, &mut ev); // idle 2.1 > 2.0 -> starts
        assert!(b.ar_running);
        ev.clear();
        b.autorepair_update(0.9, true, &mut ev); // timer 0.9 <= 1.0
        assert!(b.side_damage[0] > 0.0);
        b.autorepair_update(0.2, true, &mut ev); // 1.1 > 1.0 -> repaired
        assert_eq!(b.side_damage, [0.0; 4]);
        assert!(!b.ar_running);
        assert!(ev.contains(&DamageEvent::Repaired));
        // like the original, the slots stay in state 0 until game-mode code calls `check_visual_damage`
        assert!((0..3).all(|i| b.visual_damage(i) == DamageState::NoBody));
        assert!(!ev.iter().any(|e| matches!(e, DamageEvent::PartMaterialised { .. })));
        // a hit resets the idle timer
        b.ar_idle = 5.0;
        let mut ev2 = Vec::new();
        b.add_impact_damage(Vec3::Z, Vec3::ZERO, Quat::IDENTITY, 1.0, true, &mut [], &mut ev2);
        assert_eq!(b.ar_idle, 0.0);
        // one damaged piece only (count 1, needs > 1) -> idle reset, no repair
        b.side_damage = [5.0, 0.0, 5.0, 0.0];
        assert_eq!(b.damaged_piece_count(), 1);
        b.autorepair_update(2.5, true, &mut ev2);
        assert!(!b.ar_running);
        assert_eq!(b.ar_idle, 0.0);
        // pilot detached: autorepair does nothing
        b.side_damage = [5.0, 5.0, 5.0, 5.0];
        b.detach_pilot(&mut ev2);
        b.autorepair_update(5.0, true, &mut ev2);
        assert!(!b.ar_running);
        assert_eq!(b.side_damage, [5.0; 4]);
    }

    #[test]
    fn world_to_local_uses_inverse_rotation() {
        // car yawed +90 deg about Y (R: x->-z, z->x); local = R^-1 * world: world +X -> local +Z, world +Z -> local -X
        let q = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let l = world_to_car_local(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, q);
        assert!((l - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-5);
        let l = world_to_car_local(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, q);
        assert!((l - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn detached_thrust_events() {
        let mut p = part("rocket", false, None);
        p.spec.detached_thrust = 5000.0;
        let mut b = Bodywork::new(vec![p], 1.0);
        let mut ev = Vec::new();
        b.update(0.1, 1.0, &mut ev);
        assert!(ev.is_empty());
        b.break_bodywork(0, &mut ev);
        ev.clear();
        b.update(0.1, 1.0, &mut ev);
        assert_eq!(
            ev,
            vec![DamageEvent::DetachedThrust { part: 0, force_local: Vec3::new(0.0, 0.0, 500.0), at_local: Vec3::new(0.0, 0.0, -0.3) }]
        );
    }

    #[test]
    fn effects_mapping() {
        let ev = vec![DamageEvent::Repaired, DamageEvent::PilotEjected { part: 1 }];
        let fx = events_to_car_effects(3, Vec3::ZERO, &ev);
        assert!(matches!(fx[0], CarEffect::Other { tag: "CCar::FullRepair", car: Some(3), .. })); // never CarEffect::Repair (would loop)
        assert!(matches!(fx[1], CarEffect::Other { tag: "CCar::DetachPilot", .. }));
    }

    #[test]
    fn wrappers() {
        let mut b = car();
        let c = CollisionInput::simple(Vec3::new(0.0, 0.0, 1000.0), Vec3::new(0.0, 0.0, 1.0), 285.0, Vec3::Y);
        b.ramp = 1.0;
        let (o, ev) = b.apply_impact(&c, Vec3::ZERO, Quat::IDENTITY, &mut []);
        assert!(o.damage.is_some());
        assert!(ev.iter().any(|e| matches!(e, DamageEvent::PilotAnim { state: 5 })));
        assert!(b.side_damage[0] > 0.0);
        assert!(b.tick(0.016).is_empty());
        assert!(b.repair().contains(&DamageEvent::Repaired));
        assert_eq!(b.side_damage, [0.0; 4]);
    }

    #[test]
    fn repair_fraction_partial() {
        let mut b = car();
        let mut ev = Vec::new();
        b.side_damage = [10.0, 10.0, 10.0, 10.0];
        b.repair_fraction(0.5, 1, &mut ev);
        assert_eq!(b.side_damage, [5.0; 4]);
        b.repair_fraction(1.0, 1, &mut ev);
        assert_eq!(b.side_damage, [0.0; 4]);
    }
}
