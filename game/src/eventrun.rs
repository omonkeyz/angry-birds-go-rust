//! One campaign event on its real track: the race rules (`racerules`), the AI drivers (`raceai`) on real `CarSim` karts, the
//! item world (`items`: coins, boost pads, smackables, slalom gates ...), the game-mode objects (`modes/*`), the HUD and the
//! results screen that applies the rewards to the profile (`meta`).
//!
//! The player's physics, slingshot input and chase camera are the existing `Drive`; this struct wraps it.
//!
//! NOT original (listed in PORT_STATUS.md Update 9): the 3-2-1-GO countdown before every event (the original shows it only for
//! multiplayer / the first-race FTUE, `CGame::ShouldDoCountdownStart @0011b158`), the HUD / results layouts, the AI kart choice,
//! the automatic respawn key, kart-kart collisions (carsim has no chassis contact), the player-finished "brake to a stop" driver.
use crate::carsim::{CModSpec, CarInput, CarSim, CarSpec};
use crate::drive::{self, Drive, DriveAssets};
use crate::eventextras::{character_of_kart_folder, PlayerExtras};
use crate::fullkart::FullKart;
use crate::gfx::Draw;
use crate::items::{ItemEvent, ItemWorld};
use crate::meta::Meta;
use crate::modes::eventdef::{EventHeader, ModeParams};
use crate::modes::types::{CarEffect, GameModeRules, KartState as ModeKart, ModeEvent, ModeInput, ModeState};
use crate::race::{Keys, COUNTDOWN, POSITION_BADGES, TEXT_BOARD, WIN_FLAG};
use crate::raceai::{AiCtx, AiParams, AiTweaks, CarObs, CarSpecAi, Neighbor, PlayerObs, RaceAi};
use crate::racerules::{self as rr, CarState, GameMode, KartObs, RaceEvent, RaceState, RaceTrack, RulesConfig};
use crate::render3d::{Draw3d, Scene3d, World};
use crate::trackworld::TrackWorld;
use crate::ui::{ease, solid, sprite, Textures, STAGE_H, STAGE_W};
use crate::xmodel::XModel;
#[path = "abilityrun.rs"]
mod abilityrun;
use abilityrun::AbilityHost;
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// Seconds the 3-2-1 countdown runs before the race clock starts (`countdown_3/2/1` HUD textures, 1 s each).
const COUNTDOWN_SECONDS: f32 = 3.0;
/// Seconds between the game-over condition and the results screen (the original's finish camera / fireworks timing is UNRESOLVED).
const FINISH_DELAY: f32 = 2.5;
/// Everything further than this from the camera is not drawn (items).
const ITEM_DRAW_RANGE: f32 = 450.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Countdown,
    Racing,
    /// The game-over condition is met; stragglers keep driving until the results show.
    Finishing,
    Results,
}

/// An AI driver: a real `CarSim` kart, its textured model and the ported `CRaceAI`.
struct AiKart {
    id: usize,
    name: String,
    car: CarSim,
    model: Option<FullKart>,
    ai: RaceAi,
    /// Still in the slingshot (not stepped; sits on its grid slot).
    in_sling: bool,
    sling_offset: Vec3,
    wheel_spin: f32,
    air_time: f32,
    /// Grid slot pose, used by the respawn.
    grid: (Vec3, f32),
    boss: bool,
    off_world: f32,
}

/// What the results screen shows (computed once when the results appear).
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub success: bool,
    pub position: i32,
    pub stars: u8,
    pub score: i32,
    pub coins_collected: i32,
    pub bonus_coins: i32,
    pub time_s: Option<f32>,
    pub rewards: Vec<String>,
    pub rank_up: Option<(i32, i32)>,
    pub first_clear: bool,
    pub saved: bool,
    pub lines: Vec<String>,
}

pub struct EventRun {
    pub rs: RaceState,
    pub stage: Stage,
    pub stage_time: f32,
    pub player: Drive,
    ai: Vec<AiKart>,
    pub items: ItemWorld,
    item_models: HashMap<String, Option<XModel>>,
    mode: Option<Box<dyn GameModeRules>>,
    pub mode_note: String,
    mode_timer: Option<f32>,
    pub campaign_index: Option<usize>,
    pub event_name: String,
    pub coins: i32,
    pub boost_pad: bool,
    /// Test aid (NOT original): the ported CRaceAI drives the player's kart and fires its slingshot.
    pub autopilot: bool,
    player_ai: Option<RaceAi>,
    pub summary: Option<Summary>,
    /// The player's ability, bodywork damage and power-ups.
    pub extras: PlayerExtras,
    /// Abilities of every kart (player slot lives in `extras`), their world objects and effects (`abilityrun.rs`).
    abil: AbilityHost,
    /// Test aid: press the ability button when the race clock reaches this time.
    pub test_ability_at: Option<f32>,
    results_applied: bool,
    pub sounds: Vec<&'static str>,
    top_speed_time: f32,
    accel_time: f32,
    min_speed: f32,
    finish_banner: f32,
    pub log: Vec<String>,
    wheel_aspect: f32,
    dbg_t: f32,
    off_world: f32,
    rng: rr::Rng,
}

/// `KART_RED1` (the profile's `visualModel`) -> the geometry folder `kart_red_upgrade1`.
pub fn kart_folder_for_visual_model(visual: &str) -> Option<String> {
    let v = visual.to_ascii_lowercase();
    let body = v.strip_prefix("kart_")?;
    let split = body.rfind(|c: char| !c.is_ascii_digit())? + 1;
    let (name, level) = body.split_at(split);
    if level.is_empty() {
        return None;
    }
    Some(format!("kart_{name}_upgrade{level}"))
}

/// The eventdef file of campaign event `index`: `campaign[index].eventIndex` -> `<Event index>` row (episode, event, stage).
pub fn eventdef_file_for_campaign(meta: &Meta, root: &Path, index: usize) -> Option<std::path::PathBuf> {
    let ce = meta.data.events.campaign.get(index)?;
    let row = meta.data.events.events.iter().find(|e| e.index == ce.event_index)?;
    let ep = rr::EPISODE_NAMES.iter().position(|n| *n == row.episode)?;
    let name = format!("eventdef_episode{ep:02}_event{:02}_stage{:02}", row.event, row.stage);
    let p = root.join(format!("xml_gameplay/eventdef_episode{ep:02}/{name}.xml"));
    p.is_file().then_some(p)
}

fn track_of_event(xml: &str) -> Result<(String, String), String> {
    let path = xml
        .split("<Environment pathname=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .ok_or("event definition names no track")?
        .replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();
    Ok((parts.get(1).copied().unwrap_or("theme002").to_string(), parts.get(3).copied().unwrap_or("run000").to_string()))
}

pub fn track_of_eventdef(xml: &str) -> Result<(String, String), String> {
    track_of_event(xml)
}

fn spec_ai(spec: &CarSpec) -> CarSpecAi {
    CarSpecAi {
        mass: spec.m_fMass,
        down_force: spec.m_fDownForce,
        wheel_peak_grip: spec.wheels.iter().map(|w| w.fPeakGrip).collect(),
        min_desired_speed: spec.m_fMinDesiredSpeed,
        steering_speed_scale: spec.m_fSteeringSpeedScale,
        airborne_steer_scale: spec.m_fAirborneSteerScale,
        ..CarSpecAi::default()
    }
}

/// Ground height under a spline point (the race lines float above the surface).
fn grid_pose(track: &TrackWorld, point: Vec3, dir: Vec3) -> (Vec3, f32) {
    use crate::carsim::Ground;
    let (h, _) = track.ground.height_normal(point + Vec3::Y * 1.0);
    let y = if h > -1.0e3 { h } else { point.y };
    (Vec3::new(point.x, y, point.z), dir.x.atan2(dir.z))
}

impl EventRun {
    /// Builds the whole event: rules, grid, AI karts, items, mode.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scene: &mut Scene3d,
        assets: &mut DriveAssets,
        assets_dir: &Path,
        track: &TrackWorld,
        event_xml: &str,
        file_stem: &str,
        campaign_index: Option<usize>,
        meta: Option<&Meta>,
        autopilot: bool,
    ) -> Result<EventRun, String> {
        let root = assets_dir.parent().ok_or("no parent folder")?.join("assets292");
        let cfg = RulesConfig::load(&root)?;
        let ev = rr::EventDef::parse(event_xml)?;
        let mode = ev.mode;
        if matches!(mode, GameMode::Jenga | GameMode::Intro | GameMode::Intro2 | GameMode::Intro3 | GameMode::Lmr | GameMode::Qmr | GameMode::Tmr | GameMode::Unknown) {
            return Err(format!("{mode:?} events are not playable yet (their mode rules in modes/* are not driven by the race scene)"));
        }
        let rtrack = RaceTrack::from_stm(&track.stm.pvs, &track.track_xml, Some(&ev));
        let main = rtrack.main_line().ok_or("the track has no race line")?;

        // the player's kart from the profile (model folder, spec, mods) - one profile for everything
        let (kart_id, kart_cc, mods, folder) = match meta {
            Some(m) => {
                let id = m.profile.selected_kart.clone();
                let view = m.kart_view(&id);
                let folder = view.as_ref().and_then(|v| kart_folder_for_visual_model(&v.visual_model));
                let ms = m.mod_spec(&id);
                (id.clone(), m.kart_cc(&id), Some(CModSpec { grip: ms.grip, fragility: ms.fragility, thrust: ms.thrust, drag: ms.drag, sliding_ang_vel: ms.sliding_ang_vel, min_speed: ms.min_speed }), folder)
            }
            None => ("SSKM".to_string(), 54, None, None),
        };
        if let Some(f) = folder.as_deref() {
            if !assets.select_folder(scene, assets_dir, f) {
                eprintln!("event: kart folder {f} not found, keeping {}", assets.folders[assets.index]);
            }
        }
        let player_folder = assets.folders[assets.index].clone();
        let player_spec = drive::build_car_spec_with(assets_dir, &player_folder, mods)?;

        // difficulty adjust / cc
        let event_index_row = cfg.events.event_index_for_file(file_stem);
        let event_cc = event_index_row.and_then(|i| cfg.events.campaign_event(i)).map(|c| c.campaign_cc).unwrap_or(kart_cc);
        let adj = cfg.economy.difficulty_adjust_enum(event_cc, kart_cc);
        let kart_count = ev
            .kart_count
            .filter(|n| *n > 0)
            .map(|n| n as usize)
            // UNRESOLVED: the default when `kartcount` is absent (CGame+0x31b8) was not read; solo modes get 1, a boss battle 2, others 4
            .unwrap_or(match mode {
                GameMode::TimeAttack | GameMode::Slalom => 1,
                GameMode::BossBattle | GameMode::BossFruitRush | GameMode::Versus => 2,
                _ => 4,
            });
        let ai_range = cfg.events.ai_skill_range(event_index_row, mode, &cfg.economy);
        let economy = cfg.economy.clone();
        let tweaks = AiTweaks::default();
        let mut rs = RaceState::from_event(ev.clone(), rtrack, cfg, kart_count, adj);
        rs.bind_campaign_event(file_stem);
        rs.score_inputs.kart_cc = kart_cc;
        if let (Some(m), Some(i)) = (meta, campaign_index) {
            rs.stars_before = m.profile.campaign.get(i).map(|c| c.best_stars.max(0) as u8).unwrap_or(0);
        }
        rs.cars[0].is_player = true;
        rs.cars[0].is_local = true;

        // grid slots (CCar::Respawn(slot): slot 1 = the local player)
        let radii = vec![1.2f32; kart_count];
        let grid = rs.grid(main, &radii);
        let mut slot_pose = Vec::new();
        for g in &grid {
            let pose = match g {
                Some((_, _, point, dir)) => grid_pose(track, *point, *dir),
                None => track.start.map(|(p, d)| (p, d.x.atan2(d.z))).ok_or("no start pose")?,
            };
            slot_pose.push(pose);
        }
        let player = Drive::new_with_spec(assets, player_spec.clone(), Some(slot_pose[0]), Some(&track.ground as &dyn crate::carsim::Ground));
        {
            let pos: Vec<Vec3> = slot_pose.iter().map(|p| p.0).collect();
            rs.reset_trackers(&pos);
        }

        // AI karts: roster choice is mine (original: CGameMode::GetAICharacter, not ported)
        let mut rng = rr::Rng::new(0x5eed_0000 ^ (campaign_index.unwrap_or(0) as u32).wrapping_mul(2654435761));
        let mut roster: Vec<String> = ["kart_yellowrocket_upgrade1", "kart_bluebird_upgrade1", "kart_pink_upgrade1", "kart_black_upgrade1", "kart_red_upgrade1", "kart_helmetpig_upgrade1", "kart_white_upgrade1", "kart_orange_upgrade1", "kart_green_upgrade1", "kart_moustache_upgrade1", "kart_terrence_upgrade1", "kart_kingpig_upgrade1"]
            .iter()
            .map(|s| s.to_string())
            .filter(|f| *f != player_folder && assets.folders.contains(f))
            .collect();
        if roster.is_empty() {
            roster.push(player_folder.clone());
        }
// boss battles: the boss file names the kart, the pilot and the boss weapons (`CGameModeBossBattle::GetAIKart / GetAICharacter`)
        let boss_def: Option<(crate::modes::boss::BossDef, String)> = if matches!(mode, GameMode::BossBattle | GameMode::BossFruitRush) {            
crate::modes::boss::BossEventDef::from_xml(event_xml).ok().and_then(|d| {                
let def = crate::modes::boss::BossDef::load(&root, d.bosslevel).ok()?;                
let xml = std::fs::read_to_string(crate::modes::boss::BossDef::path(&root, d.bosslevel)).ok()?;                
Some((def, xml))            })        } else {            
None        };        
let mut ai_chars: Vec<(usize, String, bool)> = Vec::new();
        let grip = |_: i32| 1.0f32;
        let mut ai_karts = Vec::new();
        for id in 1..kart_count {
            let boss = matches!(mode, GameMode::BossBattle | GameMode::BossFruitRush);
            let folder = match (&boss_def, boss) {
                (Some((def, _)), true) => {
                    let base = def.kart_name_string_id.trim_start_matches("KART_").to_ascii_lowercase();
                    [format!("kart_{base}_upgrade{}", def.kart_upgrade_level), format!("kart_{base}_upgrade1")].into_iter().find(|f| assets.folders.contains(f)).unwrap_or_else(|| "kart_kingpig_upgrade1".to_string())
                }
                (None, true) => "kart_kingpig_upgrade1".to_string(),
                _ => roster[(id - 1) % roster.len()].clone(),
            };
            let folder = if assets.folders.contains(&folder) { folder } else { roster[0].clone() };
            let ai_char = match (&boss_def, boss) {
                (Some((def, _)), true) => def.pilot_name.clone(),
                _ => character_of_kart_folder(&folder).to_string(),
            };
            ai_chars.push((id, ai_char, boss));
            let model = assets.load_other_kart(scene, assets_dir, &folder);
            // UNRESOLVED: the AI karts' mods (CGame::AddAI) - they use the player's ratios here
            let spec = drive::build_car_spec_with(assets_dir, &folder, mods).unwrap_or_else(|_| player_spec.clone());
            let hubs = model.as_ref().map(DriveAssets::hubs_of).unwrap_or_else(|| [Vec3::new(0.6, 0.27, 0.78), Vec3::new(-0.6, 0.27, 0.78), Vec3::new(-0.65, 0.35, -0.79), Vec3::new(0.65, 0.35, -0.79)]);
            let pose = slot_pose[id];
            let mut car = CarSim::new_on_ground(spec.clone(), &hubs, pose.0.x, pose.0.z, pose.0.y, pose.1);
            drive::snap_car_to_ground(&mut car, pose, &track.ground);
            let skill = crate::raceai::ai_skill(&economy, adj, ai_range, &mut rng);
            let catchup = crate::raceai::catchup_enabled(mode, &economy, adj, event_index_row.and_then(|i| rs.cfg.events.campaign_event(i)).map(|c| c.disable_catchup));
            let strength = crate::raceai::catchup_strength(id - 1, &mut rng);
            let spline = rs.track.pick_ai_spline(skill, &mut rng);
            let spline = if spline >= 0 { spline as usize } else { main };
            rs.set_ai_spline(id, spline);
            let ai = RaceAi::new(AiParams { skill, boss, catchup, catchup_strength: strength, spline_id: spline, seed: rng.next_u32() }, &rs.track, spec_ai(&spec), &tweaks, &grip);
            ai_karts.push(AiKart { id, name: folder.trim_start_matches("kart_").split('_').next().unwrap_or("AI").to_uppercase(), car, model, ai, in_sling: true, sling_offset: Vec3::ZERO, wheel_spin: 0.0, air_time: 0.0, grid: pose, boss, off_world: 0.0 });
        }
        {
            let pos: Vec<Vec3> = slot_pose.iter().map(|p| p.0).collect();
            rs.reset_trackers(&pos);
        }

        // items on the real track
        let mut items = ItemWorld::build_with_assets(event_xml, &track.stm, Some(&root))?;
        items.replace_markers(0x1234 ^ campaign_index.unwrap_or(0) as u32, 0);
        if matches!(mode, GameMode::SeedRush | GameMode::BossFruitRush) {
            items.token_goal = Some(rs.cars[0].fruit_target.max(0) as u32);
        }
        let mut item_models: HashMap<String, Option<XModel>> = HashMap::new();
        for need in items.required_models() {
            let m = assets.load_xmodel(scene, assets_dir, &format!("pak/{}", need.model));
            item_models.insert(need.model.clone(), m);
        }
        // models of the objects the abilities spawn at run time (smackables that are not on the track)
        for name in abilityrun::ABILITY_OBJECT_MODELS {
            let need = items.smackable_model_need(name);
            if !item_models.contains_key(&need.model) {
                let m = assets.load_xmodel(scene, assets_dir, &format!("pak/{}", need.model));
                item_models.insert(need.model.clone(), m);
            }
        }

        // mode object (modes/*); racerules stays the authority for the finish, score and stars
        let (mode_obj, mode_note) = build_mode(event_xml, &rs, kart_count, kart_cc, event_cc, campaign_index, &root, &economy);

        let character = meta.map(|m| m.profile.selected_character.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| character_of_kart_folder(&player_folder).to_string());
        let level = meta.map(|m| m.mod_spec(&kart_id).character_level_ratio).unwrap_or(0.0);
        let extras = PlayerExtras::new(&root, &character, player_spec.m_fFragility, level);
let mut abil = AbilityHost::new(kart_count, extras.economy.clone());        
abil.add_meshes(scene);        
abil.chars[0] = crate::eventextras::character_file_indexed(&root, &character).map(|(_, i)| i).unwrap_or(0);        
abil.names[0] = extras.ability_name.clone();        
for (id, ch, boss) in &ai_chars {            
let boss_xml = if *boss { boss_def.as_ref().map(|(_, x)| x.as_str()) } else { None };            
let (slot, name, cidx, bmap) = crate::eventextras::build_ability_slot(&root, ch, level, false, boss_xml);            
abil.install(*id, slot, name, cidx, bmap, player_spec.m_fFragility);        
}
        let player_ai = if autopilot { Some(RaceAi::new(AiParams { skill: 0.1, boss: false, catchup: false, catchup_strength: 0.0, spline_id: main, seed: 99 }, &rs.track, spec_ai(&player_spec), &tweaks, &grip)) } else { None };
        let mut run = EventRun {
            rs,
            stage: Stage::Racing,
            stage_time: 0.0,
            player,
            ai: ai_karts,
            items,
            item_models,
            mode: mode_obj,
            mode_note,
            mode_timer: None,
            campaign_index,
            event_name: if ev.title.is_empty() { ev.tag.clone() } else { ev.title.clone() },
            coins: 0,
            boost_pad: false,
            autopilot,
            player_ai,
            summary: None,
            extras,
            abil,
            test_ability_at: None,
            results_applied: false,
            sounds: Vec::new(),
            top_speed_time: 0.0,
            accel_time: 0.0,
            min_speed: player_spec.m_fMinDesiredSpeed,
            finish_banner: 0.0,
            log: Vec::new(),
            wheel_aspect: 1.0,
            dbg_t: 0.0,
            off_world: 0.0,
            rng: rr::Rng::new(7),
        };
        run.log.push(format!(
            "event {} [{file_stem}] ({:?}): stars {:?}, kart {} (profile {kart_id}, mods {mods:?}), {} karts, cc {event_cc}/{kart_cc}, difficulty adjust {adj}, {} items, mode: {}",
            run.event_name,
            mode,
            run.rs.event.stars,
            player_folder,
            kart_count,
            run.items.items.len(),
            run.mode_note
        ));
        let begin = run.rs.begin_grid();
        let _ = begin;
        Ok(run)
    }

    pub fn ai_count(&self) -> usize {
        self.ai.len()
    }

    fn player_obs(&self) -> KartObs {
        let car = &self.player.car;
        let mut k = KartObs::new(0, car.rb.pos);
        k.vel = car.rb.vel;
        k.forward = car.forward();
        k.speed = car.speed();
        k.is_player = true;
        k.is_local = true;
        let c = &self.rs.cars[0];
        k.running = c.running && !c.race_completed;
        k.launched = self.player.phase == drive::Phase::Driving;
        k.in_slingshot = self.player.phase == drive::Phase::Slingshot;
        k.fruit = self.items.fruit_count as i32;
        k
    }

    fn kart_obs_ai(&self, a: &AiKart) -> KartObs {
        let mut k = KartObs::new(a.id, a.car.rb.pos);
        k.vel = a.car.rb.vel;
        k.forward = a.car.forward();
        k.speed = a.car.speed();
        let c = &self.rs.cars[a.id];
        k.running = c.running && !c.race_completed;
        k.launched = !a.in_sling;
        k.in_slingshot = a.in_sling;
        k
    }

    fn car_obs(&self, car: &CarSim, id: usize, in_sling: bool, offset: Vec3, air: f32, is_player: bool) -> CarObs {
        let (_, up, _) = car.rb.axes();
        let forward = car.forward();
        let right = forward.cross(up).normalize_or_zero();
        let c = &self.rs.cars[id];
        let (sid, spos) = (c.tracker.spline_id, c.tracker.pos);
        let lateral = self.rs.track.splines.get(sid).map(|s| s.lateral_offset(spos, car.rb.pos)).unwrap_or(0.0);
        CarObs {
            pos: car.rb.pos,
            vel: car.rb.vel,
            forward,
            right,
            up,
            orientation: car.rb.rot,
            ang_vel: car.rb.ang_vel,
            speed: car.speed(),
            forward_speed: car.forward_speed(),
            steer_angle: car.steer_angle(),
            air_time: air,
            wheels_on_ground: car.wheels_on_ground() as i32,
            in_slingshot: in_sling,
            running: c.running && !c.race_completed,
            pilot_detached: false,
            is_competitor: true,
            is_player,
            radius: 1.2,
            bumped_by_player: false,
            spline_id: sid,
            spline_pos: spos,
            lateral,
            slingshot_axes: if in_sling { Some([-right, up, -forward]) } else { None },
            slingshot_offset: offset,
            ability: None,
        }
    }

    /// The player's kart as `abilities.rs` / `damage.rs` / `powerups.rs` see it.
    fn player_kart_state(&self) -> ModeKart {
        let car = &self.player.car;
        let (_, up, _) = car.rb.axes();
        let c = &self.rs.cars[0];
        ModeKart {
            id: 0,
            is_player: true,
            character: 0,
            pos: car.rb.pos,
            forward: car.forward(),
            up,
            velocity: car.rb.vel,
            speed: car.speed(),
            wheels_on_ground: car.wheels_on_ground() as u32,
            race_position: c.position.max(0) as u32,
            spline_distance: c.tracker.distance_from_start(&self.rs.track),
            lap: 0,
            race_time: c.time_s,
        }
    }

    /// The ability button (`CCar::TriggerAbility`): only while driving.
    pub fn trigger_ability(&mut self) {
        if self.stage != Stage::Racing || self.player.phase != drive::Phase::Driving || self.rs.cars[0].race_completed {
            return;
        }
        let kart = self.player_kart_state();
        self.extras.trigger_ability(&kart, &mut self.player.car, &mut self.log);
    }

    /// Test aid: the power-up is chosen before the race (the original takes it from the shop selection).
    pub fn choose_powerup(&mut self, p: crate::powerups::EPowerup) {
        self.extras.activate_powerup(p);
        self.player.car.launch.power_up_scale = Some(self.extras.launch_scale());
        self.log.push(format!("power-up chosen: {p:?}"));
    }

    /// Player position in the race (1 = leading).
    pub fn position(&self) -> i32 {
        self.rs.cars[0].position
    }

    pub fn kart_count(&self) -> usize {
        self.rs.cars.len()
    }

    /// Respawn on the race line (`CCar::Respawn(-1)` analogue; counts as a death for the score).
    pub fn respawn_player(&mut self, track: &TrackWorld) {
        if self.stage != Stage::Racing || self.player.phase != drive::Phase::Driving {
            return;
        }
        let c = &self.rs.cars[0];
        let Some(s) = self.rs.track.splines.get(c.tracker.spline_id) else { return };
        let (point, dir) = s.info(c.tracker.pos, 0.0);
        let pose = grid_pose(track, point, dir);
        self.player.respawn_on(pose, &track.ground, 18.0);
        self.rs.score_inputs.deaths += 1;
        self.log.push(format!("player respawned at spline {:.1} (deaths {})", self.rs.cars[0].tracker.pos, self.rs.score_inputs.deaths));
    }

    // ------------------------------------------------------------------------------------------------ per frame

    pub fn update(&mut self, dt: f32, keys: &Keys, track: &TrackWorld) {
        let dt = dt.min(0.1);
        self.stage_time += dt;
        match self.stage {
            Stage::Countdown => {
                // karts held on the grid; the player's camera settles
                self.player.update(dt, &Keys::default(), &track.ground);
                let t = self.stage_time;
                if t >= COUNTDOWN_SECONDS {
                    let _ = self.rs.go();
                    self.stage = Stage::Racing;
                    self.stage_time = 0.0;
                }
                return;
            }
            Stage::Results => {
                self.player.update(dt, &Keys::default(), &track.ground);
                return;
            }
            _ => {}
        }
        let started = self.player.phase == drive::Phase::Driving;
        // single player start (CGame::ShouldDoCountdownStart is false): the grid holds until the local player lets go of the slingshot
        if started && self.rs.phase == rr::Phase::Grid {
            let _ = self.rs.go();
        }
        if let Some(t) = self.test_ability_at {
            if self.rs.clock_s >= t {
                self.test_ability_at = None;
                self.trigger_ability();
            }
        }
        if self.autopilot {
            self.dbg_t += dt;
            if self.dbg_t >= 2.0 {
                self.dbg_t = 0.0;
                let c = &self.rs.cars[0];
                let lat = self.rs.track.splines.get(c.tracker.spline_id).map(|s| s.lateral_offset(c.tracker.pos, self.player.pos())).unwrap_or(0.0);
                self.log.push(format!("t={:.0}s [boost {} ability_active {} charged {:.2} damage {:.2} hits {}] player pos {:.0},{:.0},{:.0} speed {:.1} spline {}:{:.1} lateral {:.1} wheels {} phase {:?}", self.rs.clock_s, self.extras.boost, self.extras.ability.is_active(), self.extras.ability.charged_fraction_cache, self.extras.damage_level(), self.extras.hits, self.player.pos().x, self.player.pos().y, self.player.pos().z, self.player.speed(), c.tracker.spline_id, c.tracker.pos, lat, self.player.car.wheels_on_ground(), self.player.phase));
                if let Some(a) = self.ai.first() {
                    let c = &self.rs.cars[a.id];
                    let lat = self.rs.track.splines.get(c.tracker.spline_id).map(|s| s.lateral_offset(c.tracker.pos, a.car.rb.pos)).unwrap_or(0.0);
                    self.log.push(format!("t={:.0}s ai1 pos {:.0},{:.0},{:.0} speed {:.1} spline {}:{:.1} lateral {:.1} sling {}", self.rs.clock_s, a.car.rb.pos.x, a.car.rb.pos.y, a.car.rb.pos.z, a.car.speed(), c.tracker.spline_id, c.tracker.pos, lat, a.in_sling));
                }
            }
        }

        // ---- the player
        if self.autopilot && self.player.phase == drive::Phase::Slingshot && self.stage_time > 0.4 {
            self.player.launch_with(0.9);
        }
        let player_done = self.rs.cars[0].race_completed;
        if self.player.phase == drive::Phase::Driving {
            // direction of the race spline at the car (`CarInput::track_direction`): the real spline node, not a guess
            let c = &self.rs.cars[0];
            if let Some(s) = self.rs.track.splines.get(c.tracker.spline_id) {
                self.player.track_dir = Some(s.nodes[(c.tracker.pos as usize).min(s.count() - 1)].dir);
            }
            if player_done {
                // finished: brake to a stop (NOT original: the original hands the kart to the AI)
                self.player.auto_steer = Some(0.0);
                self.player.auto_brake = Some(1.0);
            } else if self.autopilot {
                // test aid: the ported CRaceAI drives the player's kart (the same driver the opponents use)
                let obs = self.car_obs(&self.player.car, 0, false, Vec3::ZERO, 0.0, false);
                let eco = self.rs.cfg.economy.clone();
                let grip = |_: i32| 1.0f32;
                let ctx = AiCtx {
                    phase: self.rs.phase,
                    mode: self.rs.mode,
                    episode: self.rs.score_inputs.episode,
                    race_clock_s: self.rs.clock_s,
                    human_count: 1,
                    countdown_time: 0.0,
                    countdown_go_time: 0.0,
                    player: None,
                    neighbors: &[],
                    obstacles: &[],
                    tweaks: AiTweaks::default(),
                    economy: &eco,
                    grip_scale: &grip,
                };
                let mut want_respawn = false;
                if let Some(ai) = self.player_ai.as_mut() {
                    let out = ai.update(dt, &obs, &self.rs.track, &ctx);
                    want_respawn = out.respawn;
                    self.player.auto_steer = Some(-out.input.steer);
                    self.player.auto_brake = Some(out.input.brake);
                    for (a, b) in &out.accelerations {
                        self.player.car.apply_acceleration(*a, *b);
                    }
                }
                if want_respawn {
                    // `CRaceAI` stuck detection (the autopilot only; a human uses the R key)
                    self.respawn_player(track);
                }
            }
        }
        let _ = started;
        let keys = if player_done { Keys::default() } else { *keys };
        self.player.pad_boost = self.boost_pad;
        self.player.boost = self.extras.boost;
        self.player.steer_mul = self.extras.steer_multiplier;
        self.player.cam_impact_shake = self.extras.bodywork.cam_shake_mod();
        self.player.update(dt, &keys, &track.ground);
        if self.player.phase == drive::Phase::Driving {
            let kart = self.player_kart_state();
            let wear: f32 = self.player.car.wheels.iter().filter(|w| w.on_ground).map(|w| crate::carsim::phys_material(w.material).wear_rate[0]).sum();
            self.extras.update(dt, &kart, &mut self.player.car, wear, false);
        }
        if self.player.phase == drive::Phase::Driving && !player_done {
            use crate::carsim::Ground;
            self.off_world = if track.ground.height_normal(self.player.pos()).0 < -1.0e3 { self.off_world + dt } else { 0.0 };
            if self.off_world > 1.5 {
                self.off_world = 0.0;
                self.respawn_player(track);
            }
        }
        // score telemetry (CScoreCounterTopSpeed / Acceleration inputs)
        if self.player.phase == drive::Phase::Driving && !player_done {
            if self.player.speed() >= self.min_speed * self.rs.cfg.score.top_speed_threshold {
                self.top_speed_time += dt;
            }
            if !keys.down {
                self.accel_time += dt;
            }
        }
        self.rs.score_inputs.top_speed_seconds = self.top_speed_time;
        self.rs.score_inputs.accel_seconds = self.accel_time;

        // ---- the AI karts
        self.update_ai(dt, track);

        // ---- race rules
        let mut obs = vec![self.player_obs()];
        obs.extend(self.ai.iter().map(|a| self.kart_obs_ai(a)));
        let events = self.rs.update(dt, &obs);
        for e in events {
            match e {
                RaceEvent::Finished { id, position, time_s, state } => {
                    self.log.push(format!("kart {id} finished: position {position}, time {time_s:.1}s, {state:?}"));
                    if id == 0 {
                        self.finish_banner = 3.0;
                    }
                    if let Some(m) = self.mode.as_mut() {
                        let mut ev = Vec::new();
                        m.on_input(&ModeInput::KartFinished { car: id, time: time_s }, &mut ev);
                    }
                }
                RaceEvent::GameOver => {
                    self.log.push("game over condition: every human kart is done".into());
                    self.stage = Stage::Finishing;
                    self.stage_time = 0.0;
                }
                RaceEvent::TimerWarning { .. } => self.sounds.push("ABY_ui_countdown_beep"),
                RaceEvent::PhaseChanged(_) => {}
            }
        }

        // ---- items
        self.update_items(dt);

        // ---- mode object
        self.update_mode(dt);

        if self.stage == Stage::Finishing && self.stage_time >= FINISH_DELAY {
            let _ = self.rs.show_results();
            self.stage = Stage::Results;
            self.stage_time = 0.0;
        }
        self.finish_banner = (self.finish_banner - dt).max(0.0);
    }

    fn update_ai(&mut self, dt: f32, track: &TrackWorld) {
        let n = self.rs.cars.len();
        let player_started = self.player.phase == drive::Phase::Driving || self.player.pull > 0.0;
        let eco = self.rs.cfg.economy.clone();
        let tweaks = AiTweaks::default();
        let grip = |_: i32| 1.0f32;
        // snapshot of everybody for the neighbour lists
        let snap: Vec<(Vec3, Vec3, Vec3, f32, usize, f32, f32, bool)> = (0..n)
            .map(|i| {
                let (car, is_player) = if i == 0 { (&self.player.car, true) } else { (&self.ai[i - 1].car, false) };
                let c = &self.rs.cars[i];
                let lat = self.rs.track.splines.get(c.tracker.spline_id).map(|s| s.lateral_offset(c.tracker.pos, car.rb.pos)).unwrap_or(0.0);
                (car.rb.pos, car.rb.vel, car.forward(), car.speed(), c.tracker.spline_id, c.tracker.pos, lat, is_player)
            })
            .collect();
        let pl = &self.rs.cars[0];
        let player_obs = PlayerObs { speed: self.player.speed(), spline_pos: pl.tracker.pos, spline_id: pl.tracker.spline_id, pos: self.player.pos(), started: player_started };
        let phase = self.rs.phase;
        let clock = self.rs.clock_s;
        let mode = self.rs.mode;
        let episode = self.rs.score_inputs.episode;
        for k in 0..self.ai.len() {
            let id = self.ai[k].id;
            let neighbors: Vec<Neighbor> = (0..n)
                .filter(|&j| j != id && (snap[j].0 - snap[id].0).length_squared() < 90.0 * 90.0)
                .map(|j| Neighbor {
                    id: j,
                    pos: snap[j].0,
                    vel: snap[j].1,
                    forward: snap[j].2,
                    speed: snap[j].3,
                    spline_id: snap[j].4,
                    spline_pos: snap[j].5,
                    lateral: snap[j].6,
                    radius: 1.2,
                    noncollide_timer: 0.0,
                    is_competitor: true,
                    is_player: snap[j].7,
                })
                .collect();
            let obs = {
                let a = &self.ai[k];
                self.car_obs(&a.car, id, a.in_sling, a.sling_offset, a.air_time, false)
            };
            let ctx = AiCtx {
                phase,
                mode,
                episode,
                race_clock_s: clock,
                human_count: 1,
                countdown_time: 0.0,
                countdown_go_time: 0.0,
                player: Some(player_obs.clone()),
                neighbors: &neighbors,
                obstacles: &[],
                tweaks: tweaks.clone(),
                economy: &eco,
                grip_scale: &grip,
            };
            let out = self.ai[k].ai.update(dt, &obs, &self.rs.track, &ctx);
            let a = &mut self.ai[k];
            if let Some(s) = out.set_spline {
                self.rs.set_ai_spline(id, s);
            }
            if out.frozen {
                continue;
            }
            if a.in_sling {
                if let Some(cmd) = out.slingshot {
                    a.sling_offset = cmd.offset;
                    if cmd.release {
                        let (_, up, fwd) = a.car.rb.axes();
                        let cam_fwd = (fwd * 6.4 + up * -1.6).normalize_or_zero();
                        let off = if a.sling_offset.length() > 0.01 { a.sling_offset } else { -fwd * 4.0 };
                        a.car.rb.vel = a.car.slingshot_release(off, cam_fwd);
                        a.in_sling = false;
                        a.sling_offset = Vec3::ZERO;
                    }
                }
                continue;
            }
            // the AI steers with the original sign convention (+1 = right); the car frame is mirrored (see `Drive::update`)
            let input = CarInput { steer: -out.input.steer, brake: out.input.brake, track_direction: out.input.track_direction, boost: false, steer_angle: out.input.steer_angle, pad_boost: false };
            a.car.step(dt, &input, &track.ground);
            for (acc, b) in &out.accelerations {
                a.car.apply_acceleration(*acc, *b);
            }
            a.wheel_spin += a.car.forward_speed() * dt / 0.3;
            a.air_time = if a.car.wheels_on_ground() == 0 { a.air_time + dt } else { 0.0 };
            {
                use crate::carsim::Ground;
                a.off_world = if track.ground.height_normal(a.car.rb.pos).0 < -1.0e3 { a.off_world + dt } else { 0.0 };
            }
            if out.respawn || a.off_world > 1.5 {
                a.off_world = 0.0;
                let c = &self.rs.cars[id];
                if let Some(s) = self.rs.track.splines.get(c.tracker.spline_id) {
                    let (point, dir) = s.info(c.tracker.pos, 0.0);
                    let pose = grid_pose(track, point, dir);
                    let spec = a.car.spec.clone();
                    let hubs = a.model.as_ref().map(DriveAssets::hubs_of).unwrap_or([Vec3::new(0.6, 0.27, 0.78), Vec3::new(-0.6, 0.27, 0.78), Vec3::new(-0.65, 0.35, -0.79), Vec3::new(0.65, 0.35, -0.79)]);
                    let mut car = CarSim::new_on_ground(spec, &hubs, pose.0.x, pose.0.z, pose.0.y, pose.1);
                    drive::snap_car_to_ground(&mut car, pose, &track.ground);
                    car.rb.vel = car.forward() * 18.0;
                    a.car = car;
                }
            }
        }
    }

    fn update_items(&mut self, dt: f32) {
        let mut karts = Vec::with_capacity(1 + self.ai.len());
        let p = &self.player.car;
        karts.push(crate::items::KartState {
            pos: p.rb.pos,
            vel: p.rb.vel,
            radius: 1.2,
            is_ai: false,
            has_player: true,
            time_left: self.rs.cars[0].timer_remaining,
            ..Default::default()
        });
        for a in &self.ai {
            karts.push(crate::items::KartState { pos: a.car.rb.pos, vel: a.car.rb.vel, radius: 1.2, is_ai: true, has_player: false, ..Default::default() });
        }
        // items only react once the player is on the track (the grid / slingshot phase keeps everything still)
        let events = self.items.update_karts(dt, &karts);
        self.boost_pad = false;
        for e in events {
            match e {
                ItemEvent::CoinCollected { value, .. } | ItemEvent::MegaCoinCollected { value, .. } | ItemEvent::TokenCoin { value, .. } => self.coins += value,
                ItemEvent::GemCollected { .. } => self.log.push("gem collected".into()),
                ItemEvent::TokenCollected { count, goal_reached, .. } => {
                    if let Some(m) = self.mode.as_mut() {
                        let mut ev = Vec::new();
                        m.on_input(&ModeInput::SeedCollected { car: 0, count: 1 }, &mut ev);
                    }
                    if goal_reached {
                        self.log.push(format!("seed rush goal reached ({count} fruit)"));
                    }
                }
                ItemEvent::TokenLargeCollected { count, .. } => {
                    let _ = count;
                }
                ItemEvent::BoostPadHit { .. } => self.boost_pad = true,
                ItemEvent::SmackableHit { kart_dv, smashed, type_id, .. } => {
                    self.log.push(format!("t={:.1} smackable {type_id} hit, smashed {smashed}, kart_dv {:?}, speed {:.1}", self.rs.clock_s, kart_dv, self.player.speed()));
                    self.player.car.rb.vel += kart_dv;
                    {
                        let (_, up, fwd) = self.player.car.rb.axes();
                        let ejected = self.extras.on_smackable_hit(kart_dv, type_id, self.player.car.rb.pos, fwd, up, self.player.car.rb.rot, self.player.car.spec.m_fMass);
                        if ejected {
                            self.log.push("pilot ejected by damage".into());
                        }
                    }
                    if smashed {
                        if let Some(m) = self.mode.as_mut() {
                            let mut ev = Vec::new();
                            m.on_input(&ModeInput::Smashed { car: 0, object: format!("smackable{type_id}") }, &mut ev);
                        }
                    }
                }
                ItemEvent::GateMissed { penalty, .. } => {
                    if self.rs.mode == GameMode::Slalom {
                        self.rs.cars[0].timer_remaining -= penalty;
                        self.log.push(format!("slalom gate missed: -{penalty:.1}s"));
                    }
                }
                ItemEvent::GatePassed { gate } => {
                    if let Some(m) = self.mode.as_mut() {
                        let mut ev = Vec::new();
                        m.on_input(&ModeInput::GatePassed { car: 0, gate: gate as u32, clean: true }, &mut ev);
                    }
                }
                _ => {}
            }
        }
        if !self.player_started_ok() {
            // nothing: the world still animates
        }
    }

    fn player_started_ok(&self) -> bool {
        self.player.phase == drive::Phase::Driving
    }

    fn update_mode(&mut self, dt: f32) {
        let Some(m) = self.mode.as_mut() else { return };
        let mut karts = Vec::new();
        for i in 0..self.rs.cars.len() {
            let (car, is_player) = if i == 0 { (&self.player.car, true) } else { (&self.ai[i - 1].car, false) };
            let c = &self.rs.cars[i];
            let launched = if i == 0 { self.player.phase == drive::Phase::Driving } else { !self.ai[i - 1].in_sling };
            let (_, up, _) = car.rb.axes();
            karts.push(ModeKart {
                id: i,
                is_player,
                character: 0,
                pos: car.rb.pos,
                forward: car.forward(),
                up,
                velocity: car.rb.vel,
                speed: car.speed(),
                wheels_on_ground: car.wheels_on_ground() as u32,
                race_position: c.position.max(0) as u32,
                spline_distance: c.tracker.distance_from_start(&self.rs.track),
                lap: 0,
                race_time: if launched { c.time_s.max(1e-3) } else { 0.0 },
            });
        }
        let mut effects: Vec<CarEffect> = Vec::new();
        let mut events: Vec<ModeEvent> = Vec::new();
        m.update(dt, &karts, &mut effects, &mut events);
        for e in events {
            if let ModeEvent::Timer { remaining } = e {
                self.mode_timer = Some(remaining);
            }
        }
        // UNRESOLVED: the effects (boss weapons, coins, slalom TNT requests) are not applied to the world
        let _ = effects;
    }

    /// Applies the finished event to the profile and saves it (called once when the results are on screen).
    pub fn apply_results(&mut self, meta: Option<&mut Meta>, now: u64, save: bool) {
        if self.results_applied || self.stage != Stage::Results {
            return;
        }
        self.results_applied = true;
        let Some(res) = self.rs.results().and_then(|r| r.player.clone()) else { return };
        let mut s = Summary {
            success: res.success,
            position: res.position,
            stars: res.stars,
            score: res.score.total,
            coins_collected: self.coins,
            bonus_coins: res.bonus_coins,
            time_s: res.time_s,
            ..Default::default()
        };
        if let Some(m) = meta {
            if res.bonus_coins + self.coins > 0 {
                m.add_coins(self.coins + res.bonus_coins);
            }
            if let (true, Some(i)) = (res.success, self.campaign_index) {
                match m.complete_campaign_event(i, res.stars as i32, res.score.total, now) {
                    Ok(out) => {
                        s.first_clear = out.first_clear;
                        for r in &out.rewards {
                            s.rewards.push(format!("{} {} x{}", r.kind, r.sub, r.quantity));
                        }
                        s.rank_up = out.rank_up.as_ref().map(|r| (r.from, r.to));
                    }
                    Err(e) => s.lines.push(format!("profile update failed: {e:?}")),
                }
            }
            if save {
                match m.save(now) {
                    Ok(()) => s.saved = true,
                    Err(e) => s.lines.push(format!("save failed: {e}")),
                }
            }
        } else {
            s.lines.push("no profile loaded: nothing saved".into());
        }
        self.log.push(format!(
            "results: success {} position {} stars {} score {} coins {}+{} rewards {:?} rank_up {:?} saved {}",
            s.success, s.position, s.stars, s.score, s.coins_collected, s.bonus_coins, s.rewards, s.rank_up, s.saved
        ));
        self.summary = Some(s);
        let _ = &mut self.rng;
    }

    // ------------------------------------------------------------------------------------------------ drawing

    pub fn world(&self, assets: &DriveAssets, track: &TrackWorld, aspect: f32) -> World {
        let mut w = self.player.world(assets, Some(track), aspect);
        let eye = w.camera.eye;
        // AI karts
        for a in &self.ai {
            let back = if a.in_sling { a.sling_offset } else { Vec3::ZERO };
            let body = Mat4::from_translation(a.car.rb.pos + back) * Mat4::from_quat(a.car.rb.rot);
            match &a.model {
                Some(m) => m.draws(&mut w.draws, body, a.car.steer_angle(), a.wheel_spin),
                None => assets.kart.draws(&mut w.draws, body, [0.7, 0.7, 0.7], a.car.steer_angle(), a.wheel_spin),
            }
        }
        // items
        for inst in self.items.draw_instances() {
            let p = inst.world.w_axis.truncate();
            if (p - eye).length_squared() > ITEM_DRAW_RANGE * ITEM_DRAW_RANGE {
                continue;
            }
            if let Some(Some(model)) = self.item_models.get(&inst.model) {
                model.draws(&mut w.draws, inst.world, inst.tint);
            }
        }
        let _ = self.wheel_aspect;
        w
    }

    fn shadow(font: &crate::font::Font, text: &str, cx: f32, cy: f32, scale: f32, color: [f32; 4], out: &mut Vec<Draw>) {
        font.draw_centered(text, cx + 2.5 * scale, cy + 2.5 * scale, scale, [0.0, 0.0, 0.0, 0.65 * color[3]], out);
        font.draw_centered(text, cx, cy, scale, color, out);
    }

    fn clock_text(t: f32) -> String {
        let t = t.max(0.0);
        format!("{}:{:05.2}", (t / 60.0) as u32, t % 60.0)
    }

    pub fn hud(&self, tx: &Textures) -> Vec<Draw> {
        let mut out = self.player.hud_help(tx, Some("LEFT / RIGHT STEER   DOWN BRAKE   R RESPAWN   ESC LEAVE"));
        let ig = &tx.ingame;
        let c0 = &self.rs.cars[0];
        // top-left board: mode line + clock
        out.push(sprite(ig, TEXT_BOARD, [28.0, 70.0, 380.0, 48.5], 0.92));
        let mode_name = match self.rs.mode {
            GameMode::Race => "RACE",
            GameMode::TimeAttack => "TIME ATTACK",
            GameMode::SeedRush | GameMode::BossFruitRush => "FRUIT RUSH",
            GameMode::BossBattle => "BOSS BATTLE",
            GameMode::Slalom => "SLALOM",
            GameMode::Versus => "VERSUS",
            _ => "EVENT",
        };
        Self::shadow(&tx.body_font, mode_name, 218.0, 94.0, 0.9, [1.0, 1.0, 1.0, 1.0], &mut out);
        let clock = if self.rs.mode.has_timer() {
            Self::clock_text(c0.timer_remaining)
        } else {
            Self::clock_text(c0.time_s)
        };
        let low = self.rs.mode.has_timer() && c0.timer_remaining < 10.0;
        Self::shadow(&tx.body_font, &clock, 218.0, 146.0, 1.0, if low { [1.0, 0.35, 0.3, 1.0] } else { [1.0, 1.0, 1.0, 1.0] }, &mut out);
        if matches!(self.rs.mode, GameMode::SeedRush | GameMode::BossFruitRush) {
            let txt = format!("FRUIT {}/{}", self.items.fruit_count, c0.fruit_target);
            Self::shadow(&tx.body_font, &txt, 218.0, 196.0, 0.95, [1.0, 0.85, 0.3, 1.0], &mut out);
        }
        // top-right: position + coins
        let position = c0.position.max(1) as usize;
        let bx = STAGE_W - 170.0;
        if position <= 3 {
            out.push(sprite(ig, POSITION_BADGES[position - 1], [bx, 70.0, 316.0 * 0.4, 454.0 * 0.4], 1.0));
        } else {
            Self::shadow(&tx.title_font, &format!("{position}TH"), bx + 63.0, 140.0, 1.0, [1.0, 1.0, 1.0, 1.0], &mut out);
        }
        if self.rs.cars.len() > 1 {
            Self::shadow(&tx.body_font, &format!("OF {}", self.rs.cars.len()), bx + 63.0, 262.0, 0.8, [1.0, 1.0, 1.0, 1.0], &mut out);
        }
        Self::shadow(&tx.body_font, &format!("{}", self.coins), bx + 63.0, 300.0, 1.1, [1.0, 0.86, 0.25, 1.0], &mut out);

        // race progress bar (the tracks are point to point: no laps)
        let (x0, x1, y) = (STAGE_W / 2.0 - 450.0, STAGE_W / 2.0 + 450.0, 92.0);
        out.push(solid(tx.white, [x0 - 6.0, y - 10.0, x1 - x0 + 12.0, 20.0], [0.0, 0.0, 0.0], 0.45));
        out.push(solid(tx.white, [x0, y - 3.0, x1 - x0, 6.0], [0.85, 0.85, 0.85], 0.8));
        for c in self.rs.cars.iter().skip(1) {
            let f = c.tracker.progress_fraction(&self.rs.track);
            out.push(solid(tx.white, [x0 + f * (x1 - x0) - 7.0, y - 7.0, 14.0, 14.0], [0.95, 0.35, 0.3], 1.0));
        }
        let f = c0.tracker.progress_fraction(&self.rs.track);
        out.push(solid(tx.white, [x0 + f * (x1 - x0) - 10.0, y - 10.0, 20.0, 20.0], [1.0, 0.85, 0.2], 1.0));
        Self::shadow(&tx.body_font, &format!("{:.0}%", f * 100.0), STAGE_W / 2.0, y + 30.0, 0.7, [1.0, 1.0, 1.0, 0.9], &mut out);

        // ability button bar (charged fraction) and damage meter - layouts are mine
        {
            let mut ex_hud = Vec::new();
            let (x, y, w, h) = (STAGE_W / 2.0 - 160.0, STAGE_H - 78.0, 320.0, 24.0);
            let frac = if self.extras.has_ability() { self.extras.ability.charged_fraction_cache.clamp(0.0, 1.0) } else { 0.0 };
            ex_hud.push(solid(tx.white, [x - 4.0, y - 4.0, w + 8.0, h + 8.0], [0.0, 0.0, 0.0], 0.5));
            ex_hud.push(solid(tx.white, [x, y, w * frac, h], if frac >= 0.999 { [0.4, 0.9, 0.4] } else { [1.0, 0.75, 0.1] }, 0.95));
            let label = if self.extras.has_ability() { format!("E  {}", self.extras.ability_name.to_uppercase()) } else { format!("NO ABILITY ({})", self.extras.character.to_uppercase()) };
            Self::shadow(&tx.body_font, &label, STAGE_W / 2.0, y - 22.0, 0.6, [1.0, 1.0, 1.0, 0.9], &mut ex_hud);
            let dmg = (self.extras.damage_level() / 10.0).clamp(0.0, 1.0);
            ex_hud.push(solid(tx.white, [60.0, STAGE_H - 78.0, 204.0, 24.0], [0.0, 0.0, 0.0], 0.5));
            ex_hud.push(solid(tx.white, [62.0, STAGE_H - 76.0, 200.0 * dmg, 20.0], [0.9, 0.25, 0.2], 0.95));
            Self::shadow(&tx.body_font, "DAMAGE", 162.0, STAGE_H - 98.0, 0.6, [1.0, 1.0, 1.0, 0.9], &mut ex_hud);
            if self.extras.boost {
                Self::shadow(&tx.body_font, "BOOST", STAGE_W / 2.0, STAGE_H - 130.0, 0.9, [0.5, 1.0, 0.6, 1.0], &mut ex_hud);
            }
            if self.stage != Stage::Results {
                out.extend(ex_hud);
            }
        }

        // countdown / GO / finish banner
        match self.stage {
            Stage::Countdown => {
                let left = COUNTDOWN_SECONDS - self.stage_time;
                let n = left.ceil() as i32;
                let (idx, frac) = match n {
                    3 => (Some(0), left - 2.0),
                    2 => (Some(1), left - 1.0),
                    1 => (Some(2), left),
                    _ => (None, 0.0),
                };
                if let Some(i) = idx {
                    let r = COUNTDOWN[i];
                    let pop = 0.85 + 0.25 * ease(1.0 - frac * 1.2);
                    let (w, h) = (r[2] * pop * 0.95, r[3] * pop * 0.95);
                    out.push(sprite(ig, r, [STAGE_W / 2.0 - w / 2.0, STAGE_H * 0.42 - h / 2.0, w, h], 1.0));
                }
            }
            Stage::Racing | Stage::Finishing => {
                if self.finish_banner > 0.0 || self.stage == Stage::Finishing {
                    Self::shadow(&tx.title_font, "FINISH!", STAGE_W / 2.0, STAGE_H * 0.36, 1.6, [1.0, 0.9, 0.3, 1.0], &mut out);
                }
            }
            Stage::Results => self.results_panel(tx, &mut out),
        }
        out
    }

    fn results_panel(&self, tx: &Textures, out: &mut Vec<Draw>) {
        let Some(res) = self.rs.results() else { return };
        let player = res.player.as_ref();
        out.push(solid(tx.white, [0.0, 0.0, STAGE_W, STAGE_H], [0.0, 0.0, 0.0], 0.55));
        let panel = [420.0, 110.0, 1080.0, 860.0];
        out.push(solid(tx.white, panel, [0.12, 0.09, 0.07], 0.93));
        out.push(solid(tx.white, [panel[0], panel[1], panel[2], 6.0], [1.0, 0.75, 0.2], 1.0));
        let success = player.map(|p| p.success).unwrap_or(false);
        Self::shadow(&tx.title_font, if success { "EVENT COMPLETE" } else { "EVENT FAILED" }, STAGE_W / 2.0, 175.0, 1.15, if success { [1.0, 0.85, 0.3, 1.0] } else { [1.0, 0.5, 0.4, 1.0] }, out);
        let place = player.map(|p| p.position.max(1) as usize).unwrap_or(0);
        if (1..=3).contains(&place) {
            out.push(sprite(&tx.ingame, POSITION_BADGES[place - 1], [450.0, 215.0, 316.0 * 0.55, 454.0 * 0.55], 1.0));
        } else {
            Self::shadow(&tx.title_font, &format!("{place}TH"), 560.0, 330.0, 1.5, [1.0, 1.0, 1.0, 1.0], out);
        }
        if place == 1 {
            out.push(sprite(&tx.ingame, WIN_FLAG, [1190.0, 600.0, 633.0 * 0.4, 508.0 * 0.4], 1.0));
        }
        // stars
        if let Some(p) = player {
            for i in 0..3 {
                let on = (i as u8) < p.stars;
                let x = 540.0 + i as f32 * 86.0;
                out.push(solid(tx.white, [x, 480.0, 64.0, 64.0], if on { [1.0, 0.82, 0.15] } else { [0.3, 0.27, 0.22] }, 1.0));
            }
            tx.body_font.draw(&format!("SCORE {}", p.score.total), 520.0, 570.0, 1.0, [1.0, 1.0, 1.0, 1.0], out);
        }
        if let Some(s) = &self.summary {
            let mut y = 625.0;
            tx.body_font.draw(&format!("COINS +{}", s.coins_collected + s.bonus_coins), 520.0, y, 0.9, [1.0, 0.86, 0.25, 1.0], out);
            y += 44.0;
            for r in s.rewards.iter().take(5) {
                tx.body_font.draw(&r.to_uppercase(), 520.0, y, 0.75, [0.8, 1.0, 0.8, 1.0], out);
                y += 36.0;
            }
            if let Some((a, b)) = s.rank_up {
                tx.body_font.draw(&format!("RANK UP {a} -> {b}"), 520.0, y, 0.85, [0.6, 0.9, 1.0, 1.0], out);
            }
        }
        // standings
        for (i, row) in res.rows.iter().enumerate().take(8) {
            let y = 260.0 + i as f32 * 56.0;
            let (x0, x1) = (860.0, 1450.0);
            if row.is_player {
                out.push(solid(tx.white, [x0 - 20.0, y - 24.0, x1 - x0 + 40.0, 48.0], [1.0, 0.75, 0.2], 0.28));
            }
            let color = if row.is_player { [1.0, 0.9, 0.4, 1.0] } else { [1.0, 1.0, 1.0, 1.0] };
            let name = if row.id == 0 { "YOU".to_string() } else { self.ai.get(row.id - 1).map(|a| a.name.clone()).unwrap_or_default() };
            tx.body_font.draw(&format!("{}", row.position), x0, y - 16.0, 0.9, color, out);
            tx.body_font.draw(&name, x0 + 60.0, y - 16.0, 0.9, color, out);
            let time = match row.time_s {
                Some(t) => Self::clock_text(t),
                None => "DNF".to_string(),
            };
            let w = tx.body_font.width(&time, 0.9);
            tx.body_font.draw(&time, x1 - w, y - 16.0, 0.9, color, out);
        }
        let saved = self.summary.as_ref().map(|s| s.saved).unwrap_or(false);
        Self::shadow(&tx.body_font, if saved { "PROFILE SAVED    ENTER CONTINUE    R RETRY" } else { "ENTER CONTINUE    R RETRY" }, STAGE_W / 2.0, 930.0, 0.8, [1.0, 1.0, 1.0, 0.9], out);
    }

    /// One line per kart for the capture log.
    pub fn standings_text(&self) -> String {
        let mut s = String::new();
        for c in &self.rs.cars {
            s += &format!("  kart {} pos {} spline {:.0}/{:.0} state {:?} t {:.1}\n", c.id, c.position, c.tracker.pos, self.rs.track.splines.get(c.tracker.spline_id).map(|x| x.finish_pos).unwrap_or(0.0), c.state, c.time_s);
        }
        let _ = CarState::Racing;
        s
    }

    pub fn mode_state_text(&self) -> String {
        match &self.mode {
            Some(m) => format!("{:?} {:?} score {} stars {}", m.kind(), m.state(), m.score(), m.stars()),
            None => format!("no modes/* object ({})", self.mode_note),
        }
    }

    pub fn mode_state(&self) -> Option<ModeState> {
        self.mode.as_ref().map(|m| m.state())
    }

    pub fn is_boss_kart(&self, id: usize) -> bool {
        id > 0 && self.ai.get(id - 1).map(|a| a.boss).unwrap_or(false)
    }

    pub fn draw_count_hint(&self) -> usize {
        self.item_models.len()
    }

    pub fn ai_grid(&self) -> Vec<(Vec3, f32)> {
        self.ai.iter().map(|a| a.grid).collect()
    }

    pub fn mode_timer(&self) -> Option<f32> {
        self.mode_timer
    }

    pub fn _unused(&self, _d: Draw3d) {}
}

/// Mode objects of `modes/*` for the event's game mode. They run beside `racerules` (which decides finish / score / stars).
#[allow(clippy::too_many_arguments)]
fn build_mode(event_xml: &str, rs: &RaceState, kart_count: usize, kart_cc: i32, event_cc: i32, campaign_index: Option<usize>, root: &Path, economy: &rr::Economy) -> (Option<Box<dyn GameModeRules>>, String) {
    use crate::modes::{boss, seedrush, timeattack, versus};
    let Some(header) = EventHeader::from_xml(event_xml) else { return (None, "event header not parsed by modes/eventdef".into()) };
    let params = ModeParams {
        kart_count,
        kart_cc,
        event_cc,
        event_index: campaign_index.map(|i| i as i32).unwrap_or(-1),
        relative_cc: economy.relative_cc,
        ..ModeParams::default()
    };
    match rs.mode {
        GameMode::TimeAttack => (Some(Box::new(timeattack::TimeAttackMode::new(&header, params))), "modes/timeattack".into()),
        GameMode::SeedRush => {
            let mut m = seedrush::SeedRushMode::new(&header, params);
            m.set_player(0, true);
            (Some(Box::new(m)), "modes/seedrush".into())
        }
        GameMode::Versus => (Some(Box::new(versus::VersusMode::new(&header, params))), "modes/versus".into()),
        GameMode::BossBattle | GameMode::BossFruitRush => match boss::BossEventDef::from_xml(event_xml).and_then(|d| boss::BossBattle::from_eventdef(root, d)) {
            Ok(b) => (Some(Box::new(b)), "modes/boss (boss abilities are not applied to the world)".into()),
            Err(e) => (None, format!("modes/boss not built: {e}")),
        },
        GameMode::Slalom => (None, "slalom: items/slalom gates + racerules timer (modes/slalom is not driven)".into()),
        GameMode::Race => (None, "race: racerules only (no mode object)".into()),
        m => (None, format!("{m:?}: no mode object wired (modes/jenga, intro, multiplayer are not driven)")),
    }
}
