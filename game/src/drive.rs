//! Free driving on a flat baseplate with the original kart model and `CarSpec`.
//!
//! The tracks are downloaded content that is not in the APK, so this is the playable stand-in: a flat plate and one kart.
//! NOTE: the vehicle dynamics (`kart.rs`) are still my own placeholder model parameterised by the original spec; they are
//! to be replaced by the game's own code ported from the `libABK.so` decompile (see PORT_STATUS.md).
use crate::camera::{CamCar, CamMode, ChaseCam};
use crate::gfx::Draw;
use crate::models;
use crate::playerinput::{CamFrame, PlayerSteering, SlingshotInput, SteerKeys, SteerTweakables};
use crate::carsim::{CModSpec, CarInput, CarSim, CarSpec, Ground};
use crate::trackworld::TrackWorld;
use crate::race::{KartMeshes, Keys, PLAYER_SLOT, ROSTER};
use crate::render3d::{Camera, Draw3d, MeshData, MeshId, Scene3d, Vertex3, World};
use crate::ui::{solid, Textures, STAGE_H, STAGE_W};
use glam::{Mat4, Vec3};
use std::path::Path;

pub const PLATE_HALF_SIZE: f32 = 600.0;
const TILE: f32 = 10.0;

pub struct DriveAssets {
    pub kart: KartMeshes,
    pub plate: MeshId,
    /// `slingshot.xgm` (v1.0.1 envobjects): the launcher the kart starts in.
    pub sling: Option<MeshId>,
    /// The kart's spec as the original builds it: `SetDefault`, the chassis block, then the 2.9.1 per-car file.
    pub car_spec: CarSpec,
    /// Every kart folder of `pak/cargeom` (64 chassis variants), and which one is loaded.
    pub folders: Vec<String>,
    pub index: usize,
    pub color: [f32; 3],
    /// The textured 2.9.2 kart with its driver (used whenever assets292 has the folder).
    pub full: Option<crate::fullkart::FullKart>,
    textures: Option<crate::xmodel::TextureIndex>,
    /// The `<Camera>` block of `debugtweakables.xml`.
    pub cam_tweaks: crate::camera::CamTweaks,
}

fn load_full(scene: &mut Scene3d, textures: &mut Option<crate::xmodel::TextureIndex>, assets: &Path, folder: &str) -> Option<crate::fullkart::FullKart> {
    let root = assets.parent()?.join("assets292");
    let tx = textures.get_or_insert_with(|| crate::xmodel::TextureIndex::new(&root));
    match crate::fullkart::FullKart::load(scene, tx, &root, folder, 2) {
        Ok(k) => Some(k),
        Err(e) => {
            eprintln!("full kart: {e}");
            None
        }
    }
}

impl DriveAssets {
    pub fn load(scene: &mut Scene3d, assets: &Path) -> Result<DriveAssets, String> {
        let mut folders: Vec<String> = std::fs::read_dir(assets.join("pak/cargeom"))
            .map_err(|e| e.to_string())?
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| n.starts_with("kart_") && n != "kart_base" && !n.contains("test"))
            .collect();
        folders.sort();
        let index = folders.iter().position(|f| f == ROSTER[PLAYER_SLOT].folder).unwrap_or(0);
        let kart = KartMeshes::load(scene, assets, &folders[index])?;
        let plate = scene.add_mesh(&baseplate());
        let sling = models::load_xgm(&assets.join("pak/envobjects/slingshot.xgm")).ok().map(|m| scene.add_mesh(&m.mesh));
        let car_spec = build_car_spec(assets, &folders[index])?;
        let color = kart_color(&folders[index]);
        let mut textures = None;
        let full = load_full(scene, &mut textures, assets, &folders[index]);
        let cam_tweaks = assets.parent().map(|p| crate::camera::CamTweaks::load(&p.join("assets292"))).unwrap_or_default();
        Ok(DriveAssets { kart, plate, sling, car_spec, folders, index, color, full, textures, cam_tweaks })
    }

    /// Loads the previous (-1) or next (+1) kart of the garage.
    pub fn cycle(&mut self, scene: &mut Scene3d, assets: &Path, step: i32) {
        let n = self.folders.len() as i32;
        let index = (self.index as i32 + step).rem_euclid(n) as usize;
        let folder = self.folders[index].clone();
        match KartMeshes::load(scene, assets, &folder) {
            Ok(kart) => {
                self.kart = kart;
                self.car_spec = build_car_spec(assets, &folder).unwrap_or_else(|_| self.car_spec.clone());
                self.color = kart_color(&folder);
                self.index = index;
                self.full = load_full(scene, &mut self.textures, assets, &folder);
            }
            Err(e) => eprintln!("kart {folder}: {e}"),
        }
    }

    pub fn name(&self) -> String {
        self.folders[self.index].trim_start_matches("kart_").replace("_upgrade", " L").to_uppercase()
    }
}

/// Paint colour per character (the kart textures were downloaded content; this stands in for them).
fn kart_color(folder: &str) -> [f32; 3] {
    let table: [(&str, [f32; 3]); 13] = [
        ("red", [0.9, 0.12, 0.08]),
        ("bluebird", [0.28, 0.52, 0.98]),
        ("pink", [0.98, 0.52, 0.78]),
        ("black", [0.16, 0.16, 0.2]),
        ("yellowrocket", [1.0, 0.85, 0.15]),
        ("helmetpig", [0.5, 0.72, 0.3]),
        ("green", [0.3, 0.75, 0.25]),
        ("orange", [1.0, 0.55, 0.1]),
        ("white", [0.92, 0.92, 0.95]),
        ("kingpig", [0.6, 0.8, 0.35]),
        ("moustache", [0.45, 0.65, 0.3]),
        ("terrence", [0.7, 0.2, 0.2]),
        ("partner", [0.6, 0.6, 0.9]),
    ];
    table.iter().find(|(k, _)| folder.starts_with(&format!("kart_{k}_"))).map(|(_, c)| *c).unwrap_or([0.7, 0.7, 0.7])
}

/// A flat square plate made of 10 m tiles in two grey tones.
fn baseplate() -> MeshData {
    let mut m = MeshData::default();
    let n = (PLATE_HALF_SIZE * 2.0 / TILE) as i32;
    for iz in 0..n {
        for ix in 0..n {
            let (x, z) = (-PLATE_HALF_SIZE + ix as f32 * TILE, -PLATE_HALF_SIZE + iz as f32 * TILE);
            let shade = if (ix + iz) % 2 == 0 { 0.46 } else { 0.40 };
            let color = [shade, shade + 0.01, shade + 0.03, 1.0];
            let base = m.vertices.len() as u32;
            for (dx, dz) in [(0.0, 0.0), (TILE, 0.0), (TILE, TILE), (0.0, TILE)] {
                m.vertices.push(Vertex3 { pos: [x + dx, 0.0, z + dz], normal: [0.0, 1.0, 0.0], color });
            }
            m.indices.extend([base, base + 3, base + 2, base, base + 2, base + 1]);
        }
    }
    m
}

/// `CarSpec::SetDefault` + the chassis block (`kart_base.xml`, v1.0.1: the 2.9.1 base spec lives in the cloud-only `carSpec.pak`)
/// + the 2.9.1 per-car file at its best upgrade (`kart_<name>_max.xml`).
fn build_car_spec(assets: &Path, folder: &str) -> Result<CarSpec, String> {
    build_car_spec_with(assets, folder, None)
}

/// `mods` = the player's upgrade ratios (`meta::mod_spec`); `None` = kart level zero from `kartupgradelevels.xml` (the old stand-in).
pub fn build_car_spec_with(assets: &Path, folder: &str, mods: Option<CModSpec>) -> Result<CarSpec, String> {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()));
    // CGame::LoadCarSpecs @ 0011302c: `CCarSpec::CCarSpec(path)` = SetDefault + Read of "CARSPEC:<car>.xml" (the full per-car file of the
    // 2.9.2 data: mass, inertia, drag, downforce, wheels ...), then the `_min.xml` / `_max.xml` pair of "CARXML:".
    let top = assets.parent().ok_or("no parent folder")?;
    let spec_dir = top.join("assets292/xml/cars/carspec");
    let mut spec = CarSpec::set_default();
    spec.read_xml(&read(spec_dir.join(format!("{folder}.xml"))).or_else(|_| read(spec_dir.join("kart_base.xml")))?);
    // kart_red_upgrade1 -> kart_red_max.xml
    let name = folder.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches("_upgrade");
    let root = top.join("assets292/xml/cars/carxml");
    let mut min = CarSpec::set_default();
    min.read_xml(&read(root.join(format!("{name}_min.xml"))).or_else(|_| read(root.join("kart_base_min.xml")))?);
    let mut max = CarSpec::set_default();
    max.read_xml(&read(root.join(format!("{name}_max.xml"))).or_else(|_| read(root.join("kart_base_max.xml")))?);
    let levels = read(top.join("assets292/xml/xml/global/kartupgradelevels.xml"))?;
    let mods = mods.unwrap_or_else(|| mods_at_level_zero(&levels, "SSKM"));
    let spec = CarSpec::copy_with_mods(&spec, &min, &max, &mods);
    if std::env::var_os("ABG_DEBUG_CAR").is_some() {
        eprintln!("spec: sliding {} thrust {} drag {} minspeed {} grip {} mass {} mods {:?}", spec.m_fSlidingTargetAngularVel, spec.m_fBelowMinSpeedThrust, spec.m_fDrag, spec.m_fMinDesiredSpeed, spec.wheels[0].fPeakGrip, spec.m_fMass, mods);
    }
    Ok(spec)
}

/// The `CModSpec` ratios of a kart with no upgrades, from `kartupgradelevels.xml` (stat entry `Index="0"` of the first tier).
/// The real setter is the CGame function that builds the player `CModSpec` (the one that calls `CKartManager::GetKartStat` five times, right after `CGame::SetPlayerCarSpecSimple @ 0011a310` in libABK291_annotated.c, ~line 88713): `CModSpec+0x10 = GetKartStat(4)`, `+0x14 = (2)`, `+0x18 = (1)`, `+0x1c = (0)`, `+0x20 = (3)` for the stat levels of the
/// kart (`+0x24` is never written, the ctor zeroes it: with equal min and max `m_fMinDesiredSpeed` it does not matter). `CCarSpec::CopyWithMods` reads
/// +0x10 grip, +0x14 fragility, +0x18 thrust, +0x1c drag, +0x20 sliding velocity, so the enum is TopSpeed 0, Acceleration 1, Strength 2, Handling 3, Grip 4,
/// the order of the XML stat blocks (inferred from that consistency; the stat-name string table of `TKartStatLevels::Parse` was not read).
// UNRESOLVED: the kart name "SSKM" is hard-coded (the original takes it from `CPlayerInfo`'s selected kart via `CKartManager::GetKartInfoByIndex`); only level 0 is used.
fn mods_at_level_zero(xml: &str, kart: &str) -> CModSpec {
    let Some(start) = xml.find(&format!("<Kart name=\"{kart}\"")) else { return CModSpec { grip: 0.5, fragility: 0.5, thrust: 0.5, drag: 0.5, sliding_ang_vel: 0.5, min_speed: 0.5 } };
    let body = &xml[start..];
    let body = &body[..body.find("</Kart>").unwrap_or(body.len())];
    let stat = |name: &str| -> f32 {
        let key = format!("Index=\"0\" Stat=\"{name}\"");
        let Some(at) = body.find(&key) else { return 0.5 };
        let rest = &body[at..];
        let m = rest.find("Modifier=\"").map(|i| i + 10).unwrap_or(0);
        rest[m..].split('"').next().and_then(|v| v.parse().ok()).unwrap_or(0.5)
    };
    CModSpec { grip: stat("Grip"), fragility: stat("Strength"), thrust: stat("Acceleration"), drag: stat("TopSpeed"), sliding_ang_vel: stat("Handling"), min_speed: 0.0 }
}

/// Slingshot pull limit: `GetLaunchSpeedScale` divides the pull by 8.4 m.
pub const MAX_PULL: f32 = 8.4;
/// Where the kart sits in the slingshot.
const START: Vec3 = Vec3::new(0.0, 0.0, 0.0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    /// In the slingshot: hold the pull key, release to launch.
    Slingshot,
    Driving,
}

pub struct Drive {
    pub car: CarSim,
    pub phase: Phase,
    pub pull: f32,
    /// The race camera (`CCamera`, see `camera.rs`).
    pub cam: ChaseCam,
    /// Camera inputs the owner keeps current: `CCar::GetImpactCamShakeMod`, the SpeedBooster `GetCamBehindMod` branch and the up
    /// vector of the race spline at the car (`CSpline::GetUpVectorInterpolated`, `None` = no upside-down correction).
    pub cam_impact_shake: f32,
    pub cam_behind_boost: Option<f32>,
    pub track_up: Option<Vec3>,
    wheel_spin: f32,
    ground_y: f32,
    /// Spawn pose (position on the ground, yaw) and whether a real track is loaded.
    spawn: (Vec3, f32),
    on_track: bool,
    steering: PlayerSteering,
    sling: SlingshotInput,
    /// Mouse / touch position on the stage while the button is down.
    pub touch: Option<(f32, f32)>,
    pull_offset_now: Vec3,
    fire_prev: bool,
    dbg_t: f32,
    /// Capture hook: camera offset in kart-local axes (x left, y up, z forward).
    pub debug_cam_rel: Option<Vec3>,
    /// Test aid: analogue steering (car-frame sign, as `CarInput::steer`) that replaces the key input.
    pub auto_steer: Option<f32>,
    pub auto_brake: Option<f32>,
    /// Direction of the race spline at the car (`CarInput::track_direction`), set by the track owner each frame.
    pub track_dir: Option<Vec3>,
    /// An item boost pad overlaps the kart this frame (`CarInput::pad_boost`).
    pub pad_boost: bool,
    /// Speed-boost power-up active (`CarInput::boost`).
    pub boost: bool,
    /// Ability steering multiplier (`SetSteeringMultiplier`, 1.0 = normal).
    pub steer_mul: f32,
    hubs: [Vec3; 4],
}

fn wrap_angle(a: f32) -> f32 {
    let mut a = a;
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

impl Drive {
    pub fn new(assets: &DriveAssets, spawn: Option<(Vec3, f32)>, ground: Option<&dyn Ground>) -> Drive {
        Self::new_with_spec(assets, assets.car_spec.clone(), spawn, ground)
    }

    /// Same with an explicit car spec (the player's profile mods).
    pub fn new_with_spec(assets: &DriveAssets, spec: CarSpec, spawn: Option<(Vec3, f32)>, ground: Option<&dyn Ground>) -> Drive {
        let on_track = spawn.is_some();
        let spawn = spawn.unwrap_or((START, 0.0));
        let mut d = Drive {
            car: CarSim::new_on_ground(spec, &Self::hubs(assets), spawn.0.x, spawn.0.z, spawn.0.y, spawn.1),
            spawn,
            on_track,
            phase: Phase::Slingshot,
            pull: 0.0,
            cam: ChaseCam::new(assets.cam_tweaks.clone()),
            cam_impact_shake: 0.0,
            cam_behind_boost: None,
            track_up: None,
            wheel_spin: 0.0,
            ground_y: 0.0,
            steering: PlayerSteering::new(SteerTweakables::default()),
            sling: SlingshotInput::new(STAGE_W, STAGE_H),
            touch: None,
            pull_offset_now: Vec3::ZERO,
            fire_prev: false,
            dbg_t: 0.0,
            debug_cam_rel: None,
            auto_steer: None,
            auto_brake: None,
            track_dir: None,
            pad_boost: false,
            boost: false,
            steer_mul: 1.0,
            hubs: Self::hubs(assets),
        };
        // on a real track the kart sits on the slope: height and orientation from the ground under the start line
        if let Some(g) = ground {
            snap_car_to_ground(&mut d.car, d.spawn, g);
        }
        d.update_camera(0.0, ground);
        if std::env::var_os("ABG_DEBUG_CAR").is_some() {
            eprintln!("spawn {:?} yaw {} forward {:?}", d.spawn, d.spawn.1, d.car.forward());
        }
        d
    }

    /// Hub positions in the car frame, order FL, FR, RR, RL (what `CCarModel::GetWheelPos` returns).
    fn hubs(assets: &DriveAssets) -> [Vec3; 4] {
        let (f, r) = (assets.kart.front_hub, assets.kart.rear_hub);
        [f, Vec3::new(-f.x, f.y, f.z), Vec3::new(-r.x, r.y, r.z), r]
    }

    fn make_car(assets: &DriveAssets, spawn: (Vec3, f32)) -> CarSim {
        CarSim::new_on_ground(assets.car_spec.clone(), &Self::hubs(assets), spawn.0.x, spawn.0.z, spawn.0.y, spawn.1)
    }

    pub fn reset(&mut self, assets: &DriveAssets, ground: Option<&dyn Ground>) {
        let spawn = if self.on_track { Some(self.spawn) } else { None };
        *self = Drive::new(assets, spawn, ground);
    }

    pub fn speed(&self) -> f32 {
        self.car.speed()
    }

    pub fn pos(&self) -> Vec3 {
        self.car.rb.pos
    }

    fn yaw(&self) -> f32 {
        let f = self.car.forward();
        f.x.atan2(f.z)
    }

    /// The pull offset the original keeps in `CCar+0x46c`: from the slingshot to the kart, i.e. backwards.
    fn pull_offset(&self) -> Vec3 {
        if self.pull_offset_now == Vec3::ZERO {
            -self.car.forward() * self.pull
        } else {
            self.pull_offset_now
        }
    }

    /// Screenshot hook: pull `fraction` of the way back and let go.
    pub fn launch_with(&mut self, fraction: f32) {
        self.pull = fraction.clamp(0.0, 1.0) * MAX_PULL;
        let v = self.car.slingshot_release(self.pull_offset(), self.slingshot_cam_forward());
        self.car.rb.vel = v;
        self.phase = Phase::Driving;
        self.pull = 0.0;
    }

    pub fn update(&mut self, dt: f32, keys: &Keys, ground: &dyn Ground) {
        let dt = dt.min(0.1);
        match self.phase {
            Phase::Slingshot => {
                let cam = self.cam_frame();
                // a mouse drag pulls the kart back (the touch path of UpdateSlingshotLaunch); without one, the pad path: Down pulls, Space fires
                let out = match self.touch {
                    Some(t) => self.sling.update_touch(dt, Some(t), true, &cam, false, false),
                    None => {
                        // keyboard: hold Down (or Space) to pull the slingshot back, let go of Space to fire
                        // (digital keys read as 1.0 in the original, so the pull is full)
                        let fire = !keys.handbrake && self.fire_prev;
                        let pulling = keys.down || keys.handbrake;
                        let any = pulling || keys.left || keys.right;
                        let ax = keys.right as i8 as f32 - keys.left as i8 as f32;
                        let ay = keys.up as i8 as f32 - pulling as i8 as f32;
                        self.sling.update_pad(dt, ax, ay, any, fire, &cam, false, false)
                    }
                };
                let out = if self.touch.is_none() && out.pull_offset_dir.is_none() && self.pull_offset_now != Vec3::ZERO && !out.launch {
                    // finger lifted this frame: the release branch decides
                    out
                } else {
                    out
                };
                self.pull = out.linear_ratio * MAX_PULL;
                if let Some(o) = out.pull_offset_dir {
                    self.pull_offset_now = o;
                }
                if out.launch {
                    self.car.launch.player_pull_ratio = Some(out.pull_ratio);
                    let v = self.car.slingshot_release(self.pull_offset(), self.slingshot_cam_forward());
                    self.car.rb.vel = v;
                    self.phase = Phase::Driving;
                    self.pull = 0.0;
                    self.pull_offset_now = Vec3::ZERO;
                }
                self.fire_prev = keys.handbrake;
            }
            Phase::Driving => {
                // CPlayer::ProcessInput: push-button steering (pad keys 2 / 3) ramps the value; the brake key is my analogue of the two-finger brake
                let (steer, brake) = self.steering.update(dt, None, SteerKeys { left: keys.left, right: keys.right, brake: keys.down });
                // the car frame has +x on the left (the mesh convention) while the original steer sign is +1 = right
                let input = CarInput {
                    // `auto_steer` = the test-aid autopilot, fed like an AI car (CCar::SetSteering with an analogue value)
                    steer: (self.auto_steer.unwrap_or(-steer) * self.steer_mul).clamp(-1.0, 1.0),
                    brake: self.auto_brake.unwrap_or(brake),
                    track_direction: self.track_dir,
                    pad_boost: self.pad_boost,
                    boost: self.boost,
                    ..Default::default()
                };
                self.car.step(dt, &input, ground);
                self.wheel_spin += self.car.forward_speed() * dt / 0.3;
                // the plate is finite: stop the kart at its edge
                let limit = if self.on_track { f32::MAX } else { PLATE_HALF_SIZE - 2.0 };
                for (p, v) in [(&mut self.car.rb.pos.x, &mut self.car.rb.vel.x), (&mut self.car.rb.pos.z, &mut self.car.rb.vel.z)] {
                    if p.abs() > limit {
                        *p = p.signum() * limit;
                        *v = 0.0;
                    }
                }
                if std::env::var_os("ABG_DEBUG_CAR").is_some() {
                    self.dbg_t += dt;
                    if self.dbg_t > 0.25 {
                        self.dbg_t = 0.0;
                        println!("wind={} pos=({:.0},{:.1},{:.0}) ground_y={:.1} v={:.1} up.y={:.2} steer_angle={:.3} yaw_target={:.2} wheels={} angvel={:?}", self.car.rb.wind.y, self.car.rb.pos.x, self.car.rb.pos.y, self.car.rb.pos.z, ground.height_normal(self.car.rb.pos).0, self.car.speed(), self.car.up().y, self.car.steer_angle(), self.car.yaw_target, self.car.wheels_on_ground(), self.car.rb.ang_vel);
                    }
                }
            }
        }
        self.update_camera(dt, Some(ground));
    }

    /// The car state `CCamera` reads (`CCar::GetCamTargetPosition / Velocity`, `GetCamHeightMod` ...).
    fn cam_car(&self) -> CamCar {
        let (x, up, fwd) = self.car.rb.axes();
        CamCar {
            pos: self.car.rb.pos,
            vel: if self.phase == Phase::Slingshot { Vec3::ZERO } else { self.car.rb.vel },
            rot: self.car.rb.rot,
            speed: self.car.speed(),
            com_z: self.car.spec.m_vCOMOffset.z,
            wheels_on_ground: self.car.wheels_on_ground(),
            accel: 0.0,
            behind_boost: self.cam_behind_boost,
            impact_shake: self.cam_impact_shake,
            track_up: self.track_up,
            sling_pull: (self.pull / MAX_PULL).clamp(0.0, 1.0),
            sling_frame: Some([x, up, -fwd]),
        }
    }

    /// `CCamera::SetCameraType` + `Process`: the slingshot camera while the kart is in the slingshot, then the rear camera.
    fn update_camera(&mut self, dt: f32, ground: Option<&dyn Ground>) {
        self.cam.set_mode(if self.phase == Phase::Slingshot { CamMode::Slingshot } else { CamMode::Rear });
        let car = self.cam_car();
        self.cam.update(dt, &car, ground);
    }

    /// The camera as a `CamFrame` (the slingshot touch ray is cast through it).
    fn cam_frame(&self) -> CamFrame {
        CamFrame { pos: self.cam.eye, target: self.cam.look, up: self.cam.up }
    }

    pub fn world(&self, assets: &DriveAssets, track: Option<&TrackWorld>, aspect: f32) -> World {
        let pos = self.pos();
        let (cam_eye, cam_look, cam_up, fov) = self.cam.view();
        let (eye, target, up) = match self.debug_cam_rel {
            Some(rel) => (pos + self.car.rb.rot * Vec3::new(rel.x, rel.y, rel.z), pos + Vec3::Y * 0.4, Vec3::Y),
            None => (cam_eye, cam_look, cam_up),
        };
        let view = Mat4::look_at_rh(eye, target, up);
        let proj = Mat4::perspective_rh(fov, aspect, 0.1, 3000.0);
        let mut draws = match track {
            Some(t) => t.draws.clone(),
            None => vec![Draw3d { mesh: assets.plate, model: Mat4::IDENTITY, tint: [1.0, 1.0, 1.0, 1.0] }],
        };
        if let Some(sling) = assets.sling {
            draws.push(Draw3d { mesh: sling, model: Mat4::from_translation(self.spawn.0 + Vec3::new(self.spawn.1.sin(), 0.0, self.spawn.1.cos()) * 2.0) * Mat4::from_rotation_y(self.spawn.1) * Mat4::from_scale(Vec3::splat(0.35)), tint: [0.55, 0.38, 0.2, 1.0] });
        }
        let back = if self.phase == Phase::Slingshot { self.pull_offset() } else { Vec3::ZERO };
        let body = Mat4::from_translation(pos + back) * Mat4::from_quat(self.car.rb.rot);
        match &assets.full {
            Some(f) => f.draws(&mut draws, body, self.car.steer_angle(), self.wheel_spin),
            None => assets.kart.draws(&mut draws, body, assets.color, self.car.steer_angle(), self.wheel_spin),
        }
        World { camera: Camera { view_proj: proj * view, eye }, draws, sky: track.map(|t| t.sky).unwrap_or([0.62, 0.74, 0.88]), fog_density: if track.is_some() { 0.0006 } else { 0.0011 }, sun_dir: Vec3::new(-0.45, -0.8, -0.35), overlay: false }
    }

    pub fn engine_sound(&self) -> (f32, f32) {
        let speed = self.speed().abs();
        ((0.25 + speed / 35.0).min(0.9), (0.7 + speed / 40.0).min(2.2))
    }

    pub fn hud(&self, tx: &Textures) -> Vec<Draw> {
        self.hud_help(tx, None)
    }

    /// `help` replaces the key-help strip (an event shows its own).
    pub fn hud_help(&self, tx: &Textures, help_override: Option<&str>) -> Vec<Draw> {
        let mut out = Vec::new();
        let kmh = (self.speed().abs() * 3.6).round() as i32;
        let shadow = |font: &crate::font::Font, text: &str, cx: f32, cy: f32, scale: f32, color: [f32; 4], out: &mut Vec<Draw>| {
            font.draw_centered(text, cx + 2.5 * scale, cy + 2.5 * scale, scale, [0.0, 0.0, 0.0, 0.65], out);
            font.draw_centered(text, cx, cy, scale, color, out);
        };
        shadow(&tx.title_font, &format!("{kmh}"), STAGE_W - 190.0, STAGE_H - 100.0, 1.35, [1.0, 1.0, 1.0, 1.0], &mut out);
        shadow(&tx.body_font, "KM/H", STAGE_W - 190.0, STAGE_H - 36.0, 0.85, [1.0, 0.85, 0.3, 1.0], &mut out);
        out.push(solid(tx.white, [0.0, 0.0, STAGE_W, 54.0], [0.0, 0.0, 0.0], 0.35));
        let help = match self.phase {
            Phase::Slingshot => "DRAG DOWN WITH THE MOUSE (OR HOLD DOWN) TO PULL BACK, RELEASE (OR SPACE) TO LAUNCH    ESC BACK",
            Phase::Driving => "LEFT / RIGHT STEER    DOWN BRAKE    R RESET    ESC BACK",
        };
        let help = help_override.unwrap_or(help);
        tx.body_font.draw_centered(help, STAGE_W / 2.0, 27.0, 0.8, [1.0, 1.0, 1.0, 0.95], &mut out);
        if self.phase == Phase::Slingshot {
            let (x, y, w, h) = (STAGE_W / 2.0 - 250.0, STAGE_H - 90.0, 500.0, 28.0);
            out.push(solid(tx.white, [x, y, w, h], [0.0, 0.0, 0.0], 0.5));
            out.push(solid(tx.white, [x + 4.0, y + 4.0, (w - 8.0) * self.pull / MAX_PULL, h - 8.0], [1.0, 0.75, 0.1], 0.95));
        }
        out
    }
}

/// The garage view: the kart turning on the spot, seen from the `selectCamPos` / `selectCamLookAt` of `uikartgaragescreen.xml`
/// (camera (0, 2.3, -7) looking at (-2.45, 1, 0); `kartSpinSpeed` 0.5 rad/s).
pub fn kart_showroom(assets: &DriveAssets, time: f32, aspect: f32) -> World {
    let eye = Vec3::new(0.0, 2.3, -7.0);
    let target = Vec3::new(-2.45, 1.0, 0.0);
    let view = Mat4::look_at_rh(eye, target, Vec3::Y);
    let proj = Mat4::perspective_rh(40.0f32.to_radians(), aspect, 0.5, 500.0);
    let mut draws = Vec::new();
    let body = Mat4::from_rotation_y(time * 0.5);
    match &assets.full {
        Some(f) => f.draws(&mut draws, body, 0.0, 0.0),
        None => assets.kart.draws(&mut draws, body, assets.color, 0.0, 0.0),
    }
    World { camera: Camera { view_proj: proj * view, eye }, draws, sky: [0.5, 0.5, 0.5], fog_density: 0.0, sun_dir: Vec3::new(-0.45, -0.8, -0.35), overlay: true }
}

impl Drive {
    /// The kart's heading on the ground plane.
    pub fn forward_flat(&self) -> Vec3 {
        let f = self.car.forward();
        Vec3::new(f.x, 0.0, f.z).normalize_or_zero()
    }
}

impl Drive {
    /// Forward axis of the slingshot camera used in `update` (camera 6.4 m behind / 2.5 m above the kart, looking 0.9 m above it).
    fn slingshot_cam_forward(&self) -> Vec3 {
        let (_, up, fwd) = self.car.rb.axes();
        ((up * 0.9) - (-fwd * 6.4 + up * 2.5)).normalize_or_zero()
    }
}

/// On a real track the kart sits on the slope: height and orientation from the ground under `spawn`.
pub fn snap_car_to_ground(car: &mut CarSim, spawn: (Vec3, f32), g: &dyn Ground) {
    let (h, n) = g.height_normal(spawn.0 + Vec3::Y * 2.0);
    if h > -1.0e3 {
        let flat = Vec3::new(spawn.1.sin(), 0.0, spawn.1.cos());
        let fwd = (flat - n * flat.dot(n)).normalize_or_zero();
        let x = n.cross(fwd).normalize_or_zero();
        car.rb.rot = glam::Quat::from_mat3(&glam::Mat3::from_cols(x, n, fwd));
        car.rb.pos.y = h + (car.rb.pos.y - spawn.0.y).max(0.3);
    }
}

impl DriveAssets {
    /// Hub positions of a loaded full kart in the car frame, order FL, FR, RR, RL (as `Drive::hubs`).
    pub fn hubs_of(kart: &crate::fullkart::FullKart) -> [Vec3; 4] {
        let (f, r) = (kart.front_hub, kart.rear_hub);
        [f, Vec3::new(-f.x, f.y, f.z), Vec3::new(-r.x, r.y, r.z), r]
    }

    /// Loads the textured kart + driver of `folder` (kept in the shared texture cache) without changing the selected kart.
    pub fn load_other_kart(&mut self, scene: &mut Scene3d, assets: &Path, folder: &str) -> Option<crate::fullkart::FullKart> {
        load_full(scene, &mut self.textures, assets, folder)
    }

    /// Makes `folder` the active kart (model and base spec); false when it does not exist.
    pub fn select_folder(&mut self, scene: &mut Scene3d, assets: &Path, folder: &str) -> bool {
        let Some(index) = self.folders.iter().position(|f| f == folder) else { return false };
        if index != self.index {
            let step = index as i32 - self.index as i32;
            self.cycle(scene, assets, step);
        }
        self.index == index
    }
}

impl DriveAssets {
    /// Loads a textured model of `assets292` (`rel` below `assets292/`, e.g. `pak/envobjects/coin.xgm`) through the shared texture cache.
    pub fn load_xmodel(&mut self, scene: &mut Scene3d, assets: &Path, rel: &str) -> Option<crate::xmodel::XModel> {
        let root = assets.parent()?.join("assets292");
        let tx = self.textures.get_or_insert_with(|| crate::xmodel::TextureIndex::new(&root));
        crate::xmodel::XModel::load(scene, tx, &root.join(rel), true).ok()
    }
}

impl Drive {
    /// `CCar::Respawn(-1)` analogue: a fresh car of the same spec on `pose` (position on the ground, yaw), already driving at `speed` m/s.
    pub fn respawn_on(&mut self, pose: (Vec3, f32), ground: &dyn Ground, speed: f32) {
        let spec = self.car.spec.clone();
        let mut car = CarSim::new_on_ground(spec, &self.hubs, pose.0.x, pose.0.z, pose.0.y, pose.1);
        snap_car_to_ground(&mut car, pose, ground);
        car.rb.vel = car.forward() * speed;
        self.car = car;
        self.phase = Phase::Driving;
        self.cam.cut();
        self.pull = 0.0;
        self.pull_offset_now = Vec3::ZERO;
    }
}
