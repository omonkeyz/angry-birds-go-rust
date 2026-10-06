//! Hal boomerang: CHalAbility (player) and CHalBossAbility (boss)
//!
//! `CHalAbility` (vtable id 0xf, "HalAbility", green bird). The boomerang is NOT a rigid body / smackable: it is a point that
//! `OnCarUpdate` moves along the race `CSpline` (distance += dt * speed, lateral offset kept, height above the track decaying), a
//! particle effect is moved to it every physics step (`OnCarIntegrate`) and every other car that comes within `Radius` of it (once
//! per car) takes `AddImpactDamage(Damage)` + `Spin360`. So the port needs the car's spline, which the host passes in
//! `AbilityEnv::car_spline / car_spline_index / car_spline_lateral` (contract addition of this port).
//!
//! xml tags (char_011.xml, order of the loader `LoadAbilityValuesFromXML @001c2e14`, all `GetAbilityFloatForLevel` with default 0):
//! `Radius`->+0xec (+0xf0 = Radius^2), `Damage`->+0xf4, `AddSpeedOnTrigger`->+0xfc, `DescendingSpeed`->+0x100,
//! `SpinTime`->+0x104, `Spins`->+0x108; then the base loader (Duration, BoostStrength 2200, BoostDuration 2.0, AIActivationTime).
//! The tag <-> field mapping is by order and by use (AddSpeedOnTrigger = extra boomerang speed over the car speed, DescendingSpeed
//! = how fast the height above the track shrinks) and is certain except for `SpinTime` / `Spins` (see UNRESOLVED below).
//!
//! UNRESOLVED: the original `CCar::Spin360` call is decompiled as `Spin360(victim, <junk>, <junk>, *(this+0x108))`; `+0x104`
//! (`SpinTime`) is never read anywhere in Hal, so it must be the lost second float argument (`CCar+0x4a8/+0x4ac` = spin time, read from
//! r2 in `Spin360`) and `+0x108` (`Spins`) the third (`CCar+0x4b0 = Spins * 3.0` = spin strength). [`CarEffect::SpinOut`] is emitted
//! with `time = SpinTime`, `spins = Spins` (raw xml values; the host maps `spins` to `4b0 = 3 * spins`).
//! UNRESOLVED: `AddImpactDamage` is emitted as [`CarEffect::Damage`] (no hit position in the effect, the original passes the
//! boomerang position and a "directional" flag to split the damage between the four sides of the car).

use super::*;
use crate::items::spline::CSpline;
use std::sync::Arc;

/// `DAT_001c2c28`: the global "explode power" set before the rigid-body sweep.
const HAL_SWEEP_POWER: f32 = 60.0;
/// `DAT_001c2c2c`: seconds between two sweeps.
const HAL_SWEEP_INTERVAL: f32 = 0.1;
/// 0x3a83126f: `+0x10c` after the trigger (0.001: the first hit sweeps at once).
const HAL_SWEEP_INITIAL: f32 = 0.001;
/// `CSmackable::ApplyExplodeForce` only pushes a body when `|diff| * power / max(d^2, 4) > sqrt(10)`: for `power = 60` that is
/// `d < 60 / sqrt(10)`.
const HAL_SWEEP_RADIUS: f32 = 18.973665;
/// `DAT_001c2744`: the minimum track half width for the lateral normalisation (1e-5).
const MIN_WIDTH: f32 = 1e-5;
/// `DAT_001c242c`: fraction of the boomerang flight the AI chance looks ahead.
const AI_LOOKAHEAD_FRACTION: f32 = 0.9;

/// `CSpline::GetSplinePosFromDistance(float) @0018fb80`: fractional point index of the spline distance `d` (binary search for the first
/// point with `dist >= d`, then `(d - dist[prev]) / seg_len` clamped to 0..1; 0.0 before the first point).
pub fn spline_pos_from_distance(s: &CSpline, d: f32) -> f32 {
    let lo = s.points.partition_point(|p| p.dist < d);
    if lo == 0 {
        return 0.0;
    }
    let seg = &s.points[lo - 1];
    let f = (d - seg.dist) * (1.0 / seg.seg_len);
    (lo - 1) as f32 + f.clamp(0.0, 1.0)
}

/// `CSpline::GetUpVectorInterpolated(float) @0018fe14`: the point normals lerped by the fraction (indices clamped to `count - 1`).
pub fn spline_up_interpolated(s: &CSpline, t: f32) -> Vec3 {
    let n = s.points.len() as i64;
    let i = (t as i64).clamp(0, n - 1);
    let j = (i + 1).min(n - 1);
    let f = (t - i as f32).clamp(0.0, 1.0);
    let a = s.points[i as usize].normal;
    let b = s.points[j as usize].normal;
    a + (b - a) * f
}

/// Lateral offset in metres of the normalised lateral `lat` at spline index `t`: `lat * (lat <= 0 ? LeftWidth : RightWidth)`.
fn lateral_offset(s: &CSpline, t: f32, lat: f32) -> f32 {
    let w = if lat <= 0.0 { s.left_width(t) } else { s.right_width(t) };
    lat * w
}

/// `CHalAbility` (vtable id 0xf): the green bird's boomerang.
pub struct HalAbility {
    pub base: AbilityBase,
    /// +0x98..+0xb4: the 8 cars already hit by this throw (`memset(this+0x98, 0, 0x20)` on trigger).
    pub hit: [Option<CarId>; 8],
    /// +0xb8 the car's spline captured at trigger.
    pub spline: Option<Arc<CSpline>>,
    /// +0xbc spline distance of the boomerang (m).
    pub dist: f32,
    /// +0xc0 lateral position as a fraction of the track half width (-1 left .. 1 right).
    pub lateral: f32,
    /// +0xc4 signed height above the track surface (m); shrinks by `DescendingSpeed * dt` while positive.
    pub height: f32,
    /// +0xc8..+0xd0 boomerang world position, +0xd4..+0xdc its velocity.
    pub pos: Vec3,
    pub vel: Vec3,
    /// +0xe0..+0xe8 track up vector under the boomerang.
    pub up: Vec3,
    /// +0xec `<Radius>` (ctor 1.0) and +0xf0 `Radius^2` (ctor 1.0).
    pub radius: f32,
    pub radius2: f32,
    /// +0xf4 `<Damage>` (ctor 10.0).
    pub damage: f32,
    /// +0xf8 boomerang speed along the spline: car speed at trigger + `AddSpeedOnTrigger` (ctor 1.0).
    pub speed: f32,
    /// +0xfc `<AddSpeedOnTrigger>` (ctor 20.0).
    pub add_speed: f32,
    /// +0x100 `<DescendingSpeed>` (ctor 1.0).
    pub descending_speed: f32,
    /// +0x104 `<SpinTime>` (ctor 1.0), +0x108 `<Spins>` (ctor 1.0).
    pub spin_time: f32,
    pub spins: f32,
    /// +0x10c rigid-body sweep timer (ctor -1.0; 0.001 on trigger; reset to -1.0 by `StopEffects`).
    pub sweep_timer: f32,
    /// The kart as the host last showed it (`CalcCurrentAITriggerChance` takes no argument in the Rust trait, the original reads the car).
    last_kart: Option<KartState>,
}

impl HalAbility {
    /// `CHalAbility::CHalAbility(CCar*) @001c2448`.
    pub fn new(level: f32) -> HalAbility {
        HalAbility {
            base: AbilityBase::new(level),
            hit: [None; 8],
            spline: None,
            dist: 0.0,
            lateral: 0.0,
            height: 0.0,
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            up: Vec3::ZERO,
            radius: 1.0,
            radius2: 1.0,
            damage: 10.0,
            speed: 1.0,
            add_speed: 20.0,
            descending_speed: 1.0,
            spin_time: 1.0,
            spins: 1.0,
            sweep_timer: -1.0,
            last_kart: None,
        }
    }

    /// The particle effect move of `OnCarIntegrate @001c2ce4` / `StopEffects @001c2efc`: the matrix is built from the track up vector
    /// (`+0xe0`) with the boomerang position (`+0xc8`); `vals` carry the up vector.
    fn move_effect(&self, tag: &'static str, kart: &KartState, out: &mut Vec<CarEffect>) {
        out.push(CarEffect::Other { tag, car: Some(kart.id), vals: [self.up.x, self.up.y, self.up.z, 0.0], pos: self.pos });
    }

    /// The hit-list logic of `OnCarUpdate`: false when the car is already in the list, else it is stored in the first free slot
    /// (the 8th slot is only filled when free; a 9th distinct car is still hit but not remembered).
    fn register_hit(&mut self, car: CarId) -> bool {
        for i in 0..8 {
            match self.hit[i] {
                None => {
                    self.hit[i] = Some(car);
                    return true;
                }
                Some(c) if c == car => return false,
                _ => {}
            }
        }
        true
    }
}

impl Ability for HalAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::Hal
    }
    /// `CHalAbility::GetAbilityName @001c21fc` (a literal, "HalAbility" = the xml `name`).
    fn name(&self) -> &str {
        "HalAbility"
    }

    /// `CHalAbility::LoadAbilityValuesFromXML @001c2e14`, then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.radius = p.float_for_level("Radius", 0.0, lvl);
        self.radius2 = self.radius * self.radius;
        self.damage = p.float_for_level("Damage", 0.0, lvl);
        self.add_speed = p.float_for_level("AddSpeedOnTrigger", 0.0, lvl);
        self.descending_speed = p.float_for_level("DescendingSpeed", 0.0, lvl);
        self.spin_time = p.float_for_level("SpinTime", 0.0, lvl);
        self.spins = p.float_for_level("Spins", 0.0, lvl);
        base_load_values(self, p);
    }

    /// `CHalAbility::TriggerAbility @001c24e8`: clears the hit list, starts the boomerang at the car (position, velocity = the car's),
    /// captures the car's spline, `speed = car speed + AddSpeedOnTrigger`, the spline distance of the car, the car's lateral position
    /// as a fraction of the half width, the up vector there, the signed height of the car above the spline-lateral point, arms the
    /// sweep timer (0.001), then `CBaseAbility::TriggerAbility`.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.hit = [None; 8];
        self.pos = kart.pos;
        self.vel = kart.velocity;
        self.spline = self.base.env.car_spline.clone();
        self.speed = kart.speed + self.add_speed;
        if let Some(sp) = self.spline.clone() {
            if !sp.points.is_empty() {
                let t = self.base.env.car_spline_index;
                let i = sp.clamp_index(t);
                self.dist = sp.points[i].dist + (t - t.trunc()) * sp.points[i].seg_len;
                let lat = self.base.env.car_spline_lateral;
                let w = if lat <= 0.0 { sp.left_width(t) } else { sp.right_width(t) };
                self.lateral = if w > MIN_WIDTH { lat / w } else { 0.0 };
                self.up = spline_up_interpolated(&sp, t);
                let on_track = sp.position(t) + sp.points[i].right * lateral_offset(&sp, t, self.lateral);
                let diff = kart.pos - on_track;
                let len = diff.length();
                let sign = if len > 0.0 && (diff / len).dot(self.up) < 0.0 { -1.0 } else { 1.0 };
                self.height = sign * len;
            }
        }
        self.sweep_timer = HAL_SWEEP_INITIAL;
        base_trigger(self, kart, out);
    }

    /// `CHalAbility::OnCarUpdate @001c274c`: advance along the spline; past the end the ability is ended (`effect_timer = time = -1`,
    /// base update finishes it). Otherwise the position is re-derived (spline point at the new distance + lateral * width along the
    /// point's right vector + height along the interpolated up vector), the velocity is the position delta / dt, and every hittable
    /// car (not the owner, not a team mate in team mode) inside `Radius` that is not yet in the hit list takes `Damage` + a spin; the
    /// first hit of each 0.1 s also sweeps the rigid bodies (`CSmackable::ApplyExplodeForce` with power 60 around the boomerang).
    fn update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        let Some(sp) = self.spline.clone() else {
            self.base.effect_timer = -1.0;
            self.base.time_remaining = -1.0;
            base_update(self, dt, kart, out);
            return;
        };
        self.dist += dt * self.speed;
        if sp.length <= self.dist || sp.points.is_empty() {
            self.base.effect_timer = -1.0;
            self.base.time_remaining = -1.0;
            base_update(self, dt, kart, out);
            return;
        }
        let t = spline_pos_from_distance(&sp, self.dist);
        let off = lateral_offset(&sp, t, self.lateral);
        let right = sp.points[sp.clamp_index(t)].right;
        if self.height > 0.0 {
            self.height = (self.height - dt * self.descending_speed).max(0.0);
        }
        self.up = spline_up_interpolated(&sp, t);
        let old = self.pos;
        self.pos = sp.position(t) + right * off + self.up * self.height;
        if dt > 0.0 {
            self.vel = (self.pos - old) / dt;
        }
        let others = self.base.env.others.clone();
        let team_mode = self.base.env.team_mode;
        let my_team = self.base.env.team;
        for o in others.iter() {
            if o.id == kart.id || !o.hittable {
                continue;
            }
            if team_mode && o.team == my_team {
                continue;
            }
            if (self.pos - o.pos).length_squared() > self.radius2 {
                continue;
            }
            if !self.register_hit(o.id) {
                continue;
            }
            // CCar::AddImpactDamage(victim, boomerang pos, Damage); (player owner: victim+0x1b10 = 0.0 bookkeeping not modelled)
            out.push(CarEffect::Damage { car: o.id, amount: self.damage, source: Some(kart.id) });
            // CCar::Spin360 (skipped by the victim's ability when it ImmuneToSpin)
            if !o.immune_spin {
                out.push(CarEffect::SpinOut { car: o.id, time: self.spin_time, spins: self.spins });
            }
            // (player owner: CChallengeManager::Event - not modelled)
            if self.sweep_timer > 0.0 {
                self.sweep_timer -= dt;
                if self.sweep_timer <= 0.0 {
                    out.push(CarEffect::Explosion { center: self.pos, radius: HAL_SWEEP_RADIUS, force: HAL_SWEEP_POWER, damage: 0.0, source: Some(kart.id) });
                    self.sweep_timer = HAL_SWEEP_INTERVAL;
                }
            }
        }
        base_update(self, dt, kart, out);
    }

    /// `CHalAbility::OnCarIntegrate @001c2ce4`: base integrate, then while the start or end effect lives the particle effect(s) follow
    /// the boomerang.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_on_integrate(self, kart, out);
        if self.base.fx_alive || self.base.end_fx_alive {
            if self.base.fx_alive {
                self.move_effect("CXGSParticleEffectManager::MoveEffect(Hal start effect -> boomerang)", kart, out);
            }
            if self.base.end_fx_alive {
                self.move_effect("CXGSParticleEffectManager::MoveEffect(Hal end effect)", kart, out);
            }
        }
    }

    /// `CBaseAbility::OnCarAlwaysUpdate` plus the kart snapshot for the AI chance.
    fn always_update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.last_kart = Some(kart.clone());
        base_always_update(self, dt, kart, out);
    }

    /// `CHalAbility::StopEffects @001c2efc`: base stop, the end effect is placed at the boomerang, and the sweep timer is disarmed (-1).
    fn stop_effects(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_stop_effects(self, kart, out);
        if self.base.end_fx_alive {
            self.move_effect("CXGSParticleEffectManager::MoveEffect(Hal end effect)", kart, out);
        }
        self.sweep_timer = -1.0;
    }

    /// `CHalAbility::CalcCurrentAITriggerChance @001c226c`: the boomerang is predicted to fly
    /// `(car speed + AddSpeedOnTrigger) * Duration * 0.9` along the spline from the car; every other car (penalty timer <= 0) whose
    /// spline distance lies between `mine + 1.0` and that end point adds `1 / (cars - 1)`; a team mate in the way makes it 0.
    /// (The original also requires `CCar::CheckIfOverlappingSpline` when the other car is on another spline; the host exposes one
    /// distance per car, so cars are assumed on the same spline - UNRESOLVED.)
    fn ai_trigger_chance(&self) -> f32 {
        let Some(k) = self.last_kart.as_ref() else { return 0.0 };
        let env = &self.base.env;
        let mine = k.spline_distance;
        let end = mine + (k.speed + self.add_speed) * self.base.duration * AI_LOOKAHEAD_FRACTION;
        let n = env.others.len() as f32; // cars - 1
        let mut chance = 0.0;
        for o in env.others.iter() {
            if o.penalty <= 0.0 && end > o.spline_distance && mine + 1.0 < o.spline_distance {
                if env.team_mode && o.team == env.team {
                    return 0.0;
                }
                chance += 1.0 / n;
            }
        }
        chance
    }
}

/// STUB (replace with the ported class).
pub struct HalBossAbility {
    pub base: AbilityBase,
}

impl HalBossAbility {
    pub fn new(level: f32) -> HalBossAbility {
        HalBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for HalBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::HalBoss
    }
    fn name(&self) -> &str {
        "HalBossAbility"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::spline::SplinePoint;

    fn straight_spline(n: usize, step: f32) -> CSpline {
        let mut points = Vec::new();
        for i in 0..n {
            points.push(SplinePoint {
                pos: Vec3::new(0.0, 0.0, i as f32 * step),
                normal: Vec3::Y,
                right: Vec3::Y.cross(Vec3::Z),
                tangent: Vec3::Z,
                seg_len: step,
                dist: i as f32 * step,
                left_w: 10.0,
                right_w: 10.0,
            });
        }
        CSpline { name: "t".into(), points, length: (n - 1) as f32 * step }
    }

    fn hal() -> Option<HalAbility> {
        let xml = std::fs::read_to_string(format!("{}/xml/characters/charxml/char_011.xml", ASSETS292)).ok()?;
        let p = AbilityParams::from_character_xml(&xml)?;
        let mut a = HalAbility::new(0.0);
        a.load_values(&p);
        a.base.env.effects = p.effects.clone();
        Some(a)
    }

    fn other(id: CarId, pos: Vec3) -> OtherCar {
        OtherCar {
            id,
            pos,
            vel: Vec3::ZERO,
            forward: Vec3::Z,
            up: Vec3::Y,
            team: id as i32,
            is_player: false,
            hittable: true,
            progress_value: 0.0,
            penalty: 0.0,
            spline_distance: pos.z,
            race_position: 2,
            immune_spin: false,
            immune_damage: false,
            immune_explosions: false,
        }
    }

    #[test]
    fn values_from_xml() {
        let Some(a) = hal() else { return };
        assert_eq!(a.radius, 8.0);
        assert_eq!(a.radius2, 64.0);
        assert_eq!(a.damage, 8.0);
        assert_eq!(a.add_speed, 20.0);
        assert_eq!(a.descending_speed, 10.0);
        assert_eq!((a.spin_time, a.spins), (2.0, 1.0));
        assert_eq!(a.base.duration, 3.0);
        assert_eq!(a.base.boost_strength, 2200.0);
        assert_eq!(a.base.boost_duration, 2.0);
        assert_eq!((a.base.ai_min_time, a.base.ai_max_time), (10.0, 60.0));
        assert_eq!(a.base.charges, 1);
        assert_eq!(a.id(), BirdAbility::Hal);
    }

    #[test]
    fn spline_helpers() {
        let s = straight_spline(10, 10.0);
        assert_eq!(spline_pos_from_distance(&s, 0.0), 0.0);
        assert!((spline_pos_from_distance(&s, 25.0) - 2.5).abs() < 1e-5);
        assert!((spline_pos_from_distance(&s, 30.0) - 3.0).abs() < 1e-5);
        assert_eq!(spline_up_interpolated(&s, 2.5), Vec3::Y);
    }

    #[test]
    fn boomerang_flies_along_spline_and_hits_once() {
        let Some(mut a) = hal() else { return };
        let sp = Arc::new(straight_spline(100, 10.0));
        a.base.env.car_spline = Some(sp.clone());
        a.base.env.car_spline_index = 2.0; // z = 20
        a.base.env.car_spline_lateral = 0.0;
        let kart = KartState { id: 1, is_player: true, pos: Vec3::new(0.0, 1.5, 20.0), velocity: Vec3::new(0.0, 0.0, 30.0), speed: 30.0, spline_distance: 20.0, ..Default::default() };
        a.base.env.others = vec![other(2, Vec3::new(0.0, 0.0, 70.0)), other(3, Vec3::new(40.0, 0.0, 70.0))];
        let mut out = Vec::new();
        assert!(a.can_trigger(&kart));
        a.trigger_player(&kart, &mut out);
        assert!(a.base.active);
        assert_eq!(a.speed, 50.0); // car speed 30 + AddSpeedOnTrigger 20
        assert_eq!(a.dist, 20.0);
        assert!((a.height - 1.5).abs() < 1e-5);
        assert_eq!(a.sweep_timer, HAL_SWEEP_INITIAL);
        assert!((a.ai_trigger_chance()).abs() >= 0.0);
        out.clear();
        let dt = 1.0 / 30.0;
        let mut spins = 0;
        let mut dmg = 0;
        let mut sweeps = 0;
        for _ in 0..100 {
            tick_ability(&mut a, dt, &kart, &mut out);
            if !a.base.active {
                break;
            }
        }
        for e in &out {
            match e {
                CarEffect::Damage { car, amount, source } => {
                    assert_eq!(*car, 2);
                    assert_eq!(*amount, 8.0);
                    assert_eq!(*source, Some(1));
                    dmg += 1;
                }
                CarEffect::SpinOut { car, time, spins: s } => {
                    assert_eq!(*car, 2);
                    assert_eq!((*time, *s), (2.0, 1.0));
                    spins += 1;
                }
                CarEffect::Explosion { .. } => sweeps += 1,
                _ => {}
            }
        }
        assert_eq!((dmg, spins, sweeps), (1, 1, 1)); // car 2 only, once; car 3 is 40 m off the line
        // the boomerang ran out after Duration (3 s) or the spline end, whichever first
        assert!(!a.base.active);
        assert_eq!(a.sweep_timer, -1.0); // StopEffects disarms the sweep
    }

    #[test]
    fn height_descends_and_ability_ends_at_spline_end() {
        let Some(mut a) = hal() else { return };
        a.base.env.car_spline = Some(Arc::new(straight_spline(5, 10.0))); // 40 m long
        a.base.env.car_spline_index = 3.0;
        let kart = KartState { id: 1, pos: Vec3::new(0.0, 3.0, 30.0), speed: 10.0, ..Default::default() };
        let mut out = Vec::new();
        a.trigger(&kart, &mut out);
        assert!((a.height - 3.0).abs() < 1e-5);
        a.update(0.1, &kart, &mut out);
        assert!((a.height - 2.0).abs() < 1e-4); // DescendingSpeed 10 * 0.1
        assert!((a.pos.z - (30.0 + 0.1 * 30.0)).abs() < 1e-3);
        a.update(0.5, &kart, &mut out); // 30 + 3 + 15 > 40: past the end
        assert_eq!(a.base.time_remaining, -1.0); // finished (base finish stores -1)
        assert!(!a.base.active);
    }

    #[test]
    fn ai_chance_counts_cars_ahead_in_reach() {
        let Some(mut a) = hal() else { return };
        let kart = KartState { id: 1, speed: 30.0, spline_distance: 100.0, ..Default::default() };
        a.base.env.others = vec![other(2, Vec3::new(0.0, 0.0, 150.0)), other(3, Vec3::new(0.0, 0.0, 400.0)), other(4, Vec3::new(0.0, 0.0, 50.0))];
        let mut out = Vec::new();
        a.always_update(0.0, &kart, &mut out);
        // reach: 100 + (30 + 20) * 3 * 0.9 = 235 -> only car 2 (1 of 3 others)
        assert!((a.ai_trigger_chance() - 1.0 / 3.0).abs() < 1e-6);
    }
}
