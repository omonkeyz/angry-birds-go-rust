//! The player's ability, bodywork damage and power-ups inside an event: glue between `abilities.rs` / `damage.rs` / `powerups.rs`
//! (pure data ports that return requests) and the real `CarSim` kart of the player.
//!
//! Wired: the character's bird ability (`CarAbilitySlot`: trigger button, per-physics-step `OnCarIntegrate`, per-frame update,
//! `BodyForce` effects applied to the car, steering multiplier), bodywork wear + smackable hits through `Bodywork::on_collision`,
//! the power-ups `SpeedBooster` (-> `CarInput::boost`), `KingSling` (launch scale) and `AutoRepair`.
//! Not wired (see PORT_STATUS.md Update 9): every ability that `abilities.rs` has not ported (only the speed boosts exist),
//! particles / sounds requested by effects, time-scale / gravity / downforce effects, body part rigid bodies, car-car collisions.
use crate::abilities::{AbilityEconomy, AbilityParams, CarAbilitySlot, RaceAbilityCtx};
use crate::carsim::CarSim;
use crate::damage::{Bodywork, CollisionInput, DamageEvent, OtherBody};
use crate::modes::types::{CarEffect, KartState};
use crate::powerups::{integrate_powerups, king_sling_launch_scale, DebugTweakables, EPowerup, PlayerPowerups, PowerupInput, PowerupVfx};
use glam::Vec3;
use std::collections::BTreeSet;
use std::path::Path;

/// Seconds of one physics step (`rb+0x98`, UNRESOLVED 1/60 as in carsim).
const PHYSICS_DT: f32 = 1.0 / 60.0;

pub struct PlayerExtras {
    pub bodywork: Bodywork,
    pub ability: CarAbilitySlot,
    pub ability_name: String,
    pub character: String,
    pub powerups: PlayerPowerups,
    vfx: PowerupVfx,
    tweaks: DebugTweakables,
    pub economy: AbilityEconomy,
    accum: f32,
    /// Effects the glue could not apply (shown once in the log).
    pub unsupported: BTreeSet<String>,
    pub steer_multiplier: f32,
    pub boost: bool,
    pub hits: u32,
    pub damage_events: u32,
    pub triggers: u32,
}

/// The driver (xml `<Name>`) of a kart folder such as `kart_bluebird_upgrade2`.
pub fn character_of_kart_folder(folder: &str) -> &'static str {
    let rest = folder.trim_start_matches("kart_");
    let stem = rest.split("_upgrade").next().unwrap_or(rest).split('_').next().unwrap_or(rest);
    match stem {
        "red" => "Red",
        "bluebird" | "blue" => "Blue",
        "pink" => "Pink",
        "black" => "Black",
        "yellowrocket" | "yellow" => "Yellow",
        "helmetpig" => "Helmet_Pig",
        "green" => "Green",
        "orange" => "Orange",
        "white" => "White",
        "kingpig" => "King_Pig",
        "moustache" => "Moustache_Pig",
        "terrence" | "terence" => "Big_Red",
        _ => "Red",
    }
}

/// xml `<Name>` of the character a name in the profile / folder table refers to (old and new spellings).
fn canonical_character(name: &str) -> String {
    let t = name.trim();
    let alias = match t.to_ascii_lowercase().as_str() {
        "bomb" | "black" => "Black",
        "chuck" | "yellow" => "Yellow",
        "blues" | "blue" => "Blue",
        "stella" | "pink" => "Pink",
        "matilda" | "white" => "White",
        "bubbles" | "orange" => "Orange",
        "hal" | "green" => "Green",
        "minion" | "helmet_pig" | "helmetpig" => "Helmet_Pig",
        "kingpig" | "king_pig" => "King_Pig",
        "moustache" | "moustache_pig" => "Moustache_Pig",
        "terence" | "big_red" | "bigred" | "terrence" => "Big_Red",
        "red" => "Red",
        _ => t,
    };
    alias.to_string()
}

/// Character xml (`char_NNN.xml`) and its index `NNN - 1` (the original's character byte) by `<Name>`.
pub fn character_file_indexed(root: &Path, name: &str) -> Option<(String, u8)> {
    let want = canonical_character(name);
    for i in 1..=16 {
        let p = root.join("xml/characters/charxml").join(format!("char_{i:03}.xml"));
        let Ok(xml) = std::fs::read_to_string(&p) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&xml) else { continue };
        let n = doc.root_element().children().find(|c| c.tag_name().name() == "Name").and_then(|c| c.text()).unwrap_or("");
        if n.eq_ignore_ascii_case(&want) {
            return Some((xml, (i - 1) as u8));
        }
    }
    None
}

fn character_file(root: &Path, name: &str) -> Option<String> {
    character_file_indexed(root, name).map(|(x, _)| x)
}

/// Build the ability slot of a kart driven by `character`: its bird ability (`CreateAbility`), optionally the boss weapons of
/// `boss_xml` (`CCar::LoadBossAbilities`). Returns the slot, the ability name and the character byte.
pub fn build_ability_slot(root: &Path, character: &str, level: f32, is_player: bool, boss_xml: Option<&str>) -> (CarAbilitySlot, String, u8, Vec<Option<usize>>) {
    let (xml, ch) = character_file_indexed(root, character).unwrap_or_default();
    let mut ability = None;
    let mut name = String::new();
    if let Some(params) = AbilityParams::from_character_xml(&xml) {
        name = params.name.clone();
        ability = crate::abilities::create_ability(&name, &params, level);
    }
    let mut slot = CarAbilitySlot::new(ability);
    // CCar::LoadBossAbilities: the boss weapons of the boss xml (index in the xml -> index in the slot, None when not ported)
    let mut map = Vec::new();
    if let Some(bx) = boss_xml {
        for i in 0..AbilityParams::boss_ability_count(bx) {
            let made = AbilityParams::from_boss_xml(bx, i).and_then(|p| crate::abilities::create_boss_ability(&p.ty.clone(), &p, level));
            match made {
                Some(a) => {
                    if slot.push_boss(a) {
                        map.push(Some(slot.boss.len() - 1));
                    } else {
                        map.push(None);
                    }
                }
                None => map.push(None),
            }
        }
    }
    let _ = is_player;
    (slot, name, ch, map)
}

impl PlayerExtras {
    pub fn new(root: &Path, character: &str, fragility: f32, level: f32) -> PlayerExtras {
        let (slot, name, _ch, _) = build_ability_slot(root, character, level, true, None);
        let economy = std::fs::read_to_string(root.join("xml_gameplay/misc/economy.xml")).map(|x| AbilityEconomy::from_economy_xml(&x)).unwrap_or_default();
        let tweaks = DebugTweakables::from_xml(&std::fs::read_to_string(root.join("xml_gameplay/misc/debugtweakables.xml")).unwrap_or_default()).unwrap_or_default();
        PlayerExtras {
            bodywork: Bodywork::new(Vec::new(), fragility),
            ability: slot,
            ability_name: name,
            character: character.to_string(),
            powerups: PlayerPowerups::default(),
            vfx: PowerupVfx::default(),
            tweaks,
            economy,
            accum: 0.0,
            unsupported: BTreeSet::new(),
            steer_multiplier: 1.0,
            boost: false,
            hits: 0,
            damage_events: 0,
            triggers: 0,
        }
    }

    pub fn has_ability(&self) -> bool {
        self.ability.ability.is_some()
    }

    /// `KingSling` launch scale to put on the car's launch parameters (1.0 without the power-up).
    pub fn launch_scale(&self) -> f32 {
        king_sling_launch_scale(&self.powerups, true, &self.tweaks)
    }

    pub fn charged_fraction(&mut self) -> f32 {
        self.ability.charged_fraction(true).clamp(0.0, 1.0)
    }

    /// One frame: ability clocks (per-frame `OnCarUpdate`, per-physics-step `OnCarIntegrate`), bodywork wear, power-up frame.
    pub fn update(&mut self, dt: f32, kart: &KartState, car: &mut CarSim, wear_sum: f32, repairing: bool) {
        // (the ability clocks / effects are driven by `eventrun::abilityrun`, which owns the environment of every kart)
        let _ = (car, &mut self.accum);
        let mut ev = Vec::new();
        self.bodywork.wear_step(dt, kart.speed, kart.wheels_on_ground, wear_sum, &mut ev);
        self.bodywork.update(dt, 1.0, &mut ev);
        let active_autorepair = self.powerups.is_active(1);
        self.bodywork.autorepair_update(dt, active_autorepair, &mut ev);
        self.damage_events += ev.len() as u32;
        // power-ups
        let inp = PowerupInput {
            car: kart.id,
            is_player: true,
            character: kart.character,
            repairing,
            ability_type4_active: false,
            boost_suppressed: false,
            branded: false,
            local_audio: false,
            rb_98: PHYSICS_DT,
            boost_start_sound_playing: false,
        };
        let frame = integrate_powerups(&inp, &self.powerups, &mut self.vfx, &self.tweaks);
        self.boost = frame.boost;
        let _ = frame.effects;
    }

    /// A smackable hit pushed the kart by `kart_dv`: the damage maths of `CCar::CollisionCallback`. The impulse is rebuilt from the
    /// item's velocity change (NOT original: the original reads the physics contact impulse, carsim has no chassis contacts).
    pub fn on_smackable_hit(&mut self, kart_dv: Vec3, type_id: u32, pos: Vec3, forward: Vec3, up: Vec3, rot: glam::Quat, mass: f32) -> bool {
        self.hits += 1;
        let mass = if mass > 0.0 { mass } else { 1.0 };
        let mut c = CollisionInput::simple(kart_dv * mass * 2.0, pos + forward * 1.5, mass, up);
        c.other = OtherBody::Smackable { type_id: type_id as i32 };
        c.self_is_player = true;
        let (_, ev) = self.bodywork.apply_impact(&c, pos, rot, &mut []);
        self.damage_events += ev.len() as u32;
        ev.iter().any(|e| matches!(e, DamageEvent::PilotEjected { .. }))
    }

    pub fn pilot_detached(&self) -> bool {
        self.bodywork.is_pilot_detached()
    }

    /// Max damage accumulator, for the HUD (the damage the pilot ejection rule looks at).
    pub fn damage_level(&self) -> f32 {
        self.bodywork.side_damage.iter().cloned().fold(0.0, f32::max)
    }

    /// The ability button: `CCar::TriggerAbility` through the slot (clock, charges and reuse delay checked there).
    pub fn trigger_ability(&mut self, kart: &KartState, car: &mut CarSim, _log: &mut Vec<String>) {
        let mut out = Vec::new();
        if self.ability.trigger(kart, &mut out) {
            self.triggers += 1;
            self.apply(out, car);
        }
    }

    pub fn activate_powerup(&mut self, p: EPowerup) {
        self.powerups.chosen[p as usize] = true;
    }

    fn apply(&mut self, effects: Vec<CarEffect>, car: &mut CarSim) {
        for e in effects {
            match e {
                CarEffect::BodyForce { force_local, at_local, .. } => car.rb.apply_body_force(force_local, at_local),
                CarEffect::WorldForce { force, at, .. } => car.rb.apply_world_force(force, at),
                CarEffect::SetSteeringMultiplier { mul, .. } => self.steer_multiplier = mul,
                CarEffect::Particle { .. } | CarEffect::Sound { .. } => {
                    self.unsupported.insert("particles / sounds of abilities".into());
                }
                CarEffect::SetTimeScale { .. } => {
                    self.unsupported.insert("SetTimeScale (per-car physics time dilation)".into());
                }
                CarEffect::SetGravityMultiplier { .. } | CarEffect::SetDownforceMultiplier { .. } => {
                    self.unsupported.insert("gravity / downforce multipliers".into());
                }
                CarEffect::Other { tag, .. } => {
                    self.unsupported.insert(format!("Other:{tag}"));
                }
                other => {
                    self.unsupported.insert(format!("{:?}", std::mem::discriminant(&other)));
                }
            }
        }
    }
}
