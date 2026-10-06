//! Race AI drivers of Angry Birds Go! 2.9.1 (`CRaceAI`), ported from the Ghidra decompile + the ARM listing of `libABK291.so`
//! (Ghidra addresses; file offset = address - 0x10000). Standalone: depends on `racerules` (splines / economy), `glam` and
//! `crate::carsim::CarInput` only; no dependency on `drive.rs` / `game.rs`.
//!
//! Ported (address -> function here):
//! * `CRaceAI::CRaceAI @00189ce8`, `Reset @001874d0`, `ResetTimers @001874c0`      -> `RaceAi::new`, `RaceAi::reset`, `reset_timers`
//! * `CRaceAI::Process @0018af08`                                                -> `RaceAi::update` (speed / brake block, rubber band,
//!   slingshot pull + release, stuck / off-line timers, spline switching, ability triggering; speed controller from the listing)
//! * `CRaceAI::AimAtPoint @00187908`                                             -> `RaceAi::aim_at_point` (yaw-rate steering controller, listing)
//! * `CRaceAI::CalcTargetSpeed @00189370` / `Precalc @0018a110`                  -> `calc_target_speed` (corner speed limit + braking pass)
//! * `CRaceAI::GetTargetSpeed @00187cd4`                                         -> `RaceAi::target_speed`
//! * `CRaceAI::AvoidCollisions @00187de0`                                        -> `RaceAi::avoid_collisions` (40-cell lateral time-to-impact grid)
//! * `CRaceAI::SetHotspotTarget @0018c960`                                       -> `RaceAi::set_hotspot_target` (+ `HotspotKind::radius`, `CPickupAIHotspot*::Create`)
//! * `CRaceAI::UpdateRamAI @0018ab24`, `ShouldStartRamming @0018c524`, `IsRamming`, `StopRamming`, `SetAsBoss @0018c92c` -> ram methods
//! * `CRaceAI::GetSteeringAssistValue @0018c65c`                                 -> `RaceAi::steering_assist_value`
//! * `CRaceAI::MatchSpeed @0018a3b0`                                             -> `RaceAi::match_speed`
//! * `CGame::AddAI @00114720` (spline choice by skill-interpolated weights) -> `racerules::RaceTrack::pick_ai_spline`;
//!   `CGameModeManager::InitialiseCars @00183a08` (skill / catch-up) -> `ai_skill`, `catchup_enabled`, `catchup_strength`.
//!
//! The ability / debug constants come from `debugtweakables.xml` (values the game loads in `SetDebugTweakablesFromXML @002c7258`,
//! mapped to the float table index by the store addresses): 0x28 Gravity_Scale 2.0, 0x3f..0x43 Ram_*, 0x44 Ability_Distance,
//! 0x45 Initial_Ability_Delay 4.0, 0x49/0x4a/0x4b AISlingshot Min/Max/RandomOffset, 0x4e Ghost_AI Ability_Chance_Fudge 3.0.
//!
//! # UNRESOLVED (also marked in code)
//! * `CalcTargetSpeed`: the ballistic "jump" pass (crest detection with the 4-sub-step projectile integration, 3 passes) is not
//!   ported; only the corner limit and the braking propagation are. `PhysMaterial_GetPeakGripScale` per material is supplied by the caller.
//! * `AvoidCollisions` (decompile only, not listing-checked): the cell fill / ttc (`gap/(closing+1)`; the second value `gap/closing` is
//!   computed but not used for the cells here), the 4-m clamp of the lookahead and the cell scan follow the decompile; the two cost
//!   weights (`0.07*scale*k` for the distance from the desired cell 19, `0.05*k` from the previous cell, `k = 0.01/scale`) are my reading
//!   of globals whose writers were not traced; the corridor limit cells are approximated.
//!
//! Listing-verified (ARM disassembly): `AimAtPoint` (yaw-rate controller), the speed / brake / acceleration block of `Process` (45, 12.5, 30
//!   m/s^2 pushes, 1.0 / 0.6 brake, 0.2 rad gate, episode-4 air gate), rubber band formula, stuck / off-line timers and their reset, slingshot
//!   release gate (`ms_fCountDownTime / ms_fCountDownGoTime`), grid freeze, hit timer. Decompile only: `CalcTargetSpeed` corner/braking passes,
//!   spline switching, ability triggering, `UpdateRamAI`.
//! * `CGame+0x3298` (condition for the rubber band, "> 0"): taken as "race clock > 0".
//! * Boss abilities (`CCar::GetBossAbility`, `LoadBossAbilities`), `CBaseAbility::ShouldGhostUseAI`, `CCar::IsCarOnMyTeam`.
//! * The car `+0x1b54` last-hit-by timer (`+0x150`) of `Process` start.
#![allow(dead_code, clippy::too_many_arguments, clippy::needless_range_loop)]

use crate::carsim::CarInput;
use crate::racerules::{Economy, GameMode, Phase, RaceTrack, Rng, SplineKind};
use glam::{Quat, Vec3};

// =============================================================================================================
// Tweakables (debugtweakables.xml values)
// =============================================================================================================

#[derive(Clone, Copy, Debug)]
pub struct AiTweaks {
    /// `Gameplay/Gravity_Scale` = debug float 0x28 (2.0 in the shipped file; the code default is 1.0)
    pub gravity_scale: f32,
    /// `AISlingshotMinDelay / MaxDelay / MaxRandomOffset` = 0x49 / 0x4a / 0x4b (non-boss); the boss uses 0x46/0x47 (Catapult_Min/Max_Delay)
    pub sling_min_delay: f32,
    pub sling_max_delay: f32,
    pub sling_random_offset: f32,
    pub catapult_min_delay: f32,
    pub catapult_max_delay: f32,
    /// `BossBattle/Initial_Ability_Delay` = 0x45
    pub initial_ability_delay: f32,
    /// `BossBattle/Ram_Interval, Ram_Duration, Ram_Distance_Behind, Ram_Distance_Ahead, Ram_Sharpness` = 0x3f..0x43
    pub ram_interval: f32,
    pub ram_duration: f32,
    pub ram_distance_behind: f32,
    pub ram_distance_ahead: f32,
    pub ram_sharpness: f32,
    /// `Ghost_AI/Ability_Chance_Fudge` = 0x4e
    pub ability_chance_fudge: f32,
}
impl Default for AiTweaks {
    fn default() -> Self {
        AiTweaks {
            gravity_scale: 2.0,
            sling_min_delay: 0.0,
            sling_max_delay: 0.3,
            sling_random_offset: 0.1,
            catapult_min_delay: 0.0,
            catapult_max_delay: 0.0,
            initial_ability_delay: 4.0,
            ram_interval: 3.0,
            ram_duration: 1.5,
            ram_distance_behind: 5.0,
            ram_distance_ahead: 2.0,
            ram_sharpness: 0.8,
            ability_chance_fudge: 3.0,
        }
    }
}

// =============================================================================================================
// Inputs
// =============================================================================================================

/// The few `CCarSpec` / rigid-body values the AI reads.
#[derive(Clone, Debug)]
pub struct CarSpecAi {
    /// rigid body mass (`CXGSRigidBody+0x8c`)
    pub mass: f32,
    /// `CCarSpec+0x430` m_fDownForce
    pub down_force: f32,
    /// `CCarSpec` wheel `fPeakGrip` (`+0x13c + 0x54*i`)
    pub wheel_peak_grip: Vec<f32>,
    /// `CCarSpec+0x438` m_fMinDesiredSpeed (TopSpeed counter threshold; not used by the AI itself)
    pub min_desired_speed: f32,
    /// `CCarSpec+0x450` m_fSteeringSpeedScale (guarded: carsim leaves it 0 until the base spec is loaded)
    pub steering_speed_scale: f32,
    /// `CCarSpec+0x46c` m_fAirborneSteerScale
    pub airborne_steer_scale: f32,
    /// wheel `+0x4c` (front axle z offset used as the steering reference point)
    pub front_axle_offset: f32,
    /// body matrix `+0x44` / `+0x3c` extents used for the lateral clearance of `AvoidCollisions`
    pub half_width: f32,
}
impl Default for CarSpecAi {
    fn default() -> Self {
        CarSpecAi {
            mass: 285.0,
            down_force: 1.0,
            wheel_peak_grip: vec![0.73; 4],
            min_desired_speed: 30.0,
            steering_speed_scale: 30.0,
            airborne_steer_scale: 0.0,
            front_axle_offset: 1.0,
            half_width: 0.9,
        }
    }
}

/// Another kart as the AI sees it (`CGame::GetNearbyCars`).
#[derive(Clone, Debug)]
pub struct Neighbor {
    pub id: usize,
    pub pos: Vec3,
    pub vel: Vec3,
    pub forward: Vec3,
    pub speed: f32,
    pub spline_id: usize,
    pub spline_pos: f32,
    pub lateral: f32,
    pub radius: f32,
    /// `CCar+0x42c` non-collideable timer: karts with >= 5 s left are ignored
    pub noncollide_timer: f32,
    pub is_competitor: bool,
    pub is_player: bool,
}

#[derive(Clone, Debug)]
pub struct Obstacle {
    pub pos: Vec3,
    pub radius: f32,
}

/// Local human player as the rubber band sees it.
#[derive(Clone, Debug)]
pub struct PlayerObs {
    pub speed: f32,
    pub spline_pos: f32,
    pub spline_id: usize,
    pub pos: Vec3,
    /// `CCar+0x4b4 != 0` of the local player's car: the player has pulled / released the slingshot (AI only pulls after that)
    pub started: bool,
}

#[derive(Clone, Debug)]
pub struct AbilityObs {
    /// `ability->IsReady()` (vtbl+0x70)
    pub ready: bool,
    /// charges left (`vtbl+0x74`, or +0x34)
    pub charges: i32,
    /// `ability+0x80`: cooldown (min/max both read the same field in the original)
    pub cooldown: f32,
    /// `vtbl+0x84` trigger-chance factor (default 0)
    pub chance_factor: f32,
}

#[derive(Clone, Debug)]
pub struct CarObs {
    pub pos: Vec3,
    pub vel: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// body orientation + angular velocity (world) for the local yaw rate used by the steering controller
    pub orientation: Quat,
    pub ang_vel: Vec3,
    /// `CCar+0x1ab4` |v|
    pub speed: f32,
    /// `CCar+0x1aac` signed forward speed
    pub forward_speed: f32,
    /// `CCar+0x4f8` wheel steer angle
    pub steer_angle: f32,
    /// `CCar+0x4ec` airborne time
    pub air_time: f32,
    pub wheels_on_ground: i32,
    pub in_slingshot: bool,
    pub running: bool,
    pub pilot_detached: bool,
    /// `CCar+0x1afc` competitor
    pub is_competitor: bool,
    /// `CCar+0x1af0 != 0` (human)
    pub is_player: bool,
    pub radius: f32,
    /// `CCar+0x1b54` (last collided with) is a human player's car and `body+0x2f4 < 0.05`: the AI stops steering for 0.5 s
    pub bumped_by_player: bool,
    /// `CCar+0x1a88/1a8c/1a98` spline id / position / lateral (kept by `racerules::SplineTracker`)
    pub spline_id: usize,
    pub spline_pos: f32,
    pub lateral: f32,
    /// slingshot frame: (right, up, back) axes `CCar::GetSlingshotMatrix` and the current offset `CCar+0x46c..0x474`
    pub slingshot_axes: Option<[Vec3; 3]>,
    pub slingshot_offset: Vec3,
    pub ability: Option<AbilityObs>,
}

impl CarObs {
    pub fn simple(pos: Vec3, vel: Vec3, forward: Vec3) -> CarObs {
        let f = forward.normalize_or_zero();
        let up = Vec3::Y;
        let right = f.cross(up).normalize_or_zero();
        CarObs {
            pos,
            vel,
            forward: f,
            right,
            up,
            orientation: Quat::IDENTITY,
            ang_vel: Vec3::ZERO,
            speed: vel.length(),
            forward_speed: vel.dot(f),
            steer_angle: 0.0,
            air_time: 0.0,
            wheels_on_ground: 4,
            in_slingshot: false,
            running: true,
            pilot_detached: false,
            is_competitor: true,
            is_player: false,
            radius: 1.2,
            bumped_by_player: false,
            spline_id: 0,
            spline_pos: 0.0,
            lateral: 0.0,
            slingshot_axes: None,
            slingshot_offset: Vec3::ZERO,
            ability: None,
        }
    }
}

/// Everything about the race the AI needs besides the track.
pub struct AiCtx<'a> {
    pub phase: Phase,
    pub mode: GameMode,
    /// `CGame+0x2c4` episode index (the episode-4 branch of the lookahead distance)
    pub episode: i32,
    /// `CGameMode+0x1c` race clock in seconds
    pub race_clock_s: f32,
    /// number of human players (`CGame+0x31c0`)
    pub human_count: i32,
    /// `ms_fCountDownTime` / `ms_fCountDownGoTime` (single player: both 0)
    pub countdown_time: f32,
    pub countdown_go_time: f32,
    pub player: Option<PlayerObs>,
    pub neighbors: &'a [Neighbor],
    pub obstacles: &'a [Obstacle],
    pub tweaks: AiTweaks,
    pub economy: &'a Economy,
    pub grip_scale: &'a dyn Fn(i32) -> f32,
}

// =============================================================================================================
// Output
// =============================================================================================================

/// `CCar::SetSlingshotOffset` + `SetUserTouchingSlingshot(1)` + release (`SetInSlingshot(-1)`).
#[derive(Clone, Debug)]
pub struct SlingshotCmd {
    pub offset: Vec3,
    pub release: bool,
}

#[derive(Clone, Debug)]
pub struct AiOutput {
    /// steer (`CCar::SetSteering`) and brake (`CCar::SetBrake`); `steer_angle: Some(0.0)` + `brake 1` = frozen on the grid
    pub input: CarInput,
    /// `CCar::ApplyAcceleration(a, b)` calls: force = `a * mass * b` along the car forward axis (`b` = dt)
    pub accelerations: Vec<(f32, f32)>,
    /// `CCar::Respawn(-1)` (stuck > 2.5 - 0.25 skill seconds)
    pub respawn: bool,
    /// `CCar::SetSplineID`
    pub set_spline: Option<usize>,
    pub slingshot: Option<SlingshotCmd>,
    /// `CCar::TriggerAbility`
    pub trigger_ability: bool,
    /// grid freeze: zero the body velocities (`Process` @0018b868)
    pub frozen: bool,
}
impl AiOutput {
    fn new() -> AiOutput {
        AiOutput {
            input: CarInput::default(),
            accelerations: Vec::new(),
            respawn: false,
            set_spline: None,
            slingshot: None,
            trigger_ability: false,
            frozen: false,
        }
    }
}

// =============================================================================================================
// Skill / catch-up (CGameModeManager::InitialiseCars @00183a08)
// =============================================================================================================

/// AI skill of one opponent: `SkillBase[adj] + rand(SkillVariance)` clamped into the event's `[aiSkillMin, aiSkillMax]`.
pub fn ai_skill(eco: &Economy, adj: usize, range: (f32, f32), rng: &mut Rng) -> f32 {
    let v = eco.skill_base[adj.min(4)] + rng.range(eco.skill_variance.0, eco.skill_variance.1);
    if range.0 <= v {
        v.min(range.1)
    } else {
        range.0
    }
}

/// `+0x1ac`: rubber banding enabled for this AI. Boss battle: `adj >= BossCatchupDifficulty`; others: campaign event without
/// `disableCatchup` (or no campaign data) and `adj >= AICatchupDifficulty`.
pub fn catchup_enabled(mode: GameMode, eco: &Economy, adj: usize, campaign_disable_catchup: Option<bool>) -> bool {
    if mode == GameMode::BossBattle {
        adj >= eco.boss_catchup_difficulty
    } else {
        let enabled = match campaign_disable_catchup {
            Some(d) => !d,
            None => true,
        };
        enabled && adj >= eco.ai_catchup_difficulty
    }
}

/// `+0x1b0`: rubber band strength of the `ai_index`-th opponent (0-based): `1 - clamp(max(n-3, 0)/5 - rand(0, 0.2), 0, 1)`, n = index + 1.
pub fn catchup_strength(ai_index: usize, rng: &mut Rng) -> f32 {
    let n = (ai_index as i32 + 1 - 3).max(0) as f32 / 5.0;
    let v = n - rng.range(0.0, 0.2);
    if v < 0.0 {
        1.0
    } else if v > 1.0 {
        0.0
    } else {
        1.0 - v
    }
}

// =============================================================================================================
// Target speed table (CRaceAI::CalcTargetSpeed @00189370)
// =============================================================================================================

/// Per-node maximum speed along a race line.
///
/// * corner limit `v = sqrt(gripScale(material) * gripBase * a * radius)`, `gripBase = mean(wheel peak grip) * 5`,
///   `a = clamp(9.8 * Gravity_Scale + 30^2 * DownForce / mass, 0.5, 20)` (the speed estimate of the original is 22 -> floored at 30),
///   result floored at 30 and capped at 220 (`fVar3 < 30 -> 30`, array initialised to 220.0);
/// * braking pass (`@0x00189a00..`): `table[i-1] = min(table[i-1], sqrt(min_j(table[i]... table[j+1]^2 + gripScale * gripBase * 9.8 * g * dist)))`
///   over the next 200 m (`DAT_0018975c`), the original stores into the PREVIOUS index (listing-confirmed off-by-one) and works in place.
// UNRESOLVED: the ballistic jump pass (3 passes over a projectile integrated in 4 sub-steps per node) is not ported.
pub fn calc_target_speed(track: &RaceTrack, spline_id: usize, spec: &CarSpecAi, grip_scale: &dyn Fn(i32) -> f32, gravity_scale: f32) -> Vec<f32> {
    let s = &track.splines[spline_id];
    let n = s.count();
    let mut table = vec![220.0f32; n];
    let wheels = spec.wheel_peak_grip.len().max(1) as f32;
    let grip_base = spec.wheel_peak_grip.iter().sum::<f32>() / wheels * 5.0;
    let g = 9.8 * gravity_scale;
    let accel = (g + 30.0 * 30.0 * spec.down_force / spec.mass.max(1.0)).clamp(0.5, 20.0);
    let brake_decel = grip_base * g; // `fVar32`: gripBase * 9.8 * dbg(0x28)
    for i in 0..n {
        let node = &s.nodes[i];
        let gs = if node.phys_material == 0 { 1.0 } else { grip_scale(node.phys_material) };
        let v = (gs * grip_base * accel * node.radius).sqrt();
        table[i] = table[i].min(v.max(30.0));
    }
    if s.kind == SplineKind::Race {
        let mut prev: isize = -1;
        for i in 0..n {
            let node = &s.nodes[i];
            let gs = if node.phys_material == 0 { 1.0 } else { grip_scale(node.phys_material) };
            let mut best = table[i] * table[i];
            let mut j = i;
            let mut dist = 0.0f32;
            loop {
                let jn = if j + 1 == n { 0 } else { j + 1 };
                dist += s.nodes[j].seg_len;
                let cand = gs * brake_decel * dist + table[jn] * table[jn];
                if cand < best {
                    best = cand;
                }
                j = jn;
                if dist > 200.0 || j == i {
                    break;
                }
            }
            let tgt = if prev < 0 { n - 1 } else { prev as usize };
            let v = best.sqrt();
            if v < table[tgt] {
                table[tgt] = v;
            }
            prev = i as isize;
        }
    }
    table
}

// =============================================================================================================
// Hotspots (CPickupAIHotspot*)
// =============================================================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HotspotKind {
    Small,
    Medium,
    Large,
    ExtraLarge,
}
impl HotspotKind {
    /// `CPickupAIHotspot*::Create @001e6430..`: object `+0x78` radius 25 / 35 / 50 / 65.
    pub fn radius(self) -> f32 {
        match self {
            HotspotKind::Small => 25.0,
            HotspotKind::Medium => 35.0,
            HotspotKind::Large => 50.0,
            HotspotKind::ExtraLarge => 65.0,
        }
    }
    /// `TrackItem helpername="ai_hotspot_small|medium|large|extralarge"`
    pub fn from_helper(name: &str) -> Option<HotspotKind> {
        match name {
            "ai_hotspot_small" => Some(HotspotKind::Small),
            "ai_hotspot_medium" => Some(HotspotKind::Medium),
            "ai_hotspot_large" => Some(HotspotKind::Large),
            "ai_hotspot_extralarge" => Some(HotspotKind::ExtraLarge),
            _ => None,
        }
    }
}

// =============================================================================================================
// The driver
// =============================================================================================================

pub const CELLS: usize = 40; // 0x28
const NO_HIT: f32 = 5.0;

#[derive(Clone, Debug)]
pub struct AiParams {
    /// `CRaceAI(car, in_r2)`: base skill (the jitter -0.02..0.02 is added and clamped to 0..1 by the constructor)
    pub skill: f32,
    pub boss: bool,
    pub catchup: bool,
    pub catchup_strength: f32,
    /// `CCar+0x1a88` initial spline id (`CGame::AddAI`)
    pub spline_id: usize,
    pub seed: u32,
}
impl Default for AiParams {
    fn default() -> Self {
        AiParams { skill: 0.5, boss: false, catchup: false, catchup_strength: 1.0, spline_id: 0, seed: 1 }
    }
}

#[derive(Clone, Debug)]
pub struct RaceAi {
    // `+0x12c` ability (ctor param), `+0x130` skill
    pub ability: f32,
    pub skill: f32,
    /// `+0x134` rubber band value (-0.8..0.8)
    pub band: f32,
    /// `+0x144` stuck timer, `+0x148` off-line timer, `+0x14c` pause after a line switch, `+0x150` hit timer
    pub t_stuck: f32,
    pub t_offline: f32,
    pub t_switch_pause: f32,
    pub t_hit: f32,
    /// last steering command (kept while the AI is not allowed to steer)
    pub last_steer: f32,
    /// `+0x154` braking figure (`gripBase * 9.8 * g * 1.6`)
    pub brake_figure: f32,
    /// `+0x158` chosen lateral cell (-1 = none)
    pub cell: i32,
    /// `+0x15c` spline id forced by a nearby competitor (`AvoidCollisions`), -1 = none
    pub forced_spline: i32,
    /// hotspot: `+0x160` active, `+0x164` armed, `+0x168..0x170` target, `+0x174` lateral, `+0x178` d^2, `+0x17c` timer, `+0x180` spline pos, `+0x184` re-arm timer
    pub hotspot_active: bool,
    pub hotspot_armed: bool,
    pub hotspot_target: Vec3,
    pub hotspot_lateral: f32,
    pub hotspot_d2: f32,
    pub hotspot_timer: f32,
    pub hotspot_spline_pos: f32,
    pub hotspot_rearm: f32,
    /// slingshot: `+0x188` lateral, `+0x18c` vertical, `+0x190` pull, `+0x194` release delay
    pub sling_lateral: f32,
    pub sling_vertical: f32,
    pub sling_pull: f32,
    pub sling_delay: f32,
    /// `+0x198` lookahead distance, `+0x19c` lateral target, `+0x1a0` target speed, `+0x1a4` cell-stuck counter
    pub lookahead: f32,
    pub lateral_target: f32,
    pub target_speed: f32,
    pub cell_counter: i32,
    /// `+0x1a8` boss, `+0x1ac` catch-up enabled, `+0x1b0` strength
    pub boss: bool,
    pub catchup: bool,
    pub catchup_strength: f32,
    /// ram: `+0x1b4` target present, `+0x1b8` ramming, `+0x1bc` interval timer, `+0x1c0` ram time left
    pub ram_target: Option<usize>,
    pub ramming: bool,
    pub ram_interval_timer: f32,
    pub ram_time_left: f32,
    /// abilities: `+0x1c4` timer, `+0x1c8/1cc/1d0` boss ability timings, `+0x1d4` boss ability timer, `+0x1d8` ability cooldown
    pub ability_timer: f32,
    pub boss_ability_timer: f32,
    pub ability_cooldown: f32,
    /// `+0x1dc` spline-switch cooldown
    pub switch_cooldown: f32,
    /// speed tables per spline id (`+0x2c + 4*id`)
    pub speed_tables: Vec<Vec<f32>>,
    pub spec: CarSpecAi,
    prev_cell: i32,
    rng: Rng,
}

impl RaceAi {
    /// `CRaceAI::CRaceAI @00189ce8` (+ `Reset`): builds the per-spline speed tables (`CalcTargetSpeed` for every spline).
    pub fn new(params: AiParams, track: &RaceTrack, spec: CarSpecAi, tw: &AiTweaks, grip_scale: &dyn Fn(i32) -> f32) -> RaceAi {
        let mut rng = Rng::new(params.seed);
        let jitter = rng.range(-0.02, 0.02);
        let skill = (params.skill + jitter).clamp(0.0, 1.0);
        let tables = (0..track.splines.len()).map(|i| calc_target_speed(track, i, &spec, grip_scale, tw.gravity_scale)).collect();
        let wheels = spec.wheel_peak_grip.len().max(1) as f32;
        let grip_base = spec.wheel_peak_grip.iter().sum::<f32>() / wheels * 5.0;
        let mut ai = RaceAi {
            ability: params.skill,
            skill,
            band: 0.0,
            t_stuck: 0.0,
            t_offline: 0.0,
            t_switch_pause: 0.0,
            t_hit: 0.0,
            last_steer: 0.0,
            brake_figure: grip_base * 9.8 * tw.gravity_scale * 1.6,
            cell: -1,
            forced_spline: -1,
            hotspot_active: false,
            hotspot_armed: true,
            hotspot_target: Vec3::ZERO,
            hotspot_lateral: 0.0,
            hotspot_d2: 0.0,
            hotspot_timer: 0.0,
            hotspot_spline_pos: 0.0,
            hotspot_rearm: 1.0,
            sling_lateral: 0.0,
            sling_vertical: 0.0,
            sling_pull: 0.0,
            sling_delay: 0.0,
            lookahead: 8.0,
            lateral_target: 0.0,
            target_speed: 0.0,
            cell_counter: 0,
            boss: params.boss,
            catchup: params.catchup,
            catchup_strength: params.catchup_strength,
            ram_target: None,
            ramming: false,
            ram_interval_timer: 0.0,
            ram_time_left: 0.0,
            ability_timer: tw.initial_ability_delay,
            boss_ability_timer: tw.initial_ability_delay,
            ability_cooldown: 0.0,
            switch_cooldown: 0.0,
            speed_tables: tables,
            spec,
            prev_cell: -1,
            rng,
        };
        ai.spline_id_hint(params.spline_id);
        ai.reset(tw);
        ai
    }

    fn spline_id_hint(&mut self, _id: usize) {}

    /// `CRaceAI::ResetTimers @001874c0`
    pub fn reset_timers(&mut self) {
        self.t_stuck = 0.0;
        self.t_offline = 0.0;
        self.t_switch_pause = 0.0;
        self.t_hit = 0.0;
    }

    /// `CRaceAI::Reset @001874d0`: slingshot pull parameters and timers.
    pub fn reset(&mut self, tw: &AiTweaks) {
        self.t_stuck = 0.0;
        self.t_offline = 0.0;
        self.t_switch_pause = 0.0;
        self.t_hit = 0.0;
        self.hotspot_active = false;
        self.hotspot_armed = true;
        self.hotspot_rearm = 1.0;
        self.hotspot_d2 = 0.0;
        self.hotspot_spline_pos = 0.0;
        self.cell = -1;
        self.cell_counter = 0;
        self.ability_timer = tw.initial_ability_delay;
        self.boss_ability_timer = tw.initial_ability_delay;
        self.ability_cooldown = 0.0;
        // slingshot: lateral -0.25..0.25, vertical -1.2..0.5, pull (skill*0.2 + 1) * 8.4 (the 8.4 m maximum pull of the player)
        self.sling_lateral = self.rng.range(-0.25, 0.25);
        self.sling_vertical = -1.2 + self.rng.unit() * 1.7;
        self.sling_pull = (self.skill * 0.2 + 1.0) * 8.4;
        // release delay: lerp(max, min, skill) + rand(+-offset), clamped into [min, max] (boss uses the Catapult delays)
        let (lo, hi) = if self.boss { (tw.catapult_min_delay, tw.catapult_max_delay) } else { (tw.sling_min_delay, tw.sling_max_delay) };
        let d = hi + (lo - hi) * self.skill + self.rng.range(-tw.sling_random_offset, tw.sling_random_offset);
        self.sling_delay = if lo <= d { d.min(hi) } else { lo };
    }

    pub fn set_as_boss(&mut self) {
        self.boss = true;
    }

    // ---- helpers ------------------------------------------------------------------------------------------

    /// `GetTargetSpeed @00187cd4`: interpolated table entry scaled by `clamp((100 - |lateral|) * 0.01, 0.5, 1)`.
    pub fn target_speed_at(&self, spline_id: usize, pos: f32, lateral: f32, node_count: usize) -> f32 {
        let t = &self.speed_tables[spline_id];
        let i = (pos as usize).min(node_count - 1);
        let nxt = if i + 1 < node_count { i + 1 } else { 0 };
        let f = pos - i as f32;
        let k = ((100.0 - lateral.abs()) * 0.01).clamp(0.5, 1.0);
        (t[i] + (t[nxt] - t[i]) * f) * k
    }

    /// local yaw rate = y component of `R^T * w` (`AimAtPoint` @00187a1c..)
    pub fn local_yaw_rate(q: Quat, w: Vec3) -> f32 {
        let l = q.conjugate() * w;
        l.y
    }

    /// `CRaceAI::AimAtPoint @00187908` (listing). Returns (raw angle, steer command in -1..1).
    pub fn aim_at_point(&self, obs: &CarObs, track: &RaceTrack, target: Vec3, scale: f32) -> (f32, f32) {
        let airborne = self.spec.airborne_steer_scale > 0.0 && obs.air_time > 0.2;
        let off = if airborne { 0.0 } else { self.spec.front_axle_offset };
        let mut d = target - (obs.pos + obs.forward * off);
        if airborne {
            // project out the component along n = up-in-the-vertical-plane of the spline direction at the car (listing @00187b70)
            if let Some(s) = track.splines.get(obs.spline_id) {
                let node = &s.nodes[(obs.spline_pos as usize).min(s.count() - 1)];
                let (x, y, z) = (node.dir.x, node.dir.y, node.dir.z);
                let n = Vec3::new(x * y, -(x * x + z * z), z * y);
                if n.length_squared() > 1e-12 {
                    let nh = n.normalize();
                    d -= nh * d.dot(nh);
                }
            }
        }
        let fwd = d.dot(obs.forward);
        let lat = d.dot(obs.right);
        let angle = if fwd > 1e-5 { (lat / fwd).atan() } else if lat <= 0.0 { -std::f32::consts::FRAC_PI_2 } else { std::f32::consts::FRAC_PI_2 };
        let yaw = Self::local_yaw_rate(obs.orientation, obs.ang_vel);
        let same_sign = angle * yaw > 0.0;
        let steer;
        if airborne {
            // in the air: factor = same sign ? (up.y >= 0 ? (up.y - 0.5) * 2 : -1) : (the previous s15 = local yaw), steer = clamp(angle * factor * 3)
            let factor = if same_sign { if obs.up.y >= 0.0 { (obs.up.y - 0.5) * 2.0 } else { -1.0 } } else { yaw };
            steer = (angle * factor * 3.0).clamp(-1.0, 1.0);
        } else {
            let mut a = angle;
            if same_sign {
                // damping when already rotating fast: divide by (|yaw| - 2.2 + 1 if |yaw| >= 2.2 else 1)
                let k = yaw.abs() - 2.2;
                let div = if k >= 0.0 { k + 1.0 } else { 1.0 };
                a /= div;
            }
            let ss = if self.spec.steering_speed_scale > 1e-6 { self.spec.steering_speed_scale } else { 30.0 };
            let raw = 2.0 * a * (obs.speed / ss) - yaw;
            steer = (raw * 0.7).clamp(-1.0, 1.0);
        }
        let cmd = if obs.pilot_detached { 0.0 } else { steer * scale };
        (angle, cmd)
    }

    /// `CRaceAI::GetSteeringAssistValue @0018c65c` (the angle to a point 30 m ahead of the lookahead; sets lookahead 30). Returns the
    /// angle in radians (0 when slower than 5 m/s).
    pub fn steering_assist_value(&mut self, obs: &CarObs, track: &RaceTrack, lookahead_m: f32) -> f32 {
        if obs.speed < 5.0 {
            return 0.0;
        }
        self.lookahead = 30.0;
        let Some(s) = track.splines.get(obs.spline_id) else { return 0.0 };
        let (p, _) = s.lookahead(obs.spline_pos, lookahead_m);
        let (target, _) = s.info(p, 0.0);
        let airborne = self.spec.airborne_steer_scale > 0.0 && obs.air_time > 0.2;
        let off = if airborne { 0.0 } else { self.spec.front_axle_offset };
        let d = target - (obs.pos + obs.forward * off);
        let fwd = d.dot(obs.forward);
        let lat = d.dot(obs.right);
        if fwd <= 1e-5 {
            return 0.0;
        }
        (lat / fwd).atan()
    }

    // ---- hotspots -----------------------------------------------------------------------------------------

    /// `CRaceAI::SetHotspotTarget @0018c960` (called by `CPickupAIHotspot::OnCarInRadius`): aim at `pos` when it is ahead of the
    /// car and `|angle| * speed < 30`. Returns true when accepted.
    pub fn set_hotspot_target(&mut self, obs: &CarObs, track: &RaceTrack, pos: Vec3) -> bool {
        if !self.hotspot_armed {
            return false;
        }
        let off = self.spec.front_axle_offset;
        let d = pos - (obs.pos + obs.forward * off);
        let fwd = d.dot(obs.forward);
        let angle = if fwd > 1e-5 { (d.dot(obs.right) / fwd).atan().abs() } else { std::f32::consts::FRAC_PI_2 };
        if angle * obs.speed >= 30.0 {
            return false;
        }
        self.hotspot_active = true;
        self.hotspot_target = pos;
        self.hotspot_timer = 2.0;
        let dd = pos - obs.pos;
        self.hotspot_d2 = dd.length_squared();
        if let Some(s) = track.splines.get(obs.spline_id) {
            let (p, _) = s.closest_pos(pos);
            self.hotspot_spline_pos = p;
            self.hotspot_lateral = s.lateral_offset(p, pos);
        }
        true
    }

    // ---- ramming (boss / ghost modes) ---------------------------------------------------------------------

    pub fn is_ramming(&self) -> bool {
        self.ram_target.is_some() && self.ram_time_left > 0.0
    }
    pub fn stop_ramming(&mut self) {
        self.ram_target = None;
        self.ramming = false;
        self.ram_time_left = 0.0;
    }

    /// `ShouldStartRamming @0018c524` (listing-free): `dist` = signed spline distance AI -> candidate; accept when it lies inside
    /// `(-Ram_Distance_Behind, Ram_Distance_Ahead)` and the lateral gap is < 1.5 m.
    pub fn should_start_ramming(&self, tw: &AiTweaks, signed_dist_to_target: f32, lateral_gap: f32) -> bool {
        signed_dist_to_target < tw.ram_distance_ahead && -tw.ram_distance_behind < signed_dist_to_target && lateral_gap < 1.5
    }

    /// `CRaceAI::UpdateRamAI @0018ab24` (subset): the interval timer + duration bookkeeping; the candidate search is done by the caller
    /// with `should_start_ramming`. Call once per frame for boss AIs.
    pub fn update_ram_timers(&mut self, tw: &AiTweaks, dt: f32) {
        if self.ram_target.is_some() && self.ram_time_left > 0.0 {
            self.ram_time_left -= dt;
            if self.ram_time_left <= 0.0 {
                self.stop_ramming();
            }
        } else {
            self.ram_interval_timer -= dt;
        }
        let _ = tw;
    }
    pub fn start_ramming(&mut self, tw: &AiTweaks, target: usize) {
        self.ram_target = Some(target);
        self.ramming = true;
        self.ram_time_left = tw.ram_duration;
        self.ram_interval_timer = tw.ram_interval;
    }

    /// `CRaceAI::MatchSpeed @0018a3b0` (ghost cars): returns the new speed the car's linear velocity must be rescaled to, or None.
    /// `player_speed_along` = the ghost's speed along its own heading, `gap` = distance behind, `tolerance`.
    pub fn match_speed(own_speed: f32, other_speed_along: f32, a: f32, b: f32, c: f32) -> Option<f32> {
        if other_speed_along < own_speed {
            if a < other_speed_along.sqrt() + (b - own_speed.sqrt()) {
                let t = own_speed - (c + c);
                let v = if other_speed_along <= t { other_speed_along } else { t };
                return Some(if v > own_speed { own_speed } else { v });
            }
            return Some(other_speed_along.sqrt() + b - a);
        }
        None
    }

    // ---- avoidance ----------------------------------------------------------------------------------------

    /// `CRaceAI::AvoidCollisions @00187de0`: fills a 40-cell lateral time-to-impact grid from nearby karts / obstacles, then chooses
    /// the safest cell near the current one; sets `lateral_target` (`+0x19c`), `cell` (`+0x158`), `forced_spline` (`+0x15c`) and caps the
    /// lookahead (`+0x198`).
    pub fn avoid_collisions(&mut self, obs: &CarObs, track: &RaceTrack, ctx: &AiCtx) {
        let Some(s) = track.splines.get(obs.spline_id) else { return };
        let n = s.count();
        let pos = obs.spline_pos;
        let node = &s.nodes[(pos as usize).min(n - 1)];
        let my_dist = s.dist_at(pos);
        let hw = self.spec.half_width;
        let mut ttc = [NO_HIT; CELLS];
        let cell_of = |lat: f32| -> i32 { (((lat + 15.0) * 1.3) as i32).clamp(0, CELLS as i32 - 1) };
        let race_t = ctx.race_clock_s;
        // early in the race the closing speed is scaled up so the AI reacts earlier: scale = time < 5 ? time * 0.2 : 1
        let scale = if race_t < 5.0 { (race_t * 0.2).max(0.01) } else { 1.0 };
        let closing_me = obs.vel.dot(node.dir) + 1.0;
        let mut min_forced: Option<(f32, usize)> = None;
        let fill = |lat: f32, half: f32, t: f32, cells: &mut [f32; CELLS]| {
            let lo = cell_of(lat - half);
            let hi = cell_of(lat + half);
            for c in lo..=hi {
                if cells[c as usize] > t {
                    cells[c as usize] = t;
                }
            }
        };
        // smackables / obstacles (closest 100 within 200 m)
        let mut obs_list: Vec<&Obstacle> = ctx.obstacles.iter().filter(|o| (o.pos - obs.pos).length_squared() < 40000.0).collect();
        obs_list.sort_by(|a, b| (a.pos - obs.pos).length_squared().partial_cmp(&(b.pos - obs.pos).length_squared()).unwrap());
        obs_list.truncate(100);
        for o in obs_list {
            let (op, _) = s.closest_pos(o.pos);
            let lat = s.lateral_offset(op, o.pos);
            let rad = hw + o.radius;
            let long = s.dist_at(op) - my_dist;
            if long >= -rad {
                let gap = (long - rad).max(0.0);
                let t = if closing_me > 0.0 { (gap / closing_me).min(NO_HIT) } else { NO_HIT };
                fill(lat, rad, t, &mut ttc);
            }
        }
        // karts
        for k in ctx.neighbors.iter().filter(|k| k.noncollide_timer < 5.0) {
            let (kp, kd) = if k.spline_id == obs.spline_id {
                (k.spline_pos, s.dist_at(k.spline_pos))
            } else {
                let (np, _) = s.new_pos((obs.spline_pos as usize).min(n - 1), k.pos);
                (np, s.dist_at(np))
            };
            let lat = s.lateral_offset(kp, k.pos);
            let knode = &s.nodes[(kp as usize).min(n - 1)];
            let mut long = kd - my_dist;
            if long > s.length * 0.5 {
                long -= s.length;
            } else if long < -s.length * 0.5 {
                long += s.length;
            }
            let rad = hw + k.radius;
            // two time-to-impact values are computed in the original: `gap / (closing + 1)` (the one that fills the cells, used here) and
            // `gap / closing`; closing = (my velocity along my node) - (its velocity along its node)
            let closing = (closing_me - 1.0) - knode.dir.dot(k.vel);
            let gap = (long - rad).max(0.0);
            let t = if closing + 1.0 > 0.0 { (gap / (closing + 1.0)).min(NO_HIT) } else { NO_HIT };
            if long >= -rad && long < 200.0 {
                fill(lat, rad, t, &mut ttc);
                if k.is_competitor && long.abs() < 10.0 && min_forced.map_or(true, |m| long.abs() < m.0) {
                    min_forced = Some((long.abs(), k.spline_id));
                }
            }
        }
        self.forced_spline = min_forced.map_or(-1, |m| m.1 as i32);
        // corridor limits: cells outside [-(minLeft - hw), minRight - hw] are not allowed
        let look_end = s.lookahead(pos, self.lookahead.max(5.0)).0;
        let left = (s.min_left_width(pos, look_end) - hw).max(0.0);
        let right = (s.min_right_width(pos, look_end) - hw).max(0.0);
        let (lo_cell, hi_cell) = if left + right <= 0.0 { (19, 19) } else { (cell_of(-left), cell_of(right)) };
        let cur = (((obs.lateral + 15.0) * 40.0 / 30.0) as i32).clamp(0, CELLS as i32 - 1).clamp(lo_cell, hi_cell);
        let prev = if self.cell < 0 { cur } else { self.cell.clamp(lo_cell, hi_cell) };
        let safety = |c: i32| -> f32 {
            let t = |i: i32| ttc[i.clamp(0, CELLS as i32 - 1) as usize];
            let mid = if t(c) < NO_HIT { t(c) * 0.5 } else { 2.5 };
            let l = if t(c - 1) < NO_HIT { t(c - 1) * 0.25 } else { 1.25 };
            let r = if t(c + 1) < NO_HIT { t(c + 1) * 0.25 } else { 1.25 };
            mid + l + r
        };
        // UNRESOLVED: weights of the two distance terms (see module doc)
        let k = 0.01 / scale.max(0.01);
        let w_cur = 0.07 * scale * k;
        let w_prev = k * 0.05;
        // the first distance term is measured from the DESIRED cell: 0x13 (19, the race line) when not ramming (the original stores 0x13
        // in a global at @00188f50 and reads it back in the cost); the second from the previous choice (`+0x158`)
        let desired = 0x13;
        let cost = |c: i32| -> f32 { -safety(c) + w_cur * (c - desired).abs() as f32 + w_prev * (c - prev).abs() as f32 };
        let mut best = cur;
        let mut best_cost = cost(cur);
        for k in 1..CELLS as i32 {
            for c in [cur + k, cur - k] {
                if c >= lo_cell && c <= hi_cell {
                    let v = cost(c);
                    if v < best_cost {
                        best_cost = v;
                        best = c;
                    }
                }
            }
        }
        if obs.wheels_on_ground == 0 {
            best = (cur + best) >> 1;
        }
        // `+0x158` = chosen cell; every 76 frames (`+0x1a4 > 0x4b`) it drifts one cell toward the centre cell 19 (the lateral target
        // below uses the undrifted choice)
        self.cell = best;
        self.prev_cell = best;
        self.cell_counter += 1;
        if self.cell_counter > 0x4b {
            self.cell_counter = 0;
            if best < 0x14 {
                if best != 0x13 {
                    self.cell = best + 1;
                }
            } else {
                self.cell = best - 1;
            }
        }
        self.lateral_target = (best as f32 + 0.5) * (1.0 / 1.3) - 15.0;
        // lookahead is capped by speed * min ttc between the current and the chosen cell (at least 4 m)
        let (a, b) = if cur <= best { (cur, best) } else { (best, cur) };
        let mut mt = NO_HIT;
        for c in a..=b {
            mt = mt.min(ttc[c as usize]);
        }
        let cap = if mt < 1.0 { 4.0 } else { (obs.speed * mt).max(4.0) };
        if cap < self.lookahead {
            self.lookahead = cap;
        }
    }

    // ---- the per-frame driver -----------------------------------------------------------------------------

    /// `CRaceAI::Process @0018af08`. `dt` seconds.
    pub fn update(&mut self, dt: f32, obs: &CarObs, track: &RaceTrack, ctx: &AiCtx) -> AiOutput {
        let mut out = AiOutput::new();
        let tw = &ctx.tweaks;
        let Some(sp) = track.splines.get(obs.spline_id) else { return out };
        let node_count = sp.count();

        // not running (finished): steer toward the middle, brake hard (@0018af4c)
        if !obs.running {
            out.input.steer = if obs.lateral > 0.0 { 1.0 } else { -1.0 };
            out.input.brake = 1.0;
            return out;
        }
        out.input.track_direction = Some(sp.nodes[(obs.spline_pos as usize).min(node_count - 1)].dir);

        // `+0x150` (listing @0018b068..): bumped by the human -> 0.5 s without steering; counts down only while not bumped
        if !self.boss && obs.bumped_by_player {
            if self.t_hit <= 0.0 {
                self.t_hit = 0.5;
            }
        } else {
            self.t_hit -= dt;
        }

        // hotspot: aim straight at the pickup for up to 2 s (@0018afb4..)
        if self.hotspot_active {
            self.hotspot_timer -= dt;
            self.hotspot_d2 = (obs.pos - self.hotspot_target).length_squared();
            if !(self.hotspot_timer < 0.0) && !(self.hotspot_d2 < 1.0) && !(obs.spline_pos > self.hotspot_spline_pos) {
                let (_, steer) = self.aim_at_point(obs, track, self.hotspot_target, 1.0);
                out.input.steer = steer;
                return out;
            }
            self.hotspot_active = false;
            self.hotspot_armed = false;
            self.hotspot_rearm = 1.0;
        } else if !self.hotspot_armed {
            self.hotspot_rearm -= dt;
            if self.hotspot_rearm < 0.0 {
                self.hotspot_armed = true;
            }
        }

        // rubber band (@0018b6c4..0018b868)
        let race_running = ctx.race_clock_s > 0.0;
        self.band = 0.0;
        if race_running && ctx.human_count == 1 && obs.is_competitor && (self.ramming || self.catchup) && !obs.is_player {
            if let Some(pl) = &ctx.player {
                let pl_line = track.splines.get(pl.spline_id).unwrap_or(sp);
                let t = (pl.spline_pos / pl_line.finish_pos).clamp(0.0, 1.0);
                let d = sp.signed_distance_from_race_pos(obs.spline_pos, pl.spline_pos);
                let strength = self.catchup_strength;
                let (shift, lo, hi);
                if self.ramming {
                    let lat = self.ram_lateral_gap(obs, ctx).abs();
                    let d2 = d + lat * 0.3;
                    shift = if d2 > 0.0 { 20.0 + d2 } else { d2 - 20.0 };
                    lo = -0.4;
                    hi = 0.4;
                } else {
                    let a = strength * 30.0 + 5.0;
                    let a = a + ((strength * 100.0 - 105.0) - a) * t;
                    shift = a + d;
                    lo = -0.8;
                    hi = 0.8;
                }
                let sd = pl.speed - obs.speed;
                let f26 = if sd < -40.0 {
                    -80.0
                } else if sd <= 20.0 {
                    sd * 2.0
                } else {
                    40.0
                };
                let f29 = if 50.0 - shift.abs() >= 0.0 { (50.0 - shift.abs()) * 0.02 } else { 0.0 };
                self.band = ((shift + f26 * f29) * 0.02).clamp(lo, hi);
            }
        }

        // grid: frozen (game state 5) (@0018b868)
        if ctx.phase == Phase::Grid {
            out.input.brake = 1.0;
            out.input.steer_angle = Some(0.0);
            out.frozen = true;
            return out;
        }

        // slingshot (@0018b0cc..0018b614): only in game state 8 after the player has started
        if obs.in_slingshot {
            if ctx.phase != Phase::Racing || ctx.player.is_none() {
                return out;
            }
            if !ctx.player.as_ref().map_or(false, |p| p.started) {
                return out;
            }
            if let Some([x, y, z]) = obs.slingshot_axes {
                let fwd = (-z).normalize_or_zero();
                let target = x.normalize_or_zero() * self.sling_lateral + fwd * (-self.sling_pull) + y.normalize_or_zero() * self.sling_vertical;
                let cur = obs.slingshot_offset;
                let new = cur + (target - cur) * (dt * 3.5);
                let mut cmd = SlingshotCmd { offset: new, release: false };
                // release when the countdown is over (CountDownTime <= GoTime) and the delay ran out
                if !(ctx.countdown_time > ctx.countdown_go_time) {
                    self.sling_delay -= dt;
                    if self.sling_delay < 0.0 {
                        cmd.release = true;
                    }
                }
                out.slingshot = Some(cmd);
            }
            return out;
        }

        // ---- driving ----
        let airborne = self.spec.airborne_steer_scale > 0.0 && obs.air_time > 0.2;
        let pos = obs.spline_pos;
        // target speed (`+0x1a0`)
        self.target_speed = self.target_speed_at(obs.spline_id, pos, obs.lateral, node_count);
        // lookahead distance (`+0x198`): speed along the line * 0.5 + 2 (max 40), episode 4: speed * 0.6 + 3 (max 50)
        let node = &sp.nodes[(pos as usize).min(node_count - 1)];
        let mut look = if ctx.episode == 4 { (obs.speed * 0.6 + 3.0).min(50.0) } else { (obs.vel.dot(node.dir) * 0.5 + 2.0).min(40.0) };
        let end_pos = sp.lookahead(pos, look).0;
        let w = sp.min_left_width(pos, end_pos).min(sp.min_right_width(pos, end_pos));
        let wf = (w * 0.08 + 0.5).clamp(0.6, 1.0);
        look *= wf;
        // early-race cap: 26 - 4 t for the first 4 s, then 10 m
        let cap = if ctx.race_clock_s < 4.0 { 26.0 - ctx.race_clock_s * 4.0 } else { 10.0 };
        self.lookahead = look.min(cap).min(120.0);

        // spline switching (`+0x1dc` cooldown, @0018bb68..)
        if self.switch_cooldown <= 0.0 {
            let eco = ctx.economy;
            self.switch_cooldown = eco.spline_switch_cooldown.0 + (eco.spline_switch_cooldown.1 - eco.spline_switch_cooldown.0) * self.skill;
            let target = if self.forced_spline >= 0 {
                Some(self.forced_spline as usize)
            } else {
                // weighted pick over the race lines with the skill-interpolated weights
                let total: f32 = track
                    .splines
                    .iter()
                    .filter(|s| s.kind == SplineKind::Race)
                    .map(|s| s.min_ai_weighting + self.skill * (s.max_ai_weighting - s.min_ai_weighting))
                    .sum();
                let r = self.rng.range(0.0, total);
                let mut acc = 0.0;
                let mut pick = None;
                for (i, s) in track.splines.iter().enumerate() {
                    if s.kind == SplineKind::Race {
                        acc += s.min_ai_weighting + self.skill * (s.max_ai_weighting - s.min_ai_weighting);
                        if r <= acc {
                            pick = Some(i);
                            break;
                        }
                    }
                }
                pick
            };
            if let Some(t) = target {
                if t != obs.spline_id && track.splines[t].kind == SplineKind::Race {
                    // only when the two lines are within 0.04 m^2 (0.2 m) at the car and at the lookahead (@0018c02c..)
                    let a0 = self.target_speed_at(obs.spline_id, pos, obs.lateral, node_count);
                    let a1 = self.target_speed_at(t, pos, obs.lateral, node_count);
                    if (a0 - a1).abs() < 1.0 {
                        let other = &track.splines[t];
                        let p0 = sp.position(pos);
                        let p1 = other.position(pos);
                        let q0 = sp.position(sp.lookahead(pos, self.lookahead).0);
                        let q1 = other.position(other.lookahead(pos, self.lookahead).0);
                        if (p0 - p1).length_squared() < 0.04 && (q0 - q1).length_squared() < 0.04 {
                            out.set_spline = Some(t);
                        }
                    }
                }
            }
        }
        self.avoid_collisions(obs, track, ctx);
        self.switch_cooldown -= dt;

        // boss / ghost modes only: ramming
        if self.boss || matches!(ctx.mode, GameMode::Qmr | GameMode::Tmr | GameMode::Lmr) {
            self.update_ram_timers(&ctx.tweaks, dt);
        }

        // ability (@0018b1b4.., non-boss)
        if !self.boss {
            if let Some(ab) = &obs.ability {
                if ab.ready {
                    let ghost = matches!(ctx.mode, GameMode::Qmr | GameMode::Tmr | GameMode::Lmr);
                    if ghost {
                        // chance per frame = dt * Ability_Chance_Fudge * progress * factor * charges > rand01
                        let progress = (pos / sp.finish_pos).clamp(0.0, 1.0);
                        let chance = dt * ctx.tweaks.ability_chance_fudge * progress * ab.chance_factor * ab.charges as f32;
                        if chance > self.rng.unit() {
                            out.trigger_ability = true;
                        }
                    } else if ab.charges > 0 {
                        self.ability_cooldown -= dt;
                        if self.ability_cooldown <= 0.0 {
                            self.ability_cooldown = ab.cooldown;
                            out.trigger_ability = true;
                        }
                    }
                }
            }
        }

        // speed controller (listing @0018b2d4..0018bf74)
        let speed = obs.speed;
        let brake: f32;
        if speed >= self.target_speed {
            // over the target: brake 1.0 (steer angle < 0.2 rad) else 0.6; a negative rubber band raises the floor
            let base = if obs.steer_angle.abs() < 0.2 { 1.0 } else { 0.6 };
            brake = if self.band >= 0.0 { base } else { base.max(-self.band).min(1.0) };
        } else {
            let total = self.band + self.skill;
            let x = if self.ability <= total {
                if total > 1.0 {
                    None
                } else {
                    Some(total)
                }
            } else {
                Some(self.ability)
            };
            let s = match x {
                None => 1.0,
                Some(x) => (x - 0.5) * 2.0,
            };
            if s > 0.0 {
                // `CGame+0x2c4 == 4` (episode 4, SubZero): only while the car has been airborne < 0.1 s (listing @0018b310..0018c334); else always
                if ctx.episode != 4 || obs.air_time < 0.1 {
                    out.accelerations.push((s * 45.0, dt));
                }
            } else if obs.forward_speed > 30.0 {
                out.accelerations.push((s * 30.0, dt));
            }
            if self.band > 0.0 {
                if let Some(pl) = &ctx.player {
                    if pl.speed > speed {
                        out.accelerations.push((self.band * 12.5, dt));
                    }
                }
            }
            brake = 0.0;
        }
        out.input.brake = brake;

        // steering: ram target > hotspot-less normal lookahead point
        let (steer, _) = if let (true, Some(_)) = (self.is_ramming(), self.ram_target) {
            (self.aim_at_point(obs, track, obs.pos + obs.forward * 10.0, tw.ram_sharpness).1, 0.0)
        } else {
            let (lp, _) = sp.lookahead(pos, self.lookahead);
            let (pt, _) = sp.info(lp, self.lateral_target);
            let scale = 1.0;
            (self.aim_at_point(obs, track, pt, scale).1, 0.0)
        };
        let ghost = matches!(ctx.mode, GameMode::Qmr | GameMode::Tmr | GameMode::Lmr);
        if airborne {
            self.t_hit = 0.0;
        }
        if self.t_hit > 0.0 && !ghost {
            out.input.steer = self.last_steer; // @0018b3a4: no steering while recently bumped (non-ghost modes)
        } else {
            out.input.steer = steer;
            self.last_steer = steer;
        }

        // stuck / off-line timers (game state 5 / 8 / 9, listing @0018b93c..0018b9f0 and @0018bd24..)
        if matches!(ctx.phase, Phase::Racing | Phase::Grid | Phase::Finishing) {
            // `+0x148` accumulates while the car is nearly stopped on the ground and `+0x14c` (the pause set below) is <= 0
            if obs.forward_speed < 1.0 && obs.wheels_on_ground > 0 && !(self.t_switch_pause > 0.0) {
                self.t_offline += dt;
            } else {
                self.t_offline = 0.0;
            }
            let limit = 1.7 - self.skill * 0.25;
            if self.t_offline > limit {
                // `+0x14c = 1.25 + rand01 * 0.5`; it is NEVER decremented in Process, only `ResetTimers` (called by `CCar::Respawn`) clears it
                self.t_switch_pause = 1.25 + self.rng.unit() * 0.5;
                // try another race line the car is inside of (smallest |lateral|, height within 4 m)
                let mut best: Option<(usize, f32)> = None;
                for (i, s) in track.splines.iter().enumerate() {
                    if i == obs.spline_id || s.kind != SplineKind::Race || s.count() != sp.count() {
                        continue;
                    }
                    if (s.height(pos) - obs.pos.y).abs() < 4.0 {
                        let l = s.lateral_offset(pos, obs.pos);
                        if l <= s.right_width(pos) && -s.left_width(pos) <= l && best.map_or(true, |b| l.abs() < b.1) {
                            best = Some((i, l.abs()));
                        }
                    }
                }
                if let Some((i, _)) = best {
                    out.set_spline = Some(i);
                }
            }
            if obs.speed < 1.0 {
                self.t_stuck += dt;
            } else {
                self.t_stuck = 0.0;
            }
            if self.t_stuck > 2.5 - self.skill * 0.25 {
                out.respawn = true;
                // `CCar::Respawn` calls `CRaceAI::Reset` + `ResetTimers`
                self.reset_timers();
                self.t_stuck = 0.0;
                self.t_offline = 0.0;
            }
        }
        out
    }

    /// Convenience for callers without race context: Racing phase, RACE mode, no neighbours, default tweakables / economy.
    pub fn drive_simple(&mut self, dt: f32, obs: &CarObs, track: &RaceTrack) -> CarInput {
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let ctx = AiCtx {
            phase: Phase::Racing,
            mode: GameMode::Race,
            episode: 0,
            race_clock_s: 10.0,
            human_count: 1,
            countdown_time: 0.0,
            countdown_go_time: 0.0,
            player: None,
            neighbors: &[],
            obstacles: &[],
            tweaks: AiTweaks::default(),
            economy: &eco,
            grip_scale: &f,
        };
        self.update(dt, obs, track, &ctx).input
    }

    fn ram_lateral_gap(&self, _obs: &CarObs, _ctx: &AiCtx) -> f32 {
        // UNRESOLVED: lateral distance to the ram target (needs the target kart; supplied through `Neighbor` by the caller)
        0.0
    }
}

// =============================================================================================================
// Tests
// =============================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::racerules::{RaceTrack, SplineWeight};

    fn straight(n: usize, step: f32) -> RaceTrack {
        let pts: Vec<[f32; 3]> = (0..n).map(|i| [0.0, 0.0, i as f32 * step]).collect();
        let ex: Vec<[f32; 7]> = (0..n).map(|_| [0.0, 1.0, 0.0, 0.0, 10.0, 10.0, 0.0]).collect();
        RaceTrack::from_parts(&[("race_001".to_string(), pts, ex)], &[], &[], None)
    }

    fn curve() -> RaceTrack {
        let n = 80;
        let pts: Vec<[f32; 3]> = (0..n)
            .map(|i| {
                let a = i as f32 / (n - 1) as f32 * std::f32::consts::PI;
                [100.0 * (1.0 - a.cos()), 0.0, 100.0 * a.sin()]
            })
            .collect();
        let ex: Vec<[f32; 7]> = (0..n).map(|_| [0.0, 1.0, 0.0, 0.0, 12.0, 12.0, 0.0]).collect();
        RaceTrack::from_parts(&[("race_001".to_string(), pts, ex)], &[], &[], None)
    }

    fn assets_dir() -> Option<std::path::PathBuf> {
        if let Some(v) = std::env::var_os("ABG_ASSETS292") {
            return Some(std::path::PathBuf::from(v));
        }
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets292");
        p.is_dir().then_some(p)
    }

    fn mk_ai(t: &RaceTrack, skill: f32) -> RaceAi {
        RaceAi::new(AiParams { skill, seed: 3, ..Default::default() }, t, CarSpecAi::default(), &AiTweaks::default(), &|_| 1.0)
    }

    fn ctx<'a>(eco: &'a Economy, nb: &'a [Neighbor], ob: &'a [Obstacle], f: &'a dyn Fn(i32) -> f32) -> AiCtx<'a> {
        AiCtx {
            phase: Phase::Racing,
            mode: GameMode::Race,
            episode: 0,
            race_clock_s: 10.0,
            human_count: 1,
            countdown_time: 0.0,
            countdown_go_time: 0.0,
            player: None,
            neighbors: nb,
            obstacles: ob,
            tweaks: AiTweaks::default(),
            economy: eco,
            grip_scale: f,
        }
    }

    fn car_on(t: &RaceTrack, z: f32, lateral: f32, speed: f32) -> CarObs {
        let s = &t.splines[0];
        let pos = s.pos_from_distance(z);
        let (p, _) = s.info(pos, lateral);
        let mut o = CarObs::simple(p + Vec3::Y * 0.5, Vec3::Z * speed, Vec3::Z);
        o.spline_pos = pos;
        o.lateral = lateral;
        o
    }

    #[test]
    fn speed_table_corner_and_straight() {
        let t = curve();
        let tab = calc_target_speed(&t, 0, &CarSpecAi::default(), &|_| 1.0, 2.0);
        // radius 100 m: v = sqrt(1.0 * 3.65 * 20 * 100) = 85 m/s; the straight tail is capped at 220
        let mid = tab[40];
        assert!(mid > 60.0 && mid < 120.0, "{mid}");
        assert!(tab.iter().all(|v| *v >= 30.0 && *v <= 220.0));
        let s = straight(60, 10.0);
        let tab = calc_target_speed(&s, 0, &CarSpecAi::default(), &|_| 1.0, 2.0);
        assert!(tab.iter().all(|v| (*v - 220.0).abs() < 1e-3), "{:?}", &tab[..3]);
    }

    #[test]
    fn braking_pass_slows_before_a_hairpin() {
        // straight 300 m then a tight 90 degree corner of radius 12 m
        let mut pts: Vec<[f32; 3]> = (0..31).map(|i| [0.0, 0.0, i as f32 * 10.0]).collect();
        for k in 1..=10 {
            let a = k as f32 / 10.0 * std::f32::consts::FRAC_PI_2;
            pts.push([12.0 * (1.0 - a.cos()), 0.0, 300.0 + 12.0 * a.sin()]);
        }
        for i in 1..=30 {
            pts.push([12.0 + i as f32 * 10.0, 0.0, 312.0]);
        }
        let ex: Vec<[f32; 7]> = (0..pts.len()).map(|_| [0.0, 1.0, 0.0, 0.0, 12.0, 12.0, 0.0]).collect();
        let t = RaceTrack::from_parts(&[("race_001".to_string(), pts, ex)], &[], &[], None);
        let tab = calc_target_speed(&t, 0, &CarSpecAi::default(), &|_| 1.0, 2.0);
        let corner = tab[35];
        assert!(corner < 60.0, "{corner}");
        // the table 100 m before the corner is already below the open-road cap
        assert!(tab[20] < 220.0 && tab[28] < tab[20], "{} {}", tab[20], tab[28]);
    }

    #[test]
    fn aim_steers_toward_the_line() {
        let t = straight(60, 10.0);
        let ai = mk_ai(&t, 0.5);
        let mut obs = car_on(&t, 100.0, 4.0, 40.0); // 4 m to the right of the line (side = +x)
        obs.right = Vec3::X; // forward +Z, right = +X for a Y-up frame here
        obs.forward = Vec3::Z;
        let target = Vec3::new(0.0, 0.0, 130.0);
        let (angle, steer) = ai.aim_at_point(&obs, &t, target, 1.0);
        assert!(angle < 0.0 && steer < 0.0, "{angle} {steer}");
        let (a2, s2) = ai.aim_at_point(&obs, &t, Vec3::new(20.0, 0.0, 130.0), 1.0);
        assert!(a2 > 0.0 && s2 > 0.0);
        // pilot detached -> no steering
        obs.pilot_detached = true;
        assert_eq!(ai.aim_at_point(&obs, &t, target, 1.0).1, 0.0);
    }

    #[test]
    fn grid_phase_freezes_the_car() {
        let t = straight(60, 10.0);
        let mut ai = mk_ai(&t, 0.5);
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let mut c = ctx(&eco, &[], &[], &f);
        c.phase = Phase::Grid;
        let o = ai.update(0.016, &car_on(&t, 30.0, 0.0, 0.0), &t, &c);
        assert!(o.frozen);
        assert_eq!(o.input.brake, 1.0);
        assert_eq!(o.input.steer_angle, Some(0.0));
    }

    #[test]
    fn not_running_brakes_and_steers_to_centre_side() {
        let t = straight(60, 10.0);
        let mut ai = mk_ai(&t, 0.5);
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let c = ctx(&eco, &[], &[], &f);
        let mut obs = car_on(&t, 200.0, 3.0, 20.0);
        obs.running = false;
        let o = ai.update(0.016, &obs, &t, &c);
        assert_eq!(o.input.brake, 1.0);
        assert_eq!(o.input.steer, 1.0);
        obs.lateral = -2.0;
        assert_eq!(ai.update(0.016, &obs, &t, &c).input.steer, -1.0);
    }

    #[test]
    fn speed_controller_brake_and_accel() {
        let t = straight(80, 10.0);
        let mut ai = mk_ai(&t, 0.9);
        ai.target_speed = 0.0; // recomputed each frame
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        // a corner-free table: lower the target explicitly through the table
        for v in ai.speed_tables[0].iter_mut() {
            *v = 40.0;
        }
        let c = ctx(&eco, &[], &[], &f);
        // 50 m/s > 40 m/s target: brake 1 (|steer angle| < 0.2)
        let o = ai.update(0.016, &car_on(&t, 200.0, 0.0, 50.0), &t, &c);
        assert_eq!(o.input.brake, 1.0);
        let mut obs = car_on(&t, 200.0, 0.0, 50.0);
        obs.steer_angle = 0.5;
        assert_eq!(ai.update(0.016, &obs, &t, &c).input.brake, 0.6);
        // 20 m/s < 40: no brake; strong AI (ability 0.9 -> (0.9-0.5)*2 = 0.8) pushes with 0.8 * 45 m/s^2
        ai.ability = 0.9;
        ai.skill = 0.9;
        let o = ai.update(0.016, &car_on(&t, 200.0, 0.0, 20.0), &t, &c);
        assert_eq!(o.input.brake, 0.0);
        assert!(o.accelerations.iter().any(|(a, b)| (*a - 0.8 * 45.0).abs() < 1e-3 && *b == 0.016), "{:?}", o.accelerations);
    }

    #[test]
    fn rubber_band_values() {
        let t = straight(80, 10.0);
        let mut ai = mk_ai(&t, 0.5);
        ai.catchup = true;
        ai.catchup_strength = 1.0;
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let mut c = ctx(&eco, &[], &[], &f);
        let me = car_on(&t, 300.0, 0.0, 30.0);
        // player 100 m ahead and faster: band > 0 (AI is catching up), clamped to 0.8
        c.player = Some(PlayerObs { speed: 40.0, spline_pos: t.splines[0].pos_from_distance(400.0), spline_id: 0, pos: Vec3::ZERO, started: true });
        ai.update(0.016, &me, &t, &c);
        assert!(ai.band > 0.1 && ai.band <= 0.8, "{}", ai.band);
        // player 100 m behind and slower: band < 0 (AI eases off)
        c.player = Some(PlayerObs { speed: 20.0, spline_pos: t.splines[0].pos_from_distance(200.0), spline_id: 0, pos: Vec3::ZERO, started: true });
        ai.update(0.016, &me, &t, &c);
        assert!(ai.band < 0.0 && ai.band >= -0.8, "{}", ai.band);
        // no catch-up flag -> 0
        ai.catchup = false;
        ai.update(0.016, &me, &t, &c);
        assert_eq!(ai.band, 0.0);
    }

    #[test]
    fn avoidance_dodges_a_car_ahead() {
        let t = straight(80, 10.0);
        let mut ai = mk_ai(&t, 0.5);
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let me = car_on(&t, 200.0, 0.0, 30.0);
        let nb = vec![Neighbor {
            id: 1,
            pos: Vec3::new(0.0, 0.5, 215.0),
            vel: Vec3::Z * 10.0,
            forward: Vec3::Z,
            speed: 10.0,
            spline_id: 0,
            spline_pos: t.splines[0].pos_from_distance(215.0),
            lateral: 0.0,
            radius: 1.2,
            noncollide_timer: 0.0,
            is_competitor: false,
            is_player: true,
        }];
        let c = ctx(&eco, &nb, &[], &f);
        for _ in 0..3 {
            ai.update(0.016, &me, &t, &c);
        }
        assert!(ai.lateral_target.abs() > 1.0, "{}", ai.lateral_target);
        // empty road: stays on the line
        let mut ai2 = mk_ai(&t, 0.5);
        let c2 = ctx(&eco, &[], &[], &f);
        ai2.update(0.016, &me, &t, &c2);
        assert!(ai2.lateral_target.abs() < 1.0, "{}", ai2.lateral_target);
    }

    #[test]
    fn stuck_respawn_after_the_timeout() {
        let t = straight(80, 10.0);
        let mut ai = mk_ai(&t, 0.0);
        ai.skill = 0.0;
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let c = ctx(&eco, &[], &[], &f);
        let me = car_on(&t, 300.0, 0.0, 0.0);
        let mut respawned = false;
        for _ in 0..200 {
            let o = ai.update(0.016, &me, &t, &c);
            if o.respawn {
                respawned = true;
                break;
            }
        }
        // 2.5 s at 60 Hz = 156 frames
        assert!(respawned);
        assert!((ai.t_stuck - 0.0).abs() < 1e-6);
    }

    #[test]
    fn slingshot_pull_and_release() {
        let t = straight(80, 10.0);
        let mut ai = mk_ai(&t, 1.0);
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let mut c = ctx(&eco, &[], &[], &f);
        c.player = Some(PlayerObs { speed: 0.0, spline_pos: 3.0, spline_id: 0, pos: Vec3::ZERO, started: true });
        let mut me = car_on(&t, 30.0, 0.0, 0.0);
        me.in_slingshot = true;
        me.slingshot_axes = Some([Vec3::X, Vec3::Y, Vec3::Z]);
        let mut released = false;
        for _ in 0..60 {
            let o = ai.update(0.016, &me, &t, &c);
            let s = o.slingshot.unwrap();
            me.slingshot_offset = s.offset;
            if s.release {
                released = true;
                break;
            }
        }
        assert!(released);
        // pulled back along -forward by up to (skill*0.2+1)*8.4
        assert!(me.slingshot_offset.z < 0.0 || me.slingshot_offset.length() > 0.1);
        // before the player started nothing happens
        let mut ai2 = mk_ai(&t, 1.0);
        c.player = Some(PlayerObs { speed: 0.0, spline_pos: 3.0, spline_id: 0, pos: Vec3::ZERO, started: false });
        assert!(ai2.update(0.016, &me, &t, &c).slingshot.is_none());
    }

    #[test]
    fn skill_catchup_and_hotspots() {
        let eco = Economy { skill_base: [0.3, 0.4, 0.5, 0.55, 0.6], ..Economy::default() };
        let mut rng = Rng::new(5);
        for adj in 0..5 {
            for _ in 0..50 {
                let s = ai_skill(&eco, adj, (0.0, 0.6), &mut rng);
                assert!(s >= 0.0 && s <= 0.6);
                assert!((s - eco.skill_base[adj]).abs() <= 0.0501 || s == 0.6);
            }
        }
        assert_eq!(ai_skill(&eco, 4, (0.0, 0.13), &mut rng), 0.13); // C05: aiSkillMax 0.13 clamps the 0.6 base
        assert!(catchup_enabled(GameMode::BossBattle, &Economy { boss_catchup_difficulty: 2, ..Economy::default() }, 2, None));
        assert!(!catchup_enabled(GameMode::Race, &Economy { ai_catchup_difficulty: 3, ..Economy::default() }, 2, None));
        assert!(catchup_enabled(GameMode::Race, &Economy { ai_catchup_difficulty: 3, ..Economy::default() }, 3, None));
        assert!(!catchup_enabled(GameMode::Race, &Economy { ai_catchup_difficulty: 3, ..Economy::default() }, 3, Some(true)));
        // later opponents rubber band less
        let mut r = Rng::new(9);
        assert_eq!(catchup_strength(0, &mut r), 1.0);
        assert_eq!(catchup_strength(2, &mut r), 1.0);
        assert!(catchup_strength(7, &mut r) < 0.25);
        assert_eq!(HotspotKind::from_helper("ai_hotspot_large").unwrap().radius(), 50.0);
        assert_eq!(HotspotKind::ExtraLarge.radius(), 65.0);
        assert_eq!(HotspotKind::Small.radius(), 25.0);
        assert_eq!(HotspotKind::Medium.radius(), 35.0);
    }

    #[test]
    fn hotspot_target_accepts_only_reachable_points() {
        let t = straight(80, 10.0);
        let mut ai = mk_ai(&t, 0.5);
        let me = car_on(&t, 200.0, 0.0, 30.0);
        // 20 m ahead, 2 m to the side: angle 0.1 rad * 30 m/s = 3 < 30 -> accepted
        assert!(ai.set_hotspot_target(&me, &t, Vec3::new(2.0, 0.5, 220.0)));
        assert!(ai.hotspot_active);
        // behind the car: rejected
        let mut ai2 = mk_ai(&t, 0.5);
        assert!(!ai2.set_hotspot_target(&me, &t, Vec3::new(0.0, 0.5, 150.0)));
    }

    #[test]
    fn ai_spline_choice_uses_skill_interpolated_weights() {
        let a: Vec<[f32; 3]> = (0..30).map(|i| [0.0, 0.0, i as f32 * 10.0]).collect();
        let ex: Vec<[f32; 7]> = (0..30).map(|_| [0.0, 1.0, 0.0, 0.0, 10.0, 10.0, 0.0]).collect();
        let w = vec![
            SplineWeight { name: "race_001".into(), min_ai_weighting: 1.0, max_ai_weighting: 0.0 },
            SplineWeight { name: "race_002".into(), min_ai_weighting: 0.0, max_ai_weighting: 1.0 },
        ];
        let t = RaceTrack::from_parts(&[("race_001".into(), a.clone(), ex.clone()), ("race_002".into(), a, ex)], &[], &w, None);
        let mut rng = Rng::new(11);
        // skill 1 -> weights 0 : 1 -> always the second line; skill 0 -> always the first
        assert!((0..50).all(|_| t.pick_ai_spline(1.0, &mut rng) == 1));
        assert!((0..50).all(|_| t.pick_ai_spline(0.0, &mut rng) == 0));
    }

    /// Closed-loop check of the steering controller: a point-mass car (yaw rate follows `steer * 1.6 rad/s` with a 0.1 s lag, the
    /// arcade yaw target of the player input) is driven by the AI along the real race line of theme002/run000. The AI must stay
    /// inside the corridor and keep making progress.
    #[test]
    fn closed_loop_follows_the_real_race_line() {
        let Some(root) = assets_dir() else { return };
        let Ok(data) = std::fs::read(root.join("tracks/theme002/run000/track.stm")) else { return };
        let pvs = abgtool::stm::parse_pvs_only(&data).unwrap();
        let xml = std::fs::read_to_string(root.join("xml_tracks/theme002/run000/track.xml")).unwrap_or_default();
        let t = RaceTrack::from_stm(&pvs, &xml, None);
        let main = t.main_line().unwrap();
        let mut ai = RaceAi::new(
            AiParams { skill: 0.6, spline_id: main, seed: 9, ..Default::default() },
            &t,
            CarSpecAi::default(),
            &AiTweaks::default(),
            &|_| 1.0,
        );
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let c = ctx(&eco, &[], &[], &f);
        let s = &t.splines[main];
        // start on the line 30 m in, heading along it
        let mut spos = s.lookahead(0.0, 30.0).0;
        let (p0, d0) = s.info(spos, 0.0);
        let mut pos = Vec3::new(p0.x, p0.y, p0.z);
        let mut psi = d0.x.atan2(d0.z);
        let mut speed = 25.0f32;
        let mut yaw = 0.0f32;
        let mut steer_cmd;
        let mut max_lat = 0.0f32;
        let dt = 1.0 / 60.0;
        let mut t_end = 0.0;
        for step in 0..(60 * 60) {
            let fwd = Vec3::new(psi.sin(), 0.0, psi.cos());
            let right = Vec3::new(psi.cos(), 0.0, -psi.sin());
            let (np, _) = s.new_pos((spos as usize).min(s.count() - 1), pos);
            spos = np;
            let lat = s.lateral_offset(spos, pos);
            max_lat = max_lat.max(lat.abs());
            let mut obs = CarObs::simple(pos + Vec3::Y * 0.5, fwd * speed, fwd);
            obs.right = right;
            obs.orientation = Quat::from_rotation_y(psi);
            obs.ang_vel = Vec3::new(0.0, yaw, 0.0);
            obs.spline_id = main;
            obs.spline_pos = spos;
            obs.lateral = lat;
            let o = ai.update(dt, &obs, &t, &c);
            steer_cmd = o.input.steer;
            yaw += (steer_cmd * 1.6 - yaw) * (dt / 0.1).min(1.0);
            psi += yaw * dt;
            let acc: f32 = o.accelerations.iter().map(|(a, _)| *a).sum();
            speed = (speed + (acc * 0.5 + 6.0) * dt - o.input.brake * 25.0 * dt).clamp(8.0, 55.0);
            pos += Vec3::new(psi.sin(), 0.0, psi.cos()) * speed * dt;
            pos.y = s.position(spos).y;
            t_end = step as f32 * dt;
            if spos > s.finish_pos - 1.0 {
                break;
            }
        }
        eprintln!("closed loop: reached pos {spos:.1}/{:.1} after {t_end:.1}s, max |lateral| {max_lat:.2} m, final speed {speed:.1}", s.finish_pos);
        assert!(max_lat < 8.0, "left the corridor: {max_lat}");
        assert!(spos > 20.0, "no progress: {spos}");
    }

    #[test]
    fn real_track_ai_laps_without_panics() {
        // drive an AI kinematically along the real theme002/run000 race line for 60 s of simulated time
        let Some(root) = assets_dir() else { return };
        let stm = root.join("tracks/theme002/run000/track.stm");
        let Ok(data) = std::fs::read(&stm) else { return };
        let pvs = abgtool::stm::parse_pvs_only(&data).unwrap();
        let xml = std::fs::read_to_string(root.join("xml_tracks/theme002/run000/track.xml")).unwrap_or_default();
        let t = RaceTrack::from_stm(&pvs, &xml, None);
        let main = t.main_line().unwrap();
        let mut ai = RaceAi::new(
            AiParams { skill: 0.6, spline_id: main, seed: 9, ..Default::default() },
            &t,
            CarSpecAi::default(),
            &AiTweaks::default(),
            &|_| 1.0,
        );
        let eco = Economy::default();
        let f = |_: i32| 1.0f32;
        let c = ctx(&eco, &[], &[], &f);
        let s = &t.splines[main];
        let mut spos = s.lookahead(0.0, 30.0).0;
        let mut speed = 25.0f32;
        let mut max_lat = 0.0f32;
        for _ in 0..3600 {
            let dt = 1.0 / 60.0;
            let (p, dir) = s.info(spos, 0.0);
            let mut obs = CarObs::simple(p + Vec3::Y * 0.5, dir * speed, dir);
            obs.spline_id = main;
            obs.spline_pos = spos;
            obs.right = s.nodes[(spos as usize).min(s.count() - 1)].side;
            let o = ai.update(dt, &obs, &t, &c);
            // kinematic follow: brake pulls the speed down, accelerations push it up
            speed = (speed - o.input.brake * 30.0 * dt + o.accelerations.iter().map(|(a, _)| *a).sum::<f32>() * dt * 0.5 + 8.0 * dt).clamp(5.0, 70.0);
            let adv = speed * dt;
            spos = s.lookahead(spos, adv).0;
            max_lat = max_lat.max(o.input.steer.abs());
            assert!(o.input.steer.abs() <= 1.0 && o.input.brake >= 0.0 && o.input.brake <= 1.0);
            if spos > s.finish_pos {
                break;
            }
        }
        assert!(spos > 50.0, "advanced to {spos}");
    }
}
