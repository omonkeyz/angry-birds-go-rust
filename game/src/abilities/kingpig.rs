//! King Pig abilities: CKingPigAbility (player) and CKingPigBossAbility (boss)

use super::*;

/// `CKingPigAbility` (vtable id 0xb, "KingPigAbility"; `char_006.xml`). King Pig flies on a hover-glide: for `Duration`
/// (4 s) gravity and downforce are removed, steering is ignored (`OnCarSetSteering` returns 0), the car is pulled towards
/// a point `TargetHeight` above the racing line `AimAheadDistance` ahead with a spring force of `SteeringForce`, and its
/// speed is pinned to `speed-at-trigger * SpeedMultiplier`. It is immune to spin / damage / explosions while active and
/// ends early when it hits the world. Source: `CKingPigAbility::*` @001c4214..@001c4714.
///
/// xml -> field (call order of `LoadAbilityValuesFromXML @001c4348` = xml order after `CamBehindMod`/`CamBehindTime`,
/// ctor defaults all 1.0):
///
/// | field | `<Tag>` | use |
/// |-------|---------|-----|
/// | +0xa4 | `TargetHeight` | height above the spline point the car is pulled to (`TargetHeight * spline up`) |
/// | +0xa8 | `AimAheadDistance` | `CSpline::Lookahead` distance (the argument is in a VFP register the decompile dropped; +0xa8 is otherwise unread, so this assignment is by elimination) |
/// | +0xac | `SteeringForce` | spring gain: `force = rb+0x98 * SteeringForce * (target - pos)` |
/// | +0xa0 | `SpeedMultiplier` | `TriggerAbility`: speed target (+0x9c) = `CCar+0x1ab4` (current speed) `* SpeedMultiplier` |
/// | +0xb0 | `CamBehindTime` | `OnCarCamBehindMod` ramp time |
/// | +0x5c | `CamBehindMod` | (base loader) camera-behind distance scale |
///
/// The ctor also sets +0x68/+0x6c/+0x70 = 1 (immune spin / damage / explosions while active) and `CamBehindMod = 1`.
pub struct KingPigAbility {
    pub base: AbilityBase,
    /// +0x98 "spent" latch (set by `FinishAbility` / `OnCarCollision` when `CanRetrigger()` is false; never in the shipped config).
    pub spent: bool,
    /// +0x9c speed target, set in `TriggerAbility`.
    pub speed_target: f32,
    /// +0xa0 `<SpeedMultiplier>`.
    pub speed_multiplier: f32,
    /// +0xa4 `<TargetHeight>`.
    pub target_height: f32,
    /// +0xa8 `<AimAheadDistance>`.
    pub aim_ahead_distance: f32,
    /// +0xac `<SteeringForce>`.
    pub steering_force: f32,
    /// +0xb0 `<CamBehindTime>`.
    pub cam_behind_time: f32,
}

impl KingPigAbility {
    /// `CKingPigAbility::CKingPigAbility(CCar*) @001c42e4`.
    pub fn new(level: f32) -> KingPigAbility {
        let mut base = AbilityBase::new(level);
        base.immune_spin = true; // +0x68
        base.immune_damage = true; // +0x6c
        base.immune_explosions = true; // +0x70
        base.cam_behind_mod = 1.0; // +0x5c
        KingPigAbility {
            base,
            spent: false,
            speed_target: 0.0,
            speed_multiplier: 1.0,
            target_height: 1.0,
            aim_ahead_distance: 1.0,
            steering_force: 1.0,
            cam_behind_time: 1.0,
        }
    }

    /// `CCar::SetGliding(1)` immediately followed by `SetGliding(0)` (`FinishAbility`, `OnCarCollision`): the car ends up
    /// with its normal gravity vector (`CCar+0x1bfc`) and its bodywork downforce (`spec+0x430`) again.
    fn restore_gravity(&self, kart: &KartState, out: &mut Vec<CarEffect>) {
        out.push(CarEffect::SetGravityMultiplier { car: kart.id, mul: 1.0 });
        out.push(CarEffect::SetDownforceMultiplier { car: kart.id, mul: 1.0 });
    }
}

impl Ability for KingPigAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    /// `CKingPigAbility::GetId @001c4224`.
    fn id(&self) -> BirdAbility {
        BirdAbility::KingPig
    }
    /// `CKingPigAbility::GetAbilityName @001c4214`.
    fn name(&self) -> &str {
        "KingPigAbility"
    }

    /// `CKingPigAbility::LoadAbilityValuesFromXML @001c4348`: TargetHeight, AimAheadDistance, SteeringForce,
    /// SpeedMultiplier, CamBehindTime (`GetAbilityFloatForLevel`, missing = 0), then the base loader (which reads
    /// `CamBehindMod`).
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.target_height = p.float_for_level("TargetHeight", 0.0, lvl);
        self.aim_ahead_distance = p.float_for_level("AimAheadDistance", 0.0, lvl);
        self.steering_force = p.float_for_level("SteeringForce", 0.0, lvl);
        self.speed_multiplier = p.float_for_level("SpeedMultiplier", 0.0, lvl);
        self.cam_behind_time = p.float_for_level("CamBehindTime", 0.0, lvl);
        base_load_values(self, p);
    }

    /// `CKingPigAbility::OnCarSetSteering @001c422c`: the player's steering is ignored (returns 0).
    fn on_set_steering(&self, _steer: f32) -> f32 {
        0.0
    }

    /// `CKingPigAbility::OnCarCamBehindMod @001c4234`: with `T = CamBehindTime`, `r = max(time_remaining, T)`:
    /// `(1 - (r - T) / T) * CamBehindMod` (the input is ignored).
    fn on_cam_behind_mod(&self, _input: f32) -> f32 {
        let t = self.cam_behind_time;
        let rem = self.base.time_remaining;
        let r = if rem <= t { t } else { rem };
        (1.0 - (r - t) / t) * self.base.cam_behind_mod
    }

    /// `CKingPigAbility::OnCarImpactDamage @001c42c0`: a tail call of `CBaseAbility::IsActive`, so while active (the only
    /// time `CCar::AddImpactDamage` calls it) every impact does exactly 1 damage point (0/1 bool as the int amount).
    fn on_impact_damage(&mut self, _damage: f32) -> f32 {
        if self.base.is_active() {
            1.0
        } else {
            0.0
        }
    }

    /// `CKingPigAbility::TriggerAbility @001c4400`: `rb->SetGravity(<global vector>)` and `rb->SetDownForce(..)` (the
    /// global is not recoverable from the decompile - UNRESOLVED, taken as the zero vector: no gravity / no downforce
    /// while gliding), `+0x9c = CCar+0x1ab4 (speed) * SpeedMultiplier`, then `CBaseAbility::TriggerAbility`.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        out.push(CarEffect::SetGravityMultiplier { car: kart.id, mul: 0.0 });
        out.push(CarEffect::SetDownforceMultiplier { car: kart.id, mul: 0.0 });
        self.speed_target = kart.speed * self.speed_multiplier;
        base_trigger(self, kart, out);
    }

    /// `CKingPigAbility::FinishAbility @001c445c`: latch when not retriggerable, `SetGliding(1); SetGliding(0)`
    /// (gravity / downforce back to normal), then the base finish.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if !self.base.env.debug.can_retrigger() {
            self.spent = true;
        }
        self.restore_gravity(kart, out);
        base_finish(self, kart, out);
    }

    /// `CKingPigAbility::OnCarCollision @001c4498`: when the collision partner pointer is null (the world) the ability
    /// ends at once: `FinishAbility` (virtual slot 0x98, the same body), `StopEffects` (slot 0x2c), `effect_timer = 0`.
    /// Always returns 1.0. The "partner == null means world" reading of the stack argument is UNRESOLVED.
    fn on_collision_ex(&mut self, hit_world: bool, kart: &KartState, out: &mut Vec<CarEffect>) -> f32 {
        if hit_world {
            self.finish(kart, out);
            self.stop_effects(kart, out);
            self.base.effect_timer = 0.0;
        }
        1.0
    }

    /// `CKingPigAbility::CanTriggerAbility @001c453c`.
    fn can_trigger(&self, kart: &KartState) -> bool {
        if self.spent {
            return false;
        }
        base_can_trigger(&self.base, kart)
    }

    fn spline_lookahead_request(&self) -> Option<f32> {
        if self.base.active {
            Some(self.aim_ahead_distance)
        } else {
            None
        }
    }

    /// `CKingPigAbility::OnCarIntegrate @001c4554`. While `active`: the spring force towards
    /// `spline_pos + TargetHeight * spline_up` ([`AbilityEnv::spline_ahead`], sampled `AimAheadDistance` ahead):
    /// `WorldForce( rb+0x98 * SteeringForce * (target - pos) )`, applied at `pos + axis * extent * 0.5` of the chassis
    /// shape (`rb+0x34` data, UNRESOLVED: applied at the body position here); then the velocity (rb+0x10..0x18, copied to
    /// the previous-velocity slot rb+0xd4..0xdc) is rescaled to `speed_target` along its current direction and the body
    /// woken (`SetSleep(0)`): [`CarEffect::Other`] tag `"CXGSRigidBody::SetVelocity(KingPig)"`, `vals = [vx, vy, vz, speed]`.
    /// Then `CBaseAbility::OnCarIntegrate`.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.base.active {
            if let Some((spline_pos, spline_up)) = self.base.env.spline_ahead {
                let target = spline_pos + spline_up * self.target_height;
                let k = self.base.env.physics_dt * self.steering_force;
                out.push(CarEffect::WorldForce { car: kart.id, force: (target - kart.pos) * k, at: kart.pos });
            }
            let len = kart.velocity.length();
            if len > 0.0 {
                let v = kart.velocity * (self.speed_target / len);
                out.push(CarEffect::Other { tag: "CXGSRigidBody::SetVelocity(KingPig)", car: Some(kart.id), vals: [v.x, v.y, v.z, self.speed_target], pos: kart.pos });
            }
        }
        base_on_integrate(self, kart, out);
    }

    /// `CKingPigAbility::OnCarUpdate @001c4714`: the same body as `CBaseAbility::OnCarUpdate @001b987c` (effect timer,
    /// duration timer, finish with `boost_time` saved/restored).
    fn update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_update(self, dt, kart, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Option<AbilityParams> {
        let xml = std::fs::read_to_string(format!("{}/xml/characters/charxml/char_006.xml", ASSETS292)).ok()?;
        AbilityParams::from_character_xml(&xml)
    }

    fn kart() -> KartState {
        KartState {
            id: 0,
            is_player: true,
            character: 5,
            pos: Vec3::new(0.0, 1.0, 0.0),
            velocity: Vec3::new(0.0, 0.0, 20.0),
            speed: 20.0,
            ..Default::default()
        }
    }

    #[test]
    fn values_from_real_xml() {
        let Some(p) = params() else { return };
        let mut a = KingPigAbility::new(0.0);
        a.load_values(&p);
        assert_eq!(a.base.duration, 4.0);
        assert_eq!(a.base.cam_behind_mod, 4.0);
        assert_eq!(a.cam_behind_time, 3.0);
        assert_eq!(a.target_height, 10.0);
        assert_eq!(a.aim_ahead_distance, 20.0);
        assert_eq!(a.steering_force, 2000.0);
        assert!((a.speed_multiplier - 1.15).abs() < 1e-6);
        assert!(a.base.immune_spin && a.base.immune_damage && a.base.immune_explosions);
        assert_eq!(a.on_set_steering(0.7), 0.0);
    }

    #[test]
    fn glide_cycle() {
        let Some(p) = params() else { return };
        let mut a = create_ability("KingPigAbility", &p, 0.0).unwrap();
        let mut env = a.base().env.clone();
        env.effects = p.effects.clone();
        env.spline_ahead = Some((Vec3::new(0.0, 0.0, 30.0), Vec3::Y));
        a.set_env(env);
        let k = kart();
        assert!(a.can_trigger(&k));
        let mut out = Vec::new();
        a.trigger_player(&k, &mut out);
        assert!(a.is_active());
        assert!(out.contains(&CarEffect::SetGravityMultiplier { car: 0, mul: 0.0 }));
        assert!(a.immune_to_spin() && a.immune_to_damage() && a.immune_to_explosions());
        assert_eq!(a.on_impact_damage(10.0), 1.0);
        assert_eq!(a.spline_lookahead_request(), Some(20.0));
        out.clear();
        a.on_integrate(&k, &mut out);
        let f = out.iter().find_map(|e| if let CarEffect::WorldForce { force, .. } = e { Some(*force) } else { None }).unwrap();
        // dt * 2000 * ((0,10,30) - (0,1,0))
        assert!((f - Vec3::new(0.0, 9.0, 30.0) * (2000.0 / 60.0)).length() < 1e-2);
        let v = out
            .iter()
            .find_map(|e| if let CarEffect::Other { tag: "CXGSRigidBody::SetVelocity(KingPig)", vals, .. } = e { Some(*vals) } else { None })
            .unwrap();
        assert!((v[2] - 23.0).abs() < 1e-4 && (v[3] - 23.0).abs() < 1e-4); // 20 * 1.15
        // camera-behind ramp: 4.0 remaining, T = 3: (1 - 1/3) * 4
        a.base_mut().time_remaining = 4.0;
        assert!((a.on_cam_behind_mod(0.0) - 8.0 / 3.0).abs() < 1e-5);
        a.base_mut().time_remaining = 1.0;
        assert!((a.on_cam_behind_mod(0.0) - 4.0).abs() < 1e-5);
        // world collision ends it
        out.clear();
        assert_eq!(a.on_collision_ex(false, &k, &mut out), 1.0);
        assert!(a.base().active);
        assert_eq!(a.on_collision_ex(true, &k, &mut out), 1.0);
        assert!(!a.base().active);
        assert!(out.contains(&CarEffect::SetGravityMultiplier { car: 0, mul: 1.0 }));
        assert_eq!(a.base().effect_timer, 0.0);
    }

    #[test]
    fn runs_out_after_duration() {
        let Some(p) = params() else { return };
        let mut a = create_ability("KingPigAbility", &p, 0.0).unwrap();
        let k = kart();
        let mut out = Vec::new();
        a.trigger_player(&k, &mut out);
        let mut t = 0.0f32;
        while a.base().active && t < 6.0 {
            tick_ability(a.as_mut(), 1.0 / 60.0, &k, &mut out);
            t += 1.0 / 60.0;
        }
        assert!(!a.base().active);
        assert!((t - 4.0).abs() < 0.05, "ran {} s", t);
    }
}

/// STUB (replace with the ported class).
pub struct KingPigBossAbility {
    pub base: AbilityBase,
}

impl KingPigBossAbility {
    pub fn new(level: f32) -> KingPigBossAbility {
        KingPigBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for KingPigBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::KingPigBoss
    }
    fn name(&self) -> &str {
        "KingPigBossAbility"
    }
}
