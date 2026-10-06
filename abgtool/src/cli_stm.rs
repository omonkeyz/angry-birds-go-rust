//! CLI for STME (`track.stm`) files. Dispatched from main before its own command table.
use abgtool::stm;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub const USAGE: &str = "\
 \x20 abgtool stm-info <track.stm>                      (header, toc summary, materials, splines, helpers, bounds, collision)\n\
 \x20 abgtool stm-obj  <track.stm> <out.obj> [q2|q3|all]  (visual meshes: positions, normals, uvs, per-material groups; writes out.mtl too)\n\
 \x20 abgtool stm-col  <track.stm> <out.obj>              (collision triangles of every kd tree, one group per surface material id)\n\
 \x20 abgtool stm-scan <dir>                              (parse every track.stm under dir, validate, print totals)\n\
 \x20 abgtool stm-toc  <track.stm>                        (raw table of contents)\n\
 \x20 abgtool stm-dump <track.stm> <entry>                (hex words of one toc entry's in-memory image)";

pub fn dispatch(args: &[String]) -> Option<Result<(), String>> {
    let cmd = args.first()?.as_str();
    match cmd {
        "stm-info" if args.len() == 2 => Some(info(&args[1])),
        "stm-obj" if args.len() == 3 || args.len() == 4 => Some(obj(&args[1], &args[2], args.get(3).map(String::as_str).unwrap_or("q2"))),
        "stm-col" if args.len() == 3 => Some(col(&args[1], &args[2])),
        "stm-scan" if args.len() == 2 => Some(scan(&args[1])),
        "stm-helpers" if args.len() == 2 => Some(helpers(&args[1])),
        "stm-spline" if args.len() == 4 => Some(spline_dump(&args[1], &args[2], args[3].parse().unwrap_or(10))),
        "stm-preview" if args.len() == 3 => Some(preview(&args[1], &args[2])),
        "stm-toc" if args.len() == 2 => Some(toc(&args[1])),
        "stm-dump" if args.len() == 3 => Some(dump_entry(&args[1], &args[2])),
        _ => None,
    }
}

fn load(path: &str) -> Result<stm::Stm, String> {
    let data = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    stm::parse(&data).map_err(|e| format!("{path}: {e}"))
}

#[derive(Default)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub nan: usize,
    pub n: usize,
}
impl Bounds {
    pub fn new() -> Self {
        Bounds { min: [f32::MAX; 3], max: [f32::MIN; 3], nan: 0, n: 0 }
    }
    pub fn add(&mut self, p: [f32; 3]) {
        self.n += 1;
        for k in 0..3 {
            if !p[k].is_finite() {
                self.nan += 1;
                return;
            }
        }
        for k in 0..3 {
            self.min[k] = self.min[k].min(p[k]);
            self.max[k] = self.max[k].max(p[k]);
        }
    }
}

fn info(path: &str) -> Result<(), String> {
    let s = load(path)?;
    println!("{path}");
    println!("  magic STME, version 0x{:08X}, tag {:?}, toc entries {}", s.version, String::from_utf8_lossy(&s.tag), s.toc.len());
    let mut kinds: std::collections::BTreeMap<u32, usize> = Default::default();
    for e in &s.toc {
        *kinds.entry(e.kind).or_default() += 1;
    }
    println!("  toc kinds (kind: count) {kinds:?}   [1 pvs, 2 kd meta, 3 kd tree, 4 texture .tex, 5 .mp1, 6 section .q02, 7 section .q03]");
    println!("  pvs: {} sections, {} visibility cells, {} base sections, {} helpers, {} splines, {} cameras, {} markup blocks", s.pvs.section_count, s.pvs.cell_count, s.pvs.base_section_count, s.pvs.helpers.len(), s.pvs.splines.len(), s.pvs.cameras.len(), s.pvs.markup.len());
    println!("  materials {} / textures {}", s.materials.len(), s.textures.len());
    for (i, m) in s.materials.iter().enumerate() {
        println!("    mat {i:3} flags 0x{:02x} stride {:2} {:<48} tex {:?}", m.vertex_flags, stm::vertex_stride(m.vertex_flags), m.name, m.textures);
    }
    for sp in &s.pvs.splines {
        let mut b = Bounds::new();
        for p in &sp.points {
            b.add(*p);
        }
        println!("  spline {:<24} {:4} points  bbox {:?} .. {:?}", sp.name, sp.points.len(), b.min, b.max);
    }
    let q2 = s.meshes_for_quality(2);
    let mut b = Bounds::new();
    let (mut v, mut t) = (0usize, 0usize);
    for m in &q2 {
        v += m.vertices.len();
        t += m.indices.len() / 3;
        for x in &m.vertices {
            b.add(x.pos);
        }
    }
    println!("  visual (q02 preferred): {} meshes, {v} vertices, {t} triangles, bbox {:?} .. {:?}, nan {}", q2.len(), b.min, b.max, b.nan);
    let mut cb = Bounds::new();
    let mut ct = 0usize;
    let mut mats: std::collections::BTreeMap<u16, usize> = Default::default();
    for k in &s.kd_trees {
        ct += k.triangles.len();
        for tr in &k.triangles {
            *mats.entry(tr.material).or_default() += 1;
            for p in tr.v {
                cb.add(p);
            }
        }
    }
    println!("  kd grid {}x{} origin {:?} inv_cell {:?} y {:?} cells {:?}", s.kd_grid.dims[0], s.kd_grid.dims[1], s.kd_grid.origin, s.kd_grid.inv_cell, s.kd_grid.y_range, s.kd_grid.cells);
    println!("  collision: {} trees, {ct} triangles, bbox {:?} .. {:?}, nan {}, surface material ids {mats:?}", s.kd_trees.len(), cb.min, cb.max, cb.nan);
    Ok(())
}

fn obj(path: &str, out: &str, which: &str) -> Result<(), String> {
    let s = load(path)?;
    let meshes: Vec<&stm::Mesh> = match which {
        "q2" => s.meshes_for_quality(2),
        "q3" => s.meshes_for_quality(3),
        "all" => s.meshes.iter().collect(),
        _ => return Err("quality must be q2, q3 or all".into()),
    };
    let mtl_name = Path::new(out).with_extension("mtl");
    let mut o = String::new();
    let _ = writeln!(o, "mtllib {}", mtl_name.file_name().and_then(|n| n.to_str()).unwrap_or("track.mtl"));
    let mut base = 1usize;
    let mut used: std::collections::BTreeSet<usize> = Default::default();
    for m in &meshes {
        let _ = writeln!(o, "o {}_{}", m.section_name, m.submesh);
        let _ = writeln!(o, "usemtl m{}", m.material);
        used.insert(m.material);
        for v in &m.vertices {
            let _ = writeln!(o, "v {} {} {}", v.pos[0], v.pos[1], v.pos[2]);
        }
        let has_uv = m.vertex_flags & 0x20 != 0;
        let has_n = m.vertex_flags & 0x04 != 0;
        if has_uv {
            for v in &m.vertices {
                let _ = writeln!(o, "vt {} {}", v.uv[0], 1.0 - v.uv[1]);
            }
        }
        if has_n {
            for v in &m.vertices {
                let _ = writeln!(o, "vn {} {} {}", v.normal[0], v.normal[1], v.normal[2]);
            }
        }
        for t in m.indices.chunks_exact(3) {
            let f = |i: u32| {
                let k = i as usize + base;
                match (has_uv, has_n) {
                    (true, true) => format!("{k}/{k}/{k}"),
                    (true, false) => format!("{k}/{k}"),
                    (false, true) => format!("{k}//{k}"),
                    _ => format!("{k}"),
                }
            };
            let _ = writeln!(o, "f {} {} {}", f(t[0]), f(t[1]), f(t[2]));
        }
        base += m.vertices.len();
    }
    fs::write(out, o).map_err(|e| e.to_string())?;
    let mut mt = String::new();
    for i in used {
        let m = &s.materials[i];
        let _ = writeln!(mt, "newmtl m{i}\n# {}", m.name);
        let _ = writeln!(mt, "Kd 1 1 1");
        if let Some(t) = m.textures.first() {
            let _ = writeln!(mt, "map_Kd {}.png", t.trim_end_matches(".tex"));
        }
    }
    fs::write(&mtl_name, mt).map_err(|e| e.to_string())?;
    println!("wrote {out}: {} meshes", meshes.len());
    Ok(())
}

fn col(path: &str, out: &str) -> Result<(), String> {
    let s = load(path)?;
    let mut o = String::new();
    let mut base = 1usize;
    let mut by_mat: std::collections::BTreeMap<u16, Vec<[[f32; 3]; 3]>> = Default::default();
    for t in s.kd_trees.iter().flat_map(|k| k.triangles.iter()) {
        by_mat.entry(t.material).or_default().push(t.v);
    }
    for (m, tris) in by_mat {
        let _ = writeln!(o, "g surface_{m}");
        for t in tris {
            for p in t {
                let _ = writeln!(o, "v {} {} {}", p[0], p[1], p[2]);
            }
            let _ = writeln!(o, "f {} {} {}", base, base + 1, base + 2);
            base += 3;
        }
    }
    fs::write(out, o).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}

fn find_stm(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                find_stm(&p, out);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("stm")) {
                out.push(p);
            }
        }
    }
}

fn scan(dir: &str) -> Result<(), String> {
    let mut files = Vec::new();
    find_stm(Path::new(dir), &mut files);
    files.sort();
    let (mut ok, mut failed, mut problems) = (0usize, 0usize, 0usize);
    for f in &files {
        let data = fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        match stm::parse(&data) {
            Ok(s) => {
                ok += 1;
                let q2 = s.meshes_for_quality(2);
                let mut b = Bounds::new();
                let mut tris = 0usize;
                let mut bad = 0usize;
                for m in &s.meshes {
                    if m.indices.iter().any(|&i| i as usize >= m.vertices.len()) {
                        bad += 1;
                    }
                }
                for m in &q2 {
                    tris += m.indices.len() / 3;
                    for v in &m.vertices {
                        b.add(v.pos);
                    }
                }
                let mut cb = Bounds::new();
                let mut ct = 0usize;
                for t in s.kd_trees.iter().flat_map(|k| k.triangles.iter()) {
                    ct += 1;
                    for p in t.v {
                        cb.add(p);
                    }
                }
                let ext = |b: &Bounds| (0..3).map(|k| b.max[k] - b.min[k]).fold(0.0f32, f32::max);
                let flag = if b.nan + cb.nan + bad > 0 { problems += 1; " PROBLEM" } else { "" };
                println!(
                    "{} ok: {} sections {} meshes {} tris(q02), visual extent {:.0} m, collision {} tris extent {:.0} m, splines {}, nan {}, bad-index meshes {}{flag}",
                    f.display(), s.sections.len(), q2.len(), tris, ext(&b), ct, ext(&cb), s.pvs.splines.len(), b.nan + cb.nan, bad
                );
            }
            Err(e) => {
                failed += 1;
                println!("{} FAILED: {e}", f.display());
            }
        }
    }
    println!("parsed {ok}, failed {failed}, with problems {problems}");
    if failed > 0 {
        return Err(format!("{failed} files failed"));
    }
    Ok(())
}

fn toc(path: &str) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    for (i, e) in stm::toc(&data)?.iter().enumerate() {
        println!(
            "{i:3} kind {} {:<34} off {:>9} disk {:>8} size {:>8} fixup {:>7} skip {}/{}",
            e.kind, e.name, e.offset, e.disk_size, e.size, e.fixup_bytes, e.skip_a, e.skip_b
        );
    }
    Ok(())
}

fn dump_entry(path: &str, name: &str) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let t = stm::toc(&data)?;
    let e = t.iter().find(|e| e.name.eq_ignore_ascii_case(name)).ok_or("no such entry")?;
    println!("{e:?}");
    let blk = &data[e.offset as usize..(e.offset + e.disk_size) as usize];
    for (i, w) in blk.chunks(16).enumerate().take(4096) {
        let words: Vec<String> = w
            .chunks(4)
            .map(|c| if c.len() == 4 { format!("{:08x}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])) } else { String::new() })
            .collect();
        println!("{:04x}: {}", i * 16, words.join(" "));
    }
    Ok(())
}

/// Top-down (x, z) preview: collision triangles coloured by surface id, spline centre lines in white.
pub fn preview(path: &str, out: &str) -> Result<(), String> {
    use abgtool::png;
    let s = load(path)?;
    let (w, h) = (1400usize, 1000usize);
    let mut img = vec![0u8; w * h * 4];
    for p in img.chunks_exact_mut(4) {
        p.copy_from_slice(&[20, 24, 32, 255]);
    }
    let mut b = Bounds::new();
    for t in s.kd_trees.iter().flat_map(|k| k.triangles.iter()) {
        for p in t.v {
            b.add(p);
        }
    }
    let sx = (w as f32 - 20.0) / (b.max[0] - b.min[0]);
    let sz = (h as f32 - 20.0) / (b.max[2] - b.min[2]);
    let sc = sx.min(sz);
    let proj = |p: [f32; 3]| (10.0 + (p[0] - b.min[0]) * sc, 10.0 + (p[2] - b.min[2]) * sc);
    let colour = |m: u16| -> [u8; 3] {
        let k = (m as u32).wrapping_mul(2654435761);
        [(k >> 8) as u8 | 64, (k >> 16) as u8 | 64, (k >> 24) as u8 | 64]
    };
    for t in s.kd_trees.iter().flat_map(|k| k.triangles.iter()) {
        let pts = [proj(t.v[0]), proj(t.v[1]), proj(t.v[2])];
        let (x0, x1) = (pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0), pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(w as f32 - 1.0));
        let (y0, y1) = (pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0), pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(h as f32 - 1.0));
        let c = colour(t.material);
        let edge = |a: (f32, f32), b: (f32, f32), p: (f32, f32)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        let area = edge(pts[0], pts[1], pts[2]);
        if area == 0.0 {
            continue;
        }
        let mut y = y0;
        while y <= y1 {
            let mut x = x0;
            while x <= x1 {
                let p = (x + 0.5, y + 0.5);
                let (e0, e1, e2) = (edge(pts[1], pts[2], p), edge(pts[2], pts[0], p), edge(pts[0], pts[1], p));
                if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0 && area > 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0 && area < 0.0) {
                    let at = (y as usize * w + x as usize) * 4;
                    img[at..at + 3].copy_from_slice(&c);
                }
                x += 1.0;
            }
            y += 1.0;
        }
    }
    for sp in &s.pvs.splines {
        for p in &sp.points {
            let (x, y) = proj(*p);
            if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h {
                let at = (y as usize * w + x as usize) * 4;
                img[at..at + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
    }
    fs::write(out, png::encode_rgba(w as u32, h as u32, &img)).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}

pub fn spline_dump(path: &str, name: &str, n: usize) -> Result<(), String> {
    let s = load(path)?;
    let sp = s.spline(name).ok_or("no such spline")?;
    for (i, p) in sp.points.iter().enumerate().take(n) {
        let e = sp.extra.get(i);
        println!("{i:3} {:9.3} {:9.3} {:9.3}   extra {:?} u32 {:?}", p[0], p[1], p[2], e.map(|e| &e[..6]), e.map(|e| e[6].to_bits()));
    }
    Ok(())
}

pub fn helpers(path: &str) -> Result<(), String> {
    let s = load(path)?;
    for h in &s.pvs.helpers {
        println!("{:<28} pos {:9.2} {:9.2} {:9.2}", h.name, h.matrix[12], h.matrix[13], h.matrix[14]);
    }
    Ok(())
}
