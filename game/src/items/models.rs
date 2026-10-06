//! Model bookkeeping for the items: which `.xgm` each item uses, its bounds (the pickup trigger radius is derived from them) and the
//! textures it needs. Models live in `assets292/pak/envobjects` (`ENVOBJ:`) and `assets292/pak/smackables`.
use abgtool::xgm;
use glam::Vec3;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ModelInfo {
    /// path under `assets292/pak`, e.g. `envobjects/coin.xgm`
    pub rel_path: String,
    pub bbox_min: Vec3,
    pub bbox_max: Vec3,
    /// `CXGSModel+0xCC` (`InitModel @00499250`): half diagonal of the model's AABB
    pub radius: f32,
    /// texture file names referenced by the model's materials (as stored, usually `*.xgt`; converted PNGs live in `assets292/textures`)
    pub textures: Vec<String>,
    pub loaded: bool,
    /// `pivot` helper node (`LoadSmackable` keeps it per type): the model is placed so its origin sits at `-pivot`
    pub pivot: Vec3,
    /// other helper nodes (name, model-space position): the fragment spawn points of smackables
    pub nodes: Vec<(String, Vec3)>,
}

/// `CXGSModel::InitModel @00499250`: `radius = sqrt(dx^2 + dy^2 + dz^2)` with `d = (bbmax - bbmin) * 0.5`.
pub fn half_diagonal(min: Vec3, max: Vec3) -> f32 {
    ((max - min) * 0.5).length()
}

fn case_insensitive_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(e.path());
        }
    }
    None
}

/// Loads `pak/<sub>/<file>` (case-insensitive file name) and collects bounds + textures.
pub fn load_model_info(assets: &Path, sub: &str, file: &str) -> ModelInfo {
    let mut info = ModelInfo { rel_path: format!("{sub}/{}", file.to_ascii_lowercase()), ..Default::default() };
    let Some(p) = case_insensitive_file(&assets.join("pak").join(sub), file) else { return info };
    let Ok(data) = std::fs::read(&p) else { return info };
    let Ok(m) = xgm::parse(&data) else { return info };
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for mesh in &m.meshes {
        if mesh.positions.is_empty() {
            continue;
        }
        lo = lo.min(Vec3::from(mesh.bbox_min));
        hi = hi.max(Vec3::from(mesh.bbox_max));
    }
    if lo.x <= hi.x {
        info.bbox_min = lo;
        info.bbox_max = hi;
        info.radius = half_diagonal(lo, hi);
    }
    for mat in &m.materials {
        for (_, t) in &mat.textures {
            if !info.textures.contains(t) {
                info.textures.push(t.clone());
            }
        }
    }
    for n in &m.nodes {
        if n.name.eq_ignore_ascii_case("pivot") {
            info.pivot = Vec3::from(n.position);
        } else {
            info.nodes.push((n.name.clone(), Vec3::from(n.position)));
        }
    }
    info.loaded = true;
    info
}

/// Cache of the models an `ItemWorld` needs.
#[derive(Default)]
pub struct ModelLibrary {
    pub by_name: HashMap<String, ModelInfo>,
}

impl ModelLibrary {
    pub fn get(&self, name: &str) -> Option<&ModelInfo> {
        self.by_name.get(&name.to_ascii_lowercase())
    }
}
