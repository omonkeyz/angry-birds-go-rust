//! World placement of a track item: port of `CEnvObjectManager::GenerateHelperAtSplineRelativePosition @001df28c`
//! (called per record by `CalculateEventDefinitionTrackItemsMutableData @001dfee4`).
//!
//! Maths, as read from the decompile:
//!  * spline lookup by name (case-insensitive) among the track's `CSpline`s;
//!  * `t = fraction * N` (N = point count); when `fraction >= 1.0` it is a distance in metres: `t *= 1 / length`;
//!    stripes add `index * N * spacing / length`;  `pos = GetPosition(t)`;
//!  * lateral: `pos += lateral * width * right[(int)t]` where `width` = min(left or right half width at t, next point) - the right
//!    width when `lateral > 0`, the left width otherwise (so `lateral` is a fraction of the half width, positive = right);
//!  * bar: `+ index * spacing * right` when `lateral > 0`, `- index * spacing * right` when `lateral <= 0`;
//!    tower: `+ index * spacing * (0,1,0)`;
//!  * snap to the collision surface with `GetGeometryBelow`: use the hit when it is within 20 m (squared 400, `DAT_001df6cc`);
//!    else retry from 10 m higher (`DAT_001dfed4` = 400); else keep the spline point with normal (0,1,0);  `y = hit.y + height`;
//!  * orientation: Y = surface normal, X = normal x tangent, Z = X x normal; then the local offset (`+0x5c` along X, `+0x58` along Y).
use super::eventdef::{ItemRecord, Pattern};
use super::ground::ItemGround;
use super::spline::CSpline;
use glam::{Mat4, Vec3};

/// Squared snap distance (`DAT_001df6cc` / `DAT_001dfed4`, both read from libABK291.so = 400.0).
pub const SNAP_DIST_SQ: f32 = 400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapResult {
    /// found the surface from the spline-level position
    Direct,
    /// found it only after retrying 10 m higher
    Raised,
    /// no surface within range: spline point kept
    NoSurface,
}

#[derive(Debug, Clone)]
pub struct Placed {
    /// world transform: columns = X (normal x tangent), Y (normal), Z (X x normal), translation = position
    pub world: Mat4,
    /// pre-snap spline-relative position
    pub spline_pos: Vec3,
    pub snap: SnapResult,
    /// the spline index (`t`) the placement used, before clamping
    pub t: f32,
    /// true when `t` fell outside `0..N` (the original reads out of bounds there)
    pub t_out_of_range: bool,
    pub ground_material: Option<u16>,
}

pub fn find_spline<'a>(splines: &'a [CSpline], name: &str) -> Option<&'a CSpline> {
    splines.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

pub fn place(r: &ItemRecord, splines: &[CSpline], ground: &ItemGround) -> Option<Placed> {
    // explicit `position="x y z"` items (pickup_egg): the original builds a translation matrix from it (MakeTranslationMatrix32 path).
    if let Some(p) = r.position {
        return Some(Placed {
            world: Mat4::from_translation(p),
            spline_pos: p,
            snap: SnapResult::NoSurface,
            t: 0.0,
            t_out_of_range: false,
            ground_material: None,
        });
    }
    let sp = find_spline(splines, &r.spline)?;
    let n = sp.count();
    if n < 2 {
        return None;
    }
    let nf = n as f32;
    let mut t = r.fraction * nf;
    if !(r.fraction < 1.0) {
        t *= 1.0 / sp.length;
    }
    if r.pattern == Pattern::Stripe {
        t += (r.index as f32 * nf) * r.spacing * (1.0 / sp.length);
    }
    let out_of_range = t < 0.0 || t >= nf;
    let mut pos = sp.position(t);
    let pi = sp.clamp_index(t);
    let pt = sp.points[pi];
    let width = if r.lateral > 0.0 { sp.right_width(t) } else { sp.left_width(t) };
    let lat = r.lateral * width;
    pos += pt.right * lat;
    match r.pattern {
        Pattern::Bar => {
            let off = if r.lateral > 0.0 { r.index as f32 * r.spacing } else { -(r.index as f32 * r.spacing) };
            pos += pt.right * off;
        }
        Pattern::Tower => pos += Vec3::Y * (r.index as f32 * r.spacing),
        // UNRESOLVED: Pattern::Ring (sin/cos branch is garbled in the decompile; no shipped event uses it)
        _ => {}
    }
    let spline_pos = pos;

    // ground snap (GetGeometryBelow, retried 10 m higher, else keep the spline point)
    let mut snap = SnapResult::Direct;
    let mut normal = Vec3::Y;
    let mut surface_y = pos.y;
    let mut material = None;
    let mut ok = false;
    if let Some(h) = ground.below(pos) {
        if (h.point - pos).length_squared() < SNAP_DIST_SQ {
            ok = true;
            surface_y = h.point.y;
            normal = h.normal;
            material = Some(h.material);
        }
    }
    if !ok {
        let probe = pos + Vec3::Y * 10.0;
        snap = SnapResult::Raised;
        if let Some(h) = ground.below(probe) {
            if (h.point - probe).length_squared() < SNAP_DIST_SQ {
                ok = true;
                pos.x = h.point.x;
                pos.z = h.point.z;
                surface_y = h.point.y;
                normal = h.normal;
                material = Some(h.material);
            }
        }
    }
    if !ok {
        snap = SnapResult::NoSurface;
        normal = Vec3::Y;
        surface_y = pos.y;
    }
    pos.y = surface_y + r.height;

    // orientation rows (see the matrix block of the decompile): X = n x f, Y = n, Z = X x n
    let f = pt.tangent;
    let x = normal.cross(f);
    let y = normal;
    let z = x.cross(normal);
    // local offset applied in the item frame (L x M with L a translation)
    let world_pos = pos + x * r.local_x + y * r.local_y;
    // UNRESOLVED: sign/convention of the `+0x68` rotation about X (MakeVectorRotationMatrix32 is garbled); LayoutStructure's use of it
    // is reported separately. With pitch == 0 it is the identity.
    let (y, z) = if r.pitch != 0.0 {
        let (s, c) = (-r.pitch).sin_cos();
        (y * c + z * s, z * c - y * s)
    } else {
        (y, z)
    };
    Some(Placed {
        world: Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), world_pos.extend(1.0)),
        spline_pos,
        snap,
        t,
        t_out_of_range: out_of_range,
        ground_material: material,
    })
}
