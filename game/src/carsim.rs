//! Angry Birds Go! 2.9.1 vehicle dynamics, ported 1:1 from the Ghidra decompile of `libABK291.so`
//! (`C:\AngryBirdsRef\decomp291\libABK291_annotated.c`, Ghidra addresses; file/vaddr = Ghidra - 0x10000).
//!
//! Depends only on `std` and `glam` so it can be tested in a scratch crate through `#[path]`.
//!
//! # What the original car really is (verified in the decompile)
//! * The 2.9.1 kart has NO engine/gearbox simulation. `m_fTorqueLbFt_*`, gear/clutch fields are parsed by
//!   `CCarSpec::Read` but never read by `CCar::Integrate` / `CCar::Update` (spec offsets 0x360..0x428 and
//!   `m_fTorqueScalar` 0x43c are only touched by Read/SetDefault/Reset). Forward drive comes from
//!   `CCar::Update @ 001a6cd8`: when not braking, `ApplyBodyForce(0,0,dt*m_fBelowMinSpeedThrust*clamp(m_fMinDesiredSpeed-speed,0.01,1))`.
//!   Speed is bounded by the rigid-body quadratic drag (`m_fDrag`) in `CXGSRigidBody::Integrate @ 005b73e8`.
//! * Order per physics step (`CXGSPhys::Update @ 005b3e9c` -> `StepSimulation` -> per body `CXGSRigidBody::Integrate`):
//!   `_CarIntegrateCallback -> CCar::Integrate @ 001abaa8` (steering, brakes, anti-roll, ground rays, `CWheel::Integrate`
//!   force application, post processing) and then the rigid body step (drag, downforce, gravity, quaternion/position).
//!   `CCar::Update` runs once per game frame before the physics steps.
//! * All forces in the car code are IMPULSES (already multiplied by the physics time step `rb+0x98`), the rigid body
//!   adds them straight to velocity (`ApplyWorldForce @ 005b57a0`).
//!
//! # UNRESOLVED (see the markers in the code)
//! See the report; every spot that could not be determined from the decompile/data carries `// UNRESOLVED:`.
#![allow(non_snake_case, dead_code, clippy::too_many_arguments, clippy::needless_range_loop)]

use glam::{Quat, Vec3};

// ===========================================================================================================
// Ground interface
// ===========================================================================================================

/// Result of a ray against the world (`CXGSEnv::RayIntersect @ 005a695c` result as used by `UpdateGroundInfo`).
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub point: Vec3,
    pub normal: Vec3,
    /// `EPhysMaterial` id (1-based; 1 is the default surface, see [`PHYS_MATERIALS`]).
    pub material: u32,
}

/// The track is not available, so the world is an interface. `height_normal(p)` returns the surface height at
/// `(p.x, p.z)` and the up-pointing surface normal there.
pub trait Ground {
    /// Interface (no original function): surface height at (p.x, p.z) and its up normal.
    fn height_normal(&self, p: Vec3) -> (f32, Vec3);
    /// Surface material id at `p` (`EPhysMaterial`, 1-based). Default: 1, the default/road surface.
    fn material(&self, _p: Vec3) -> u32 {
        1
    }
    /// `CGame::IsPosInMaterialVolume @ 00119610` (`CCar::Update` asks for ids 0x1d / 0x25 / 0x26): casts a ray straight up from `p`
    /// through the triangles of that surface id only; the position is inside the volume when the first triangle hit is a back face.
    /// Default: never inside (flat plate, no volumes).
    fn in_material_volume(&self, _p: Vec3, _material: u32) -> bool {
        false
    }
    /// Segment query `origin -> origin + vec` against the surface (first crossing from above).
    /// Default implementation marches the segment and bisects on `height_normal`.
    fn ray(&self, origin: Vec3, vec: Vec3) -> Option<RayHit> {
        let d = |t: f32| {
            let p = origin + vec * t;
            p.y - self.height_normal(p).0
        };
        if d(0.0) < 0.0 {
            return None;
        }
        const N: usize = 64;
        let mut prev = 0.0f32;
        for i in 1..=N {
            let t = i as f32 / N as f32;
            if d(t) <= 0.0 {
                let (mut lo, mut hi) = (prev, t);
                for _ in 0..30 {
                    let mid = 0.5 * (lo + hi);
                    if d(mid) <= 0.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                let p = origin + vec * hi;
                let (h, n) = self.height_normal(p);
                return Some(RayHit { point: Vec3::new(p.x, h, p.z), normal: n, material: self.material(p) });
            }
            prev = t;
        }
        None
    }
    /// Camera segment query (`CXGSEnv::RayIntersect` as `CCamera::DoCollisionCheck` calls it with the camera surface filter
    /// `_FilterCameraCollision @ 000df7f4`: surface ids 0x1d, 0x1e, 0x21, 0x25, 0x26 are ignored). Returns the nearest crossing of
    /// the segment `origin -> origin + vec` with a collision triangle, from either side: `(point, normal)`.
    /// Default: no camera collision.
    fn cam_ray(&self, _origin: Vec3, _vec: Vec3) -> Option<(Vec3, Vec3)> {
        None
    }
    /// Camera sphere query (`CXGSEnv::SphereIntersect` in `DoCollisionCheck`, same surface filter): the deepest contact of a sphere
    /// with the collision triangles, `(contact point, normal pointing to the sphere centre)`. Default: none.
    fn cam_sphere(&self, _centre: Vec3, _radius: f32) -> Option<(Vec3, Vec3)> {
        None
    }
}

/// Flat baseplate at `y = height`.
#[derive(Clone, Copy, Debug)]
pub struct FlatGround {
    pub height: f32,
    pub material: u32,
}
impl Default for FlatGround {
    /// Interface/helper: plane at y = 0, default material 1 (no original function).
    fn default() -> Self {
        FlatGround { height: 0.0, material: 1 }
    }
}
impl Ground for FlatGround {
    /// Interface: flat plane height/normal (no original function).
    fn height_normal(&self, _p: Vec3) -> (f32, Vec3) {
        (self.height, Vec3::Y)
    }
    /// Interface: surface material of the flat plane (stands in for the triangle material id of CXGSEnv::RayIntersect).
    fn material(&self, _p: Vec3) -> u32 {
        self.material
    }
    /// `CGame::IsPosInMaterialVolume @ 00119610` (`CCar::Update` asks for ids 0x1d / 0x25 / 0x26): casts a ray straight up from `p`
    /// through the triangles of that surface id only; the position is inside the volume when the first triangle hit is a back face.
    /// Default: never inside (flat plate, no volumes).
    fn in_material_volume(&self, _p: Vec3, _material: u32) -> bool {
        false
    }
    /// Interface: exact segment-vs-plane test (stands in for the engine ray cast CXGSEnv::RayIntersect).
    fn ray(&self, origin: Vec3, vec: Vec3) -> Option<RayHit> {
        let s0 = origin.y - self.height;
        let s1 = origin.y + vec.y - self.height;
        if s0 < 0.0 || s1 > 0.0 || (s0 - s1) == 0.0 {
            return None;
        }
        let t = s0 / (s0 - s1);
        let p = origin + vec * t;
        Some(RayHit { point: Vec3::new(p.x, self.height, p.z), normal: Vec3::Y, material: self.material })
    }
    /// Interface: the plane stops the camera segment from either side.
    fn cam_ray(&self, origin: Vec3, vec: Vec3) -> Option<(Vec3, Vec3)> {
        let s0 = origin.y - self.height;
        let s1 = s0 + vec.y;
        if (s0 > 0.0) == (s1 > 0.0) || s0 == s1 {
            return None;
        }
        let t = s0 / (s0 - s1);
        let p = origin + vec * t;
        Some((p, if s0 > 0.0 { Vec3::Y } else { -Vec3::Y }))
    }
    /// Interface: sphere against the plane.
    fn cam_sphere(&self, centre: Vec3, radius: f32) -> Option<(Vec3, Vec3)> {
        let d = centre.y - self.height;
        if d.abs() >= radius {
            return None;
        }
        Some((Vec3::new(centre.x, self.height, centre.z), if d >= 0.0 { Vec3::Y } else { -Vec3::Y }))
    }
}

// ===========================================================================================================
// Physics materials (PhysMaterial_* @ 00145b1c..00145f30; table data read from libABK291.so)
// ===========================================================================================================

/// One `TXGSPhysMaterial` row as read by `PhysMaterial_Get*`: offsets 0x00..0x24 of the struct the table points to.
#[derive(Clone, Copy, Debug)]
pub struct PhysMaterial {
    pub bump_freq: f32,          // +0x00 (PhysMaterial_GetBumpOffset)
    pub bump_amp: f32,           // +0x04
    pub peak_grip_scale: f32,    // +0x08 PhysMaterial_GetPeakGripScale @ 00145de0
    pub peak_grip_slip_angle: f32, // +0x0c PhysMaterial_GetPeakGripSlipAngle @ 00145e00 (degrees; wheel code multiplies by 0.0175)
    pub peak_grip_slip_ratio: f32, // +0x10 PhysMaterial_GetPeakGripSlipRatio @ 00145e20
    pub rolling_resistance: f32, // +0x14 PhysMaterial_GetRollingResistance @ 00145e40 (not read by CWheel::Integrate)
    pub softness: f32,           // +0x18 PhysMaterial_GetSoftness @ 00145e80
    pub damping: f32,            // +0x1c PhysMaterial_GetDamping @ 00145e60
    pub wear_rate: [f32; 2],     // +0x20 / +0x24 PhysMaterial_GetWearRate @ 00145ea0 (arg 0 = road tyre, 1 = off-road tyre)
}

/// Interface/helper: const constructor for the PHYS_MATERIALS rows (no original function).
const fn pm(r: [f32; 10]) -> PhysMaterial {
    PhysMaterial {
        bump_freq: r[0],
        bump_amp: r[1],
        peak_grip_scale: r[2],
        peak_grip_slip_angle: r[3],
        peak_grip_slip_ratio: r[4],
        rolling_resistance: r[5],
        softness: r[6],
        damping: r[7],
        wear_rate: [r[8], r[9]],
    }
}

/// The 38 (`0x26`, see `CXGSPhys::SetPhysMaterialList(..., 0x26)` in `PhysMaterial_Setup @ 00145f90`) material rows,
/// index = EPhysMaterial id - 1. Values dumped from the `.so` (pointer table at vaddr 0xd9d118, stride 0x24).
/// Material NAMES are not recoverable from the code, only the ids; id 1 is the default surface the original itself
/// uses for its flat-plane mode (`CCar::UpdateGroundInfo @ 0019f2b8` writes material 1 for the fake plane).
pub static PHYS_MATERIALS: [PhysMaterial; 38] = [
    /* id  1 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id  2 */ pm([0.6, 0.03, 1.45, 12.0, 0.15, 0.04, 0.0, 0.0, 0.002, 0.002]),
    /* id  3 */ pm([0.1, 0.05, 1.3, 14.0, 0.15, 0.15, 0.0, 0.0, 0.004, 0.004]),
    /* id  4 */ pm([0.5, 0.3, 1.3, 13.0, 0.12, 0.1, 0.0, 0.5, 0.1, 0.002]),
    /* id  5 */ pm([0.5, 0.2, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.1, 0.002]),
    /* id  6 */ pm([0.1, 0.03, 1.4, 13.0, 0.12, 0.15, 0.0, 0.0, 0.005, 0.005]),
    /* id  7 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id  8 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id  9 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 10 */ pm([0.08, 0.05, 1.3, 16.0, 0.18, 0.1, 0.0, 0.0, 0.004, 0.004]),
    /* id 11 */ pm([0.0, 0.0, 1.2, 16.0, 0.18, 0.1, 0.0, 2.0, 0.004, 0.004]),
    /* id 12 */ pm([0.0, 0.0, 1.45, 12.0, 0.15, 0.03, 0.0, 0.0, 0.002, 0.002]),
    /* id 13 */ pm([0.04, 0.04, 1.0, 16.0, 0.18, 0.1, 0.0, 0.25, 0.002, 0.002]),
    /* id 14 */ pm([0.0, 0.0, 1.45, 12.0, 0.15, 0.03, 0.0, 0.0, 0.002, 0.002]),
    /* id 15 */ pm([0.04, 0.02, 0.2, 16.0, 0.18, 0.1, 0.0, 0.0, 0.002, 0.002]),
    /* id 16 */ pm([0.5, 0.02, 1.1, 14.0, 0.14, 0.15, 0.0, 0.15, 0.006, 0.006]),
    /* id 17 */ pm([0.04, 0.04, 1.0, 16.0, 0.18, 0.1, 0.0, 0.25, 0.01, 0.01]),
    /* id 18 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 19 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 20 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 21 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 22 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 23 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 24 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 25 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.1, 0.0, 2.5, 0.002, 0.002]),
    /* id 26 */ pm([0.5, 0.12, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.002, 0.002]),
    /* id 27 */ pm([0.3, 0.05, 1.5, 12.0, 0.15, 0.2, 0.0, 2.0, 0.002, 0.002]),
    /* id 28 */ pm([0.0, 0.0, 0.1, 12.0, 0.15, 0.03, 0.0, 0.0, 0.002, 0.002]),
    /* id 29 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 30 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 31 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 32 */ pm([0.6, 0.03, 1.45, 12.0, 0.15, 0.04, 0.0, 0.0, 0.002, 0.002]),
    /* id 33 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 34 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 35 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 36 */ pm([0.6, 0.03, 1.45, 12.0, 0.15, 0.04, 0.0, 0.0, 0.002, 0.002]),
    /* id 37 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
    /* id 38 */ pm([0.0, 0.0, 1.5, 12.0, 0.15, 0.03, 0.0, 0.0, 0.0, 0.002]),
];

/// Material rows are 1-based (`(param_1 + -1) * 0x24`). Ids outside the table fall back to id 1.
pub fn phys_material(id: i32) -> &'static PhysMaterial {
    let i = if (1..=38).contains(&id) { (id - 1) as usize } else { 0 };
    &PHYS_MATERIALS[i]
}

/// `PhysMaterial_GetBumpOffset @ 00145b1c`. Returns `amp * noise(pos*freq)` for materials with `amp > 0`, otherwise the
/// constant `DAT_00145dd0` (0.0, read from the binary).
// UNRESOLVED: for bump_amp > 0 the original samples a 3D gradient-noise table (256-entry permutation + 256x3 gradient
// table) that `PhysMaterial_Setup @ 00145f90` fills with the engine RNG at start-up; those tables are runtime data that
// cannot be recovered statically. Materials with amplitude 0 (including the default id 1) are exact; for the others this
// returns 0.0 (flat), which is NOT 1:1.
pub fn phys_material_bump_offset(id: i32, _p: Vec3) -> f32 {
    let m = phys_material(id);
    if m.bump_amp <= 0.0 {
        return 0.0;
    }
    0.0
}

// ===========================================================================================================
// Fast math / tables
// ===========================================================================================================

/// `powf_fast(float,float) @ 001b76f4` as inlined in `CWheel::Integrate @ 001b86b4` (suspension exponent) -
/// float-bits log2/exp2 approximation, constants read from the binary: 127.0, 0.346607, 0.33971, fixed point 23 bits.
pub fn powf_fast(x: f32, e: f32) -> f32 {
    // vcvt.f32.s32 s,#0x17: reinterpret the float bits as s32 with 23 fractional bits.
    let lg = (x.to_bits() as i32) as f32 / 8_388_608.0 - 127.0;
    let fl = lg.floor();
    let fr = lg - fl;
    let lg = lg + (fr - fr * fr) * 0.346607;
    let y = e * lg;
    let fy = y.floor();
    let fr2 = y - fy;
    let r = (y + 127.0) - (fr2 - fr2 * fr2) * 0.33971;
    // vcvt.s32.f32 s,#0x17: back to float bits (round towards zero, saturating)
    f32::from_bits((r * 8_388_608.0) as i32 as u32)
}

/// Both 20-entry slip-force curves (`DAT_001b916c + 0x1b8f94`, `DAT_001b9170 + 0x1b9008`, identical, read from the binary):
/// `0, .25, .5, .75, 1, 1, ...`
const SLIP_CURVE: [f32; 20] =
    [0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

/// The inline curve lookup in `CWheel::Integrate @ 001b86b4` (0x1b8f94..0x1b9024).
fn slip_curve(x: f32) -> f32 {
    let x4 = x.abs() * 4.0;
    let i = x4 as i32;
    if i < 20 {
        let j = if i + 1 == 20 { 19 } else { (i + 1) as usize };
        let a = SLIP_CURVE[i as usize];
        a + (SLIP_CURVE[j] - a) * (x4 - i as f32)
    } else {
        SLIP_CURVE[19]
    }
}

/// `CWheel::PrecalcTyreLoadSensitivity @ 001b7778`: 256 entries, `table[k] = powf(max(k,1)/128, exponent)`
/// (entry 0 repeats entry 1, the loop starts at 1/128 and steps 1/128).
pub fn precalc_tyre_load_sensitivity(exponent: f32) -> Vec<f32> {
    let step = 0.0078125f32; // 0x3c000000
    let mut t = vec![0.0f32; 256];
    t[0] = step.powf(exponent);
    let mut s = step;
    for k in 1..256 {
        t[k] = s.powf(exponent);
        s += step;
    }
    t
}

// ===========================================================================================================
// CCarSpec  (Read @ 001b58d4, SetDefault @ 001b51f0, CopyWithMods @ 001b5700)
// ===========================================================================================================

/// `CCarSpec::TWheelSpec` (stride 0x54 from spec+0x120). Field names are the XML attribute names read by `CCarSpec::Read`.
#[derive(Clone, Debug)]
pub struct WheelSpec {
    pub bOnLeft: bool,              // +0x00
    pub bOffRoadTyre: bool,         // +0x04
    pub bRotating: bool,            // +0x08
    pub bHasTread: bool,            // +0x0c
    pub fRenderScale: f32,          // +0x10
    pub fRenderOffset: f32,         // +0x14
    pub fTreadSpeedScale: f32,      // +0x18
    pub fPeakGrip: f32,             // +0x1c
    pub fGripAngAdd: f32,           // +0x20
    pub fWheelInertia: f32,         // +0x24
    pub fBrakeTorque: f32,          // +0x28
    pub fTorqueSplit: f32,          // +0x2c
    pub fMaxSteeringLock: f32,      // +0x30
    pub fSuspensionStiffness: f32,  // +0x34
    pub fSuspensionDamping: f32,    // +0x38
    pub fSuspensionExponent: f32,   // +0x3c
    pub fSuspensionTravel: f32,     // +0x40
    pub fWheelRadius: f32,          // +0x44
    pub fOptimumLoad: f32,          // +0x48
    pub fTyreExponent: f32,         // +0x4c
    pub fWheelBearingResistance: f32, // +0x50
}

/// `CCarSpec::m_aAntiRollBars[i]` (spec+0x320 + 12*i): `iLeftWheel`, `iRightWheel`, `fStiffness`.
#[derive(Clone, Copy, Debug, Default)]
pub struct AntiRollBar {
    pub iLeftWheel: i32,
    pub iRightWheel: i32,
    pub fStiffness: f32,
}

/// `CModSpec` ratios used by `CCarSpec::CopyWithMods @ 001b5700` (CModSpec+0x10..0x24, each 0..1 between the
/// `_min.xml` and `_max.xml` car specs).
// UNRESOLVED: how the player's upgrade levels become these six ratios (`CModSpec` setters) was not ported; callers pass them.
#[derive(Clone, Copy, Debug, Default)]
pub struct CModSpec {
    pub grip: f32,            // +0x10 -> wheel fPeakGrip
    pub fragility: f32,       // +0x14 -> m_fFragility
    pub thrust: f32,          // +0x18 -> m_fBelowMinSpeedThrust
    pub drag: f32,            // +0x1c -> m_fDrag
    pub sliding_ang_vel: f32, // +0x20 -> m_fSlidingTargetAngularVel
    pub min_speed: f32,       // +0x24 -> m_fMinDesiredSpeed
}

/// `CCarSpec` (physics-relevant fields; names/units as `CCarSpec::Read`, offsets in the comments).
#[derive(Clone, Debug)]
pub struct CarSpec {
    pub m_iNumWheels: usize,        // +0x11c
    pub wheels: [WheelSpec; 6],     // +0x120 + 0x54*i  (order FL, FR, RR, RL, ...)
    pub m_fToeIn: f32,              // +0x318
    pub m_iNumAntiRollBars: usize,  // +0x31c
    pub m_aAntiRollBars: [AntiRollBar; 4], // +0x320
    pub m_vCOMOffset: Vec3,         // +0x350
    pub m_fInertia: f32,            // +0x35c
    /// +0x360..0x3fc, 40 entries every 500 rpm. Parsed, NEVER used by the 2.9.1 car code (see module doc).
    pub m_fTorqueLbFt: [f32; 40],
    pub m_fLowerRevLimit: f32,      // +0x400 (unused by car code)
    pub m_fUpperRevLimit: f32,      // +0x404
    pub m_fShiftUpRPM: f32,         // +0x408
    pub m_fShiftDownRPM: f32,       // +0x40c
    pub m_fFirstShiftRPM: f32,      // +0x410
    pub m_fClutchStiffness: f32,    // +0x414
    pub m_fClutchLimit: f32,        // +0x418
    pub m_fEngineFriction: f32,     // +0x41c
    pub m_fEngineInertia: f32,      // +0x420
    pub m_fEngineChassisTorque: f32, // +0x424
    pub m_fTCS: f32,                // +0x428
    pub m_fDrag: f32,               // +0x42c
    pub m_fDownForce: f32,          // +0x430
    pub m_fBelowMinSpeedThrust: f32, // +0x434
    pub m_fMinDesiredSpeed: f32,    // +0x438
    pub m_fTorqueScalar: f32,       // +0x43c (unused by car code)
    pub m_vDownForcePos: Vec3,      // +0x440
    pub m_fMass: f32,               // +0x44c
    pub m_fSteeringSpeedScale: f32, // +0x450
    pub m_fSoftSteeringScale: f32,  // +0x454
    pub m_fMaxSpeedSteerScale: f32, // +0x458
    pub m_fSlidingTargetAngularAccel: f32, // +0x45c
    pub m_fSlidingTargetAngularVel: f32, // +0x460
    pub m_fSteerAdjRate: f32,       // +0x464
    pub m_fCentreSteerAdjRate: f32, // +0x468
    pub m_fAirborneSteerScale: f32, // +0x46c
    pub m_fAirborneRotation: f32,   // +0x470
    pub m_fAirborneLateralGrip: f32, // +0x474
    pub m_iClass: i32,              // +0x478
    pub m_fFullRepairCost: f32,     // +0x47c
    pub m_fFragility: f32,          // +0x480
}

/// Per-wheel defaults of CCarSpec::SetDefault @ 001b51f0 / CCarSpec::CCarSpec() @ 001b4b00.
fn default_wheel(i: usize) -> WheelSpec {
    // SetDefault @ 001b51f0 (constants decoded from the immediate stores / literal pool)
    WheelSpec {
        bOnLeft: [true, false, false, true, true, false][i], // +0x120=1, +0x174=0, +0x1c8=0, +0x21c=1, +0x270=1, +0x2c4=0
        bOffRoadTyre: false,
        bRotating: true,
        bHasTread: false,
        fRenderScale: 1.0,
        fRenderOffset: 0.0,
        fTreadSpeedScale: 0.1,                  // 0x3dcccccd
        fPeakGrip: 0.73,                        // 0x3f3ae148
        fGripAngAdd: 0.0,
        fWheelInertia: 3.0,                     // 0x40400000
        fBrakeTorque: 2500.0,                    // DAT_001b55f0 = 0x451c4000
        fTorqueSplit: 0.25,                     // 0x3e800000
        fMaxSteeringLock: 1.05,                 // 0x3f866666
        fSuspensionStiffness: 12000.0,          // 0x463b8000
        fSuspensionDamping: 1500.0,             // 0x44bb8000
        fSuspensionExponent: 1.0,
        fSuspensionTravel: 1.2,                 // 0x3f99999a
        fWheelRadius: 0.4,                      // 0x3ecccccd
        fOptimumLoad: 3400.0,                   // 0x45548000
        fTyreExponent: 0.95,                    // 0x3f733333
        fWheelBearingResistance: 0.0,
    }
}

impl CarSpec {
    /// `CCarSpec::SetDefault @ 001b51f0` / `CCarSpec::CCarSpec() @ 001b4b00`.
    // UNRESOLVED: SetDefault never sets m_fMass (0x44c), m_fInertia (0x35c), m_fDownForce (0x430), the COM offset,
    // m_fSteeringSpeedScale (0x450) or m_fDrag (0x42c) (they stay zero here, exactly like the original object): they come
    // from the 2.9.1 base car spec in `Data/Cars/carSpec.pak` (mounted as "CARSPEC:") which is a cloud-downloaded archive
    // that is NOT in the APK. Use `read_xml` on a base spec and see `apply_player_car_tweakables` for the only 2.9.1
    // values that are available.
    pub fn set_default() -> CarSpec {
        let wheels: [WheelSpec; 6] = std::array::from_fn(default_wheel);
        CarSpec {
            m_iNumWheels: 4,
            wheels,
            m_fToeIn: 0.0,
            m_iNumAntiRollBars: 0,
            m_aAntiRollBars: [AntiRollBar::default(); 4],
            m_vCOMOffset: Vec3::ZERO,
            m_fInertia: 0.0,
            m_fTorqueLbFt: [0.0; 40],
            m_fLowerRevLimit: 0.0,
            m_fUpperRevLimit: 0.0,
            m_fShiftUpRPM: 0.0,
            m_fShiftDownRPM: 0.0,
            m_fFirstShiftRPM: 0.0,
            m_fClutchStiffness: 0.0,
            m_fClutchLimit: 0.0,
            m_fEngineFriction: 0.0,
            m_fEngineInertia: 0.0,
            m_fEngineChassisTorque: -30.0, // 0xc1f00000
            m_fTCS: 0.0,
            m_fDrag: 0.0,
            m_fDownForce: 0.0,
            m_fBelowMinSpeedThrust: 2000.0, // 0x44fa0000
            m_fMinDesiredSpeed: 40.0,       // = CDebugManager::GetDebugFloat(0x27) = tweakable "Minimum_Speed" (debugtweakables.xml: 40.0)
            m_fTorqueScalar: 0.0,
            m_vDownForcePos: Vec3::ZERO,
            m_fMass: 0.0,
            m_fSteeringSpeedScale: 0.0,
            m_fSoftSteeringScale: 0.0,
            m_fMaxSpeedSteerScale: 100.0, // DAT_001b5604 = 0x42c80000
            m_fSlidingTargetAngularAccel: 0.0,
            m_fSlidingTargetAngularVel: 1.4, // 0x3fb33333
            m_fSteerAdjRate: 12.0,           // 0x41400000
            m_fCentreSteerAdjRate: 16.0,     // 0x41800000
            m_fAirborneSteerScale: 0.0,
            m_fAirborneRotation: 0.0,
            m_fAirborneLateralGrip: 0.0,
            m_iClass: 0,
            m_fFullRepairCost: 1000.0, // DAT_001b55ec
            m_fFragility: 1.0,
        }
    }

    /// Interface/helper (no original function): the player-car values that the 2.9.1 `debugtweakables.xml` still carries
    /// (`Player_Car_Mass` 285, `Player_Car_Inertia` 1.65, `Player_Car_Drag` 0.25, `Player_Car_DownForce` 1.0,
    /// `Player_Car_Wheel_*_PeakGrip` 0.73, `Player_Car_TorqueScalar` 0.32). These equal the v1.0.1 `kart_base` spec and are
    /// only a stand-in for the missing 2.9.1 base spec - NOT proven to equal the shipped per-car values.
    pub fn apply_player_car_tweakables(&mut self) {
        self.m_fMass = 285.0;
        self.m_fInertia = 1.65;
        self.m_fDrag = 0.25;
        self.m_fDownForce = 1.0;
        self.m_fTorqueScalar = 0.32;
        for w in self.wheels.iter_mut() {
            w.fPeakGrip = 0.73;
        }
    }

    /// `CCarSpec::Read @ 001b58d4`: reads the decoded XML (`<CarSpec m_f...="..."><EWheel_FL fPeakGrip="..."/>...`).
    /// Attributes that are absent keep their current value, exactly like the original (`GetAttribute == NULL` -> skip).
    pub fn read_xml(&mut self, xml: &str) {
        let top = xml_tag_attrs(xml, "CarSpec");
        let f = |k: &str, dst: &mut f32| {
            if let Some(v) = top.get(k).and_then(|s| s.trim().parse::<f64>().ok()) {
                *dst = v as f32;
            }
        };
        if let Some(v) = top.get("m_iNumWheels").and_then(|s| s.trim().parse::<i64>().ok()) {
            self.m_iNumWheels = (v.max(0) as usize).min(6);
        }
        const NAMES: [&str; 6] = ["EWheel_FL", "EWheel_FR", "EWheel_RR", "EWheel_RL", "EWheel_5", "EWheel_6"];
        for i in 0..self.m_iNumWheels.min(4) {
            let a = xml_tag_attrs(xml, NAMES[i]);
            let w = &mut self.wheels[i];
            let b = |k: &str, dst: &mut bool| {
                if let Some(v) = a.get(k).and_then(|s| s.trim().parse::<i64>().ok()) {
                    *dst = v == 1;
                }
            };
            let g = |k: &str, dst: &mut f32| {
                if let Some(v) = a.get(k).and_then(|s| s.trim().parse::<f64>().ok()) {
                    *dst = v as f32;
                }
            };
            b("bOnLeft", &mut w.bOnLeft);
            b("bOffRoadTyre", &mut w.bOffRoadTyre);
            b("bRotating", &mut w.bRotating);
            b("bHasTread", &mut w.bHasTread);
            g("fRenderScale", &mut w.fRenderScale);
            g("fRenderOffset", &mut w.fRenderOffset);
            g("fTreadSpeedScale", &mut w.fTreadSpeedScale);
            g("fPeakGrip", &mut w.fPeakGrip);
            g("fGripAngAdd", &mut w.fGripAngAdd);
            g("fWheelInertia", &mut w.fWheelInertia);
            g("fBrakeTorque", &mut w.fBrakeTorque);
            g("fTorqueSplit", &mut w.fTorqueSplit);
            g("fMaxSteeringLock", &mut w.fMaxSteeringLock);
            g("fSuspensionStiffness", &mut w.fSuspensionStiffness);
            g("fSuspensionDamping", &mut w.fSuspensionDamping);
            g("fSuspensionExponent", &mut w.fSuspensionExponent);
            g("fSuspensionTravel", &mut w.fSuspensionTravel);
            g("fWheelRadius", &mut w.fWheelRadius);
            g("fOptimumLoad", &mut w.fOptimumLoad);
            g("fTyreExponent", &mut w.fTyreExponent);
            g("fWheelBearingResistance", &mut w.fWheelBearingResistance);
        }
        f("m_fToeIn", &mut self.m_fToeIn);
        if let Some(v) = top.get("m_iNumAntiRollBars").and_then(|s| s.trim().parse::<i64>().ok()) {
            self.m_iNumAntiRollBars = (v.max(0) as usize).min(4);
        }
        for i in 0..self.m_iNumAntiRollBars {
            let a = xml_tag_attrs(xml, &format!("m_aAntiRollBars_{i}"));
            if let Some(v) = a.get("iLeftWheel").and_then(|s| s.trim().parse::<i32>().ok()) {
                self.m_aAntiRollBars[i].iLeftWheel = v;
            }
            if let Some(v) = a.get("iRightWheel").and_then(|s| s.trim().parse::<i32>().ok()) {
                self.m_aAntiRollBars[i].iRightWheel = v;
            }
            if let Some(v) = a.get("fStiffness").and_then(|s| s.trim().parse::<f64>().ok()) {
                self.m_aAntiRollBars[i].fStiffness = v as f32;
            }
        }
        f("m_vCOMOffset_x", &mut self.m_vCOMOffset.x);
        f("m_vCOMOffset_y", &mut self.m_vCOMOffset.y);
        f("m_vCOMOffset_z", &mut self.m_vCOMOffset.z);
        f("m_fInertia", &mut self.m_fInertia);
        for k in 0..40 {
            let key = format!("m_fTorqueLbFt_{}RPM", k * 500);
            if let Some(v) = top.get(&key).and_then(|s| s.trim().parse::<f64>().ok()) {
                self.m_fTorqueLbFt[k] = v as f32;
            }
        }
        f("m_fLowerRevLimit", &mut self.m_fLowerRevLimit);
        f("m_fUpperRevLimit", &mut self.m_fUpperRevLimit);
        f("m_fShiftUpRPM", &mut self.m_fShiftUpRPM);
        f("m_fShiftDownRPM", &mut self.m_fShiftDownRPM);
        f("m_fFirstShiftRPM", &mut self.m_fFirstShiftRPM);
        f("m_fClutchStiffness", &mut self.m_fClutchStiffness);
        f("m_fClutchLimit", &mut self.m_fClutchLimit);
        f("m_fEngineFriction", &mut self.m_fEngineFriction);
        f("m_fEngineInertia", &mut self.m_fEngineInertia);
        f("m_fEngineChassisTorque", &mut self.m_fEngineChassisTorque);
        f("m_fTCS", &mut self.m_fTCS);
        f("m_fDrag", &mut self.m_fDrag);
        f("m_fMinDesiredSpeed", &mut self.m_fMinDesiredSpeed);
        f("m_fDownForce", &mut self.m_fDownForce);
        f("m_fTorqueScalar", &mut self.m_fTorqueScalar);
        f("m_vDownForcePos_x", &mut self.m_vDownForcePos.x);
        f("m_vDownForcePos_y", &mut self.m_vDownForcePos.y);
        f("m_vDownForcePos_z", &mut self.m_vDownForcePos.z);
        f("m_fMass", &mut self.m_fMass);
        f("m_fSteeringSpeedScale", &mut self.m_fSteeringSpeedScale);
        f("m_fSoftSteeringScale", &mut self.m_fSoftSteeringScale);
        f("m_fMaxSpeedSteerScale", &mut self.m_fMaxSpeedSteerScale);
        f("m_fSlidingTargetAngularAccel", &mut self.m_fSlidingTargetAngularAccel);
        f("m_fSlidingTargetAngularVel", &mut self.m_fSlidingTargetAngularVel);
        f("m_fSteerAdjRate", &mut self.m_fSteerAdjRate);
        f("m_fCentreSteerAdjRate", &mut self.m_fCentreSteerAdjRate);
        f("m_fAirborneSteerScale", &mut self.m_fAirborneSteerScale);
        f("m_fAirborneRotation", &mut self.m_fAirborneRotation);
        f("m_fAirborneLateralGrip", &mut self.m_fAirborneLateralGrip);
        f("m_fBelowMinSpeedThrust", &mut self.m_fBelowMinSpeedThrust);
        if let Some(v) = top.get("m_iClass").and_then(|s| s.trim().parse::<i32>().ok()) {
            self.m_iClass = v;
        }
        f("m_fFullRepairCost", &mut self.m_fFullRepairCost);
        f("m_fFragility", &mut self.m_fFragility);
    }

    /// `CCarSpec::CopyWithMods @ 001b5700` (and the identical `CCarSpec(const CCarSpec*, const CModSpec*) @ 001b5018`):
    /// copies `base`, then lerps fragility, thrust, drag, min desired speed, sliding angular velocity and every wheel's
    /// peak grip between the `min` and `max` specs by the `CModSpec` ratios.
    pub fn copy_with_mods(base: &CarSpec, min: &CarSpec, max: &CarSpec, mods: &CModSpec) -> CarSpec {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let mut s = base.clone();
        s.m_fFragility = lerp(min.m_fFragility, max.m_fFragility, mods.fragility); // 0x480, +0x14
        s.m_fBelowMinSpeedThrust = lerp(min.m_fBelowMinSpeedThrust, max.m_fBelowMinSpeedThrust, mods.thrust); // 0x434, +0x18
        s.m_fDrag = lerp(min.m_fDrag, max.m_fDrag, mods.drag); // 0x42c, +0x1c
        s.m_fMinDesiredSpeed = lerp(min.m_fMinDesiredSpeed, max.m_fMinDesiredSpeed, mods.min_speed); // 0x438, +0x24
        s.m_fSlidingTargetAngularVel = lerp(min.m_fSlidingTargetAngularVel, max.m_fSlidingTargetAngularVel, mods.sliding_ang_vel); // 0x460, +0x20
        for i in 0..s.m_iNumWheels {
            s.wheels[i].fPeakGrip = lerp(min.wheels[i].fPeakGrip, max.wheels[i].fPeakGrip, mods.grip); // +0x10
        }
        s
    }
}

/// Attribute map of the first element named `tag` (flat, decoded XML as produced by `abgtool xox`).
fn xml_tag_attrs(xml: &str, tag: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(rel) = xml[from..].find(&open) {
        let start = from + rel + open.len();
        let next = xml[start..].chars().next();
        if matches!(next, Some(' ') | Some('/') | Some('>') | Some('\t') | Some('\n') | Some('\r')) {
            let rest = &xml[start..];
            let end = rest.find('>').unwrap_or(rest.len());
            let mut body = &rest[..end];
            while let Some(eq) = body.find("=\"") {
                let name = body[..eq].trim().trim_end_matches('/').to_string();
                let vs = eq + 2;
                let Some(q) = body[vs..].find('"') else { break };
                map.insert(name, body[vs..vs + q].to_string());
                body = &body[vs + q + 1..];
            }
            return map;
        }
        from = start;
    }
    map
}

// ===========================================================================================================
// CXGSRigidBody (the parts the car uses)
// ===========================================================================================================

/// Dynamic rigid body: `CXGSRigidBody` fields at the offsets noted (type 0 = dynamic branch of `Integrate @ 005b73e8`).
#[derive(Clone, Debug)]
pub struct RigidBody {
    pub pos: Vec3,      // +0x38 (centre of mass)
    pub vel: Vec3,      // +0x10
    pub ang_vel: Vec3,  // +0x00 (world frame)
    pub rot: Quat,      // +0x44 (x,y,z,w), body -> world
    pub mass: f32,      // +0x8c
    pub inertia: f32,   // +0x88 (scalar, multiplied by mass: 1/(m*I) = +0x2cc)
    pub gravity: Vec3,  // +0x54
    /// Quadratic drag coefficients (+0x78 x(right), +0x7c y(up), +0x80 z(forward)); acts as `a = -drag*v|v|/mass`.
    pub drag: Vec3,
    pub down_k: f32,    // +0x90
    pub down_pos: Vec3, // +0x60
    pub down_cap: f32,  // +0x94 (speed cap for the downforce term)
    pub wind: Vec3,     // +0x6c
    pub lin_damp: f32,  // +0xe8
    pub ang_damp: f32,  // +0xec
    pub dt: f32,        // +0x98
}

impl RigidBody {
    /// `CXGSRigidBody::CXGSRigidBody @ 005b4f74` defaults + `SetMass @ 005b5584`, `SetInertia @ 005b6b3c`.
    pub fn new(mass: f32, inertia: f32, dt: f32) -> RigidBody {
        RigidBody {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            rot: Quat::IDENTITY,
            mass,
            inertia,
            gravity: Vec3::ZERO,
            drag: Vec3::ZERO,
            down_k: 0.0,
            down_pos: Vec3::ZERO,
            down_cap: f32::INFINITY,
            wind: Vec3::ZERO,
            lin_damp: 0.0,
            ang_damp: 0.0,
            dt,
        }
    }
    /// CXGSRigidBody+0x30 = 1/mass (SetMass @ 005b5584).
    fn inv_mass(&self) -> f32 {
        1.0 / self.mass // +0x30
    }
    /// CXGSRigidBody+0x2cc = 1/(mass*inertia) (SetMass @ 005b5584 / SetInertia @ 005b6b3c).
    fn inv_ang(&self) -> f32 {
        1.0 / (self.mass * self.inertia) // +0x2cc
    }
    /// Body axes in world space: `(C = body x / right, A = body y / up, B = body z / forward)`. In the decompile the
    /// collision-object matrix rows at +0x28 (C), +0x10 (A), +0x1c (B) are `q * x`, `q * y`, `q * z`.
    pub fn axes(&self) -> (Vec3, Vec3, Vec3) {
        (self.rot * Vec3::X, self.rot * Vec3::Y, self.rot * Vec3::Z)
    }

    /// `CXGSRigidBody::ApplyWorldForce @ 005b57a0`: `f` is an impulse (force * dt), `p` the world application point.
    pub fn apply_world_force(&mut self, f: Vec3, p: Vec3) {
        self.vel += f * self.inv_mass();
        self.ang_vel += (p - self.pos).cross(f) * self.inv_ang();
    }

    /// `CXGSRigidBody::ApplyWorldForceTorqueOnly @ 005b58b4`.
    pub fn apply_world_force_torque_only(&mut self, f: Vec3, p: Vec3) {
        self.ang_vel += (p - self.pos).cross(f) * self.inv_ang();
    }

    /// `CXGSRigidBody::ApplyBodyForce @ 005b5b3c`: force and point in body space.
    pub fn apply_body_force(&mut self, f_local: Vec3, p_local: Vec3) {
        let tau = p_local.cross(f_local);
        self.ang_vel += (self.rot * tau) * self.inv_ang();
        self.vel += (self.rot * f_local) * self.inv_mass();
    }

    /// Dynamic branch of `CXGSRigidBody::Integrate @ 005b73e8` (quadratic drag, speed-squared downforce, gravity,
    /// linear/angular damping, quaternion and position integration). `0x2b4..0x2bc` = m*g*dt impulse, `0x2c0..0x2c8` = drag*dt.
    // UNRESOLVED: the rest of Integrate (collision detection against the track KD-tree, contact solver
    // `CXGSSequentialImpulseSolver`, sleeping/wake logic, swept collisions) is engine code that is not ported; the
    // chassis does not collide with the world here, only the wheel rays hold the car up.
    pub fn integrate(&mut self) {
        let dt = self.dt;
        let (c, a, b) = self.axes();
        let rel = self.vel - self.wind;
        let d_a = rel.dot(a);
        let d_c = rel.dot(c);
        let d_b = rel.dot(b);
        // downforce term: -(dt * min(|rel - A (rel.A)|^2, cap^2) * k) along A, applied at down_pos (body space)
        let perp = rel - a * d_a;
        let cap2 = self.down_cap * self.down_cap;
        let perp2 = perp.length_squared().min(cap2);
        let down = -(dt * perp2 * self.down_k);
        // torque = P x (0, down, 0) in body space, rotated to world, scaled by 1/(m*I)
        let tau_local = Vec3::new(-down * self.down_pos.z, 0.0, down * self.down_pos.x);
        self.ang_vel += (self.rot * tau_local) * self.inv_ang();
        let inv_m = self.inv_mass();
        self.vel += a * down * inv_m;
        // drag + gravity impulse
        let imp = a * (-(d_a * d_a.abs()) * self.drag.y * dt)
            + c * (-(d_c * d_c.abs()) * self.drag.x * dt)
            + b * (-(d_b * d_b.abs()) * self.drag.z * dt)
            + self.gravity * (self.mass * dt);
        self.vel += imp * inv_m;
        // damping (1 - dt*0xe8 / 0xec)
        self.vel *= 1.0 - dt * self.lin_damp;
        self.ang_vel *= 1.0 - dt * self.ang_damp;
        // q += 0.5 * (dt*w, 0) * q ; normalise
        let w = self.ang_vel * dt;
        let q = self.rot;
        let dq = Quat::from_xyzw(
            0.5 * (w.x * q.w + w.y * q.z - w.z * q.y),
            0.5 * (-w.x * q.z + w.y * q.w + w.z * q.x),
            0.5 * (w.x * q.y - w.y * q.x + w.z * q.w),
            0.5 * (-w.x * q.x - w.y * q.y - w.z * q.z),
        );
        self.rot = Quat::from_xyzw(q.x + dq.x, q.y + dq.y, q.z + dq.z, q.w + dq.w).normalize();
        self.pos += self.vel * dt;
    }
}

// ===========================================================================================================
// CWheel
// ===========================================================================================================

/// Per-wheel ground query result (`CWheel_TGroundInfo`, stride 0x80 at car+0x120; +0x58 material, +0x5c ray origin,
/// +0x68 hit point, +0x74 normal).
#[derive(Clone, Copy, Debug)]
pub struct GroundInfo {
    pub material: i32, // -1 = nothing under the wheel
    pub origin: Vec3,
    pub hit: Vec3,
    pub normal: Vec3,
}
impl Default for GroundInfo {
    /// Interface/helper: empty ground info = nothing under the wheel (material -1, as UpdateGroundInfo @ 0019f2b8 initialises).
    fn default() -> Self {
        GroundInfo { material: -1, origin: Vec3::ZERO, hit: Vec3::ZERO, normal: Vec3::Y }
    }
}

/// `CWheel` (fields keep the decompile offsets in comments).
#[derive(Clone, Debug)]
pub struct Wheel {
    pub spec: WheelSpec,
    pub pos: Vec3,            // +0x1c model-space attachment
    pub attach: Vec3,         // +0x44 = pos + spec.m_vCOMOffset (ctor adds *(spec+0x350))
    pub steer: f32,           // +0x2c steering angle (rad)
    pub len: f32,             // +0x28 current suspension length
    pub rest_len: f32,        // +0x70 = fSuspensionTravel
    pub min_len: f32,         // +0x74
    pub phase: f32,           // +0x30 rotation angle (rad)
    pub tread: f32,           // +0x34 tread texture phase (render only)
    pub spin: f32,            // +0x38 wheel surface speed (m/s)
    pub brake_torque: f32,    // +0x3c (set by DoBrakes)
    pub material: i32,        // +0xac
    pub slip_over: f32,       // +0xb0 (mu - 1 when beyond peak, drives skid effects)
    pub slip_over_smooth: f32, // +0xb4
    pub load: f32,            // +0xb8 suspension force impulse accumulator
    pub alpha: f32,           // +0xbc slip angle (rad)
    pub slip_ratio: f32,      // +0xc0
    pub force_lat: f32,       // +0xc4
    pub force_long: f32,      // +0xc8
    pub on_ground: bool,      // +0xe4
    pub contact_vel: Vec3,    // +0x88
    pub hit_point: Vec3,      // +0x7c
    pub normal: Vec3,         // +0xa0
    pub long_axis: Vec3,      // +0x94
    pub peak_ratio: f32,      // +0xd8 peak slip ratio of the current surface (0 when no tyre force was computed)
    pub slip_speed: f32,      // +0xdc
    pub lat_speed: f32,       // +0xe0
    // precalc (CWheel::Precalc @ 001b8430)
    k_spring: f32,    // +0x50 stiffness*dt
    inv_inertia: f32, // +0x58
    k_brake: f32,     // +0x5c dt*radius/inertia
    k_bearing: f32,   // +0x60 dt*bearing resistance
    neg_peak: f32,    // +0x54 -fPeakGrip
    load_unit: f32,   // +0x64 dt*optimumLoad
    load_index: f32,  // +0x68 128/(dt*optimumLoad)
    table: Vec<f32>,  // +0x6c tyre load sensitivity table for fTyreExponent
    dt: f32,
}

impl Wheel {
    /// `CWheel::CWheel @ 001b788c`. `dt` = `rb+0x98` (the ctor falls back to 1/60 without a body).
    /// `min_len`: see UNRESOLVED below.
    pub fn new(spec: &WheelSpec, pos: Vec3, com_offset: Vec3, dt: f32, min_len: f32) -> Wheel {
        let mut w = Wheel {
            spec: spec.clone(),
            pos,
            attach: pos + com_offset,
            steer: 0.0,
            len: spec.fSuspensionTravel,
            rest_len: spec.fSuspensionTravel,
            min_len,
            phase: 0.0,
            tread: 0.0,
            spin: 0.0,
            brake_torque: 0.0,
            material: 0,
            slip_over: 0.0,
            slip_over_smooth: 0.0,
            load: 0.0,
            alpha: 0.0,
            slip_ratio: 0.0,
            force_lat: 0.0,
            force_long: 0.0,
            on_ground: false,
            contact_vel: Vec3::ZERO,
            hit_point: Vec3::ZERO,
            normal: Vec3::Y,
            long_axis: Vec3::ZERO,
            peak_ratio: 0.0,
            slip_speed: 0.0,
            lat_speed: 0.0,
            k_spring: 0.0,
            inv_inertia: 0.0,
            k_brake: 0.0,
            k_bearing: 0.0,
            neg_peak: 0.0,
            load_unit: 0.0,
            load_index: 0.0,
            table: Vec::new(),
            dt,
        };
        w.precalc(dt);
        w
    }

    /// `CWheel::Precalc @ 001b8430` (also run by `CCar::NotifyBaseTimeStepChanged @ 0019b2d0`).
    pub fn precalc(&mut self, dt: f32) {
        let s = &self.spec;
        self.dt = dt;
        self.k_spring = s.fSuspensionStiffness * dt; // +0x50
        self.k_brake = (dt * s.fWheelRadius) / s.fWheelInertia; // +0x5c
        self.inv_inertia = 1.0 / s.fWheelInertia; // +0x58
        self.k_bearing = dt * s.fWheelBearingResistance; // +0x60
        self.load_unit = dt * s.fOptimumLoad; // +0x64
        self.load_index = 128.0 / (dt * s.fOptimumLoad); // +0x68 (DAT_001b8600/001b85f4 = 128.0 read from binary)
        self.neg_peak = -s.fPeakGrip; // +0x54
        self.rest_len = s.fSuspensionTravel; // +0x70
        self.table = precalc_tyre_load_sensitivity(s.fTyreExponent); // +0x6c
        self.table_ok();
    }
    /// Interface/helper (no original function): table length sanity check.
    fn table_ok(&self) {
        debug_assert_eq!(self.table.len(), 256);
    }

    /// `CWheel::CalcEffectiveLoad @ 001b8610`: tyre-load-sensitivity table lookup for a suspension load impulse.
    pub fn calc_effective_load(&self, load: f32) -> f32 {
        self.load_sensitivity(load) * self.load_unit
    }
    /// The table interpolation shared by `CalcEffectiveLoad` and the inline copy in `CWheel::Integrate`
    /// (index = load * 128/(dt*optimumLoad), 256 entries, clamped at 255).
    fn load_sensitivity(&self, load: f32) -> f32 {
        let (i, j, frac);
        if load > 0.0 {
            let x = load * self.load_index;
            let idx = x as i32;
            if idx < 256 {
                if idx == 255 {
                    i = 255;
                    j = 255;
                    frac = x - 255.0;
                } else {
                    i = idx as usize;
                    j = idx as usize + 1;
                    frac = x - idx as f32;
                }
            } else {
                i = 255;
                j = 255;
                frac = x - 255.0;
            }
        } else {
            i = 0;
            j = 1;
            frac = 0.0;
        }
        let a = self.table[i];
        a + (self.table[j] - a) * frac
    }

    /// `CWheel::Integrate(CWheel_TGroundInfo*, float, float) @ 001b86b4`.
    /// `car_speed` = `CCar+0x1ab4`, `car_brake` = `CCar+0x4fc`; `p2`/`p3` are the two scale arguments the caller passes
    /// (`base * car+0x534` and `1/car+0x534`, both 1.0 for a stock car).
    pub fn integrate(&mut self, gi: &GroundInfo, rb: &mut RigidBody, car_speed: f32, car_brake: f32, p2: f32, p3: f32) {
        let dt = rb.dt;
        self.on_ground = false;
        self.peak_ratio = 0.0; // +0xd8 (reset every step; set again when a tyre force is computed)
        let mut contact = false;
        let mut cur_len = self.rest_len;
        if gi.material == -1 {
            self.material = 0;
            self.slip_over = 0.0;
            self.slip_over_smooth = 0.0;
            self.alpha = 0.0;
            self.slip_ratio = 0.0;
            self.len = self.rest_len;
        } else {
            self.material = gi.material;
            let (_, a_up, b_fwd) = rb.axes();
            let (c_right, _, _) = rb.axes();
            let mut p = gi.hit;
            let bump = phys_material_bump_offset(gi.material, p);
            self.hit_point = gi.hit; // +0x7c (before the bump)
            p += a_up * bump;
            let r = p - rb.pos;
            self.contact_vel = rb.vel + rb.ang_vel.cross(r); // +0x88
            self.normal = gi.normal; // +0xa0
            // suspension length measured along the car's up axis from the ray origin
            let len = -(a_up.dot(p - gi.origin));
            if len < self.rest_len {
                contact = true;
                self.on_ground = true;
                let s = &self.spec;
                let mut len = len;
                if len < self.min_len {
                    len = self.min_len;
                }
                cur_len = len;
                let mut comp = (self.rest_len - len) * self.k_spring;
                if (s.fSuspensionExponent - 1.0).abs() > 0.001 {
                    comp = powf_fast(comp, s.fSuspensionExponent);
                }
                let mut damp = (len - self.len) * s.fSuspensionDamping;
                if damp > 0.0 {
                    damp *= 0.5;
                }
                let mut b8 = (comp - damp) + self.load; // + anti-roll accumulator already in +0xb8
                // slope scaling of the spring impulse
                let cosang = a_up.dot(self.normal);
                let mut sc = 0.6 + (cosang - 0.6) * 1.25;
                if sc < 0.0 {
                    sc = 0.0;
                } else if sc > 1.0 {
                    sc = 1.0;
                }
                b8 = b8 * sc * p3;
                self.load = b8;
                if b8 > 0.0 {
                    let m = phys_material(gi.material);
                    let grip_scale = m.peak_grip_scale;
                    let peak_angle = (m.peak_grip_slip_angle + s.fGripAngAdd) * 0.0175; // +0xd4
                    let peak_ratio = m.peak_grip_slip_ratio; // +0xd8
                    self.peak_ratio = peak_ratio;
                    let mut lf = a_up * b8;
                    // surface damping (PhysMaterial_GetDamping) * load * dt along the contact velocity
                    let dmp = m.damping * b8 * dt;
                    lf -= self.contact_vel * dmp;
                    // wheel heading F = cos(steer) * forward + sin(steer) * right
                    let (sn, cs) = self.steer.sin_cos();
                    let f = a_up * 0.0 + c_right * sn + b_fwd * cs;
                    let n = self.normal;
                    let t = f.cross(n); // lateral axis
                    let u = n.cross(t); // longitudinal axis (+0x94)
                    self.long_axis = u;
                    let v = self.contact_vel;
                    let vl = v.dot(u);
                    let vlat = v.dot(t);
                    let slip = vl - self.spin; // +0xdc
                    self.lat_speed = vlat; // +0xe0
                    self.slip_speed = slip;
                    let denom = if vl.abs() < 1.0e-5 { 1.0e-5 } else { vl.abs() };
                    let dt4 = dt * 4.0;
                    let mut sr = 0.0f32;
                    if dt4 <= slip.abs() {
                        sr = slip / denom;
                    }
                    let mut alpha = (vlat / denom).atan();
                    if car_speed < 1.0 && car_brake > 0.1 && dt4 > vlat.abs() {
                        alpha = 0.0;
                    }
                    let alpha_n = alpha * (1.0 / peak_angle);
                    let sr_n = sr * (1.0 / peak_ratio);
                    self.slip_ratio = sr;
                    self.alpha = alpha;
                    let mu = (1.0e-5 + alpha_n * alpha_n + sr_n * sr_n).sqrt();
                    if mu > 1.0 && (slip.abs() > 0.5 || vlat.abs() > 0.5) {
                        self.slip_over = mu - 1.0;
                    } else {
                        self.slip_over = 0.0;
                    }
                    self.slip_over_smooth += (self.slip_over - self.slip_over_smooth) * 0.5;
                    let mut f_long = sr_n;
                    let mut f_lat = alpha_n;
                    if mu > 1.0e-5 {
                        let l = self.load_sensitivity(self.load);
                        let fr = self.neg_peak * (l * self.load_unit);
                        let fr = grip_scale * fr * p2;
                        let inv_mu = 1.0 / mu;
                        let softness = m.softness;
                        let s_long = slip.abs().sqrt() * softness;
                        let s_lat = softness * vlat.abs().sqrt();
                        let ta = slip_curve(mu);
                        let tb = slip_curve(mu);
                        f_long = (s_long + ta) * sr_n * inv_mu * fr;
                        f_lat = (s_lat + tb) * alpha_n * inv_mu * fr;
                    }
                    // wheel spin update from the longitudinal force
                    let mut w38 = self.spin - f_long * self.inv_inertia;
                    // clamp when the wheel overshoots the road speed
                    if slip > 0.0 && w38 > vl {
                        self.spin = vl;
                        f_long += (w38 - vl) * s.fWheelInertia;
                        w38 = vl;
                    } else if slip < 0.0 && w38 < vl {
                        self.spin = vl;
                        f_long += (w38 - vl) * s.fWheelInertia;
                        w38 = vl;
                    } else {
                        self.spin = w38;
                    }
                    // brake torque slows the wheel towards zero
                    let bt = self.brake_torque * self.k_brake;
                    if w38 > 0.0 {
                        w38 -= bt;
                        if w38 < 0.0 {
                            w38 = 0.0;
                        }
                    } else {
                        w38 += bt;
                        if w38 > 0.0 {
                            w38 = 0.0;
                        }
                    }
                    self.spin = w38;
                    lf += u * f_long + t * f_lat;
                    self.force_long = f_long; // +0xc8
                    self.force_lat = f_lat; // +0xc4
                    rb.apply_world_force(lf, p);
                }
            } else {
                self.len = self.rest_len;
            }
        }
        if contact {
            self.len = cur_len;
        }
        if gi.material == -1 || !contact {
            self.slip_over = 0.0;
            self.slip_ratio = 0.0;
            self.alpha = 0.0;
            self.slip_over_smooth = 0.0;
        }
        // tail (label 001b8848): bearing resistance and rotation integration
        let w = self.spin - self.spin * self.k_bearing;
        self.spin = w;
        let mut tread = self.tread + w * dt;
        // The original wraps this render-only phase with a +/-1000 toggle (DAT_001b88e0); kept as written.
        if tread > 1000.0 {
            tread -= 1000.0;
        } else if tread < 1000.0 {
            tread += 1000.0;
        }
        self.tread = tread;
        self.phase += dt * (w / self.spec.fWheelRadius);
    }
}

// ===========================================================================================================
// CCar
// ===========================================================================================================

/// `CCar::CalcRestingWheelPosition(const CCarSpec*) @ 0019e948`: mean suspension travel minus the static compression,
/// `avg(travel) - (mass*9.8)^(1/avg(exponent)) / sum(stiffness)` (`DAT_0019ea78` = 9.8).
pub fn calc_resting_wheel_position(spec: &CarSpec) -> f32 {
    let n = spec.m_iNumWheels.min(6);
    let (mut st, mut tr, mut ex) = (0.0f32, 0.0f32, 0.0f32);
    for w in &spec.wheels[..n] {
        st += w.fSuspensionStiffness;
        tr += w.fSuspensionTravel;
        ex += w.fSuspensionExponent;
    }
    let inv = 1.0 / n as f32;
    let pow = (spec.m_fMass * 9.8).powf(1.0 / (ex * inv));
    tr * inv - pow / st
}

/// `CCar::CalcRestingHeight(const CCarModel*, const CCarSpec*) @ 0019ea7c`: height of the body origin above the ground
/// at rest: `(avg(travel) - (mass*9.8)^(1/avg(exp))/sum(stiffness)) - avg(hub y) - COM.y`.
/// `wheel_positions` = the model hub positions (`CCarModel::GetWheelPos`).
pub fn calc_resting_height(wheel_positions: &[Vec3], spec: &CarSpec) -> f32 {
    let n = spec.m_iNumWheels.min(6).min(wheel_positions.len());
    let avg_y = wheel_positions[..n].iter().map(|p| p.y).sum::<f32>() * (1.0 / n as f32);
    calc_resting_wheel_position(spec) - avg_y - spec.m_vCOMOffset.y
}

/// What the player/AI feeds the car each frame (`CCar::SetSteering @ 0019b198`, `SetBrake @ 0019b2c4`).
#[derive(Clone, Copy, Debug, Default)]
pub struct CarInput {
    /// -1..1 (`CCar+0x4c4`).
    pub steer: f32,
    /// 0..1 (`CCar+0x4fc`).
    pub brake: f32,
    /// Direction of the race spline at the car (used by the BelowMinSpeedThrust gate and boost pads).
    // UNRESOLVED: the original reads `CSpline` data of the race track; tracks are unavailable, so the caller supplies it.
    // `None` = aligned with the car (thrust gate passes, boost pushes along the car's forward axis).
    pub track_direction: Option<Vec3>,
    /// Speed-boost power-up active (`CPlayerInfo::IsPowerUpActive(2)` in `CCar::Integrate`).
    pub boost: bool,
    /// `Some(angle)` = `CCar::SetSteerAngle` (direct wheel angle, input integration off); `None` = `SetSteering(steer)`.
    pub steer_angle: Option<f32>,
    /// `CCar+0x1be0`: the kart overlaps an item boost pad (`CPickupBoostPad`, reported by `ItemWorld` as `BoostPadHit`); same push as the 0x18 / 0x22 pad materials.
    pub pad_boost: bool,
}

/// Slingshot launch inputs for [`slingshot_launch`] (`CCar::GetLaunchVelocity @ 00199c04`).
#[derive(Clone, Copy, Debug)]
pub struct LaunchParams {
    /// `*(*(game+0x2c2c) + game+0x2f0*0x10 + 0xc)`: per-selection launch force scale.
    // UNRESOLVED: that table (`CGame` +0x2c2c, indexed by +0x2f0) is game/event data that was not located; default 1.0.
    pub launch_force_scale: f32,
    /// `CGame::GetGameMode() == 10` selects speeds 22..52 instead of 30..50.
    pub game_mode_10: bool,
    /// `CPlayer+0x230` for a player-driven car; `None` = AI path (`min(|pull|/8.4, 1)`).
    // UNRESOLVED: CPlayer::UpdateSlingshotLaunch @ 00147a90 (touch input + camera matrices) that produces +0x230 is not ported.
    pub player_pull_ratio: Option<f32>,
    /// `CDebugManager::GetDebugFloat(0x5c)` multiplier when power-up 0 is active.
    pub power_up_scale: Option<f32>,
}
impl Default for LaunchParams {
    /// Interface/helper: neutral launch parameters (no original function).
    fn default() -> Self {
        LaunchParams { launch_force_scale: 1.0, game_mode_10: false, player_pull_ratio: None, power_up_scale: None }
    }
}

/// Result of the launch computation; the extra fields are what `GetLaunchVelocity` stores into `CCar+0x480..0x498`
/// when its `param_2` flag is set (used by the in-slingshot phase of `CCar::Integrate`).
#[derive(Clone, Copy, Debug)]
pub struct LaunchInfo {
    pub velocity: Vec3,      // returned vector
    pub speed: f32,          // +0x480/+0x484 : |v| before lift
    pub lift: f32,           // +0x488/+0x48c : (1 - ratio) * 7
    pub horizontal_dir: Vec3, // +0x490..0x498
}

/// `CCar::GetLaunchSpeedScale @ 00199b84`.
pub fn launch_speed_scale(pull_offset: Vec3, player_pull_ratio: Option<f32>) -> f32 {
    match player_pull_ratio {
        Some(r) => r,
        None => (pull_offset.length() / 8.4).min(1.0), // DAT_00199c00 = 8.4
    }
}

/// `CCar::GetLaunchForce @ 0019999c` (AI/player pull force vector added to the camera "up" lift).
/// `cam_up` = the camera matrix row `(+0x10,+0x14,+0x18)` of `game+8 + idx*0x40 + 0x4520`.
pub fn slingshot_launch_force(pull_offset: Vec3, player_pull_ratio: Option<f32>, cam_up: Vec3) -> Vec3 {
    let f = match player_pull_ratio {
        None => pull_offset * -3000.0, // DAT_00199b70
        Some(r) => {
            let l2 = pull_offset.length_squared();
            let d = if l2 > 1.0e-6 { pull_offset / l2.sqrt() } else { Vec3::ZERO }; // zero vector fallback
            d * (r * -3000.0 * 8.4) // DAT_00199b74
        }
    };
    let up = cam_up.normalize_or_zero();
    f + up * (f.length() * 0.2) // DAT_00199b78
}

/// `CCar::GetLaunchVelocity @ 00199c04`: launch velocity for a slingshot pull `pull_offset` (`CCar+0x46c..0x474`,
/// set by `SetSlingshotOffset @ 0019aa8c`).
pub fn slingshot_launch(pull_offset: Vec3, p: &LaunchParams) -> LaunchInfo {
    let mag = pull_offset.length();
    // direction is the pull reversed; 0.001 threshold (DAT_00199f48), zero vector fallback (CXGSVector32::s_vZeroVector)
    let dir = if mag > 0.001 { pull_offset * (-1.0 / mag) } else { Vec3::ZERO };
    let ratio = launch_speed_scale(pull_offset, p.player_pull_ratio);
    let (lo, hi) = if p.game_mode_10 { (22.0, 52.0) } else { (30.0, 50.0) }; // DAT_00199f44/40
    let speed = lo + (hi - lo) * ratio;
    let mut v = dir * (p.launch_force_scale * speed);
    if p.player_pull_ratio.is_some() {
        if let Some(k) = p.power_up_scale {
            v *= k;
        }
    }
    let vmag = v.length();
    // DAT_00199f4c = 0.0: no constant vertical term in the lift; (1 - ratio) * 7.0
    let lift = ratio * 0.0 + (1.0 - ratio) * 7.0;
    let hl2 = v.x * v.x + v.z * v.z;
    let horizontal_dir = if hl2 > 1.0e-6 { Vec3::new(v.x, 0.0, v.z) / hl2.sqrt() } else { Vec3::ZERO };
    let nv = if vmag > 1.0e-6 { v / vmag } else { Vec3::ZERO };
    LaunchInfo { velocity: nv * (vmag + lift), speed: vmag, lift, horizontal_dir }
}

/// The original's tweakables that the car code reads (`CDebugManager::GetDebugFloat`); values from
/// `assets291/xml/gameplay/misc/debugtweakables.xml` (indices resolved through `SetDebugTweakablesFromXML @ 002c7258`).
#[derive(Clone, Copy, Debug)]
pub struct Tweakables {
    /// GetDebugFloat(0x28) = `Gravity_Scale` (2.0). Gravity = -9.8 * this (`DAT_001a8b2c` = -9.8, CCar ctor @ 001a8588).
    pub gravity_scale: f32,
    /// GetDebugFloat(0x22) = `Steer_Rate` (1.15), multiplies the arcade yaw-rate target in `IntegrateSteering`.
    pub steer_rate: f32,
    /// GetDebugFloat(0x4d) = `Spin_Out_Arcade_Steer_Rate` (0.25): yaw blend factor right after a collision (not used here:
    /// the "time since collision" timers `rb+0x2f4/0x2f8` stay at +inf without a collision solver).
    pub spin_out_arcade_steer_rate: f32,
    /// GetDebugFloat(0x57) = `Boost_Push_Factor` (4.0): scale of the speed-boost power-up push in `CCar::Integrate`.
    pub boost_push_factor: f32,
    /// GetDebugFloat(0x9d) = `Thermal_Strength` (1.0, parsed from "1.0f"): vertical wind speed in a thermal (surface id 0x1d volume) is this times
    /// `DAT_001a8580` = 50 m/s (`CCar::Update`).
    pub thermal_strength: f32,
}
impl Default for Tweakables {
    /// Interface/helper (no original function): the XML values listed above.
    fn default() -> Self {
        Tweakables { gravity_scale: 2.0, steer_rate: 1.15, spin_out_arcade_steer_rate: 0.25, boost_push_factor: 4.0, thermal_strength: 1.0 }
    }
}

/// Full car: spec, rigid body and four (up to six) wheels.
pub struct CarSim {
    pub spec: CarSpec,
    pub rb: RigidBody,
    pub wheels: Vec<Wheel>,
    pub ground_info: Vec<GroundInfo>,
    pub tweaks: Tweakables,
    pub launch: LaunchParams,
    /// Fixed physics step (`rb+0x98`). UNRESOLVED: the engine passes the value through a lost register argument of
    /// `CXGSPhys::SetBaseTimeStep`; 1/60 is the default `CWheel::CWheel` uses (`DAT_001b7c00` = 0.016666668).
    pub base_dt: f32,
    accum: f32,
    // CCar state (offsets in comments)
    steer_angle: f32,   // +0x4f8
    steer_input: f32,   // +0x4c4
    brake: f32,         // +0x4fc
    signed_speed: f32,  // +0x1aac forward speed
    abs_signed: f32,    // +0x1ab0
    speed: f32,         // +0x1ab4 |v|
    air_time: f32,      // +0x4ec
    ground_dist2: f32,  // +0x920 squared distance to the ground below (0 = >=3 wheels down)
    on_ground_prev: usize, // wheel contact count at the start of the step (`local_f8` in CCar::Integrate)
    drag_scale: f32,    // +0x538
    steer_scale: f32,   // +0x534
    /// Last wheel-angle target before the 3.25 rad/s slew (IntegrateSteering, for inspection/tests).
    pub steer_target: f32,
    /// Last arcade yaw-rate target (rad/s) written by IntegrateSteering (for inspection/tests).
    pub yaw_target: f32,
    /// `car+0x4c0 != 0`: steering by input (`SetSteering`), 0 = direct angle (`SetSteerAngle`).
    steer_input_mode: bool,
    /// `true` = local human player (simple DoBrakes branch); `false` = AI/remote car (per-wheel ABS branch).
    pub local_player: bool,
}

impl CarSim {
    /// `wheel_positions[i]` = model-space wheel attachment (the `CCarModel::GetWheelPos @ 001b1f08` helper nodes), order
    /// FL, FR, RR, RL, body axes x = right (the steering direction for positive angles), y = up, z = forward.
    // UNRESOLVED: GetWheelPos reads the helper nodes of the car model and negates x for flagged wheels; the caller supplies
    // already-resolved positions, so the left/right sign convention of +x is the caller's.
    // UNRESOLVED: `CWheel::CWheel` sets the minimum suspension length (+0x74) from the chassis collision object
    // (`*(rb+0x34)+0x4c`); the collision mesh is not available, `min_len` is -inf (never clamps) unless you pass one.
    pub fn new(spec: CarSpec, wheel_positions: &[Vec3], position: Vec3, yaw: f32) -> CarSim {
        Self::new_with(spec, wheel_positions, position, yaw, f32::NEG_INFINITY)
    }

    /// `CCar::CCar @ 001a8588` (rigid body setup, wheel creation).
    pub fn new_with(spec: CarSpec, wheel_positions: &[Vec3], position: Vec3, yaw: f32, chassis_min_len: f32) -> CarSim {
        assert!(spec.m_fMass > 0.0 && spec.m_fInertia > 0.0, "CarSpec has no mass/inertia: read the base car spec (see CarSpec::set_default)");
        let base_dt = 1.0 / 60.0;
        let tweaks = Tweakables::default();
        let mut rb = RigidBody::new(spec.m_fMass, spec.m_fInertia, base_dt);
        rb.pos = position;
        rb.rot = Quat::from_rotation_y(yaw);
        // SetGravity(vec(0, DebugFloat(0x28) * -9.8, 0)) (ctor @ 001a8588)
        rb.gravity = Vec3::new(0.0, tweaks.gravity_scale * -9.8, 0.0);
        // SetDrag(spec.drag, 3.0, spec.drag); SetDownForce(spec.m_fDownForce, spec.m_vDownForcePos) (ctor); both are
        // overwritten every frame by CCar::Update (see `update`).
        rb.drag = Vec3::new(spec.m_fDrag, 3.0, spec.m_fDrag);
        rb.down_k = spec.m_fDownForce;
        rb.down_pos = spec.m_vDownForcePos;
        let n = spec.m_iNumWheels.min(wheel_positions.len());
        let wheels: Vec<Wheel> = (0..n)
            .map(|i| Wheel::new(&spec.wheels[i], wheel_positions[i], spec.m_vCOMOffset, base_dt, chassis_min_len))
            .collect();
        CarSim {
            ground_info: vec![GroundInfo::default(); n],
            spec,
            rb,
            wheels,
            tweaks,
            launch: LaunchParams::default(),
            base_dt,
            accum: 0.0,
            steer_angle: 0.0,
            steer_input: 0.0,
            brake: 0.0,
            signed_speed: 0.0,
            abs_signed: 0.0,
            speed: 0.0,
            air_time: 0.0,
            ground_dist2: 0.0,
            on_ground_prev: 0,
            drag_scale: 1.0,
            steer_scale: 1.0,
            steer_target: 0.0,
            yaw_target: 0.0,
            steer_input_mode: true,
            local_player: true,
        }
    }

    /// Spawn on the ground at the suspension rest height: `y = ground_y + CCar::CalcRestingHeight @ 0019ea7c`
    /// (the original `CCar::SpawnOnGround @ 001a47d4` places the body the same way; its track-spline handling is not ported).
    pub fn new_on_ground(spec: CarSpec, wheel_positions: &[Vec3], x: f32, z: f32, ground_y: f32, yaw: f32) -> CarSim {
        let h = calc_resting_height(wheel_positions, &spec);
        Self::new(spec, wheel_positions, Vec3::new(x, ground_y + h, z), yaw)
    }

    /// `CCar::NotifyBaseTimeStepChanged @ 0019b2d0` + `CXGSRigidBody::SetTimeStep @ 005b5574`.
    pub fn set_base_time_step(&mut self, dt: f32) {
        self.base_dt = dt;
        self.rb.dt = dt;
        for w in self.wheels.iter_mut() {
            w.precalc(dt);
        }
    }

    /// Interface/helper accessor: signed forward speed, CCar+0x1aac (no original function).
    pub fn forward_speed(&self) -> f32 {
        self.signed_speed
    }
    /// Interface/helper accessor: |velocity|, CCar+0x1ab4 (no original function).
    pub fn speed(&self) -> f32 {
        self.speed
    }
    /// Interface/helper accessor: front wheel angle, CCar+0x4f8 (no original function).
    pub fn steer_angle(&self) -> f32 {
        self.steer_angle
    }
    /// Interface/helper accessor: alias of get_num_wheels_on_ground (no original function).
    pub fn wheels_on_ground(&self) -> usize {
        self.wheels.iter().filter(|w| w.on_ground).count()
    }
    /// Interface/helper accessor: body up axis (no original function).
    pub fn up(&self) -> Vec3 {
        self.rb.axes().1
    }
    /// Interface/helper accessor: body forward axis (no original function).
    pub fn forward(&self) -> Vec3 {
        self.rb.axes().2
    }

    /// `CCar::SetSteering(float) @ 0019b198`: steering by input (`car+0x4c4 = v`, `car+0x4c0 = 1`).
    // UNRESOLVED: the ability override (`car+0x1c2c` ability vfunc +0x54 replaces the value while an ability is active).
    pub fn set_steering(&mut self, v: f32) {
        self.steer_input = v;
        self.steer_input_mode = true;
    }

    /// `CCar::SetSteerAngle(float) @ 0019b204`: direct wheel angle, input integration off (`car+0x4c0 = 0`).
    pub fn set_steer_angle(&mut self, a: f32) {
        self.steer_input_mode = false;
        self.set_steer_angle_internal(a);
    }

    /// `CCar::SetSteerAngle_Internal(float) @ 0019b268`: clamp to +-pi/2, store, front wheels `a +- toe-in`.
    pub fn set_steer_angle_internal(&mut self, a: f32) {
        let a = a.clamp(-1.570_796_4, 1.570_796_4); // DAT_0019b2bc / DAT_0019b2c0
        self.steer_angle = a;
        let toe = self.spec.m_fToeIn;
        if let Some(w) = self.wheels.get_mut(0) {
            w.steer = toe + a;
        }
        if let Some(w) = self.wheels.get_mut(1) {
            w.steer = a - toe;
        }
    }

    /// `CCar::SetBrake(float) @ 0019b2c4`.
    pub fn set_brake(&mut self, v: f32) {
        self.brake = v;
    }

    /// `CCar::GetNumWheelsOnGround() @ 001a3e80`: plain count of wheels with `+0xe4 != 0`.
    pub fn get_num_wheels_on_ground(&self) -> usize {
        self.wheels.iter().filter(|w| w.on_ground).count()
    }

    /// `CCar::ApplyAcceleration(float,float) @ 0019ff08`: impulse `a * mass * dt` along the forward axis applied at
    /// `pos + up * -0.1` (DAT_001a0068). (The original also pushes attached bodywork pieces, not modelled.)
    pub fn apply_acceleration(&mut self, a: f32, dt: f32) {
        let (_, up, fwd) = self.rb.axes();
        let imp = fwd * (a * self.rb.mass * dt);
        let p = self.rb.pos + up * -0.1;
        self.rb.apply_world_force(imp, p);
    }

    /// Slingshot launch velocity for a pull offset using this car's [`LaunchParams`] (`CCar::GetLaunchVelocity @ 00199c04`).
    pub fn slingshot_launch(&self, pull_offset: Vec3) -> Vec3 {
        slingshot_launch(pull_offset, &self.launch).velocity
    }

    /// The release branch of `CCar::SetInSlingshot(-1) @ 00199f64`: the rigid body velocity becomes the `GetLaunchVelocity` vector, except that its
    /// component along the slingshot camera's forward axis (`game+8 + cam*0x40 + 0x4530`) is raised to at least 5 m/s.
    /// `CCar+0x480..0x498` (speed / lift / horizontal direction stored by `GetLaunchVelocity`) are read by `CCar::Integrate` only in game mode 10
    /// (the `0x490` velocity override and the `0x484` cap); the campaign race modes never read them.
    // UNRESOLVED: the early-launch branch (release before the pull timer `+0x45c` ran out: scale `GetDebugFloat(0x33) - ratio * GetDebugFloat(0x36)`,
    // `CPlayer::OnEarlyLaunch`, the spin penalty `+0x4a8..0x4b0`) is not ported; the normal path uses scale 1.0.
    pub fn slingshot_release(&self, pull_offset: Vec3, cam_forward: Vec3) -> Vec3 {
        let v = self.slingshot_launch(pull_offset);
        let f = cam_forward.normalize_or_zero();
        let along = v.dot(f);
        if along < 5.0 {
            v - f * along + f * 5.0
        } else {
            v
        }
    }

    /// One game frame: `CCar::Update @ 001a6cd8` once, then `CXGSPhys::Update @ 005b3e9c` (accumulate up to 0.1 s, fixed
    /// steps of `base_dt`, each = `CCar::Integrate` + `CXGSRigidBody::Integrate`).
    pub fn step(&mut self, dt: f32, input: &CarInput, ground: &dyn Ground) {
        match input.steer_angle {
            Some(a) => self.set_steer_angle(a),
            None => self.set_steering(input.steer),
        }
        self.set_brake(input.brake);
        // CCar::Update @ 001a6cd8: IsPosInMaterialVolume(id 0x1d) -> wind velocity (0, Thermal_Strength * 50, 0) (CXGSRigidBody::SetWindVelocity).
        // UNRESOLVED: ids 0x25 / 0x26 (wind along a per-spline-sample vector of the race track) do not occur in the 2.9.2 tracks, not ported.
        self.rb.wind = if ground.in_material_volume(self.rb.pos, 0x1d) { Vec3::new(0.0, self.tweaks.thermal_strength * 50.0, 0.0) } else { Vec3::ZERO };
        self.update(dt, input);
        let dt = dt.min(0.1);
        self.accum += dt;
        while self.accum >= self.base_dt {
            self.car_integrate(ground, input);
            self.rb.integrate();
            self.resolve_ground_penetration(ground);
            self.accum -= self.base_dt;
        }
    }

    /// The physics-relevant part of `CCar::Update @ 001a6cd8`: BelowMinSpeedThrust and the per-frame drag/downforce set-up.
    // UNRESOLVED (not applicable here): slipstream (`car+0x438`, needs other cars), material-volume wind
    // (`IsPosInMaterialVolume` ids 0x1d/0x25/0x26 need the track), multiplayer handicaps (tweakables 0xb1..0xb3).
    pub fn update(&mut self, dt: f32, input: &CarInput) {
        let (_, _, b) = self.rb.axes();
        // thrust (asm 001a6ddc..001a7ba0): brake < 0.1 (DAT_001a71b4) and spline direction . forward > 0
        if self.brake < 0.1 {
            let aligned = match input.track_direction {
                Some(d) => d.dot(b) > 0.0,
                None => true,
            };
            let min_speed = self.spec.m_fMinDesiredSpeed;
            if aligned && min_speed > self.signed_speed {
                let f = (min_speed - self.signed_speed).clamp(0.01, 1.0); // DAT_001a71b8 = 0.01 .. 1.0
                let imp = dt * self.spec.m_fBelowMinSpeedThrust * f;
                self.rb.apply_body_force(Vec3::new(0.0, 0.0, imp), Vec3::ZERO);
            }
        }
        // drag / downforce (asm 001a8054..001a8114); slipstream factor = 1 - 0.5*car+0x438 with car+0x438 = 0 (no other cars)
        let d = self.spec.m_fDrag * 1.0;
        if self.ground_dist2 < 4.0 || !(self.spec.m_fDownForce < 0.0) {
            self.rb.down_k = 0.0;
            self.rb.down_pos = self.spec.m_vDownForcePos;
            let k = d * self.drag_scale;
            self.rb.drag = Vec3::new(k, k, k);
            self.rb.down_cap = f32::INFINITY;
        } else {
            self.rb.down_k = self.spec.m_fDownForce;
            self.rb.down_pos = self.spec.m_vDownForcePos;
            self.rb.drag = Vec3::new(d * self.drag_scale, self.drag_scale * 2.0, d * self.drag_scale);
            self.rb.down_cap = 30.0; // 0x41f00000
        }
    }

    /// `CCar::DoBrakes() @ 0019ec5c`. Local human player (`local_player`): torque = `fBrakeTorque * brake` per wheel.
    /// Otherwise (AI / remote cars): per-wheel ABS - zero off the ground, full torque at <= 2 m/s, above that the torque
    /// is limited to `brake * allowed/slip_ratio` where `allowed = peakSlipRatio * (1 - |alpha|/(pi/2))`
    /// (`DAT_0019ee74` = 1.5707964, `DAT_0019ee78` = 0).
    fn do_brakes(&mut self) {
        for (i, w) in self.wheels.iter_mut().enumerate() {
            let t = self.spec.wheels[i].fBrakeTorque * self.brake;
            if self.local_player {
                w.brake_torque = t;
                continue;
            }
            let mut out = 0.0f32;
            if w.on_ground {
                out = t;
                if 2.0 < self.speed {
                    let sr = w.slip_ratio; // +0xc0
                    let a = w.alpha.abs() / 1.570_796_4; // +0xbc
                    let mut allowed = 0.0f32;
                    if a < 1.0 {
                        allowed = w.peak_ratio * (1.0 - a); // +0xd8
                    }
                    let lim;
                    if allowed < sr {
                        lim = (t * allowed) / sr;
                    } else {
                        allowed = -allowed;
                        if sr < allowed {
                            lim = (t * allowed) / sr;
                        } else {
                            lim = t;
                        }
                    }
                    out = 0.0;
                    if 0.0 <= t {
                        out = t;
                        if lim < t {
                            out = lim;
                        }
                    }
                }
            }
            w.brake_torque = out;
        }
    }

    /// `CCar::IntegrateSteering() [clone .part.137] @ 001974d4` (normal driving path, steering input mode `car+0x4c0 != 0`)
    /// plus the thin wrapper `CCar::IntegrateSteering @ 0019ee88`.
    // UNRESOLVED: (a) the airborne spline steering-assist (needs the race spline) is skipped; (b) the slingshot spin-out
    // penalty (`car+0x4ac`) and bird/pilot branch (`car+0x1bf0`) are not ported; (c) the airborne "bank" roll of the body
    // (`MakeZRotationMatrix32(clamp(-w_y*0.61086524/62.831856, +-0.61086524))` composed with the orientation through
    // `MatrixMultiply32_Fast`, only when `m_fAirborneSteerScale > 0` and few wheels touch) is NOT ported (matrix
    // multiply operand order not verified). With `m_fAirborneRotation > 0` the original forces the wheel count to 4; ported.
    fn integrate_steering(&mut self) {
        if !self.steer_input_mode {
            return; // SetSteerAngle mode (car+0x4c0 == 0): the angle was set directly, no input integration
        }
        let dt = self.rb.dt;
        let n_on = self.wheels.iter().filter(|w| w.on_ground).count();
        let spec = self.spec.clone();
        let nw = self.wheels.len();
        let (c_axis, a_axis, b_axis) = self.rb.axes();

        let mut input = self.steer_input.clamp(-1.0, 1.0);
        if self.signed_speed < 0.0 {
            input = -input;
        }
        // body slip angle: atan( (v . right) / max(|fwd speed|, 0.1) )
        let denom = if self.abs_signed < 0.1 { 0.1 } else { self.abs_signed };
        let beta = (self.rb.vel.dot(c_axis) / denom).atan();

        // average over steered wheels that touch something (PhysMaterial softness / slip angle), max steering lock
        let mut lock = 0.0f32;
        let mut sum_soft = 0.0f32;
        let mut sum_ang = 0.0f32;
        let mut cnt = 0.0f32;
        let mut have_lock0 = false;
        for i in 0..nw {
            let l = spec.wheels[i].fMaxSteeringLock;
            if i == 0 {
                lock = 0.0;
                if l > 0.0 {
                    if self.wheels[0].material != 0 {
                        let m = phys_material(self.wheels[0].material);
                        sum_soft += m.softness;
                        sum_ang += m.peak_grip_slip_angle;
                        cnt += 1.0;
                    }
                    lock = l;
                    have_lock0 = true;
                }
            } else if l > 0.0 {
                if self.wheels[i].material != 0 {
                    let m = phys_material(self.wheels[i].material);
                    sum_soft += m.softness;
                    sum_ang += m.peak_grip_slip_angle;
                    cnt += 1.0;
                }
                if lock < l {
                    lock = l;
                }
            }
        }
        let _ = have_lock0;
        let (avg_soft, avg_ang);
        if nw == 0 {
            avg_soft = 0.0;
            avg_ang = 12.0;
            lock = 0.0;
        } else if cnt <= 0.0 {
            avg_soft = 0.0;
            avg_ang = 12.0;
            if self.signed_speed < 0.1 && lock > 1.0 {
                lock = 1.0;
            }
        } else {
            avg_soft = sum_soft / cnt;
            avg_ang = sum_ang / cnt;
            if lock > 1.0 && self.signed_speed < 0.1 {
                lock = 1.0;
            }
        }
        // speed-limited steering angle
        let mut limit = lock;
        if self.speed > 0.1 {
            let soft = (avg_soft * 6.666_666_5).min(1.0); // DAT_0019826c
            let sp = self.speed.min(spec.m_fMaxSpeedSteerScale);
            let v = (spec.m_fSteeringSpeedScale * (soft * spec.m_fSoftSteeringScale + 1.0) * (avg_ang / 12.0)) / sp;
            limit = if v > lock { lock } else { v };
        }
        // target wheel angle
        let mut target;
        let reverse_assist;
        if input < 0.0 {
            let mut a = -limit;
            if beta < a {
                a = beta;
            }
            if a < -lock {
                a = -lock;
            }
            target = -(input * a);
            let b7 = beta < -0.12; // DAT_00198ba0
            reverse_assist = if self.speed >= 4.0 { !b7 } else { true };
        } else {
            let mut a = beta;
            if a <= limit {
                a = limit;
            }
            if lock < a {
                a = lock;
            }
            target = input * a;
            let b7 = 0.12 < beta; // DAT_00198270
            reverse_assist = if self.speed >= 4.0 { !b7 } else { true };
        }
        let reverse_assist = reverse_assist && self.signed_speed < 0.0;
        if reverse_assist {
            let t = input * limit * 1.5;
            target = t.clamp(-lock, lock);
        }
        // lateral velocity bleed (m_fAirborneLateralGrip)
        if spec.m_fAirborneLateralGrip > 0.0 {
            let vl = self.rb.vel.dot(c_axis);
            let step = spec.m_fAirborneLateralGrip * dt;
            let nv = if vl > 0.0 {
                (vl - step).max(0.0)
            } else if vl < 0.0 {
                (vl + step).min(0.0)
            } else {
                vl
            };
            self.rb.vel += c_axis * (nv - vl);
        }
        if input.abs() < 0.2 {
            target = 0.0; // DAT_00198274
        }
        self.steer_target = target;
        // slew at 3.25 rad/s
        let cur = self.steer_angle;
        if target <= cur {
            let lo = cur - dt * 3.25;
            if target < lo {
                target = lo;
            }
        } else {
            let hi = dt * 3.25 + cur;
            if hi <= target {
                target = hi;
            }
        }
        target = target.clamp(-1.570_796_4, 1.570_796_4);
        self.steer_angle = target;
        if nw > 0 {
            self.wheels[0].steer = target + spec.m_fToeIn;
        }
        if nw > 1 {
            self.wheels[1].steer = target - spec.m_fToeIn;
        }
        // arcade yaw rate (angular velocity steering)
        let count_for_rotation = if spec.m_fAirborneRotation > 0.0 { 4 } else { n_on };
        let mut inp = input;
        if self.signed_speed < 0.0 {
            inp = -inp;
        }
        if count_for_rotation > 1 {
            let mut yaw_in = inp * spec.m_fSlidingTargetAngularVel * self.steer_scale;
            if spec.m_fAirborneSteerScale > 0.0 && self.rb.down_k >= 0.0 {
                yaw_in *= 0.5;
            }
            if n_on < 2 && spec.m_fAirborneRotation > 0.0 {
                yaw_in *= spec.m_fAirborneRotation;
            }
            let speed_ramp = (self.speed * 0.1).min(1.0); // DAT_00198284 = 0.1
            let airborne_scale = if spec.m_fAirborneSteerScale > 0.0 { count_for_rotation < 3 } else { count_for_rotation == 0 };
            let scale_air = if airborne_scale { spec.m_fAirborneSteerScale } else { 1.0 };
            let blend = 0.5; // player car; 0.05 after a collision (rb+0x2f4/0x2f8 < 0.05), see Tweakables.spin_out_arcade_steer_rate
            let target_yaw = scale_air * speed_ramp * self.tweaks.steer_rate * yaw_in;
            self.yaw_target = target_yaw;
            let mut wl = self.rb.rot.inverse() * self.rb.ang_vel;
            wl.y += (target_yaw - wl.y) * blend;
            self.rb.ang_vel = self.rb.rot * wl;
            if spec.m_fAirborneSteerScale > 0.0 {
                // damp lateral and vertical velocity (relative to the wind) while the arcade steer scale is active
                let rel = self.rb.vel - self.rb.wind * 0.05;
                let ramp = if self.speed >= 10.0 { 1.0 } else { (self.speed * 0.1).clamp(0.0, 1.0) };
                let kc = (dt * ramp).min(1.0);
                let ka = (dt * ramp).min(1.0);
                let dc = rel.dot(c_axis) * kc;
                let da = rel.dot(a_axis) * ka;
                self.rb.vel -= c_axis * dc + a_axis * da;
            }
            let _ = b_axis;
        }
    }


    /// STAND-IN for the chassis-vs-world contact of `CXGSRigidBody::Integrate @ 005b73e8` (the collision object of the chassis is solved by
    /// `CXGSSequentialImpulseSolver` against the track KD-tree; none of that is ported). Without it a hard landing from a long jump puts the wheel
    /// hubs below the surface, where their rays (which only look `travel + 0.2` downwards) find nothing and the kart falls through the track.
    /// Here: when a wheel hub is below the surface, lift the body by that depth and drop the velocity component into the surface (restitution 0).
    // UNRESOLVED: the original contact response (restitution, friction, chassis shape) is not known; no constants are invented here beyond
    // the 1.5 m search height above the hub.
    fn resolve_ground_penetration(&mut self, ground: &dyn Ground) {
        let (c, a, b) = self.rb.axes();
        let mut lift = 0.0f32;
        let mut normal = Vec3::ZERO;
        for w in self.wheels.iter() {
            let l = w.attach;
            let hub = self.rb.pos + a * l.y + c * l.x + b * l.z;
            if let Some(h) = ground.ray(hub + Vec3::Y * 1.5, Vec3::new(0.0, -1.5, 0.0)) {
                let depth = h.point.y - hub.y;
                if depth > lift {
                    lift = depth;
                    normal = h.normal;
                }
            }
        }
        if lift > 0.0 {
            self.rb.pos.y += lift;
            let vn = self.rb.vel.dot(normal);
            if vn < 0.0 {
                self.rb.vel -= normal * vn;
            }
        }
    }
    /// `UpdateGroundInfo @ 0019f2b8` (normal, non-plane mode): one ray per wheel, `travel + 0.2` long (DAT_0019f660), along -up.
    // UNRESOLVED (not applicable): the original skips the ray for wheels whose tyre damage value (`car+0x19fc..`) exceeds 20.
    fn update_ground_info(&mut self, ground: &dyn Ground) {
        let (c, a, b) = self.rb.axes();
        for (i, w) in self.wheels.iter().enumerate() {
            let l = w.attach;
            let origin = self.rb.pos + a * l.y + c * l.x + b * l.z;
            let gi = &mut self.ground_info[i];
            gi.material = -1;
            gi.origin = origin;
            let dir = a * (-(w.spec.fSuspensionTravel + 0.2));
            if let Some(h) = ground.ray(origin, dir) {
                gi.hit = h.point;
                gi.normal = h.normal;
                gi.material = h.material as i32;
            }
        }
    }

    /// `CCar::Integrate @ 001abaa8` (normal mode, physics-relevant subset in the original order).
    // UNRESOLVED (not ported, game logic or data not available): slingshot-attached phase (`car+0x464`), pilot/bird
    // attachments, powerups (`IntegratePowerups`), score counters, respawn timers, tyre wear/damage accumulation
    // (`PhysMaterial_GetWearRate`), spline-height assist (`car+0xbf4`), multiplayer catch-up, AI-style per-wheel ABS brakes
    // (the non-local-player branch of DoBrakes, asm 001aca04), the plane-mode (`car+0xf4`) garage branch.
    fn car_integrate(&mut self, ground: &dyn Ground, input: &CarInput) {
        let dt = self.rb.dt;
        let n_on = self.wheels.iter().filter(|w| w.on_ground).count();
        self.on_ground_prev = n_on;
        // steering (asm 001abcf0 region)
        self.integrate_steering();
        // airborne timer (car+0x4ec)
        self.air_time = if n_on == 0 { dt + self.air_time } else { 0.0 };
        // boost pads: material ids 0x18 / 0x22 under any wheel (asm 001ae...), push along the spline / forward
        if n_on != 0 {
            let boosting = input.pad_boost || self.wheels.iter().any(|w| w.on_ground && (w.material == 0x18 || w.material == 0x22));
            if boosting {
                let dir = input.track_direction.unwrap_or_else(|| self.rb.axes().2);
                let pos = self.rb.pos;
                self.rb.apply_world_force(dir * (dt * 36000.0), pos); // DAT_001ae0dc = 36000
            }
        }
        self.do_brakes();
        for w in self.wheels.iter_mut() {
            w.load = 0.0; // suspension accumulator reset (asm 001acad4)
        }
        // anti-roll bars (asm 001acd..)
        for k in 0..self.spec.m_iNumAntiRollBars {
            let bar = self.spec.m_aAntiRollBars[k];
            let (l, r) = (bar.iLeftWheel as usize, bar.iRightWheel as usize);
            if l < self.wheels.len() && r < self.wheels.len() {
                let (ll, lr, rl, rr) = (self.wheels[l].len, self.wheels[l].rest_len, self.wheels[r].len, self.wheels[r].rest_len);
                if rl < rr && ll < lr {
                    let d = (rl - ll) * bar.fStiffness * dt;
                    self.wheels[l].load += d;
                    self.wheels[r].load -= d;
                }
            }
        }
        // UpdateGroundInfo + CWheel::Integrate x n
        self.update_ground_info(ground);
        let k = self.steer_scale;
        let (p2, p3) = (1.0 * k, 1.0 / k);
        for i in 0..self.wheels.len() {
            let gi = self.ground_info[i];
            self.wheels[i].integrate(&gi, &mut self.rb, self.speed, self.brake, p2, p3);
        }
        // speed-boost power-up (CPlayerInfo::IsPowerUpActive(2)), asm 001ad..: impulse
        // dt * Boost_Push_Factor(GetDebugFloat(0x57)) * clamp(1 - air_time, 0, 1) * mass along forward at pos + up*-0.1
        if input.boost {
            let (_, up, fwd) = self.rb.axes();
            let k = (1.0 - self.air_time).clamp(0.0, 1.0);
            let imp = dt * self.tweaks.boost_push_factor * k * self.rb.mass;
            let p = self.rb.pos + up * -0.1; // DAT_001ad0a4
            self.rb.apply_world_force(fwd * imp, p);
            // UNRESOLVED: the follow-up push block (car+0xc74/0xc78: rate from game+0x28+0x46b0, source car+0xc70, active
            // when `car+0xaf4 == 0 && wheels-on-ground != 0`) and the bodywork-piece pushes are not ported.
        }
        // speeds (asm 001ad2a0)
        let (c, a, b) = self.rb.axes();
        self.signed_speed = self.rb.vel.dot(b);
        self.abs_signed = self.signed_speed.abs();
        self.speed = self.rb.vel.length();
        // angular velocity post-processing, asm 001ad3ec..001ad7b0
        let q = self.rb.rot;
        let mut wl = q.inverse() * self.rb.ang_vel; // x = right(C), y = up(A), z = forward(B)
        if self.speed < 1.0 && self.brake > 0.1 {
            // stationary brake hold: bleed 3.5*dt of velocity and 0.6*dt of spin per axis
            let lim = dt * 3.5;
            let mut v = self.rb.vel;
            for e in 0..3 {
                v[e] += (-v[e]).clamp(-lim, lim);
            }
            self.rb.vel = v;
            let lim2 = dt * 0.6;
            for e in 0..3 {
                wl[e] += (-wl[e]).clamp(-lim2, lim2);
            }
        }
        if self.air_time > 0.1 {
            // airborne for more than 0.1 s: all angular components decay by min(3*dt, 1)
            let k = (dt * 3.0).min(1.0);
            wl -= wl * k;
        }
        self.ground_dist2 = 0.0;
        if self.on_ground_prev < 3 {
            // ray straight down from the centre of mass (CXGSEnv::RayIntersect with (0,-1e6,0))
            let hit = ground.ray(self.rb.pos, Vec3::new(0.0, -1.0e6, 0.0));
            let (kd, ground_n);
            match hit {
                None => {
                    self.ground_dist2 = 1000.0; // 0x447a0000
                    kd = 3.5;
                    ground_n = Vec3::Y;
                }
                Some(h) => {
                    let d2 = (h.point - self.rb.pos).length_squared();
                    self.ground_dist2 = d2;
                    kd = if d2 >= 0.11 {
                        if d2 > 1.1 {
                            3.5
                        } else {
                            (d2 - 0.1) * 3.5
                        }
                    } else {
                        0.035
                    };
                    ground_n = h.normal;
                }
            }
            let f = 1.0 - dt * kd;
            wl.x *= f;
            wl.z *= f;
            self.rb.ang_vel = q * wl;
            // levelling torque rotates the up axis towards the ground normal
            let d = ground_n - a;
            let l2 = d.length_squared();
            if l2 > 0.01 {
                let scale = if self.ground_dist2 < 6.25 { 15.0 } else { 7.0 };
                let mag = dt * dt * scale * self.rb.mass * 120.0; // DAT_001ad78c = 120
                let force = d * (mag / l2.sqrt());
                let pt = self.rb.pos + a;
                self.rb.apply_world_force_torque_only(force, pt);
            }
        } else {
            self.rb.ang_vel = q * wl;
        }
        let _ = c;
    }
}

// ===========================================================================================================
// tests
// ===========================================================================================================
#[cfg(test)]
mod tests {
    use super::*;

    // Kart geometry: the original kart_base helper nodes used by the game (front_left_wheel / rear_left_wheel hub
    // positions from `race.rs`), order FL, FR, RR, RL.
    fn hubs() -> Vec<Vec3> {
        vec![
            Vec3::new(0.6, 0.23, 0.56),
            Vec3::new(-0.6, 0.23, 0.56),
            Vec3::new(-0.65, 0.31, -0.5),
            Vec3::new(0.65, 0.31, -0.5),
        ]
    }

    // Per-car data that the 2.9.1 APK still contains: `kart_red_max.xml` (2.9.1, CarSpec deltas applied by Read after
    // SetDefault) ...
    const RED_MAX: &str = r#"<CarSpec m_fMinDesiredSpeed="40.0" m_fDrag="1" m_fSlidingTargetAngularVel="10" m_fBelowMinSpeedThrust="10000" m_fFragility="0"><EWheel_FL fPeakGrip="1"/><EWheel_FR fPeakGrip="1"/><EWheel_RR fPeakGrip="1"/><EWheel_RL fPeakGrip="1"/></CarSpec>"#;
    const RED_MIN: &str = r#"<CarSpec m_fMinDesiredSpeed="40.0" m_fDrag="0" m_fSlidingTargetAngularVel="0" m_fBelowMinSpeedThrust="0" m_fFragility="10"><EWheel_FL fPeakGrip="0"/><EWheel_FR fPeakGrip="0"/><EWheel_RR fPeakGrip="0"/><EWheel_RL fPeakGrip="0"/></CarSpec>"#;
    // ... on top of the base chassis (mass/inertia/suspension). UNRESOLVED: the 2.9.1 base spec is in the missing
    // carSpec.pak; this is the v1.0.1 `kart_base.xml` chassis/suspension block (mass 285, inertia 1.65 equal the 2.9.1
    // Player_Car_* tweakables).
    const BASE_CHASSIS: &str = r#"<CarSpec m_fInertia="1.650000" m_fDrag="0.2500" m_fDownForce="1.000000" m_fMass="285.000000" m_fSteeringSpeedScale="4.000000" m_fSoftSteeringScale="0.350000" m_fMaxSpeedSteerScale="50.000000" m_fSlidingTargetAngularVel="1.600000" m_iNumWheels="4" m_iNumAntiRollBars="0"><EWheel_FL bOnLeft="1" fPeakGrip="0.730000" fWheelInertia="3.000000" fBrakeTorque="2500.000000" fMaxSteeringLock="1.050000" fSuspensionStiffness="30000.000000" fSuspensionDamping="5000.000000" fSuspensionExponent="1.000000" fSuspensionTravel="0.650000" fWheelRadius="0.25000" fOptimumLoad="3400.000000" fTyreExponent="0.950000"/><EWheel_FR bOnLeft="0" fPeakGrip="0.730000" fWheelInertia="3.000000" fBrakeTorque="2500.000000" fMaxSteeringLock="1.050000" fSuspensionStiffness="30000.000000" fSuspensionDamping="5000.000000" fSuspensionExponent="1.000000" fSuspensionTravel="0.650000" fWheelRadius="0.2500" fOptimumLoad="3400.000000" fTyreExponent="0.950000"/><EWheel_RR bOnLeft="0" fPeakGrip="0.730000" fWheelInertia="3.000000" fBrakeTorque="2400.000000" fMaxSteeringLock="0.000000" fSuspensionStiffness="35000.000000" fSuspensionDamping="5200.000000" fSuspensionExponent="1.000000" fSuspensionTravel="0.650000" fWheelRadius="0.30000" fOptimumLoad="3400.000000" fTyreExponent="0.950000"/><EWheel_RL bOnLeft="1" fPeakGrip="0.730000" fWheelInertia="3.000000" fBrakeTorque="2400.000000" fMaxSteeringLock="0.000000" fSuspensionStiffness="35000.000000" fSuspensionDamping="5200.000000" fSuspensionExponent="1.000000" fSuspensionTravel="0.650000" fWheelRadius="0.30000" fOptimumLoad="3400.000000" fTyreExponent="0.950000"/></CarSpec>"#;

    /// Test helper: 2.9.1 SetDefault + v1.0.1 base chassis + 2.9.1 kart_red_max.xml deltas.
    fn red_max_spec() -> CarSpec {
        let mut s = CarSpec::set_default();
        s.read_xml(BASE_CHASSIS);
        s.read_xml(RED_MAX);
        s
    }

    /// Spawn at the rest height the suspension settles to (found by dropping from just above).
    fn settled(spec: CarSpec, ground: &FlatGround) -> CarSim {
        let mut car = CarSim::new(spec, &hubs(), Vec3::new(0.0, 0.9, 0.0), 0.0);
        let input = CarInput { brake: 1.0, ..Default::default() }; // hold the brake: no thrust, the car settles at rest
        for _ in 0..(60 * 4) {
            car.step(1.0 / 60.0, &input, ground);
        }
        car
    }

    #[test]
    /// Test: CCarSpec::SetDefault @ 001b51f0 / Read @ 001b58d4 / CopyWithMods @ 001b5700.
    fn spec_defaults_and_xml() {
        let s = CarSpec::set_default();
        assert_eq!(s.m_iNumWheels, 4);
        assert_eq!(s.wheels[0].fSuspensionStiffness, 12000.0);
        assert_eq!(s.wheels[0].fSuspensionTravel, 1.2);
        assert!((s.wheels[0].fWheelRadius - 0.4).abs() < 1e-6);
        assert!((s.m_fSlidingTargetAngularVel - 1.4).abs() < 1e-6);
        let mx = red_max_spec();
        assert_eq!(mx.m_fBelowMinSpeedThrust, 10000.0);
        assert_eq!(mx.m_fMinDesiredSpeed, 40.0);
        assert_eq!(mx.wheels[2].fWheelRadius, 0.30);
        // CopyWithMods lerp at 0.5
        let mut mn = CarSpec::set_default();
        mn.read_xml(RED_MIN);
        let half = CModSpec { grip: 0.5, fragility: 0.5, thrust: 0.5, drag: 0.5, sliding_ang_vel: 0.5, min_speed: 0.5 };
        let m = CarSpec::copy_with_mods(&mx, &mn, &mx, &half);
        assert!((m.m_fBelowMinSpeedThrust - 5000.0).abs() < 1e-3);
        assert!((m.m_fDrag - 0.5).abs() < 1e-6);
        assert!((m.wheels[0].fPeakGrip - 0.5).abs() < 1e-6);
    }

    #[test]
    /// Test: powf_fast @ 001b76f4 tracks powf.
    fn powf_fast_is_a_close_pow() {
        for &(x, e) in &[(2.0f32, 1.5f32), (10.0, 0.9), (0.5, 1.3), (123.0, 1.1)] {
            let a = powf_fast(x, e);
            let b = x.powf(e);
            assert!((a / b - 1.0).abs() < 0.01, "{x}^{e}: {a} vs {b}");
        }
    }

    #[test]
    /// Test: PrecalcTyreLoadSensitivity @ 001b7778 table and the slip curves.
    fn tyre_table_and_curves() {
        let t = precalc_tyre_load_sensitivity(0.95);
        assert!((t[0] / t[1] - 1.0).abs() < 1e-5);
        assert!((t[128] - 1.0).abs() < 1e-5); // (128/128)^e
        assert_eq!(slip_curve(0.5), 0.5);
        assert_eq!(slip_curve(3.0), 1.0);
    }

    /// The car stays upright, level and at a stable ride height on a flat plane.
    /// (Front/rear hub heights and stiffnesses differ, so the settled chassis has a small constant pitch; the test checks
    /// that it is small and does not drift.)
    #[test]
    fn upright_on_flat_plane() {
        let g = FlatGround::default();
        let mut car = settled(red_max_spec(), &g);
        let input = CarInput { brake: 1.0, ..Default::default() }; // braking: no thrust
        let y0 = car.rb.pos.y;
        let up0 = car.up();
        let mut ymin = f32::MAX;
        let mut ymax = f32::MIN;
        for _ in 0..(60 * 3) {
            car.step(1.0 / 60.0, &input, &g);
            ymin = ymin.min(car.rb.pos.y);
            ymax = ymax.max(car.rb.pos.y);
        }
        assert!(car.up().dot(Vec3::Y) > 0.99, "tilted: up = {:?}", car.up());
        assert!(car.up().dot(up0) > 0.99999, "attitude drifts: {:?} -> {:?}", up0, car.up());
        assert_eq!(car.wheels_on_ground(), 4);
        assert!((car.rb.pos.y - y0).abs() < 0.01 && ymax - ymin < 0.01, "ride height drifts {ymin}..{ymax}");
        assert!(car.rb.vel.length() < 0.2, "creeping at {:?}", car.rb.vel);
        assert!(car.rb.pos.y > 0.2 && car.rb.pos.y < 1.5, "y = {}", car.rb.pos.y);
        assert!(car.rb.ang_vel.length() < 0.05, "{:?}", car.rb.ang_vel);
    }

    /// Top speed.
    ///
    /// Derivation from the decompiled formulas (`CCar::Update @ 001a6cd8`, `CXGSRigidBody::Integrate @ 005b73e8`):
    /// * thrust impulse per frame: `dt * BelowMinSpeedThrust * clamp(MinDesiredSpeed - v, 0.01, 1)`, i.e. a force
    ///   `F = 10000 * clamp(40 - v, 0.01, 1)` while `v < 40` (kart_red_max.xml: thrust 10000, min speed 40);
    /// * drag: `a = -drag * v|v| / m` with `drag = m_fDrag = 1` (uniform drag vector, `Update` branch B).
    ///
    /// Equilibrium: `drag*v^2 = 10000*(40 - v)` -> `v^2 + 10000 v - 400000 = 0` ->
    /// `v = (-10000 + sqrt(1e8 + 1.6e6)) / 2 = 39.84 m/s` (40 - v = 0.16, inside the 0.01..1 clamp). The mass cancels.
    /// Free-rolling wheels add almost no force in steady state, so the simulated top speed must sit on this value
    /// (observed 39.79, 0.05 below: wheel-bearing/slip losses of the tyre model).
    #[test]
    fn straight_line_top_speed_matches_the_formula() {
        let g = FlatGround::default();
        let mut car = settled(red_max_spec(), &g);
        let input = CarInput::default();
        let expected = (-10000.0f32 + (1.0e8f32 + 4.0 * 400000.0).sqrt()) / 2.0;
        assert!((expected - 39.84).abs() < 0.01, "{expected}");
        for _ in 0..(60 * 30) {
            car.step(1.0 / 60.0, &input, &g);
        }
        let v = car.forward_speed();
        assert!((v - expected).abs() < 0.2, "top speed {v} vs analytic {expected}");
        assert!(car.up().dot(Vec3::Y) > 0.99);
        assert!(car.rb.vel.x.abs() < 1.0 && car.rb.vel.z > 39.0, "{:?}", car.rb.vel);
    }

    /// The pure steering formulas of `IntegrateSteering` (`CCar::IntegrateSteering [.part.137] @ 001974d4`):
    /// wheel-angle target `= input * clamp(max(beta, limit), lock)`,
    /// `limit = SteeringSpeedScale * (softness*6.667*SoftSteeringScale + 1) * (slipAngle/12) / min(speed, MaxSpeedSteerScale)`,
    /// and the arcade yaw-rate target `min(speed*0.1, 1) * Steer_Rate * input * SlidingTargetAngularVel`
    /// (default surface: softness 0, slip angle 12), blended into the body yaw rate by 0.5 per step. The wheel angle is
    /// slewed at 3.25 rad/s and the input has a 0.2 dead zone.
    #[test]
    fn steering_at_speed_uses_the_original_formula() {
        let g = FlatGround::default();
        // mid upgrade level: sliding target angular velocity lerp(0, 10, 0.1) = 1.0 rad/s per unit input
        let mx = red_max_spec();
        let mut mn = CarSpec::set_default();
        mn.read_xml(BASE_CHASSIS);
        mn.read_xml(RED_MIN);
        let spec = CarSpec::copy_with_mods(&mx, &mn, &mx, &CModSpec { grip: 1.0, fragility: 0.0, thrust: 1.0, drag: 1.0, sliding_ang_vel: 0.1, min_speed: 1.0 });
        assert!((spec.m_fSlidingTargetAngularVel - 1.0).abs() < 1e-5);
        let mut car = settled(spec, &g);
        car.rb.vel = Vec3::new(0.0, 0.0, 30.0);
        for w in car.wheels.iter_mut() {
            w.spin = 30.0;
        }
        let input = CarInput { steer: 0.3, ..Default::default() };
        // --- first step: everything is exact (no previous slip angle)
        car.step(1.0 / 60.0, &CarInput::default(), &g); // let CCar+0x1ab4 (speed) pick up the new velocity
        let sp = car.speed();
        car.step(1.0 / 60.0, &input, &g);
        let limit = (4.0f32 * 1.0 * (12.0 / 12.0)) / sp.min(50.0); // SteeringSpeedScale / min(speed, MaxSpeedSteerScale)
        assert!((car.steer_target - 0.3 * limit).abs() < 2e-3, "wheel target {} vs {}", car.steer_target, 0.3 * limit);
        assert!((car.steer_angle() - (1.0f32 / 60.0 * 3.25).min(0.3 * limit)).abs() < 1e-3);
        let yaw_target = 1.0f32 * 1.15 * 0.3 * 1.0; // ramp(30 m/s -> 1) * Steer_Rate * input * SlidingTargetAngularVel
        assert!((car.yaw_target - yaw_target).abs() < 1e-3, "{} vs {}", car.yaw_target, yaw_target);
        // blend of 0.5 from zero in the first step (tyre torque on top, small): w' ~ 0.5 * target
        assert!(car.rb.ang_vel.y > 0.0 && (car.rb.ang_vel.y / (0.5 * yaw_target) - 1.0).abs() < 0.3, "{}", car.rb.ang_vel.y);
        // --- after a second: the body turns at about the target rate (positive steer = towards +x = positive yaw)
        for _ in 0..60 {
            car.step(1.0 / 60.0, &input, &g);
        }
        let yaw = car.rb.ang_vel.y;
        assert!((yaw / yaw_target - 1.0).abs() < 0.3, "yaw rate {yaw} vs target {yaw_target}");
        // no input -> no yaw target, rate decays
        let none = CarInput::default();
        for _ in 0..60 {
            car.step(1.0 / 60.0, &none, &g);
        }
        assert!(car.rb.ang_vel.y.abs() < 0.05, "{}", car.rb.ang_vel.y);
        // dead zone: 0.15 input -> zero target
        car.step(1.0 / 60.0, &CarInput { steer: 0.15, ..Default::default() }, &g);
        assert_eq!(car.steer_target, 0.0);
        // low-speed ramp: at 5 m/s the yaw target is halved (speed*0.1)
        let mut slow = settled(car_spec_for_ramp(), &g);
        slow.rb.vel = Vec3::new(0.0, 0.0, 5.0);
        slow.step(1.0 / 60.0, &CarInput::default(), &g);
        slow.step(1.0 / 60.0, &input, &g);
        assert!((slow.yaw_target - 0.5 * 1.15 * 0.3 * 1.0).abs() < 0.01, "{}", slow.yaw_target);
    }

    /// Test helper: red max spec with a 1 rad/s sliding target angular velocity and no thrust.
    fn car_spec_for_ramp() -> CarSpec {
        let mut s = red_max_spec();
        s.m_fSlidingTargetAngularVel = 1.0;
        s.m_fMinDesiredSpeed = 0.0; // no thrust, so the 5 m/s state stays put for the single step
        s
    }

    /// Test: CalcRestingHeight @ 0019ea7c / CalcRestingWheelPosition @ 0019e948 against the hand formula, and that the
    /// car spawned there stays put (the settled height is a little lower because the game's gravity is 2 x 9.8 while
    /// the formula uses 9.8, so the compression is doubled).
    #[test]
    fn resting_height_formula() {
        let g = FlatGround::default();
        let spec = red_max_spec();
        let h = calc_resting_height(&hubs(), &spec);
        let expect = 0.65 - (285.0f32 * 9.8) / 130000.0 - (0.23 + 0.23 + 0.31 + 0.31) / 4.0;
        assert!((h - expect).abs() < 1e-5, "{h} vs {expect}");
        let mut car = CarSim::new_on_ground(spec, &hubs(), 0.0, 0.0, 0.0, 0.0);
        let input = CarInput { brake: 1.0, ..Default::default() };
        for _ in 0..(60 * 3) {
            car.step(1.0 / 60.0, &input, &g);
        }
        assert!((car.rb.pos.y - h).abs() < 0.05, "settled {} vs spawn {}", car.rb.pos.y, h);
        assert_eq!(car.get_num_wheels_on_ground(), 4);
    }

    /// Test: the speed-boost impulse of CCar::Integrate: `dt * Boost_Push_Factor * mass` along forward, i.e. +4 m/s^2.
    #[test]
    fn boost_push_matches_the_formula() {
        let g = FlatGround::default();
        let mut spec = red_max_spec();
        spec.m_fBelowMinSpeedThrust = 0.0; // isolate the boost
        // level chassis (equal front/rear hub heights, the stock hub layout pitches the car 4 degrees and its spring force then
        // has a horizontal component) so the push can be read directly
        let hubs = vec![Vec3::new(0.6, 0.27, 0.56), Vec3::new(-0.6, 0.27, 0.56), Vec3::new(-0.65, 0.27, -0.5), Vec3::new(0.65, 0.27, -0.5)];
        let mut car = CarSim::new(spec, &hubs, Vec3::new(0.0, 0.6, 0.0), 0.0);
        let brake = CarInput { brake: 1.0, ..Default::default() };
        for _ in 0..240 {
            car.step(1.0 / 60.0, &brake, &g);
        }
        let v0 = car.rb.vel.z;
        let input = CarInput { boost: true, ..Default::default() };
        for _ in 0..60 {
            car.step(1.0 / 60.0, &input, &g);
        }
        let dv = car.rb.vel.z - v0;
        // 60 steps * (1/60 * 4 * m)/m = 4.0 m/s (drag a = v^2/285 and the tyres change it by a few percent)
        assert!((dv - 4.0).abs() < 0.4, "boost delta v {dv}");
    }

    /// Test: SetSteerAngle @ 0019b204 / SetSteerAngle_Internal @ 0019b268 (direct angle, clamp, toe) and ABS brakes.
    #[test]
    fn direct_steer_angle_and_abs_path() {
        let g = FlatGround::default();
        let mut car = settled(red_max_spec(), &g);
        car.spec.m_fToeIn = 0.02;
        car.step(1.0 / 60.0, &CarInput { steer_angle: Some(0.3), brake: 1.0, ..Default::default() }, &g);
        assert!((car.steer_angle() - 0.3).abs() < 1e-6);
        assert!((car.wheels[0].steer - 0.32).abs() < 1e-6 && (car.wheels[1].steer - 0.28).abs() < 1e-6);
        car.set_steer_angle(9.0);
        assert!((car.steer_angle() - 1.570_796_4).abs() < 1e-6);
        // AI/ABS branch runs and keeps torque finite
        car.local_player = false;
        car.rb.vel = Vec3::new(0.0, 0.0, 10.0);
        for _ in 0..30 {
            car.step(1.0 / 60.0, &CarInput { brake: 1.0, ..Default::default() }, &g);
        }
        assert!(car.wheels.iter().all(|w| w.brake_torque.is_finite() && w.brake_torque >= 0.0));
        assert!(car.forward_speed() < 10.0);
    }

    /// Test: slingshot launch formula (CCar::GetLaunchVelocity @ 00199c04).
    #[test]
    fn slingshot_launch_matches_the_decompiled_formula() {
        // pull of 4.2 along +z, AI path (ratio = 4.2/8.4 = 0.5), force scale 1.0:
        // dir = -z, speed = 30 + 20*0.5 = 40, lift = (1-0.5)*7 = 3.5 -> |v| = 43.5 along -z
        let p = LaunchParams::default();
        let info = slingshot_launch(Vec3::new(0.0, 0.0, 4.2), &p);
        assert!((info.velocity.z + 43.5).abs() < 1e-4, "{:?}", info.velocity);
        assert!(info.velocity.x.abs() < 1e-5 && info.velocity.y.abs() < 1e-5);
        assert!((info.speed - 40.0).abs() < 1e-4 && (info.lift - 3.5).abs() < 1e-5);
        // full pull (>= 8.4): speed 50, no lift
        let info = slingshot_launch(Vec3::new(0.0, 0.0, 9.0), &p);
        assert!((info.velocity.z + 50.0).abs() < 1e-4, "{:?}", info.velocity);
        // game mode 10 uses 22..52
        let q = LaunchParams { game_mode_10: true, ..p };
        let info = slingshot_launch(Vec3::new(0.0, 0.0, 9.0), &q);
        assert!((info.velocity.z + 52.0).abs() < 1e-4);
    }
}
