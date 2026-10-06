//! Procedural race tracks.
//!
//! The original track geometry was downloaded content (`models.pak` / `textures.pak` per theme) that is not in the APK,
//! so circuits are generated here from a closed centre line: a polar curve with a few harmonics, resampled at an even
//! spacing, with rolling hills. Everything else (road, kerbs, terrain, scenery, start arch) is built from that line.
use crate::models;
use crate::render3d::{MeshData, Vertex3};
use glam::{Mat4, Vec3};

pub const SAMPLE_SPACING: f32 = 2.0;

pub struct TrackDef {
    pub name: &'static str,
    pub music: &'static str,
    pub radius: f32,
    /// (harmonic, amplitude as a fraction of the radius, phase)
    pub harmonics: &'static [(f32, f32, f32)],
    pub hill_height: f32,
    pub width: f32,
    pub laps: u32,
    pub grass: [f32; 3],
    pub road: [f32; 3],
    pub sky: [f32; 3],
    pub seed: u32,
}

pub const TRACKS: [TrackDef; 3] = [
    TrackDef {
        name: "COBALT PLATEAU",
        music: "audio/music/mp3s/aby_music_cobalt.mp3",
        radius: 165.0,
        harmonics: &[(2.0, 0.24, 0.3), (3.0, 0.13, 1.1), (5.0, 0.045, 2.0)],
        hill_height: 2.6,
        width: 14.0,
        laps: 3,
        grass: [0.30, 0.55, 0.22],
        road: [0.23, 0.23, 0.26],
        sky: [0.55, 0.75, 0.95],
        seed: 7,
    },
    TrackDef {
        name: "ROCKY ROAD",
        music: "audio/music/mp3s/aby_music_rocky_road.mp3",
        radius: 190.0,
        harmonics: &[(2.0, 0.20, 2.2), (3.0, 0.17, 0.4), (4.0, 0.08, 1.3), (7.0, 0.03, 0.6)],
        hill_height: 4.5,
        width: 13.0,
        laps: 3,
        grass: [0.52, 0.42, 0.28],
        road: [0.30, 0.27, 0.25],
        sky: [0.88, 0.72, 0.58],
        seed: 21,
    },
    TrackDef {
        name: "AERIE PEAKS",
        music: "audio/music/mp3s/aby_music_aerie_peaks.mp3",
        radius: 150.0,
        harmonics: &[(2.0, 0.22, 1.7), (3.0, 0.10, 2.6), (5.0, 0.07, 0.2), (6.0, 0.03, 1.0)],
        hill_height: 3.4,
        width: 14.0,
        laps: 3,
        grass: [0.78, 0.84, 0.88],
        road: [0.26, 0.28, 0.32],
        sky: [0.70, 0.82, 0.95],
        seed: 63,
    },
];

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Road surface point (centre line).
    pub pos: Vec3,
    /// Horizontal unit direction of travel.
    pub tangent: Vec3,
    /// Horizontal unit vector to the right of the direction of travel.
    pub right: Vec3,
    /// Arc length from the start line.
    pub s: f32,
    /// Signed horizontal curvature (positive = turning right), 1/m.
    pub curvature: f32,
    /// Height change per metre travelled.
    pub slope: f32,
}

pub struct Track {
    pub def: &'static TrackDef,
    pub samples: Vec<Sample>,
    pub length: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub index: usize,
    /// Signed distance from the centre line, positive to the right.
    pub lateral: f32,
    pub on_road: bool,
}

/// Small deterministic generator for scenery placement.
pub struct Lcg(pub u32);

impl Lcg {
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.0 >> 8) & 0xFFFF) as f32 / 65535.0
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Track {
    pub fn generate(def: &'static TrackDef) -> Track {
        // 1. dense polar curve
        let raw_n = 4000;
        let mut raw: Vec<Vec3> = Vec::with_capacity(raw_n);
        for i in 0..raw_n {
            let th = i as f32 / raw_n as f32 * std::f32::consts::TAU;
            let mut r = 1.0;
            for &(k, a, ph) in def.harmonics {
                r += a * (k * th + ph).cos();
            }
            let r = r * def.radius;
            // start heading along +X at the origin: shift so sample 0 is on a straight-ish part
            raw.push(Vec3::new(r * th.cos(), 0.0, r * th.sin()));
        }
        // 2. resample at an even spacing by arc length
        let mut cumulative = vec![0.0f32];
        for i in 1..=raw_n {
            let d = raw[i % raw_n].distance(raw[i - 1]);
            cumulative.push(cumulative[i - 1] + d);
        }
        let total = cumulative[raw_n];
        let count = (total / SAMPLE_SPACING).round() as usize;
        let spacing = total / count as f32;
        let mut points = Vec::with_capacity(count);
        let mut j = 0usize;
        for k in 0..count {
            let target = k as f32 * spacing;
            while cumulative[j + 1] < target {
                j += 1;
            }
            let t = (target - cumulative[j]) / (cumulative[j + 1] - cumulative[j]).max(1e-6);
            points.push(raw[j].lerp(raw[(j + 1) % raw_n], t));
        }
        // 3. recentre so the start line is at the origin heading along +X
        let origin = points[0];
        let forward = (points[1] - points[0]).normalize();
        let yaw = Mat4::from_rotation_y(forward.z.atan2(forward.x));
        let to_local = yaw.inverse();
        for p in &mut points {
            *p = to_local.transform_point3(*p - origin);
        }
        // 4. hills, tangents, curvature
        let mut seed_rng = Lcg(def.seed);
        let phases = [seed_rng.next_f32() * 6.28, seed_rng.next_f32() * 6.28, seed_rng.next_f32() * 6.28];
        let n = points.len();
        for (i, p) in points.iter_mut().enumerate() {
            let u = i as f32 / n as f32 * std::f32::consts::TAU;
            // zero height at the start line so the grid and the arch sit on flat ground
            let hill = (u + phases[0]).sin() * 0.55 + (2.0 * u + phases[1]).sin() * 0.3 + (3.0 * u + phases[2]).sin() * 0.15;
            let base = (phases[0]).sin() * 0.55 + (phases[1]).sin() * 0.3 + (phases[2]).sin() * 0.15;
            let fade = 1.0 - smoothstep(0.0, 0.06, (u.min(std::f32::consts::TAU - u)) / std::f32::consts::TAU);
            p.y = (hill - base * 1.0) * def.hill_height * (1.0 - fade * 0.0);
            p.y *= smoothstep(0.0, 0.05, u.min(std::f32::consts::TAU - u) / std::f32::consts::TAU);
        }
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let prev = points[(i + n - 1) % n];
            let next = points[(i + 1) % n];
            let flat = Vec3::new(next.x - prev.x, 0.0, next.z - prev.z);
            let tangent = flat.normalize_or_zero();
            let right = Vec3::new(-tangent.z, 0.0, tangent.x);
            let t_prev = Vec3::new(points[i].x - prev.x, 0.0, points[i].z - prev.z).normalize_or_zero();
            let t_next = Vec3::new(next.x - points[i].x, 0.0, next.z - points[i].z).normalize_or_zero();
            let turn = t_prev.cross(t_next).y; // > 0: counter-clockwise seen from +Y
            let curvature = -turn.clamp(-1.0, 1.0).asin() / spacing; // positive = right turn
            let slope = (next.y - prev.y) / (2.0 * spacing);
            samples.push(Sample { pos: points[i], tangent, right, s: i as f32 * spacing, curvature, slope });
        }
        // smooth curvature a little so the AI speed profile is not jittery
        let raw_curv: Vec<f32> = samples.iter().map(|s| s.curvature).collect();
        for i in 0..n {
            let mut acc = 0.0;
            for d in -3i32..=3 {
                acc += raw_curv[(i as i32 + d).rem_euclid(n as i32) as usize];
            }
            samples[i].curvature = acc / 7.0;
        }
        Track { def, samples, length: n as f32 * spacing, width: def.width }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Index of the sample nearest to `p` (horizontal distance), searching +-`window` around `hint` when given.
    pub fn nearest(&self, p: Vec3, hint: Option<usize>, window: usize) -> usize {
        let n = self.len();
        let dist2 = |i: usize| {
            let s = self.samples[i].pos;
            (s.x - p.x) * (s.x - p.x) + (s.z - p.z) * (s.z - p.z)
        };
        match hint {
            Some(h) => {
                let mut best = h;
                let mut best_d = dist2(h);
                for d in 1..=window as i32 {
                    for sign in [-1i32, 1] {
                        let i = (h as i32 + sign * d).rem_euclid(n as i32) as usize;
                        let dd = dist2(i);
                        if dd < best_d {
                            best_d = dd;
                            best = i;
                        }
                    }
                }
                best
            }
            None => (0..n).min_by(|&a, &b| dist2(a).partial_cmp(&dist2(b)).unwrap()).unwrap_or(0),
        }
    }

    pub fn surface(&self, p: Vec3, hint: Option<usize>) -> Surface {
        let index = self.nearest(p, hint, 40);
        let s = &self.samples[index];
        let lateral = (p - s.pos).dot(s.right);
        Surface { index, lateral, on_road: lateral.abs() <= self.width * 0.5 + 1.2 }
    }

    /// Road height at the centre-line sample, extrapolated along the slope for points between samples.
    pub fn road_height(&self, p: Vec3, index: usize) -> f32 {
        let s = &self.samples[index];
        s.pos.y + (p - s.pos).dot(s.tangent) * s.slope
    }

    /// Terrain height at (x, z): follows the road near it and relaxes to flat ground away from it.
    pub fn terrain_height(&self, x: f32, z: f32, hint: Option<usize>) -> f32 {
        let p = Vec3::new(x, 0.0, z);
        let index = self.nearest(p, hint, 60);
        let s = &self.samples[index];
        let dist = ((p.x - s.pos.x).powi(2) + (p.z - s.pos.z).powi(2)).sqrt();
        let road_edge = self.width * 0.5 + 1.4;
        let t = smoothstep(road_edge, road_edge + 40.0, dist);
        let far = -0.4 + 1.2 * ((x * 0.011).sin() * (z * 0.013).cos()) + 2.0 * smoothstep(240.0, 520.0, dist);
        s.pos.y * (1.0 - t) + far * t - smoothstep(0.0, road_edge + 2.0, dist) * 0.0
    }

    // ------------------------------------------------------------------ meshes

    /// Road surface, lane markings, kerbs and the checkered start line.
    pub fn road_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
        let n = self.len();
        let half = self.width * 0.5;
        let up = [0.0, 1.0, 0.0];
        let quad = |m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: [f32; 4]| {
            let base = m.vertices.len() as u32;
            for p in [a, b, c, d] {
                m.vertices.push(Vertex3 { pos: p.to_array(), normal: up, color });
            }
            m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        };
        let at = |i: usize, lateral: f32, lift: f32| {
            let s = &self.samples[i % n];
            s.pos + s.right * lateral + Vec3::Y * lift
        };
        let road = self.def.road;
        for i in 0..n {
            let j = i + 1;
            let shade = if (i / 5) % 2 == 0 { 1.0 } else { 0.93 };
            let c = [road[0] * shade, road[1] * shade, road[2] * shade, 1.0];
            quad(&mut m, at(i, -half, 0.0), at(i, half, 0.0), at(j, half, 0.0), at(j, -half, 0.0), c);
            // solid edge lines
            let white = [0.92, 0.92, 0.9, 1.0];
            for side in [-1.0f32, 1.0] {
                let (a, b) = (side * (half - 0.9), side * (half - 0.55));
                quad(&mut m, at(i, a, 0.03), at(i, b, 0.03), at(j, b, 0.03), at(j, a, 0.03), white);
            }
            // dashed centre line
            if i % 4 < 2 {
                quad(&mut m, at(i, -0.12, 0.03), at(i, 0.12, 0.03), at(j, 0.12, 0.03), at(j, -0.12, 0.03), [0.95, 0.85, 0.3, 1.0]);
            }
            // kerbs
            let kerb = if (i / 2) % 2 == 0 { [0.85, 0.12, 0.1, 1.0] } else { [0.93, 0.93, 0.93, 1.0] };
            for side in [-1.0f32, 1.0] {
                let (a, b) = (side * half, side * (half + 1.3));
                quad(&mut m, at(i, a, 0.05), at(i, b, 0.05), at(j, b, 0.05), at(j, a, 0.05), kerb);
            }
        }
        // checkered start line over samples 0..2
        let cells = 14usize;
        for row in 0..2usize {
            for c in 0..cells {
                let (a, b) = (-half + self.width * c as f32 / cells as f32, -half + self.width * (c + 1) as f32 / cells as f32);
                let color = if (c + row) % 2 == 0 { [0.96, 0.96, 0.96, 1.0] } else { [0.05, 0.05, 0.05, 1.0] };
                quad(&mut m, at(row, a, 0.06), at(row, b, 0.06), at(row + 1, b, 0.06), at(row + 1, a, 0.06), color);
            }
        }
        m
    }

    /// Terrain grid (grass, with a dirt shoulder next to the road) around the whole circuit.
    pub fn terrain_mesh(&self) -> MeshData {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for s in &self.samples {
            min = min.min(s.pos);
            max = max.max(s.pos);
        }
        let margin = 330.0;
        let (x0, z0) = (min.x - margin, min.z - margin);
        let (x1, z1) = (max.x + margin, max.z + margin);
        let cell = 14.0;
        let nx = ((x1 - x0) / cell).ceil() as usize + 1;
        let nz = ((z1 - z0) / cell).ceil() as usize + 1;
        let g = self.def.grass;
        let mut m = MeshData::default();
        let mut heights = vec![0.0f32; nx * nz];
        let mut colors = vec![[0.0f32; 4]; nx * nz];
        let mut hint: Option<usize> = None;
        for iz in 0..nz {
            for ix in 0..nx {
                let (x, z) = (x0 + ix as f32 * cell, z0 + iz as f32 * cell);
                let p = Vec3::new(x, 0.0, z);
                let idx = self.nearest(p, hint, 50);
                let idx = if hint.is_none() || ix == 0 { self.nearest(p, None, 0) } else { idx };
                hint = Some(idx);
                let s = &self.samples[idx];
                let dist = ((x - s.pos.x).powi(2) + (z - s.pos.z).powi(2)).sqrt();
                heights[iz * nx + ix] = self.terrain_height(x, z, Some(idx)) - 0.06;
                let shoulder = 1.0 - smoothstep(self.width * 0.5 + 1.5, self.width * 0.5 + 7.0, dist);
                let noise = 0.88 + 0.12 * ((x * 0.07).sin() * (z * 0.09).cos());
                let dirt = [0.45, 0.38, 0.27];
                let mix = |a: f32, b: f32| a * (1.0 - shoulder) + b * shoulder;
                colors[iz * nx + ix] = [mix(g[0] * noise, dirt[0]), mix(g[1] * noise, dirt[1]), mix(g[2] * noise, dirt[2]), 1.0];
            }
        }
        for iz in 0..nz {
            for ix in 0..nx {
                let i = iz * nx + ix;
                let h = |dx: i32, dz: i32| {
                    let (jx, jz) = ((ix as i32 + dx).clamp(0, nx as i32 - 1) as usize, (iz as i32 + dz).clamp(0, nz as i32 - 1) as usize);
                    heights[jz * nx + jx]
                };
                let normal = Vec3::new(h(-1, 0) - h(1, 0), 2.0 * cell, h(0, -1) - h(0, 1)).normalize();
                m.vertices.push(Vertex3 { pos: [x0 + ix as f32 * cell, heights[i], z0 + iz as f32 * cell], normal: normal.to_array(), color: colors[i] });
            }
        }
        for iz in 0..nz - 1 {
            for ix in 0..nx - 1 {
                let a = (iz * nx + ix) as u32;
                let (b, c, d) = (a + 1, a + nx as u32 + 1, a + nx as u32);
                m.indices.extend([a, d, c, a, c, b]);
            }
        }
        m
    }

    /// Pine forest outside the road plus a ring of far mountains, merged into one mesh.
    pub fn scenery_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
        let mut rng = Lcg(self.def.seed.wrapping_mul(2654435761));
        let pine_small = models::pine(1.0);
        let n = self.len();
        let mut placed = 0;
        let mut tries = 0;
        while placed < 520 && tries < 20000 {
            tries += 1;
            let base = &self.samples[(rng.next_f32() * n as f32) as usize % n];
            let side = if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
            let lateral = side * (self.width * 0.5 + 9.0 + rng.next_f32() * 70.0);
            let p = base.pos + base.right * lateral;
            let idx = self.nearest(p, None, 0);
            let dist = ((p.x - self.samples[idx].pos.x).powi(2) + (p.z - self.samples[idx].pos.z).powi(2)).sqrt();
            if dist < self.width * 0.5 + 8.0 {
                continue;
            }
            let y = self.terrain_height(p.x, p.z, Some(idx)) - 0.1;
            let scale = 0.9 + rng.next_f32() * 0.9;
            let transform = Mat4::from_translation(Vec3::new(p.x, y, p.z)) * Mat4::from_rotation_y(rng.next_f32() * 6.28) * Mat4::from_scale(Vec3::splat(scale));
            m.append(&pine_small, transform);
            placed += 1;
        }
        // far mountains
        for k in 0..18 {
            let a = k as f32 / 18.0 * std::f32::consts::TAU + rng.next_f32() * 0.2;
            let r = 760.0 + rng.next_f32() * 160.0;
            let h = 160.0 + rng.next_f32() * 150.0;
            let w = 150.0 + rng.next_f32() * 90.0;
            let tone = 0.50 + rng.next_f32() * 0.14;
            let snow = self.def.grass[0] > 0.7;
            let color = if snow { [0.82, 0.86, 0.92, 1.0] } else { [tone * 0.62, tone * 0.72, tone * 0.95, 1.0] };
            m.append(&models::cone(w, h, 10, color), Mat4::from_translation(Vec3::new(r * a.cos() + 40.0, -8.0, r * a.sin())));
        }
        m
    }

    /// Start / finish arch over the road at sample 0.
    pub fn arch_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
        let s = &self.samples[0];
        let half = self.width * 0.5 + 2.2;
        let yaw = Mat4::from_rotation_y((-s.tangent.z).atan2(s.tangent.x));
        let place = |lat: f32, y: f32| Mat4::from_translation(s.pos + Vec3::new(0.0, y, 0.0)) * yaw * Mat4::from_translation(Vec3::new(0.0, 0.0, lat));
        let post = models::cuboid(Vec3::new(0.9, 8.0, 0.9), [0.85, 0.15, 0.12, 1.0]);
        m.append(&post, place(-half, 4.0));
        m.append(&post, place(half, 4.0));
        m.append(&models::cuboid(Vec3::new(1.2, 2.2, self.width + 5.5), [0.95, 0.95, 0.95, 1.0]), place(0.0, 8.0));
        for k in 0..12 {
            let c = if k % 2 == 0 { [0.05, 0.05, 0.05, 1.0] } else { [0.95, 0.95, 0.95, 1.0] };
            let lat = -half + 0.6 + k as f32 * (2.0 * half - 1.2) / 11.0;
            m.append(&models::cuboid(Vec3::new(1.25, 1.0, 1.1), c), place(lat, 8.0));
        }
        m
    }
}
