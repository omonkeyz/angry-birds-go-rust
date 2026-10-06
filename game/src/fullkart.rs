//! A complete kart of the 2.9.x data: textured chassis, wheels, every body part on its attach point and the driver on `attach_pilot_1`.
use crate::render3d::{Draw3d, Scene3d};
use crate::xmodel::{TextureIndex, XModel};
use glam::{Mat4, Quat, Vec3};
use std::path::Path;

pub struct FullKart {
    pub name: String,
    chassis: XModel,
    parts: Vec<(XModel, Mat4)>,
    wheel_front: XModel,
    wheel_rear: XModel,
    pilot: Option<(XModel, Mat4)>,
    /// Left hub positions (right = mirrored in x) and wheel scale (diameter) of the front and rear axle.
    pub front_hub: Vec3,
    pub rear_hub: Vec3,
    front_scale: f32,
    rear_scale: f32,
}

fn node_matrix(n: &abgtool::xgm::Node) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::from(n.scale), Quat::from_array(n.rotation_engine), Vec3::from(n.position))
}

/// `kart_red_upgrade1` -> the driver model stem `red`.
fn pilot_stem(folder: &str) -> &str {
    let rest = folder.trim_start_matches("kart_");
    rest.split("_upgrade").next().unwrap_or(rest).split('_').next().unwrap_or(rest)
}

impl FullKart {
    /// `root` = assets292, `folder` = `kart_red_upgrade1`; `lod` = 2, 3 or 4 (`_l02` .. `_l04`).
    pub fn load(scene: &mut Scene3d, textures: &mut TextureIndex, root: &Path, folder: &str, lod: u8) -> Result<FullKart, String> {
        // the kart lives under one of the theme folders (and `telepod`)
        let dir = (2..=6)
            .map(|t| root.join(format!("pak/cars/theme00{t}/cargeom/{folder}")))
            .chain(std::iter::once(root.join(format!("pak/cars/telepod/cargeom/{folder}"))))
            .find(|d| d.is_dir())
            .ok_or_else(|| format!("kart {folder} not found in assets292"))?;
        let suffix = format!("_l0{lod}.xgm");
        let load = |scene: &mut Scene3d, textures: &mut TextureIndex, name: &str| XModel::load(scene, textures, &dir.join(format!("{name}{suffix}")), true);
        let chassis = load(scene, textures, "chassis")?;
        let wheel_front = load(scene, textures, "wheelfront")?;
        let wheel_rear = load(scene, textures, "wheelrear")?;
        let hub = |name: &str| chassis.node(name).map(|n| (Vec3::from(n.position), n.scale[0]));
        // the chassis nodes carry a scale of 1.76 that is not the wheel size: the wheel meshes have unit diameter and the car spec
        // (kart_base.xml fWheelRadius 0.25 front / 0.30 rear) gives the real size
        let front_hub = hub("front_left_wheel").map(|h| h.0).unwrap_or(Vec3::new(0.6, 0.27, 0.78));
        let rear_hub = hub("rear_left_wheel").map(|h| h.0).unwrap_or(Vec3::new(0.65, 0.35, -0.79));
        let (front_scale, rear_scale) = (0.5, 0.6);

        let mut parts = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            let mut names: Vec<String> = read
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .filter(|n| n.ends_with(&suffix) && !n.contains("outline") && !n.starts_with("chassis") && !n.starts_with("wheel"))
                .collect();
            names.sort();
            for file in names {
                let stem = file.trim_end_matches(&suffix).to_string();
                let Ok(part) = XModel::load(scene, textures, &dir.join(&file), true) else { continue };
                let want = format!("attach_{stem}_1");
                let (Some(a), Some(b)) = (chassis.node(&want), part.node("attach_1")) else { continue };
                let local = node_matrix(a) * node_matrix(b).inverse();
                parts.push((part, local));
            }
        }

        let pilot = chassis.node("attach_pilot_1").and_then(|seat| {
            let path = root.join(format!("pak/characters/models/{}{suffix}", pilot_stem(folder)));
            let bird = XModel::load(scene, textures, &path, true).ok()?;
            // like every part: the model's own `attach_1` helper sits on the kart's `attach_pilot_1`
            let local = match bird.node("attach_1") {
                Some(a) => node_matrix(seat) * node_matrix(a).inverse(),
                None => node_matrix(seat),
            };
            Some((bird, local))
        });
        Ok(FullKart { name: folder.to_string(), chassis, parts, wheel_front, wheel_rear, pilot, front_hub, rear_hub, front_scale, rear_scale })
    }

    /// Wheel radius in metres (the wheel meshes have unit diameter).
    pub fn wheel_radii(&self) -> (f32, f32) {
        (self.front_scale * 0.5, self.rear_scale * 0.5)
    }

    /// `body` = kart-to-world transform, `steer` = front wheel angle, `spin` = wheel rotation.
    pub fn draws(&self, out: &mut Vec<Draw3d>, body: Mat4, steer: f32, spin: f32) {
        let white = [1.0; 4];
        self.chassis.draws(out, body, white);
        for (part, local) in &self.parts {
            part.draws(out, body * *local, white);
        }
        if let Some((bird, seat)) = &self.pilot {
            bird.draws(out, body * *seat, white);
        }
        for (wheel, hub, scale, angle) in [(&self.wheel_front, self.front_hub, self.front_scale, steer), (&self.wheel_rear, self.rear_hub, self.rear_scale, 0.0)] {
            for side in [1.0f32, -1.0] {
                // left wheels use the mesh as it is, right wheels are mirrored across x
                let m = body
                    * Mat4::from_translation(Vec3::new(hub.x * side, hub.y, hub.z))
                    * Mat4::from_rotation_y(angle)
                    * Mat4::from_rotation_x(spin)
                    * Mat4::from_scale(Vec3::new(scale * side, scale, scale));
                wheel.draws(out, m, white);
            }
        }
    }
}
