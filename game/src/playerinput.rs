//! Player input of Angry Birds Go! 2.9.1, ported 1:1 from the Ghidra decompile/disassembly of `libABK291.so`
//! (Ghidra addresses; file offset = address - 0x10000). Depends only on `std` and `glam`.
//!
//! Sources
//! * `CPlayer::ProcessInput(float) @ 001470e8`  -> steering + brake that reach `CCar::SetSteering @ 0019b198` / `SetBrake @ 0019b2c4`
//! * `CPlayer::Process @ 00146bb8`              -> the "not racing" override (brake 1, steer 0)
//! * `CPlayer::UpdateSlingshotLaunch(float) @ 00147a90` -> slingshot touch drag, pull ratio (`CPlayer+0x22c/+0x230/+0x234`),
//!   launch trigger and `CCar::SetSlingshotOffset @ 0019aa8c` vector.
//!
//! The ARM VFP code is mis-decompiled in places (arguments lost), so everything below was read from the disassembly listing
//! (headless Ghidra `DumpListing`), and the literal pools/curve table were read from the `.so` bytes.
//!
//! # Facts established from the binary
//! * `CRaceAI::GetSteeringAssistValue @ 0018c65c` is NOT used for the human player: `CPlayer::ProcessInput` never calls it
//!   (it lives in `CRaceAI`, used only by AI drivers). No steering assist exists on the player path.
//! * The shipped default is `Push_Button_Controls = true` (debug bool 0x18, `SetDebugTweakablesFromXML @ 002c7258` stores it at
//!   0xDE0690 = bool table 0xDE0630 + 0x18*4): steering is ramped by left/right *buttons* (touches on the left/right half of the
//!   screen, or pad keys 2 = left / 3 = right). The drag mode (`Steer_Touch_Width`) is the `Push_Button_Controls = false` branch.
//!   So the keyboard path below is the original pad-key path exactly, not an analogue.
//! * Debug bool 0x17 = `Steer_Flip`, floats 0x23 = `Steer_Touch_Width`, 0x24 = `Steer_Exponent`, 0x25/0x26 = Motion_Steer_*.
//! * `CCarSpec+0x464` = `m_fSteerAdjRate` (12.0), `+0x468` = `m_fCentreSteerAdjRate` (16.0): the button ramp rates.
//!
//! # UNRESOLVED (also marked in code)
//! * Accelerometer/tilt steering (`MotionGetSensorData`, Motion_Steer_*; only when the tilt option is enabled): not ported.
//! * Debug bool 0x1a (debug rumble + `PadGetAxis` override of the steering): not ported.
//! * `GetTouchRay @ 002a2780` (camera unprojection of the touch) and the camera matrices `CPlayer+0x54/+0xa4/+0xb0`:
//!   the caller supplies the ray / camera frame.
//! * Multiplayer early-launch condition (`game+0x1c0 > 1` and two global floats differ by > 0.75): caller flag.
//! * Game mode 8 (FTUE auto-launch demonstration through `CPlayer+0x278`) and the slingshot camera yaw/pitch calls.
//! * The FE screen hit test used to ignore touches over UI (`CXGSFEScreenStack current screen vfunc 0xb0`): caller flag.
#![allow(dead_code, clippy::too_many_arguments)]

use glam::Vec3;

// =============================================================================================================
// Steering: CPlayer::ProcessInput @ 001470e8
// =============================================================================================================

/// Values `ProcessInput` reads through `CDebugManager::GetDebugBool/GetDebugFloat` and from the car spec.
#[derive(Clone, Copy, Debug)]
pub struct SteerTweakables {
    /// `GetDebugBool(0x18)` = `Push_Button_Controls` (true in debugtweakables.xml).
    pub push_button_controls: bool,
    /// `GetDebugBool(0x17)` = `Steer_Flip` (false).
    pub steer_flip: bool,
    /// `GetDebugFloat(0x23)` = `Steer_Touch_Width` (257.0).
    pub steer_touch_width: f32,
    /// `GetDebugFloat(0x24)` = `Steer_Exponent` (1.1).
    pub steer_exponent: f32,
    /// `CCarSpec+0x464` `m_fSteerAdjRate` (default 12.0, per-car in the spec XML).
    pub steer_adj_rate: f32,
    /// `CCarSpec+0x468` `m_fCentreSteerAdjRate` (default 16.0).
    pub centre_steer_adj_rate: f32,
}
impl Default for SteerTweakables {
    fn default() -> Self {
        SteerTweakables {
            push_button_controls: true,
            steer_flip: false,
            steer_touch_width: 257.0,
            steer_exponent: 1.1,
            steer_adj_rate: 12.0,
            centre_steer_adj_rate: 16.0,
        }
    }
}

/// `DAT_0014763c` = 0x3a800000 = 1/1024: `s17 = display_width * (1/1024)` in the drag formula.
pub const STEER_WIDTH_SCALE: f32 = 0.000_976_562_5;
/// `DAT_00147644` = 0x3ccccccd: minimum brake while `CPlayer+0x1f0 != 0` (`max(car.brake, 0.025)`).
pub const MIN_BRAKE_FLAGGED: f32 = 0.025;
/// Hard-coded exponent of the analogue pad axis (`movt r1,#0x3f8c; movw r1,#0xcccd` = 0x3f8ccccd) in `ProcessInput`.
pub const PAD_AXIS_EXPONENT: f32 = 1.1;

/// One touch point as `CControlsManager::GetTouchPointsForViewport @ 00284adc` returns it (only `x` is used for steering).
#[derive(Clone, Copy, Debug, Default)]
pub struct SteerTouch {
    /// x in pixels (display coordinates).
    pub x: f32,
    /// The current FE screen's hit test (`vfunc 0xb0`) claims this touch (UI button): ignored in push-button mode.
    // UNRESOLVED: the screen hit test is UI state; the caller decides.
    pub over_ui: bool,
}

/// Per-frame state that gates `ProcessInput` (all read from game/car objects in the original).
#[derive(Clone, Copy, Debug)]
pub struct SteerContext {
    /// false when `ProcessInput` takes its first early-out: game mode 2/5/9 (`game+0xc8`) or `car+0x1ae8 == 0`
    /// (race not started): brake = 1.0 and steering is not touched.
    pub input_enabled: bool,
    /// FTUE state 0 active and `game+0x58...+0xa30 == 0`: `SetSteering(0)` instead of the input.
    pub ftue_blocks_steering: bool,
    /// `CPlayer+0x1f0 != 0`: brake is raised to at least 0.025 after the input brake.
    pub min_brake_flag: bool,
}
impl Default for SteerContext {
    fn default() -> Self {
        SteerContext { input_enabled: true, ftue_blocks_steering: false, min_brake_flag: false }
    }
}

/// Everything `ProcessInput` samples in one frame.
#[derive(Clone, Copy, Debug)]
pub struct SteerFrame<'a> {
    pub dt: f32,
    /// `CLayoutManager::GetDisplayWidthPixelsIgnoreSafezone`.
    pub screen_w_px: f32,
    pub touches: &'a [SteerTouch],
    /// Pad keys 2 / 3 (`CControlsManager::PadKeyDown(pad, 2|3)`): left / right.
    pub key_left: bool,
    pub key_right: bool,
    /// Analogue pad axis 0 x (`PadGetAxis`), -1..1; 0 for keyboard.
    pub axis_x: f32,
    pub ctx: SteerContext,
}

/// Port of the steering/brake part of `CPlayer::ProcessInput @ 001470e8`.
#[derive(Clone, Debug)]
pub struct PlayerSteering {
    pub tw: SteerTweakables,
    /// `CPlayer+0x224`: the ramped button steering value (-1..1). Reset to 0 in the ctor and `OnRestart @ 001469ac`.
    pad_steer: f32,
    /// Last value passed to `SetSteering` (kept when the early-out does not set steering).
    pub steer: f32,
    /// Display width used by the convenience [`PlayerSteering::update`].
    pub screen_w_px: f32,
}

/// Keyboard stand-in for the pad keys, see [`PlayerSteering::update`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SteerKeys {
    pub left: bool,
    pub right: bool,
    /// ANALOGUE (no original equivalent in push-button mode, where the brake is always 0): equals the original's
    /// two-finger brake of the drag mode (`brake = 1.0`).
    pub brake: bool,
}

impl PlayerSteering {
    pub fn new(tw: SteerTweakables) -> PlayerSteering {
        PlayerSteering { tw, pad_steer: 0.0, steer: 0.0, screen_w_px: 1280.0 }
    }

    /// `CPlayer::OnRestart @ 001469ac` / ctor: `+0x224 = 0`.
    pub fn reset(&mut self) {
        self.pad_steer = 0.0;
    }

    /// `CPlayer+0x224` (for inspection).
    pub fn pad_steer(&self) -> f32 {
        self.pad_steer
    }

    /// `CPlayer::Process @ 00146bb8` override (`SetBrake(1.0); SetSteering(0.0)`): applies when the game mode is 2 or 9,
    /// `car+0x1ae8 == 0`, `car+0x1b5c != 0` (finished) or the pad id is negative. Returns `(steer, brake)`.
    pub fn process_override() -> (f32, f32) {
        (0.0, 1.0)
    }

    /// Convenience API. `touch_dx_px`: `Some(x - screen_w/2)` of a single touch = DRAG mode (`Push_Button_Controls = false`
    /// formula, port of 0x1478c0); `None` = push-button mode with the keys (the original pad-key path 0x147690..).
    ///
    /// Returns `(steer, brake)` to feed `CarInput { steer, brake }`.
    ///
    /// ANALOGUE parts: only `keys.brake` (see [`SteerKeys::brake`]). Holding a key is NOT turned into a virtual touch drag:
    /// the original has a faithful digital path (the button ramp with `m_fSteerAdjRate`/`m_fCentreSteerAdjRate`), which is
    /// what the keys drive. A drag therefore only exists when `touch_dx_px` is supplied (mouse/touch).
    pub fn update(&mut self, dt: f32, touch_dx_px: Option<f32>, keys: SteerKeys) -> (f32, f32) {
        let w = self.screen_w_px;
        let t = [SteerTouch { x: w * 0.5 + touch_dx_px.unwrap_or(0.0), over_ui: false }];
        let drag = touch_dx_px.is_some();
        let saved = self.tw.push_button_controls;
        if drag {
            self.tw.push_button_controls = false;
        }
        let frame = SteerFrame {
            dt,
            screen_w_px: w,
            touches: if drag { &t } else { &[] },
            key_left: keys.left,
            key_right: keys.right,
            axis_x: 0.0,
            ctx: SteerContext::default(),
        };
        let (s, mut b) = self.update_frame(&frame);
        self.tw.push_button_controls = saved;
        if keys.brake {
            b = 1.0;
        }
        (s, b)
    }

    /// `CPlayer::ProcessInput @ 001470e8` (steering + brake). Returns `(steer, brake)`.
    pub fn update_frame(&mut self, f: &SteerFrame) -> (f32, f32) {
        // 0x14712c..0x147154: game mode 2/5/9 or car not started -> SetBrake(1.0), return (no SetSteering).
        if !f.ctx.input_enabled {
            return (self.steer, 1.0);
        }
        let half = f.screen_w_px * 0.5; // s16 = (float)width * 0.5 (0x1472f4)
        let mut brake = 0.0f32; // r5
        let mut s: f32; // s16

        if self.tw.push_button_controls {
            // 0x1475c4..: count touches left/right of the screen centre (UI-claimed touches ignored), then the pad keys.
            let mut n: i32 = 0;
            for t in f.touches {
                if !t.over_ui {
                    if t.x - half > 0.0 {
                        n += 1;
                    } else {
                        n -= 1;
                    }
                }
            }
            if f.key_left {
                n -= 1; // PadKeyDown(pad, 2)
            }
            if f.key_right {
                n += 1; // PadKeyDown(pad, 3)
            }
            let v = self.pad_steer;
            if n > 0 {
                // 0x147690: v += (1 - v) * dt * m_fSteerAdjRate, min(v, 1)
                let r = v + (1.0 - v) * f.dt * self.tw.steer_adj_rate;
                self.pad_steer = if r <= 1.0 { r } else { 1.0 };
            } else if n == 0 {
                // 0x147864: v += (0 - v) * dt * m_fCentreSteerAdjRate; clamp to [0, 1]
                let r = v + (0.0 - v) * f.dt * self.tw.centre_steer_adj_rate;
                self.pad_steer = if r < 0.0 {
                    0.0
                } else if r > 1.0 {
                    1.0
                } else {
                    r
                };
            } else {
                // 0x147984: v += (-1 - v) * dt * m_fSteerAdjRate, max(v, -1)
                let r = v + (-1.0 - v) * f.dt * self.tw.steer_adj_rate;
                self.pad_steer = if r >= -1.0 { r } else { -1.0 };
            }
            // 0x1476cc: analogue axis only extends the value (keyboard: axis_x == 0)
            let a = f.axis_x;
            if a < 0.0 {
                let m = -((-a).powf(PAD_AXIS_EXPONENT));
                if m < self.pad_steer {
                    self.pad_steer = m; // min(v, -|a|^1.1)
                }
            } else if a > 0.0 {
                let m = a.powf(PAD_AXIS_EXPONENT);
                if self.pad_steer <= m {
                    self.pad_steer = m; // max(v, |a|^1.1)
                }
            }
            s = self.pad_steer;
        } else {
            // DRAG mode, Push_Button_Controls == false (0x1472ec..0x147368, 0x1478c0..0x147908)
            let sw = f.screen_w_px * STEER_WIDTH_SCALE; // s17
            let denom = sw * (sw * self.tw.steer_touch_width);
            let n = f.touches.len().min(8);
            if n == 1 {
                s = -(f.touches[0].x - half) / denom;
                s = s.clamp(-1.0, 1.0);
            } else if n < 1 {
                s = 0.0;
            } else {
                // two (or more) touches: x of the first two averaged, and the BRAKE is set to 1.0 (r5 = 1.0f)
                let avg = (f.touches[0].x + f.touches[1].x) * 0.5;
                s = -(avg - half) / denom;
                s = s.clamp(-1.0, 1.0);
                brake = 1.0;
            }
        }

        // 0x147720: Steer_Flip
        if self.tw.steer_flip {
            s = -s;
        }
        // 0x14773c: signed power with Steer_Exponent
        s = if s < 0.0 { -((-s).powf(self.tw.steer_exponent)) } else { s.powf(self.tw.steer_exponent) };

        // 0x1473c0..0x147420: SetSteering(clamp(s, -1, 1)), or SetSteering(0) while the FTUE blocks it.
        self.steer = if f.ctx.ftue_blocks_steering { 0.0 } else { s.clamp(-1.0, 1.0) };
        // UNRESOLVED: tilt steering (0x14742c..0x1474f8) needs the accelerometer; not ported.

        // 0x147500: SetBrake(r5); 0x147508: min brake while CPlayer+0x1f0 is set.
        if f.ctx.min_brake_flag && brake <= MIN_BRAKE_FLAGGED {
            brake = MIN_BRAKE_FLAGGED;
        }
        (self.steer, brake)
    }
}

// =============================================================================================================
// Slingshot: CPlayer::UpdateSlingshotLaunch @ 00147a90
// =============================================================================================================

/// The pull curve table at 0xCEBED0 (`DAT_00148b48 + 0x148d08`, identical to `DAT_00148b4c + 0x148d58`): 8 (x, y) pairs read
/// from the `.so`. Input `x = 2 * dy/H` (`dy` = vertical drag / screen height), output = `CPlayer+0x230`.
pub const PULL_CURVE: [(f32, f32); 8] = [
    (f32::from_bits(0x3e2e147b), f32::from_bits(0x3e051eb8)), // 0.17 -> 0.13
    (f32::from_bits(0x3e6b851f), f32::from_bits(0x3e3851ec)), // 0.23 -> 0.18
    (f32::from_bits(0x3e8f5c29), f32::from_bits(0x3e75c28f)), // 0.28 -> 0.24
    (f32::from_bits(0x3ea3d70a), f32::from_bits(0x3e99999a)), // 0.32 -> 0.30
    (f32::from_bits(0x3ebd70a4), f32::from_bits(0x3ecccccd)), // 0.37 -> 0.40
    (f32::from_bits(0x3eeb851f), f32::from_bits(0x3f000000)), // 0.46 -> 0.50
    (f32::from_bits(0x3f35c28f), f32::from_bits(0x3f333333)), // 0.71 -> 0.70
    (f32::from_bits(0x3f75c28f), f32::from_bits(0x3f800000)), // 0.96 -> 1.00
];
/// Output below the first breakpoint (and for negative input): `DAT_00148b14` = 0x3e051eb8.
pub const PULL_CURVE_FLOOR: f32 = f32::from_bits(0x3e051eb8);
/// Release launches when `CPlayer+0x22c > DAT_00148280` (0x3e8a3d71).
pub const LAUNCH_MIN_RATIO: f32 = f32::from_bits(0x3e8a3d71);
/// `ratio (+0x22c) = clamp(dy / 0.2, 0, 1)`: `DAT_00147bd8` = 0x3e4ccccd.
pub const RATIO_FULL_DRAG: f32 = f32::from_bits(0x3e4ccccd);
/// Pitch term starts at `dy = 0.2` (`DAT_0014828c`) and spans 0.3 (`DAT_00148270` = 0x3e99999a) to `PI` (`DAT_00148278`).
pub const PITCH_START: f32 = f32::from_bits(0x3e4ccccd);
pub const PITCH_SPAN: f32 = f32::from_bits(0x3e99999a);
/// Lateral scale `ratio * 8.4` (`DAT_00148274` = 0x41066666).
pub const PULL_LENGTH: f32 = f32::from_bits(0x41066666);
/// Pitch weight in the vertical term: 0.5, or `DAT_0014827c` = 0x3ea8f5c3 (0.33) in game mode 10.
pub const PITCH_WEIGHT: f32 = 0.5;
pub const PITCH_WEIGHT_MODE10: f32 = f32::from_bits(0x3ea8f5c3);
/// Yaw: `theta = (x/W - 0.5) * 0.25 * PI` (+-22.5 deg); outside the screen the constants are cos/sin(22.5 deg) as doubles
/// (`0x3fed906bccf3cb87`, `0x3fd87de2b185d541`).
pub const YAW_RANGE: f32 = 0.25;
pub const COS_22_5: f64 = f64::from_bits(0x3fed906b_ccf3cb87);
pub const SIN_22_5: f64 = f64::from_bits(0x3fd87de2_b185d541);
/// Early-launch penalty cooldown `CPlayer+0x254 = 0x3f266666` (0.65 s).
pub const EARLY_LAUNCH_COOLDOWN: f32 = f32::from_bits(0x3f266666);
/// Ray-sphere test around the kart: radius^2 = 2.25 (`vmov.f32 s11,#0x40100000`), centre = kart node + (0, 0.2, 0)
/// (`DAT_0014828c`).
pub const KART_PICK_RADIUS_SQ: f32 = 2.25;
pub const KART_PICK_Y_OFFSET: f32 = f32::from_bits(0x3e4ccccd);
/// Analogue/key stick dead zone: `DAT_00147bd4` = 0x3dcccccd (0.1). Key values are `/255` (`DAT_00147bd0`).
pub const STICK_DEADZONE: f32 = f32::from_bits(0x3dcccccd);

/// The pull curve: `CPlayer+0x230` from `v = 2 * dy / screen_h` (0x147e3c..0x147ed4 / 0x1487f4..0x14888c).
pub fn pull_curve(v: f32) -> f32 {
    if v < 0.0 {
        return PULL_CURVE_FLOOR;
    }
    let v = if v > 1.0 { 1.0 } else { v };
    if v < PULL_CURVE[0].0 {
        return PULL_CURVE_FLOOR;
    }
    // first threshold the value is strictly below (chain of compares vs x[1]..x[7]), segment i uses x[i-1]..x[i]
    for i in 1..8 {
        if v < PULL_CURVE[i].0 {
            let (x0, y0) = PULL_CURVE[i - 1];
            let (x1, y1) = PULL_CURVE[i];
            let t = (v - x0) / (x1 - x0);
            return t * y1 + (1.0 - t) * y0;
        }
    }
    1.0
}

/// `ratio` of `CPlayer+0x22c`: `clamp(dy / 0.2, 0, 1)` with `dy = (touch_y - anchor_y) / screen_h`.
pub fn linear_ratio(dy: f32) -> f32 {
    let r = dy / RATIO_FULL_DRAG;
    if r < 0.0 {
        0.0
    } else if r > 1.0 {
        1.0
    } else {
        r
    }
}

/// Camera data `CPlayer` keeps for the slingshot (`+0xa4` camera position, `+0xb0` look-at target, `+0x54` up vector).
#[derive(Clone, Copy, Debug)]
pub struct CamFrame {
    pub pos: Vec3,
    pub target: Vec3,
    pub up: Vec3,
}

/// `SetSlingshotOffset` vector (`0x147f4c..0x14804c`, `0x148908..0x148a08`) in the player's camera frame:
/// `offset = b * w + c * r + a * f` with `f = normalize(target - pos)`, `r = up x f`, `w = f x r`,
/// `a = -L cos(theta)`, `c = L sin(theta)`, `L = ratio * 8.4`, `b = -ratio * (1 + k * t)`, `t = clamp((dy - 0.2)/0.3, 0, 1) * PI`.
/// `x_frac = touch_x / screen_w`, `dy = (touch_y - anchor_y) / screen_h`, `ratio` = [`linear_ratio`].
pub fn slingshot_offset(ratio: f32, dy: f32, x_frac: f32, cam: &CamFrame, game_mode_10: bool) -> Vec3 {
    // sin/cos of theta in double precision (vcvt.f64.f32 + libm sin/cos), constants outside [0,1]
    let (sin_t, cos_t) = if x_frac < 0.0 {
        (-SIN_22_5, COS_22_5)
    } else if x_frac > 1.0 {
        (SIN_22_5, COS_22_5)
    } else {
        let theta: f32 = (x_frac - 0.5) * YAW_RANGE * std::f32::consts::PI;
        let th = theta as f64;
        (th.sin(), th.cos())
    };
    let l = ratio * PULL_LENGTH;
    let a = (-(l as f64) * cos_t) as f32;
    let c = ((l as f64) * sin_t) as f32;
    let tp = {
        let p = (dy - PITCH_START) / PITCH_SPAN;
        if p < 0.0 {
            0.0
        } else if p <= 1.0 {
            p * std::f32::consts::PI
        } else {
            std::f32::consts::PI
        }
    };
    let k = if game_mode_10 { PITCH_WEIGHT_MODE10 } else { PITCH_WEIGHT };
    let b = -(ratio * (1.0 + k * tp));

    let d = cam.target - cam.pos;
    let len = d.length();
    let f = d * (1.0 / len);
    let u = cam.up;
    let r = Vec3::new(u.y * f.z - u.z * f.y, u.z * f.x - u.x * f.z, u.x * f.y - u.y * f.x); // up x f
    let w = Vec3::new(r.z * f.y - r.y * f.z, r.x * f.z - r.z * f.x, r.y * f.x - r.x * f.y); // f x r
    w * b + r * c + f * a
}

/// The kart pick of `UpdateSlingshotLaunch` (0x1482a0..0x1483f4): ray vs sphere (radius 1.5 m) around
/// `kart_node + (0, 0.2, 0)`. `dir` must be unit length (`GetTouchRay @ 002a2780`).
// UNRESOLVED: GetTouchRay (camera unprojection) is not ported; the caller builds origin/dir from its camera.
pub fn ray_hits_kart(origin: Vec3, dir: Vec3, kart_node: Vec3) -> bool {
    let center = kart_node + Vec3::new(0.0, KART_PICK_Y_OFFSET, 0.0);
    let oc = origin - center;
    let c = oc.length_squared() - KART_PICK_RADIUS_SQ;
    if c < 0.0 {
        return true; // origin inside the sphere (blt 0x1483f4)
    }
    let b = dir.dot(oc);
    let b2 = b + b;
    let disc = b2 * b2 - c * 4.0;
    if disc < 0.0 {
        return false;
    }
    let t = (-b2 - disc.sqrt()) * 0.5; // min of the two roots, halved
    t >= 0.0
}

/// One frame of slingshot output.
#[derive(Clone, Copy, Debug, Default)]
pub struct SlingshotOutput {
    /// `CPlayer+0x230`: the curve value (feed `carsim::LaunchParams.player_pull_ratio`). On the launch frame this is the
    /// latched `CPlayer+0x234`.
    pub pull_ratio: f32,
    /// `CPlayer+0x22c`: the linear 0..1 pull (drives the HUD/animation).
    pub linear_ratio: f32,
    /// `CCar::SetSlingshotOffset` argument while pulling (`None` when no pull is in progress). On the launch frame it is the
    /// last offset (the car keeps it in `CCar+0x46c`).
    pub pull_offset_dir: Option<Vec3>,
    /// `car+0x478` (`SetUserTouchingSlingshot`).
    pub touching: bool,
    /// `CCar::SetInSlingshot(-1)` (launch) happens this frame.
    pub launch: bool,
}

/// Port of `CPlayer::UpdateSlingshotLaunch @ 00147a90`. State fields mirror `CPlayer+0x218..0x278`.
#[derive(Clone, Debug)]
pub struct SlingshotInput {
    /// `CPlayer+0x258` / `+0x25c` both non-zero (`SetSlingshotEnabled @ 00148ea8`); if either is 0 the function returns.
    pub enabled: bool,
    /// `CPlayer+0x254`: cooldown after an early launch penalty.
    cooldown: f32,
    /// `CPlayer+0x22c`
    ratio: f32,
    /// `CPlayer+0x230`
    curve: f32,
    /// `CPlayer+0x234` (latched at launch)
    latched: f32,
    /// `car+0x478`
    touching: bool,
    /// `CPlayer+0x250` (touch y at the moment the kart was picked / halfH for the pad cursor)
    anchor_y: f32,
    /// `CPlayer+0x220 == 0`: pad/keyboard cursor mode active (the ctor sets 0x220 = 1 = idle/touch).
    pad_active: bool,
    /// `CPlayer+0x218/0x21c`: virtual cursor of the pad mode.
    cursor: (f32, f32),
    last_offset: Option<Vec3>,
    /// Display size in pixels (`GetDisplayWidthPixels` / `Height`).
    pub screen_w_px: f32,
    pub screen_h_px: f32,
}

impl SlingshotInput {
    pub fn new(screen_w_px: f32, screen_h_px: f32) -> SlingshotInput {
        SlingshotInput {
            enabled: true,
            cooldown: 0.0,
            ratio: 0.0,
            curve: 0.0,
            latched: 0.0,
            touching: false,
            anchor_y: 0.0,
            pad_active: false,
            cursor: (screen_w_px * 0.5, screen_h_px * 0.5),
            last_offset: None,
            screen_w_px,
            screen_h_px,
        }
    }

    /// `CPlayer+0x230` / `+0x22c` (for HUD).
    pub fn pull_ratio(&self) -> f32 {
        self.curve
    }
    pub fn linear_ratio(&self) -> f32 {
        self.ratio
    }
    pub fn is_touching(&self) -> bool {
        self.touching
    }

    fn idle_out(&self) -> SlingshotOutput {
        SlingshotOutput { pull_ratio: self.curve, linear_ratio: self.ratio, pull_offset_dir: None, touching: self.touching, launch: false }
    }

    /// The release branch (0x1480c4..0x1481bc).
    fn release(&mut self, early_launch_penalty: bool) -> SlingshotOutput {
        let mut out = SlingshotOutput::default();
        if self.ratio > LAUNCH_MIN_RATIO && self.touching {
            if early_launch_penalty {
                // 0x148c94: CPlayer+0x254 = 0.65, no launch.
                // UNRESOLVED: the original condition is `game+0x1c0 >= 2 && (*global_a - *global_b) > 0.75`.
                self.cooldown = EARLY_LAUNCH_COOLDOWN;
            } else {
                self.latched = self.curve; // +0x234 = +0x230
                out.launch = true;
                out.pull_ratio = self.latched;
                out.linear_ratio = self.ratio;
                out.pull_offset_dir = self.last_offset;
            }
        }
        // 0x1481a4: ratio = curve = 0, SetUserTouchingSlingshot(0)
        self.ratio = 0.0;
        self.curve = 0.0;
        self.touching = false;
        self.pad_active = false;
        self.cursor = (self.screen_w_px * 0.5, self.screen_h_px * 0.5);
        if !out.launch {
            out.pull_ratio = 0.0;
            out.linear_ratio = 0.0;
        }
        out.touching = false;
        out
    }

    /// The held branch (0x147dfc..0x14804c): pull ratio, curve and offset from the current touch/cursor position.
    fn pull(&mut self, x: f32, y: f32, cam: &CamFrame, game_mode_10: bool) -> SlingshotOutput {
        let dy = (y - self.anchor_y) / self.screen_h_px;
        self.ratio = linear_ratio(dy);
        self.curve = pull_curve(dy + dy);
        let x_frac = x / self.screen_w_px;
        let off = slingshot_offset(self.ratio, dy, x_frac, cam, game_mode_10);
        self.last_offset = Some(off);
        SlingshotOutput { pull_ratio: self.curve, linear_ratio: self.ratio, pull_offset_dir: Some(off), touching: true, launch: false }
    }

    /// Touch path (`CPlayer+0x220 == 1`).
    ///
    /// * `touch`: `Some((x_px, y_px))` of the first touch point, `None` when no finger is down.
    /// * `kart_picked`: result of [`ray_hits_kart`] for this touch (only used while the kart is not yet held).
    /// * `early_launch_penalty`: multiplayer start-gate condition (UNRESOLVED, see [`SlingshotInput::release`]).
    pub fn update_touch(
        &mut self,
        dt: f32,
        touch: Option<(f32, f32)>,
        kart_picked: bool,
        cam: &CamFrame,
        game_mode_10: bool,
        early_launch_penalty: bool,
    ) -> SlingshotOutput {
        if !self.enabled {
            return SlingshotOutput::default();
        }
        // 0x147afc: while CPlayer+0x254 > 0 only count it down (floor 0) and return.
        if self.cooldown > 0.0 {
            self.cooldown = (self.cooldown - dt).max(0.0);
            return self.idle_out();
        }
        match touch {
            Some((x, y)) => {
                if !self.touching {
                    if !kart_picked {
                        return self.idle_out();
                    }
                    // 0x148404..0x148428: anchor points + CPlayer+0x250 = touch y, SetUserTouchingSlingshot(1)
                    self.touching = true;
                    self.anchor_y = y;
                }
                self.pull(x, y, cam, game_mode_10)
            }
            None => self.release(early_launch_penalty),
        }
    }

    /// Pad / keyboard path (the digital stick path of the original: `CPlayer+0x220 == 0`, virtual cursor).
    ///
    /// * `ax`, `ay`: stick axes (-1..1) as in 0x147c9c..0x147cfc: `ax = axis_x + key_right - key_left`,
    ///   `ay = axis_y - key_down + key_up` (key values are 0/1 for a keyboard = 255/255).
    /// * `fire`: `PadKeyPressed(pad, 4)` or `(pad, 0x12)` (edge).
    ///
    /// The cursor is `x = clamp(halfW * (1 + ax), W/8, 0.75 W)`, `y = clamp(halfH * (1 - ay), halfH, H)`; the anchor is `halfH`
    /// (so only pulling DOWN, `ay < 0`, builds a pull; a held key jumps straight to the full value because the original
    /// reads a digital key as 1.0). `fire` releases (launch if `linear_ratio > 0.27`). Game mode 8 auto launch is not ported.
    pub fn update_pad(
        &mut self,
        dt: f32,
        ax: f32,
        ay: f32,
        any_dir_key_down: bool,
        fire: bool,
        cam: &CamFrame,
        game_mode_10: bool,
        early_launch_penalty: bool,
    ) -> SlingshotOutput {
        if !self.enabled {
            return SlingshotOutput::default();
        }
        if self.cooldown > 0.0 {
            self.cooldown = (self.cooldown - dt).max(0.0);
            return self.idle_out();
        }
        let half_w = self.screen_w_px * 0.5;
        let half_h = self.screen_h_px * 0.5;
        let deflected = ax.abs() > STICK_DEADZONE || ay.abs() > STICK_DEADZONE;
        // 0x147d4c / 0x148450: a direction input while idle (0x220 == 1) re-initialises the cursor and starts the pull.
        if (any_dir_key_down || deflected) && !self.pad_active {
            self.cursor = (half_w, half_h);
            self.pad_active = true;
            self.anchor_y = half_h;
            self.touching = true;
        }
        if self.pad_active && fire {
            // 0x147dd0: CPlayer+0x220 = 1 then the release branch (no touches)
            return self.release(early_launch_penalty);
        }
        if !self.pad_active {
            return self.idle_out();
        }
        // 0x148b8c: cursor from the stick
        let x = (half_w + ax * half_w).max(self.screen_w_px * 0.25 * 0.5).min(self.screen_w_px * 3.0 * 0.25);
        let y = (half_h - ay * half_h).max(half_h).min(self.screen_h_px);
        self.cursor = (x, y);
        self.pull(x, y, cam, game_mode_10)
    }

    /// Convenience API: a drag of `drag_dy_px` pixels (down = positive) from the pick point at the horizontal screen centre.
    /// `released` = finger lifted. The kart is assumed picked when the drag starts. Pull ratio output feeds
    /// `carsim::LaunchParams.player_pull_ratio` (`Some(out.pull_ratio)`) on the frame `out.launch` is true.
    pub fn update(&mut self, dt: f32, drag_dy_px: f32, screen_h_px: f32, released: bool, cam: &CamFrame) -> SlingshotOutput {
        self.screen_h_px = screen_h_px;
        if released {
            return self.update_touch(dt, None, false, cam, false, false);
        }
        let y0 = if self.touching { self.anchor_y } else { screen_h_px * 0.5 };
        self.update_touch(dt, Some((self.screen_w_px * 0.5, y0 + drag_dy_px)), true, cam, false, false)
    }
}

// =============================================================================================================
// Tests
// =============================================================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn curve_table_values_from_binary() {
        let x = [0.17, 0.23, 0.28, 0.32, 0.37, 0.46, 0.71, 0.96];
        let y = [0.13, 0.18, 0.24, 0.30, 0.40, 0.50, 0.70, 1.00];
        for i in 0..8 {
            assert!(close(PULL_CURVE[i].0, x[i]), "x{}", i);
            assert!(close(PULL_CURVE[i].1, y[i]), "y{}", i);
        }
        assert!(close(LAUNCH_MIN_RATIO, 0.27));
        assert!(close(RATIO_FULL_DRAG, 0.2));
        assert!(close(PULL_LENGTH, 8.4));
        assert!(close(PITCH_WEIGHT_MODE10, 0.33));
        assert!(close(EARLY_LAUNCH_COOLDOWN, 0.65));
        assert!((COS_22_5 - (std::f64::consts::PI / 8.0).cos()).abs() < 1e-7);
        assert!((SIN_22_5 - (std::f64::consts::PI / 8.0).sin()).abs() < 1e-7);
    }

    #[test]
    fn curve_shape() {
        assert!(close(pull_curve(-1.0), 0.13));
        assert!(close(pull_curve(0.0), 0.13));
        assert!(close(pull_curve(0.16), 0.13));
        assert!(close(pull_curve(0.17), 0.13));
        assert!(close(pull_curve(0.20), 0.155)); // halfway 0.13..0.18
        assert!(close(pull_curve(0.23), 0.18));
        assert!(close(pull_curve(0.46), 0.50));
        assert!(close(pull_curve(0.585), 0.60)); // halfway 0.50..0.70
        assert!(close(pull_curve(0.96), 1.0));
        assert!(close(pull_curve(1.0), 1.0));
        assert!(close(pull_curve(5.0), 1.0));
        // monotonic
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = pull_curve(i as f32 / 100.0);
            assert!(v >= prev - 1e-6);
            prev = v;
        }
    }

    #[test]
    fn ratio_and_launch_threshold() {
        assert!(close(linear_ratio(-0.1), 0.0));
        assert!(close(linear_ratio(0.1), 0.5));
        assert!(close(linear_ratio(0.3), 1.0));
        let cam = CamFrame { pos: Vec3::ZERO, target: Vec3::Z, up: Vec3::Y };
        // 1080 px high: dy = 0.05 -> ratio 0.25 (<= 0.27, no launch); dy = 0.06 -> 0.30 (launch)
        let mut s = SlingshotInput::new(1920.0, 1080.0);
        s.update(0.016, 0.0, 1080.0, false, &cam);
        s.update(0.016, 54.0, 1080.0, false, &cam);
        let o = s.update(0.016, 54.0, 1080.0, true, &cam);
        assert!(!o.launch);
        let mut s = SlingshotInput::new(1920.0, 1080.0);
        s.update(0.016, 0.0, 1080.0, false, &cam);
        let held = s.update(0.016, 64.8, 1080.0, false, &cam);
        assert!(close(held.linear_ratio, 0.3));
        let o = s.update(0.016, 64.8, 1080.0, true, &cam);
        assert!(o.launch);
        assert!(close(o.pull_ratio, pull_curve(0.12)));
        assert!(!s.is_touching());
    }

    #[test]
    fn offset_in_camera_frame() {
        let cam = CamFrame { pos: Vec3::ZERO, target: Vec3::Z, up: Vec3::Y };
        // full pull straight down the middle: dy = 0.2 -> ratio 1, pitch term 0 => (0, -1, -8.4)
        let o = slingshot_offset(1.0, 0.2, 0.5, &cam, false);
        assert!((o - Vec3::new(0.0, -1.0, -8.4)).length() < 1e-4, "{:?}", o);
        // pull the finger to the right edge of the screen: sideways component is positive along r = up x f = +x
        let o = slingshot_offset(1.0, 0.2, 1.0, &cam, false);
        assert!(o.x > 0.0 && o.z < 0.0);
        assert!((o.x as f64 - 8.4 * SIN_22_5).abs() < 1e-3);
        // finger past the screen edge uses the constant 22.5 degree
        let o2 = slingshot_offset(1.0, 0.2, 2.0, &cam, false);
        assert!((o2 - o).length() < 1e-5);
    }

    #[test]
    fn kart_pick_sphere() {
        let node = Vec3::new(0.0, 0.0, 10.0);
        assert!(ray_hits_kart(Vec3::ZERO, Vec3::Z, node));
        assert!(!ray_hits_kart(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), node));
        assert!(!ray_hits_kart(Vec3::new(0.0, 0.0, 20.0), Vec3::Z, node)); // sphere behind the ray
        assert!(ray_hits_kart(node, Vec3::Z, node)); // inside
    }

    #[test]
    fn steering_push_button_ramp() {
        let mut p = PlayerSteering::new(SteerTweakables::default());
        let dt = 1.0 / 60.0;
        let (s, b) = p.update(dt, None, SteerKeys { right: true, ..Default::default() });
        // v = 0 + 1 * dt * 12 = 0.2, then ^1.1
        assert!(close(p.pad_steer(), 0.2));
        assert!(close(s, 0.2f32.powf(1.1)));
        assert_eq!(b, 0.0);
        // reaches 1.0 and stays
        for _ in 0..120 {
            p.update(dt, None, SteerKeys { right: true, ..Default::default() });
        }
        assert!(p.pad_steer() <= 1.0 && p.pad_steer() > 0.999);
        // release: decays with the centre rate 16
        let v0 = p.pad_steer();
        p.update(dt, None, SteerKeys::default());
        assert!(close(p.pad_steer(), v0 + (0.0 - v0) * dt * 16.0));
        // left and right together cancel
        let mut q = PlayerSteering::new(SteerTweakables::default());
        q.update(dt, None, SteerKeys { left: true, right: true, brake: false });
        assert!(close(q.pad_steer(), 0.0));
        // left: negative with the signed power
        let (s, _) = q.update(dt, None, SteerKeys { left: true, ..Default::default() });
        assert!(s < 0.0 && close(s, -(0.2f32.powf(1.1))));
    }

    #[test]
    fn steering_drag_mode() {
        let mut p = PlayerSteering::new(SteerTweakables::default());
        p.screen_w_px = 1280.0;
        // sw = 1280/1024 = 1.25; denom = 1.25 * 1.25 * 257
        let denom = 1.25f32 * 1.25 * 257.0;
        let (s, b) = p.update(0.016, Some(100.0), SteerKeys::default());
        let want = -(100.0 / denom);
        assert!(close(s, -((-want).powf(1.1))), "{} vs {}", s, want);
        assert_eq!(b, 0.0);
        // saturates
        let (s, _) = p.update(0.016, Some(-2000.0), SteerKeys::default());
        assert!(close(s, 1.0));
        // two touches brake (frame API)
        let mut p = PlayerSteering::new(SteerTweakables { push_button_controls: false, ..Default::default() });
        let t = [SteerTouch { x: 600.0, over_ui: false }, SteerTouch { x: 700.0, over_ui: false }];
        let f = SteerFrame { dt: 0.016, screen_w_px: 1280.0, touches: &t, key_left: false, key_right: false, axis_x: 0.0, ctx: SteerContext::default() };
        let (s, b) = p.update_frame(&f);
        assert_eq!(b, 1.0);
        assert!(close(s, -((((650.0f32 - 640.0) / denom).abs()).powf(1.1)) * (650.0f32 - 640.0).signum()));
    }

    #[test]
    fn steering_gates() {
        let mut p = PlayerSteering::new(SteerTweakables::default());
        let f = SteerFrame {
            dt: 0.016,
            screen_w_px: 1280.0,
            touches: &[],
            key_left: false,
            key_right: true,
            axis_x: 0.0,
            ctx: SteerContext { input_enabled: false, ..Default::default() },
        };
        assert_eq!(p.update_frame(&f), (0.0, 1.0));
        assert_eq!(PlayerSteering::process_override(), (0.0, 1.0));
        let f = SteerFrame { ctx: SteerContext { min_brake_flag: true, ..Default::default() }, ..f };
        let (_, b) = p.update_frame(&f);
        assert!(close(b, 0.025));
    }
}
