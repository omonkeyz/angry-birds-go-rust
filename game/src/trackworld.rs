//! A real track of the 2.9.x data: `track.stm` visuals (textured), the collision surface the car drives on, the race spline and start line.
use crate::carsim::{Ground, RayHit};
use crate::render3d::{Draw3d, MeshId, Scene3d, TVertex3, TexMeshData};
use abgtool::stm::{self, Stm};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// Surface ids the original's ray query skips (`CXGSEnv::RayIntersect` filter, see FORMATS.md).
const IGNORED_SURFACES: [u16; 6] = [7, 9, 29, 30, 37, 38];
const CELL: f32 = 8.0;

pub struct TrackGround {
    tris: Vec<[Vec3; 3]>,
    normals: Vec<Vec3>,
    material: Vec<u16>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Triangles of the ray-ignored surface ids (volumes such as the thermals, id 29) with their stored plane normal, in a column grid.
    volumes: Vec<([Vec3; 3], Vec3, u16)>,
    volume_cells: HashMap<(i32, i32), Vec<u32>>,
    /// Triangles the camera collides with (`_FilterCameraCollision @ 000df7f4`: every surface id except 0x1d, 0x1e, 0x21, 0x25, 0x26;
    /// unlike the car's ray this keeps ids 7 and 9), with a column grid.
    cam_tris: Vec<[Vec3; 3]>,
    cam_cells: HashMap<(i32, i32), Vec<u32>>,
}

/// Surface ids the camera ray / sphere ignores (`_FilterCameraCollision @ 000df7f4`).
const CAMERA_IGNORED_SURFACES: [u16; 5] = [0x1d, 0x1e, 0x21, 0x25, 0x26];

/// Closest point of triangle `abc` to `p` (Ericson, Real-Time Collision Detection).
fn closest_point_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

impl TrackGround {
    fn build(stm: &Stm) -> TrackGround {
        let mut g = TrackGround { tris: Vec::new(), normals: Vec::new(), material: Vec::new(), cells: HashMap::new(), volumes: Vec::new(), volume_cells: HashMap::new(), cam_tris: Vec::new(), cam_cells: HashMap::new() };
        for tree in &stm.kd_trees {
            for t in &tree.triangles {
                let v = [Vec3::from(t.v[0]), Vec3::from(t.v[1]), Vec3::from(t.v[2])];
                if !CAMERA_IGNORED_SURFACES.contains(&t.material) {
                    let index = g.cam_tris.len() as u32;
                    let (min, max) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
                    for cx in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
                        for cz in (min.z / CELL).floor() as i32..=(max.z / CELL).floor() as i32 {
                            g.cam_cells.entry((cx, cz)).or_default().push(index);
                        }
                    }
                    g.cam_tris.push(v);
                }
                if IGNORED_SURFACES.contains(&t.material) {
                    if matches!(t.material, 29 | 37 | 38) {
                        let index = g.volumes.len() as u32;
                        let (min, max) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
                        for cx in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
                            for cz in (min.z / CELL).floor() as i32..=(max.z / CELL).floor() as i32 {
                                g.volume_cells.entry((cx, cz)).or_default().push(index);
                            }
                        }
                        g.volumes.push((v, Vec3::from(t.normal), t.material));
                    }
                    continue;
                }
                let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
                let index = g.tris.len() as u32;
                let (min, max) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
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

    /// Nearest crossing of the segment with a collision triangle (Moller-Trumbore), as the fraction along the segment.
    fn cast(&self, origin: Vec3, vec: Vec3) -> Option<(f32, usize)> {
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
                    let p = vec.cross(e2);
                    let det = e1.dot(p);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = origin - a;
                    let u = s.dot(p) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = s.cross(e1);
                    let v = vec.dot(q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let t = e2.dot(q) * inv;
                    // only crossings from above: the segment must start on the upper side of the face
                    if (0.0..=1.0).contains(&t) && self.normals[i as usize].dot(vec) < 0.0 && best.map_or(true, |(bt, _)| t < bt) {
                        best = Some((t, i as usize));
                    }
                }
            }
        }
        best
    }
}

impl Ground for TrackGround {
    fn height_normal(&self, p: Vec3) -> (f32, Vec3) {
        match self.cast(p + Vec3::Y * 3.0, Vec3::new(0.0, -200.0, 0.0)) {
            Some((t, i)) => (p.y + 3.0 - 200.0 * t, self.normals[i]),
            None => (-1.0e4, Vec3::Y),
        }
    }

    fn material(&self, p: Vec3) -> u32 {
        match self.cast(p + Vec3::Y * 3.0, Vec3::new(0.0, -200.0, 0.0)) {
            Some((_, i)) => self.material[i] as u32,
            None => 1,
        }
    }

    fn in_material_volume(&self, p: Vec3, material: u32) -> bool {
        let Some(list) = self.volume_cells.get(&((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32)) else { return false };
        // straight up from `p`: first triangle of that surface id above, vertical ray in the xz projection
        let mut best: Option<(f32, Vec3)> = None;
        for &i in list {
            let ([a, b, c], n, m) = self.volumes[i as usize];
            if m as u32 != material {
                continue;
            }
            let (v0, v1, v2) = ((b.x - a.x, b.z - a.z), (c.x - a.x, c.z - a.z), (p.x - a.x, p.z - a.z));
            let det = v0.0 * v1.1 - v1.0 * v0.1;
            if det.abs() < 1e-9 {
                continue;
            }
            let u = (v2.0 * v1.1 - v1.0 * v2.1) / det;
            let w = (v0.0 * v2.1 - v2.0 * v0.1) / det;
            if u < 0.0 || w < 0.0 || u + w > 1.0 {
                continue;
            }
            let y = a.y + u * (b.y - a.y) + w * (c.y - a.y);
            let t = y - p.y;
            if t >= 0.0 && best.map_or(true, |(bt, _)| t < bt) {
                best = Some((t, n));
            }
        }
        // back face (stored plane normal points along the ray) = we are inside the closed volume
        best.map_or(false, |(_, n)| n.y > 0.0)
    }

    fn ray(&self, origin: Vec3, vec: Vec3) -> Option<RayHit> {
        let (t, i) = self.cast(origin, vec)?;
        Some(RayHit { point: origin + vec * t, normal: self.normals[i], material: self.material[i] as u32 })
    }

    /// Two-sided nearest crossing of the segment with the camera collision triangles (Moller-Trumbore).
    fn cam_ray(&self, origin: Vec3, vec: Vec3) -> Option<(Vec3, Vec3)> {
        let end = origin + vec;
        let (min, max) = (origin.min(end), origin.max(end));
        let mut best: Option<(f32, Vec3)> = None;
        let mut seen: Vec<u32> = Vec::new();
        for cx in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
            for cz in (min.z / CELL).floor() as i32..=(max.z / CELL).floor() as i32 {
                let Some(list) = self.cam_cells.get(&(cx, cz)) else { continue };
                for &i in list {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let [a, b, c] = self.cam_tris[i as usize];
                    let (e1, e2) = (b - a, c - a);
                    let p = vec.cross(e2);
                    let det = e1.dot(p);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = origin - a;
                    let u = s.dot(p) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = s.cross(e1);
                    let v = vec.dot(q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let t = e2.dot(q) * inv;
                    if (0.0..=1.0).contains(&t) && best.map_or(true, |(bt, _)| t < bt) {
                        let n = e1.cross(e2).normalize_or_zero();
                        best = Some((t, if n.dot(vec) > 0.0 { -n } else { n }));
                    }
                }
            }
        }
        best.map(|(t, n)| (origin + vec * t, n))
    }

    /// Deepest overlap of the sphere with the camera collision triangles; the normal points from the surface to the centre.
    fn cam_sphere(&self, centre: Vec3, radius: f32) -> Option<(Vec3, Vec3)> {
        let (min, max) = (centre - Vec3::splat(radius), centre + Vec3::splat(radius));
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        let mut seen: Vec<u32> = Vec::new();
        for cx in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
            for cz in (min.z / CELL).floor() as i32..=(max.z / CELL).floor() as i32 {
                let Some(list) = self.cam_cells.get(&(cx, cz)) else { continue };
                for &i in list {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let [a, b, c] = self.cam_tris[i as usize];
                    let q = closest_point_on_triangle(centre, a, b, c);
                    let d = (centre - q).length();
                    if d < radius && best.map_or(true, |(bd, _, _)| d < bd) {
                        let n = if d > 1e-5 {
                            (centre - q) / d
                        } else {
                            let n = (b - a).cross(c - a).normalize_or_zero();
                            if n == Vec3::ZERO { Vec3::Y } else { n }
                        };
                        best = Some((d, q, n));
                    }
                }
            }
        }
        best.map(|(_, q, n)| (q, n))
    }
}

pub struct TrackWorld {
    pub name: String,
    pub draws: Vec<Draw3d>,
    pub ground: TrackGround,
    /// Position and forward direction of the start line (`spline_startline`) and the finish line.
    pub start: Option<(Vec3, Vec3)>,
    pub finish: Option<(Vec3, Vec3)>,
    /// The racing line (`race_001`) with its per-point extra data.
    pub race_line: Vec<Vec3>,
    pub sky: [f32; 3],
    /// The parsed track (splines, helpers, collision) and its `track.xml`, kept for the race rules and the item world.
    pub stm: Stm,
    pub track_xml: String,
}

fn helper_pose(stm: &Stm, name: &str) -> Option<(Vec3, Vec3)> {
    let h = stm.pvs.helpers.iter().find(|h| h.name.eq_ignore_ascii_case(name))?;
    let m = Mat4::from_cols_array(&h.matrix);
    Some((m.transform_point3(Vec3::ZERO), m.transform_vector3(Vec3::Z).normalize_or_zero()))
}

impl TrackWorld {
    /// `root` = the assets292 folder, `theme` = `theme002`, `run` = `run000`.
    pub fn load(scene: &mut Scene3d, root: &Path, theme: &str, run: &str, quality: u8) -> Result<TrackWorld, String> {
        let path = root.join("tracks").join(theme).join(run).join("track.stm");
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let stm = stm::parse(&data).map_err(|e| format!("{}: {e}", path.display()))?;

        // one mesh per texture: every section is already in world space
        let mut textures: HashMap<String, Option<crate::render3d::TexId>> = HashMap::new();
        let mut groups: HashMap<String, TexMeshData> = HashMap::new();
        for mesh in stm.meshes_for_quality(quality) {
            let material = &stm.materials[mesh.material];
            if material.name.contains("Sea") || material.name.to_ascii_lowercase().contains("shadow") {
                // water and shadow-projector materials need their own shaders; skipped for now
            }
            let tex_name = material.textures.first().cloned().unwrap_or_default();
            let group = groups.entry(tex_name).or_default();
            let base = group.vertices.len() as u32;
            for v in &mesh.vertices {
                group.vertices.push(TVertex3 {
                    pos: v.pos,
                    normal: v.normal,
                    // the vertex colour of the track meshes is not a diffuse tint (it made terrain dark purple), so it is not applied
                    color: [1.0; 4],
                    uv: v.uv,
                });
            }
            group.indices.extend(mesh.indices.iter().map(|i| i + base));
        }
        let mut draws = Vec::new();
        for (tex_name, data) in groups {
            let id = textures
                .entry(tex_name.clone())
                .or_insert_with(|| {
                    let stem = tex_name.trim_end_matches(".tex");
                    let png = root.join("textures/environments").join(theme).join("textures").join(format!("{stem}.png"));
                    let img = image::open(&png).ok()?.to_rgba8();
                    let (w, h) = img.dimensions();
                    let mut raw = img.into_raw();
                    Some(scene.add_texture(w, h, raw))
                })
                .to_owned();
            let mesh: MeshId = match id {
                Some(t) => scene.add_tex_mesh(&data, t),
                None => {
                    // texture missing: flat grey
                    let mut plain = crate::render3d::MeshData::default();
                    plain.vertices = data.vertices.iter().map(|v| crate::render3d::Vertex3 { pos: v.pos, normal: v.normal, color: [0.6, 0.6, 0.6, 1.0] }).collect();
                    plain.indices = data.indices.clone();
                    scene.add_mesh(&plain)
                }
            };
            draws.push(Draw3d { mesh, model: Mat4::IDENTITY, tint: [1.0, 1.0, 1.0, 1.0] });
        }

        let ground = TrackGround::build(&stm);
        let race_line: Vec<Vec3> = stm
            .pvs
            .splines
            .iter()
            .find(|s| s.name.starts_with("race_001"))
            .map(|s| s.points.iter().map(|p| Vec3::from(*p)).collect())
            .unwrap_or_default();
        // the helper matrix gives the position; the travel direction is the racing line at that point
        let mut start = helper_pose(&stm, "spline_startline");
        if let (Some((p, d)), true) = (start.as_mut(), race_line.len() > 3) {
            let nearest = (0..race_line.len() - 2).min_by(|&a, &b| race_line[a].distance(*p).total_cmp(&race_line[b].distance(*p))).unwrap_or(0);
            let forward = race_line[nearest + 2] - race_line[nearest];
            let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
            if flat != Vec3::ZERO {
                *d = flat;
            }
        }
        Ok(TrackWorld {
            name: format!("{theme}/{run}"),
            draws,
            ground,
            start,
            finish: helper_pose(&stm, "spline_finishline"),
            race_line,
            sky: [0.62, 0.74, 0.9],
            track_xml: std::fs::read_to_string(root.join("tracks").join(theme).join(run).join("track.xml")).unwrap_or_default(),
            stm,
        })
    }
}

/// Test aid (NOT original): pure-pursuit driver on the race line plus lap statistics. It feeds an analogue steering value like an AI
/// car does (`CCar::SetSteering`) and reports how far along the race line the kart got and whether it finished or left the road.
pub struct LineFollower {
    /// Index of the race-line segment the kart is on (monotonic search window).
    pub index: usize,
    /// Arc length along the race line at the nearest point.
    pub distance: f32,
    pub total: f32,
    pub time: f32,
    /// Seconds without ground under the kart / farthest lateral distance from the line.
    off_ground: f32,
    pub max_lateral: f32,
    pub verbose: bool,
    /// Lateral acceleration the speed control allows (m/s^2).
    pub lat_accel: f32,
    air_time: f32,
    max_air: f32,
    max_below: f32,
    max_surface_error: f32,
    max_index_jump: usize,
    pub finished: Option<f32>,
    pub left_road: Option<(f32, f32)>,
    report: f32,
    cumulative: Vec<f32>,
}

impl LineFollower {
    pub fn new(line: &[Vec3]) -> LineFollower {
        let mut cumulative = vec![0.0];
        for w in line.windows(2) {
            cumulative.push(cumulative.last().unwrap() + w[0].distance(w[1]));
        }
        LineFollower { index: 0, distance: 0.0, total: *cumulative.last().unwrap_or(&0.0), time: 0.0, off_ground: 0.0, max_lateral: 0.0, verbose: false, air_time: 0.0, max_air: 0.0, max_below: f32::MIN, max_surface_error: 0.0, max_index_jump: 0, lat_accel: std::env::var("ABG_LATACC").ok().and_then(|v| v.parse().ok()).unwrap_or(9.0), finished: None, left_road: None, report: 0.0, cumulative }
    }

    /// Returns (analogue steer in `CarInput` sign, brake 0..1, spline direction at the kart).
    pub fn update(&mut self, line: &[Vec3], ground: &dyn Ground, dt: f32, pos: Vec3, fwd_flat: Vec3, speed: f32) -> (f32, f32, Vec3) {
        self.time += dt;
        let n = line.len();
        // nearest segment within a window ahead of / behind the last one
        // first call: nearest segment of the whole line (the start line is not always at its first point); afterwards a window around the last one
        let first = self.time <= dt * 1.5;
        let lo = if first { 0 } else { self.index.saturating_sub(4) };
        let hi = if first { n - 2 } else { (self.index + 24).min(n - 2) };
        let seg_dist = |i: usize| {
            let (a, b) = (line[i], line[i + 1]);
            let ab = b - a;
            let t = ((pos - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (a + ab * t).distance(pos)
        };
        let previous_index = self.index;
        self.index = (lo..=hi).min_by(|&a, &b| seg_dist(a).total_cmp(&seg_dist(b))).unwrap_or(self.index);
        let index_jump = self.index.abs_diff(previous_index);
        self.max_index_jump = self.max_index_jump.max(index_jump);
        let (a, b) = (line[self.index], line[self.index + 1]);
        let ab = b - a;
        let t = ((pos - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let on_line = a + ab * t;
        self.distance = self.distance.max(self.cumulative[self.index] + ab.length() * t);
        let lateral = (on_line - pos).with_y(0.0).length();
        let below_line = on_line.y - pos.y;
        self.max_lateral = self.max_lateral.max(lateral);
        let ground_h = ground.height_normal(pos).0;
        let has_ground = ground_h > -1.0e3;
        let grounded = has_ground && pos.y - ground_h < 3.0;
        // the surface the race line was laid over (first surface below the line), compared with what the kart stands on: catches landing on a lower road after falling off a bridge
        let line_surface = ground.height_normal(on_line - Vec3::Y * 2.5).0;
        let wrong_surface = grounded && line_surface > -1.0e3 && (ground_h - line_surface).abs() > 5.0 + 0.8 * lateral;
        self.air_time = if grounded { 0.0 } else { self.air_time + dt };
        self.max_air = self.max_air.max(self.air_time);
        if grounded {
            // landing / driving: how far below the race line the kart sits (the line floats 0-5 m above the surface)
            self.max_below = self.max_below.max(below_line);
            if line_surface > -1.0e3 {
                self.max_surface_error = self.max_surface_error.max((ground_h - line_surface).abs() - 0.8 * lateral);
            }
        }
        self.off_ground = if has_ground { 0.0 } else { self.off_ground + dt };
        if self.finished.is_none() && self.left_road.is_none() {
            if self.distance >= self.total - 6.0 {
                self.finished = Some(self.time);
                if self.verbose { eprintln!("AUTOPILOT: FINISHED t={:.1}s distance {:.0}/{:.0} m (max lateral {:.1} m, longest airborne {:.1} s, deepest grounded below line {:.1} m, worst surface error {:.1} m, largest index jump {})", self.time, self.distance, self.total, self.max_lateral, self.max_air, self.max_below, self.max_surface_error, self.max_index_jump); }
            } else if wrong_surface || (index_jump > 8 && !first) || (self.off_ground > 1.0 && below_line > 45.0) || self.off_ground > 6.0 || lateral > 40.0 {
                self.left_road = Some((self.time, self.distance));
                if self.verbose { eprintln!("AUTOPILOT: LEFT THE ROAD t={:.1}s at {:.0}/{:.0} m (segment {}, lateral {:.1} m, below line {:.1} m, grounded {}, ground under kart {:.1}, surface under line {:.1}, index jump {}, pos {:?})", self.time, self.distance, self.total, self.index, lateral, below_line, grounded, ground_h, line_surface, index_jump, pos); }
            }
        }
        self.report += dt;
        if self.report > 10.0 {
            self.report = 0.0;
            if self.verbose { eprintln!("AUTOPILOT: t={:.0}s distance {:.0}/{:.0} m speed {:.1} lateral {:.1}", self.time, self.distance, self.total, speed, lateral); }
        }
        // look-ahead point on the line
        let look = (speed * 0.6).clamp(8.0, 30.0);
        let want = self.cumulative[self.index] + ab.length() * t + look;
        let mut j = self.index;
        while j + 1 < n - 1 && self.cumulative[j + 1] < want {
            j += 1;
        }
        let seg = (line[j + 1] - line[j]).length().max(1e-3);
        let target = line[j].lerp(line[j + 1], ((want - self.cumulative[j]) / seg).clamp(0.0, 1.0));
        let target = Self::centre_on_road(ground, target, (line[j + 1] - line[j]).with_y(0.0).normalize_or_zero());
        let to = (target - pos).with_y(0.0).normalize_or_zero();
        let angle = (fwd_flat.x * to.z - fwd_flat.z * to.x).atan2(fwd_flat.dot(to));
        // original steer sign: +1 = right; CarInput uses the opposite sign (see `Drive::update`)
        // pure pursuit: curvature 2 sin(angle) / L -> yaw rate speed * curvature; the ported arcade steering gives yaw = 1.15 * SlidingTargetAngularVel * steer (about 1.6)
        let yaw_rate = speed.max(10.0) * 2.0 * angle.sin() / look;
        let steer = -(yaw_rate / 1.6).clamp(-1.0, 1.0);
        // speed control (test aid, NOT original): brake so that speed^2 / radius of the bend ahead stays below a lateral acceleration the tyres hold
        let at = |d: f32| {
            let want = self.cumulative[self.index] + ab.length() * t + d;
            let mut j = self.index;
            while j + 1 < n - 1 && self.cumulative[j + 1] < want {
                j += 1;
            }
            let seg = (line[j + 1] - line[j]).length().max(1e-3);
            (line[j].lerp(line[j + 1], ((want - self.cumulative[j]) / seg).clamp(0.0, 1.0)), (line[j + 1] - line[j]).with_y(0.0).normalize_or_zero())
        };
        let mut v_des = 60.0f32;
        for k in 1..=6 {
            let (d0, d1) = (at(k as f32 * 12.0).1, at(k as f32 * 12.0 + 12.0).1);
            let turn = d0.dot(d1).clamp(-1.0, 1.0).acos().max(1e-3);
            let radius = 12.0 / turn;
            v_des = v_des.min(((self.lat_accel * radius).sqrt()).max(14.0) + 0.5 * k as f32);
        }
        let brake = ((speed - v_des) / 6.0).clamp(0.0, 1.0);
        if self.verbose && std::env::var_os("ABG_AP_TRACE").is_some() && (self.time * 4.0).fract() < dt * 4.0 {
            eprintln!("AP t={:.2} idx={} dist={:.0} lat={:.1} angle={:.2} steer={:.2} brake={:.2} vdes={:.1} v={:.1} target={:?} pos={:?}", self.time, self.index, self.distance, lateral, angle, steer, brake, v_des, speed, target, pos);
        }
        (steer, brake, ab.normalize_or_zero())
    }
}

impl LineFollower {
    /// Test aid: the race line is not always centred on the drivable surface (rope bridges), so aim at the middle of the contiguous run of
    /// ground found sideways from `target` (within 3.5 m of its height), limited to +-8 m.
    fn centre_on_road(ground: &dyn Ground, target: Vec3, dir: Vec3) -> Vec3 {
        let right = Vec3::new(dir.z, 0.0, -dir.x);
        let solid = |o: f32| {
            let p = target + right * o;
            ground.ray(p + Vec3::Y * 4.0, Vec3::new(0.0, -9.0, 0.0)).map_or(false, |h| (h.point.y - target.y).abs() < 3.5)
        };
        // nearest solid offset to the line, then grow the run both ways
        let Some(start) = (0..=24).map(|k| if k % 2 == 0 { (k / 2) as f32 } else { -((k + 1) / 2) as f32 }).find(|&o| solid(o)) else { return target };
        let (mut lo, mut hi) = (start, start);
        while lo > -12.0 && solid(lo - 1.0) {
            lo -= 1.0;
        }
        while hi < 12.0 && solid(hi + 1.0) {
            hi += 1.0;
        }
        // keep a margin of 1.5 m from the edges where the surface is wide enough
        if hi - lo > 14.0 {
            return target; // wide road: stay on the line
        }
        let (a, b) = if hi - lo > 6.0 { (lo + 1.5, hi - 1.5) } else { (lo, hi) };
        target + right * ((a + b) * 0.5).clamp(-8.0, 8.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets_dir() -> Option<std::path::PathBuf> {
        if let Some(v) = std::env::var_os("ABG_ASSETS292") {
            return Some(std::path::PathBuf::from(v));
        }
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets292");
        p.is_dir().then_some(p)
    }

    #[test]
    fn dump_chassis_bounds() {
        let Some(root) = assets_dir() else { return };
        for (theme, folder) in [("theme002", "kart_base"), ("theme002", "kart_red_upgrade1"), ("theme002", "kart_black_upgrade1"), ("theme003", "kart_red_upgrade3")] {
            let p = root.join(format!("pak/cars/{theme}/cargeom/{folder}/chassis_l02.xgm"));
            let Ok(d) = std::fs::read(&p) else { continue };
            let x = abgtool::xgm::parse(&d).unwrap();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for m in &x.meshes {
                println!("  mesh {} verts {}", m.name, m.transformed_positions().len());
                if m.name.to_ascii_lowercase().contains("outline") { continue; }
                for p in m.transformed_positions() {
                    for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); }
                }
            }
            println!("{folder}: lo {lo:?} hi {hi:?} nodes {:?}", x.nodes.iter().map(|n| (n.name.clone(), n.position)).collect::<Vec<_>>());
        }
    }
}
