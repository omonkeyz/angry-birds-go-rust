//! In-race glue for the abilities (child module of `eventrun`): every kart (player, AI, boss) owns a [`CarAbilitySlot`]; this module
//! refreshes each ability's environment, ticks it, applies the `CarEffect`s it returns to the real `CarSim` karts, executes its
//! `WorldRequest`s (smackable bodies of the item world, shields, visuals), delivers the world callbacks (`WorldEvent`) and triggers
//! the abilities: the player's button, `CRaceAI`'s ability output for the AI karts, the boss ability schedule of `modes/boss.rs`.
//!
//! NOT original (flagged UNRESOLVED where it matters): kart-vs-ability-object contacts use the sphere / box contact of `items::smackables`
//! (carsim has no chassis contacts), objects ignore their owner for 1 s after spawn, spin-outs are applied as a yaw angular velocity
//! (`CCar::IntegrateSteering` Spin360 term), particles / sounds have no engine and are only logged, explosions / shield bubbles are drawn
//! as simple opaque meshes.
use super::*;
use crate::abilities::{AbilityEconomy, AbilityEnv, CarAbilitySlot, OtherCar, RaceAbilityCtx, WorldEvent, WorldRequest};
use crate::damage::Bodywork;
use crate::eventextras::build_ability_slot;
use crate::items::smackables::{self, SmackState};
use crate::render3d::{MeshData, MeshId, Vertex3};
use std::collections::BTreeSet;

/// Seconds of one physics step (UNRESOLVED 1/60 as in carsim / eventextras).
const PHYSICS_DT: f32 = 1.0 / 60.0;
/// Smackable names of the objects abilities spawn at run time (none ported yet: bomb/boss objects need the object-spawn abilities).
pub const ABILITY_OBJECT_MODELS: &[&str] = &[];
/// An ability object does not touch its owner for this long after it spawned (NOT original).
const OWNER_GRACE: f32 = 1.0;

/// `CCar::Spin360` state (`+0x4a8` total time, `+0x4ac` time left, `+0x4b0` = 3 * spins).
#[derive(Clone, Copy, Debug)]
pub struct SpinState {
    pub total: f32,
    pub left: f32,
    pub spins: f32,
}

/// A visible explosion (opaque expanding sphere; the original plays a particle effect).
#[derive(Clone, Copy, Debug)]
pub struct Blast {
    pub pos: Vec3,
    pub radius: f32,
    pub age: f32,
}

/// A shield sphere (`SetShield`).
#[derive(Clone, Debug)]
pub struct ShieldVis {
    pub owner: usize,
    pub pos: Vec3,
    pub radius: f32,
    pub enabled: bool,
    pub model: String,
}

#[derive(Clone, Debug)]
pub struct VisualVis {
    pub model: String,
    pub pos: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub scale: f32,
}

/// Where each ability object lives in the item world.
#[derive(Clone, Copy, Debug)]
struct ObjRef {
    index: usize,
    owner: usize,
    age: f32,
    last_vy: f32,
}

pub struct AbilityHost {
    /// Slot of every AI kart by car id (id 0 = the player, whose slot lives in `PlayerExtras`).
    pub slots: Vec<Option<CarAbilitySlot>>,
    pub names: Vec<String>,
    pub chars: Vec<u8>,
    /// Boss ability index of the boss file -> index in `CarAbilitySlot::boss`.
    pub boss_maps: Vec<Vec<Option<usize>>>,
    /// AI karts have no `PlayerExtras`: a bodywork each (hit points / pilot ejection counters).
    pub bodyworks: Vec<Option<Bodywork>>,
    pub spins: Vec<Option<SpinState>>,
    pub ground: Vec<Option<(f32, Vec3)>>,
    pub invuln: Vec<f32>,
    accum: Vec<f32>,
    objects: HashMap<u32, ObjRef>,
    pub shields: HashMap<u32, ShieldVis>,
    pub visuals: HashMap<u32, VisualVis>,
    pub blasts: Vec<Blast>,
    /// `CGame::EnterSlowMo`: (scale, seconds left).
    pub slowmo: Option<(f32, f32)>,
    base_gravity: Vec<f32>,
    pub trigger_counts: Vec<u32>,
    /// AI karts that asked for their pilot ability this frame (`CRaceAI` output).
    pub ai_requests: Vec<usize>,
    /// (car, boss ability index in the boss file) requested by the boss schedule.
    pub boss_requests: Vec<(usize, usize)>,
    pub economy: AbilityEconomy,
    pub unsupported: BTreeSet<String>,
    pub log: Vec<String>,
    /// Effect names / sounds seen (diagnostics for captures).
    pub fx_seen: BTreeSet<String>,
    pub sphere: Option<MeshId>,
    pub hits: u32,
    pub spawned: u32,
    pub explosions: u32,
    pub spin_outs: u32,
    pub damage_applied: f32,
}

fn unit_sphere(color: [f32; 4]) -> MeshData {
    let (lat, lon) = (10usize, 16usize);
    let mut m = MeshData::default();
    for i in 0..=lat {
        let t = i as f32 / lat as f32 * std::f32::consts::PI;
        for j in 0..=lon {
            let p = j as f32 / lon as f32 * std::f32::consts::TAU;
            let n = Vec3::new(t.sin() * p.cos(), t.cos(), t.sin() * p.sin());
            m.vertices.push(Vertex3 { pos: n.to_array(), normal: n.to_array(), color });
        }
    }
    for i in 0..lat {
        for j in 0..lon {
            let a = (i * (lon + 1) + j) as u32;
            let b = a + (lon + 1) as u32;
            m.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m
}

impl AbilityHost {
    pub fn new(n_cars: usize, economy: AbilityEconomy) -> AbilityHost {
        AbilityHost {
            slots: (0..n_cars).map(|_| None).collect(),
            names: vec![String::new(); n_cars],
            chars: vec![0; n_cars],
            boss_maps: vec![Vec::new(); n_cars],
            bodyworks: (0..n_cars).map(|_| None).collect(),
            spins: vec![None; n_cars],
            ground: vec![None; n_cars],
            invuln: vec![0.0; n_cars],
            accum: vec![0.0; n_cars],
            objects: HashMap::new(),
            shields: HashMap::new(),
            visuals: HashMap::new(),
            blasts: Vec::new(),
            slowmo: None,
            base_gravity: vec![f32::NAN; n_cars],
            trigger_counts: vec![0; n_cars],
            ai_requests: Vec::new(),
            boss_requests: Vec::new(),
            economy,
            unsupported: BTreeSet::new(),
            log: Vec::new(),
            fx_seen: BTreeSet::new(),
            sphere: None,
            hits: 0,
            spawned: 0,
            explosions: 0,
            spin_outs: 0,
            damage_applied: 0.0,
        }
    }

    /// Register the sphere mesh used for explosions / shield bubbles.
    pub fn add_meshes(&mut self, scene: &mut Scene3d) {
        self.sphere = Some(scene.add_mesh(&unit_sphere([1.0, 1.0, 1.0, 1.0])));
    }

    /// Install the ability slot of AI kart `id` (the player's slot stays in `PlayerExtras`).
    pub fn install(&mut self, id: usize, slot: CarAbilitySlot, name: String, character: u8, boss_map: Vec<Option<usize>>, fragility: f32) {
        self.slots[id] = Some(slot);
        self.names[id] = name;
        self.chars[id] = character;
        self.boss_maps[id] = boss_map;
        self.bodyworks[id] = Some(Bodywork::new(Vec::new(), fragility));
    }

    /// Time scale of the whole game from the slow-mo ability (`CGame::GetCurrentSlowMoTimeMultiplier`).
    pub fn time_scale(&self) -> f32 {
        self.slowmo.map(|(s, _)| s).unwrap_or(1.0)
    }
}

/// What the host knows about one kart for the ability code.
#[derive(Clone)]
struct Snap {
    pos: Vec3,
    vel: Vec3,
    forward: Vec3,
    up: Vec3,
    rot: glam::Quat,
    mass: f32,
    spline_distance: f32,
    position: u32,
    wheels: u32,
    speed: f32,
}

impl EventRun {
    fn car_ref(&self, id: usize) -> &CarSim {
        if id == 0 {
            &self.player.car
        } else {
            &self.ai[id - 1].car
        }
    }

    fn car_mut(&mut self, id: usize) -> &mut CarSim {
        if id == 0 {
            &mut self.player.car
        } else {
            &mut self.ai[id - 1].car
        }
    }

    fn snap(&self, id: usize) -> Snap {
        let car = self.car_ref(id);
        let (_, up, _) = car.rb.axes();
        let c = &self.rs.cars[id];
        Snap {
            pos: car.rb.pos,
            vel: car.rb.vel,
            forward: car.forward(),
            up,
            rot: car.rb.rot,
            mass: car.spec.m_fMass,
            spline_distance: c.tracker.distance_from_start(&self.rs.track),
            position: c.position.max(0) as u32,
            wheels: car.wheels_on_ground() as u32,
            speed: car.speed(),
        }
    }

    fn kart_state(&self, id: usize) -> ModeKart {
        let s = self.snap(id);
        ModeKart {
            id,
            is_player: id == 0,
            character: self.abil.chars[id],
            pos: s.pos,
            forward: s.forward,
            up: s.up,
            velocity: s.vel,
            speed: s.speed,
            wheels_on_ground: s.wheels,
            race_position: s.position,
            spline_distance: s.spline_distance,
            lap: 0,
            race_time: self.rs.cars[id].time_s,
        }
    }

    fn take_slot(&mut self, id: usize) -> Option<CarAbilitySlot> {
        if id == 0 {
            if self.extras.ability.ability.is_none() && self.extras.ability.boss.is_empty() {
                return None;
            }
            Some(std::mem::replace(&mut self.extras.ability, CarAbilitySlot::new(None)))
        } else {
            self.abil.slots.get_mut(id).and_then(|s| s.take())
        }
    }

    fn put_slot(&mut self, id: usize, slot: CarAbilitySlot) {
        if id == 0 {
            self.extras.ability = slot;
        } else {
            self.abil.slots[id] = Some(slot);
        }
    }

    fn has_slot(&self, id: usize) -> bool {
        if id == 0 {
            self.extras.ability.ability.is_some() || !self.extras.ability.boss.is_empty()
        } else {
            self.abil.slots.get(id).map_or(false, |s| s.is_some())
        }
    }

    fn slot_immunities(&self, id: usize) -> (bool, bool, bool) {
        if id == 0 {
            self.extras.ability.immunities()
        } else {
            self.abil.slots.get(id).and_then(|s| s.as_ref()).map(|s| s.immunities()).unwrap_or((false, false, false))
        }
    }

    /// The environment of kart `id` (`CGame` car list etc.).
    fn make_env(&self, id: usize, snaps: &[Snap]) -> AbilityEnv {
        let n = snaps.len();
        let mut env = AbilityEnv::default();
        env.physics_dt = PHYSICS_DT;
        env.is_local_player = id == 0;
        env.driver_ability_flag = id != 0;
        env.team = id as i32;
        env.race_time = self.rs.clock_s;
        env.num_players = 1;
        env.team_mode = false;
        for j in 0..n {
            if j == id {
                continue;
            }
            let s = &snaps[j];
            let (isp, idm, iex) = self.slot_immunities(j);
            env.others.push(OtherCar {
                id: j,
                pos: s.pos,
                vel: s.vel,
                forward: s.forward,
                up: s.up,
                team: j as i32,
                is_player: j == 0,
                hittable: self.abil.invuln[j] <= 0.0,
                progress_value: self.rs.cars[j].tracker.pos,
                penalty: self.abil.spins[j].map(|s| s.left).unwrap_or(0.0),
                spline_distance: s.spline_distance,
                race_position: s.position,
                immune_spin: isp,
                immune_damage: idm,
                immune_explosions: iex,
            });
        }

        env.ground_below = self.abil.ground[id];
        let c = &self.rs.cars[id];
        if let Some(s) = self.rs.track.splines.get(c.tracker.spline_id) {
            env.track_dir = Some(s.nodes[(c.tracker.pos as usize).min(s.count() - 1)].dir);
        }
        env.progress_value = c.tracker.pos;
        env
    }

    /// Every kart's per-frame ability update: env refresh, `tick`, `integrate` per physics step, effects, world requests.
    pub(super) fn update_abilities(&mut self, dt: f32, track: &TrackWorld) {
        let n = self.rs.cars.len();
        // slow-mo bookkeeping (CGame::Process)
        if let Some((s, left)) = self.abil.slowmo {
            let left = left - dt;
            self.abil.slowmo = if left > 0.0 { Some((s, left)) } else { None };
        }
        for t in self.abil.invuln.iter_mut() {
            *t = (*t - dt).max(0.0);
        }
        for b in self.abil.blasts.iter_mut() {
            b.age += dt;
        }
        self.abil.blasts.retain(|b| b.age < 0.6);
        // spin timers
        for id in 0..n {
            if let Some(s) = self.abil.spins[id].as_mut() {
                s.left -= PHYSICS_DT.min(dt);
            }
            if self.abil.spins[id].map_or(false, |s| s.left <= 0.0) {
                self.abil.spins[id] = None;
            }
        }
        let started = self.player.phase == drive::Phase::Driving;
        if !started {
            return;
        }
        let snaps: Vec<Snap> = (0..n).map(|i| self.snap(i)).collect();
        {
            use crate::carsim::Ground;
            for i in 0..n {
                let (h, nrm) = track.ground.height_normal(snaps[i].pos);
                self.abil.ground[i] = if h > -1.0e3 { Some((h, nrm)) } else { None };
            }
        }
        let mut pending: Vec<(usize, Vec<CarEffect>, Vec<WorldRequest>)> = Vec::new();
        for id in 0..n {
            if !self.has_slot(id) {
                continue;
            }
            let launched = id == 0 || !self.ai[id - 1].in_sling;
            if !launched {
                continue;
            }
            let env = self.make_env(id, &snaps);
            let kart = self.kart_state(id);
            let Some(mut slot) = self.take_slot(id) else { continue };
            slot.set_env(&env);
            let mut out = Vec::new();
            slot.tick(dt, &kart, &mut out);
            let _ = slot.charged_fraction(true);
            self.abil.accum[id] += dt;
            let mut steps = 0;
            while self.abil.accum[id] >= PHYSICS_DT && steps < 8 {
                self.abil.accum[id] -= PHYSICS_DT;
                steps += 1;
                slot.integrate(&kart, &mut out);
            }
            let reqs = slot.drain_requests();
            self.put_slot(id, slot);
            pending.push((id, out, reqs));
        }
        for (id, out, reqs) in pending {
            self.apply_effects(id, out, dt);
            self.apply_requests(id, reqs);
        }
        self.refresh_visuals();
    }

    /// The ability button / an AI request: `CCar::TriggerAbility`. `boss_index` = the boss ability of the boss file (CCar::TriggerBossAbility).
    pub(super) fn trigger_car(&mut self, id: usize, boss_index: Option<usize>) -> bool {
        if self.stage != Stage::Racing && self.stage != Stage::Finishing {
            return false;
        }
        if id >= self.rs.cars.len() || self.rs.cars[id].race_completed || !self.has_slot(id) {
            return false;
        }
        if id == 0 && self.player.phase != drive::Phase::Driving {
            return false;
        }
        if id > 0 && self.ai[id - 1].in_sling {
            return false;
        }
        let n = self.rs.cars.len();
        let snaps: Vec<Snap> = (0..n).map(|i| self.snap(i)).collect();
        let env = self.make_env(id, &snaps);
        let kart = self.kart_state(id);
        let Some(mut slot) = self.take_slot(id) else { return false };
        slot.set_env(&env);
        let mut out = Vec::new();
        let fired = match boss_index {
            Some(i) => {
                let idx = self.abil.boss_maps.get(id).and_then(|m| m.get(i).copied().flatten());
                match idx {
                    Some(k) => slot.trigger_boss(k, &kart, true, &mut out),
                    None => false,
                }
            }
            None => {
                let ctx = RaceAbilityCtx {
                    game_state: 8,
                    game_state_aux: 0,
                    event_allows_abilities: true,
                    multiple_abilities_enabled: id == 0,
                    num_players: 1,
                    mp_game_active: false,
                    dbg77: false,
                    economy: &self.abil.economy,
                };
                (id != 0 || slot.can_trigger(&kart, &ctx)) && slot.trigger(&kart, &mut out)
            }
        };
        let reqs = slot.drain_requests();
        self.put_slot(id, slot);
        if fired {
            self.abil.trigger_counts[id] += 1;
            if id == 0 {
                self.extras.triggers += 1;
            }
            let name = match boss_index {
                Some(i) => format!("boss ability {i}"),
                None => self.abil_name(id),
            };
            self.log.push(format!("t={:.1} kart {id} triggers {name} ({} effects, {} requests)", self.rs.clock_s, out.len(), reqs.len()));
            self.apply_effects(id, out, 0.0);
            self.apply_requests(id, reqs);
            self.refresh_visuals();
        }
        fired
    }

    fn abil_name(&self, id: usize) -> String {
        if id == 0 {
            self.extras.ability_name.clone()
        } else {
            self.abil.names[id].clone()
        }
    }

    /// Per-frame `CarEffect` application (`dt` = 0 for the instant effects of a trigger).
    fn apply_effects(&mut self, from: usize, effects: Vec<CarEffect>, _dt: f32) {
        for e in effects {
            match e {
                CarEffect::BodyForce { car, force_local, at_local } => {
                    if car < self.rs.cars.len() {
                        self.car_mut(car).rb.apply_body_force(force_local, at_local);
                    }
                }
                CarEffect::WorldForce { car, force, at } => {
                    if car < self.rs.cars.len() {
                        self.car_mut(car).rb.apply_world_force(force, at);
                    }
                }
                CarEffect::SetSteeringMultiplier { car, mul } => {
                    if car == 0 {
                        self.extras.steer_multiplier = mul;
                    }
                }
                CarEffect::SetGravityMultiplier { car, mul } => {
                    if car < self.rs.cars.len() {
                        let base = if self.abil.base_gravity[car].is_nan() { self.car_ref(car).rb.gravity.y } else { self.abil.base_gravity[car] };
                        self.abil.base_gravity[car] = base;
                        self.car_mut(car).rb.gravity.y = base * mul;
                    }
                }
                CarEffect::SetDownforceMultiplier { .. } => {
                    self.abil.unsupported.insert("SetDownforceMultiplier (carsim recomputes the downforce every frame)".into());
                }
                CarEffect::SetInvulnerable { car, seconds } => {
                    if car < self.abil.invuln.len() {
                        self.abil.invuln[car] = self.abil.invuln[car].max(seconds);
                    }
                }
                CarEffect::SpinOut { car, time, spins } => {
                    if car < self.rs.cars.len() {
                        let (_, imm, _) = (0, false, 0);
                        let (immune_spin, _, _) = self.slot_immunities(car);
                        let _ = imm;
                        if !immune_spin {
                            // CCar::Spin360: time <= 0 -> spins * 1.5 s; spins count is the int argument
                            let t = if time <= 0.0 { spins * 1.5 } else { time };
                            self.abil.spins[car] = Some(SpinState { total: t.max(0.05), left: t.max(0.05), spins });
                            self.abil.spin_outs += 1;
                            self.log.push(format!("t={:.1} kart {car} spins out ({spins} spin(s), {t:.1}s)", self.rs.clock_s));
                        }
                    }
                }
                CarEffect::Damage { car, amount, source } => {
                    if car < self.rs.cars.len() {
                        self.damage_car(car, amount, source);
                    }
                }
                CarEffect::Explosion { center, radius, force, damage, source } => {
                    self.explosion(center, radius, force, damage, source);
                }
                CarEffect::SlowMo { scale, seconds } => {
                    if seconds > 0.0 {
                        self.abil.slowmo = Some((scale.clamp(0.05, 1.0), seconds));
                    }
                }
                CarEffect::Repair { car, fraction } => {
                    if car == 0 {
                        let mut ev = Vec::new();
                        self.extras.bodywork.repair_fraction(fraction, 0, &mut ev);
                    }
                }
                CarEffect::Particle { name, .. } => {
                    self.abil.fx_seen.insert(format!("particle:{name}"));
                }
                CarEffect::Sound { name, .. } => {
                    self.abil.fx_seen.insert(format!("sound:{name}"));
                }
                CarEffect::SetTimeScale { .. } => {
                    self.abil.unsupported.insert("SetTimeScale (per-car physics time dilation)".into());
                }
                CarEffect::CameraBehind { .. } => {
                    self.abil.unsupported.insert("CameraBehind effect".into());
                }
                CarEffect::SpawnObject { kind, pos, velocity, owner } => {
                    self.log.push(format!("legacy SpawnObject {kind} at {pos:?} from {from}"));
                    let _ = (velocity, owner);
                }
                CarEffect::Other { tag, .. } => {
                    self.abil.unsupported.insert(format!("Other:{tag}"));
                }
            }
        }
    }

    /// `CCar::AddImpactDamage` of kart `car`: the victim's shields absorb first, then bodywork damage.
    fn damage_car(&mut self, car: usize, amount: f32, source: Option<usize>) {
        if self.abil.invuln[car] > 0.0 {
            return;
        }
        let kart = self.kart_state(car);
        let mut out = Vec::new();
        let mut d = amount;
        if let Some(mut slot) = self.take_slot(car) {
            d = slot.absorb_damage(amount, false, &kart, &mut out);
            self.put_slot(car, slot);
        }
        if !out.is_empty() {
            self.apply_effects(car, out, 0.0);
        }
        if d <= 0.0 {
            return;
        }
        self.abil.damage_applied += d;
        let (pos, rot) = (kart.pos, self.car_ref(car).rb.rot);
        let hit = source.filter(|s| *s < self.rs.cars.len()).map(|s| self.car_ref(s).rb.pos);
        let hit_world = hit.map(|h| (h, pos, rot));
        let mut ev = Vec::new();
        if car == 0 {
            self.extras.bodywork.apply_damage_effect(d, hit_world, &mut [], &mut ev);
            self.extras.damage_events += ev.len() as u32;
        } else if let Some(b) = self.abil.bodyworks[car].as_mut() {
            b.apply_damage_effect(d, hit_world, &mut [], &mut ev);
        }
        self.log.push(format!("t={:.1} kart {car} takes {d:.1} damage from {source:?}", self.rs.clock_s));
    }

    /// `CSmackable::Explode` world effect + the damage of area abilities: velocity change law of `smackables::explosion_velocity` on every
    /// kart (except those immune to explosions) and every loose smackable; `damage` to karts inside `radius`.
    fn explosion(&mut self, center: Vec3, radius: f32, force: f32, damage: f32, source: Option<usize>) {
        self.abil.explosions += 1;
        self.abil.blasts.push(Blast { pos: center, radius: radius.max(3.0).min(30.0), age: 0.0 });
        self.items.explode(center, force);
        let n = self.rs.cars.len();
        for id in 0..n {
            let (_, _, imm_ex) = self.slot_immunities(id);
            let p = self.car_ref(id).rb.pos;
            if force > 0.0 && !imm_ex {
                if let Some(v) = smackables::explosion_velocity(force, p - center) {
                    let car = self.car_mut(id);
                    car.rb.vel += v;
                }
            }
            if damage > 0.0 && radius > 0.0 && Some(id) != source && (p - center).length() <= radius {
                self.damage_car(id, damage, source);
            }
        }
    }

    /// Execute the world requests of kart `id`'s abilities.
    fn apply_requests(&mut self, id: usize, reqs: Vec<WorldRequest>) {
        use glam::Mat4;
        for r in reqs {
            match r {
                WorldRequest::SpawnSmackable { handle, type_id, pos, forward, up, vel, ang_vel, scale, owner, gravity, ignore_owner_explode, explode_power, sleep } => {
                    let fwd = forward.normalize_or_zero();
                    let fwd = if fwd == Vec3::ZERO { Vec3::Z } else { fwd };
                    let up = up.normalize_or_zero();
                    let up = if up == Vec3::ZERO { Vec3::Y } else { up };
                    let right = up.cross(fwd).normalize_or_zero();
                    let up = fwd.cross(right).normalize_or_zero();
                    let world = Mat4::from_cols(right.extend(0.0), up.extend(0.0), fwd.extend(0.0), pos.extend(1.0));
                    if type_id as usize >= crate::items::smackdefs::SMACKABLES.len() {
                        continue;
                    }
                    let i = self.items.spawn_smackable(type_id, world);
                    let s = &mut self.items.smackables[i];
                    s.vel = vel;
                    s.ang_vel = ang_vel;
                    s.scale = if scale > 0.0 { scale } else { 1.0 };
                    s.no_gravity = !gravity;
                    s.asleep = sleep;
                    s.ability_owner = Some(owner);
                    if explode_power > 0.0 {
                        s.explosion_override = Some(explode_power);
                    }
                    let _ = ignore_owner_explode;
                    self.abil.objects.insert(handle, ObjRef { index: i, owner, age: 0.0, last_vy: vel.y });
                    self.abil.spawned += 1;
                }
                WorldRequest::UpdateSmackable { handle, pos, vel, ang_vel, forward_up, scale, gravity, sleep } => {
                    let Some(o) = self.abil.objects.get(&handle).copied() else { continue };
                    let s = &mut self.items.smackables[o.index];
                    if s.state == SmackState::Smashed {
                        continue;
                    }
                    if let Some(p) = pos {
                        s.world.w_axis = p.extend(1.0);
                    }
                    if let Some(v) = vel {
                        s.vel = v;
                    }
                    if let Some(a) = ang_vel {
                        s.ang_vel = a;
                    }
                    if let Some((f, u)) = forward_up {
                        let f = f.normalize_or_zero();
                        let u = u.normalize_or_zero();
                        if f != Vec3::ZERO && u != Vec3::ZERO {
                            let right = u.cross(f).normalize_or_zero();
                            let u = f.cross(right);
                            let p = s.world.w_axis;
                            s.world = Mat4::from_cols(right.extend(0.0), u.extend(0.0), f.extend(0.0), p);
                        }
                    }
                    if let Some(sc) = scale {
                        s.scale = sc.max(0.001);
                    }
                    if let Some(g) = gravity {
                        s.no_gravity = !g;
                    }
                    if let Some(sl) = sleep {
                        s.asleep = sl;
                    }
                }
                WorldRequest::RemoveSmackable { handle, shatter } => {
                    if let Some(o) = self.abil.objects.remove(&handle) {
                        if self.items.smackables[o.index].state != SmackState::Smashed {
                            if shatter {
                                let ev = self.items.smash_index(o.index, 1.0 / 60.0);
                                self.route_item_events(ev);
                            } else {
                                self.items.smackables[o.index].state = SmackState::Smashed;
                            }
                        }
                    }
                }
                WorldRequest::SetShield { handle, owner, pos, radius, enabled, model } => {
                    self.abil.shields.insert(handle, ShieldVis { owner, pos, radius, enabled, model });
                }
                WorldRequest::RemoveShield { handle } => {
                    self.abil.shields.remove(&handle);
                }
                WorldRequest::SetVisual { handle, model, pos, forward, up, scale } => {
                    self.abil.visuals.insert(handle, VisualVis { model, pos, forward, up, scale });
                }
                WorldRequest::RemoveVisual { handle } => {
                    self.abil.visuals.remove(&handle);
                }
            }
        }
        let _ = id;
    }

    /// Visual requests of the abilities' `render_visuals` (the `OnCarRender` models) refreshed once per frame.
    fn refresh_visuals(&mut self) {
        let n = self.rs.cars.len();
        let mut reqs = Vec::new();
        for id in 0..n {
            if id == 0 {
                reqs.extend(self.extras.ability.visuals());
            } else if let Some(s) = self.abil.slots[id].as_ref() {
                reqs.extend(s.visuals());
            }
        }
        for r in reqs {
            match r {
                WorldRequest::SetShield { handle, owner, pos, radius, enabled, model } => {
                    self.abil.shields.insert(handle, ShieldVis { owner, pos, radius, enabled, model });
                }
                WorldRequest::SetVisual { handle, model, pos, forward, up, scale } => {
                    self.abil.visuals.insert(handle, VisualVis { model, pos, forward, up, scale });
                }
                WorldRequest::RemoveShield { handle } => {
                    self.abil.shields.remove(&handle);
                }
                WorldRequest::RemoveVisual { handle } => {
                    self.abil.visuals.remove(&handle);
                }
                _ => {}
            }
        }
    }

    /// Deliver a world callback to the abilities of kart `owner` and apply what comes back.
    fn deliver_event(&mut self, owner: usize, ev: WorldEvent) {
        let kart = self.kart_state(owner);
        let Some(mut slot) = self.take_slot(owner) else { return };
        let mut out = Vec::new();
        slot.deliver(&ev, &kart, &mut out);
        let reqs = slot.drain_requests();
        self.put_slot(owner, slot);
        self.apply_effects(owner, out, 0.0);
        self.apply_requests(owner, reqs);
    }

    /// Kart contacts with ability-owned smackables (`CSmackable::CollisionCallback` + `ObjectCollisionCallback`), ground contacts and
    /// smash notifications. Runs after the item world moved the bodies.
    pub(super) fn ability_contacts(&mut self, dt: f32) {
        if self.abil.objects.is_empty() {
            return;
        }
        let n = self.rs.cars.len();
        let handles: Vec<u32> = self.abil.objects.keys().copied().collect();
        for h in handles {
            let Some(mut o) = self.abil.objects.get(&h).copied() else { continue };
            o.age += dt;
            let idx = o.index;
            if self.items.smackables[idx].state == SmackState::Smashed {
                self.abil.objects.remove(&h);
                let pos = self.items.smackables[idx].center();
                self.deliver_event(o.owner, WorldEvent::SmackableSmashed { handle: h, pos });
                continue;
            }
            // ground contact: the vertical speed turned around since last frame
            {
                let s = &self.items.smackables[idx];
                let vy = s.vel.y;
                if o.last_vy < -1.0 && vy > o.last_vy * 0.9 && !s.no_gravity {
                    let pos = s.world.w_axis.truncate();
                    let speed = -o.last_vy;
                    self.abil.objects.insert(h, ObjRef { last_vy: vy, ..o });
                    self.deliver_event(o.owner, WorldEvent::SmackableGround { handle: h, pos, normal: Vec3::Y, speed });
                    o = match self.abil.objects.get(&h).copied() {
                        Some(x) => x,
                        None => continue,
                    };
                }
                o.last_vy = self.items.smackables[o.index].vel.y;
            }
            self.abil.objects.insert(h, o);
            if self.items.smackables[idx].asleep {
                continue;
            }
            for car in 0..n {
                if car == o.owner && o.age < OWNER_GRACE {
                    continue;
                }
                let (kpos, kvel, mass, fwd, up, rot) = {
                    let c = self.car_ref(car);
                    let (_, up, _) = c.rb.axes();
                    (c.rb.pos, c.rb.vel, c.spec.m_fMass, c.forward(), up, c.rb.rot)
                };
                if self.abil.invuln[car] > 0.0 {
                    continue;
                }
                let Some(c) = smackables::kart_contact(&self.items.smackables[idx], kpos, kvel, 1.2) else { continue };
                let def = *self.items.smackables[idx].def();
                let fixed = self.items.smackables[idx].fixed;
                let mag = smackables::impact_magnitude(&def, c.closing_speed.max(0.0));
                let dv = smackables::kart_response(&def, fixed, &c, mass);
                self.car_mut(car).rb.vel += dv;
                if !fixed && c.closing_speed > 0.0 {
                    let push = -c.normal * (c.closing_speed * (1.0 + def.restitution) * mass / (mass + def.mass));
                    self.items.smackables[idx].vel += push;
                }
                self.items.smackables[idx].accum += mag;
                self.abil.hits += 1;
                let (svel, spos) = (self.items.smackables[idx].vel, self.items.smackables[idx].center());
                if car == 0 {
                    let tid = self.items.smackables[idx].type_id;
                    let ejected = self.extras.on_smackable_hit(dv, tid, kpos, fwd, up, rot, mass);
                    if ejected {
                        self.log.push("pilot ejected by an ability object".into());
                    }
                }
                self.deliver_event(o.owner, WorldEvent::SmackableHit { handle: h, car: Some(car), pos: spos, vel: svel, impulse: mag });
                if self.items.smackables[idx].state != SmackState::Smashed && self.items.smackables[idx].should_smash() {
                    let ev = self.items.smash_index(idx, dt);
                    self.route_item_events(ev);
                }
                if self.items.smackables[idx].state == SmackState::Smashed {
                    break;
                }
            }
        }
    }

    /// Item-world events of smashes triggered by the host (explosions push karts).
    fn route_item_events(&mut self, events: Vec<ItemEvent>) {
        for e in events {
            if let ItemEvent::Explosion { pos, strength } = e {
                self.explosion(pos, 0.0, strength, 0.0, None);
            }
        }
    }

    /// Spin-out torque (`CCar::IntegrateSteering` Spin360 term): yaw angular velocity `f^2 * 3 * spins * 2pi / T` about the car's up axis, f = time left / T.
    pub(super) fn apply_spin(&mut self, id: usize) {
        let Some(s) = self.abil.spins.get(id).copied().flatten() else { return };
        let f = (s.left / s.total).clamp(0.0, 1.0);
        let w = f * f * 3.0 * s.spins * std::f32::consts::TAU / s.total;
        let car = self.car_mut(id);
        let (_, up, _) = car.rb.axes();
        let along = car.rb.ang_vel.dot(up);
        car.rb.ang_vel += up * (w - along);
    }

    /// Everything the abilities draw: explosion shells, shield bubbles, loose visual models.
    pub(super) fn ability_draws(&self, out: &mut Vec<crate::render3d::Draw3d>) {
        use glam::Mat4;
        let Some(sphere) = self.abil.sphere else { return };
        for b in &self.abil.blasts {
            let t = (b.age / 0.6).clamp(0.0, 1.0);
            let r = b.radius * (0.25 + 0.75 * (1.0 - (1.0 - t) * (1.0 - t)));
            let fade = 1.0 - t;
            out.push(crate::render3d::Draw3d { mesh: sphere, model: Mat4::from_translation(b.pos) * Mat4::from_scale(Vec3::splat(r)), tint: [1.0, 0.45 + 0.4 * fade, 0.1, 1.0] });
        }
        for s in self.abil.shields.values() {
            if !s.enabled || s.radius <= 0.0 {
                continue;
            }
            // three thin rings read as a bubble without blending
            for axis in 0..3 {
                let rot = match axis {
                    0 => glam::Quat::IDENTITY,
                    1 => glam::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    _ => glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                };
                out.push(crate::render3d::Draw3d {
                    mesh: sphere,
                    model: Mat4::from_translation(s.pos) * Mat4::from_quat(rot) * Mat4::from_scale(Vec3::new(s.radius, s.radius, 0.04)),
                    tint: [0.5, 0.85, 1.0, 1.0],
                });
            }
        }
    }
}

