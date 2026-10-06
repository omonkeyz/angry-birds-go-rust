//! Parses every track.stm found in ../assets292/tracks (skipped when the data is not there).
use abgtool::stm;
use std::path::{Path, PathBuf};

fn tracks() -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let q = e.path();
                if q.is_dir() {
                    walk(&q, out);
                } else if q.file_name().is_some_and(|n| n == "track.stm") {
                    out.push(q);
                }
            }
        }
    }
    let mut v = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets292/tracks"), &mut v);
    v.sort();
    v
}

fn point_in_tri_xz(p: [f32; 3], t: &stm::CollisionTri) -> Option<f32> {
    let (a, b, c) = (t.v[0], t.v[1], t.v[2]);
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-9 {
        return None;
    }
    let l1 = ((b[2] - c[2]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[2] - c[2])) / d;
    let l2 = ((c[2] - a[2]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[2] - c[2])) / d;
    let l3 = 1.0 - l1 - l2;
    if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
        Some(l1 * a[1] + l2 * b[1] + l3 * c[1])
    } else {
        None
    }
}

#[test]
fn every_track_parses_and_is_sane() {
    for f in tracks() {
        let data = std::fs::read(&f).unwrap();
        let s = stm::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(!s.sections.is_empty() && !s.meshes.is_empty() && !s.kd_trees.is_empty(), "{}", f.display());
        for m in &s.meshes {
            assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()), "{} {}", f.display(), m.section_name);
            assert!(m.vertices.iter().all(|v| v.pos.iter().all(|c| c.is_finite())));
        }
        let tris = s.collision_triangles();
        assert!(tris.iter().all(|t| t.v.iter().all(|p| p.iter().all(|c| c.is_finite()))));
        // the race spline must run on the collision surface (sample every 5th point; most must lie over a triangle (the line floats 0-5 m above the surface; multi-level tracks differ more))
        let sp = s.spline("race_001").or_else(|| s.pvs.splines.iter().find(|x| x.name.starts_with("race_"))).expect("race spline");
        let (mut hit, mut total) = (0, 0);
        for p in sp.points.iter().step_by(5) {
            total += 1;
            if tris.iter().filter_map(|t| point_in_tri_xz(*p, t)).any(|y| y.is_finite()) {
                hit += 1;
            }
        }
        assert!(hit * 10 >= total * 9, "{}: only {hit}/{total} race spline points lie on the collision surface", f.display());
    }
}

#[test]
#[ignore]
fn dbg_spline_offsets() {
    for f in tracks() {
        let s = stm::parse(&std::fs::read(&f).unwrap()).unwrap();
        let tris = s.collision_triangles();
        let sp = s.pvs.splines.iter().find(|x| x.name == "race_001").unwrap();
        let mut offs = Vec::new();
        for p in sp.points.iter().step_by(10) {
            let mut best: Option<f32> = None;
            for y in tris.iter().filter_map(|t| point_in_tri_xz(*p, t)) {
                let d = p[1] - y;
                if best.map_or(true, |b| d.abs() < b.abs()) { best = Some(d); }
            }
            offs.push(best.map(|d| (d * 10.0).round() / 10.0));
        }
        println!("{}: {:?}", f.display(), offs);
    }
}

#[test]
fn normals_are_unit_length() {
    for f in tracks() {
        let s = stm::parse(&std::fs::read(&f).unwrap()).unwrap();
        let (mut n, mut ok) = (0usize, 0usize);
        for m in s.meshes.iter().filter(|m| m.vertex_flags & 4 != 0) {
            for v in &m.vertices {
                n += 1;
                let l = (v.normal[0] * v.normal[0] + v.normal[1] * v.normal[1] + v.normal[2] * v.normal[2]).sqrt();
                if (l - 1.0).abs() < 0.06 {
                    ok += 1;
                }
            }
        }
        println!("{}: {ok}/{n}", f.display());
        assert!(n == 0 || ok * 10 >= n * 9, "{}: normals are not unit length ({ok}/{n})", f.display());
    }
}

#[test]
fn toc_has_no_split_entries_and_tiles_the_file() {
    for f in tracks() {
        let data = std::fs::read(&f).unwrap();
        let s = stm::parse(&data).unwrap();
        // NonStreamedLoad's split/skip fields (+0x30/+0x34) are unused in every shipped track
        assert!(s.toc.iter().all(|e| e.skip_a == 0 && e.skip_b == 0), "{}", f.display());
        // every byte after the toc belongs to exactly one entry (disk block + streamed section data)
        let mut spans: Vec<(u32, u32, String)> = Vec::new();
        for e in s.toc.iter().filter(|e| e.disk_size != 0) {
            let mut len = e.disk_size;
            if (6..=8).contains(&e.kind) {
                let sec = stm::sections(&data).unwrap();
                let x = sec.iter().find(|x| x.name == e.name).unwrap();
                len += x.index_count * 2 + x.vertex_bytes;
                len = (len + 16) & !15; // padded to the next 16-byte boundary strictly above the data
            }
            spans.push((e.offset, len, e.name.clone()));
        }
        spans.sort();
        let mut at = 16 + 64 * s.toc.len() as u32;
        for (off, len, name) in &spans {
            assert_eq!(*off, at, "{}: gap/overlap before {name}", f.display());
            at = off + len;
        }
        assert_eq!(at as usize, data.len(), "{}: bytes after the last entry", f.display());
    }
}

#[test]
fn visual_meshes_lie_on_the_collision_surface() {
    // road meshes are world space: most vertices sit within 1 m of a collision triangle (no per-section transform)
    let mut any = false;
    for f in tracks().into_iter().take(6) {
        let s = stm::parse(&std::fs::read(&f).unwrap()).unwrap();
        let tris = s.collision_triangles();
        let (mut near, mut total) = (0, 0);
        for m in s.meshes_for_quality(2).into_iter().filter(|m| m.section_name.to_lowercase().starts_with("road_")) {
            for v in m.vertices.iter().step_by(37) {
                total += 1;
                if tris.iter().filter_map(|t| point_in_tri_xz(v.pos, t)).any(|y| (y - v.pos[1]).abs() < 1.0) {
                    near += 1;
                }
            }
        }
        println!("{}: {near}/{total}", f.display());
        if total > 0 {
            any = true;
            assert!(near * 10 >= total * 6, "{}: {near}/{total}", f.display());
        }
    }
    assert!(any);
}

#[test]
fn collision_triangle_normals_match_geometry() {
    for f in tracks().into_iter().take(5) {
        let s = stm::parse(&std::fs::read(&f).unwrap()).unwrap();
        let (mut n, mut ok, mut same_dir) = (0usize, 0usize, 0usize);
        for t in s.collision_triangles() {
            let e1 = [t.v[1][0] - t.v[0][0], t.v[1][1] - t.v[0][1], t.v[1][2] - t.v[0][2]];
            let e2 = [t.v[2][0] - t.v[0][0], t.v[2][1] - t.v[0][1], t.v[2][2] - t.v[0][2]];
            let c = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            if l < 1e-6 {
                continue;
            }
            let d = (c[0] * t.normal[0] + c[1] * t.normal[1] + c[2] * t.normal[2]) / l;
            n += 1;
            if d.abs() > 0.99 {
                ok += 1;
            }
            if d > 0.0 {
                same_dir += 1;
            }
        }
        println!("{}: {ok}/{n} normals match the triangle plane, {same_dir} wound the same way", f.display());
        assert!(ok * 100 >= n * 99);
    }
}

#[test]
#[ignore]
fn dbg_tri_flags() {
    for f in tracks() {
        let s = stm::parse(&std::fs::read(&f).unwrap()).unwrap();
        let mut fl: std::collections::BTreeMap<u16, usize> = Default::default();
        let mut us: std::collections::BTreeMap<u32, usize> = Default::default();
        let mut mt: std::collections::BTreeMap<u16, usize> = Default::default();
        for t in s.collision_triangles() {
            *fl.entry(t.flags).or_default() += 1;
            *us.entry(t.user).or_default() += 1;
            *mt.entry(t.material).or_default() += 1;
        }
        println!("{}: mat {mt:?} flags {fl:?} user {:?}", f.display(), us.iter().take(6).collect::<Vec<_>>());
    }
}
