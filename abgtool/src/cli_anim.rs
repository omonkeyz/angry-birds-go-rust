//! CLI for animation (`.xga`) files. Dispatched from main before its own command table.
use abgtool::anim::{self, Anim, TrackSet};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const USAGE: &str = "\
 \x20 abgtool anim-info   <file.xga> [keys=3]            (header, rate/duration, groups, tracks, key counts, first keys, helpers)\n\
 \x20 abgtool anim-scan   <dir> [models_dir]             (parse + validate every .xga under dir; character track names / hierarchy are cross-checked against characters/models/*.xgm)\n\
 \x20 abgtool anim-xref   <file.xga> <model.xgm>         (compare track names and hierarchy structure with a skeleton .xgm)\n\
 \x20 abgtool anim-sample <file.xga> <group> <track> <t>  (evaluate one track at t seconds: scale, rotation xyzw, translation)\n\
 \x20 abgtool anim-scml   <file.scml>                      (Spriter 2D UI animation: image files, entities, animations, timelines)";

pub fn dispatch(args: &[String]) -> Option<Result<(), String>> {
    let cmd = args.first()?.as_str();
    match cmd {
        "anim-info" if args.len() == 2 || args.len() == 3 => {
            Some(info(&args[1], args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3)))
        }
        "anim-scan" if args.len() == 2 || args.len() == 3 => Some(scan(&args[1], args.get(2).map(String::as_str))),
        "anim-xref" if args.len() == 3 => Some(xref(&args[1], &args[2])),
        "anim-scml" if args.len() == 2 => Some(scml(&args[1])),
        "anim-sample" if args.len() == 5 => Some(sample(&args[1], &args[2], &args[3], &args[4])),
        _ => None,
    }
}

fn load(path: &str) -> Result<Anim, String> {
    let d = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    anim::parse(&d).map_err(|e| format!("{path}: {e}"))
}

fn files_with_extension(root: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn storage_name(s: u32) -> &'static str {
    match s {
        1 => "dense samples",
        2 => "sparse keys + frame map",
        4 => "dense samples (physique)",
        0 => "matrices",
        _ => "?",
    }
}

fn info(path: &str, keys: usize) -> Result<(), String> {
    let a = load(path)?;
    println!("{path}");
    println!(
        "  first word 0x{:08X}  format 0x{:08X}  bones {}  collision blocks {}  chunks {}  trailing bytes {}",
        a.word0,
        a.format_version,
        a.bone_count,
        a.collision_blocks,
        a.chunks.len(),
        a.trailing
    );
    println!(
        "  rate {} fps  key stride {} frames  frames {}  duration {:.4} s  rotation conjugated on load: {}",
        a.fps,
        a.key_stride,
        a.frames,
        a.duration(),
        a.negate_quat
    );
    println!("  ext header words: {}", a.ext.iter().map(|w| format!("{w:08X}")).collect::<Vec<_>>().join(" "));
    for g in &a.groups {
        println!(
            "  group {} name {:?} chunks {} hierarchy {}",
            g.index,
            g.name.as_deref().unwrap_or(""),
            g.chunks.len(),
            g.hierarchy.as_ref().map(|h| h.node_count.to_string()).unwrap_or_else(|| "-".into())
        );
        let Some(ts) = &g.track_set else {
            println!("    no animation data");
            continue;
        };
        println!(
            "    chunk 0x{:02X} storage {} ({})  header count {}  frames {}  tracks {}",
            ts.chunk_kind,
            ts.storage,
            storage_name(ts.storage),
            ts.count,
            ts.frames,
            ts.tracks.len()
        );
        for (i, t) in ts.tracks.iter().enumerate() {
            println!("    track {i:2} node {:2} {:<24} keys {}", t.node_index, t.name, t.keys.len());
            for k in t.keys.iter().take(keys) {
                println!(
                    "        f{:<4} inv_dt {:<8.4} S[{:.4} {:.4} {:.4}] R[{:.5} {:.5} {:.5} {:.5}] T[{:.4} {:.4} {:.4}]",
                    k.frame, k.inv_dt, k.scale[0], k.scale[1], k.scale[2], k.rot[0], k.rot[1], k.rot[2], k.rot[3], k.pos[0], k.pos[1], k.pos[2]
                );
            }
        }
    }
    if !a.helpers.is_empty() {
        println!("  helpers ({}):", a.helpers.len());
        for h in &a.helpers {
            println!("    {:<16} limit {} samples {}", h.name, h.limit, h.samples.len());
            for s in h.samples.iter().take(keys) {
                println!("        idx {} pos {:?} rot {:?} scale {:?}", s.index, s.pos, s.rot, s.scale);
            }
        }
    }
    let mut ids: BTreeMap<u16, usize> = BTreeMap::new();
    for c in &a.chunks {
        *ids.entry(c.id).or_default() += 1;
    }
    println!("  chunk ids: {}", ids.iter().map(|(k, v)| format!("0x{k:02X}x{v}")).collect::<Vec<_>>().join(" "));
    Ok(())
}

fn sample(path: &str, group: &str, track: &str, t: &str) -> Result<(), String> {
    let a = load(path)?;
    let g: usize = group.parse().map_err(|_| "bad group")?;
    let tr: usize = track.parse().map_err(|_| "bad track")?;
    let t: f32 = t.parse().map_err(|_| "bad time")?;
    let s = a.sample(g, tr, t).ok_or("no such track")?;
    println!("frame {:.3}", a.frame_at(t));
    println!("scale {:?}\nrot(xyzw) {:?}\npos {:?}", s.scale, s.rot, s.pos);
    Ok(())
}

fn has_nan(ts: &TrackSet) -> bool {
    ts.tracks.iter().flat_map(|t| &t.keys).any(|k| {
        k.inv_dt.is_nan()
            || k.scale.iter().chain(&k.rot).chain(&k.pos).chain(&k.tail).any(|v| !v.is_finite())
    })
}

/// Structural checks beyond parsing. Returns a list of problems.
pub fn validate(a: &Anim) -> Vec<String> {
    let mut bad = Vec::new();
    if a.trailing != 0 {
        bad.push(format!("{} trailing bytes", a.trailing));
    }
    if a.ext.len() >= 9 && a.ext[1] as usize != a.helpers.len() {
        bad.push(format!("ext helper count {} != {} helper headers", a.ext[1], a.helpers.len()));
    }
    if a.ext.len() >= 9 && (a.ext[0] != 0 || a.ext[6] != 0) {
        bad.push(format!("ext words 0 / 6 are {} / {}", a.ext[0], a.ext[6]));
    }
    for g in &a.groups {
        let Some(ts) = &g.track_set else { continue };
        if ts.frames != a.frames {
            bad.push(format!("group {} frames {} != {}", g.index, ts.frames, a.frames));
        }
        if has_nan(ts) {
            bad.push(format!("group {} has NaN/inf", g.index));
        }
        if let Some(h) = &g.hierarchy {
            if ts.chunk_kind == 0x1B && h.node_count as usize != ts.tracks.len() {
                bad.push(format!("group {} hierarchy {} != tracks {}", g.index, h.node_count, ts.tracks.len()));
            }
        }
        for (ti, t) in ts.tracks.iter().enumerate() {
            if t.node_index as usize != ti && ts.chunk_kind == 0x1B {
                bad.push(format!("group {} track {ti} node index {}", g.index, t.node_index));
            }
            if t.name.is_empty() {
                bad.push(format!("group {} track {ti} has no name", g.index));
            }
            if ts.is_dense() {
                for (i, k) in t.keys.iter().enumerate() {
                    let want = ((i as f32 * a.key_stride) as i64).min(a.frames as i64);
                    if k.frame as i64 != want {
                        bad.push(format!("group {} track {ti} sample {i} frame {} != {want}", g.index, k.frame));
                        break;
                    }
                }
            } else {
                for i in 0..t.keys.len() {
                    if i == 0 {
                        if t.keys[0].inv_dt != 0.0 || t.keys[0].frame != 0 {
                            bad.push(format!("group {} track {ti} first key inv_dt {} frame {}", g.index, t.keys[0].inv_dt, t.keys[0].frame));
                        }
                        continue;
                    }
                    let (p, k) = (&t.keys[i - 1], &t.keys[i]);
                    if k.frame == p.frame && i + 1 == t.keys.len() && k.inv_dt == 0.0 && k.pos == p.pos && k.rot == p.rot && k.scale == p.scale {
                        continue;
                    }
                    if k.frame <= p.frame {
                        bad.push(format!("group {} track {ti} key {i} frame {} not after {}", g.index, k.frame, p.frame));
                        break;
                    }
                    let want = 1.0 / (k.frame - p.frame) as f32;
                    if (k.inv_dt - want).abs() > want * 1.0e-4 {
                        bad.push(format!("group {} track {ti} key {i} inv_dt {} != 1/{}", g.index, k.inv_dt, k.frame - p.frame));
                        break;
                    }
                }
                if t.frame_map.len() != ts.frames as usize {
                    bad.push(format!("group {} track {ti} map length {}", g.index, t.frame_map.len()));
                }
                for (f, &m) in t.frame_map.iter().enumerate() {
                    let m = m as usize;
                    let ok = m < t.keys.len()
                        && t.keys[m].frame as usize <= f
                        && t.keys.get(m + 1).map_or(true, |n| n.frame as usize > f);
                    if !ok {
                        bad.push(format!("group {} track {ti} frame {f} maps to key {m} (frames {:?})", g.index, t.keys.get(m).map(|k| k.frame)));
                        break;
                    }
                }
            }
        }
    }
    for g in &a.groups {
        if let Some(h) = &g.hierarchy {
            // byte 0 = first child, byte 1 = next sibling (0xff none): a walk from the unreferenced roots must reach every node exactly once (a forest)
            let n = h.nodes.len();
            let mut seen = vec![false; n];
            let mut referenced = vec![false; n];
            for nd in &h.nodes {
                let hd = nd.head();
                for link in [hd[0], hd[1]] {
                    if (link as usize) < n {
                        referenced[link as usize] = true;
                    }
                }
            }
            let mut stack: Vec<usize> = (0..n).rev().filter(|&i| !referenced[i]).collect();
            let mut count = 0;
            let mut broken = false;
            while let Some(i) = stack.pop() {
                if i >= n || seen[i] {
                    broken = true;
                    break;
                }
                seen[i] = true;
                count += 1;
                let hd = h.nodes[i].head();
                for link in [hd[0], hd[1]] {
                    if link != 0xFF {
                        stack.push(link as usize);
                    }
                }
            }
            if n > 0 && (broken || count != n) {
                bad.push(format!("group {} hierarchy child/sibling walk visits {count} of {n} nodes (broken {broken})", g.index));
            }
        }
        if let Some(ts) = &g.track_set {
            for (ti, t) in ts.tracks.iter().enumerate() {
                if let Some(k) = t.keys.iter().find(|k| ((k.rot.iter().map(|v| v * v).sum::<f32>()).sqrt() - 1.0).abs() > 0.01) {
                    bad.push(format!("group {} track {ti} rotation length {} at frame {}", g.index, k.rot.iter().map(|v| v * v).sum::<f32>().sqrt(), k.frame));
                    break;
                }
            }
        }
    }
    for h in &a.helpers {
        if h.samples.iter().any(|s| {
            s.pos.iter().flatten().chain(s.rot.iter().flatten()).chain(s.scale.iter().flatten()).any(|v| !v.is_finite())
        }) {
            bad.push(format!("helper {} has NaN", h.name));
        }
    }
    bad
}

/// Compare an animation's track names (and hierarchy chunk) with a skeleton. Returns (names_equal, hierarchy_equal).
fn compare_skeleton(a: &Anim, xgm: &[u8]) -> Result<(bool, bool), String> {
    let (names, hier) = anim::skeleton_of_xgm(xgm)?;
    let g = a.groups.iter().find(|g| g.track_set.as_ref().is_some_and(|t| t.chunk_kind == 0x1B)).ok_or("no physique group")?;
    let ts = g.track_set.as_ref().unwrap();
    let names_eq = ts.tracks.len() == names.len() && ts.tracks.iter().zip(&names).all(|(t, n)| &t.name == n);
    let hier_eq = match (&g.hierarchy, hier.first()) {
        (Some(h), Some(x)) => {
            let nodes_ok = x.len() == 8 + h.nodes.len() * 0x48
                && h.nodes.iter().enumerate().all(|(i, n)| {
                    let o = 8 + i * 0x48;
                    n.raw[0..4] == x[o..o + 4] && n.raw[0x44..0x48] == x[o + 0x44..o + 0x48]
                });
            nodes_ok
        }
        _ => false,
    };
    Ok((names_eq, hier_eq))
}

fn xref(xga: &str, xgm: &str) -> Result<(), String> {
    let a = load(xga)?;
    let m = fs::read(xgm).map_err(|e| format!("{xgm}: {e}"))?;
    let (names, _) = anim::skeleton_of_xgm(&m)?;
    let (ne, he) = compare_skeleton(&a, &m)?;
    println!("skeleton {} nodes, names equal index by index: {ne}, hierarchy node head/tail bytes identical: {he}", names.len());
    if let Some(ts) = a.groups.iter().filter_map(|g| g.track_set.as_ref()).find(|t| t.chunk_kind == 0x1B) {
        for (i, (t, n)) in ts.tracks.iter().zip(&names).enumerate() {
            if &t.name != n {
                println!("  node {i}: track {:?} vs skeleton {:?}", t.name, n);
            }
        }
    }
    Ok(())
}

fn scan(dir: &str, models: Option<&str>) -> Result<(), String> {
    let files = files_with_extension(Path::new(dir), "xga")?;
    let (mut ok, mut failed, mut problems) = (0usize, 0usize, 0usize);
    let (mut tracks, mut keys, mut helpers, mut samples, mut terminators) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut storage: BTreeMap<(u16, u32), usize> = BTreeMap::new();
    let mut groups_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut fps_hist: BTreeMap<String, usize> = BTreeMap::new();
    let (mut xref_total, mut xref_names, mut xref_hier, mut xref_missing, mut xref_other) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut env_found, mut env_total) = (0usize, 0usize);
    let mut helper_limits: BTreeMap<u32, usize> = BTreeMap::new();
    let mut helper_stats = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut stride_hist: BTreeMap<(u16, u32, u32), usize> = BTreeMap::new();
    let mut other_pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    for f in &files {
        let d = match fs::read(f) {
            Ok(d) => d,
            Err(e) => {
                failed += 1;
                println!("FAIL {}: {e}", f.display());
                continue;
            }
        };
        let a = match anim::parse(&d) {
            Ok(a) => a,
            Err(e) => {
                failed += 1;
                println!("FAIL {}: {e}", f.display());
                continue;
            }
        };
        ok += 1;
        let bad = validate(&a);
        if !bad.is_empty() {
            problems += 1;
            println!("PROBLEM {}: {}", f.display(), bad.join("; "));
        }
        *groups_hist.entry(a.groups.len()).or_default() += 1;
        *fps_hist.entry(format!("{} fps stride {}", a.fps, a.key_stride)).or_default() += 1;
        for g in &a.groups {
            if let Some(ts) = &g.track_set {
                *storage.entry((ts.chunk_kind, ts.storage)).or_default() += 1;
                *stride_hist.entry((ts.chunk_kind, ts.storage, a.key_stride as u32)).or_default() += 1;
                tracks += ts.tracks.len();
                keys += ts.tracks.iter().map(|t| t.keys.len()).sum::<usize>();
            }
        }
        helpers += a.helpers.len();
        terminators += terminator_keys(&a);
        for h in &a.helpers {
            *helper_limits.entry(h.limit).or_default() += 1;
            helper_stats.0 += 1;
            helper_stats.1 += h.samples.iter().filter(|s| s.pos.is_some()).count();
            helper_stats.2 += h.samples.iter().filter(|s| s.rot.is_some()).count();
            helper_stats.3 += h.samples.iter().filter(|s| s.scale.is_some()).count();
            helper_stats.4 += h.samples.iter().filter(|s| s.index as u32 >= h.limit).count();
        }
        samples += a.helpers.iter().map(|h| h.samples.len()).sum::<usize>();
        if !comps_is_character(f) {
            let (hit, total) = env_targets(f, &a);
            env_found += hit;
            env_total += total;
            if hit != total {
                println!("ENV {}: {hit} of {total} object names found in sibling models", f.display());
            }
        }
        // skeleton cross-check: <root>/characters/animation/<name>/x.xga  <->  <root>/characters/models/<name>_l0N.xgm
        let comps: Vec<String> = f.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if let Some(p) = comps.iter().position(|c| c == "animation") {
            if p > 0 && comps[p - 1] == "characters" && p + 2 < comps.len() && a.groups.iter().any(|g| g.hierarchy.is_some()) {
                let root: PathBuf = match models {
                    Some(m) => PathBuf::from(m),
                    None => comps[..p].iter().collect::<PathBuf>().join("models"),
                };
                xref_total += 1;
                let mut cands: Vec<PathBuf> = files_with_extension(&root, "xgm").unwrap_or_default();
                let stem = comps[p + 1].clone();
                cands.sort_by_key(|c| !c.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&stem)));
                let mut hit = None;
                for m in &cands {
                    if let Ok(xd) = fs::read(m) {
                        if let Ok((n, h)) = compare_skeleton(&a, &xd) {
                            if n {
                                hit = Some((m.clone(), h));
                                break;
                            }
                        }
                    }
                }
                match hit {
                    Some((m, h)) => {
                        xref_names += 1;
                        xref_hier += h as usize;
                        let same = m.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&stem));
                        if !same {
                            xref_other += 1;
                            *other_pairs.entry((stem.clone(), m.file_name().unwrap().to_string_lossy().into_owned())).or_default() += 1;
                        }
                        if !h {
                            println!("XREF {} vs {}: hierarchy structure differs", f.display(), m.display());
                        }
                    }
                    None => {
                        xref_missing += 1;
                        println!("XREF {}: no model has identical track names", f.display());
                    }
                }
            }
        }
    }
    println!("{} files: parsed {ok}, failed {failed}, with validation problems {problems}", files.len());
    println!("tracks {tracks}, keys {keys} (incl. {terminators} duplicate end-of-track keys), helpers {helpers}, helper samples {samples}");
    for ((k, s), n) in &storage {
        println!("  track sets chunk 0x{k:02X} storage {s} ({}): {n}", storage_name(*s));
    }
    println!("  (chunk, storage, stride) histogram: {stride_hist:?}");
    println!("  groups per file: {groups_hist:?}");
    println!("  rates: {fps_hist:?}");
    println!(
        "skeleton cross-check (character animations): {xref_total} compared, track names identical to a character model {xref_names} ({xref_other} only to a model of another name), hierarchy structure identical {xref_hier}, no model {xref_missing}"
    );
    {
        println!("helpers: {} headers, limits {:?}, samples with pos/rot/scale: {}/{}/{}, samples at index >= limit: {}", helper_stats.0, helper_limits, helper_stats.1, helper_stats.2, helper_stats.3, helper_stats.4);
    }
    println!("environment animations: object names of chunk 0x2d found in a sibling .xgm: {env_found} of {env_total}");
    for ((a, m), n) in &other_pairs {
        println!("  animation dir {a} -> model {m}: {n} files");
    }
    if failed > 0 || problems > 0 {
        return Err(format!("{failed} failed, {problems} with problems"));
    }
    Ok(())
}

/// Sparse tracks end with a copy of the last key at the same frame (inv_dt 0): counts them.
fn terminator_keys(a: &Anim) -> usize {
    a.groups
        .iter()
        .filter_map(|g| g.track_set.as_ref())
        .filter(|ts| !ts.is_dense())
        .flat_map(|ts| &ts.tracks)
        .filter(|t| t.keys.len() > 1 && t.keys[t.keys.len() - 1].frame == t.keys[t.keys.len() - 2].frame)
        .count()
}

fn contains_name(hay: &[u8], name: &str) -> bool {
    let n = name.as_bytes();
    !n.is_empty() && hay.windows(n.len() + 1).any(|w| &w[..n.len()] == n && (w[n.len()] == 0 || n.len() >= 31))
}

/// Environment / prop animations name their target object in chunk 0x2d; the object must exist as a NUL-terminated
/// name inside a sibling model (`.xgm`) of the `animations` directory. Returns (found, total) object names.
fn env_targets(path: &Path, a: &Anim) -> (usize, usize) {
    let dir = path.parent().unwrap_or(Path::new("."));
    let search = if dir.file_name().is_some_and(|n| n == "animations") { dir.parent().unwrap_or(dir) } else { dir };
    let models: Vec<Vec<u8>> = fs::read_dir(search)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "xgm"))
        .filter_map(|p| fs::read(p).ok())
        .collect();
    let names: Vec<&str> = a.groups.iter().filter_map(|g| g.name.as_deref()).collect();
    (names.iter().filter(|n| models.iter().any(|m| contains_name(m, n))).count(), names.len())
}

fn comps_is_character(p: &Path) -> bool {
    let s = p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    s.contains("characters/animation/")
}

fn scml(path: &str) -> Result<(), String> {
    let s = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let d = anim::scml::parse(&s)?;
    println!("{path}: Spriter SCML, {} image files, {} entities", d.files.len(), d.entities.len());
    for f in &d.files {
        println!("  file {}/{} {} {}x{} pivot {:?}", f.folder, f.id, f.name, f.width, f.height, f.pivot);
    }
    for e in &d.entities {
        println!("  entity {:?}", e.name);
        for a in &e.animations {
            println!(
                "    animation {:?} length {} ms looping {} mainline keys {} timelines {}",
                a.name,
                a.length_ms,
                a.looping,
                a.mainline.len(),
                a.timelines.len()
            );
            for t in &a.timelines {
                println!("      timeline {} {:?}: {} keys", t.id, t.name, t.keys.len());
                for k in t.keys.iter().take(3) {
                    println!("        t={} spin {} curve {:?} {:?}", k.time_ms, k.spin, k.curve_type, k.object);
                }
            }
        }
    }
    Ok(())
}
