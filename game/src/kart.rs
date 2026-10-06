//! Kart driving model, parameterised by the game's own `CarSpec` data (mass, engine torque curve, gears, tyres, brakes).
//!
//! Planar rigid body with a bicycle-style tyre model: each axle makes a lateral force from its slip angle, limited by
//! grip x load; the engine drives through an automatic gearbox using the original torque table and gear ratios.
//! Conventions: yaw 0 faces +Z; increasing yaw turns left; `left = (cos yaw, -sin yaw)`.
use glam::Vec3;
use std::collections::HashMap;

const G: f32 = 9.81;
const LB_FT_TO_NM: f32 = 1.3558;

/// Attribute map of the first element named `tag` in `xml`.
fn attributes(xml: &str, tag: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let open = format!("<{tag} ");
    let Some(start) = xml.find(&open) else { return map };
    let rest = &xml[start + open.len()..];
    let end = rest.find('>').unwrap_or(rest.len());
    let mut body = &rest[..end];
    while let Some(eq) = body.find("=\"") {
        let name = body[..eq].trim().trim_end_matches('/').to_string();
        let value_start = eq + 2;
        let Some(quote) = body[value_start..].find('"') else { break };
        map.insert(name, body[value_start..value_start + quote].to_string());
        body = &body[value_start + quote + 1..];
    }
    map
}

#[derive(Clone, Debug)]
pub struct KartSpec {
    pub name: String,
    pub mass: f32,
    pub inertia_scale: f32,
    pub drag: f32,
    pub down_force: f32,
    pub torque_scalar: f32,
    pub gear_ratios: Vec<f32>,
    pub final_drive: f32,
    /// (rpm, N m) before the torque scalar.
    pub torque_curve: Vec<(f32, f32)>,
    pub rev_limit_low: f32,
    pub rev_limit_high: f32,
    pub shift_up_rpm: f32,
    pub shift_down_rpm: f32,
    pub wheel_radius_front: f32,
    pub wheel_radius_rear: f32,
    pub grip: f32,
    pub brake_torque: f32,
    pub max_steer_lock: f32,
    pub soft_steer_scale: f32,
    pub max_speed_steer_scale: f32,
    pub wheelbase_front: f32,
    pub wheelbase_rear: f32,
}

impl KartSpec {
    /// `xml` is a decoded `kart_*.xml` (see `abgtool xox`); the axle offsets come from the chassis model's wheel nodes.
    pub fn from_xml(name: &str, xml: &str, front_z: f32, rear_z: f32) -> Result<KartSpec, String> {
        let car = attributes(xml, "CarSpec");
        if car.is_empty() {
            return Err(format!("{name}: no <CarSpec> element"));
        }
        let num = |key: &str| -> Result<f32, String> {
            car.get(key).and_then(|v| v.parse().ok()).ok_or_else(|| format!("{name}: missing or invalid {key}"))
        };
        let gear_count = num("m_iNumGears")? as usize;
        let gear_ratios = (1..=gear_count).map(|g| num(&format!("m_fGearRatio_Gear{g}"))).collect::<Result<Vec<_>, _>>()?;
        let mut torque_curve = Vec::new();
        for k in 0..=40 {
            let rpm = k as f32 * 500.0;
            if let Ok(t) = num(&format!("m_fTorqueLbFt_{}RPM", rpm as u32)) {
                torque_curve.push((rpm, t * LB_FT_TO_NM));
            }
        }
        let wheel = |tag: &str| attributes(xml, tag);
        let (fl, rl) = (wheel("EWheel_FL"), wheel("EWheel_RL"));
        let wnum = |w: &HashMap<String, String>, key: &str, default: f32| w.get(key).and_then(|v| v.parse().ok()).unwrap_or(default);
        Ok(KartSpec {
            name: name.to_string(),
            mass: num("m_fMass")?,
            inertia_scale: num("m_fInertia")?,
            drag: num("m_fDrag")?,
            down_force: num("m_fDownForce").unwrap_or(1.0),
            torque_scalar: num("m_fTorqueScalar")?,
            gear_ratios,
            final_drive: num("m_fFinalDriveRatio")?,
            torque_curve,
            rev_limit_low: num("m_fLowerRevLimit")?,
            rev_limit_high: num("m_fUpperRevLimit")?,
            shift_up_rpm: num("m_fShiftUpRPM")?,
            shift_down_rpm: num("m_fShiftDownRPM")?,
            wheel_radius_front: wnum(&fl, "fWheelRadius", 0.25),
            wheel_radius_rear: wnum(&rl, "fWheelRadius", 0.30),
            grip: wnum(&fl, "fPeakGrip", 0.73),
            brake_torque: wnum(&fl, "fBrakeTorque", 2500.0) * 2.0 + wnum(&rl, "fBrakeTorque", 2400.0) * 2.0,
            max_steer_lock: wnum(&fl, "fMaxSteeringLock", 1.05),
            soft_steer_scale: num("m_fSoftSteeringScale").unwrap_or(0.35),
            max_speed_steer_scale: num("m_fMaxSpeedSteerScale").unwrap_or(50.0),
            wheelbase_front: front_z.abs().max(0.2),
            wheelbase_rear: rear_z.abs().max(0.2),
        })
    }

    fn torque_at(&self, rpm: f32) -> f32 {
        let c = &self.torque_curve;
        if c.is_empty() {
            return 0.0;
        }
        if rpm <= c[0].0 {
            return c[0].1;
        }
        for w in c.windows(2) {
            if rpm <= w[1].0 {
                let t = (rpm - w[0].0) / (w[1].0 - w[0].0);
                return w[0].1 + (w[1].1 - w[0].1) * t;
            }
        }
        c.last().unwrap().1
    }

    fn wheel_radius(&self) -> f32 {
        (self.wheel_radius_front + self.wheel_radius_rear) * 0.5
    }
}

/// Tuning that turns the raw spec into arcade-friendly behaviour.
mod tuning {
    /// Multiplies the data-sheet tyre grip into a friction coefficient.
    pub const GRIP_SCALE: f32 = 1.75;
    pub const CORNERING_STIFFNESS: f32 = 12.0;
    pub const DRAG_SCALE: f32 = 2.0;
    pub const POWER_SCALE: f32 = 1.35;
    pub const BRAKE_SCALE: f32 = 0.30;
    pub const GRASS_GRIP: f32 = 0.55;
    pub const GRASS_DRAG: f32 = 0.45;
    pub const BOOST_FORCE: f32 = 2600.0;
    pub const BOOST_SPEED_BONUS: f32 = 1.35;
    pub const HANDBRAKE_REAR_GRIP: f32 = 0.55;
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// 0..1
    pub throttle: f32,
    /// 0..1
    pub brake: f32,
    /// -1 (left) .. 1 (right)
    pub steer: f32,
    pub handbrake: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub on_road: bool,
    /// Height change per metre along the direction of travel of the track.
    pub slope: f32,
    /// Horizontal unit tangent of the track here (for the slope force).
    pub tangent: Vec3,
}

impl Default for Ground {
    fn default() -> Self {
        Ground { on_road: true, slope: 0.0, tangent: Vec3::Z }
    }
}

#[derive(Clone, Debug)]
pub struct Kart {
    pub pos: Vec3,
    pub yaw: f32,
    /// Horizontal velocity (x, z); `y` is unused.
    pub vel: Vec3,
    pub yaw_rate: f32,
    pub steer_angle: f32,
    pub gear: usize,
    pub rpm: f32,
    pub shift_cooldown: f32,
    pub boost_time: f32,
    pub wheel_spin: f32,
    /// Lateral acceleration of the last step, for body roll.
    pub lateral_accel: f32,
    pub drifting: bool,
    pub on_road: bool,
}

impl Kart {
    pub fn new(pos: Vec3, yaw: f32) -> Kart {
        Kart {
            pos,
            yaw,
            vel: Vec3::ZERO,
            yaw_rate: 0.0,
            steer_angle: 0.0,
            gear: 0,
            rpm: 1000.0,
            shift_cooldown: 0.0,
            boost_time: 0.0,
            wheel_spin: 0.0,
            lateral_accel: 0.0,
            drifting: false,
            on_road: true,
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    pub fn left(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Forward speed in m/s (negative when rolling backwards).
    pub fn speed(&self) -> f32 {
        self.vel.dot(self.forward())
    }

    pub fn top_gear(spec: &KartSpec) -> usize {
        spec.gear_ratios.len() - 1
    }

    /// Advances the simulation by `dt` seconds (internally sub-stepped).
    pub fn step(&mut self, spec: &KartSpec, input: &Input, ground: &Ground, dt: f32) {
        let substeps = ((dt / (1.0 / 240.0)).ceil() as usize).max(1);
        let h = dt / substeps as f32;
        for _ in 0..substeps {
            self.substep(spec, input, ground, h);
        }
        self.boost_time = (self.boost_time - dt).max(0.0);
        self.shift_cooldown = (self.shift_cooldown - dt).max(0.0);
    }

    fn substep(&mut self, spec: &KartSpec, input: &Input, ground: &Ground, dt: f32) {
        let (a, b) = (spec.wheelbase_front, spec.wheelbase_rear);
        let wheelbase = a + b;
        let mass = spec.mass;
        let forward = self.forward();
        let left = self.left();
        let vf = self.vel.dot(forward);
        let vl = self.vel.dot(left);
        let speed = self.vel.length();

        // ---- steering: the front wheel angle follows the input, and the lock shrinks with speed
        let lock = spec.max_steer_lock / (1.0 + (speed / (spec.max_speed_steer_scale * 0.32)).powi(2));
        let target_angle = -input.steer.clamp(-1.0, 1.0) * lock; // input right (+) = wheel turns right = negative angle
        let rate = 5.0;
        self.steer_angle += (target_angle - self.steer_angle).clamp(-rate * dt, rate * dt) * 1.0;
        let delta = self.steer_angle;

        // ---- engine and gearbox
        let wheel_r = spec.wheel_radius();
        let ratio = spec.gear_ratios[self.gear] * spec.final_drive;
        let wheel_omega = vf.abs() / wheel_r;
        let engine_rpm = (wheel_omega * ratio * 60.0 / std::f32::consts::TAU).clamp(1200.0, spec.rev_limit_high);
        self.rpm += (engine_rpm - self.rpm) * (1.0 - (-18.0 * dt).exp());
        if self.shift_cooldown <= 0.0 {
            if self.gear + 1 < spec.gear_ratios.len() && engine_rpm > spec.shift_up_rpm {
                self.gear += 1;
                self.shift_cooldown = 0.25;
            } else if self.gear > 0 && engine_rpm < spec.shift_down_rpm {
                self.gear -= 1;
                self.shift_cooldown = 0.25;
            }
        }
        let limiter = if self.rpm >= spec.rev_limit_low {
            ((spec.rev_limit_high - self.rpm) / (spec.rev_limit_high - spec.rev_limit_low)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let boosting = self.boost_time > 0.0;
        let torque = spec.torque_at(self.rpm.max(1500.0)) * spec.torque_scalar * tuning::POWER_SCALE * input.throttle * limiter;
        let mut drive = torque * ratio * 0.9 / wheel_r;
        if boosting {
            drive += tuning::BOOST_FORCE;
        }

        // ---- longitudinal forces along the kart's forward axis
        let mut f_long = 0.0f32;
        if input.throttle > 0.0 {
            f_long += drive;
        }
        if input.brake > 0.0 {
            if vf > 0.8 {
                f_long -= input.brake * spec.brake_torque * tuning::BRAKE_SCALE / wheel_r;
            } else {
                // reverse
                f_long -= input.brake * 2200.0 * (1.0 - (-vf / 9.0).clamp(0.0, 1.0));
            }
        }
        let surface_drag = if ground.on_road { 1.0 } else { 1.0 + tuning::GRASS_DRAG * 4.0 };
        f_long -= spec.drag * tuning::DRAG_SCALE * 1.4 * surface_drag * vf * vf.abs();
        let rolling = if ground.on_road { 0.012 } else { 0.09 };
        f_long -= rolling * mass * G * vf.clamp(-1.0, 1.0);
        if boosting {
            // a boost lifts the top speed instead of fighting the drag curve
            f_long += spec.drag * 1.4 * (1.0 - 1.0 / tuning::BOOST_SPEED_BONUS.powi(2)) * vf * vf.abs();
        }

        // ---- lateral tyre forces
        let down = spec.down_force * 0.5 * vf * vf * 0.4;
        let load = mass * G + down;
        let (n_front, n_rear) = (load * b / wheelbase, load * a / wheelbase);
        let grip = spec.grip * tuning::GRIP_SCALE * if ground.on_road { 1.0 } else { tuning::GRASS_GRIP };
        let rear_grip = grip * if input.handbrake { tuning::HANDBRAKE_REAR_GRIP } else { 1.0 };
        let v_long = vf.abs().max(1.5);
        let dir = if vf >= 0.0 { 1.0 } else { -1.0 };
        let alpha_f = ((vl + a * self.yaw_rate) / v_long).atan() - delta * dir;
        let alpha_r = ((vl - b * self.yaw_rate) / v_long).atan();
        let fy_front = (-tuning::CORNERING_STIFFNESS * n_front * alpha_f).clamp(-grip * n_front, grip * n_front);
        let fy_rear = (-tuning::CORNERING_STIFFNESS * n_rear * alpha_r).clamp(-rear_grip * n_rear, rear_grip * n_rear);
        self.drifting = (alpha_r.abs() > 0.22 || input.handbrake && speed > 6.0) && speed > 5.0;

        // ---- assemble in body axes, integrate in the world frame
        let force_long = f_long - fy_front * delta.sin();
        let force_lat = fy_front * delta.cos() + fy_rear;
        let yaw_moment = a * fy_front * delta.cos() - b * fy_rear;
        let inertia = spec.inertia_scale * mass * a * b;

        let mut accel = (forward * force_long + left * force_lat) / mass;
        accel += -ground.tangent * (G * ground.slope);
        self.lateral_accel = force_lat / mass;
        self.vel += accel * dt;
        self.yaw_rate += yaw_moment / inertia * dt;
        self.yaw_rate *= (1.0 - 0.4 * dt).max(0.0);
        self.yaw += self.yaw_rate * dt;
        self.pos += self.vel * dt;
        self.wheel_spin += vf / wheel_r * dt;
        self.on_road = ground.on_road;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> KartSpec {
        let xml = include_str!("../../assets/xml_pak/cargeom/kart_base/kart_base.xml");
        KartSpec::from_xml("kart_base", xml, 0.564, -0.496).unwrap()
    }

    #[test]
    fn parses_the_original_spec() {
        let s = spec();
        assert_eq!(s.gear_ratios.len(), 6);
        assert!((s.mass - 285.0).abs() < 0.01);
        assert!(s.torque_curve.len() > 15);
        assert!((s.wheel_radius_rear - 0.30).abs() < 0.001);
    }

    /// Prints the straight-line numbers so the tuning constants can be judged, and checks they are plausible.
    #[test]
    fn straight_line_performance_is_plausible() {
        let s = spec();
        let mut k = Kart::new(Vec3::ZERO, 0.0);
        let input = Input { throttle: 1.0, ..Default::default() };
        let ground = Ground::default();
        let (mut t, mut t100, mut t60) = (0.0f32, None, None);
        while t < 40.0 {
            k.step(&s, &input, &ground, 1.0 / 60.0);
            t += 1.0 / 60.0;
            if t60.is_none() && k.speed() * 3.6 >= 60.0 {
                t60 = Some(t);
            }
            if t100.is_none() && k.speed() * 3.6 >= 100.0 {
                t100 = Some(t);
            }
        }
        println!("0-60 km/h: {:?} s, 0-100 km/h: {:?} s, top speed after 40 s: {:.0} km/h in gear {}", t60, t100, k.speed() * 3.6, k.gear + 1);
        assert!(k.speed() * 3.6 > 70.0 && k.speed() * 3.6 < 220.0, "top speed {:.0} km/h", k.speed() * 3.6);
        assert!(t60.is_some());
    }

    #[test]
    fn steering_turns_the_kart_the_right_way() {
        let s = spec();
        let mut k = Kart::new(Vec3::ZERO, 0.0);
        k.vel = Vec3::new(0.0, 0.0, 15.0);
        let input = Input { throttle: 0.4, steer: 1.0, ..Default::default() };
        for _ in 0..120 {
            k.step(&s, &input, &Ground::default(), 1.0 / 60.0);
        }
        // steering right from +Z must move the kart towards -X (right of +Z in this coordinate system)
        assert!(k.pos.x < -2.0, "x = {}", k.pos.x);
        assert!(k.yaw < 0.0, "yaw = {}", k.yaw);
    }
}
