//! `CSpline` of the original (ctor `CSpline::CSpline(int,int,int) @0018cbb0`, `GetPosition @0018d4c0`, `GetLeftWidth @0018dfb0`,
//! `GetRightWidth @0018e008`), built from the track's pvs splines (position + 7 extra floats per point).
//! Every track spline except `bird_*` / `DragSpline*` becomes a `CSpline` (the loader that calls the ctor is at decompile line 90860).
use abgtool::stm::{Spline as StmSpline, Stm};
use glam::Vec3;

/// One spline point as `CSpline` stores it (0x3c bytes in the original).
#[derive(Debug, Clone, Copy, Default)]
pub struct SplinePoint {
    pub pos: Vec3,
    /// extra[0..3]: the surface normal at the point
    pub normal: Vec3,
    /// `+0x10`: `normal x tangent` (not normalised), the "right" vector the lateral offset moves along
    pub right: Vec3,
    /// `+0x1c`: unit direction towards the next point (the last point repeats the previous segment)
    pub tangent: Vec3,
    /// `+0x28`: distance to the next point
    pub seg_len: f32,
    /// `+0x34`: distance from the start
    pub dist: f32,
    /// extra[4], extra[5]: track half widths left / right (`CSpline` reads `*(extra + 0x10)` / `+0x14`)
    pub left_w: f32,
    pub right_w: f32,
}

#[derive(Debug, Clone, Default)]
pub struct CSpline {
    pub name: String,
    pub points: Vec<SplinePoint>,
    /// `this+0x1c`: total length (sum of the first N-1 segments)
    pub length: f32,
}

impl CSpline {
    pub fn from_stm(s: &StmSpline) -> CSpline {
        let n = s.points.len();
        let mut points: Vec<SplinePoint> = (0..n)
            .map(|i| {
                let e = s.extra.get(i).copied().unwrap_or([0.0, 1.0, 0.0, 0.0, 20.0, 20.0, 0.0]);
                SplinePoint { pos: Vec3::from(s.points[i]), normal: Vec3::new(e[0], e[1], e[2]), left_w: e[4], right_w: e[5], ..Default::default() }
            })
            .collect();
        let mut dist = 0.0f32;
        // segments 0..N-2 as in the loop of the ctor; the last point copies the previous segment's data (the code after the loop).
        for i in 0..n.saturating_sub(1) {
            let d = points[i + 1].pos - points[i].pos;
            let len = d.length();
            let inv = 1.0 / len;
            let t = d * inv;
            points[i].seg_len = len;
            points[i].dist = dist;
            points[i].tangent = t;
            points[i].right = points[i].normal.cross(t);
            dist += len;
        }
        if n >= 2 {
            let prev = points[n - 2];
            let last = &mut points[n - 1];
            last.seg_len = prev.seg_len;
            last.dist = dist;
            last.tangent = prev.tangent;
            last.right = last.normal.cross(prev.tangent);
        }
        CSpline { name: s.name.clone(), points, length: dist }
    }

    pub fn count(&self) -> usize {
        self.points.len()
    }

    /// `if i + 1 < N { i + 1 } else { 0 }` (GetPosition / GetLeftWidth / GetRightWidth wrap to point 0)
    fn next_index(&self, i: usize) -> usize {
        if i + 1 < self.points.len() {
            i + 1
        } else {
            0
        }
    }

    /// `CSpline::GetPosition(float)`: linear interpolation between point `floor(t)` and the next one.
    pub fn position(&self, t: f32) -> Vec3 {
        let i = self.clamp_index(t);
        let f = t - i as f32;
        let a = self.points[i].pos;
        let b = self.points[self.next_index(i)].pos;
        a + (b - a) * f
    }

    /// The integer part the original uses for table reads (`(int)t`); guarded to the valid range (the original reads out of bounds).
    pub fn clamp_index(&self, t: f32) -> usize {
        (t as i64).clamp(0, self.points.len() as i64 - 1) as usize
    }

    pub fn left_width(&self, t: f32) -> f32 {
        let i = self.clamp_index(t);
        self.points[i].left_w.min(self.points[self.next_index(i)].left_w)
    }

    pub fn right_width(&self, t: f32) -> f32 {
        let i = self.clamp_index(t);
        self.points[i].right_w.min(self.points[self.next_index(i)].right_w)
    }
}

/// All `CSpline`s of a track in the order of the pvs spline table (the item loader looks them up by name, case-insensitive).
pub fn splines_of(stm: &Stm) -> Vec<CSpline> {
    stm.pvs
        .splines
        .iter()
        .filter(|s| {
            let n = s.name.to_ascii_lowercase();
            !(n.starts_with("bird_") || n.starts_with("dragspline"))
        })
        .map(CSpline::from_stm)
        .collect()
}
