//! Downward ray queries against the track's collision triangles (`CXGSEnv::GetGeometryBelow @00537a90`, `CXGSKDTree::RayIntersect`).
//! Own grid over the kd-tree triangles; the surface-id filter and the "crossing from above only" rule match `trackworld.rs`
//! (`MaterialCollisionCallback` ignores 7, 9, 29, 30, 37, 38 - FORMATS.md).
use abgtool::stm::Stm;
use glam::Vec3;
use std::collections::HashMap;

const IGNORED_SURFACES: [u16; 6] = [7, 9, 29, 30, 37, 38];
const CELL: f32 = 8.0;

#[derive(Debug, Clone, Copy)]
pub struct GroundHit {
    pub point: Vec3,
    pub normal: Vec3,
    pub material: u16,
}

pub struct ItemGround {
    tris: Vec<[Vec3; 3]>,
    normals: Vec<Vec3>,
    material: Vec<u16>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    min_y: f32,
}

impl ItemGround {
    pub fn build(stm: &Stm) -> ItemGround {
        let mut g = ItemGround { tris: Vec::new(), normals: Vec::new(), material: Vec::new(), cells: HashMap::new(), min_y: f32::MAX };
        for tree in &stm.kd_trees {
            for t in &tree.triangles {
                if IGNORED_SURFACES.contains(&t.material) {
                    continue;
                }
                let v = [Vec3::from(t.v[0]), Vec3::from(t.v[1]), Vec3::from(t.v[2])];
                let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
                let index = g.tris.len() as u32;
                let (min, max) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
                g.min_y = g.min_y.min(min.y);
                for cx in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
                    for cz in (min.z / CELL).floor() as i32..=(max.z / CELL).floor() as i32 {
                        g.cells.entry((cx, cz)).or_default().push(index);
                    }
                }
                g.tris.push(v);
                g.normals.push(if n.y < 0.0 { -n } else { n });
                g.material.push(t.material);
            }
        }
        g
    }

    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// Nearest collision surface straight below `p` (down to the lowest triangle of the track, as `GetGeometryBelow` limits the ray).
    pub fn below(&self, p: Vec3) -> Option<GroundHit> {
        let len = p.y - self.min_y + 0.01;
        if len <= 0.0 {
            return None;
        }
        self.cast(p, Vec3::new(0.0, -len, 0.0))
    }

    /// Nearest crossing of the segment `origin .. origin + vec` with a collision triangle (Moller-Trumbore), faces seen from above only.
    pub fn cast(&self, origin: Vec3, vec: Vec3) -> Option<GroundHit> {
        let end = origin + vec;
        let (min, max) = (origin.min(end), origin.max(end));
        let mut best: Option<(f32, usize)> = None;
        let mut seen: Vec<u32> = Vec::new();
        for cx in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
            for cz in (min.z / CELL).floor() as i32..=(max.z / CELL).floor() as i32 {
                let Some(list) = self.cells.get(&(cx, cz)) else { continue };
                for &i in list {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let [a, b, c] = self.tris[i as usize];
                    let (e1, e2) = (b - a, c - a);
                    let pv = vec.cross(e2);
                    let det = e1.dot(pv);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = origin - a;
                    let u = s.dot(pv) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = s.cross(e1);
                    let v = vec.dot(q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let t = e2.dot(q) * inv;
                    if (0.0..=1.0).contains(&t) && self.normals[i as usize].dot(vec) < 0.0 && best.map_or(true, |(bt, _)| t < bt) {
                        best = Some((t, i as usize));
                    }
                }
            }
        }
        best.map(|(t, i)| GroundHit { point: origin + vec * t, normal: self.normals[i], material: self.material[i] })
    }
}
