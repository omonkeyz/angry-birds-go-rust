//! The speed-type abilities: `CSpeedAbility` (Yellow / Senna / SennaHelmet "SpeedBoost": physics time scale + global
//! slow-mo), `CRedSpeedAbility` ("RedSpeedBoost"), `CBlueSpeedAbility` ("BlueSpeedBoost", 3 charges) and
//! `COvertakeSpeedAbility` (boss "OvertakeSpeedBoost"). All numbers come from the character / boss xml
//! (`assets292/xml/characters/charxml/char_*.xml`, `boss_*.xml`) through [`AbilityParams`].

use super::*;

/// `CRedSpeedAbility::OnCarIntegrate @001cf7c8`, `COvertakeSpeedAbility::OnCarIntegrate @001cf5a8`,
/// `CBlueSpeedAbility::OnCarIntegrate @001be368`: while the ability is running a forward force of
/// `rb+0x98 * SpeedForce` (an impulse in carsim's convention), then `CBaseAbility::OnCarIntegrate`.
fn speed_force_integrate<A: Ability + ?Sized>(a: &mut A, force: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
    if a.base().active {
        out.push(CarEffect::BodyForce { car: kart.id, force_local: Vec3::new(0.0, 0.0, a.base().env.physics_dt * force), at_local: Vec3::ZERO });
    }
    base_on_integrate(a, kart, out);
}

/// `max(CGame+0x3298, 2.0)` request (player triggers of Red / Blue / Yellow abilities).
fn grace_effect(kart: &KartState) -> CarEffect {
    CarEffect::Other { tag: "CGame+0x3298=max(old,2.0)", car: Some(kart.id), vals: [GAME_ABILITY_GRACE_TIME, 0.0, 0.0, 0.0], pos: kart.pos }
}

// ---------------------------------------------------------------------------------------------------------------------
// CSpeedAbility
// ---------------------------------------------------------------------------------------------------------------------

/// `CSpeedAbility` (vtable @00d92d90). Yellow bird "SpeedBoost": the car's rigid-body time scale is set to
/// `IntegrateScalar` (rb+0xc float / rb+0x114 int) and the whole game slows to `1/SlowMoScalar` for
/// `SlowMoScalar * Duration` seconds, so the car appears to run normally while the world crawls.
pub struct SpeedAbility {
    pub base: AbilityBase,
    /// +0x98: "spent" latch, set by `FinishAbility` when `CanRetrigger()` is false (never in the shipped config).
    pub spent: bool,
    /// +0x9c `<IntegrateScalar>` (int, `GetAbilityIntForLevel`; ctor default 3).
    pub integrate_scalar: i32,
    /// +0xa0 `<SlowMoScalar>` (ctor default 2.5).
    pub slowmo_scalar: f32,
}

impl SpeedAbility {
    /// `CSpeedAbility::CSpeedAbility(CCar*) @001cf8cc`.
    pub fn new(level: f32) -> SpeedAbility {
        let mut base = AbilityBase::new(level);
        base.cam_behind_mod = 1.0;
        SpeedAbility { base, spent: false, integrate_scalar: 3, slowmo_scalar: 2.5 }
    }

    fn set_time_scale(&self, kart: &KartState, scalar: i32, out: &mut Vec<CarEffect>) {
        out.push(CarEffect::SetTimeScale { car: kart.id, scale: scalar as f32, integrate_scalar: scalar });
        if self.base.env.part_bodies > 0 {
            // The same two stores go to every attached part body (`*(*(CCar+0x548)+0xe1c)` of them).
            out.push(CarEffect::Other {
                tag: "SetTimeScale(part bodies)",
                car: Some(kart.id),
                vals: [scalar as f32, scalar as f32, self.base.env.part_bodies as f32, 0.0],
                pos: kart.pos,
            });
        }
    }
}

impl Ability for SpeedAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::SpeedBoost
    }
    /// `CSpeedAbility::GetAbilityName @001cf830`.
    fn name(&self) -> &str {
        "SpeedBoost"
    }

    /// `CSpeedAbility::LoadAbilityValuesFromXML @001cf918`: IntegrateScalar -> +0x9c, SlowMoScalar -> +0xa0,
    /// CamBehindMod -> +0x5c, then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.integrate_scalar = p.int_for_level("IntegrateScalar", lvl);
        self.slowmo_scalar = p.float_for_level("SlowMoScalar", 0.0, lvl);
        self.base.cam_behind_mod = p.float_for_level("CamBehindMod", 0.0, lvl);
        base_load_values(self, p);
    }

    /// `CSpeedAbility::TriggerAbility @001cf98c`. After the base trigger: for players, a shockwave post-process (local
    /// player only) and `CGame::EnterSlowMo(1/SlowMoScalar, SlowMoScalar*Duration, 0, 0)` + the 2.0 grace floor; for
    /// every car the time scale of the chassis (and part bodies) becomes `IntegrateScalar`; players' pilot animation
    /// runs at `SlowMoScalar`.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        base_trigger(self, kart, out);
        if kart.is_player {
            if self.base.env.is_local_player {
                // `CPostProcess_Shockwave::TriggerCustomShockwave(screen_x, screen_y, 60.0, 2.0, 0.85, 0.0)`, the
                // screen position being the car's position relative to the active camera.
                out.push(CarEffect::Other {
                    tag: "CPostProcess_Shockwave::TriggerCustomShockwave",
                    car: Some(kart.id),
                    vals: [60.0, 2.0, 0.85, 0.0],
                    pos: kart.pos,
                });
            }
            // CGame::EnterSlowMo(this, scale, hold, ramp_in, ramp_out): scale = 1/s, hold = s * (+0x10), ramps 0 / 0.
            out.push(CarEffect::SlowMo { scale: 1.0 / self.slowmo_scalar, seconds: self.slowmo_scalar * self.base.duration });
            out.push(grace_effect(kart));
        }
        self.set_time_scale(kart, self.integrate_scalar, out);
        if kart.is_player {
            out.push(CarEffect::Other { tag: "CPilotAnimationHandler::SetAnimRate", car: Some(kart.id), vals: [self.slowmo_scalar, 0.0, 0.0, 0.0], pos: kart.pos });
        }
    }

    /// `CSpeedAbility::FinishAbility @001cfd70`: latch when not retriggerable, restore time scale 1.0 / int 1 on the
    /// chassis and every part body, base finish, pilot animation rate 1.0 (players), music speed offset 0.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if !self.base.env.debug.can_retrigger() {
            self.spent = true;
        }
        self.set_time_scale(kart, 1, out);
        base_finish(self, kart, out);
        if kart.is_player {
            out.push(CarEffect::Other { tag: "CPilotAnimationHandler::SetAnimRate", car: Some(kart.id), vals: [1.0, 0.0, 0.0, 0.0], pos: kart.pos });
        }
        out.push(CarEffect::Other { tag: "ABKSound::CMusicController::SetMusicSpeed", car: Some(kart.id), vals: [0.0, 0.0, 0.0, 0.0], pos: kart.pos });
    }

    /// `CSpeedAbility::CanTriggerAbility @001cfe28`.
    fn can_trigger(&self, kart: &KartState) -> bool {
        if self.spent {
            return false;
        }
        base_can_trigger(&self.base, kart)
    }

    /// `CSpeedAbility::OnCarUpdate @001cfe44`: players hear the music speed ramp down/up during the first and last
    /// half second (`SetMusicSpeed(-(D - remaining))` / `SetMusicSpeed(-remaining)`), then `CBaseAbility::OnCarUpdate`.
    fn update(&mut self, dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        if kart.is_player {
            let d = self.duration();
            let rem = self.base.time_remaining;
            let elapsed = d - rem;
            let music = if elapsed < 0.5 {
                Some(-elapsed)
            } else if rem < 0.5 {
                Some(-rem)
            } else {
                None
            };
            if let Some(m) = music {
                out.push(CarEffect::Other { tag: "ABKSound::CMusicController::SetMusicSpeed", car: Some(kart.id), vals: [m, 0.0, 0.0, 0.0], pos: kart.pos });
            }
        }
        base_update(self, dt, kart, out);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CRedSpeedAbility
// ---------------------------------------------------------------------------------------------------------------------

/// `CRedSpeedAbility` (vtable @00d92cd0): a plain forward push of `SpeedForce` for `Duration` seconds with reduced
/// steering (`SteeringMultiplier`). Red: Duration 1.66 s, SpeedForce 5500, SteeringMultiplier 0.75, reuse 5.0.
pub struct RedSpeedAbility {
    pub base: AbilityBase,
    /// +0x98 "spent" latch (see [`SpeedAbility::spent`]).
    pub spent: bool,
    /// +0x9c `<SteeringMultiplier>` (ctor 1.0).
    pub steering_multiplier: f32,
    /// +0xa0 `<SpeedForce>` (ctor 0).
    pub speed_force: f32,
}

impl RedSpeedAbility {
    /// `CRedSpeedAbility::CRedSpeedAbility(CCar*) @001cf6a8`.
    pub fn new(level: f32) -> RedSpeedAbility {
        let mut base = AbilityBase::new(level);
        base.cam_behind_mod = 1.0;
        RedSpeedAbility { base, spent: false, steering_multiplier: 1.0, speed_force: 0.0 }
    }
}

impl Ability for RedSpeedAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::RedSpeedBoost
    }
    /// `CRedSpeedAbility::GetAbilityName @001cf610`.
    fn name(&self) -> &str {
        "RedSpeedBoost"
    }

    /// `CRedSpeedAbility::LoadAbilityValuesFromXML @001cf6ec`: SteeringMultiplier -> +0x9c, SpeedForce -> +0xa0 (both
    /// default 0 when missing), then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.steering_multiplier = p.float_for_level("SteeringMultiplier", 0.0, lvl);
        self.speed_force = p.float_for_level("SpeedForce", 0.0, lvl);
        base_load_values(self, p);
    }

    /// `CRedSpeedAbility::TriggerAbility @001cf744`: players raise the game grace timer to >= 2.0, then base trigger.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if kart.is_player {
            out.push(grace_effect(kart));
        }
        base_trigger(self, kart, out);
    }

    /// `CRedSpeedAbility::FinishAbility @001cf78c`.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if !self.base.env.debug.can_retrigger() {
            self.spent = true;
        }
        base_finish(self, kart, out);
    }

    /// `CRedSpeedAbility::CanTriggerAbility @001cf7b0`.
    fn can_trigger(&self, kart: &KartState) -> bool {
        if self.spent {
            return false;
        }
        base_can_trigger(&self.base, kart)
    }

    /// `CRedSpeedAbility::OnCarIntegrate @001cf7c8`.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        let f = self.speed_force;
        speed_force_integrate(self, f, kart, out);
    }

    /// `CRedSpeedAbility::OnCarSetSteering @001cf628`: `steer * SteeringMultiplier`.
    fn on_set_steering(&self, steer: f32) -> f32 {
        steer * self.steering_multiplier
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CBlueSpeedAbility
// ---------------------------------------------------------------------------------------------------------------------

/// `CBlueSpeedAbility` (vtable @00d92100): like Red but with `ChargeCount` 3 and no "spent" latch; it cannot be
/// retriggered while running. Blue: Duration 2.0, SpeedForce 2000, SteeringMultiplier 0.75, ChargeCount 3.
/// (`char_005.xml` carries a `<ReuseDelay>` element that no loader reads: the reuse delay is the 5.0 default.)
pub struct BlueSpeedAbility {
    pub base: AbilityBase,
    /// +0x98 `<SteeringMultiplier>` (ctor 1.0).
    pub steering_multiplier: f32,
    /// +0x9c `<SpeedForce>` (ctor 0).
    pub speed_force: f32,
}

impl BlueSpeedAbility {
    /// `CBlueSpeedAbility::CBlueSpeedAbility(CCar*) @001be220`.
    pub fn new(level: f32) -> BlueSpeedAbility {
        let mut base = AbilityBase::new(level);
        base.cam_behind_mod = 1.0;
        BlueSpeedAbility { base, steering_multiplier: 1.0, speed_force: 0.0 }
    }
}

impl Ability for BlueSpeedAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::BlueSpeedBoost
    }
    /// `CBlueSpeedAbility::GetAbilityName @001be0c4`.
    fn name(&self) -> &str {
        "BlueSpeedBoost"
    }

    /// `CBlueSpeedAbility::LoadAbilityValuesFromXML @001be25c`: SteeringMultiplier -> +0x98, SpeedForce -> +0x9c,
    /// CamBehindMod -> +0x5c, then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        let lvl = self.base.level;
        self.steering_multiplier = p.float_for_level("SteeringMultiplier", 0.0, lvl);
        self.speed_force = p.float_for_level("SpeedForce", 0.0, lvl);
        self.base.cam_behind_mod = p.float_for_level("CamBehindMod", 0.0, lvl);
        base_load_values(self, p);
    }

    /// `CBlueSpeedAbility::TriggerAbility @001be2d4`: `StopEffects()` first (restarts the effect), players raise the
    /// game grace timer, then base trigger.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        self.stop_effects(kart, out);
        if kart.is_player {
            out.push(grace_effect(kart));
        }
        base_trigger(self, kart, out);
    }

    /// `CBlueSpeedAbility::CanTriggerAbility @001be33c`: not while running.
    fn can_trigger(&self, kart: &KartState) -> bool {
        if self.base.active {
            return false;
        }
        base_can_trigger(&self.base, kart)
    }

    /// `CBlueSpeedAbility::OnCarIntegrate @001be368`.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        let f = self.speed_force;
        speed_force_integrate(self, f, kart, out);
    }

    /// `CBlueSpeedAbility::OnCarSetSteering @001be0dc`.
    fn on_set_steering(&self, steer: f32) -> f32 {
        steer * self.steering_multiplier
    }

    /// `CBlueSpeedAbility::OnCarCamBehindMod @001be148`: the base formula without the `CamBehindMod >= 0` guard.
    fn on_cam_behind_mod(&self, _input: f32) -> f32 {
        let d = self.duration();
        let rem = if self.base.time_remaining <= 0.0 { 0.0 } else { self.base.time_remaining };
        (1.0 - (d - rem) / d) * self.base.cam_behind_mod
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// COvertakeSpeedAbility
// ---------------------------------------------------------------------------------------------------------------------

/// `COvertakeSpeedAbility` (vtable @00d92c28): boss/AI catch-up boost. It starts when the car is `StartDistance` or more
/// behind (`CanTriggerAtDistance`), pushes with `SpeedForce`, scales impact damage by `DamageMultiplier`, overrides the
/// AI speed factors with `BoostAI`, and ends once the distance parameter reaches `EndDistance`. No `boss_*.xml` in
/// `assets292` carries this ability, so it is covered by a synthetic-xml test only.
pub struct OvertakeSpeedAbility {
    pub base: AbilityBase,
    /// +0x98 `<StartDistance>` (ctor 10.0).
    pub start_distance: f32,
    /// +0x9c `<EndDistance>` (ctor 1.0).
    pub end_distance: f32,
    /// +0xa0 `<SpeedForce>` (ctor 0).
    pub speed_force: f32,
    /// +0xa4 `<BoostAI>` (default 1.0).
    pub boost_ai: f32,
    /// +0xa8 `<DamageMultiplier>` (ctor 1.0).
    pub damage_multiplier: f32,
    /// +0xac last distance passed to `CanTriggerAtDistance` (ctor 0).
    pub distance: f32,
    /// +0xb0 / +0xb4 saved AI factors, restored on finish.
    pub saved_ai_factors: (f32, f32),
    /// +0xb8 name buffer, ctor "OvertakeSpeed", replaced by the `name` attribute.
    pub ability_name: String,
}

impl OvertakeSpeedAbility {
    /// `COvertakeSpeedAbility::COvertakeSpeedAbility(CCar*) @001cf360` (+0x60 = 1: boss ability).
    pub fn new(level: f32) -> OvertakeSpeedAbility {
        let mut base = AbilityBase::new(level);
        base.is_boss = true;
        OvertakeSpeedAbility {
            base,
            start_distance: 10.0,
            end_distance: 1.0,
            speed_force: 0.0,
            boost_ai: 1.0,
            damage_multiplier: 1.0,
            distance: 0.0,
            saved_ai_factors: (0.0, 0.0),
            ability_name: "OvertakeSpeed".to_string(),
        }
    }

    fn restore(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        out.push(CarEffect::Other { tag: "CCar+0x1b50", car: Some(kart.id), vals: [0.0; 4], pos: kart.pos });
        out.push(CarEffect::Other {
            tag: "AI speed factors (+0x12c,+0x130) restore",
            car: Some(kart.id),
            vals: [self.saved_ai_factors.0, self.saved_ai_factors.1, 0.0, 0.0],
            pos: kart.pos,
        });
    }
}

impl Ability for OvertakeSpeedAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::OvertakeSpeedBoost
    }
    /// `COvertakeSpeedAbility::GetAbilityName @001cf2c0`: the +0xb8 buffer.
    fn name(&self) -> &str {
        &self.ability_name
    }

    /// `COvertakeSpeedAbility::LoadAbilityValuesFromXML @001cf3e0` (plain `CXmlUtil::GetFloat` child texts, no level
    /// interpolation; missing -> 0 except BoostAI 1.0), the `name` attribute, then the base loader.
    fn load_values(&mut self, p: &AbilityParams) {
        self.start_distance = p.scalar_f32("StartDistance", 0.0);
        self.end_distance = p.scalar_f32("EndDistance", 0.0);
        self.speed_force = p.scalar_f32("SpeedForce", 0.0);
        self.damage_multiplier = p.scalar_f32("DamageMultiplier", 0.0);
        self.boost_ai = p.scalar_f32("BoostAI", 1.0);
        if let Some(n) = p.attr("name") {
            self.ability_name = n.to_string();
        }
        base_load_values(self, p);
    }

    /// `COvertakeSpeedAbility::TriggerAbility @001cf4b0`: no-op while running; marks the car, saves the AI speed factors
    /// and overrides both with `BoostAI`; then base trigger.
    fn trigger(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.base.active {
            return;
        }
        out.push(CarEffect::Other { tag: "CCar+0x1b50", car: Some(kart.id), vals: [1.0, 0.0, 0.0, 0.0], pos: kart.pos });
        self.saved_ai_factors = self.base.env.ai_boost_factors;
        out.push(CarEffect::Other {
            tag: "AI speed factors (+0x12c,+0x130) set",
            car: Some(kart.id),
            vals: [self.boost_ai, self.boost_ai, 0.0, 0.0],
            pos: kart.pos,
        });
        base_trigger(self, kart, out);
    }

    /// `COvertakeSpeedAbility::OnCarUpdate @001cf4fc`: replaces the base timer entirely - the ability only finishes
    /// once the distance parameter has reached `EndDistance`.
    fn update(&mut self, _dt: f32, kart: &KartState, out: &mut Vec<CarEffect>) {
        if self.distance < self.end_distance {
            return;
        }
        self.finish(kart, out);
    }

    /// `COvertakeSpeedAbility::FinishAbility @001cf56c`.
    fn finish(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        if !self.base.active {
            return;
        }
        self.restore(kart, out);
        base_finish(self, kart, out);
    }

    /// `COvertakeSpeedAbility::OnCarIntegrate @001cf5a8`.
    fn on_integrate(&mut self, kart: &KartState, out: &mut Vec<CarEffect>) {
        let f = self.speed_force;
        speed_force_integrate(self, f, kart, out);
    }

    /// `COvertakeSpeedAbility::OnCarImpactDamage @001cf2f4`: `damage * DamageMultiplier`.
    fn on_impact_damage(&mut self, damage: f32) -> f32 {
        damage * self.damage_multiplier
    }

    /// `COvertakeSpeedAbility::CanTriggerAtDistance(a, dist) @001cf2d0`: remembers `dist` and allows the trigger when the
    /// car is at least `StartDistance` behind (`dist <= -StartDistance`).
    fn can_trigger_at_distance(&mut self, _a: f32, dist: f32) -> bool {
        self.distance = dist;
        self.start_distance <= -dist
    }
}
