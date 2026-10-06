//! Loads the game's `.xgm` models into renderer meshes and builds the simple procedural shapes the tracks need.
use crate::render3d::{MeshData, Vertex3};
use abgtool::xgm;
use glam::{Mat4, Vec3};
use std::path::Path;

pub struct Model {
    pub mesh: MeshData,
    pub nodes: Vec<xgm::Node>,
}

/// Loads a `.xgm`; meshes without stored normals get flat per-triangle normals.
pub fn load_xgm(path: &Path) -> Result<Model, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let model = xgm::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = MeshData::default();
    for m in &model.meshes {
        let base = out.vertices.len() as u32;
        if !m.normals.is_empty() {
            for (i, p) in m.positions.iter().enumerate() {
                let c = m.colors[i];
                out.vertices.push(Vertex3 {
                    pos: *p,
                    normal: m.normals[i],
                    color: [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0],
                });
            }
            out.indices.extend(m.indices.iter().map(|&i| base + i as u32));
        } else {
            for t in m.indices.chunks_exact(3) {
                let (a, b, c) = (Vec3::from(m.positions[t[0] as usize]), Vec3::from(m.positions[t[1] as usize]), Vec3::from(m.positions[t[2] as usize]));
                let n = (b - a).cross(c - a).normalize_or_zero().to_array();
                for p in [a, b, c] {
                    out.indices.push(out.vertices.len() as u32);
                    out.vertices.push(Vertex3 { pos: p.to_array(), normal: n, color: [1.0, 1.0, 1.0, 1.0] });
                }
            }
        }
    }
    Ok(Model { mesh: out, nodes: model.nodes })
}

impl Model {
    pub fn node(&self, name: &str) -> Option<&xgm::Node> {
        self.nodes.iter().find(|n| n.name == name)
    }
}

/// Mesh with every vertex colour multiplied by `rgb`.
pub fn tinted(mesh: &MeshData, rgb: [f32; 3]) -> MeshData {
    let mut m = mesh.clone();
    for v in &mut m.vertices {
        v.color = [v.color[0] * rgb[0], v.color[1] * rgb[1], v.color[2] * rgb[2], v.color[3]];
    }
    m
}

/// Axis-aligned box, `size` = full extents, centred on the origin.
pub fn cuboid(size: Vec3, color: [f32; 4]) -> MeshData {
    let h = size * 0.5;
    let mut m = MeshData::default();
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (Vec3::NEG_Y, Vec3::Z, Vec3::NEG_X),
        (Vec3::Z, Vec3::Y, Vec3::NEG_X),
        (Vec3::NEG_Z, Vec3::Y, Vec3::X),
    ];
    for (n, up, right) in faces {
        let c = n * h.dot(n.abs());
        let (u, r) = (up * h.dot(up.abs()), right * h.dot(right.abs()));
        let base = m.vertices.len() as u32;
        for (su, sr) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)] {
            m.vertices.push(Vertex3 { pos: (c + u * su + r * sr).to_array(), normal: n.to_array(), color });
        }
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

/// Cone along +Y from y = 0 (base, radius `r`) to y = `h`.
pub fn cone(r: f32, h: f32, sides: usize, color: [f32; 4]) -> MeshData {
    let mut m = MeshData::default();
    let slope = r / h;
    for i in 0..sides {
        let (a0, a1) = (i as f32 / sides as f32 * std::f32::consts::TAU, (i + 1) as f32 / sides as f32 * std::f32::consts::TAU);
        let am = (a0 + a1) / 2.0;
        let normal = Vec3::new(am.cos(), slope, am.sin()).normalize().to_array();
        let base = m.vertices.len() as u32;
        m.vertices.push(Vertex3 { pos: [r * a0.cos(), 0.0, r * a0.sin()], normal, color });
        m.vertices.push(Vertex3 { pos: [r * a1.cos(), 0.0, r * a1.sin()], normal, color });
        m.vertices.push(Vertex3 { pos: [0.0, h, 0.0], normal, color });
        m.indices.extend([base, base + 1, base + 2]);
    }
    m
}

/// Closed cylinder-ish trunk: a cone with a flat top is enough, so this is a tapered prism along +Y.
pub fn trunk(r_bottom: f32, r_top: f32, h: f32, sides: usize, color: [f32; 4]) -> MeshData {
    let mut m = MeshData::default();
    for i in 0..sides {
        let (a0, a1) = (i as f32 / sides as f32 * std::f32::consts::TAU, (i + 1) as f32 / sides as f32 * std::f32::consts::TAU);
        let am = (a0 + a1) / 2.0;
        let normal = Vec3::new(am.cos(), 0.0, am.sin()).to_array();
        let base = m.vertices.len() as u32;
        m.vertices.push(Vertex3 { pos: [r_bottom * a0.cos(), 0.0, r_bottom * a0.sin()], normal, color });
        m.vertices.push(Vertex3 { pos: [r_bottom * a1.cos(), 0.0, r_bottom * a1.sin()], normal, color });
        m.vertices.push(Vertex3 { pos: [r_top * a1.cos(), h, r_top * a1.sin()], normal, color });
        m.vertices.push(Vertex3 { pos: [r_top * a0.cos(), h, r_top * a0.sin()], normal, color });
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

/// Round pine: trunk + two stacked cones, ready to be placed with a transform.
pub fn pine(scale: f32) -> MeshData {
    let mut m = MeshData::default();
    m.append(&trunk(0.35 * scale, 0.25 * scale, 2.2 * scale, 7, [0.38, 0.25, 0.14, 1.0]), Mat4::IDENTITY);
    m.append(&cone(2.4 * scale, 4.0 * scale, 9, [0.13, 0.42, 0.18, 1.0]), Mat4::from_translation(Vec3::new(0.0, 1.6 * scale, 0.0)));
    m.append(&cone(1.8 * scale, 3.4 * scale, 9, [0.17, 0.50, 0.22, 1.0]), Mat4::from_translation(Vec3::new(0.0, 3.6 * scale, 0.0)));
    m
}
