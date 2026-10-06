//! Tests of the item placement against the real track data (skipped when `assets292` is not found).
//! Set `ABG_ASSETS` to the `assets292` folder to override the search (default: `../assets292` next to the game crate).
use super::eventdef::{self, Pattern};
use super::ground::ItemGround;
use super::place::{self, SnapResult};
use super::spline;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn assets_root() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("ABG_ASSETS") {
        c.push(PathBuf::from(p));
    }
    c.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets292"));
    c.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets292"));
    c.into_iter().find(|p| p.join("xml_gameplay").is_dir() && p.join("tracks").is_dir())
}

/// height of the triangle's plane under (x, z) when (x, z) lies inside it (xz projection)
fn plane_y(t: &abgtool::stm::CollisionTri, x: f32, z: f32) -> Option<f32> {
    let (a, b, c) = (t.v[0], t.v[1], t.v[2]);
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-9 {
        return None;
    }
    let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
    let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
    let l3 = 1.0 - l1 - l2;
    (l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0).then(|| l1 * a[1] + l2 * b[1] + l3 * c[1])
}

fn eventdef_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for ep in std::fs::read_dir(root.join("xml_gameplay")).unwrap().flatten() {
        if ep.path().is_dir() {
            for f in std::fs::read_dir(ep.path()).unwrap().flatten() {
                if f.path().extension().is_some_and(|e| e == "xml") {
                    files.push(f.path());
                }
            }
        }
    }
    files.sort();
    files
}

fn atoi_prefix(s: &str) -> usize {
    s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
}

/// independent count of the records an event must produce: sums per `<TrackItem ...>` tag, text-searching the attributes
fn expected_record_count(xml: &str) -> usize {
    let mut total = 0usize;
    for tag in xml.split("<TrackItem ").skip(1) {
        let tag = &tag[..tag.find('>').unwrap()];
        let attr = |k: &str| -> Option<String> {
            let key = format!(" {k}=\"");
            let key2 = format!("{k}=\"");
            let at = tag.find(&key).map(|i| i + key.len()).or_else(|| tag.starts_with(&key2).then(|| key2.len()))?;
            Some(tag[at..at + tag[at..].find('"')?].to_string())
        };
        if attr("helpername").as_deref() == Some("slalom_gate") {
            total += 2;
        } else if let Some(s) = attr("structuretype") {
            total += match s.as_str() {
                "test" => 5,
                "longblock" | "upright" | "bigblock" | "smallblock" => 1,
                "smallsquare4" => 4,
                "longtower" | "smalltriangle" | "tower" | "smalltower2" => 3,
                "arch1" => 5,
                "arch2" => 8,
                "smalltower1" => 2,
                "triangle" => 4,
                _ => 0,
            };
        } else if let (Some(_), Some(mx)) = (attr("minitemcount"), attr("maxitemcount")) {
            total += atoi_prefix(&mx);
        } else if let Some(c) = attr("itemcount") {
            total += atoi_prefix(&c);
        } else {
            total += 1;
        }
    }
    total
}

#[test]
fn record_counts_match_the_xml() {
    let Some(root) = assets_root() else { return };
    let mut events = 0;
    let mut items = 0;
    for f in eventdef_files(&root) {
        let xml = std::fs::read_to_string(&f).unwrap();
        let ev = eventdef::parse(&xml).unwrap();
        let recs = eventdef::expand(&ev.defs, ev.difficulty_level);
        assert_eq!(recs.len(), expected_record_count(&xml), "{}", f.display());
        // every record of a pattern carries its index 0..count
        for r in &recs {
            assert!(r.index >= 0 && r.index < r.count);
        }
        events += 1;
        items += recs.len();
    }
    eprintln!("record counts: {events} event files, {items} item records, all equal to the independent xml count");
    assert!(events > 100);
}

struct Track {
    ground: ItemGround,
    splines: Vec<spline::CSpline>,
    tris: Vec<abgtool::stm::CollisionTri>,
}

fn load_track(root: &std::path::Path, th: u32, run: u32) -> Option<Track> {
    let p = root.join(format!("tracks/theme{th:03}/run{run:03}/track.stm"));
    let stm = abgtool::stm::parse(&std::fs::read(p).ok()?).ok()?;
    Some(Track { ground: ItemGround::build(&stm), splines: spline::splines_of(&stm), tris: stm.collision_triangles() })
}

/// Coins, boost pads, tokens and blocks of many events, on all 15 tracks: nearly all land on the collision surface and
/// the snap distance matches the documented spline height (0-5 m above the surface).
#[test]
fn placed_items_lie_on_the_collision_surface() {
    let Some(root) = assets_root() else { return };
    let mut tracks: HashMap<(u32, u32), Option<Track>> = HashMap::new();
    let (mut total, mut on_surface, mut unplaced) = (0usize, 0usize, 0usize);
    let (mut gap_checked, mut gap_ok) = (0usize, 0usize);
    let mut shifts: Vec<f32> = Vec::new();
    let mut max_err = 0.0f32;
    let (mut brute_seen, mut brute_n, mut brute_ok) = (0usize, 0usize, 0usize);
    let mut per_track: HashMap<(u32, u32), (usize, usize)> = HashMap::new();
    for (n, f) in eventdef_files(&root).into_iter().enumerate() {
        if n % 4 != 0 {
            continue; // every 4th event keeps the test fast and still covers all 15 tracks
        }
        let xml = std::fs::read_to_string(&f).unwrap();
        let ev = eventdef::parse(&xml).unwrap();
        let Some(key) = ev.environment else { continue };
        let t = tracks.entry(key).or_insert_with(|| load_track(&root, key.0, key.1));
        let Some(t) = t else { continue };
        for r in eventdef::expand(&ev.defs, ev.difficulty_level) {
            if r.position.is_some() {
                continue;
            }
            let Some(pl) = place::place(&r, &t.splines, &t.ground) else {
                unplaced += 1; // the xml names a spline the track does not have (typos like "centre_2"): the original skips it too
                continue;
            };
            total += 1;
            let e = per_track.entry(key).or_default();
            e.0 += 1;
            let pos = pl.world.w_axis.truncate();
            match pl.snap {
                SnapResult::Direct | SnapResult::Raised => {
                    on_surface += 1;
                    e.1 += 1;
                    if r.local_x == 0.0 && r.local_y == 0.0 {
                        let surface_y = pos.y - r.height;
                        shifts.push((pl.spline_pos.y - surface_y).abs());
                        brute_seen += 1;
                        if brute_seen % 6 == 0 {
                            // independent check: brute force over every (non-ignored) collision triangle, barycentric plane height under (x, z)
                            let mut best = f32::MAX;
                            for tri in t.tris.iter().filter(|tri| ![7u16, 9, 29, 30, 37, 38].contains(&tri.material)) {
                                if let Some(y) = plane_y(tri, pos.x, pos.z) {
                                    best = best.min((y - surface_y).abs());
                                }
                            }
                            brute_n += 1;
                            if best < 0.02 {
                                brute_ok += 1;
                            }
                            max_err = max_err.max(if best.is_finite() { best } else { 1e9 });
                        }
                    }
                }
                SnapResult::NoSurface => {
                    // claims "no surface within 20 m below (also after +10 m)": verify with an independent query
                    gap_checked += 1;
                    let a = t.ground.below(pl.spline_pos);
                    let b = t.ground.below(pl.spline_pos + glam::Vec3::Y * 10.0);
                    let far = |h: Option<super::ground::GroundHit>, p: glam::Vec3| h.map_or(true, |h| (h.point - p).length_squared() >= place::SNAP_DIST_SQ);
                    if far(a, pl.spline_pos) && far(b, pl.spline_pos + glam::Vec3::Y * 10.0) {
                        gap_ok += 1;
                    }
                }
            }
        }
    }
    shifts.sort_by(|a, b| a.total_cmp(b));
    let pct = on_surface as f32 * 100.0 / total as f32;
    eprintln!(
        "placement: {total} items on {} tracks; on collision surface {on_surface} ({pct:.1}%); in gaps (>20 m from any surface) {gap_checked} (all confirmed: {}); skipped for a missing spline {unplaced}",
        per_track.len(),
        gap_ok == gap_checked
    );
    eprintln!("spline->surface distance: median {:.2} m, p90 {:.2} m (FORMATS.md: the race spline floats 0-5 m above the surface); brute-force triangle-plane check: {brute_ok}/{brute_n} items within 2 cm (worst {max_err:.5} m)", shifts[shifts.len() / 2], shifts[shifts.len() * 9 / 10]);
    for (k, v) in &per_track {
        eprintln!("  theme{:03} run{:03}: {} items, {:.1}% on the surface", k.0, k.1, v.0, v.1 as f32 * 100.0 / v.0 as f32);
    }
    assert!(per_track.len() >= 12, "tracks covered");
    assert!(pct > 90.0, "only {pct:.1}% on the surface");
    assert_eq!(gap_ok, gap_checked);
    assert!(brute_ok as f32 >= brute_n as f32 * 0.98, "brute force: {brute_ok}/{brute_n}");
    assert!(shifts[shifts.len() / 2] < 5.0);
}

/// Bar / stripe / tower patterns: spacing between consecutive items equals the xml `patternspacing` (before ground snapping effects: checked on
/// the pre-snap spline positions, which are exact)
#[test]
fn pattern_spacing_matches_the_xml() {
    let Some(root) = assets_root() else { return };
    let Some(t) = load_track(&root, 3, 0) else { return };
    let mk = |pattern, spacing, lateral| eventdef::ItemRecord {
        helper: "pickup_coin".into(),
        spline: "centre_001".into(),
        index: 0,
        count: 3,
        pattern,
        spacing,
        fraction: 0.3,
        height: 0.0,
        local_y: 0.0,
        local_x: 0.0,
        lateral,
        spread: 0.0,
        pitch: 0.0,
        position: None,
        def_index: 0,
        slalom: None,
    };
    // bar: items are `spacing` apart across the track (right vector), stripe: along the spline in index space
    let mut bar = Vec::new();
    for i in 0..3 {
        let mut r = mk(Pattern::Bar, 4.0, 0.2);
        r.index = i;
        bar.push(place::place(&r, &t.splines, &t.ground).unwrap().spline_pos);
    }
    assert!(((bar[1] - bar[0]).length() - 4.0).abs() < 0.3, "bar spacing {}", (bar[1] - bar[0]).length());
    assert!(((bar[2] - bar[1]).length() - 4.0).abs() < 0.3);
    let mut tower = Vec::new();
    for i in 0..3 {
        let mut r = mk(Pattern::Tower, 2.0, 0.0);
        r.index = i;
        tower.push(place::place(&r, &t.splines, &t.ground).unwrap().spline_pos);
    }
    assert!(((tower[1] - tower[0]) - glam::Vec3::Y * 2.0).length() < 1e-4);
    // stripe of 8 m spacing: consecutive items are ~8 m apart along the spline (index space maps back to metres via N/length)
    let mut stripe = Vec::new();
    for i in 0..4 {
        let mut r = mk(Pattern::Stripe, 8.0, 0.0);
        r.index = i;
        stripe.push(place::place(&r, &t.splines, &t.ground).unwrap().spline_pos);
    }
    for w in stripe.windows(2) {
        let d = (w[1] - w[0]).length();
        assert!((d - 8.0).abs() < 3.0, "stripe spacing {d}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ItemWorld behaviour on real events

use super::world::{ItemEvent, ItemKind, ItemWorld, KartState};
use glam::Vec3;

/// First event file (sorted) whose xml contains `needle` and whose track exists.
fn find_event(root: &std::path::Path, needle: &str) -> Option<(String, abgtool::stm::Stm)> {
    for f in eventdef_files(root) {
        let xml = std::fs::read_to_string(&f).unwrap();
        if !xml.contains(needle) {
            continue;
        }
        let ev = eventdef::parse(&xml).unwrap();
        let Some((th, run)) = ev.environment else { continue };
        let p = root.join(format!("tracks/theme{th:03}/run{run:03}/track.stm"));
        let Ok(d) = std::fs::read(p) else { continue };
        return Some((xml, abgtool::stm::parse(&d).unwrap()));
    }
    None
}

fn kart_at(p: Vec3, v: Vec3) -> KartState {
    KartState { pos: p, vel: v, ..Default::default() }
}

#[test]
fn coins_and_boost_pads_trigger_like_the_original() {
    let Some(root) = assets_root() else { return };
    let Some((xml, stm)) = find_event(&root, "boost_pad") else { return };
    let mut w = ItemWorld::build_with_assets(&xml, &stm, Some(&root)).unwrap();
    let coin = w.items.iter().position(|i| i.kind == ItemKind::Coin).expect("coin");
    let cpos = w.items[coin].pos();
    let r = w.items[coin].radius;
    assert!((r - 1.428).abs() < 0.01, "coin model half diagonal {r}");
    // just outside 2.0 + r: nothing; just inside: collected once, doubled with the coin doubler
    let out = w.update(0.016, &kart_at(cpos + Vec3::new(2.0 + r + 0.05, 0.0, 0.0), Vec3::ZERO));
    assert!(!out.iter().any(|e| matches!(e, ItemEvent::CoinCollected { item, .. } if *item == coin)));
    let ai = KartState { is_ai: true, ..kart_at(cpos, Vec3::ZERO) };
    assert!(w.update(0.016, &ai).iter().all(|e| !matches!(e, ItemEvent::CoinCollected { .. })), "AI karts cannot take coins");
    let k = KartState { coin_doubler: true, ..kart_at(cpos + Vec3::new(2.0 + r - 0.05, 0.0, 0.0), Vec3::ZERO) };
    let ev = w.update(0.016, &k);
    assert!(ev.iter().any(|e| matches!(e, ItemEvent::CoinCollected { item, value: 2 } if *item == coin)), "{ev:?}");
    let again = w.update(0.016, &k);
    assert!(!again.iter().any(|e| matches!(e, ItemEvent::CoinCollected { item, .. } if *item == coin)), "collected twice");
    // boost pad: reports while overlapping, `rising` only on the first frame, never consumed
    let pad = w.items.iter().position(|i| i.kind == ItemKind::BoostPad).expect("boost pad");
    let ppos = w.items[pad].pos();
    let e1 = w.update(0.016, &kart_at(ppos, Vec3::ZERO));
    assert!(e1.iter().any(|e| matches!(e, ItemEvent::BoostPadHit { item, rising: true } if *item == pad)), "{e1:?}");
    let e2 = w.update(0.016, &kart_at(ppos, Vec3::ZERO));
    assert!(e2.iter().any(|e| matches!(e, ItemEvent::BoostPadHit { item, rising: false } if *item == pad)));
    assert!(w.items[pad].active);
    let e3 = w.update(0.016, &kart_at(ppos + Vec3::new(50.0, 0.0, 0.0), Vec3::ZERO));
    assert!(!e3.iter().any(|e| matches!(e, ItemEvent::BoostPadHit { .. })));
}

#[test]
fn glass_block_smashes_into_fragments_and_stone_resists() {
    let Some(root) = assets_root() else { return };
    let Some((xml, stm)) = find_event(&root, "structuretype=\"smalltriangle\" structurematerial=\"glass\"") else { return };
    let mut w = ItemWorld::build_with_assets(&xml, &stm, Some(&root)).unwrap();
    let gi = w.items.iter().position(|i| i.helper.eq_ignore_ascii_case("smck_block_glass_2X2")).expect("glass block");
    let si = w.items[gi].smackable.unwrap();
    let centre = w.smackables[si].center();
    // slow bump (0.5 m/s): below the 96 threshold, no smash; nobody moves a fixed amount
    let slow = KartState { radius: 1.0, ..kart_at(centre + Vec3::new(0.0, 0.0, -1.9), Vec3::new(0.0, 0.0, 0.5)) };
    let ev = w.update(0.016, &slow);
    assert!(ev.iter().any(|e| matches!(e, ItemEvent::SmackableHit { smashed: false, .. })), "{ev:?}");
    // fast hit (15 m/s along the block's own z axis)
    let dir = w.smackables[si].world.z_axis.truncate().normalize();
    let fast = KartState { radius: 1.0, ..kart_at(centre - dir * 1.9, dir * 15.0) };
    let ev = w.update(0.016, &fast);
    let smashed = ev.iter().any(|e| matches!(e, ItemEvent::SmackableSmashed { type_id: 55, fragments: 3, .. }));
    assert!(smashed, "{ev:?}");
    assert_eq!(w.smackables.iter().filter(|s| s.temporary).count(), 3, "3 glass fragments");
    // the kart is slowed by the hit
    assert!(ev.iter().any(|e| matches!(e, ItemEvent::SmackableHit { kart_dv, .. } if kart_dv.dot(dir) < -0.1)));
}

#[test]
fn slalom_gate_pass_and_miss_on_a_real_track() {
    let Some(root) = assets_root() else { return };
    let Some((xml, stm)) = find_event(&root, "slalom_gate") else { return };
    let mut w = ItemWorld::build_with_assets(&xml, &stm, Some(&root)).unwrap();
    assert!(!w.gates.is_empty());
    let posts = w.items.iter().filter(|i| i.kind == ItemKind::SlalomPost).count();
    assert_eq!(posts, w.gates.len() * 2);
    // pass: the kart crosses the gate midpoint just after the gate's spline position
    let g0 = w.gates[0].clone();
    let sp = w.race_progress_for_test();
    let (mid_pos, progress) = (g0.midpoint(), g0.spline_pos + 0.5);
    let _ = sp;
    let ev = w.update(0.016, &KartState { progress: Some(progress), time_left: 30.0, ..kart_at(mid_pos, Vec3::new(0.0, 0.0, 10.0)) });
    assert!(ev.iter().any(|e| matches!(e, ItemEvent::GatePassed { gate: 0 })), "{ev:?}");
    // miss: far to the side of the second gate
    let g1 = w.gates[1].clone();
    let side = g1.post_a + (g1.post_a - g1.post_b).normalize() * 25.0;
    let before = w.smackables.len();
    let ev = w.update(0.016, &KartState { progress: Some(g1.spline_pos + 0.5), time_left: 30.0, ..kart_at(side, Vec3::new(0.0, 0.0, 10.0)) });
    assert!(ev.iter().any(|e| matches!(e, ItemEvent::GateMissed { gate: 1, penalty } if *penalty > 0.0)), "{ev:?}");
    assert_eq!(w.smackables.len() - before, (g1.cols * g1.rows) as usize, "TNT wall of cols x rows crates");
}

#[test]
fn required_models_exist() {
    let Some(root) = assets_root() else { return };
    let Some((xml, stm)) = find_event(&root, "pickup_seedrushtoken_snow") else { return };
    let mut w = ItemWorld::build_with_assets(&xml, &stm, Some(&root)).unwrap();
    let need = w.required_models();
    assert!(need.len() > 3);
    let mut tex = std::collections::BTreeSet::new();
    for n in &need {
        let info = super::models::load_model_info(&root, n.model.split('/').next().unwrap(), n.model.split('/').nth(1).unwrap());
        assert!(info.loaded, "missing model {}", n.model);
        tex.extend(n.textures.iter().cloned());
    }
    eprintln!("models needed ({}): {:?}\ntextures: {:?}", need.len(), need.iter().map(|n| n.model.clone()).collect::<Vec<_>>(), tex);
}
