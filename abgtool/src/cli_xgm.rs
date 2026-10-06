//! Command line for `.xgm` models. The lead calls `cli_xgm::dispatch(&args)` before the old match in main.rs:
//! it returns `None` for commands that are not handled here.
//!
//!   abgtool xgm-info <file.xgm>            version, chunks, meshes, vertex layout, sub meshes, skin, materials, skeleton, nodes
//!   abgtool xgm-obj <file.xgm> <out.obj>   OBJ with uvs, normals, one group + usemtl per sub mesh (+ out.mtl next to it)
//!   abgtool xgm-scan <dir>                 parse every .xgm under dir: parsed / failed / invalid counts, strides, chunk ids
//!   abgtool xgm-skin <file.xgm>            per mesh skin summary (blocks, influences, bone usage, weight sums)
//!
//! (`xgm-preview <file.xgm> <out.png>` stays in main.rs and works on both versions.)

use abgtool::xgm::{self, Xgm};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn dispatch(args: &[String]) -> Option<Result<(), String>> {
    let cmd = args.first()?.as_str();
    match (cmd, args.len()) {
        ("xgm-info", 2) => Some(info(&args[1])),
        ("xgm-obj", 3) => Some(obj(&args[1], &args[2])),
        ("xgm-scan", 2) => Some(scan(&args[1])),
        ("xgm-skin", 2) => Some(skin(&args[1])),
        ("kart-check", 2) => Some(kart_check(&args[1])),
        _ => None,
    }
}

fn load(path: &str) -> Result<Xgm, String> {
    let data = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    xgm::parse(&data).map_err(|e| format!("{path}: {e}"))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.extension().map_or(false, |e| e.eq_ignore_ascii_case("xgm")) {
            out.push(p);
        }
    }
    Ok(())
}

fn bounds(x: &Xgm) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for m in &x.meshes {
        for p in &m.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    (lo, hi)
}

fn info(path: &str) -> Result<(), String> {
    let x = load(path)?;
    println!("version 0x{:08X}, {} chunks, {} meshes, {} materials, {} skeletons, {} nodes", x.version, x.chunks.len(), x.meshes.len(), x.materials.len(), x.skeletons.len(), x.nodes.len());
    let mut ids: BTreeMap<u32, usize> = BTreeMap::new();
    for c in &x.chunks {
        *ids.entry(c.id & 0xFFFF).or_default() += 1;
    }
    println!("chunk ids: {}", ids.iter().map(|(k, v)| format!("0x{k:02X}x{v}")).collect::<Vec<_>>().join(" "));
    let (lo, hi) = bounds(&x);
    if lo[0] <= hi[0] {
        println!("model bbox {lo:?} .. {hi:?} size {:.3} x {:.3} x {:.3}", hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
    }
    for (i, m) in x.meshes.iter().enumerate() {
        println!(
            "mesh {i} \"{}\": platform {}, {} vertices (stride {}), {} triangles, bbox {:?} .. {:?}, valid: {:?}",
            m.name, m.platform, m.positions.len(), m.stride, m.indices.len() / 3, m.bbox_min, m.bbox_max, m.validate_deep(x.materials.len())
        );
        if m.platform == 6 {
            println!(
                "  flags 0x{:X}, radius {:.3}, uv scale {:?} bias {:?}, normals {}, colors {}, uvs {}, uvs2 {}, tangents {}",
                m.mesh_flags, m.radius, m.uv_scale, m.uv_bias, m.normals.len(), m.colors.len(), m.uvs.len(), m.uvs2.len(), m.tangents.len()
            );
            println!("  mesh matrix {:?} anim_matrix {:?}", m.matrix, m.anim_matrix);
            println!("  elements: {}", m.elements.iter().map(|e| format!("(type 0x{:X} usage {}.{} @{})", e.kind, e.usage, e.usage_index, e.offset)).collect::<Vec<_>>().join(" "));
            for (si, s) in m.submeshes.iter().enumerate() {
                let mat = s.material.and_then(|i| x.materials.get(i).map(|m| format!("{i}:{}", m.name))).unwrap_or_else(|| "-".into());
                println!("  part {si}: {} tris @ index {}, vertices {}+{}, skinned {}, material {mat}", s.tri_count, s.first_index, s.first_vertex, s.vertex_count, s.skinned());
            }
            if let Some(k) = &m.skin {
                println!("  skin: {} bones, {} skinned vertices, {} blocks, {} weight bytes, flags {:?}, skeleton {:?}", k.num_bones, k.skinned_vertices, k.blocks.len(), k.weight_bytes, k.flags, m.skeleton);
            }
        }
    }
    for (i, m) in x.materials.iter().enumerate() {
        println!(
            "material {i} \"{}\": textures {:?}, colour0 {:?}, colour1 {:?}, colour2 {:?}, shininess byte {}, tag 0x{:08X}",
            m.name, m.textures, m.colors[0], m.colors[1], m.colors[2], m.shininess_raw, m.tag
        );
    }
    for (i, s) in x.skeletons.iter().enumerate() {
        println!("skeleton {i}: {} bones, physique type {} frames {}", s.bones.len(), s.physique_type, s.physique_frames);
        for (b, bone) in s.bones.iter().enumerate() {
            println!(
                "  bone {b:2} {:<18} parent {:>4} rest pos {:?} rot {:?}",
                bone.name, bone.parent.map_or("-".to_string(), |p| p.to_string()), bone.rest_position, bone.rest_rotation
            );
        }
    }
    for n in &x.nodes {
        println!("node {:<22} pos {:?} rot {:?} (engine {:?}) scale {:?} frames {}", n.name, n.position, n.rotation, n.rotation_engine, n.scale, n.frames);
    }
    for (id, name) in &x.collisions {
        println!("collision chunk 0x{id:02X} \"{name}\"");
    }
    Ok(())
}

fn obj(path: &str, out: &str) -> Result<(), String> {
    let x = load(path)?;
    let mut text = xgm::to_obj(&x);
    let mtl_path = Path::new(out).with_extension("mtl");
    if !x.materials.is_empty() {
        let name = mtl_path.file_name().and_then(|n| n.to_str()).unwrap_or("model.mtl").to_string();
        text = text.replacen("mtllib model.mtl", &format!("mtllib {name}"), 1);
        fs::write(&mtl_path, xgm::to_mtl(&x)).map_err(|e| e.to_string())?;
    }
    fs::write(out, text).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}

fn skin(path: &str) -> Result<(), String> {
    let x = load(path)?;
    for (i, m) in x.meshes.iter().enumerate() {
        let Some(k) = &m.skin else {
            println!("mesh {i}: not skinned");
            continue;
        };
        let mut infl: BTreeMap<u8, usize> = BTreeMap::new();
        let mut bones: BTreeMap<u8, usize> = BTreeMap::new();
        let (mut min_sum, mut max_sum) = (f32::MAX, f32::MIN);
        for v in 0..k.skinned_vertices as usize {
            let n = m.influences[v] as usize;
            *infl.entry(n as u8).or_default() += 1;
            let mut sum = 0.0;
            for j in 0..n {
                sum += m.bone_weights[v][j];
                *bones.entry(m.bone_indices[v][j]).or_default() += 1;
            }
            min_sum = min_sum.min(sum);
            max_sum = max_sum.max(sum);
        }
        println!("mesh {i}: {} skinned of {} vertices, influences {infl:?}, weight sum {min_sum:.3}..{max_sum:.3}", k.skinned_vertices, m.positions.len());
        let names: Vec<String> = bones
            .iter()
            .map(|(b, n)| {
                let nm = m.skeleton.and_then(|s| x.skeletons[s].bones.get(*b as usize)).map(|b| b.name.clone()).unwrap_or_default();
                format!("{b}:{nm}({n})")
            })
            .collect();
        println!("  bones used: {}", names.join(" "));
    }
    Ok(())
}

fn scan(dir: &str) -> Result<(), String> {
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files)?;
    let mut strides: BTreeMap<usize, usize> = BTreeMap::new();
    let mut chunk_ids: BTreeMap<u32, usize> = BTreeMap::new();
    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();
    let mut platforms: BTreeMap<u16, usize> = BTreeMap::new();
    let mut layouts: BTreeMap<String, usize> = BTreeMap::new();
    let (mut ok, mut failed, mut no_mesh, mut invalid) = (0usize, 0usize, 0usize, 0usize);
    let (mut verts, mut tris, mut skinned_meshes, mut skel, mut mats, mut nodes, mut nan) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut max_infl = 0usize;
    for file in &files {
        let data = match fs::read(file) {
            Ok(d) => d,
            Err(e) => {
                failed += 1;
                eprintln!("UNREADABLE {}: {e}", file.display());
                continue;
            }
        };
        match xgm::parse(&data) {
            Ok(model) => {
                ok += 1;
                *versions.entry(model.version).or_default() += 1;
                for c in &model.chunks {
                    *chunk_ids.entry(c.id & 0xFFFF).or_default() += 1;
                }
                if model.meshes.is_empty() {
                    no_mesh += 1;
                }
                skel += model.skeletons.len();
                mats += model.materials.len();
                nodes += model.nodes.len();
                for m in &model.meshes {
                    *strides.entry(m.stride).or_default() += 1;
                    *platforms.entry(m.platform).or_default() += 1;
                    verts += m.positions.len();
                    tris += m.indices.len() / 3;
                    if m.skin.is_some() {
                        skinned_meshes += 1;
                    }
                    if m.platform == 6 {
                        let key = m.elements.iter().map(|e| format!("{}.{}:0x{:X}", e.usage, e.usage_index, e.kind)).collect::<Vec<_>>().join(" ");
                        *layouts.entry(format!("stride {} flags {} [{key}]", m.stride, m.mesh_flags)).or_default() += 1;
                    }
                    max_infl = max_infl.max(m.influences.iter().copied().max().unwrap_or(0) as usize);
                    if m.positions.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
                        nan += 1;
                    }
                    if let Err(e) = m.validate_deep(model.materials.len()) {
                        invalid += 1;
                        if invalid <= 8 {
                            eprintln!("INVALID {}: {e}", file.display());
                        }
                    }
                }
            }
            Err(e) => {
                failed += 1;
                if failed <= 8 {
                    eprintln!("FAILED {}: {e}", file.display());
                }
            }
        }
    }
    println!("files {}: parsed {ok}, failed {failed}, without a mesh chunk {no_mesh}, meshes failing validation {invalid}, meshes with NaN {nan}", files.len());
    println!("versions: {}", versions.iter().map(|(k, v)| format!("0x{k:08X}:{v}")).collect::<Vec<_>>().join(" "));
    println!("mesh platforms (id: meshes): {platforms:?}");
    println!("totals: {verts} vertices, {tris} triangles, {skinned_meshes} skinned meshes (max influences {max_infl}), {skel} skeletons, {mats} materials, {nodes} nodes");
    println!("vertex strides (stride: meshes): {strides:?}");
    println!("vertex layouts (usage.index:type):");
    for (k, v) in &layouts {
        println!("  {v:5} x {k}");
    }
    println!("chunk ids (id: count): {}", chunk_ids.iter().map(|(k, v)| format!("0x{k:02X}:{v}")).collect::<Vec<_>>().join(" "));
    Ok(())
}

/// `abgtool kart-check <assets292>`: loads every kart variant (themes 2-6 and telepod, LOD 2..4) with its parts, wheels and driver.
fn kart_check(root: &str) -> Result<(), String> {
    let (checked, problems) = abgtool::kartdata::check_all(Path::new(root));
    for p in &problems {
        println!("PROBLEM {p}");
    }
    println!("{checked} kart variants (folder x LOD) checked, {} problems", problems.len());
    if problems.is_empty() { Ok(()) } else { Err(format!("{} problems", problems.len())) }
}
