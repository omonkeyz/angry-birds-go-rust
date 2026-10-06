//! `CCamera` (race chase camera): port of the rear / race camera (camera type 0), the slingshot camera (type 9), the slingshot -> rear
//! hand-over, the smoothing, the impact shake and the track collision, from the 2.9.1 decompile (`libABK291_annotated.c`).
//!
//! Ported (Ghidra addresses are file addresses of the annotated decompile):
//! * `CCamera::SetRearCam(CCamSettings&, ...) [clone .part.31] @ 000df91c`: the camera and look-at offsets in the car frame
//!   (position `(0, (1 - CamHeightMod) * Height, (-4 + 2 * COMz - CamBehindMod) * Behind) + (0, 0.8, 0)`, look-at `(0, 1.8, 2.5)`),
//!   the lateral slide offset `Lateral_Move_With_Slide`, the blend of the car's forward axis towards the velocity direction
//!   `Blend_To_Move_Dir`, the re-derivation of the right axis `fwd x up`, the field of view `0.84823 * FOV * CamZoomMod`.
//! * `CCar::GetCamHeightMod @ 001a3fa4`, `GetCamZoomMod @ 001a4070`, `GetCamBehindMod @ 001a4210` (the ability hook at `car+0x1c2c` and
//!   the acceleration term `car+0x1b78`, which is only ever written with 0 in the decompile, are inputs of [`CamCar`]).
//! * `CCamera::DoSmoothing @ 000e3b70` case 0: `x' = (new + k * old) / (1 + k)` with `k = Smooth_* / 60 / dt` for position, look-at and
//!   up vector (up re-normalised), field of view `old + (new - old) * 3 dt`.
//! * `CCamera::Process @ 000e46d4` mode 0 / 9 tail: the upside-down correction (`UpsideDownCorrection @ 000e60e0`, inlined), the
//!   minimum camera distance (|offset| < 8 is scaled to 8), the up-vector nudge when looking along it, the car position added, the
//!   look-at shake offset, then the inlined `CCamera::DoCollisionCheck @ 000e3fcc` (ray from the look-at point to the camera, sphere
//!   test of radius 0.75, camera raised by a quarter of the pulled-in distance) against the track (`Ground::cam_ray / cam_sphere`
//!   with `_FilterCameraCollision @ 000df7f4`).
//! * `CCamera::ApplyCameraShake @ 000e1740` (the impact part: `ImpactShake * (0.006, 0.008) * sin(t)` in the car frame, added to the look-at).
//! * `CCamera::SetSlingshotCam @ 000e3050` (frame, distance `-10 - 7.4 * pull`, look-at 10 ahead, up) and the slingshot -> rear
//!   hand-over of `Process` (`this+0x170`: smoothstep blend of the stored slingshot camera into the rear camera from 0.25 s over 0.9 s,
//!   the camera lifted by `5 * t`).
//! * Not ported, on purpose: `CCamera::ApplyDriftRoll @ 000e16e4` and `GetCarHPR @ 000e1f60` (no caller in the decompile, the roll
//!   accumulator `this+0xd0` is only ever reset to 0), the pilot / bird camera (`car+0x1bf0`), camera types 1, 2, 3, 4-8, 10-12.
//!
//! UNRESOLVED (see the report in `PORT_STATUS.md` Update 10): the intro camera (types 3 / 2: `SetIntroCam @ 000e2844`, `UpdateIntroCam`
//! need the spline-cam data), the touch-driven slingshot yaw / pitch (the arguments of `SetSlingshotCamYaw / Pitch` are lost in the
//! decompile; zero here, so the slingshot camera looks straight along the launch direction), the angle easing chain of
//! `SetSlingshotCam`, the shake time base and the phase of its second `sinf`, `CCamera::Shake(a, b, c)` (`this+0x90..0x9c`), the
//! `CCar::GetSpline` up vector (an input, `None` = no upside-down correction), the vertical-vs-horizontal meaning of the field of
//! view (it is used as the full vertical angle: the frontend default `0x3f59259b` = 0.848 rad = 48.6 deg).
use crate::carsim::Ground;
use glam::{Quat, Vec3};

/// `CCamera+0x34 / +0xcc` default and `CCamSettings::Reset` field of view (`0x3f59259b`).
pub const FOV_DEFAULT: f32 = 0.84823006;
/// `DAT_000e3fa8`: the smoothing time unit (1/60 s).
const SMOOTH_UNIT: f32 = 0.016666668;
/// `DAT_000e3fb0 * DAT_000e3fb4` = 0.05 * 60: field of view follow rate.
const FOV_FOLLOW: f32 = 0.05 * 60.0;
/// `DAT_000e5240` (64) / the scale target 8.0: the camera is never closer than 8 m to the car target position.
const MIN_DIST: f32 = 8.0;
/// `DAT_000e5244` (0.92): the up vector is nudged when the view direction is this parallel to it.
const UP_PARALLEL: f32 = 0.92;
/// `DAT_000e5a44`: the nudge factor.
const UP_NUDGE: f32 = 0.015;
/// `DAT_000dff30`: the right axis is re-derived while `up . fwd` stays below this.
const RIGHT_REDERIVE: f32 = 0.99;
/// `DAT_000dff38`: the blended forward axis is used when its squared length exceeds this.
const FWD_EPS: f32 = 0.01;
/// `DAT_000e5a50`: the pulled-in camera point stays this far in front of the wall.
const COLLISION_INSET: f32 = 0.1;
/// `DAT_000e5a4c`: the pull-in is only used for hits further than this from the camera.
const COLLISION_MIN_PULL: f32 = 0.0001;
/// The sphere test radius / push (`0x3f400000`, `* 0.75`).
const COLLISION_RADIUS: f32 = 0.75;
/// Impact shake amplitudes `DAT_000e1af4`, `DAT_000e1af8`.
const SHAKE_X: f32 = 0.006;
const SHAKE_Y: f32 = 0.008;
/// Slingshot camera: `DAT_000e34a4` (extra distance per unit of pull) and the base distance (-10) / default (`DAT_000e34a8`).
const SLING_PULL_DIST: f32 = -7.4;
const SLING_DIST: f32 = -10.0;
const SLING_DIST_NO_PLAYER: f32 = -17.4;
/// `DAT_000e2fdc/2fe0`, `DAT_000e3010/3014`: the slingshot camera pitch / yaw clamps (25 and 30 degrees).
pub const SLING_PITCH_LIMIT: f32 = 0.43633232;
pub const SLING_YAW_LIMIT: f32 = 0.5235988;
/// Slingshot -> rear hand-over: delay (0.25 s), `1 / 0.9`, lift (5 m) as in `Process` (`DAT_000e49d4 = 1.1111112`).
const HANDOVER_DELAY: f32 = 0.25;
const HANDOVER_RATE: f32 = 1.1111112;
const HANDOVER_LIFT: f32 = 5.0;

/// The `<Camera>` block of `debugtweakables.xml` (`CDebugManager::GetDebugFloat(5..0x19, 0x2a..0x2c)` / `GetDebugBool`).
/// Index -> key: 5 `FOV`, 6 `Height`, 7 `Behind`, 8..0xc `Height_*`, 0xd..0x10 `Zoom_*`, 0x11..0x13 `Behind_Accel_*`, 0x14..0x17
/// `Behind_Min_Speed` / `Behind_Speed_*`, 0x18 `Blend_To_Move_Dir`, 0x19 `Lateral_Move_With_Slide`, 0x2a/b/c `Smooth_Cam_Pos/Look_Pos/Cam_Up`
/// (the XML order of the block; 5..0x19 verified against how `SetRearCam` and `DoSmoothing` use them).
#[derive(Clone, Debug, PartialEq)]
pub struct CamTweaks {
    /// `Dynamic_FOV` (`GetDebugBool(0x14)`).
    pub dynamic_fov: bool,
    /// `Engine_Shake` (`GetDebugBool(0x15)`): the engine part of the shake; false in the shipped file.
    pub engine_shake: bool,
    pub fov: f32,
    pub height: f32,
    pub behind: f32,
    pub height_min_dist: f32,
    pub height_max_dist: f32,
    pub height_min_speed: f32,
    pub height_speed_scale: f32,
    pub height_lag_scale: f32,
    pub zoom_min: f32,
    pub zoom_max: f32,
    pub zoom_min_speed: f32,
    pub zoom_speed_scale: f32,
    pub behind_accel_min: f32,
    pub behind_accel_max: f32,
    pub behind_accel_scale: f32,
    pub behind_min_speed: f32,
    pub behind_speed_min: f32,
    pub behind_speed_max: f32,
    pub behind_speed_scale: f32,
    pub blend_to_move_dir: f32,
    pub lateral_slide: f32,
    pub smooth_pos: f32,
    pub smooth_look: f32,
    pub smooth_up: f32,
}

impl Default for CamTweaks {
    /// The values of the shipped `assets292/xml_gameplay/misc/debugtweakables.xml` `<Camera>` block.
    fn default() -> Self {
        CamTweaks {
            dynamic_fov: true,
            engine_shake: false,
            fov: 0.75,
            height: 1.5,
            behind: 1.3,
            height_min_dist: 0.0,
            height_max_dist: 0.4,
            height_min_speed: 10.0,
            height_speed_scale: 0.005,
            height_lag_scale: 0.1,
            zoom_min: 1.0,
            zoom_max: 1.31,
            zoom_min_speed: 28.5,
            zoom_speed_scale: 0.05,
            behind_accel_min: -1.0,
            behind_accel_max: 2.0,
            behind_accel_scale: 3.5,
            behind_min_speed: 10.0,
            behind_speed_min: -1.0,
            behind_speed_max: 2.0,
            behind_speed_scale: 0.0,
            blend_to_move_dir: 0.25,
            lateral_slide: 2.0,
            smooth_pos: 15.0,
            smooth_look: 8.0,
            smooth_up: 15.0,
        }
    }
}

impl CamTweaks {
    /// Reads the `<Camera>` block; missing keys keep the shipped values.
    pub fn from_xml(xml: &str) -> CamTweaks {
        let mut t = CamTweaks::default();
        let Ok(doc) = roxmltree::Document::parse(xml) else { return t };
        let Some(sec) = doc.descendants().find(|n| n.is_element() && n.has_tag_name("Camera")) else { return t };
        let get = |name: &str| sec.children().find(|n| n.is_element() && n.has_tag_name(name)).and_then(|n| n.text()).map(|s| s.trim().to_string());
        let f = |name: &str, out: &mut f32| {
            if let Some(v) = get(name).and_then(|s| s.trim_end_matches('f').parse::<f32>().ok()) {
                *out = v;
            }
        };
        let b = |name: &str, out: &mut bool| {
            if let Some(v) = get(name) {
                *out = v.eq_ignore_ascii_case("true") || v == "1";
            }
        };
        b("Dynamic_FOV", &mut t.dynamic_fov);
        b("Engine_Shake", &mut t.engine_shake);
        f("FOV", &mut t.fov);
        f("Height", &mut t.height);
        f("Behind", &mut t.behind);
        f("Height_Min_Dist", &mut t.height_min_dist);
        f("Height_Max_Dist", &mut t.height_max_dist);
        f("Height_Min_Speed", &mut t.height_min_speed);
        f("Height_Speed_Scale", &mut t.height_speed_scale);
        f("Height_Lag_Scale", &mut t.height_lag_scale);
        f("Zoom_Min_Zoom", &mut t.zoom_min);
        f("Zoom_Max_Zoom", &mut t.zoom_max);
        f("Zoom_Min_Speed", &mut t.zoom_min_speed);
        f("Zoom_Speed_Scale", &mut t.zoom_speed_scale);
        f("Behind_Accel_Min_Dist", &mut t.behind_accel_min);
        f("Behind_Accel_Max_Dist", &mut t.behind_accel_max);
        f("Behind_Accel_Scale", &mut t.behind_accel_scale);
        f("Behind_Min_Speed", &mut t.behind_min_speed);
        f("Behind_Speed_Min_Dist", &mut t.behind_speed_min);
        f("Behind_Speed_Max_Dist", &mut t.behind_speed_max);
        f("Behind_Speed_Scale", &mut t.behind_speed_scale);
        f("Blend_To_Move_Dir", &mut t.blend_to_move_dir);
        f("Lateral_Move_With_Slide", &mut t.lateral_slide);
        f("Smooth_Cam_Pos", &mut t.smooth_pos);
        f("Smooth_Look_Pos", &mut t.smooth_look);
        f("Smooth_Cam_Up", &mut t.smooth_up);
        t
    }

    /// Loads `xml_gameplay/misc/debugtweakables.xml` (or `xml/gameplay/misc/`) below the assets292 folder.
    pub fn load(root: &std::path::Path) -> CamTweaks {
        for rel in ["xml_gameplay/misc/debugtweakables.xml", "xml/gameplay/misc/debugtweakables.xml"] {
            if let Ok(s) = std::fs::read_to_string(root.join(rel)) {
                return CamTweaks::from_xml(&s);
            }
        }
        CamTweaks::default()
    }
}

/// The camera type being processed (`CCamera+0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamMode {
    /// Type 0: the rear / race camera.
    Rear,
    /// Type 9: the slingshot camera.
    Slingshot,
}

/// What the camera reads from the car each frame.
#[derive(Clone, Debug)]
pub struct CamCar {
    /// `GetCamTargetPosition` (rigid body `+0x38`).
    pub pos: Vec3,
    /// `GetCamTargetVelocity` (rigid body `+0x10`; zero while the car is in the slingshot).
    pub vel: Vec3,
    /// Rigid body orientation (`+0x44`).
    pub rot: Quat,
    /// `CCar+0x1ab4` = |v|.
    pub speed: f32,
    /// `*(rb+0xa4)` = `spec.m_vCOMOffset.z` (`SetCOMOffset`).
    pub com_z: f32,
    /// `CCar::GetNumWheelsOnGround`.
    pub wheels_on_ground: usize,
    /// `CCar+0x1b78` (acceleration term of `GetCamBehindMod`; the decompile only ever stores 0).
    pub accel: f32,
    /// Speed-boost branch of `GetCamBehindMod` (`powerups::boost_cam_behind_mod`) while the SpeedBooster power-up is active.
    pub behind_boost: Option<f32>,
    /// `CCar::GetImpactCamShakeMod` (already scaled).
    pub impact_shake: f32,
    /// `CSpline::GetUpVectorInterpolated` at the car (`CCar::GetSpline` is null -> `None`: no upside-down correction).
    pub track_up: Option<Vec3>,
    /// `CPlayer+0x22c`: the slingshot pull ratio 0..1.
    pub sling_pull: f32,
    /// `CCar::GetSlingshotMatrix` rows (right, up, back).
    pub sling_frame: Option<[Vec3; 3]>,
}

impl CamCar {
    pub fn simple(pos: Vec3, rot: Quat) -> CamCar {
        CamCar { pos, vel: Vec3::ZERO, rot, speed: 0.0, com_z: 0.0, wheels_on_ground: 4, accel: 0.0, behind_boost: None, impact_shake: 0.0, track_up: None, sling_pull: 0.0, sling_frame: None }
    }
}

/// `CCamSettings`: camera, look-at and up vector relative to the car target position (world axes), plus the field of view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    pos: Vec3,
    tgt: Vec3,
    up: Vec3,
    fov: f32,
}

/// The stored slingshot camera (`game+0x708 / 0x720 / 0x1ce`): world positions and field of view.
#[derive(Clone, Copy, Debug)]
struct SlingStore {
    pos: Vec3,
    tgt: Vec3,
    fov: f32,
}

#[derive(Clone, Debug)]
pub struct ChaseCam {
    pub tw: CamTweaks,
    mode: CamMode,
    /// `this+4..0x34`: last frame's settings (the smoothing state).
    prev: Settings,
    /// `this+0x88`: skip the smoothing this frame.
    snap: bool,
    /// `this+0xf4`: seconds since the camera type changed.
    mode_time: f32,
    /// `this+0xf8`: the upside-down blend timer.
    flip_timer: f32,
    /// `this+0x170` + the game arrays: the slingshot camera to hand over from.
    handover: Option<SlingStore>,
    sling_store: Option<SlingStore>,
    /// `this+0x110 / 0x114 / 0x118`: slingshot camera pitch / yaw / roll targets.
    sling_angles: [f32; 3],
    /// Impact shake time base (ms of simulated time).
    time_ms: f64,
    shake: Vec3,
    /// Output: `this+0x6c` camera position, `this+0x78` look-at, `this+0x1c` up, field of view (`this+0x34`).
    pub eye: Vec3,
    pub look: Vec3,
    pub up: Vec3,
    pub fov: f32,
    started: bool,
}

fn axes_of(q: Quat) -> (Vec3, Vec3, Vec3) {
    (q * Vec3::X, q * Vec3::Y, q * Vec3::Z)
}

fn clamp_min_max(x: f32, lo: f32, hi: f32) -> f32 {
    // `if (lo <= x && (v = x, hi < x)) v = hi` with `v = lo` first: the engine's clamp, kept for NaN behaviour parity
    if lo <= x {
        if hi < x {
            hi
        } else {
            x
        }
    } else {
        lo
    }
}

impl ChaseCam {
    pub fn new(tw: CamTweaks) -> ChaseCam {
        ChaseCam {
            tw,
            mode: CamMode::Rear,
            prev: Settings { pos: Vec3::ZERO, tgt: Vec3::ZERO, up: Vec3::Y, fov: FOV_DEFAULT },
            snap: true,
            mode_time: 0.0,
            flip_timer: 0.0,
            handover: None,
            sling_store: None,
            sling_angles: [0.0; 3],
            time_ms: 0.0,
            shake: Vec3::ZERO,
            eye: Vec3::ZERO,
            look: Vec3::Z,
            up: Vec3::Y,
            fov: FOV_DEFAULT,
            started: false,
        }
    }

    pub fn mode(&self) -> CamMode {
        self.mode
    }

    /// `CCamera::SetCameraType(type, 1, 0, 1)`: only a change of type resets the timer; leaving the slingshot camera arms the hand-over.
    pub fn set_mode(&mut self, mode: CamMode) {
        if self.mode == mode {
            return;
        }
        if self.mode == CamMode::Slingshot && mode == CamMode::Rear {
            self.handover = self.sling_store;
        }
        self.mode = mode;
        self.mode_time = 0.0;
        self.snap = false;
        self.flip_timer = 0.0;
    }

    /// `this+0xd4`: a camera cut (respawn / teleport): the next frame is not smoothed.
    pub fn cut(&mut self) {
        self.snap = true;
        self.handover = None;
    }

    /// `CCamera::SetSlingshotCamPitch / Yaw` (clamped as the original: +-25 / +-30 degrees).
    pub fn set_slingshot_angles(&mut self, pitch: f32, yaw: f32) {
        self.sling_angles[0] = pitch.clamp(-SLING_PITCH_LIMIT, SLING_PITCH_LIMIT);
        self.sling_angles[1] = yaw.clamp(-SLING_YAW_LIMIT, SLING_YAW_LIMIT);
    }

    // ------------------------------------------------------------------------------------------------ car modifiers

    /// `CCar::GetCamHeightMod @ 001a3fa4`.
    pub fn height_mod(&self, car: &CamCar) -> f32 {
        let t = &self.tw;
        let v = t.height_speed_scale * (car.speed - t.height_min_speed);
        let up = car.rot * Vec3::Y;
        clamp_min_max(v, t.height_min_dist, t.height_max_dist) + t.height_lag_scale * car.vel.dot(up)
    }

    /// `CCar::GetCamZoomMod @ 001a4070` (without the ability hook).
    pub fn zoom_mod(&self, car: &CamCar) -> f32 {
        let t = &self.tw;
        clamp_min_max(t.zoom_min + t.zoom_speed_scale * (car.speed - t.zoom_min_speed), t.zoom_min, t.zoom_max)
    }

    /// `CCar::GetCamBehindMod @ 001a4210` (without the ability hook).
    pub fn behind_mod(&self, car: &CamCar) -> f32 {
        let t = &self.tw;
        let accel = clamp_min_max(car.accel, t.behind_accel_min, t.behind_accel_max);
        let second = match car.behind_boost {
            Some(b) => b,
            None => clamp_min_max(t.behind_speed_scale * (car.speed - t.behind_min_speed), t.behind_speed_min, t.behind_speed_max),
        };
        t.behind_accel_scale * accel + second
    }

    // ------------------------------------------------------------------------------------------------ settings

    /// `CCamera::SetRearCam` (type 0): the settings in world axes relative to the car position.
    fn rear_settings(&self, car: &CamCar) -> Settings {
        let t = &self.tw;
        let mut pos = Vec3::new(0.0, (1.0 - self.height_mod(car)) * t.height, 0.0);
        pos.z = (-4.0 + car.com_z + car.com_z - self.behind_mod(car)) * t.behind;
        let mut tgt = Vec3::new(0.0, 1.8, 2.5);
        let mut fov = FOV_DEFAULT * t.fov;
        if t.dynamic_fov {
            fov *= self.zoom_mod(car);
        }
        pos.y += 0.8;
        // the car frame: rows right, up, forward of the rigid body matrix
        let (mut right, up, mut fwd) = axes_of(car.rot);
        let v2 = car.vel.length_squared();
        if v2 > 1.0 {
            let vd = car.vel / v2.sqrt();
            // `car+0x4ac <= 0` (no spin-out penalty) and the pilot is not detached: not modelled, always true
            let s = right.dot(vd) * t.lateral_slide;
            pos.x += s;
            tgt.x += s;
            let f2 = fwd + (vd - fwd) * t.blend_to_move_dir;
            fwd = if f2.length_squared() > FWD_EPS { f2 / f2.length() } else { vd };
        }
        if up.dot(fwd) < RIGHT_REDERIVE {
            right = fwd.cross(up);
        }
        let to_world = |v: Vec3| right * v.x + up * v.y + fwd * v.z;
        Settings { pos: to_world(pos), tgt: to_world(tgt), up: to_world(Vec3::Y), fov }
    }

    /// `CCamera::SetSlingshotCam @ 000e3050`.
    fn slingshot_settings(&self, car: &CamCar) -> Settings {
        let [r, u, b] = car.sling_frame.unwrap_or_else(|| {
            let (x, y, z) = axes_of(car.rot);
            [x, y, -z]
        });
        let (r, u, b) = (r.normalize_or_zero(), u.normalize_or_zero(), b.normalize_or_zero());
        let [a0, a1, a2] = self.sling_angles;
        let (s0, s1, s2) = (a0.sin(), a1.sin(), a2.sin());
        let (c0, c1, c2) = (a0.cos(), a1.cos(), a2.cos());
        let uu = (s0, s1 * c0, c1 * c0);
        let ww = (c2 * c0, s2 * c1 - s1 * s0 * c2, -(s1 * s2 + c1 * s0 * c2));
        let comb = |v: (f32, f32, f32)| u * v.0 - r * v.1 - b * v.2;
        let dir = comb(uu);
        let up = comb(ww);
        let dist = SLING_DIST + car.sling_pull * SLING_PULL_DIST;
        // game mode 10 uses 0.75 x the field of view; no other mode is modelled
        Settings { pos: up + dir * dist, tgt: dir * 10.0, up, fov: FOV_DEFAULT * self.tw.fov }
    }

    // ------------------------------------------------------------------------------------------------ per frame

    /// `CCamera::Process(dt)` for camera types 0 and 9. `ground` is the track (collision), `None` = no collision.
    pub fn update(&mut self, dt: f32, car: &CamCar, ground: Option<&dyn Ground>) {
        let dt = dt.max(0.0);
        self.mode_time += dt;
        self.time_ms += dt as f64 * 1000.0;
        let mut s = match self.mode {
            CamMode::Rear => self.rear_settings(car),
            CamMode::Slingshot => self.slingshot_settings(car),
        };
        if self.mode == CamMode::Slingshot {
            // `Process` case 9 stores the world positions every frame for the hand-over
            self.sling_store = Some(SlingStore { pos: s.pos + car.pos, tgt: s.tgt + car.pos, fov: s.fov });
        }
        let mut no_smooth = false;
        if self.mode == CamMode::Rear {
            if let Some(h) = self.handover {
                // the `this+0x170` hand-over: a smoothstep from the stored slingshot camera to the rear camera
                let x = (self.mode_time - HANDOVER_DELAY) * HANDOVER_RATE;
                if x < 0.0 || x < 1.0 {
                    let x = x.max(0.0);
                    let w = x * x * (3.0 - 2.0 * x);
                    let lift = Vec3::new(0.0, x * HANDOVER_LIFT, 0.0);
                    s.pos = s.pos * w + (h.pos - car.pos + lift) * (1.0 - w);
                    s.tgt = s.tgt * w + (h.tgt - car.pos) * (1.0 - w);
                    s.fov = s.fov * w + h.fov * (1.0 - w);
                    no_smooth = true;
                } else {
                    self.handover = None;
                }
            }
        }
        // `DoSmoothing` (case 0 only: the slingshot camera is not smoothed) unless a cut / hand-over frame
        if self.mode == CamMode::Rear && !self.snap && !no_smooth && self.started {
            let t = &self.tw;
            let k = |rate: f32| rate * SMOOTH_UNIT / dt.max(1.0e-5);
            let (kp, kl, ku) = (k(t.smooth_pos), k(t.smooth_look), k(t.smooth_up));
            let old = self.prev;
            s.pos = (s.pos + old.pos * kp) / (1.0 + kp);
            s.tgt = (s.tgt + old.tgt * kl) / (1.0 + kl);
            s.up = ((s.up + old.up * ku) / (1.0 + ku)).normalize_or_zero();
            s.fov = old.fov + (s.fov - old.fov) * FOV_FOLLOW * dt;
        }
        self.snap = false;
        // impact shake (`ApplyCameraShake`, the `CCar::GetImpactCamShakeMod` part), added to the look-at point in the car frame
        self.shake = Vec3::ZERO;
        if self.mode == CamMode::Rear && car.impact_shake > 0.0 {
            let ph = (self.time_ms as f32).sin();
            let local = Vec3::new(car.impact_shake * SHAKE_X * ph, car.impact_shake * SHAKE_Y * ph, 0.0);
            self.shake = car.rot * local;
        }
        self.prev = s;
        self.started = true;

        // --- world composition (`Process` after the settings are stored)
        let mut pos = s.pos;
        // `UpsideDownCorrection` (inlined): lift the camera along the track up vector while the car is upside down
        if let Some(tu) = car.track_up {
            let tu = tu.normalize_or_zero();
            let body_up = car.rot * Vec3::Y;
            let d = tu.dot(body_up);
            let grounded = car.wheels_on_ground > 0;
            if (!grounded && d <= 0.1) || self.flip_timer > 0.0 {
                let step = if grounded { -dt } else { dt };
                let t = self.flip_timer + step;
                let (w, timer) = if t < 0.0 {
                    (0.0, 0.0)
                } else if t <= 0.5 {
                    (t + t, t)
                } else {
                    (1.0, 0.5)
                };
                self.flip_timer = timer;
                let x = (d + 0.5) / 0.6;
                let sc = if x < 0.0 { 0.2 } else if x > 1.0 { 1.0 } else { x + (1.0 - x) * 0.2 };
                let lift = sc - pos.dot(tu);
                if lift > 0.0 {
                    pos = (pos + tu * lift) * w + pos * (1.0 - w);
                }
            }
        }
        // minimum distance
        let l2 = pos.length_squared();
        if l2 < MIN_DIST * MIN_DIST && l2 > 0.0 {
            pos *= MIN_DIST / l2.sqrt();
        }
        // up-vector nudge when the view direction is (nearly) the up vector
        let dir = (s.tgt - pos).normalize_or_zero();
        if self.prev.up.dot(dir).abs() > UP_PARALLEL {
            let body_up = car.rot * Vec3::Y;
            self.prev.up = (self.prev.up + body_up * UP_NUDGE).normalize_or_zero();
        }
        self.up = self.prev.up;
        self.fov = self.prev.fov;
        self.eye = pos + car.pos;
        self.look = s.tgt + car.pos + self.shake;

        // `DoCollisionCheck` (inlined for type 0)
        if self.mode == CamMode::Rear {
            if let Some(g) = ground {
                self.collide(g, car.pos);
            }
        }
    }

    /// `CCamera::DoCollisionCheck @ 000e3fcc` / the inlined copy in `Process`.
    fn collide(&mut self, g: &dyn Ground, car_pos: Vec3) {
        let mut probe = self.eye;
        let mut raise = 0.0;
        if let Some((hit, _)) = g.cam_ray(self.look, self.eye - self.look) {
            let to_wall = hit - self.eye;
            let d = to_wall.length();
            // type 0: only walls nearer than the car itself pull the camera in
            let limit = (self.eye - car_pos).length();
            if COLLISION_MIN_PULL < d && d < limit {
                probe = hit + to_wall / d * COLLISION_INSET;
                raise = d * 0.25;
            }
        }
        match g.cam_sphere(probe, COLLISION_RADIUS) {
            Some((contact, n)) => {
                self.eye = contact + n * COLLISION_RADIUS;
                self.eye.y += raise;
            }
            None => self.eye.y += raise,
        }
    }

    /// The camera matrix inputs: position, look-at point, up vector, vertical field of view (radians).
    pub fn view(&self) -> (Vec3, Vec3, Vec3, f32) {
        (self.eye, self.look, self.up, self.fov)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<DebugTweakables><Camera><Flycam_Invert_Y>false</Flycam_Invert_Y><Dynamic_FOV>true</Dynamic_FOV><Engine_Shake>false</Engine_Shake><FOV>0.75</FOV><Height>1.5</Height><Behind>1.3</Behind><Height_Min_Dist>0.0</Height_Min_Dist><Height_Max_Dist>0.4</Height_Max_Dist><Smooth_Cam_Pos>15.0</Smooth_Cam_Pos><Smooth_Look_Pos>8.0</Smooth_Look_Pos><Smooth_Cam_Up>15.0</Smooth_Cam_Up><Lateral_Move_With_Slide>2.5</Lateral_Move_With_Slide></Camera></DebugTweakables>";

    fn cam() -> ChaseCam {
        ChaseCam::new(CamTweaks::default())
    }

    #[test]
    fn tweaks_parse_and_default() {
        let t = CamTweaks::from_xml(XML);
        assert_eq!(t.lateral_slide, 2.5);
        assert_eq!(t.smooth_pos, 15.0);
        assert!(t.dynamic_fov && !t.engine_shake);
        assert_eq!(CamTweaks::from_xml("not xml"), CamTweaks::default());
        assert_eq!(CamTweaks::default().fov, 0.75);
    }

    #[test]
    fn height_zoom_behind_modifiers() {
        let c = cam();
        let mut car = CamCar::simple(Vec3::ZERO, Quat::IDENTITY);
        car.speed = 0.0;
        assert_eq!(c.height_mod(&car), 0.0); // clamped to Height_Min_Dist
        car.speed = 40.0;
        assert!((c.height_mod(&car) - 0.15).abs() < 1e-6); // 0.005 * (40 - 10)
        car.vel = Vec3::new(0.0, 5.0, 0.0);
        assert!((c.height_mod(&car) - 0.65).abs() < 1e-6); // + 0.1 * (v . up)
        car.speed = 100.0;
        assert_eq!(c.zoom_mod(&car), 1.31);
        car.speed = 0.0;
        assert_eq!(c.zoom_mod(&car), 1.0);
        assert_eq!(c.behind_mod(&car), 0.0);
        car.accel = 5.0;
        assert_eq!(c.behind_mod(&car), 7.0); // 3.5 * clamp(5, -1, 2)
        car.behind_boost = Some(1.0);
        assert_eq!(c.behind_mod(&car), 8.0);
    }

    #[test]
    fn rest_camera_is_eight_metres_behind_and_above() {
        let mut c = cam();
        let car = CamCar::simple(Vec3::new(10.0, 2.0, 5.0), Quat::IDENTITY);
        c.update(1.0 / 60.0, &car, None);
        let rel = c.eye - car.pos;
        assert!((rel.length() - 8.0).abs() < 1e-4, "{rel:?}");
        // direction of (0, 2.3, -5.2)
        let want = Vec3::new(0.0, 2.3, -5.2).normalize() * 8.0;
        assert!((rel - want).length() < 1e-4, "{rel:?} vs {want:?}");
        assert!((c.look - (car.pos + Vec3::new(0.0, 1.8, 2.5))).length() < 1e-5);
    }

    #[test]
    fn smoothing_follows_the_formula() {
        let mut c = cam();
        let mut car = CamCar::simple(Vec3::ZERO, Quat::IDENTITY);
        let dt = 1.0 / 60.0;
        c.update(dt, &car, None);
        let before = c.prev.pos;
        // turn the car 90 degrees: the new relative position is rotated, the old one is weighted 15:1
        car.rot = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let target = c.rear_settings(&car).pos;
        c.update(dt, &car, None);
        let want = (target + before * 15.0) / 16.0;
        assert!((c.prev.pos - want).length() < 1e-4, "{:?} vs {:?}", c.prev.pos, want);
    }

    #[test]
    fn lateral_slide_and_forward_blend() {
        let c = cam();
        let mut car = CamCar::simple(Vec3::ZERO, Quat::IDENTITY);
        car.vel = Vec3::new(10.0, 0.0, 10.0); // sliding along +x while pointing along +z
        let s = c.rear_settings(&car);
        // forward axis blended 25% towards the velocity direction, right axis re-derived as fwd x up
        let vd = car.vel.normalize();
        let fwd = (Vec3::Z + (vd - Vec3::Z) * 0.25).normalize();
        let right = fwd.cross(Vec3::Y);
        let slide = Vec3::X.dot(vd) * 2.0;
        let pos = Vec3::new(slide, 1.5 * (1.0 - 0.0) + 0.8, -5.2);
        let want = right * pos.x + Vec3::Y * pos.y + fwd * pos.z;
        assert!((s.pos - want).length() < 1e-4, "{:?} vs {:?}", s.pos, want);
    }

    /// A wall at x = `x` (normal -x) and nothing else.
    struct Wall {
        x: f32,
    }
    impl Ground for Wall {
        fn height_normal(&self, _p: Vec3) -> (f32, Vec3) {
            (-1.0e4, Vec3::Y)
        }
        fn cam_ray(&self, origin: Vec3, vec: Vec3) -> Option<(Vec3, Vec3)> {
            let (s0, s1) = (origin.x - self.x, origin.x + vec.x - self.x);
            if (s0 > 0.0) == (s1 > 0.0) {
                return None;
            }
            let t = s0 / (s0 - s1);
            Some((origin + vec * t, if s0 > 0.0 { Vec3::X } else { -Vec3::X }))
        }
        fn cam_sphere(&self, c: Vec3, r: f32) -> Option<(Vec3, Vec3)> {
            let d = self.x - c.x;
            if d.abs() < r {
                Some((Vec3::new(self.x, c.y, c.z), if d > 0.0 { -Vec3::X } else { Vec3::X }))
            } else {
                None
            }
        }
    }

    #[test]
    fn collision_keeps_the_camera_in_front_of_the_wall() {
        // car yawed to look along +x, wall 3 m behind the car: the camera (8 m behind) would be inside it
        let mut c = cam();
        let car = CamCar::simple(Vec3::ZERO, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let wall = Wall { x: -3.0 };
        c.update(1.0 / 60.0, &car, Some(&wall));
        assert!(c.eye.x > -3.0, "camera went through the wall: {:?}", c.eye);
        // pushed out by the sphere radius, raised by a quarter of the pulled-in distance
        assert!((c.eye.x - (-3.0 + 0.75)).abs() < 1e-3, "{:?}", c.eye);
        assert!(c.eye.y > 2.3);
        // without a wall in between nothing changes
        let mut free = cam();
        free.update(1.0 / 60.0, &car, Some(&Wall { x: -50.0 }));
        let mut none = cam();
        none.update(1.0 / 60.0, &car, None);
        assert!((free.eye - none.eye).length() < 1e-5);
    }

    #[test]
    fn slingshot_camera_looks_along_the_launch_direction() {
        let mut c = cam();
        c.set_mode(CamMode::Slingshot);
        let mut car = CamCar::simple(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY);
        car.sling_frame = Some([Vec3::X, Vec3::Y, -Vec3::Z]); // back = -z, so the launch direction is +z
        c.update(1.0 / 60.0, &car, None);
        // up 1 m, 10 m behind; look 10 m ahead
        assert!((c.eye - (car.pos + Vec3::new(0.0, 1.0, -10.0))).length() < 1e-4, "{:?}", c.eye);
        assert!((c.look - (car.pos + Vec3::new(0.0, 0.0, 10.0))).length() < 1e-4);
        car.sling_pull = 1.0;
        c.update(1.0 / 60.0, &car, None);
        assert!((c.eye.z - (car.pos.z - 17.4)).abs() < 1e-4, "{:?}", c.eye);
    }

    #[test]
    fn handover_starts_at_the_slingshot_camera_and_ends_at_the_rear_camera() {
        let mut c = cam();
        c.set_mode(CamMode::Slingshot);
        let mut car = CamCar::simple(Vec3::ZERO, Quat::IDENTITY);
        car.sling_frame = Some([Vec3::X, Vec3::Y, -Vec3::Z]);
        c.update(1.0 / 60.0, &car, None);
        let sling_eye = c.eye;
        c.set_mode(CamMode::Rear);
        c.update(1.0 / 60.0, &car, None);
        assert!((c.eye - sling_eye).length() < 1e-3, "first frame still at the slingshot camera");
        for _ in 0..180 {
            c.update(1.0 / 60.0, &car, None);
        }
        let rear = c.rear_settings(&car).pos;
        let want = rear * (MIN_DIST / rear.length()); // |(0, 2.3, -5.2)| < 8 is pushed out to 8
        assert!((c.eye - want).length() < 0.05, "{:?} vs {:?}", c.eye, want);
    }

    #[test]
    fn upside_down_lifts_the_camera_along_the_track_up() {
        let mut c = cam();
        let mut car = CamCar::simple(Vec3::ZERO, Quat::from_rotation_z(std::f32::consts::PI)); // car on its roof
        car.wheels_on_ground = 0;
        car.track_up = Some(Vec3::Y);
        let mut c2 = cam();
        let mut upright = CamCar::simple(Vec3::ZERO, Quat::from_rotation_z(std::f32::consts::PI));
        upright.wheels_on_ground = 4;
        for _ in 0..60 {
            c.update(1.0 / 60.0, &car, None);
            c2.update(1.0 / 60.0, &upright, None);
        }
        assert!(c.eye.y > c2.eye.y, "{} vs {}", c.eye.y, c2.eye.y);
        assert!(c.eye.y >= 0.2 - 1e-3);
    }

    #[test]
    fn impact_shake_moves_only_the_look_at() {
        let mut a = cam();
        let mut b = cam();
        let mut car = CamCar::simple(Vec3::ZERO, Quat::IDENTITY);
        a.update(0.5, &car, None);
        car.impact_shake = 100.0;
        b.update(0.5, &CamCar::simple(Vec3::ZERO, Quat::IDENTITY), None);
        b.update(0.5, &car, None);
        a.update(0.5, &CamCar::simple(Vec3::ZERO, Quat::IDENTITY), None);
        assert!((a.eye - b.eye).length() < 1e-5);
        assert!((a.look - b.look).length() > 1e-3);
        assert!((a.look - b.look).length() <= 100.0 * (SHAKE_X + SHAKE_Y) + 1e-4);
    }
}
