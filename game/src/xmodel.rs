//! Textured models of the 2.9.x data (`.xgm` read by abgtool): one GPU mesh per material, textures found by name.
use crate::render3d::{MeshId, Scene3d, TVertex3, TexId, TexMeshData};
use abgtool::xgm;
use glam::Mat4;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every converted texture under `assets292/textures`, found by lower-case file stem (the engine references them as `name.tga`).
pub struct TextureIndex {
    by_stem: HashMap<String, PathBuf>,
    cache: HashMap<String, Option<TexId>>,
}

impl TextureIndex {
    pub fn new(root: &Path) -> TextureIndex {
        let mut by_stem = HashMap::new();
        let mut pending = vec![root.join("textures")];
        while let Some(dir) = pending.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for e in read.flatten() {
                let p = e.path();
                if p.is_dir() {
                    pending.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some("png") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        // first hit wins: kart / character textures have unique names, shared ones are identical
                        by_stem.entry(stem.to_ascii_lowercase()).or_insert(p);
                    }
                }
            }
        }
        TextureIndex { by_stem, cache: HashMap::new() }
    }

    /// Uploads (once) the texture called `name` (`abk_red kart colour_levelone.tga`, stem or file name).
    pub fn texture(&mut self, scene: &mut Scene3d, name: &str) -> Option<TexId> {
        let stem = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem).to_ascii_lowercase();
        if let Some(c) = self.cache.get(&stem) {
            return *c;
        }
        let id = self.by_stem.get(&stem).and_then(|p| image::open(p).ok()).map(|img| {
            let img = img.to_rgba8();
            let (w, h) = img.dimensions();
            scene.add_texture(w, h, img.into_raw())
        });
        self.cache.insert(stem, id);
        id
    }
}

/// A model on the GPU: its draw parts and the node (helper) transforms.
pub struct XModel {
    pub parts: Vec<(MeshId, bool)>,
    pub nodes: Vec<xgm::Node>,
    pub bounds: ([f32; 3], [f32; 3]),
}

impl XModel {
    /// `skip_outline`: drop the `*_outline` meshes (the cartoon outline shells).
    pub fn load(scene: &mut Scene3d, textures: &mut TextureIndex, path: &Path, skip_outline: bool) -> Result<XModel, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let model = xgm::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut parts = Vec::new();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for mesh in &model.meshes {
            if skip_outline && mesh.name.to_ascii_lowercase().contains("outline") {
                continue;
            }
            let mut positions = mesh.transformed_positions();
            // skinned character meshes are stored in the exporter's z-up space (the bind skeleton: face +y, up +z); the engine's
            // sampler swaps y and z (xga ext header [6] = 0), which is a mirror, so the winding is reversed too
            let zup = mesh.skeleton.is_some();
            if zup {
                for p in positions.iter_mut() {
                    *p = [p[0], p[2], p[1]];
                }
            }
            for p in positions.iter() {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            for sub in &mesh.submeshes {
                let a = (sub.first_index as usize).min(mesh.indices.len());
                let b = (a + 3 * sub.tri_count as usize).min(mesh.indices.len());
                let mut data = TexMeshData::default();
                let mut remap: HashMap<u16, u32> = HashMap::new();
                for &i in &mesh.indices[a..b] {
                    let idx = *remap.entry(i).or_insert_with(|| {
                        let k = i as usize;
                        let mut n = mesh.normals.get(k).copied().unwrap_or([0.0, 1.0, 0.0]);
                        if zup {
                            n = [n[0], n[2], n[1]];
                        }
                        let c = mesh.colors.get(k).copied().unwrap_or([255; 4]);
                        data.vertices.push(TVertex3 {
                            pos: positions[k],
                            normal: n,
                            color: [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0],
                            uv: mesh.uvs.get(k).copied().unwrap_or([0.0, 0.0]),
                        });
                        (data.vertices.len() - 1) as u32
                    });
                    data.indices.push(idx);
                }
                if data.indices.is_empty() {
                    continue;
                }
                if zup {
                    for t in data.indices.chunks_exact_mut(3) {
                        t.swap(1, 2);
                    }
                }
                let tex_name = sub.material.and_then(|m| model.materials.get(m)).and_then(|m| m.textures.iter().find(|t| t.0 == 0).or(m.textures.first())).map(|t| t.1.clone());
                let tex = tex_name.and_then(|n| textures.texture(scene, &n));
                match tex {
                    Some(t) => parts.push((scene.add_tex_mesh(&data, t), true)),
                    None => {
                        let plain = crate::render3d::MeshData {
                            vertices: data.vertices.iter().map(|v| crate::render3d::Vertex3 { pos: v.pos, normal: v.normal, color: [0.7, 0.7, 0.7, 1.0] }).collect(),
                            indices: data.indices.clone(),
                        };
                        parts.push((scene.add_mesh(&plain), false));
                    }
                }
            }
        }
        Ok(XModel { parts, nodes: model.nodes, bounds: (lo, hi) })
    }

    pub fn node(&self, name: &str) -> Option<&xgm::Node> {
        self.nodes.iter().find(|n| n.name.eq_ignore_ascii_case(name))
    }

    pub fn draws(&self, out: &mut Vec<crate::render3d::Draw3d>, model: Mat4, tint: [f32; 4]) {
        for (mesh, _) in &self.parts {
            out.push(crate::render3d::Draw3d { mesh: *mesh, model, tint });
        }
    }
}
