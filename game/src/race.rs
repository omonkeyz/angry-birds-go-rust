//! The race: grid, kart physics, AI drivers, laps, pickups, chase camera, 3D scene assembly and the HUD.
use crate::gfx::Draw;
use crate::kart::{Ground, Input, Kart, KartSpec};
use crate::models;
use crate::render3d::{Camera, Draw3d, MeshId, Scene3d, World};
use crate::track::{Lcg, Track, SAMPLE_SPACING, TRACKS};
use crate::ui::{self, ease, solid, sprite, Rect, Textures, STAGE_H, STAGE_W};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::path::Path;

// ------------------------------------------------------------------ roster

pub struct Character {
    pub name: &'static str,
    pub folder: &'static str,
    pub color: [f32; 3],
    /// Minimap marker in the `ingame` atlas.
    pub marker: Rect,
}

/// Start order (index 0 = pole). The player drives `ROSTER[PLAYER_SLOT]`.
pub const PLAYER_SLOT: usize = 4;
pub const ROSTER: [Character; 6] = [
    Character { name: "CHUCK", folder: "kart_yellowrocket_upgrade1", color: [1.0, 0.85, 0.15], marker: [1836.0, 75.0, 30.0, 30.0] },
    Character { name: "THE BLUES", folder: "kart_bluebird_upgrade1", color: [0.28, 0.52, 0.98], marker: [1186.0, 142.0, 30.0, 30.0] },
    Character { name: "STELLA", folder: "kart_pink_upgrade1", color: [0.98, 0.52, 0.78], marker: [1938.0, 75.0, 30.0, 30.0] },
    Character { name: "BOMB", folder: "kart_black_upgrade1", color: [0.16, 0.16, 0.2], marker: [1723.0, 108.0, 30.0, 30.0] },
    Character { name: "RED", folder: "kart_red_upgrade1", color: [0.9, 0.12, 0.08], marker: [1220.0, 142.0, 30.0, 30.0] },
    Character { name: "HELMET PIG", folder: "kart_helmetpig_upgrade1", color: [0.5, 0.72, 0.3], marker: [1152.0, 142.0, 30.0, 30.0] },
];

// HUD sprites in the `ingame` atlas.
pub const POSITION_BADGES: [Rect; 3] = [[114.0, 1173.0, 316.0, 454.0], [751.0, 661.0, 316.0, 454.0], [751.0, 1119.0, 316.0, 454.0]];
pub const COUNTDOWN: [Rect; 4] = [
    [434.0, 1173.0, 268.0, 375.0],
    [751.0, 1577.0, 268.0, 365.0],
    [1071.0, 661.0, 206.0, 355.0],
    [1512.0, 224.0, 512.0, 320.0],
];
pub const TEXT_BOARD: Rect = [2.0, 2.0, 1067.0, 136.0];
pub const ICON_BOOST: Rect = [1278.0, 1193.0, 220.0, 220.0];
pub const WIN_FLAG: Rect = [114.0, 661.0, 633.0, 508.0];

// ------------------------------------------------------------------ assets

pub struct KartMeshes {
    pub chassis: MeshId,
    pub wheel_front: MeshId,
    pub wheel_rear: MeshId,
    pub spec: KartSpec,
    /// Left wheel hub positions (right = mirrored in x).
    pub front_hub: Vec3,
    pub rear_hub: Vec3,
    pub front_scale: f32,
    pub rear_scale: f32,
    /// Body parts (nose, panels, milk carton, straw ...): each part's `attach_1` node sits on the chassis' `attach_<Part>_1` node.
    pub parts: Vec<(MeshId, Mat4)>,
}

pub struct TrackMeshes {
    pub road: MeshId,
    pub terrain: MeshId,
    pub scenery: MeshId,
    pub arch: MeshId,
}

pub struct RaceAssets {
    pub karts: Vec<KartMeshes>,
    pub boost_pad: MeshId,
    pub coin: MeshId,
    pub giftbox: MeshId,
    pub pig: MeshId,
    pub tracks: HashMap<usize, TrackMeshes>,
}

impl KartMeshes {
    /// Loads one kart (`folder` under `pak/cargeom`): the three original meshes, the wheel hub positions from the chassis
    /// nodes and the original `CarSpec`.
    pub fn load(scene: &mut Scene3d, assets: &Path, folder: &str) -> Result<KartMeshes, String> {
        let dir = assets.join("pak/cargeom").join(folder);
        let chassis = models::load_xgm(&dir.join("chassis_l02.xgm"))?;
        let wheel_front = models::load_xgm(&dir.join("wheelfront_l02.xgm"))?;
        let wheel_rear = models::load_xgm(&dir.join("wheelrear_l02.xgm"))?;
        let hub = |name: &str, fallback: Vec3| chassis.node(name).map(|n| Vec3::from(n.position)).unwrap_or(fallback);
        let front_hub = hub("front_left_wheel", Vec3::new(0.6, 0.23, 0.56));
        let rear_hub = hub("rear_left_wheel", Vec3::new(0.65, 0.31, -0.5));
        let xml_path = assets.join("xml_pak/cargeom").join(folder).join(format!("{folder}.xml"));
        let xml = std::fs::read_to_string(&xml_path).map_err(|e| format!("{}: {e}", xml_path.display()))?;
        let spec = KartSpec::from_xml(folder, &xml, front_hub.z, rear_hub.z)?;
        // the wheel meshes are unit-diameter; the spec gives the real radius
        let mut parts = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            let mut names: Vec<String> = read
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .filter(|n| n.ends_with("_l02.xgm") && !n.contains("outline") && !n.starts_with("chassis") && !n.starts_with("wheel"))
                .collect();
            names.sort();
            for file in names {
                let stem = file.trim_end_matches("_l02.xgm").to_string();
                let Ok(part) = models::load_xgm(&dir.join(&file)) else { continue };
                let want = format!("attach_{stem}_1").to_ascii_lowercase();
                let on_chassis = chassis.nodes.iter().find(|n| n.name.to_ascii_lowercase() == want);
                let on_part = part.nodes.iter().find(|n| n.name.eq_ignore_ascii_case("attach_1"));
                let (Some(a), Some(b)) = (on_chassis, on_part) else { continue };
                let node = |n: &abgtool::xgm::Node| Mat4::from_scale_rotation_translation(Vec3::from(n.scale), glam::Quat::from_array(n.rotation), Vec3::from(n.position));
                let m = node(a) * node(b).inverse();
                parts.push((scene.add_mesh(&part.mesh), m));
            }
        }
        let front_scale = spec.wheel_radius_front / 0.5;
        let rear_scale = spec.wheel_radius_rear / 0.5;
        Ok(KartMeshes {
            chassis: scene.add_mesh(&chassis.mesh),
            wheel_front: scene.add_mesh(&wheel_front.mesh),
            wheel_rear: scene.add_mesh(&wheel_rear.mesh),
            spec,
            front_hub,
            rear_hub,
            front_scale,
            rear_scale,
            parts,
        })
    }

    /// Chassis + four wheels as draw calls. `body` = kart-to-world transform, `steer` = front wheel angle, `spin` = wheel angle.
    pub fn draws(&self, out: &mut Vec<Draw3d>, body: Mat4, color: [f32; 3], steer: f32, spin: f32) {
        out.push(Draw3d { mesh: self.chassis, model: body, tint: [color[0], color[1], color[2], 1.0] });
        for (mesh, local) in &self.parts {
            out.push(Draw3d { mesh: *mesh, model: body * *local, tint: [color[0], color[1], color[2], 1.0] });
        }
        let dark = [0.09, 0.09, 0.1, 1.0];
        for (hub, scale, mesh, steer) in [(self.front_hub, self.front_scale, self.wheel_front, steer), (self.rear_hub, self.rear_scale, self.wheel_rear, 0.0)] {
            for side in [1.0f32, -1.0] {
                // left wheels use the mesh as-is; right wheels are mirrored across x
                let hub_pos = Vec3::new(hub.x * side, hub.y, hub.z);
                let m = body
                    * Mat4::from_translation(hub_pos)
                    * Mat4::from_rotation_y(steer)
                    * Mat4::from_rotation_x(spin)
                    * Mat4::from_scale(Vec3::new(scale * side, scale, scale));
                out.push(Draw3d { mesh, model: m, tint: dark });
            }
        }
    }
}

impl RaceAssets {
    pub fn load(scene: &mut Scene3d, assets: &Path) -> Result<RaceAssets, String> {
        let pak = assets.join("pak");
        let mut karts = Vec::new();
        for c in &ROSTER {
            karts.push(KartMeshes::load(scene, assets, c.folder)?);
        }
        let env = pak.join("envobjects");
        let mut prop = |name: &str| -> Result<MeshId, String> { Ok(scene.add_mesh(&models::load_xgm(&env.join(format!("{name}.xgm")))?.mesh)) };
        Ok(RaceAssets {
            karts,
            boost_pad: prop("boost_pad")?,
            coin: prop("coin")?,
            giftbox: prop("giftbox")?,
            pig: prop("piratepig1")?,
            tracks: HashMap::new(),
        })
    }

    pub fn ensure_track(&mut self, scene: &mut Scene3d, index: usize, track: &Track) {
        if self.tracks.contains_key(&index) {
            return;
        }
        let meshes = TrackMeshes {
            road: scene.add_mesh(&track.road_mesh()),
            terrain: scene.add_mesh(&track.terrain_mesh()),
            scenery: scene.add_mesh(&track.scenery_mesh()),
            arch: scene.add_mesh(&track.arch_mesh()),
        };
        self.tracks.insert(index, meshes);
    }
}

// ------------------------------------------------------------------ state

#[derive(Clone, Copy, Default, Debug)]
pub struct Keys {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub handbrake: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Countdown,
    Racing,
    /// The player has crossed the line; the others are still driving for a moment.
    Finished,
    Results,
}

#[derive(Clone, Debug)]
pub enum Event {
    Sound(&'static str),
}

struct Progress {
    index: usize,
    /// 0 = still behind the start line, 1..=laps = current lap, laps+1 = done.
    lap: u32,
    passed_half: bool,
}

struct Ai {
    skill: f32,
    lane: f32,
    stuck: f32,
}

pub struct Racer {
    pub kart: Kart,
    pub slot: usize,
    pub player: bool,
    progress: Progress,
    ai: Ai,
    pub finish_time: Option<f32>,
    pub coins: u32,
    boost_sound_cooldown: f32,
    visual_roll: f32,
    /// Statistics for the self test.
    offroad_time: f32,
    resets: u32,
    top_speed: f32,
}

struct Pad {
    pos: Vec3,
    yaw: f32,
}

struct Respawning {
    pos: Vec3,
    yaw: f32,
    /// Seconds until it is back; 0 = available.
    back_in: f32,
}

#[derive(Clone, Debug)]
pub struct ResultRow {
    pub name: &'static str,
    pub slot: usize,
    pub time: Option<f32>,
    pub player: bool,
}

pub struct Race {
    pub track: Track,
    pub track_index: usize,
    /// Driving data per roster slot (copied from the loaded assets).
    specs: Vec<KartSpec>,
    pub racers: Vec<Racer>,
    pub phase: Phase,
    countdown: f32,
    pub clock: f32,
    results_timer: f32,
    anim: f32,
    accumulator: f32,
    pads: Vec<Pad>,
    coins: Vec<Respawning>,
    boxes: Vec<Respawning>,
    pigs: Vec<Respawning>,
    rng: Lcg,
    /// The chase camera (`camera.rs`, the `CCamera` port; no track collision query here: this scene has no collision mesh).
    cam: crate::camera::ChaseCam,
    pub events: Vec<Event>,
    pub results: Vec<ResultRow>,
    last_count_sound: i32,
    /// Test hooks: the AI drives the player's kart / the camera sits at this offset (kart-local x left, y up, z forward).
    pub autopilot: bool,
    pub debug_cam_rel: Option<Vec3>,
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

fn yaw_of(t: Vec3) -> f32 {
    t.x.atan2(t.z)
}

impl Race {
    pub fn new(track_index: usize, assets: &RaceAssets) -> Race {
        let track = Track::generate(&TRACKS[track_index]);
        let n = track.len();
        let mut racers = Vec::new();
        for (slot, _c) in ROSTER.iter().enumerate() {
            let row = slot / 2;
            let col = slot % 2;
            let back = 9.0 + row as f32 * 7.0;
            let idx = (n as f32 - back / SAMPLE_SPACING).round() as usize % n;
            let s = &track.samples[idx];
            let lateral = if col == 0 { -2.8 } else { 2.8 };
            let pos = s.pos + s.right * lateral;
            let mut kart = Kart::new(pos, yaw_of(s.tangent));
            kart.pos.y = s.pos.y;
            let skill = 0.80 + 0.05 * ((slot * 7 + 3) % 5) as f32;
            racers.push(Racer {
                kart,
                slot,
                player: slot == PLAYER_SLOT,
                progress: Progress { index: idx, lap: 0, passed_half: false },
                ai: Ai { skill, lane: (slot as f32 - 2.5) * 0.8, stuck: 0.0 },
                finish_time: None,
                coins: 0,
                boost_sound_cooldown: 0.0,
                visual_roll: 0.0,
                offroad_time: 0.0,
                resets: 0,
                top_speed: 0.0,
            });
        }
        let mut race = Race {
            track,
            track_index,
            specs: assets.karts.iter().map(|k| k.spec.clone()).collect(),
            racers,
            phase: Phase::Countdown,
            countdown: 4.0,
            clock: 0.0,
            results_timer: 0.0,
            anim: 0.0,
            accumulator: 0.0,
            pads: Vec::new(),
            coins: Vec::new(),
            boxes: Vec::new(),
            pigs: Vec::new(),
            rng: Lcg(TRACKS[track_index].seed ^ 0x5eed),
            cam: crate::camera::ChaseCam::new(crate::camera::CamTweaks::default()),
            events: Vec::new(),
            results: Vec::new(),
            last_count_sound: 5,
            autopilot: false,
            debug_cam_rel: None,
        };
        race.place_pickups();
        race
    }

    fn at(&self, fraction: f32, lateral: f32) -> (Vec3, f32) {
        let n = self.track.len();
        let s = &self.track.samples[((fraction * n as f32) as usize) % n];
        (s.pos + s.right * lateral, yaw_of(s.tangent))
    }

    fn place_pickups(&mut self) {
        for (f, lat) in [(0.10, -2.5), (0.36, 2.5), (0.62, 0.0), (0.86, -2.5)] {
            let (pos, yaw) = self.at(f, lat);
            self.pads.push(Pad { pos, yaw });
        }
        for f0 in [0.05, 0.20, 0.30, 0.45, 0.55, 0.70, 0.80, 0.93] {
            for k in 0..8 {
                let f = f0 + k as f32 * 5.0 / self.track.length;
                let lat = ((f0 * 40.0 + k as f32 * 0.6).sin()) * 3.5;
                let (pos, yaw) = self.at(f, lat);
                self.coins.push(Respawning { pos, yaw, back_in: 0.0 });
            }
        }
        for f in [0.15, 0.50, 0.78] {
            for lat in [-3.8, 0.0, 3.8] {
                let (pos, yaw) = self.at(f, lat);
                self.boxes.push(Respawning { pos, yaw, back_in: 0.0 });
            }
        }
        for (f, lat) in [(0.27, 2.5), (0.58, -2.8), (0.90, 3.0)] {
            let (pos, yaw) = self.at(f, lat);
            self.pigs.push(Respawning { pos, yaw, back_in: 0.0 });
        }
    }

    pub fn player(&self) -> &Racer {
        self.racers.iter().find(|r| r.player).expect("player racer")
    }

    fn distance(&self, r: &Racer) -> f32 {
        if let Some(t) = r.finish_time {
            return 1.0e6 - t;
        }
        let n = self.track.len();
        let behind = r.progress.lap == 0 && r.progress.index > n / 2;
        (r.progress.lap as f32 - if behind { 1.0 } else { 0.0 }) * self.track.length + r.progress.index as f32 * SAMPLE_SPACING
    }

    /// 1-based race position of every racer, by index into `racers`.
    pub fn positions(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.racers.len()).collect();
        order.sort_by(|&a, &b| self.distance(&self.racers[b]).partial_cmp(&self.distance(&self.racers[a])).unwrap());
        let mut pos = vec![0; self.racers.len()];
        for (rank, &i) in order.iter().enumerate() {
            pos[i] = rank + 1;
        }
        pos
    }

    pub fn player_position(&self) -> usize {
        let pos = self.positions();
        let i = self.racers.iter().position(|r| r.player).unwrap();
        pos[i]
    }

    pub fn laps(&self) -> u32 {
        self.track.def.laps
    }

    // ------------------------------------------------------------------ simulation

    pub fn update(&mut self, dt: f32, keys: &Keys) {
        let dt = dt.min(0.1);
        self.anim += dt;
        match self.phase {
            Phase::Countdown => {
                self.countdown -= dt;
                let whole = self.countdown.ceil() as i32;
                if whole != self.last_count_sound && (0..=3).contains(&whole) {
                    self.last_count_sound = whole;
                    self.events.push(Event::Sound(match whole {
                        3 => "audio/ui/aby_ui_countdown_3.mp3",
                        2 => "audio/ui/aby_ui_countdown_2.mp3",
                        _ => "audio/ui/aby_ui_countdown_1.mp3",
                    }));
                }
                if self.countdown <= 0.0 {
                    self.phase = Phase::Racing;
                    self.events.push(Event::Sound("audio/ui/aby_ui_countdown_go.mp3"));
                    for r in &mut self.racers {
                        // a standing start: everybody gets a short launch boost window if they are not the player
                        if !r.player {
                            r.kart.boost_time = 0.6;
                        }
                    }
                }
            }
            Phase::Racing | Phase::Finished => {
                self.clock += dt;
                self.accumulator += dt;
                let h = 1.0 / 120.0;
                while self.accumulator >= h {
                    self.accumulator -= h;
                    self.step(h, keys);
                }
                if self.phase == Phase::Finished {
                    self.results_timer += dt;
                    let all_done = self.racers.iter().all(|r| r.finish_time.is_some());
                    if self.results_timer > 3.5 || all_done && self.results_timer > 1.5 {
                        self.finish_race();
                    }
                }
            }
            Phase::Results => {}
        }
        for list in [&mut self.coins, &mut self.boxes, &mut self.pigs] {
            for p in list.iter_mut() {
                p.back_in = (p.back_in - dt).max(0.0);
            }
        }
        self.update_camera(dt);
    }

    fn finish_race(&mut self) {
        let pos = self.positions();
        let mut rows: Vec<(usize, ResultRow)> = self
            .racers
            .iter()
            .enumerate()
            .map(|(i, r)| (pos[i], ResultRow { name: ROSTER[r.slot].name, slot: r.slot, time: r.finish_time, player: r.player }))
            .collect();
        rows.sort_by_key(|(p, _)| *p);
        self.results = rows.into_iter().map(|(_, row)| row).collect();
        self.phase = Phase::Results;
        let place = self.player_position();
        self.events.push(Event::Sound(if place <= 3 { "audio/music/mp3s/aby_music_win_race.mp3" } else { "audio/music/mp3s/aby_music_lose_race.mp3" }));
    }

    fn ai_input(&self, i: usize, player_distance: f32) -> Input {
        let r = &self.racers[i];
        let k = &r.kart;
        let n = self.track.len();
        let speed = k.speed().max(0.0);
        let look = (7.0 + speed * 0.42).min(34.0);
        let ahead = (r.progress.index + (look / SAMPLE_SPACING) as usize) % n;
        let s = &self.track.samples[ahead];
        let lane = r.ai.lane.clamp(-self.track.width * 0.32, self.track.width * 0.32);
        let target = s.pos + s.right * lane;
        let to = target - k.pos;
        let err = wrap_angle(to.x.atan2(to.z) - k.yaw); // > 0: the target is to the left
        let steer = (-err * 2.6).clamp(-1.0, 1.0);

        // brake for the sharpest bend within the next 1.8 s of travel
        let window = ((speed * 1.8).max(25.0) / SAMPLE_SPACING) as usize;
        let mut max_curv = 0.0f32;
        for d in 0..window.min(80) {
            max_curv = max_curv.max(self.track.samples[(r.progress.index + d) % n].curvature.abs());
        }
        let rubber = if self.distance(r) - player_distance > 90.0 { 0.9 } else if player_distance - self.distance(r) > 90.0 { 1.06 } else { 1.0 };
        let lateral_limit = 15.0 * r.ai.skill * rubber;
        let safe = if max_curv > 1e-4 { (lateral_limit / max_curv).sqrt() } else { 200.0 };
        let target_speed = safe.min(60.0 * r.ai.skill * rubber);
        let (throttle, brake) = if speed > target_speed + 3.0 { (0.0, ((speed - target_speed) / 12.0).clamp(0.3, 1.0)) } else if speed > target_speed { (0.35, 0.0) } else { (1.0, 0.0) };
        Input { throttle, brake, steer, handbrake: false }
    }

    fn step(&mut self, h: f32, keys: &Keys) {
        let n = self.track.len();
        let player_distance = self.distance(self.player());
        let player_finished = self.player().finish_time.is_some();
        for i in 0..self.racers.len() {
            let input = if self.racers[i].player && !player_finished && !self.autopilot {
                Input {
                    throttle: keys.up as u8 as f32,
                    brake: keys.down as u8 as f32,
                    steer: keys.right as i8 as f32 - keys.left as i8 as f32,
                    handbrake: keys.handbrake,
                }
            } else if self.racers[i].player && player_finished {
                // after the finish the player's kart coasts and brakes gently
                Input { throttle: 0.0, brake: 0.25, steer: 0.0, handbrake: false }
            } else {
                self.ai_input(i, player_distance)
            };

            let (pos, hint, slot) = {
                let r = &self.racers[i];
                (r.kart.pos, r.progress.index, r.slot)
            };
            let surf = self.track.surface(pos, Some(hint));
            let sample = self.track.samples[surf.index];
            let ground = Ground { on_road: surf.on_road, slope: sample.slope, tangent: sample.tangent };
            let racer = &mut self.racers[i];
            racer.kart.step(&self.specs[slot], &input, &ground, h);

            // ride height follows the road / terrain
            let target_y = if surf.on_road { self.track.road_height(racer.kart.pos, surf.index) } else { self.track.terrain_height(racer.kart.pos.x, racer.kart.pos.z, Some(surf.index)) };
            racer.kart.pos.y += (target_y - racer.kart.pos.y) * (1.0 - (-22.0 * h).exp());
            // invisible barrier well off the road
            let limit = self.track.width * 0.5 + 26.0;
            if surf.lateral.abs() > limit {
                let over = surf.lateral.abs() - limit;
                racer.kart.pos -= sample.right * over * surf.lateral.signum();
                let into = racer.kart.vel.dot(sample.right) * surf.lateral.signum();
                if into > 0.0 {
                    racer.kart.vel -= sample.right * into * surf.lateral.signum() * 1.2;
                }
            }
            racer.visual_roll += ((-racer.kart.lateral_accel * 0.012).clamp(-0.1, 0.1) - racer.visual_roll) * (1.0 - (-8.0 * h).exp());
            racer.boost_sound_cooldown = (racer.boost_sound_cooldown - h).max(0.0);
            if !surf.on_road {
                racer.offroad_time += h;
            }
            racer.top_speed = racer.top_speed.max(racer.kart.speed());

            // AI that has been stuck for a while is put back on the track
            if !racer.player {
                if racer.kart.speed().abs() < 1.2 && self.phase != Phase::Countdown {
                    racer.ai.stuck += h;
                } else {
                    racer.ai.stuck = 0.0;
                }
                if racer.ai.stuck > 2.5 {
                    let s = &self.track.samples[(surf.index + 6) % n];
                    racer.kart = Kart::new(s.pos, yaw_of(s.tangent));
                    racer.ai.stuck = 0.0;
                    racer.resets += 1;
                }
            }
        }
        self.update_progress();
        self.pickups();
        self.collide_karts();
    }

    fn update_progress(&mut self) {
        let n = self.track.len();
        let laps = self.laps();
        let clock = self.clock;
        let mut finished_now = Vec::new();
        for (i, r) in self.racers.iter_mut().enumerate() {
            let old = r.progress.index;
            let idx = self.track.nearest(r.kart.pos, Some(old), 12);
            r.progress.index = idx;
            if old > n * 3 / 4 && idx < n / 4 {
                if r.progress.lap == 0 {
                    r.progress.lap = 1;
                    r.progress.passed_half = false;
                } else if r.progress.passed_half {
                    r.progress.lap += 1;
                    r.progress.passed_half = false;
                    if r.progress.lap > laps && r.finish_time.is_none() {
                        r.finish_time = Some(clock);
                        finished_now.push(i);
                    }
                }
            } else if old < n / 4 && idx > n * 3 / 4 && r.progress.lap >= 1 {
                r.progress.lap -= 1;
                r.progress.passed_half = r.progress.lap >= 1;
            }
            if idx > n / 2 && idx < n * 3 / 4 {
                r.progress.passed_half = true;
            }
        }
        for i in finished_now {
            if self.racers[i].player && self.phase == Phase::Racing {
                self.phase = Phase::Finished;
                self.results_timer = 0.0;
                self.events.push(Event::Sound("audio/ui/results/aby_ui_finish_win_race.mp3"));
            }
        }
    }

    fn pickups(&mut self) {
        // boost pads
        for ri in 0..self.racers.len() {
            let (pos, is_player) = (self.racers[ri].kart.pos, self.racers[ri].player);
            for pad in &self.pads {
                let d = pos - pad.pos;
                let (c, s) = (pad.yaw.cos(), pad.yaw.sin());
                // pad axes: forward = (sin yaw, cos yaw), side = (cos yaw, -sin yaw)
                let along = d.x * s + d.z * c;
                let side = d.x * c - d.z * s;
                if along.abs() < 3.0 && side.abs() < 2.9 && self.racers[ri].kart.boost_time < 1.2 {
                    self.racers[ri].kart.boost_time = 2.2;
                    if is_player && self.racers[ri].boost_sound_cooldown <= 0.0 {
                        self.events.push(Event::Sound("audio/ingamegeneral/aby_general_boost.mp3"));
                        self.racers[ri].boost_sound_cooldown = 1.0;
                    }
                }
            }
            for coin in &mut self.coins {
                if coin.back_in <= 0.0 && (coin.pos - pos).length_squared() < 2.6 * 2.6 {
                    coin.back_in = 25.0;
                    if is_player {
                        self.racers[ri].coins += 1;
                        self.events.push(Event::Sound("audio/ingamegeneral/aby_general_coin_pickup.mp3"));
                    }
                }
            }
            for b in &mut self.boxes {
                if b.back_in <= 0.0 && (b.pos - pos).length_squared() < 2.6 * 2.6 {
                    b.back_in = 8.0;
                    let roll = self.rng.next_f32();
                    if roll < 0.65 {
                        self.racers[ri].kart.boost_time = 3.2;
                    } else {
                        self.racers[ri].coins += 8;
                    }
                    if is_player {
                        self.events.push(Event::Sound("audio/ingamegeneral/aby_general_gem_pickup.mp3"));
                    }
                }
            }
            for pig in &mut self.pigs {
                if pig.back_in <= 0.0 && (pig.pos - pos).length_squared() < 2.2 * 2.2 {
                    pig.back_in = 18.0;
                    self.racers[ri].kart.vel *= 0.45;
                    self.events.push(Event::Sound("audio/ingamegeneral/aby_general_pig_pop.mp3"));
                }
            }
        }
    }

    fn collide_karts(&mut self) {
        let radius = 1.0;
        for i in 0..self.racers.len() {
            for j in i + 1..self.racers.len() {
                let (a, b) = (self.racers[i].kart.pos, self.racers[j].kart.pos);
                let mut d = b - a;
                d.y = 0.0;
                let dist = d.length();
                if dist < radius * 2.0 && dist > 1e-4 {
                    let n = d / dist;
                    let push = (radius * 2.0 - dist) * 0.5;
                    self.racers[i].kart.pos -= n * push;
                    self.racers[j].kart.pos += n * push;
                    let rel = (self.racers[j].kart.vel - self.racers[i].kart.vel).dot(n);
                    if rel < 0.0 {
                        let impulse = -rel * 0.55;
                        self.racers[i].kart.vel -= n * impulse;
                        self.racers[j].kart.vel += n * impulse;
                        if (self.racers[i].player || self.racers[j].player) && rel < -2.5 {
                            self.events.push(Event::Sound("audio/karts/aby_kart_hit_wood_hard_01.mp3"));
                        }
                    }
                }
            }
        }
    }

    fn update_camera(&mut self, dt: f32) {
        let k = &self.player().kart;
        let mut car = crate::camera::CamCar::simple(k.pos, glam::Quat::from_rotation_y(k.yaw));
        car.vel = k.forward() * k.speed();
        car.speed = k.speed().abs();
        self.cam.update(dt, &car, None);
    }

    // ------------------------------------------------------------------ drawing

    pub fn world(&self, assets: &RaceAssets, aspect: f32) -> World {
        let (cam_eye, cam_look, cam_up, fov) = self.cam.view();
        let (eye, target, up) = match self.debug_cam_rel {
            Some(rel) => {
                let k = &self.player().kart;
                (k.pos + k.left() * rel.x + Vec3::Y * rel.y + k.forward() * rel.z, k.pos + Vec3::Y * 0.4, Vec3::Y)
            }
            None => (cam_eye, cam_look, cam_up),
        };
        let view = Mat4::look_at_rh(eye, target, up);
        let proj = Mat4::perspective_rh(fov, aspect, 0.1, 2500.0);
        let mut draws: Vec<Draw3d> = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        if let Some(t) = assets.tracks.get(&self.track_index) {
            for id in [t.terrain, t.road, t.scenery, t.arch] {
                draws.push(Draw3d { mesh: id, model: Mat4::IDENTITY, tint: white });
            }
        }
        for pad in &self.pads {
            let m = Mat4::from_translation(pad.pos + Vec3::Y * 0.12) * Mat4::from_rotation_y(pad.yaw) * Mat4::from_scale(Vec3::new(1.1, 1.0, 1.1));
            draws.push(Draw3d { mesh: assets.boost_pad, model: m, tint: [0.3, 0.95, 1.0, 1.0] });
        }
        for coin in self.coins.iter().filter(|c| c.back_in <= 0.0) {
            let m = Mat4::from_translation(coin.pos + Vec3::Y * (1.1 + (self.anim * 3.0 + coin.pos.x).sin() * 0.12)) * Mat4::from_rotation_y(self.anim * 3.2) * Mat4::from_scale(Vec3::splat(0.6));
            draws.push(Draw3d { mesh: assets.coin, model: m, tint: [1.0, 0.82, 0.18, 1.0] });
        }
        for b in self.boxes.iter().filter(|c| c.back_in <= 0.0) {
            let m = Mat4::from_translation(b.pos + Vec3::Y * (1.4 + (self.anim * 2.0 + b.pos.z).sin() * 0.2)) * Mat4::from_rotation_y(self.anim * 1.2) * Mat4::from_scale(Vec3::splat(0.55));
            draws.push(Draw3d { mesh: assets.giftbox, model: m, tint: [0.95, 0.45, 0.65, 1.0] });
        }
        for pig in self.pigs.iter().filter(|c| c.back_in <= 0.0) {
            let m = Mat4::from_translation(pig.pos + Vec3::Y * 0.7) * Mat4::from_rotation_y(pig.yaw + 3.14);
            draws.push(Draw3d { mesh: assets.pig, model: m, tint: [0.55, 0.85, 0.3, 1.0] });
        }
        for r in &self.racers {
            self.push_kart(&mut draws, assets, r);
        }
        World { camera: Camera { view_proj: proj * view, eye }, draws, sky: self.track.def.sky, fog_density: 0.0021, sun_dir: Vec3::new(-0.45, -0.8, -0.35), overlay: false }
    }

    fn push_kart(&self, draws: &mut Vec<Draw3d>, assets: &RaceAssets, r: &Racer) {
        let km = &assets.karts[r.slot];
        let sample = &self.track.samples[r.progress.index];
        let slope = sample.slope * r.kart.forward().dot(sample.tangent);
        let body = Mat4::from_translation(r.kart.pos)
            * Mat4::from_rotation_y(r.kart.yaw)
            * Mat4::from_rotation_x(-slope.atan())
            * Mat4::from_rotation_z(r.visual_roll);
        km.draws(draws, body, ROSTER[r.slot].color, r.kart.steer_angle, r.kart.wheel_spin);
    }

    pub fn engine_sound(&self) -> (f32, f32) {
        let k = &self.player().kart;
        let speed = k.speed().abs();
        let volume = if self.phase == Phase::Countdown { 0.15 } else { (0.25 + speed / 35.0).min(0.9) };
        let pitch = 0.7 + speed / 40.0 + if k.boost_time > 0.0 { 0.15 } else { 0.0 };
        (volume, pitch.min(2.2))
    }

    pub fn music(&self) -> Option<&'static str> {
        match self.phase {
            Phase::Countdown => None,
            Phase::Racing | Phase::Finished => Some(self.track.def.music),
            Phase::Results => Some("audio/music/mp3s/aby_music_results.mp3"),
        }
    }

    // ------------------------------------------------------------------ HUD

    fn shadow_text(font: &crate::font::Font, text: &str, cx: f32, cy: f32, scale: f32, color: [f32; 4], out: &mut Vec<Draw>) {
        font.draw_centered(text, cx + 2.5 * scale, cy + 2.5 * scale, scale, [0.0, 0.0, 0.0, 0.65 * color[3]], out);
        font.draw_centered(text, cx, cy, scale, color, out);
    }

    pub fn hud(&self, tx: &Textures) -> Vec<Draw> {
        let mut out = Vec::new();
        let p = self.player();
        let position = self.player_position();
        let ig = &tx.ingame;

        // top-left: lap board + race time
        let lap = (p.progress.lap.max(1)).min(self.laps());
        out.push(sprite(ig, TEXT_BOARD, [28.0, 22.0, 380.0, 48.5], 0.92));
        Self::shadow_text(&tx.body_font, &format!("LAP {lap}/{}", self.laps()), 218.0, 46.0, 0.95, [1.0, 1.0, 1.0, 1.0], &mut out);
        let t = if self.phase == Phase::Countdown { 0.0 } else { self.clock };
        let time = format!("{}:{:05.2}", (t / 60.0) as u32, t % 60.0);
        Self::shadow_text(&tx.body_font, &time, 218.0, 98.0, 1.0, [1.0, 1.0, 1.0, 1.0], &mut out);

        // top-right: position badge + coins
        let bx = STAGE_W - 170.0;
        if position <= 3 {
            out.push(sprite(ig, POSITION_BADGES[position - 1], [bx, 18.0, 316.0 * 0.4, 454.0 * 0.4], 1.0));
        } else {
            Self::shadow_text(&tx.title_font, &format!("{position}TH"), bx + 63.0, 90.0, 1.0, [1.0, 1.0, 1.0, 1.0], &mut out);
        }
        Self::shadow_text(&tx.body_font, &format!("{}", p.coins), bx + 63.0, 222.0, 1.1, [1.0, 0.86, 0.25, 1.0], &mut out);

        // bottom-right: speedometer
        let kmh = (p.kart.speed().abs() * 3.6).round() as i32;
        Self::shadow_text(&tx.title_font, &format!("{kmh}"), STAGE_W - 190.0, STAGE_H - 100.0, 1.35, [1.0, 1.0, 1.0, 1.0], &mut out);
        Self::shadow_text(&tx.body_font, "KM/H", STAGE_W - 190.0, STAGE_H - 36.0, 0.85, [1.0, 0.85, 0.3, 1.0], &mut out);

        // boost indicator
        if p.kart.boost_time > 0.0 {
            let pulse = 0.9 + 0.1 * (self.anim * 14.0).sin();
            out.push(sprite(ig, ICON_BOOST, ui::grown([STAGE_W / 2.0 - 60.0, STAGE_H - 150.0, 120.0, 120.0], pulse), 1.0));
        }

        self.minimap(tx, &mut out);

        match self.phase {
            Phase::Countdown => {
                let n = self.countdown.ceil() as i32;
                let (idx, frac) = match n {
                    4 => (None, 0.0),
                    3 => (Some(0), self.countdown - 2.0),
                    2 => (Some(1), self.countdown - 1.0),
                    1 => (Some(2), self.countdown),
                    _ => (None, 0.0),
                };
                if let Some(i) = idx {
                    let r = COUNTDOWN[i];
                    let pop = 0.85 + 0.25 * ease(1.0 - frac * 1.2);
                    let (w, h) = (r[2] * pop * 0.95, r[3] * pop * 0.95);
                    out.push(sprite(ig, r, [STAGE_W / 2.0 - w / 2.0, STAGE_H * 0.42 - h / 2.0, w, h], 1.0));
                }
            }
            Phase::Racing => {
                if self.clock < 1.2 {
                    let r = COUNTDOWN[3];
                    let a = 1.0 - ease((self.clock - 0.6) / 0.6);
                    let (w, h) = (r[2] * 0.95, r[3] * 0.95);
                    out.push(sprite(ig, r, [STAGE_W / 2.0 - w / 2.0, STAGE_H * 0.42 - h / 2.0, w, h], a));
                }
            }
            Phase::Finished => {
                Self::shadow_text(&tx.title_font, "FINISH!", STAGE_W / 2.0, STAGE_H * 0.36, 1.6, [1.0, 0.9, 0.3, 1.0], &mut out);
            }
            Phase::Results => self.results_panel(tx, &mut out),
        }
        out
    }

    fn minimap(&self, tx: &Textures, out: &mut Vec<Draw>) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for s in &self.track.samples {
            min = min.min(s.pos);
            max = max.max(s.pos);
        }
        let size = 250.0;
        let extent = (max.x - min.x).max(max.z - min.z);
        let scale = size / extent;
        let origin = (60.0, STAGE_H - 60.0 - size);
        let map = |p: Vec3| (origin.0 + (p.x - min.x) * scale + (size - (max.x - min.x) * scale) / 2.0, origin.1 + (p.z - min.z) * scale + (size - (max.z - min.z) * scale) / 2.0);
        out.push(solid(tx.white, [origin.0 - 14.0, origin.1 - 14.0, size + 28.0, size + 28.0], [0.0, 0.0, 0.0], 0.38));
        for (i, s) in self.track.samples.iter().enumerate().step_by(3) {
            let (x, y) = map(s.pos);
            let c = if i < 4 { [1.0, 1.0, 0.4] } else { [0.92, 0.92, 0.92] };
            out.push(solid(tx.white, [x - 2.0, y - 2.0, 4.0, 4.0], c, 0.9));
        }
        for r in self.racers.iter().filter(|r| !r.player) {
            let (x, y) = map(r.kart.pos);
            out.push(sprite(&tx.ingame, ROSTER[r.slot].marker, [x - 11.0, y - 11.0, 22.0, 22.0], 1.0));
        }
        let (x, y) = map(self.player().kart.pos);
        out.push(sprite(&tx.ingame, ROSTER[self.player().slot].marker, [x - 17.0, y - 17.0, 34.0, 34.0], 1.0));
    }

    fn results_panel(&self, tx: &Textures, out: &mut Vec<Draw>) {
        let place = self.results.iter().position(|r| r.player).map(|i| i + 1).unwrap_or(0);
        out.push(solid(tx.white, [0.0, 0.0, STAGE_W, STAGE_H], [0.0, 0.0, 0.0], 0.55));
        let panel = [420.0, 120.0, 1080.0, 840.0];
        out.push(solid(tx.white, panel, [0.12, 0.09, 0.07], 0.93));
        out.push(solid(tx.white, [panel[0], panel[1], panel[2], 6.0], [1.0, 0.75, 0.2], 1.0));
        Self::shadow_text(&tx.title_font, "RACE COMPLETE", STAGE_W / 2.0, 190.0, 1.15, [1.0, 0.85, 0.3, 1.0], out);
        if place >= 1 && place <= 3 {
            out.push(sprite(&tx.ingame, POSITION_BADGES[place - 1], [450.0, 250.0, 316.0 * 0.62, 454.0 * 0.62], 1.0));
        } else {
            Self::shadow_text(&tx.title_font, &format!("{place}TH"), 560.0, 420.0, 1.6, [1.0, 1.0, 1.0, 1.0], out);
        }
        if place == 1 {
            out.push(sprite(&tx.ingame, WIN_FLAG, [1180.0, 240.0, 633.0 * 0.28, 508.0 * 0.28], 1.0));
        }
        for (i, row) in self.results.iter().enumerate() {
            let y = 280.0 + i as f32 * 78.0;
            let (x0, x1) = (790.0, 1460.0);
            if row.player {
                out.push(solid(tx.white, [x0 - 20.0, y - 32.0, x1 - x0 + 40.0, 64.0], [1.0, 0.75, 0.2], 0.28));
            }
            let color = if row.player { [1.0, 0.9, 0.4, 1.0] } else { [1.0, 1.0, 1.0, 1.0] };
            out.push(sprite(&tx.ingame, ROSTER[row.slot].marker, [x0, y - 17.0, 34.0, 34.0], 1.0));
            tx.body_font.draw(&format!("{}", i + 1), x0 + 48.0, y - 16.0, 1.0, color, out);
            tx.body_font.draw(row.name, x0 + 100.0, y - 16.0, 1.0, color, out);
            let time = match row.time {
                Some(t) => format!("{}:{:05.2}", (t / 60.0) as u32, t % 60.0),
                None => "DNF".to_string(),
            };
            let w = tx.body_font.width(&time, 1.0);
            tx.body_font.draw(&time, x1 - w, y - 16.0, 1.0, color, out);
        }
        Self::shadow_text(&tx.body_font, "ENTER: CONTINUE      R: RACE AGAIN", STAGE_W / 2.0, 905.0, 0.95, [1.0, 1.0, 1.0, 0.95], out);
    }

    /// One line per racer: used by the headless self test.
    pub fn summary(&self) -> String {
        let pos = self.positions();
        let mut rows: Vec<String> = self
            .racers
            .iter()
            .enumerate()
            .map(|(i, r)| {
                format!(
                    "  P{} {:<10} lap {} finish {:>7} top {:>5.1} km/h off-road {:>5.1}s resets {} coins {}",
                    pos[i],
                    ROSTER[r.slot].name,
                    r.progress.lap,
                    r.finish_time.map(|t| format!("{t:.1}s")).unwrap_or_else(|| "-".into()),
                    r.top_speed * 3.6,
                    r.offroad_time,
                    r.resets,
                    r.coins
                )
            })
            .collect();
        rows.sort();
        rows.join("\n")
    }

    /// Puts the player back on the road, facing along it.
    pub fn reset_player(&mut self) {
        let i = self.racers.iter().position(|r| r.player).unwrap();
        let idx = self.racers[i].progress.index;
        let s = self.track.samples[(idx + 2) % self.track.len()];
        self.racers[i].kart = Kart::new(s.pos, yaw_of(s.tangent));
    }
}

