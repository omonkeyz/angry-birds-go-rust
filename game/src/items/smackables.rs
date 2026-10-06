//! Smackable runtime (breakable blocks, TNT, route blockers, slalom posts, random power boxes ...).
//!
//! Ported from `CSmackable::Init @001d7134`, `CSmackable::CollisionCallback @001d531c`, `CSmackable::Update @001d5db4`,
//! `CSmackableManager::AddSmackable @001d9ecc`, `CCar::CollisionEnabledCallback @00196e90`, `CEnvObject::UpdateVisibility @001dac90`,
//! `CSmackable::ApplyExplodeForce @001d4ff0` (data table: `smackdefs.rs`).
//!
//! What is 1:1: the type table, the decision rule (`accumulated hit magnitude > smash_threshold`, `car_only`, `always_collide`, fixed-in-place
//! blockers), the activation distance (50 m + model radius), the debris caps (1024 permanent / 40 temporary), fragment spawning from the
//! model's helper nodes, the explosion force law.
//! What is NOT: the rigid-body solver. The original gets the hit magnitude from `CXGSPhys` (the first vector of the collision callback,
//! unit unresolved). Here it is `(1 + restitution) * mass * closing speed` of the kart sphere against the smackable's bounding box,
//! which is the impulse of an elastic hit against a heavy kart (the thresholds divided by the mass give plausible speeds: glass 3.8 m/s,
//! stone/wood 7.7 m/s, TNT crate 2.8 m/s). `UNRESOLVED:` exact unit and the collision shape (the original uses the model's `collision` hull).
use super::smackdefs::{self, SmackableDef};
use glam::{Mat4, Vec3};

/// `CEnvObject::UpdateVisibility`: a track smackable wakes up when the car is within `R + model radius` (R = 50, 300 in game mode 0xe).
pub const ACTIVATION_RADIUS: f32 = 50.0;
/// `AddSmackable` caps.
pub const MAX_PERMANENT: usize = 0x400;
pub const MAX_TEMPORARY: usize = 0x28;
/// `CSmackable::Update`: angular velocity decays by `1 - 0.5 dt`.
pub const ANGULAR_DAMPING: f32 = 0.5;
/// The gravity the bodies use (`DebugFloat(0x28) * -9.8` with the default debug float 1.0).
pub const GRAVITY: f32 = -9.8;
/// Explosion force law (`ApplyExplodeForce @001d4ff0`): `d2 = max(|d|^2, 4)`, `v = dir * strength / d2`; applied when `|v|^2 > 10`.
pub const EXPLOSION_MIN_D2: f32 = 4.0;
pub const EXPLOSION_MIN_V2: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmackState {
    /// not activated yet (kart far away): the env object is just a placed model
    Idle,
    /// live body
    Active,
    /// broken (the env object is removed / replaced by fragments)
    Smashed,
}

#[derive(Debug, Clone)]
pub struct Smackable {
    pub type_id: u32,
    /// pose of the item frame (X = normal x tangent, Y = ground normal, Z = along the road, origin = base point)
    pub world: Mat4,
    /// `pivot` node of the model (offset of the model centre from its origin); the model is drawn with `world * translate(-pivot)`
    pub pivot: Vec3,
    /// centre of the bounding box in model space, half extents of the model's bounding box
    pub bbox_center: Vec3,
    pub half_extents: Vec3,
    pub radius: f32,
    pub state: SmackState,
    /// permanent (placed on the track) or temporary (debris)
    pub temporary: bool,
    /// fixed in place (route blockers): no gravity, no movement
    pub fixed: bool,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    /// accumulated hit magnitude of this frame (`this+0x115c`)
    pub accum: f32,
    pub age: f32,
    /// index of the track item that owns it (None for debris)
    pub item: Option<usize>,
    /// ability objects: no gravity (`CXGSRigidBody::SetGravity`), model scale (boss spawn grows `InitialScale` -> 1), asleep until woken
    pub no_gravity: bool,
    pub scale: f32,
    pub asleep: bool,
    /// spawned by an ability of this car (the host does the kart contacts of these, `abilityrun.rs`)
    pub ability_owner: Option<usize>,
    /// explosion strength override (`smackable+0x1168`), ability objects
    pub explosion_override: Option<f32>,
}

impl Smackable {
    pub fn def(&self) -> &'static SmackableDef {
        &smackdefs::SMACKABLES[self.type_id as usize]
    }

    /// Matrix that maps model space to world (centre of the bbox sits at `world * (-pivot)` offset).
    pub fn model_matrix(&self) -> Mat4 {
        self.world * Mat4::from_scale(Vec3::splat(self.scale)) * Mat4::from_translation(-self.pivot)
    }

    /// World position of the bounding box centre.
    pub fn center(&self) -> Vec3 {
        self.model_matrix().transform_point3(self.bbox_center)
    }

    /// Closest point of the (oriented) bounding box to `p`, and whether `p` is inside.
    pub fn closest_point_on_box(&self, p: Vec3) -> (Vec3, bool) {
        let m = self.model_matrix();
        let local = m.inverse().transform_point3(p) - self.bbox_center;
        let clamped = local.clamp(-self.half_extents, self.half_extents);
        let inside = local == clamped;
        (m.transform_point3(clamped + self.bbox_center), inside)
    }

    /// Smash rule of `CSmackable::Update`: `threshold <= f32::MAX && accum > threshold` (the threshold is +inf for fragments, route blockers, pigs...).
    pub fn should_smash(&self) -> bool {
        let t = self.def().smash_threshold;
        t <= f32::MAX && self.accum > t
    }
}

/// Result of one kart-vs-smackable contact test.
#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub normal: Vec3,
    pub depth: f32,
    /// closing speed along the contact normal (>0 when approaching)
    pub closing_speed: f32,
}

/// Sphere (kart) against the smackable's oriented bounding box.
pub fn kart_contact(s: &Smackable, kart_pos: Vec3, kart_vel: Vec3, kart_radius: f32) -> Option<Contact> {
    let (closest, inside) = s.closest_point_on_box(kart_pos);
    let d = kart_pos - closest;
    let dist = d.length();
    if !inside && dist >= kart_radius {
        return None;
    }
    let normal = if inside || dist < 1e-5 { (kart_pos - s.center()).normalize_or_zero() } else { d / dist };
    let normal = if normal == Vec3::ZERO { Vec3::Y } else { normal };
    let rel = kart_vel - s.vel;
    Some(Contact { normal, depth: if inside { kart_radius } else { kart_radius - dist }, closing_speed: -rel.dot(normal) })
}

/// Hit magnitude fed into `accum` (see the module doc: impulse of an elastic hit against a heavy kart).
pub fn impact_magnitude(def: &SmackableDef, closing_speed: f32) -> f32 {
    if closing_speed <= 0.0 {
        return 0.0;
    }
    (1.0 + def.restitution) * def.mass * closing_speed
}

/// Velocity change of the kart from the contact: momentum exchange with the smackable (`kart_mass` in the same units as the smackable `mass`).
/// Fixed (route blocker) smackables reflect the kart's normal velocity with their restitution.
pub fn kart_response(def: &SmackableDef, fixed: bool, contact: &Contact, kart_mass: f32) -> Vec3 {
    if contact.closing_speed <= 0.0 {
        return Vec3::ZERO;
    }
    let m = if fixed { 1.0e9 } else { def.mass };
    let j = (1.0 + def.restitution) * contact.closing_speed * (m * kart_mass) / (m + kart_mass);
    contact.normal * (j / kart_mass)
}

/// Explosion impulse on a body at `d` from the blast centre: returns the velocity change or None when below the force threshold.
pub fn explosion_velocity(strength: f32, d: Vec3) -> Option<Vec3> {
    let d2 = d.length_squared().max(EXPLOSION_MIN_D2);
    let v = d * (strength / d2);
    (v.length_squared() > EXPLOSION_MIN_V2).then_some(v)
}

/// `CSmackable::Update` debris velocity: parent velocity + omega x r + a downward kick of one timestep of gravity (`-(dt * 9.8)`).
pub fn fragment_velocity(parent_vel: Vec3, ang_vel: Vec3, r: Vec3, dt: f32) -> Vec3 {
    parent_vel + ang_vel.cross(r) + Vec3::new(0.0, -(dt * 9.8), 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(type_id: u32) -> Smackable {
        Smackable {
            type_id,
            world: Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)),
            pivot: Vec3::new(0.0, -1.0, 0.0),
            bbox_center: Vec3::ZERO,
            half_extents: Vec3::splat(1.0),
            radius: 1.73,
            state: SmackState::Active,
            temporary: false,
            fixed: false,
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            accum: 0.0,
            age: 0.0,
            item: None,
            no_gravity: false,
            scale: 1.0,
            asleep: false,
            ability_owner: None,
            explosion_override: None,
        }
    }

    #[test]
    fn glass_breaks_on_a_fast_hit_but_stone_survives_the_same_speed() {
        // smck_block_glass_2x2 = type 55 (threshold 96, mass 25), smck_block_stone_2x2 = type 63 (threshold 576, mass 75)
        let mut glass = block(55);
        let mut stone = block(63);
        assert_eq!(glass.def().name, "smck_block_glass_2x2");
        assert_eq!(stone.def().name, "smck_block_stone_2x2");
        // the box centre is at y = 1 (origin 0 minus pivot -1): a kart ball hitting its -z face at 4 m/s
        let kart = Vec3::new(0.0, 1.0, 8.5);
        let c = kart_contact(&glass, kart, Vec3::new(0.0, 0.0, 4.0), 1.0).expect("overlap");
        assert!((c.closing_speed - 4.0).abs() < 1e-4);
        glass.accum = impact_magnitude(glass.def(), c.closing_speed);
        stone.accum = impact_magnitude(stone.def(), c.closing_speed);
        assert!(glass.should_smash(), "glass accum {}", glass.accum);
        assert!(!stone.should_smash(), "stone accum {}", stone.accum);
        // 12 m/s breaks the stone as well
        stone.accum = impact_magnitude(stone.def(), 12.0);
        assert!(stone.should_smash());
    }

    #[test]
    fn route_blockers_never_break_and_miss_when_far() {
        let mut b = block(93);
        assert_eq!(b.def().name, "smck_route_blocker");
        b.accum = 1.0e9;
        assert!(!b.should_smash(), "+inf threshold");
        assert!(b.def().always_collide);
        assert!(kart_contact(&block(55), Vec3::new(0.0, 1.0, 20.0), Vec3::ZERO, 1.0).is_none());
    }

    #[test]
    fn explosion_force_law() {
        // TNT strength 120: at 3 m, d2 = 9, v = 120/9*3/3... |v| = strength / |d| = 40 m/s; far away (> strength/sqrt(10) = 38 m) nothing
        let v = explosion_velocity(120.0, Vec3::new(3.0, 0.0, 0.0)).unwrap();
        assert!((v.length() - 40.0).abs() < 1e-3);
        assert!(explosion_velocity(120.0, Vec3::new(40.0, 0.0, 0.0)).is_none());
        assert!(explosion_velocity(120.0, Vec3::new(37.0, 0.0, 0.0)).is_some());
    }
}
