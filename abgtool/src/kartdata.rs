//! Kart data of the 2.9.x install (`assets292`): which driver sits in which kart, where the kart folders are and a
//! structural check of every kart variant (`abgtool kart-check <assets292>`).
use crate::xgm;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Character models / animation sets, in the order of `characters/charxml/char_001..016.xml` (`<Name>` lower-cased:
/// `characters/models/<stem>_l0N.xgm`, `characters/animation/<stem>.xml`).
pub const CHARACTER_STEMS: [&str; 16] = [
    "red", "black", "pink", "big_red", "blue", "king_pig", "white", "moustache_pig", "orange", "helmet_pig", "green", "yellow", "senna", "sennahelmet", "minionpig", "dealer_pig",
];

/// Owner (1-based index into `CHARACTER_STEMS`) of each kart family: the `<Owner>` element of `gameplay/misc/karttype_ep*.xml`
/// (`KART_BOMBSNOW_n` -> 2 = Black, `KART_TERRENCE_n` -> 4 = Big_Red, ...). `kart_base` and the `kart_target_*` power-up
/// karts are in no karttype file: they carry whichever character the player picked, the default is Red.
const OWNERS: [(&str, usize); 31] = [
    ("red", 1),
    ("pigrally", 1),
    ("halloween", 1),
    ("thanksgiving", 1),
    ("redjet", 1),
    ("black", 2),
    ("black_blackfriday", 2),
    ("bombsnow", 2),
    ("pink", 3),
    ("terrence", 4),
    ("monkey", 4),
    ("goat", 4),
    ("terencesnow", 4),
    ("terencesnow_ltd", 4),
    ("christmaspromo", 4),
    ("xmasmk2promo", 4),
    ("bluebird", 5),
    ("blueburner", 5),
    ("kingpig", 6),
    ("kingsnow", 6),
    ("white", 7),
    ("moustache", 8),
    ("orange", 9),
    ("helmetpig", 10),
    ("green", 11),
    ("halbuggy", 11),
    ("yellowrocket", 12),
    ("chucksnow", 12),
    ("chucksnow_ltd", 12),
    ("base", 1),
    ("target", 1),
];

/// `kart_red_upgrade1` -> `red`, `kart_black_blackfriday` -> `black_blackfriday`, `kart_target_air` -> `target`.
pub fn family(folder: &str) -> String {
    let rest = folder.trim_start_matches("kart_").to_ascii_lowercase();
    let rest = match rest.find("_upgrade") {
        Some(i) => rest[..i].to_string(),
        None => rest,
    };
    if rest.starts_with("target_") {
        return "target".to_string();
    }
    rest
}

/// Model / animation stem of the driver that sits in the kart folder by default.
pub fn pilot_stem(folder: &str) -> &'static str {
    let fam = family(folder);
    let owner = OWNERS.iter().find(|(k, _)| *k == fam).map(|(_, o)| *o).unwrap_or(1);
    CHARACTER_STEMS[owner - 1]
}

/// Every kart folder under `root/pak/cars/theme00{2..6}/cargeom` and `telepod/cargeom`, sorted by name.
pub fn kart_folders(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut themes: Vec<PathBuf> = (2..=6).map(|t| root.join(format!("pak/cars/theme00{t}/cargeom"))).collect();
    themes.push(root.join("pak/cars/telepod/cargeom"));
    for dir in themes {
        if let Ok(read) = std::fs::read_dir(&dir) {
            for e in read.flatten() {
                if e.path().is_dir() {
                    out.push((e.file_name().to_string_lossy().into_owned(), e.path()));
                }
            }
        }
    }
    out.sort();
    out
}

fn texture_stems(root: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut pending = vec![root.join("textures")];
    while let Some(dir) = pending.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for e in read.flatten() {
            let p = e.path();
            if p.is_dir() {
                pending.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("png") {
                if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                    set.insert(s.to_ascii_lowercase());
                }
            }
        }
    }
    set
}

fn tex_stem(name: &str) -> String {
    let n = name.rsplit(['/', '\\']).next().unwrap_or(name);
    n.rsplit_once('.').map(|(s, _)| s).unwrap_or(n).to_ascii_lowercase()
}

/// Loads and checks every kart variant at every LOD; returns (kart variants checked, problem lines).
pub fn check_all(root: &Path) -> (usize, Vec<String>) {
    let textures = texture_stems(root);
    let mut problems = Vec::new();
    let folders = kart_folders(root);
    let mut checked = 0;
    let mut tex_missing: HashSet<String> = HashSet::new();
    let mut parse = |path: &Path, who: &str, problems: &mut Vec<String>| -> Option<xgm::Xgm> {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                problems.push(format!("{who}: {}: {e}", path.display()));
                return None;
            }
        };
        match xgm::parse(&data) {
            Ok(x) => Some(x),
            Err(e) => {
                problems.push(format!("{who}: {}: {e}", path.display()));
                None
            }
        }
    };
    for (name, dir) in &folders {
        for lod in 2..=4 {
            checked += 1;
            let sfx = format!("_l0{lod}.xgm");
            let who = format!("{name} l0{lod}");
            let Some(chassis) = parse(&dir.join(format!("chassis{sfx}")), &who, &mut problems) else { continue };
            for want in ["attach_pilot_1", "front_left_wheel", "rear_left_wheel"].iter().filter(|_| lod == 2) {
                if !chassis.nodes.iter().any(|n| n.name.eq_ignore_ascii_case(want)) {
                    problems.push(format!("{who}: chassis has no node {want}"));
                }
            }
            let mut models = vec![(format!("chassis{sfx}"), chassis)];
            for w in ["wheelfront", "wheelrear"] {
                if let Some(m) = parse(&dir.join(format!("{w}{sfx}")), &who, &mut problems) {
                    models.push((format!("{w}{sfx}"), m));
                }
            }
            let mut stems = Vec::new();
            if let Ok(read) = std::fs::read_dir(dir) {
                let mut files: Vec<String> = read.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).collect();
                files.sort();
                for f in files {
                    if f.ends_with(&sfx) && !f.contains("outline") && !f.starts_with("chassis") && !f.starts_with("wheel") {
                        stems.push(f.trim_end_matches(&sfx).to_string());
                    }
                }
            }
            for stem in &stems {
                let Some(part) = parse(&dir.join(format!("{stem}{sfx}")), &who, &mut problems) else { continue };
                if lod == 2 && !part.nodes.iter().any(|n| n.name.eq_ignore_ascii_case("attach_1")) {
                    problems.push(format!("{who}: part {stem} has no attach_1"));
                }
                let want = format!("attach_{stem}_1");
                if lod == 2 && !models[0].1.nodes.iter().any(|n| n.name.eq_ignore_ascii_case(&want)) {
                    problems.push(format!("{who}: chassis has no node {want} for part {stem}"));
                }
                models.push((format!("{stem}{sfx}"), part));
            }
            // the driver
            let stem = pilot_stem(name);
            let ppath = root.join(format!("pak/characters/models/{stem}{sfx}"));
            if let Some(p) = parse(&ppath, &who, &mut problems) {
                if lod == 2 && !p.nodes.iter().any(|n| n.name.eq_ignore_ascii_case("attach_1")) {
                    problems.push(format!("{who}: pilot {stem} has no attach_1"));
                }
                if lod == 2 && p.skeletons.is_empty() {
                    problems.push(format!("{who}: pilot {stem} has no skeleton"));
                }
                models.push((format!("pilot {stem}"), p));
            }
            for (file, m) in &models {
                if m.meshes.is_empty() {
                    problems.push(format!("{who}: {file} has no mesh"));
                }
                for mesh in &m.meshes {
                    if mesh.name.to_ascii_lowercase().contains("outline") {
                        continue;
                    }
                    if let Err(e) = mesh.validate_deep(m.materials.len()) {
                        problems.push(format!("{who}: {file} mesh {}: {e}", mesh.name));
                    }
                    for sub in &mesh.submeshes {
                        let tex = sub.material.and_then(|i| m.materials.get(i)).and_then(|mat| mat.textures.iter().find(|t| t.0 == 0).or(mat.textures.first()));
                        match tex {
                            None => problems.push(format!("{who}: {file} mesh {} has a sub mesh without texture", mesh.name)),
                            Some(t) => {
                                let s = tex_stem(&t.1);
                                if !textures.contains(&s) && tex_missing.insert(format!("{who}|{s}")) {
                                    problems.push(format!("{who}: {file}: texture {} not found", t.1));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // every driver's animation set
    for (i, stem) in CHARACTER_STEMS.iter().enumerate() {
        let xml = root.join(format!("pak/characters/animation/{stem}.xml"));
        match std::fs::read(&xml).map_err(|e| e.to_string()).and_then(|d| crate::xox::decode(&d)) {
            Err(e) => problems.push(format!("character {} ({stem}): animation set: {e}", i + 1)),
            Ok(text) => {
                for part in text.split("AddFile Name=\"").skip(1) {
                    let file = part.split('"').next().unwrap_or("").to_ascii_lowercase();
                    let path = root.join("pak/characters/animation").join(&file);
                    match std::fs::read(&path) {
                        Err(_) => problems.push(format!("character {stem}: animation file {file} missing")),
                        Ok(d) => {
                            if let Err(e) = crate::anim::parse(&d) {
                                problems.push(format!("character {stem}: {file}: {e}"));
                            }
                        }
                    }
                }
            }
        }
    }
    (checked, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        assert_eq!(family("kart_red_upgrade1"), "red");
        assert_eq!(family("kart_black_blackfriday"), "black_blackfriday");
        assert_eq!(family("kart_target_air"), "target");
        assert_eq!(pilot_stem("kart_terrence_upgrade3"), "big_red");
        assert_eq!(pilot_stem("kart_bombsnow_upgrade5"), "black");
        assert_eq!(pilot_stem("kart_kingsnow_upgrade1"), "king_pig");
        assert_eq!(pilot_stem("kart_chucksnow_ltd"), "yellow");
    }
}
