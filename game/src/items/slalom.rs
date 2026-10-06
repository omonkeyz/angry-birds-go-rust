//! Slalom gates, ported from `CGameModeSlalom` (`@00186744`; `AddLeftGate @001855d0`, `AddRightGate @001855e8`,
//! `Update @00185a04`, `GetBonusCoins @00185480`) and `CEventDefinitionManager::AddTrackItem @001005b0` (gate expansion).
//!
//! A gate = two post smackables (`slalom_post_{blue,red}_{left,right}`, 0.5 * separation either side of the placed point). The first time the
//! player's progress along its spline passes the gate's position, the gate is evaluated ONCE:
//!  * pass  - the car is between the posts (3D projection test on segment A-B): `GatePassed`;
//!  * miss  - time penalty (`penalty` seconds off the race timer) and a wall of `cols x rows` TNT crates (smackable 125 = `smck_tnt_box_3m`) is
//!    spawned ahead of the car (`GateMissed`). Hitting a post itself has no slalom effect.
use glam::Vec3;

/// Constants of `CGameModeSlalom::Update` (read from the ARM listing / binary).
pub const TNT_LOOKAHEAD_SECONDS: f32 = 1.5;
pub const TNT_RAISE: f32 = 5.0;
pub const GROUND_SNAP_DIST_SQ: f32 = 400.0;
/// Smackable type of the miss wall (`CGameModeSlalom+0x24 = 0x7d`): `smck_TNT_Box_3m`.
pub const MISS_WALL_SMACKABLE: u32 = 125;
pub const MISS_WALL_MODEL: &str = "smck_tnt_box_3m";
/// Voice / feedback timer after a gate result (`this+0x28ac`, 0x3f400000).
pub const FEEDBACK_TIME: f32 = 0.75;
/// maximum gates of the game mode object (32 slots)
pub const MAX_GATES: usize = 32;
/// `GetBonusCoins @00185480`: coins per whole second left on the clock at the finish.
pub const BONUS_COINS_PER_SECOND: i32 = 10;

#[derive(Debug, Clone)]
pub struct Gate {
    pub index: usize,
    pub penalty: f32,
    pub post_a: Vec3,
    pub post_b: Vec3,
    pub cols: i32,
    pub rows: i32,
    pub col_spacing: f32,
    pub row_spacing: f32,
    pub passed: bool,
    /// the gate's position along the racing spline (fractional point index), `closest_pos(midpoint)`
    pub spline_pos: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GateResult {
    Passed,
    Missed { penalty: f32 },
}

impl Gate {
    pub fn midpoint(&self) -> Vec3 {
        self.post_a + (self.post_b - self.post_a) * 0.5
    }

    /// The pass test of `Update`: the car must be on the B side of A and on the A side of B
    /// (`dot(dir(A,B), dir(car,A)) < 0` and `dot(dir(B,A), dir(car,B)) < 0`, all directions normalised).
    pub fn car_between_posts(&self, car: Vec3) -> bool {
        let dir = |p: Vec3, q: Vec3| (p - q).normalize_or_zero();
        dir(self.post_a, self.post_b).dot(dir(car, self.post_a)) < 0.0 && dir(self.post_b, self.post_a).dot(dir(car, self.post_b)) < 0.0
    }

    /// One evaluation: `car_progress` and the gate's `spline_pos` are positions along the same spline; `time_left` is the race clock.
    /// Returns the result the first time the car passes the gate (and `time_left > 0`), never again.
    pub fn evaluate(&mut self, car: Vec3, car_progress: f32, time_left: f32) -> Option<GateResult> {
        if self.passed || !(car_progress > self.spline_pos && time_left > 0.0) {
            return None;
        }
        self.passed = true;
        Some(if self.car_between_posts(car) { GateResult::Passed } else { GateResult::Missed { penalty: self.penalty } })
    }
}

/// Position `(col, row)` of the TNT wall that a miss spawns, relative to the wall centre (before ground snapping):
/// `right * (row - rows/2) * row_spacing + forward * (col - cols/2) * col_spacing`.
pub fn wall_offsets(g: &Gate) -> Vec<(i32, i32, f32, f32)> {
    let mut v = Vec::new();
    for fi in 0..g.cols.max(0) {
        for li in 0..g.rows.max(0) {
            let lateral = (li as f32 - g.rows as f32 * 0.5) * g.row_spacing;
            let forward = (fi as f32 - g.cols as f32 * 0.5) * g.col_spacing;
            v.push((fi, li, lateral, forward));
        }
    }
    v
}

/// `GetBonusCoins`: `max(trunc(time_left), 0) * 10`.
pub fn bonus_coins(time_left: f32) -> i32 {
    (time_left as i32).max(0) * BONUS_COINS_PER_SECOND
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> Gate {
        Gate {
            index: 0,
            penalty: 5.0,
            post_a: Vec3::new(-4.0, 0.0, 0.0),
            post_b: Vec3::new(4.0, 0.0, 0.0),
            cols: 1,
            rows: 3,
            col_spacing: 15.0,
            row_spacing: 8.0,
            passed: false,
            spline_pos: 10.0,
        }
    }

    #[test]
    fn pass_between_posts_and_miss_outside() {
        let mut g = gate();
        // not yet past the gate along the spline: nothing happens
        assert_eq!(g.evaluate(Vec3::new(0.0, 0.0, 1.0), 9.0, 30.0), None);
        assert_eq!(g.evaluate(Vec3::new(0.0, 0.0, 1.0), 10.5, 30.0), Some(GateResult::Passed));
        // evaluated once only
        assert_eq!(g.evaluate(Vec3::new(40.0, 0.0, 1.0), 11.0, 30.0), None);
        let mut m = gate();
        assert_eq!(m.evaluate(Vec3::new(9.0, 0.0, 1.0), 10.5, 30.0), Some(GateResult::Missed { penalty: 5.0 }));
        // no time left: the gate is ignored
        let mut z = gate();
        assert_eq!(z.evaluate(Vec3::ZERO, 11.0, 0.0), None);
    }

    #[test]
    fn miss_wall_layout_and_bonus() {
        let g = gate();
        let w = wall_offsets(&g);
        assert_eq!(w.len(), 3);
        // rows 3, spacing 8: lateral offsets -12, -4, 4 (li - 1.5) * 8
        assert_eq!(w.iter().map(|x| x.2).collect::<Vec<_>>(), vec![-12.0, -4.0, 4.0]);
        assert_eq!(w[0].3, -7.5);
        assert_eq!(bonus_coins(12.9), 120);
        assert_eq!(bonus_coins(-3.0), 0);
    }
}
