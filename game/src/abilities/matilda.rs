//! Matilda egg: CMatildaAbility (player) and CMatildaBossAbility (boss)
//!
//! `CMatildaAbility` (id 0x10, "MatildaAbility", white bird). Trigger: a smackable of type 0x5b (`matildaegg_whole`) is created at the
//! car (offset `SpawnPositionAbove` up / `SpawnPositionBehind` back) and held there - every physics step `OnCarIntegrate` puts its
//! rigid body back at that point while the start effect is alive. After `ReleaseDelay` seconds (`OnCarUpdate` counts `+0xc8` down)
//! `ThrowObject` gives it a velocity (car velocity + `ForwardSpeed` along the car forward + `UpwardSpeed` along the car up) and a
//! random spin of `ObjectSpinSpeed`. When the egg is smashed (`CSmackableManager::RegisterCallbackOnSmashed` ->
//! `ObjectSmashedCallback`, here [`WorldEvent::SmackableSmashed`]) every other hittable car within `Radius` of the egg takes
//! `AddImpactDamage(Damage)` and `Spin360`. The smackable's own explosion strength (`smackable+0x1168`) is set to `ExplosiveForce`
//! (`WorldRequest::SpawnSmackable::explode_power`) and the owner ignores it (`CCar::SetupToIgnoreExplodeForce`).
//!
//! xml tags (char_007.xml; the loader `LoadAbilityValuesFromXML @001c63e8` reads them in xml order, `GetAbilityFloatForLevel`
//! default 0): `Radius`->+0xa8 (+0xac = Radius^2), `ExplosiveForce`->+0xb0, `Damage`->+0xb4, `SpinTime`->+0xb8, `Spins`->+0xbc,
//! `ForwardSpeed`->+0x98, `UpwardSpeed`->+0x9c, `SpawnPositionBehind`->+0xa0, `SpawnPositionAbove`->+0xa4, `ReleaseDelay`->+0xc4,
//! `ObjectSpinSpeed`->+0xc0, text `TrailEffect`->+0xd4 (a particle effect that follows the thrown egg). `Duration` 1.0,
//! `EffectDuration` 1.0, `BoostStrength` 2200, `BoostDuration` 2.0, `CamBehindMod` 0 go to the base.
//!
//! UNRESOLVED: `Spin360` (see hal.rs): `SpinTime` is the lost time argument, `Spins` the passed one.
//! UNRESOLVED: `smackable+0x116c = 1`, `+0x1174 = 0` (flags set right after `AddSmackable`) and `rb+0x300 = 1` (set every step while
//! the egg is held) are not in the world contract; the held egg is modelled as `gravity = false` + position/velocity follow, and
//! gravity is switched on when it is thrown.

use super::*;

/// Smackable type of the egg (`AddSmackable(0x5b, ..)`; `smackdefs` "matildaegg_whole").
pub const MATILDA_EGG_TYPE: u32 = 0x5b;

/// The "hold point" every step: car position + up * above - forward * behind (the car matrix times `Translation(0, above, -behind)`).
pub(super) fn hold_point(kart: &KartState, above: f32, behind: f32) -> Vec3 {
    kart.pos + kart.up * above - kart.forward * behind
}

/// `AddSmackable(type, car matrix * Translation(0, above, -behind))` + `SetupToIgnoreExplodeForce(car)`; returns the handle.
pub(super) fn spawn_held(base: &mut AbilityBase, kart: &KartState, type_id: u32, above: f32, behind: f32, explode_power: f32) -> u32 {
    let handle = base.alloc_handle(kart.id);
    base.request(WorldRequest::SpawnSmackable {
        handle,
        type_id,
        pos: hold_point(kart, above, behind),
        forward: kart.forward,
        up: kart.up,
        vel: kart.velocity,
        ang_vel: Vec3::ZERO,
        scale: 1.0,
        owner: kart.id,
        gravity: false,
        ignore_owner_explode: true,
        explode_power: if explode_power > 0.0 { explode_power } else { 0.0 },
        sleep: false,
    });
    handle
}

/// The per-step hold of `OnCarIntegrate` (`rb pos = hold point; SetPosition; SetSleep(0); rb+0x300 = 1`).
pub(super) fn hold_request(handle: u32, kart: &KartState, above: f32, behind: f32) -> WorldRequest {
    WorldRequest::UpdateSmackable {
        handle,
        pos: Some(hold_point(kart, above, behind)),
        vel: Some(kart.velocity),
        ang_vel: None,
        forward_up: None,
        scale: None,
        gravity: Some(false),
        sleep: Some(false),
    }
}

/// The body of `ThrowObject` that is common to Matilda and Moustache: the smackable's velocity becomes the car velocity +
/// `forward_speed * car forward + up_speed * car up`, its angular velocity a random unit vector (`Range(-1, 1)` for one component,
/// `Range(-pi, pi)` for the angle around it) times `spin_speed`; it is woken (`SetSleep(0)`).
pub(super) fn throw_request(env: &mut AbilityEnv, handle: u32, kart: &KartState, forward_speed: f32, up_speed: f32, spin_speed: f32) -> WorldRequest {
    let vel = kart.velocity + kart.forward * forward_speed + kart.up * up_speed;
    let z = env.rand01() * 2.0 - 1.0;
    let ang = (env.rand01() * 2.0 - 1.0) * std::f32::consts::PI;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let dir = Vec3::new(r * ang.cos(), r * ang.sin(), z);
    WorldRequest::UpdateSmackable {
        handle,
        pos: None,
        vel: Some(vel),
        ang_vel: Some(dir * spin_speed),
        forward_up: None,
        scale: None,
        gravity: Some(true),
        sleep: Some(false),
    }
}

/// The car loop of `ObjectSmashedCallback` (Matilda @001c5a24, Moustache @001cb0b8): every car except the owner whose
/// non-collide timer has run out (`CCar+0x42c <= 0`), that is not a team mate in team mode, and lies within `sqrt(radius2)` of the
/// smashed object takes `AddImpactDamage(center, Damage)` then `Spin360` (not when the victim's ability is `ImmuneToSpin`).
/// (Owner player: `victim+0x1b10 = 0` and the `CChallengeManager::Event` are bookkeeping that is not modelled.)
pub(super) fn area_hit(env: &AbilityEnv, owner: CarId, center: Vec3, radius2: f32, damage: f32, spin_time: f32, spins: f32, out: &mut Vec<CarEffect>) {
    for o in env.others.iter() {
        if o.id == owner || !o.hittable {
            continue;
        }
        if env.team_mode && o.team == env.team {
            continue;
        }
        if (center - o.pos).length_squared() > radius2 {
            continue;
        }
        out.push(CarEffect::Damage { car: o.id, amount: damage, source: Some(owner) });
        if !o.immune_spin {
            out.push(CarEffect::SpinOut { car: o.id, time: spin_time, spins });
        }
    }
}

/// The AI chance shared by Matilda @001c5d08 and Moustache @001cb3dc: the egg is thrown from the car's position advanced by
/// `velocity * (ReleaseDelay + 0.5) + forward * ForwardSpeed * 0.5`; every other car AHEAD in race progress (`1a8c` strictly greater)
/// whose position advanced by `velocity * (ReleaseDelay + 0.5)` lies within `Radius` of that point adds `1 / (cars - 1)`; a team
/// mate in range makes the result 0.
pub(super) fn thrown_ai_chance(k: &KartState, env: &AbilityEnv, release_delay: f32, forward_speed: f32, radius2: f32) -> f32 {
    let lead = release_delay + 0.5;
    let target = k.pos + k.velocity * lead + k.forward * forward_speed * 0.5;
    let n = env.others.len() as f32;
    let mut chance = 0.0;
    for o in env.others.iter() {
        if o.progress_value > env.progress_value {
            let p = o.pos + o.vel * lead;
            if (p - target).length_squared() <= radius2 {
                if env.team_mode && o.team == env.team {
                    return 0.0;
                }
                chance += 1.0 / n;
            }
        }
    }
    chance
}

/// `CMatildaAbility` (id 0x10): the white bird's egg.
pub struct MatildaAbility {
    pub base: AbilityBase,
    /// +0x98 `<ForwardSpeed>` (ctor 30.0).
    pub forward_speed: f32,
    /// +0x9c `<UpwardSpeed>` (ctor 10.0).
    pub up_speed: f32,
    /// +0xa0 `<SpawnPositionBehind>` (ctor 0), +0xa4 `<SpawnPositionAbove>` (ctor 0).
    pub spawn_behind: f32,
    pub spawn_above: f32,
    /// +0xa8 `<Radius>` (ctor 1.0), +0xac Radius^2 (ctor 1.0).
    pub radius: f32,
    pub radius2: f32,
    /// +0xb0 `<ExplosiveForce>` (ctor 60.0).
    pub explosive_force: f32,
    /// +0xb4 `<Damage>` (ctor 10.0).
    pub damage: f32,
    /// +0xb8 `<SpinTime>` (ctor 2.0), +0xbc `<Spins>` (ctor 2.0).
    pub spin_time: f32,
    pub spins: f32,
    /// +0xc0 `<ObjectSpinSpeed>` (ctor 0).
    pub object_spin_speed: f32,
    /// +0xc4 `<ReleaseDelay>` (ctor 0).
    pub release_delay: f32,
    /// +0xc8 seconds until the throw (ctor -1.0 = none pending).
    pub release_timer: f32,
    /// +0xcc the egg smackable (0 = none); here the world handle.
    pub object: Option<u32>,
    /// +0xd0 trail effect instance alive; +0xd4 `<TrailEffect>` name (empty = none).
    pub trail_alive: bool,
    pub trail_name: String,
    last_kart: Option<KartState>,
}

impl MatildaAbility {
    /// `CMatildaAbility::CMatildaAbility(CCar*) @001c5f3c` (also registers `ObjectSmashedCallback`, which the host does by delivering
    /// [`WorldEvent::SmackableSmashed`] to every ability).
    pub fn new(level: f32) -> MatildaAbility {
        MatildaAbility {
            base: AbilityBase::new(level),
            forward_speed: 30.0,
            up_speed: 10.0,
            spawn_behind: 0.0,
            spawn_above: 0.0,
            radius: 1.0,
            radius2: 1.0,
            explosive_force: 60.0,
            damage: 10.0,
            spin_time: 2.0,
            spins: 2.0,
            object_spin_speed: 0.0,
            release_delay: 0.0,
            release_timer: -1.0,
            object: None,
            trail_alive: false,
            trail_name: String::new(),
            last_kart: None,
        }
    }

    /// `CMatildaAbility::SpawnObject @001c57bc`: the egg (type 0x5b) at car matrix * `Translation(0, above, -behind)`, explosion strength
    /// `ExplosiveForce` when > 0, ignored by the owner.
    pub fn spawn_object(&mut self, kart: &KartState) {
        let h = spawn_held(&mut self.base, kart, MATILDA_EGG_TYPE, self.spawn_above, self.spawn_behind, self.explosive_force);
        self.object = Some(h);
    }

    /// `CMatildaAbility::ThrowObject @001c601c`: removes the start effect (and a left-over trail), spawns the trail effect, and when
    /// the egg exists launches it (see [`throw_request`]) and plays the launch sound (AI / human variant).
    pub fn throw_object(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.base.fx_alive {
            out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(ability)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
            self.base.fx_alive = false;
        }
        if self.trail_alive {
            out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(trail)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
            self.trail_alive = false;
        }
        if !self.trail_name.is_empty() {
            out.push(CarEffect::Particle { name: self.trail_name.clone(), car: None, pos: kart.pos });
            self.trail_alive = true;
        }
        if let Some(h) = self.object {
            let req = throw_request(&mut self.base.env, h, kart, self.forward_speed, self.up_speed, self.object_spin_speed);
            self.base.request(req);
            let name = if kart.is_player { "ABY_abilities_white_launch_Human" } else { "ABY_abilities_white_launch_AI" };
            out.push(CarEffect::Sound { name: name.to_string(), car: Some(kart.id) });
        }
    }
}

impl Ability for MatildaAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::Matilda
    }
    /// `CMatildaAbility::GetAbilityName` (the xml `name`).
    fn name(&self) -> &str {
        "MatildaAbility"
    }

    /// `CMatildaAbility::LoadAbilityValuesFromXML @001c63e8`, then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.radius = p.float_for_level("Radius", 0.0, lvl);
        self.radius2 = self.radius * self.radius;
        self.explosive_force = p.float_for_level("ExplosiveForce", 0.0, lvl);
        self.damage = p.float_for_level("Damage", 0.0, lvl);
        self.spin_time = p.float_for_level("SpinTime", 0.0, lvl);
        self.spins = p.float_for_level("Spins", 0.0, lvl);
        self.forward_speed = p.float_for_level("ForwardSpeed", 0.0, lvl);
        self.up_speed = p.float_for_level("UpwardSpeed", 0.0, lvl);
        self.spawn_behind = p.float_for_level("SpawnPositionBehind", 0.0, lvl);
        self.spawn_above = p.float_for_level("SpawnPositionAbove", 0.0, lvl);
        self.release_delay = p.float_for_level("ReleaseDelay", 0.0, lvl);
        self.object_spin_speed = p.float_for_level("ObjectSpinSpeed", 0.0, lvl);
        if let Some(t) = p.scalar_str("TrailEffect") {
            if !t.is_empty() {
                self.trail_name = t.to_string();
            }
        }
        base_load_values(self, p);
    }

    /// `CMatildaAbility::TriggerAbility @001c65a0`: arms the release timer with `ReleaseDelay`, base trigger, spawns the egg
    /// (`SpawnObject`, virtual slot 0xa0), spawn sounds (AI / human variant) and the voice "hatch".
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.release_timer = self.release_delay;
        base_trigger(self, kart, out);
        self.spawn_object(kart);
        let name = if kart.is_player { "ABY_abilities_white_spawn_Human" } else { "ABY_abilities_white_spawn_AI" };
        out.push(CarEffect::Sound { name: name.to_string(), car: Some(kart.id) });
        out.push(CarEffect::Sound { name: "ABY_voice_matilda_ability_hatch".to_string(), car: Some(kart.id) });
    }

    /// `CMatildaAbility::OnCarUpdate @001c68f8`: the release timer counts down while >= 0 and throws when it goes below 0; then base.
    fn update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.release_timer >= 0.0 {
            self.release_timer -= dt;
            if self.release_timer < 0.0 {
                self.throw_object(kart, out);
            }
        }
        base_update(self, dt, kart, out);
    }

    /// `CMatildaAbility::CanTriggerAbility @001c6948`: no pending release, no egg alive, and the base conditions.
    fn can_trigger(&self, kart: &KartState) -> bool {
        if self.release_timer < 0.0 && self.object.is_none() {
            return base_can_trigger(&self.base, kart);
        }
        false
    }

    /// `CMatildaAbility::OnCarIntegrate @001c6980`: base; while the start effect is alive it follows the car and the held egg is put
    /// back at the hold point; every step the trail effect follows the egg.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_on_integrate(self, kart, out);
        if self.base.fx_alive {
            out.push(CarEffect::Other {
                tag: "CXGSParticleEffectManager::MoveEffect(Matilda start effect)",
                car: Some(kart.id),
                vals: [kart.forward.x, kart.forward.y, kart.forward.z, 0.0],
                pos: hold_point(kart, self.spawn_above, self.spawn_behind),
            });
            match self.object {
                None => return,
                Some(h) => self.base.request(hold_request(h, kart, self.spawn_above, self.spawn_behind)),
            }
        }
        if self.object.is_some() && self.trail_alive {
            out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::MoveEffect(Matilda trail -> egg)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
        }
    }

    /// `CMatildaAbility::FinishAbility @001c6c2c`: base finish, release timer back to -1.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_finish(self, kart, out);
        self.release_timer = -1.0;
    }

    /// `CBaseAbility::OnCarAlwaysUpdate` plus the kart snapshot for the AI chance.
    fn always_update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.last_kart = Some(kart.clone());
        base_always_update(self, dt, kart, out);
    }

    /// `CMatildaAbility::ObjectSmashedCallback @001c5a24`: when the smashed smackable is the egg, the area damage loop
    /// ([`area_hit`], radius^2 `+0xac`, `Damage`, `Spins`) around its position, then the egg slot and the trail effect are cleared.
    fn on_world_event(&mut self, ev: &WorldEvent, kart: &KartState, out: &mut Vec<CarEffect>) {
        let WorldEvent::SmackableSmashed { handle, pos } = ev else { return };
        if self.object != Some(*handle) {
            return;
        }
        area_hit(&self.base.env, kart.id, *pos, self.radius2, self.damage, self.spin_time, self.spins, out);
        self.object = None;
        if self.trail_alive {
            out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(trail)", car: Some(kart.id), vals: [0.0; 4], pos: *pos });
            self.trail_alive = false;
        }
    }

    /// `CMatildaAbility::CalcCurrentAITriggerChance @001c5d08` (see [`thrown_ai_chance`]; the car's progress is
    /// `AbilityEnv::progress_value`, the others' `OtherCar::progress_value`).
    fn ai_trigger_chance(&self) -> f32 {
        match self.last_kart.as_ref() {
            Some(k) => thrown_ai_chance(k, &self.base.env, self.release_delay, self.forward_speed, self.radius2),
            None => 0.0,
        }
    }
}

/// STUB (replace with the ported class).
pub struct MatildaBossAbility {
    pub base: AbilityBase,
}

impl MatildaBossAbility {
    pub fn new(level: f32) -> MatildaBossAbility {
        MatildaBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for MatildaBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::MatildaBoss
    }
    fn name(&self) -> &str {
        "MatildaBossAbility"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matilda() -> Option<MatildaAbility> {
        let xml = std::fs::read_to_string(format!("{}/xml/characters/charxml/char_007.xml", ASSETS292)).ok()?;
        let p = AbilityParams::from_character_xml(&xml)?;
        let mut a = MatildaAbility::new(0.0);
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
            progress_value: 5.0,
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
        let Some(a) = matilda() else { return };
        assert_eq!(a.radius, 15.0);
        assert_eq!(a.radius2, 225.0);
        assert_eq!(a.explosive_force, 600.0);
        assert_eq!(a.damage, 15.0);
        assert_eq!((a.spin_time, a.spins), (3.0, 3.0));
        assert_eq!((a.forward_speed, a.up_speed), (20.0, 9.05));
        assert_eq!((a.spawn_behind, a.spawn_above), (0.0, 3.2));
        assert_eq!(a.release_delay, 0.5);
        assert_eq!(a.object_spin_speed, 20.0);
        assert_eq!(a.trail_name, "WhiteAbilityFlight");
        assert_eq!(a.base.duration, 1.0);
        assert_eq!(a.base.effect_duration, 1.0);
        assert_eq!(a.base.boost_strength, 2200.0);
        assert_eq!(a.base.charges, 1);
    }

    #[test]
    fn egg_is_held_thrown_and_explodes() {
        let Some(mut a) = matilda() else { return };
        let kart = KartState { id: 1, is_player: true, pos: Vec3::new(0.0, 0.5, 0.0), velocity: Vec3::new(0.0, 0.0, 25.0), speed: 25.0, ..Default::default() };
        a.base.env.others = vec![other(2, Vec3::new(5.0, 0.0, 30.0)), other(3, Vec3::new(5.0, 0.0, 100.0))];
        let mut out = Vec::new();
        assert!(a.can_trigger(&kart));
        a.trigger_player(&kart, &mut out);
        assert_eq!(a.release_timer, 0.5);
        assert!(!a.can_trigger(&kart));
        // the egg was requested: type 0x5b above the car, no gravity, owner ignores its explosion, ExplosiveForce 600
        let reqs = a.drain_requests();
        let (handle, pos) = match &reqs[0] {
            WorldRequest::SpawnSmackable { handle, type_id, pos, gravity, ignore_owner_explode, explode_power, owner, .. } => {
                assert_eq!(*type_id, 0x5b);
                assert!(!*gravity && *ignore_owner_explode);
                assert_eq!(*explode_power, 600.0);
                assert_eq!(*owner, 1);
                (*handle, *pos)
            }
            r => panic!("{:?}", r),
        };
        assert!((pos - Vec3::new(0.0, 3.7, 0.0)).length() < 1e-5);
        assert!(a.base.fx_alive); // WhiteAbility start effect
        // held: the egg follows the car each integrate step
        let moved = KartState { pos: Vec3::new(0.0, 0.5, 2.0), ..kart.clone() };
        a.on_integrate(&moved, &mut out);
        let reqs = a.drain_requests();
        assert!(matches!(&reqs[0], WorldRequest::UpdateSmackable { handle: h, pos: Some(p), gravity: Some(false), .. } if *h == handle && (*p - Vec3::new(0.0, 3.7, 2.0)).length() < 1e-5));
        // before the release delay nothing is thrown
        tick_ability(&mut a, 0.4, &kart, &mut out);
        assert!(a.drain_requests().is_empty());
        tick_ability(&mut a, 0.2, &kart, &mut out); // timer < 0: throw
        let reqs = a.drain_requests();
        match &reqs[0] {
            WorldRequest::UpdateSmackable { handle: h, vel: Some(v), ang_vel: Some(w), gravity: Some(true), .. } => {
                assert_eq!(*h, handle);
                assert!((*v - Vec3::new(0.0, 9.05, 45.0)).length() < 1e-4); // car velocity + 20 forward + 9.05 up
                assert!((w.length() - 20.0).abs() < 1e-3);
            }
            r => panic!("{:?}", r),
        }
        assert!(!a.base.fx_alive);
        assert!(a.trail_alive);
        assert!(out.iter().any(|e| matches!(e, CarEffect::Sound { name, .. } if name == "ABY_abilities_white_launch_Human")));
        assert!(!a.can_trigger(&kart)); // egg still alive
        // smashed: car 2 within 15 m takes damage + spin, car 3 does not, the owner never
        let mut out2 = Vec::new();
        a.on_world_event(&WorldEvent::SmackableSmashed { handle: 999, pos: Vec3::ZERO }, &kart, &mut out2);
        assert!(out2.is_empty() && a.object.is_some());
        a.on_world_event(&WorldEvent::SmackableSmashed { handle, pos: Vec3::new(5.0, 0.0, 25.0) }, &kart, &mut out2);
        assert!(out2.contains(&CarEffect::Damage { car: 2, amount: 15.0, source: Some(1) }));
        assert!(out2.contains(&CarEffect::SpinOut { car: 2, time: 3.0, spins: 3.0 }));
        assert_eq!(out2.iter().filter(|e| matches!(e, CarEffect::Damage { .. })).count(), 1);
        assert!(a.object.is_none() && !a.trail_alive);
        // finish resets the release timer
        a.finish(&kart, &mut out2);
        assert_eq!(a.release_timer, -1.0);
        assert!(!a.base.active);
    }

    #[test]
    fn ai_chance_counts_cars_ahead_inside_radius() {
        let Some(mut a) = matilda() else { return };
        let kart = KartState { id: 1, pos: Vec3::ZERO, velocity: Vec3::new(0.0, 0.0, 20.0), forward: Vec3::Z, ..Default::default() };
        // target = 0 + v*(0.5+0.5) + fwd*20*0.5 = (0,0,30)
        let mut near = other(2, Vec3::new(0.0, 0.0, 20.0));
        near.vel = Vec3::new(0.0, 0.0, 10.0); // + 10 -> (0,0,30)
        let behind = {
            let mut o = other(3, Vec3::new(0.0, 0.0, 30.0));
            o.progress_value = -1.0;
            o
        };
        let far = other(4, Vec3::new(0.0, 0.0, 200.0));
        a.base.env.others = vec![near, behind, far];
        a.base.env.progress_value = 0.0;
        let mut out = Vec::new();
        a.always_update(0.0, &kart, &mut out);
        assert!((a.ai_trigger_chance() - 1.0 / 3.0).abs() < 1e-6);
    }
}
