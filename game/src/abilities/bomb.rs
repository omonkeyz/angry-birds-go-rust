//! Bomb ability (Black bird): `CBombAbility` (vtable id 2, "Bomb").
//!
//! Black primes a bomb: for `Duration` (0.5 s) the start effect ("BlackAbilityPriming") plays, then `FinishAbility` detonates
//! it at the bird's own position. Every other car within `Radius` (and not on the same team in team mode, and not in its
//! "non collide" window `CCar+0x42c > 0`) takes `Damage` of impact damage, is shoved by `SlowdownForce` and spun
//! (`Spin360`). The world then gets `CSmackable::Explode(ExplosiveForce, pos)` (radial impulse on every rigid body).
//! Source: `libABK291_annotated.c` `CBombAbility::*` @001be3d0..@001beb40.
//!
//! xml (`char_002.xml`, `<Ability name="Bomb">`) -> field. The tag NAMES are lost in the decompile (arguments show as
//! `in_r1`); the mapping is by call ORDER = the order of the string pool entry run "Damage, SpinTime, Spins,
//! ExplosiveForce, SlowdownForce" (file offset 0xc9f604) plus the defaults the constructor writes:
//!
//! | field | `<Tag>` | ctor default | use |
//! |-------|---------|--------------|-----|
//! | +0x98 | `Radius` | 1.0 | blast radius (the AI/finish tests compare against `+0x9c` = Radius squared) |
//! | +0xa0 | `Damage` | 10.0 | int arg of `CCar::AddImpactDamage` (`damage * bodywork+0x480`) |
//! | +0xa4 | `SpinTime` | 3.0 | seconds of the spin (`Spin360` time argument) |
//! | +0xa8 | `Spins` | 3.0 | `Spin360` int argument (`spins * 3.0` -> CCar+0x4b0) |
//! | +0xac | `ExplosiveForce` | 240.0 | first argument of `CSmackable::Explode` |
//! | +0xb0 | `SlowdownForce` | 0.0 | `-(SlowdownForce * rb+0x98)` body force on every victim (local -Z). `char_002.xml` has no such tag, so it is 0 in the shipped data |
//!
//! `Duration/EffectDuration/BoostStrength/BoostDuration/CamBehindMod/AIActivationTime` are read by the base loader.
//! The ctor also writes `+0x64 = 0` (`play_anim = false`: no pilot animation state on trigger).

use super::*;

/// `CBombAbility` (see the module docs).
pub struct BombAbility {
    pub base: AbilityBase,
    /// +0x98 `<Radius>`.
    pub radius: f32,
    /// +0x9c `Radius * Radius` (written by the loader).
    pub radius_sq: f32,
    /// +0xa0 `<Damage>`.
    pub damage: f32,
    /// +0xa4 `<SpinTime>`.
    pub spin_time: f32,
    /// +0xa8 `<Spins>`.
    pub spins: f32,
    /// +0xac `<ExplosiveForce>`.
    pub explosive_force: f32,
    /// +0xb0 `<SlowdownForce>`.
    pub slowdown_force: f32,
    /// Own position cached from the last `always_update` / `on_integrate` (the original's AI-chance function reads
    /// `CCar+0x104` -> rb position; `Ability::ai_trigger_chance(&self)` has no kart argument).
    pub my_pos: Vec3,
}

/// `CSmackable::ApplyExplodeForce @001d4ff0` (the callback `CSmackable::Explode` runs over every rigid body): the pushed
/// acceleration vector is `diff * P / max(d^2, 4)`, applied only while its squared length is > 10, i.e. `|diff| P / d^2 >
/// sqrt(10)`; for `d >= 2` that is `d < P / sqrt(10)`. This is the effective blast radius of `Explode(P, ..)`.
pub fn explode_effective_radius(power: f32) -> f32 {
    power / 10f32.sqrt()
}

impl BombAbility {
    /// `CBombAbility::CBombAbility(CCar*) @001be5ac`.
    pub fn new(level: f32) -> BombAbility {
        let mut base = AbilityBase::new(level);
        base.play_anim = false; // +0x64 = 0
        BombAbility {
            base,
            radius: 1.0,
            radius_sq: 1.0,
            damage: 10.0,
            spin_time: 3.0,
            spins: 3.0,
            explosive_force: 240.0, // 0x43700000
            slowdown_force: 0.0,
            my_pos: Vec3::ZERO,
        }
    }
}

/// Shared by Bomb and Terence: `CCar::Spin360(time, _, spins)` as a [`CarEffect::SpinOut`]. `Spin360 @001b091c` sets
/// `CCar+0x4b0 = spins * 3.0` and `CCar+0x4a8 = CCar+0x4ac = time` (`spins * 1.5` when `time <= 0`); it returns
/// without doing anything when the car's main (or any boss) ability reports `ImmuneToSpin` (slot 0x8c).
pub(crate) fn spin360_effect(victim: &OtherCar, time: f32, spins: f32) -> Option<CarEffect> {
    if victim.immune_spin {
        return None;
    }
    let t = if time <= 0.0 { spins * 1.5 } else { time };
    Some(CarEffect::SpinOut { car: victim.id, time: t, spins })
}

/// `CBombAbility::CalcCurrentAITriggerChance @001be454` / `CTerenceRageAbility::CalcCurrentAITriggerChance @001d3e18`
/// (identical except for the order of the penalty/progress tests): the sum over every car within `radius_sq` of
/// `1 / (num_cars - 1)` for the cars that are ahead of (or level with) this car in race progress and not currently
/// penalised (`CCar+0x4ac <= 0`); 0.0 as soon as a car in range is on this car's team. The initial value is the
/// constant `DAT_001be590` / `DAT_001d3f48` (data section, not recoverable from the decompile; the function only adds
/// to it and returns it, so 0.0 is assumed - UNRESOLVED).
pub(crate) fn area_ai_trigger_chance(env: &AbilityEnv, my_pos: Vec3, radius_sq: f32) -> f32 {
    let mut chance = 0.0f32;
    let n_others = env.others.len() as f32; // CGame car count (+0x31b8) - 1
    for o in &env.others {
        let d2 = (o.pos - my_pos).length_squared();
        if radius_sq < d2 {
            continue;
        }
        if o.team == env.team {
            // CCar::IsCarOnMyTeam
            return 0.0;
        }
        if o.progress_value < env.progress_value {
            continue;
        }
        if o.penalty > 0.0 {
            continue;
        }
        chance += 1.0 / n_others;
    }
    chance
}

impl Ability for BombAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    /// `CBombAbility::GetId @001be3e0`.
    fn id(&self) -> BirdAbility {
        BirdAbility::Bomb
    }
    /// `CBombAbility::GetAbilityName @001be3d0`.
    fn name(&self) -> &str {
        "Bomb"
    }

    /// `CBombAbility::LoadAbilityValuesFromXML @001be618`: Radius (+ its square), Damage, SpinTime, Spins,
    /// ExplosiveForce, SlowdownForce (all `GetAbilityFloatForLevel`, missing = 0), then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.radius = p.float_for_level("Radius", 0.0, lvl);
        self.radius_sq = self.radius * self.radius;
        self.damage = p.float_for_level("Damage", 0.0, lvl);
        self.spin_time = p.float_for_level("SpinTime", 0.0, lvl);
        self.spins = p.float_for_level("Spins", 0.0, lvl);
        self.explosive_force = p.float_for_level("ExplosiveForce", 0.0, lvl);
        self.slowdown_force = p.float_for_level("SlowdownForce", 0.0, lvl);
        base_load_values(self, p);
    }

    /// Host refresh keeps the cached own position for [`Ability::ai_trigger_chance`].
    fn always_update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.my_pos = kart.pos;
        base_always_update(self, dt, kart, out);
    }

    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.my_pos = kart.pos;
        base_on_integrate(self, kart, out);
    }

    /// `CBombAbility::CalcCurrentAITriggerChance @001be454` (see [`area_ai_trigger_chance`]).
    fn ai_trigger_chance(&self) -> f32 {
        area_ai_trigger_chance(&self.base.env, self.my_pos, self.radius_sq)
    }

    /// `CBombAbility::FinishAbility @001be6fc`. Only when `IsActive()`:
    /// 1. `ABKSound::CAbilityController::OnAbilityStart(1, 0, car)` (the detonation sound).
    /// 2. For every other car whose `CCar+0x42c <= 0` (`OtherCar::hittable`), unless it is on this car's team in team
    ///    mode (`CGame+0x32b0 == 3`), and whose squared distance to the bird is `<= Radius^2`:
    ///    `AddImpactDamage(pos, _, (int)Damage)` -> [`CarEffect::Damage`] (the victim's own ability may rescale the
    ///    amount through `OnCarImpactDamage`; the host/`Ability::on_impact_damage` does that, KingPig makes it 1);
    ///    then (unless game mode 0xb with a driver object on this car - UNRESOLVED, the game mode is not in the env, so
    ///    treated as "not mode 0xb") `ApplyBodyForce((0, 0, -SlowdownForce * rb+0x98))` on the victim
    ///    ([`CarEffect::BodyForce`], `car` = victim) and `Spin360(SpinTime, _, Spins)` ([`CarEffect::SpinOut`], skipped
    ///    when the victim `immune_spin`). The challenge-manager event (player only) is bookkeeping and not modelled.
    /// 3. `CEnvObjectManager::GetNearbyPickups(pos, 10.0)`: each pickup in range whose vtable test passes gets
    ///    `vtbl+0x44(pos, Radius)` / `vtbl+0x48(car, pos, Radius)` (pick-up destruction). The host has no pickup list
    ///    in the env, so this is only reported as `CarEffect::Other{tag:"CEnvObjectManager::GetNearbyPickups"}`
    ///    with `vals = [10.0, Radius, 0, 0]` (UNRESOLVED).
    /// 4. `CSmackable::Explode(ExplosiveForce, pos, ..)` -> [`CarEffect::Explosion`] with `damage = 0` (the callback
    ///    only pushes) and `radius = ExplosiveForce / sqrt(10)` ([`explode_effective_radius`]).
    ///
    /// Then `CBaseAbility::FinishAbility`.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.base.is_active() {
            let pos = kart.pos;
            out.push(CarEffect::Other { tag: "ABKSound::CAbilityController::OnAbilityStart(1,0)", car: Some(kart.id), vals: [1.0, 0.0, 0.0, 0.0], pos });
            let dt = self.base.env.physics_dt;
            let team_mode = self.base.env.team_mode;
            let my_team = self.base.env.team;
            let others = self.base.env.others.clone();
            for o in &others {
                if o.id == kart.id || !o.hittable {
                    continue;
                }
                if o.team == my_team && team_mode {
                    continue;
                }
                let d2 = (pos - o.pos).length_squared();
                if d2 <= self.radius_sq {
                    out.push(CarEffect::Damage { car: o.id, amount: self.damage, source: Some(kart.id) });
                    out.push(CarEffect::BodyForce {
                        car: o.id,
                        force_local: Vec3::new(0.0, 0.0, -(self.slowdown_force * dt)),
                        at_local: Vec3::ZERO,
                    });
                    if let Some(e) = spin360_effect(o, self.spin_time, self.spins) {
                        out.push(e);
                    }
                }
            }
            out.push(CarEffect::Other { tag: "CEnvObjectManager::GetNearbyPickups", car: Some(kart.id), vals: [10.0, self.radius, 0.0, 0.0], pos });
            out.push(CarEffect::Explosion {
                center: pos,
                radius: explode_effective_radius(self.explosive_force),
                force: self.explosive_force,
                damage: 0.0,
                source: Some(kart.id),
            });
        }
        base_finish(self, kart, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Option<AbilityParams> {
        let xml = std::fs::read_to_string(format!("{}/xml/characters/charxml/char_002.xml", ASSETS292)).ok()?;
        AbilityParams::from_character_xml(&xml)
    }

    fn other(id: usize, pos: Vec3) -> OtherCar {
        OtherCar {
            id,
            pos,
            vel: Vec3::ZERO,
            forward: Vec3::Z,
            up: Vec3::Y,
            team: id as i32,
            is_player: false,
            hittable: true,
            progress_value: 10.0,
            penalty: 0.0,
            spline_distance: 0.0,
            race_position: 1,
            immune_spin: false,
            immune_damage: false,
            immune_explosions: false,
        }
    }

    fn kart() -> KartState {
        KartState { id: 0, is_player: true, character: 1, pos: Vec3::new(0.0, 1.0, 0.0), ..Default::default() }
    }

    #[test]
    fn values_from_real_xml() {
        let Some(p) = params() else { return };
        let mut a = BombAbility::new(0.0);
        a.load_values(&p);
        assert_eq!(a.radius, 25.0);
        assert_eq!(a.radius_sq, 625.0);
        assert_eq!(a.damage, 10.0);
        assert_eq!(a.spin_time, 3.0);
        assert_eq!(a.spins, 1.0);
        assert_eq!(a.explosive_force, 1500.0);
        assert_eq!(a.slowdown_force, 0.0);
        assert_eq!(a.base.duration, 0.5);
        assert_eq!(a.base.effect_duration, 0.5);
        assert_eq!(a.base.boost_strength, 2200.0);
        assert!(!a.base.play_anim);
        assert_eq!((a.base.ai_min_time, a.base.ai_max_time), (10.0, 60.0));
    }

    #[test]
    fn trigger_then_detonate_hits_cars_in_radius() {
        let Some(p) = params() else { return };
        let mut a = create_ability("Bomb", &p, 0.0).unwrap();
        let mut env = a.base().env.clone();
        env.effects = p.effects.clone();
        let mut near = other(1, Vec3::new(10.0, 1.0, 0.0));
        let far = other(2, Vec3::new(30.0, 1.0, 0.0));
        let mut spin_immune = other(3, Vec3::new(0.0, 1.0, 20.0));
        spin_immune.immune_spin = true;
        let mut busy = other(4, Vec3::new(5.0, 1.0, 5.0));
        busy.hittable = false;
        near.team = 7;
        env.others = vec![near, far, spin_immune, busy];
        a.set_env(env);
        let k = kart();
        let mut out = Vec::new();
        a.trigger_player(&k, &mut out);
        assert!(a.is_active());
        // the priming effect starts
        assert!(out.iter().any(|e| matches!(e, CarEffect::Particle { name, .. } if name == "BlackAbilityPriming")));
        out.clear();
        // 0.5 s duration: run the update until it finishes
        for _ in 0..40 {
            tick_ability(a.as_mut(), 1.0 / 60.0, &k, &mut out);
            if !a.base().active {
                break;
            }
        }
        assert!(!a.base().active);
        let dmg: Vec<_> = out.iter().filter_map(|e| if let CarEffect::Damage { car, amount, .. } = e { Some((*car, *amount)) } else { None }).collect();
        assert_eq!(dmg, vec![(1, 10.0), (3, 10.0)]);
        let spins: Vec<_> = out.iter().filter_map(|e| if let CarEffect::SpinOut { car, time, spins } = e { Some((*car, *time, *spins)) } else { None }).collect();
        assert_eq!(spins, vec![(1, 3.0, 1.0)]); // car 3 is immune to spin, car 2 is out of range, car 4 not hittable
        assert!(out.iter().any(|e| matches!(e, CarEffect::Explosion { force, .. } if *force == 1500.0)));
        // the end effect ("BlackAbility") plays when the effect timer runs out
        assert!(out.iter().any(|e| matches!(e, CarEffect::Particle { name, .. } if name == "BlackAbility")));
    }

    #[test]
    fn team_mode_spares_teammates() {
        let Some(p) = params() else { return };
        let mut a = BombAbility::new(0.0);
        a.load_values(&p);
        let mut env = a.base.env.clone();
        env.team_mode = true;
        env.team = 5;
        let mut mate = other(1, Vec3::new(3.0, 1.0, 0.0));
        mate.team = 5;
        let foe = other(2, Vec3::new(3.0, 1.0, 3.0));
        env.others = vec![mate, foe];
        a.set_env(env);
        a.base.active = true;
        a.base.triggered = true;
        let mut out = Vec::new();
        a.finish(&kart(), &mut out);
        let hit: Vec<_> = out.iter().filter_map(|e| if let CarEffect::Damage { car, .. } = e { Some(*car) } else { None }).collect();
        assert_eq!(hit, vec![2]);
    }

    #[test]
    fn ai_chance_counts_cars_ahead_in_range() {
        let Some(p) = params() else { return };
        let mut a = BombAbility::new(0.0);
        a.load_values(&p);
        let mut env = a.base.env.clone();
        env.progress_value = 5.0;
        let ahead = other(1, Vec3::new(10.0, 1.0, 0.0)); // progress 10 >= 5, in range
        let mut behind = other(2, Vec3::new(10.0, 1.0, 5.0));
        behind.progress_value = 1.0;
        let far = other(3, Vec3::new(100.0, 1.0, 0.0));
        let mut pen = other(4, Vec3::new(5.0, 1.0, 5.0));
        pen.penalty = 2.0;
        env.others = vec![ahead, behind, far, pen];
        a.set_env(env);
        a.my_pos = Vec3::new(0.0, 1.0, 0.0);
        assert!((a.ai_trigger_chance() - 0.25).abs() < 1e-6);
        // a teammate in range zeroes it
        a.base.env.others[1].team = a.base.env.team;
        a.base.env.others[1].pos = Vec3::new(1.0, 1.0, 0.0);
        assert_eq!(a.ai_trigger_chance(), 0.0);
    }
}
