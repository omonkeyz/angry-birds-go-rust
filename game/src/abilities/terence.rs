//! Terence (Big Red) rage ability: `CTerenceRageAbility` (vtable id 9, "TerenceRage").
//!
//! Big Red roars: during `Duration` (1 s) he gets the generic boost (`BoostStrength`/`BoostDuration`, base class) and the
//! "BigRedAbility" start effect; `FinishAbility` then strikes every other car within `Radius` with lightning (impact
//! damage + spin-out, a "TerenceAbilityLightningEffect" instance attached to the victim). When nothing was hit,
//! `LightningOnMiss` lightning bolts land at random points in front of / beside the car.
//! Source: `libABK291_annotated.c` `CTerenceRageAbility::*` @001d3d5c..@001d4518.
//!
//! xml (`char_004.xml`, `<Ability name="TerenceRage">`) -> field, by call order of `LoadAbilityValuesFromXML @001d40b4`
//! (= xml order) and the ctor defaults:
//!
//! | field | `<Tag>` | ctor default | use |
//! |-------|---------|--------------|-----|
//! | +0x98 | `Radius` | 1.0 | strike radius (`+0x9c` = Radius squared); also the scatter radius of the miss bolts |
//! | +0xa0 | `Damage` | 10.0 | int arg of `CCar::AddImpactDamage` |
//! | +0xa4 | `SpinTime` | 1.0 | `Spin360` time (see UNRESOLVED below) |
//! | +0xa8 | `Spins` | 1.0 | `Spin360` int argument |
//! | +0xac | `LightningOnMiss` | 0 | `GetAbilityIntForLevel`: number of miss bolts |
//! | +0xb0 | `LightningEffect` | "" | `CXmlUtil::GetText` -> char[0x80] = particle effect name ("BigRedAbilityImpact") |
//!
//! `CamBehindTime` is in the xml but is read by nobody (the base loader only reads `CamBehindMod`).
//!
//! The 8 + 8 particle-instance slots (+0x130..+0x190) that the original keeps (and `OnCarIntegrate @001d4194` services,
//! moving each lightning effect with its victim's matrix and releasing finished instances) are VFX bookkeeping owned by
//! the host's particle system here: `finish` emits one [`CarEffect::Particle`] per strike and `on_integrate` is the base
//! implementation.

use super::*;
use super::bomb::{area_ai_trigger_chance, spin360_effect};

/// `CTerenceRageAbility` (see the module docs).
pub struct TerenceRageAbility {
    pub base: AbilityBase,
    /// +0x98 `<Radius>`.
    pub radius: f32,
    /// +0x9c `Radius * Radius`.
    pub radius_sq: f32,
    /// +0xa0 `<Damage>`.
    pub damage: f32,
    /// +0xa4 `<SpinTime>`.
    pub spin_time: f32,
    /// +0xa8 `<Spins>`.
    pub spins: f32,
    /// +0xac `<LightningOnMiss>` (int).
    pub lightning_on_miss: i32,
    /// +0xb0 `<LightningEffect>`.
    pub lightning_effect: String,
    /// Own position cached from the last frame/step (see [`super::bomb::BombAbility::my_pos`]).
    pub my_pos: Vec3,
}

impl TerenceRageAbility {
    /// `CTerenceRageAbility::CTerenceRageAbility(CCar*) @001d4010` (`+0x64 = 1`: pilot animation stays on).
    pub fn new(level: f32) -> TerenceRageAbility {
        let base = AbilityBase::new(level);
        TerenceRageAbility {
            base,
            radius: 1.0,
            radius_sq: 1.0,
            damage: 10.0, // 0x41200000
            spin_time: 1.0,
            spins: 1.0,
            lightning_on_miss: 0,
            lightning_effect: String::new(),
            my_pos: Vec3::ZERO,
        }
    }
}

impl Ability for TerenceRageAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    /// `CTerenceRageAbility::GetId @001d3d6c`.
    fn id(&self) -> BirdAbility {
        BirdAbility::TerenceRage
    }
    /// `CTerenceRageAbility::GetAbilityName @001d3d5c`.
    fn name(&self) -> &str {
        "TerenceRage"
    }

    /// `CTerenceRageAbility::LoadAbilityValuesFromXML @001d40b4`: Radius (+ square), Damage, SpinTime, Spins,
    /// LightningOnMiss (`GetAbilityIntForLevel`), LightningEffect (text), then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.radius = p.float_for_level("Radius", 0.0, lvl);
        self.radius_sq = self.radius * self.radius;
        self.damage = p.float_for_level("Damage", 0.0, lvl);
        self.spin_time = p.float_for_level("SpinTime", 0.0, lvl);
        self.spins = p.float_for_level("Spins", 0.0, lvl);
        self.lightning_on_miss = p.int_for_level("LightningOnMiss", lvl);
        self.lightning_effect = p.scalar_str("LightningEffect").unwrap_or("").to_string();
        base_load_values(self, p);
    }

    fn always_update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.my_pos = kart.pos;
        base_always_update(self, dt, kart, out);
    }

    /// `CTerenceRageAbility::OnCarIntegrate @001d4194`: services the particle-instance slots (host-owned here), then
    /// `CBaseAbility::OnCarIntegrate` (the generic boost + dust).
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.my_pos = kart.pos;
        base_on_integrate(self, kart, out);
    }

    /// `CTerenceRageAbility::CalcCurrentAITriggerChance @001d3e18` (see [`area_ai_trigger_chance`]).
    fn ai_trigger_chance(&self) -> f32 {
        area_ai_trigger_chance(&self.base.env, self.my_pos, self.radius_sq)
    }

    /// `CTerenceRageAbility::FinishAbility @001d42cc`. Only when `IsActive()`:
    /// * every other car with `CCar+0x42c <= 0` (`hittable`), not a teammate in team mode (`CGame+0x32b0 == 3`), and
    ///   within `Radius`: `AddImpactDamage(pos, _, (int)Damage)` -> [`CarEffect::Damage`]; `Spin360(SpinTime, _, Spins)`
    ///   -> [`CarEffect::SpinOut`] (skipped when the victim is `immune_spin`); the lightning effect
    ///   ([`CarEffect::Particle`] `name = LightningEffect`, `car = victim`; at most 8 live at once in the original) and
    ///   `ABKSound::CAbilityController::OnEvent(3, victim)`. Any victim counts as a hit even when no effect slot was free.
    /// * `CEnvObjectManager::GetNearbyPickups(pos, 10.0)`: pickups get `vtbl+0x44/+0x48` and a lightning effect too and
    ///   count as hits. UNRESOLVED (no pickup list in the env): reported as `CarEffect::Other{tag:
    ///   "CEnvObjectManager::GetNearbyPickups"}` with `vals = [10.0, Radius, 0, 0]`, never counted as a hit.
    /// * no hit: `LightningOnMiss` bolts: each at `pos + Radius * u * A + Radius * v * B` with `u` uniform in [0, 1],
    ///   `v` uniform in [-1, 1] (`2r - 1`), `A`/`B` the two axes read from the chassis body (`rb+0x34` -> +0x1c.. and
    ///   +0x28..). UNRESOLVED which axes those are: taken as `A` = forward, `B` = right (strikes ahead and to the sides).
    ///   Emitted as `CarEffect::Particle{ car: None, pos }` (a translation-matrix effect, 8 slots in the original).
    ///
    /// Then `CBaseAbility::FinishAbility`.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.base.is_active() {
            let pos = kart.pos;
            let team_mode = self.base.env.team_mode;
            let my_team = self.base.env.team;
            let others = self.base.env.others.clone();
            let mut hit = false;
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
                    if let Some(e) = spin360_effect(o, self.spin_time, self.spins) {
                        out.push(e);
                    }
                    out.push(CarEffect::Particle { name: self.lightning_effect.clone(), car: Some(o.id), pos: o.pos });
                    out.push(CarEffect::Other { tag: "ABKSound::CAbilityController::OnEvent(3)", car: Some(o.id), vals: [3.0, 0.0, 0.0, 0.0], pos: o.pos });
                    hit = true;
                }
            }
            out.push(CarEffect::Other { tag: "CEnvObjectManager::GetNearbyPickups", car: Some(kart.id), vals: [10.0, self.radius, 0.0, 0.0], pos });
            if !hit {
                let right = kart.up.cross(kart.forward);
                for _ in 0..self.lightning_on_miss.max(0) {
                    let u = self.base.env.rand01();
                    let v = 2.0 * self.base.env.rand01() - 1.0;
                    let p = pos + kart.forward * (self.radius * u) + right * (self.radius * v);
                    out.push(CarEffect::Particle { name: self.lightning_effect.clone(), car: None, pos: p });
                }
            }
        }
        base_finish(self, kart, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Option<AbilityParams> {
        let xml = std::fs::read_to_string(format!("{}/xml/characters/charxml/char_004.xml", ASSETS292)).ok()?;
        AbilityParams::from_character_xml(&xml)
    }

    fn other(id: usize, pos: Vec3) -> OtherCar {
        OtherCar {
            id,
            pos,
            vel: Vec3::ZERO,
            forward: Vec3::Z,
            up: Vec3::Y,
            team: id as i32 + 100,
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
        KartState { id: 0, is_player: true, character: 3, pos: Vec3::new(0.0, 1.0, 0.0), ..Default::default() }
    }

    #[test]
    fn values_from_real_xml() {
        let Some(p) = params() else { return };
        let mut a = TerenceRageAbility::new(0.0);
        a.load_values(&p);
        assert_eq!(a.radius, 12.0);
        assert_eq!(a.radius_sq, 144.0);
        assert_eq!(a.damage, 15.0);
        assert_eq!(a.spin_time, 2.0);
        assert_eq!(a.spins, 1.0);
        assert_eq!(a.lightning_on_miss, 3);
        assert_eq!(a.lightning_effect, "BigRedAbilityImpact");
        assert_eq!(a.base.duration, 1.0);
        assert_eq!(a.base.boost_strength, 2200.0);
        assert_eq!(a.base.boost_duration, 2.0);
        assert!(a.base.play_anim);
    }

    #[test]
    fn strike_hits_cars_in_radius_and_skips_lightning_on_miss() {
        let Some(p) = params() else { return };
        let mut a = create_ability("TerenceRage", &p, 0.0).unwrap();
        let mut env = a.base().env.clone();
        env.effects = p.effects.clone();
        env.others = vec![other(1, Vec3::new(6.0, 1.0, 0.0)), other(2, Vec3::new(20.0, 1.0, 0.0))];
        a.set_env(env);
        let k = kart();
        let mut out = Vec::new();
        a.trigger_player(&k, &mut out);
        assert!(out.iter().any(|e| matches!(e, CarEffect::Particle { name, .. } if name == "BigRedAbility")));
        out.clear();
        for _ in 0..120 {
            tick_ability(a.as_mut(), 1.0 / 60.0, &k, &mut out);
            if !a.base().active {
                break;
            }
        }
        assert!(!a.base().active);
        assert!(out.iter().any(|e| matches!(e, CarEffect::Damage { car: 1, amount, .. } if *amount == 15.0)));
        assert!(!out.iter().any(|e| matches!(e, CarEffect::Damage { car: 2, .. })));
        assert!(out.iter().any(|e| matches!(e, CarEffect::SpinOut { car: 1, time, spins } if *time == 2.0 && *spins == 1.0)));
        let bolts: Vec<_> = out.iter().filter(|e| matches!(e, CarEffect::Particle { name, car: None, .. } if name == "BigRedAbilityImpact")).collect();
        assert!(bolts.is_empty(), "no lightning on miss after a hit");
    }

    #[test]
    fn miss_spawns_lightning_bolts_ahead() {
        let Some(p) = params() else { return };
        let mut a = TerenceRageAbility::new(0.0);
        a.load_values(&p);
        let mut env = a.base.env.clone();
        env.others = vec![other(1, Vec3::new(60.0, 1.0, 0.0))];
        a.set_env(env);
        a.base.active = true;
        a.base.triggered = true;
        let k = kart();
        let mut out = Vec::new();
        a.finish(&k, &mut out);
        let bolts: Vec<Vec3> = out
            .iter()
            .filter_map(|e| if let CarEffect::Particle { name, car: None, pos } = e { (name == "BigRedAbilityImpact").then_some(*pos) } else { None })
            .collect();
        assert_eq!(bolts.len(), 3);
        for b in bolts {
            let d = b - k.pos;
            assert!(d.z >= 0.0 && d.z <= 12.0 && d.x.abs() <= 12.0);
        }
        assert!(!a.base.active);
    }

    #[test]
    fn ai_chance_matches_bomb_rule() {
        let Some(p) = params() else { return };
        let mut a = TerenceRageAbility::new(0.0);
        a.load_values(&p);
        let mut env = a.base.env.clone();
        env.progress_value = 5.0;
        env.others = vec![other(1, Vec3::new(5.0, 1.0, 0.0)), other(2, Vec3::new(500.0, 1.0, 0.0))];
        a.set_env(env);
        a.my_pos = Vec3::new(0.0, 1.0, 0.0);
        assert!((a.ai_trigger_chance() - 0.5).abs() < 1e-6);
    }
}
