//! Moustache: CMoustacheAbility (player) and CMoustacheBossAbility (boss rockets)
//!
//! `CMoustacheAbility` (id 0xe, "MoustacheAbility", the pig colonel): the same mechanism as Matilda's egg (see matilda.rs) but with
//! 3 charges and up to 3 thrown objects alive at once (`+0xd0 / +0xd4 / +0xd8` smackable slots, `+0xdc / +0xe0 / +0xe4` their trail
//! effects, `+0x98` the slot of the object that is being held / was thrown last). The object is smackable type 0x5c
//! (`moustachepig_tnt`). `SpawnObject` takes the first empty slot (random slot 0..2 when all three are used - the old object of that
//! slot is then simply forgotten, its trail effect is NOT removed), `ThrowObject` launches the current slot's object,
//! `ObjectSmashedCallback` explodes whichever slot's object got smashed.
//!
//! xml tags (char_008.xml / char_016.xml; loader `LoadAbilityValuesFromXML @001cbe54`, `GetAbilityFloatForLevel` default 0, xml order):
//! `Radius`->+0xac (+0xb0 = Radius^2), `ExplosiveForce`->+0xb4, `Damage`->+0xb8, `SpinTime`->+0xbc, `Spins`->+0xc0,
//! `ForwardSpeed`->+0x9c, `UpwardSpeed`->+0xa0, `SpawnPositionBehind`->+0xa4, `SpawnPositionAbove`->+0xa8, `ReleaseDelay`->+0xc8,
//! `ObjectSpinSpeed`->+0xc4, text `TrailEffect`->+0xe8. The xml's `<ReuseDelay>` (0.5) has no loader in the class (the base only reads
//! `<ReuseDelays>`), so the reuse delay stays at the 5.0 default; `ChargeCount` defaults to 3 for id 0xe (base loader).
//!
//! UNRESOLVED: see matilda.rs (`Spin360` arguments, `smackable+0x116c/0x1174`, `rb+0x300`).

use super::matilda::{area_hit, hold_request, hold_point, spawn_held, thrown_ai_chance, throw_request};
use super::*;

/// Smackable type of the thrown object (`AddSmackable(0x5c, ..)`; `smackdefs` "moustachepig_tnt").
pub const MOUSTACHE_TNT_TYPE: u32 = 0x5c;

/// `CMoustacheAbility` (id 0xe): the colonel's thrown TNT.
pub struct MoustacheAbility {
    pub base: AbilityBase,
    /// +0x98 slot (0..2) of the current object.
    pub slot: usize,
    /// +0x9c `<ForwardSpeed>` (ctor 30.0), +0xa0 `<UpwardSpeed>` (ctor 10.0).
    pub forward_speed: f32,
    pub up_speed: f32,
    /// +0xa4 `<SpawnPositionBehind>` (ctor 0), +0xa8 `<SpawnPositionAbove>` (ctor 0).
    pub spawn_behind: f32,
    pub spawn_above: f32,
    /// +0xac `<Radius>` (ctor 1.0), +0xb0 Radius^2 (ctor 1.0).
    pub radius: f32,
    pub radius2: f32,
    /// +0xb4 `<ExplosiveForce>` (ctor 120.0).
    pub explosive_force: f32,
    /// +0xb8 `<Damage>` (ctor 5.0).
    pub damage: f32,
    /// +0xbc `<SpinTime>` (ctor 1.0), +0xc0 `<Spins>` (ctor 1.0).
    pub spin_time: f32,
    pub spins: f32,
    /// +0xc4 `<ObjectSpinSpeed>` (ctor 0), +0xc8 `<ReleaseDelay>` (ctor 0).
    pub object_spin_speed: f32,
    pub release_delay: f32,
    /// +0xcc seconds until the throw (ctor -1.0 = none pending).
    pub release_timer: f32,
    /// +0xd0/+0xd4/+0xd8 the three object slots (world handles).
    pub objects: [Option<u32>; 3],
    /// +0xdc/+0xe0/+0xe4 trail effect alive per slot; +0xe8 `<TrailEffect>` name.
    pub trail_alive: [bool; 3],
    pub trail_name: String,
    last_kart: Option<KartState>,
}

impl MoustacheAbility {
    /// `CMoustacheAbility::CMoustacheAbility(CCar*) @001cb9b8`.
    pub fn new(level: f32) -> MoustacheAbility {
        MoustacheAbility {
            base: AbilityBase::new(level),
            slot: 0,
            forward_speed: 30.0,
            up_speed: 10.0,
            spawn_behind: 0.0,
            spawn_above: 0.0,
            radius: 1.0,
            radius2: 1.0,
            explosive_force: 120.0,
            damage: 5.0,
            spin_time: 1.0,
            spins: 1.0,
            object_spin_speed: 0.0,
            release_delay: 0.0,
            release_timer: -1.0,
            objects: [None; 3],
            trail_alive: [false; 3],
            trail_name: String::new(),
            last_kart: None,
        }
    }

    /// `CMoustacheAbility::SpawnObject @001cb5fc`: picks the slot (first empty one, whose stale trail effect is removed; else a random
    /// one of the three), then spawns type 0x5c at car matrix * `Translation(0, above, -behind)` with explosion strength
    /// `ExplosiveForce` (when > 0), ignored by the owner.
    pub fn spawn_object(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        let free = self.objects.iter().position(|o| o.is_none());
        let slot = match free {
            Some(i) => {
                if self.trail_alive[i] {
                    out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(trail)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
                    self.trail_alive[i] = false;
                }
                i
            }
            // `CXGSRandom::Range(0, 2)`: inclusive upper bound -> 0, 1 or 2
            None => ((self.base.env.rand01() * 3.0) as usize).min(2),
        };
        self.slot = slot;
        let h = spawn_held(&mut self.base, kart, MOUSTACHE_TNT_TYPE, self.spawn_above, self.spawn_behind, self.explosive_force);
        self.objects[slot] = Some(h);
    }

    /// `CMoustacheAbility::ThrowObject @001cbab4`: removes the start effect and the current slot's trail effect, spawns the trail
    /// effect for the slot, launches the slot's object (see [`throw_request`]) and fires `ABKSound::CAbilityController::OnEvent(0, car)`.
    pub fn throw_object(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.base.fx_alive {
            out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(ability)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
            self.base.fx_alive = false;
        }
        let s = self.slot;
        if self.trail_alive[s] {
            out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(trail)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
            self.trail_alive[s] = false;
        }
        if !self.trail_name.is_empty() {
            out.push(CarEffect::Particle { name: self.trail_name.clone(), car: None, pos: kart.pos });
            self.trail_alive[s] = true;
        }
        if let Some(h) = self.objects[s] {
            let req = throw_request(&mut self.base.env, h, kart, self.forward_speed, self.up_speed, self.object_spin_speed);
            self.base.request(req);
            out.push(CarEffect::Other { tag: "ABKSound::CAbilityController::OnEvent(0)", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
        }
    }
}

impl Ability for MoustacheAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::Moustache
    }
    /// `CMoustacheAbility::GetAbilityName @001cb0a4` (the xml `name`).
    fn name(&self) -> &str {
        "MoustacheAbility"
    }

    /// `CMoustacheAbility::LoadAbilityValuesFromXML @001cbe54`, then the base loader.
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

    /// `CMoustacheAbility::TriggerAbility @001cc00c`: release timer = `ReleaseDelay`, base trigger, `SpawnObject` (virtual slot 0xa0).
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.release_timer = self.release_delay;
        base_trigger(self, kart, out);
        self.spawn_object(kart, out);
    }

    /// `CMoustacheAbility::OnCarUpdate @001cc034`: release timer counts down while >= 0, throws when < 0; then base.
    fn update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.release_timer >= 0.0 {
            self.release_timer -= dt;
            if self.release_timer < 0.0 {
                self.throw_object(kart, out);
            }
        }
        base_update(self, dt, kart, out);
    }

    /// `CMoustacheAbility::CanTriggerAbility @001cc084`: not while a release is pending, else the base conditions (so up to 3
    /// objects can be in flight; charges limit the uses).
    fn can_trigger(&self, kart: &KartState) -> bool {
        if self.release_timer >= 0.0 {
            return false;
        }
        base_can_trigger(&self.base, kart)
    }

    /// `CMoustacheAbility::OnCarIntegrate @001cc0b0`: base; while the start effect is alive it follows the car and the current slot's
    /// object is held at the hold point; then every slot whose object and trail effect are alive moves its trail effect.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_on_integrate(self, kart, out);
        if self.base.fx_alive {
            out.push(CarEffect::Other {
                tag: "CXGSParticleEffectManager::MoveEffect(Moustache start effect)",
                car: Some(kart.id),
                vals: [kart.forward.x, kart.forward.y, kart.forward.z, 0.0],
                pos: hold_point(kart, self.spawn_above, self.spawn_behind),
            });
            if let Some(h) = self.objects[self.slot] {
                self.base.request(hold_request(h, kart, self.spawn_above, self.spawn_behind));
            }
        }
        for i in 0..3 {
            if self.objects[i].is_some() && self.trail_alive[i] {
                out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::MoveEffect(Moustache trail -> object)", car: Some(kart.id), vals: [i as f32, 0.0, 0.0, 0.0], pos: kart.pos });
            }
        }
    }

    /// `CMoustacheAbility::FinishAbility @001cc374`: base finish, release timer back to -1.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_finish(self, kart, out);
        self.release_timer = -1.0;
    }

    /// `CBaseAbility::OnCarAlwaysUpdate` plus the kart snapshot for the AI chance.
    fn always_update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.last_kart = Some(kart.clone());
        base_always_update(self, dt, kart, out);
    }

    /// `CMoustacheAbility::ObjectSmashedCallback @001cb0b8`: for the slot whose object was smashed, the area damage loop
    /// ([`area_hit`], radius^2 `+0xb0`, `Damage` `+0xb8`, `Spins` `+0xc0`) around the object, then the slot and its trail effect are cleared.
    fn on_world_event(&mut self, ev: &WorldEvent, kart: &KartState, out: &mut Vec<CarEffect>) {
        let WorldEvent::SmackableSmashed { handle, pos } = ev else { return };
        for i in 0..3 {
            if self.objects[i] == Some(*handle) {
                area_hit(&self.base.env, kart.id, *pos, self.radius2, self.damage, self.spin_time, self.spins, out);
                self.objects[i] = None;
                if self.trail_alive[i] {
                    out.push(CarEffect::Other { tag: "CXGSParticleEffectManager::RemoveEffect(trail)", car: Some(kart.id), vals: [i as f32, 0.0, 0.0, 0.0], pos: *pos });
                    self.trail_alive[i] = false;
                }
            }
        }
    }

    /// `CMoustacheAbility::CalcCurrentAITriggerChance @001cb3dc`: identical to Matilda's (see [`thrown_ai_chance`]) with this class'
    /// `ForwardSpeed` (+0x9c), `ReleaseDelay` (+0xc8) and radius^2 (+0xb0).
    fn ai_trigger_chance(&self) -> f32 {
        match self.last_kart.as_ref() {
            Some(k) => thrown_ai_chance(k, &self.base.env, self.release_delay, self.forward_speed, self.radius2),
            None => 0.0,
        }
    }
}

/// STUB (replace with the ported class).
pub struct MoustacheBossAbility {
    pub base: AbilityBase,
}

impl MoustacheBossAbility {
    pub fn new(level: f32) -> MoustacheBossAbility {
        MoustacheBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for MoustacheBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::MoustacheBoss
    }
    fn name(&self) -> &str {
        "MoustacheBossAbility"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moustache() -> Option<MoustacheAbility> {
        let xml = std::fs::read_to_string(format!("{}/xml/characters/charxml/char_008.xml", ASSETS292)).ok()?;
        let p = AbilityParams::from_character_xml(&xml)?;
        let mut a = MoustacheAbility::new(0.0);
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

    fn handle_of(reqs: &[WorldRequest]) -> u32 {
        match &reqs[0] {
            WorldRequest::SpawnSmackable { handle, type_id, ignore_owner_explode, .. } => {
                assert_eq!(*type_id, 0x5c);
                assert!(*ignore_owner_explode);
                *handle
            }
            r => panic!("{:?}", r),
        }
    }

    #[test]
    fn values_from_xml() {
        let Some(a) = moustache() else { return };
        assert_eq!(a.radius, 15.0);
        assert_eq!(a.radius2, 225.0);
        assert_eq!(a.explosive_force, 200.0);
        assert_eq!(a.damage, 5.0);
        assert_eq!((a.spin_time, a.spins), (2.0, 1.0));
        assert_eq!((a.forward_speed, a.up_speed), (15.0, 8.72));
        assert_eq!((a.spawn_behind, a.spawn_above), (0.0, 3.0));
        assert_eq!(a.release_delay, 0.7);
        assert_eq!(a.object_spin_speed, 20.0);
        assert_eq!(a.trail_name, "MoustachePigAbilityFlight");
        assert_eq!(a.base.charges, 3);
        assert_eq!(a.base.reuse_delay, 5.0); // <ReuseDelay> has no reader
        assert_eq!(a.base.boost_strength, 2200.0);
    }

    #[test]
    fn three_charges_three_objects() {
        let Some(mut a) = moustache() else { return };
        let kart = KartState { id: 1, is_player: true, pos: Vec3::new(0.0, 0.0, 0.0), velocity: Vec3::new(0.0, 0.0, 20.0), speed: 20.0, ..Default::default() };
        let mut out = Vec::new();
        let mut handles = Vec::new();
        for n in 0..3 {
            assert!(a.can_trigger(&kart), "use {}", n);
            a.trigger_player(&kart, &mut out);
            assert_eq!(a.slot, n);
            assert_eq!(a.release_timer, 0.7);
            assert!(!a.can_trigger(&kart)); // release pending
            let reqs = a.drain_requests();
            handles.push(handle_of(&reqs));
            if let WorldRequest::SpawnSmackable { pos, explode_power, .. } = &reqs[0] {
                assert!((*pos - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-5);
                assert_eq!(*explode_power, 200.0);
            }
            // throw after ReleaseDelay
            tick_ability(&mut a, 0.8, &kart, &mut out);
            let reqs = a.drain_requests();
            match &reqs[0] {
                WorldRequest::UpdateSmackable { handle, vel: Some(v), gravity: Some(true), .. } => {
                    assert_eq!(*handle, handles[n]);
                    assert!((*v - Vec3::new(0.0, 8.72, 35.0)).length() < 1e-4);
                }
                r => panic!("{:?}", r),
            }
            assert!(a.trail_alive[n]);
            // re-arm the ability for the next charge (the base update finishes it after Duration 1.0)
            tick_ability(&mut a, 0.5, &kart, &mut out);
            assert!(!a.base.active);
        }
        assert_eq!(a.base.charges, 0);
        assert!(!a.can_trigger(&kart)); // no charges left
        assert!(a.objects.iter().all(|o| o.is_some()));
        // smash object 1: only that slot clears; owner excluded; car in 15 m takes damage 5 + spin
        a.base.env.others = vec![other(2, Vec3::new(0.0, 0.0, 10.0)), other(3, Vec3::new(0.0, 0.0, 60.0))];
        let mut out2 = Vec::new();
        a.on_world_event(&WorldEvent::SmackableSmashed { handle: handles[1], pos: Vec3::new(0.0, 0.0, 12.0) }, &kart, &mut out2);
        assert_eq!(a.objects[0], Some(handles[0]));
        assert_eq!(a.objects[1], None);
        assert_eq!(a.objects[2], Some(handles[2]));
        assert!(!a.trail_alive[1] && a.trail_alive[0] && a.trail_alive[2]);
        assert!(out2.contains(&CarEffect::Damage { car: 2, amount: 5.0, source: Some(1) }));
        assert!(out2.contains(&CarEffect::SpinOut { car: 2, time: 2.0, spins: 1.0 }));
        assert_eq!(out2.iter().filter(|e| matches!(e, CarEffect::Damage { .. })).count(), 1);
        // the freed slot is reused by the next spawn
        a.base.charges = 1;
        a.trigger_player(&kart, &mut out);
        assert_eq!(a.slot, 1);
    }

    #[test]
    fn full_slots_pick_a_random_slot() {
        let Some(mut a) = moustache() else { return };
        let kart = KartState { id: 1, ..Default::default() };
        let mut out = Vec::new();
        a.objects = [Some(10), Some(11), Some(12)];
        a.trail_alive = [true; 3];
        a.spawn_object(&kart, &mut out);
        assert!(a.slot < 3);
        assert!(a.objects.iter().filter(|o| o.is_some()).count() == 3);
        assert!(a.trail_alive == [true; 3]); // the stale trail is not removed in the random branch
    }
}
