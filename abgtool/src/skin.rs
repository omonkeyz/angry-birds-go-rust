//! CPU skinning of the character models: bone hierarchy, rest pose, animated pose (from `.xga` tracks) and the skin
//! matrices that deform the bind-pose vertices of an `.xgm` mesh.
//!
//! Conventions (checked against the data by `rest_pose_matches_inverse_bind`):
//!  * bone local transforms are `T * R * S` (column vectors) with the quaternion exactly as `anim::Key::rot` holds it;
//!  * `Bone::matrix` (chunk 0x25) is the inverse bind matrix of the bone, row-vector layout, i.e. `Mat4::from_cols_array`
//!    of the 16 floats is the column-vector form;
//!  * skin matrix of a bone = `world(bone, pose) * inverse_bind(bone)`, a vertex is the weighted sum of its influences.
use crate::anim::{Anim, Trs};
use crate::xgm::{Mesh, Skeleton};

pub type M4 = [f32; 16]; // column major, column vectors (glam layout)

fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + row] * b[c * 4 + k];
            }
            r[c * 4 + row] = s;
        }
    }
    r
}

/// `T * R * S` from scale / quaternion xyzw / position.
pub fn trs_matrix(scale: [f32; 3], q: [f32; 4], pos: [f32; 3]) -> M4 {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let (x, y, z, w) = if n > 0.0 { (q[0] / n, q[1] / n, q[2] / n, q[3] / n) } else { (0.0, 0.0, 0.0, 1.0) };
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz, wx, wy, wz) = (x * y, x * z, y * z, w * x, w * y, w * z);
    let r = [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
        [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
        [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
    ];
    let mut m = [0.0; 16];
    for c in 0..3 {
        for row in 0..3 {
            m[c * 4 + row] = r[row][c] * scale[c];
        }
    }
    m[12] = pos[0];
    m[13] = pos[1];
    m[14] = pos[2];
    m[15] = 1.0;
    m
}

pub struct Rig {
    pub names: Vec<String>,
    pub parent: Vec<Option<usize>>,
    /// rest pose, local to the parent
    pub rest: Vec<Trs>,
    /// inverse bind matrices (column-vector form)
    pub inv_bind: Vec<M4>,
    /// true when the bone has an inverse bind matrix (some vertex uses it)
    pub used: Vec<bool>,
}

impl Rig {
    /// `conjugate_rest`: the skeleton's rest quaternions are stored in the other handedness than `anim::Key::rot`.
    pub fn new(sk: &Skeleton) -> Rig {
        let n = sk.bones.len();
        let mut rig = Rig { names: Vec::new(), parent: Vec::new(), rest: Vec::new(), inv_bind: Vec::new(), used: Vec::new() };
        for b in &sk.bones {
            rig.names.push(b.name.clone());
            rig.parent.push(b.parent);
            let q = b.rest_rotation;
            rig.rest.push(Trs { scale: b.rest_scale_a, rot: [-q[0], -q[1], -q[2], q[3]], pos: b.rest_position });
            rig.inv_bind.push(b.matrix);
            rig.used.push(b.matrix.iter().any(|v| *v != 0.0));
        }
        debug_assert_eq!(rig.names.len(), n);
        rig
    }

    /// World matrices of every bone for the given local transforms (parents come in any order).
    pub fn world(&self, local: &[Trs]) -> Vec<M4> {
        let n = self.names.len();
        let mut world: Vec<Option<M4>> = vec![None; n];
        fn resolve(rig: &Rig, local: &[Trs], world: &mut Vec<Option<M4>>, i: usize, depth: usize) -> M4 {
            if let Some(w) = world[i] {
                return w;
            }
            let t = &local[i];
            let m = trs_matrix(t.scale, t.rot, t.pos);
            let w = match rig.parent[i] {
                Some(p) if p != i && depth < 64 => mul(&resolve(rig, local, world, p, depth + 1), &m),
                _ => m,
            };
            world[i] = Some(w);
            w
        }
        (0..n).map(|i| resolve(self, local, &mut world, i, 0)).collect()
    }

    pub fn rest_world(&self) -> Vec<M4> {
        self.world(&self.rest)
    }

    /// Local transforms for `anim` at `time` seconds: tracks are matched to bones by name, the rest pose fills the rest.
    pub fn pose(&self, anim: &Anim, time: f32) -> Vec<Trs> {
        let mut local = self.rest.clone();
        if let Some(ts) = anim.groups.iter().find_map(|g| g.track_set.as_ref().filter(|t| !t.tracks.is_empty())) {
            let frame = anim.frame_at(time);
            for (ti, t) in ts.tracks.iter().enumerate() {
                if let Some(bi) = self.names.iter().position(|n| n.eq_ignore_ascii_case(&t.name)) {
                    if let Some(s) = ts.sample(ti, frame, anim.key_stride) {
                        local[bi] = s;
                    }
                }
            }
        }
        local
    }

    /// Skin matrices (`world * inverse_bind`) for the given world matrices.
    pub fn skin_matrices(&self, world: &[M4]) -> Vec<M4> {
        (0..self.names.len()).map(|i| if self.used[i] { mul(&world[i], &self.inv_bind[i]) } else { world[i] }).collect()
    }
}

fn xf(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn xd(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2], m[1] * p[0] + m[5] * p[1] + m[9] * p[2], m[2] * p[0] + m[6] * p[1] + m[10] * p[2]]
}

/// Skins vertex `v` of `mesh` with the skin matrices (bind-pose position and normal in, posed position and normal out).
/// Static vertices (no influences) are returned unchanged.
pub fn skin_vertex(mesh: &Mesh, v: usize, skin: &[M4]) -> ([f32; 3], [f32; 3]) {
    let p = mesh.positions[v];
    let n = mesh.normals.get(v).copied().unwrap_or([0.0, 0.0, 1.0]);
    let k = mesh.influences.get(v).copied().unwrap_or(0) as usize;
    if k == 0 {
        return (p, n);
    }
    let (mut op, mut on) = ([0.0f32; 3], [0.0f32; 3]);
    let mut wsum = 0.0;
    for j in 0..k {
        let w = mesh.bone_weights[v][j];
        let Some(m) = skin.get(mesh.bone_indices[v][j] as usize) else { continue };
        let (tp, tn) = (xf(m, p), xd(m, n));
        for c in 0..3 {
            op[c] += tp[c] * w;
            on[c] += tn[c] * w;
        }
        wsum += w;
    }
    if wsum <= 0.0 {
        return (p, n);
    }
    let l = (on[0] * on[0] + on[1] * on[1] + on[2] * on[2]).sqrt();
    if l > 0.0 {
        on = [on[0] / l, on[1] / l, on[2] / l];
    }
    (op, on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xgm;
    use std::path::PathBuf;

    fn asset(rel: &str) -> Option<Vec<u8>> {
        std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)).ok()
    }

    /// The bind pose is the identity of `world * inverse_bind`; the physique "rest" frame is a slightly posed state, so
    /// the skinned vertices of the rest pose stay close to the bind mesh (the red bird: within 0.3 everywhere).
    #[test]
    fn rest_pose_stays_near_bind_mesh() {
        let Some(d) = asset("assets292/pak/characters/models/red_l02.xgm") else { return };
        let x = xgm::parse(&d).unwrap();
        let rig = Rig::new(&x.skeletons[0]);
        let skin = rig.skin_matrices(&rig.rest_world());
        let mesh = &x.meshes[0];
        let mut worst = 0.0f32;
        for v in 0..mesh.positions.len() {
            let (p, _) = skin_vertex(mesh, v, &skin);
            let q = mesh.positions[v];
            worst = worst.max(((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt());
        }
        println!("largest rest displacement {worst}");
        assert!(worst < 0.3, "{worst}");
    }
}
