//! Scene state machine: launch sequence -> event select -> track select -> race.
use crate::trackworld::TrackWorld;
use crate::flow::{FlowEvent, UiFlow};
use crate::drive::{Drive, DriveAssets};
use crate::eventrun::{EventRun, Stage};
use crate::gfx::Draw;
use crate::launch::{self, LaunchScreen, LANDING_START};
use crate::menu::{self, Event, EventSelect, TrackAction, TrackSelect};
use crate::race::{self, Keys, Phase, Race, RaceAssets};
use crate::render3d::{Scene3d, World};
use crate::ui::{ease, solid, Textures, STAGE_H, STAGE_W};
use std::path::PathBuf;

// names of the sound atoms in the original sound.xml
pub const SOUND_FORWARD: &str = "ABY_ui_forward";
pub const SOUND_BACK: &str = "ABY_ui_back";
pub const MUSIC_MENU: &str = "ABY_music_main_menu";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    Launch,
    Events,
    Tracks,
    Race,
    /// Free driving on the baseplate.
    Drive,
    /// A campaign event on its real track (race rules, AI karts, items, results).
    Event,
    /// The real 2.9.1 screens (landing, map, settings ...) driven by their XML layouts.
    Ui,
}

/// Keys the game cares about, already translated from the window system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameKey {
    Up,
    Down,
    Left,
    Right,
    Handbrake,
    Reset,
    Enter,
    /// The ability button of the event HUD.
    Ability,
}

/// What the window layer should do after an input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    None,
    StartRace(usize),
    StartDrive,
    /// Start the campaign event with this index on its real track.
    StartEvent(usize),
}

pub struct Game {
    tx: Textures,
    assets_dir: PathBuf,
    launch: LaunchScreen,
    events: EventSelect,
    tracks: TrackSelect,
    scene: Scene,
    scene_start: f32,
    launch_origin: f32,
    notice: Option<(String, f32)>,
    sounds: Vec<&'static str>,
    keys: Keys,
    race_assets: Option<RaceAssets>,
    race: Option<Race>,
    drive_assets: Option<DriveAssets>,
    drive: Option<Drive>,
    /// Music chosen by the race (it changes at the start and the end of a race).
    race_music: Option<&'static str>,
    /// `--dev-race`: enables the placeholder race (procedural track, own physics tuning). It is NOT part of the 1:1 port
    /// and stays off until the real track data and the game's own driving code replace it.
    dev_race: bool,
    flow: Option<UiFlow>,
    showroom_time: f32,
    track: Option<TrackWorld>,
    track_cache_key: String,
    /// Test aid: a pure-pursuit driver that follows the racing line (`--autopilot`).
    pub autopilot: bool,
    follower: Option<crate::trackworld::LineFollower>,
    event: Option<EventRun>,
    /// A profile that is never saved (headless captures have no UI flow, hence no profile of their own).
    local_meta: Option<crate::meta::Meta>,
    /// Capture runs neither spend energy nor write the save file.
    pub capture_mode: bool,
    /// Capture runs stop as soon as the results are on screen.
    pub stop_at_results: bool,
    /// Test aids applied to the next event: chosen power-ups and the time the ability button is pressed.
    pub test_powerups: Vec<crate::powerups::EPowerup>,
    pub test_ability_at: Option<f32>,
    /// Test aid: run this eventdef (path below `assets292/xml_gameplay`) instead of campaign event `index`; no campaign bookkeeping.
    pub event_file_override: Option<String>,
    /// Test aid (`--script` `event N`): skip the lock / energy gate when starting an event.
    pub skip_gate: bool,
}

const NOTICE_SECONDS: f32 = 3.5;

impl Game {
    /// `skip` fast-forwards the launch timeline (e.g. `LANDING_START` jumps straight to the landing screen).
    pub fn new(tx: Textures, assets_dir: PathBuf, skip: f32, dev_race: bool) -> Game {
        Game {
            dev_race,
            tx,
            assets_dir,
            launch: LaunchScreen::new(),
            events: EventSelect::new(),
            tracks: TrackSelect::new(),
            scene: Scene::Launch,
            scene_start: 0.0,
            launch_origin: -skip,
            notice: None,
            sounds: Vec::new(),
            keys: Keys::default(),
            race_assets: None,
            race: None,
            drive_assets: None,
            drive: None,
            race_music: None,
            flow: None,
            showroom_time: 0.0,
            track: None,
            track_cache_key: String::new(),
            autopilot: false,
            follower: None,
            event: None,
            local_meta: None,
            capture_mode: false,
            stop_at_results: false,
            test_powerups: Vec::new(),
            test_ability_at: None,
            event_file_override: None,
            skip_gate: false,
        }
    }

    /// Loads the kart and plate meshes once, so the garage can show the kart before any drive starts.
    pub fn preload_kart(&mut self, scene3d: &mut Scene3d) {
        if self.drive_assets.is_none() {
            match DriveAssets::load(scene3d, &self.assets_dir) {
                Ok(a) => self.drive_assets = Some(a),
                Err(e) => eprintln!("kart preview unavailable: {e}"),
            }
        }
    }

    /// Index of the first 2D draw that belongs over the 3D world (everything before it is the screen's backdrop).
    pub fn split(&self) -> usize {
        match (self.scene, &self.flow) {
            (Scene::Ui, Some(f)) => f.split(),
            _ => usize::MAX,
        }
    }

    pub fn set_flow(&mut self, flow: UiFlow) {
        self.flow = Some(flow);
    }

    /// Called once per frame with the clock: the launch sequence hands over to the real landing screen.
    pub fn tick(&mut self, now: f32) {
        if self.scene == Scene::Launch && self.launch_time(now) >= LANDING_START {
            if let Some(f) = self.flow.as_mut() {
                f.open_landing();
                self.scene = Scene::Ui;
            }
        }
    }

    pub fn scene(&self) -> Scene {
        self.scene
    }

    pub fn title(&self) -> String {
        match (self.scene, &self.race) {
            (Scene::Launch, _) => "Angry Birds Go!".to_string(),
            (Scene::Events, _) => "Angry Birds Go! - Select Event".to_string(),
            (Scene::Tracks, _) => "Angry Birds Go! - Select Track".to_string(),
            (Scene::Race, Some(r)) => format!("Angry Birds Go! - {}", r.track.def.name),
            (Scene::Race, None) => "Angry Birds Go!".to_string(),
            (Scene::Drive, _) => "Angry Birds Go! - Baseplate".to_string(),
            (Scene::Event, _) => format!("Angry Birds Go! - {}", self.event.as_ref().map(|e| e.event_name.as_str()).unwrap_or("Event")),
            (Scene::Ui, _) => format!("Angry Birds Go! - {}", self.flow.as_ref().map(|f| f.screen()).unwrap_or("")),
        }
    }

    fn launch_time(&self, now: f32) -> f32 {
        now - self.launch_origin
    }

    /// Sound effects queued since the last call (paths relative to the assets folder).
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    /// Looping music that should be playing right now.
    pub fn music(&self, now: f32) -> Option<&'static str> {
        match self.scene {
            Scene::Launch if self.launch_time(now) >= LANDING_START => Some(MUSIC_MENU),
            Scene::Launch => None,
            Scene::Events | Scene::Tracks => Some(MUSIC_MENU),
            Scene::Race => self.race.as_ref().and_then(|r| r.music()),
            Scene::Drive | Scene::Event => None,
            Scene::Ui => Some(MUSIC_MENU),
        }
    }

    /// (volume, pitch) of the engine loop while racing.
    pub fn engine(&self) -> Option<(f32, f32)> {
        match (self.scene, &self.race) {
            (Scene::Race, Some(r)) if r.phase != Phase::Results => Some(r.engine_sound()),
            (Scene::Drive, _) => self.drive.as_ref().map(|d| d.engine_sound()),
            (Scene::Event, _) => self.event.as_ref().filter(|e| e.stage != Stage::Results).map(|e| e.player.engine_sound()),
            _ => None,
        }
    }

    pub fn open_events(&mut self, now: f32) {
        self.launch.reset_input();
        self.events.reset_input();
        self.scene = Scene::Events;
        self.scene_start = now;
    }

    fn open_tracks(&mut self, now: f32) {
        self.tracks.reset_input();
        self.scene = Scene::Tracks;
        self.scene_start = now;
    }

    pub fn back_to_launch(&mut self, now: f32) {
        self.events.reset_input();
        self.launch.reset_input();
        self.scene = Scene::Launch;
        // come back to the finished landing screen, not the intro
        self.launch_origin = now - (LANDING_START + 1.0);
    }

    pub fn set_launch_hover(&mut self, hover: bool) {
        self.launch.set_hover(hover);
    }

    pub fn show_notice(&mut self, text: &str, now: f32) {
        self.notice = Some((text.to_string(), now));
    }

    /// Loads (once) the kart, prop and track meshes and sets up a fresh race on track `index`.
    pub fn start_race(&mut self, scene3d: &mut Scene3d, index: usize, now: f32) -> Result<(), String> {
        if self.race_assets.is_none() {
            self.race_assets = Some(RaceAssets::load(scene3d, &self.assets_dir)?);
        }
        let assets = self.race_assets.as_mut().unwrap();
        let race = Race::new(index, assets);
        assets.ensure_track(scene3d, index, &race.track);
        self.race = Some(race);
        self.keys = Keys::default();
        self.scene = Scene::Race;
        self.scene_start = now;
        Ok(())
    }

    /// Loads (once) the kart and the baseplate and puts the kart at the origin.
    pub fn start_drive(&mut self, scene3d: &mut Scene3d, now: f32) -> Result<(), String> {
        if self.drive_assets.is_none() {
            self.drive_assets = Some(DriveAssets::load(scene3d, &self.assets_dir)?);
        }
        self.track = None;
        self.drive = self.drive_assets.as_ref().map(|a| Drive::new(a, None, None));
        self.keys = Keys::default();
        self.scene = Scene::Drive;
        self.scene_start = now;
        Ok(())
    }

    pub fn drive_mut(&mut self) -> Option<&mut Drive> {
        self.drive.as_mut()
    }

    /// Screenshot mode: hold `keys` on the baseplate for `seconds`.
    pub fn fast_forward_drive(&mut self, seconds: f32, keys: Keys) {
        self.keys = keys;
        let mut t = 0.0;
        while t < seconds {
            self.update(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
    }

    /// Per-frame simulation step.
    pub fn update(&mut self, dt: f32) {
        self.showroom_time += dt;
        if self.scene == Scene::Drive {
            if let (Some(drive), Some(assets)) = (self.drive.as_mut(), self.drive_assets.as_ref()) {
                let _ = assets;
                if drive.phase == crate::drive::Phase::Driving {
                    if let Some(t) = self.track.as_ref().filter(|t| t.race_line.len() > 3) {
                        let autopilot = self.autopilot;
                        let follower = self.follower.get_or_insert_with(|| crate::trackworld::LineFollower::new(&t.race_line));
                        follower.verbose = autopilot;
                        let (steer, brake, dir) = follower.update(&t.race_line, &t.ground, dt, drive.pos(), drive.forward_flat(), drive.speed());
                        drive.auto_steer = autopilot.then_some(steer);
                        drive.auto_brake = autopilot.then_some(brake);
                        drive.track_dir = Some(dir);
                    }
                }
                match self.track.as_ref() {
                    Some(t) => drive.update(dt, &self.keys, &t.ground),
                    None => drive.update(dt, &self.keys, &crate::carsim::FlatGround::default()),
                }
            }
            return;
        }
        if self.scene == Scene::Event {
            self.update_event(dt);
            return;
        }
        if self.scene != Scene::Race {
            return;
        }
        let Some(race) = self.race.as_mut() else { return };
        race.update(dt, &self.keys);
        for event in race.events.drain(..) {
            match event {
                race::Event::Sound(path) => self.sounds.push(path),
            }
        }
    }

    pub fn key(&mut self, key: GameKey, pressed: bool, now: f32, scene3d: &mut Scene3d) {
        // the kart garage: Left / Right page through every kart of the original game
        if pressed && self.scene == Scene::Ui && self.split() != usize::MAX {
            let step = match key {
                GameKey::Left => -1,
                GameKey::Right => 1,
                _ => 0,
            };
            if step != 0 {
                if let Some(a) = self.drive_assets.as_mut() {
                    a.cycle(scene3d, &self.assets_dir, step);
                    let text = format!("{}  ({}/{})", a.name(), a.index + 1, a.folders.len());
                    self.notice = Some((text, now));
                }
            }
        }
        match key {
            GameKey::Up => self.keys.up = pressed,
            GameKey::Down => self.keys.down = pressed,
            GameKey::Left => self.keys.left = pressed,
            GameKey::Right => self.keys.right = pressed,
            GameKey::Handbrake => self.keys.handbrake = pressed,
            GameKey::Reset if pressed && self.scene == Scene::Drive => {
                if let (Some(drive), Some(assets)) = (self.drive.as_mut(), self.drive_assets.as_ref()) {
                    drive.reset(assets, self.track.as_ref().map(|t| &t.ground as &dyn crate::carsim::Ground));
                }
            }
            GameKey::Ability if pressed && self.scene == Scene::Event => {
                if let Some(run) = self.event.as_mut() {
                    run.trigger_ability();
                }
            }
            GameKey::Reset if pressed && self.scene == Scene::Event => {
                if let (Some(run), Some(track)) = (self.event.as_mut(), self.track.as_ref()) {
                    if run.stage == Stage::Results {
                        if let Some(index) = run.campaign_index {
                            if let Err(e) = self.start_event(scene3d, index, now) {
                                eprintln!("restart failed: {e}");
                            }
                        }
                    } else {
                        run.respawn_player(track);
                    }
                }
            }
            GameKey::Enter if pressed && self.scene == Scene::Event => {
                if self.event.as_ref().is_some_and(|e| e.stage == Stage::Results) {
                    self.sounds.push(SOUND_FORWARD);
                    self.leave_event();
                }
            }
            GameKey::Reset if pressed && self.scene == Scene::Race => {
                if let Some(race) = self.race.as_mut() {
                    match race.phase {
                        Phase::Results => {
                            let index = race.track_index;
                            if let Err(e) = self.start_race(scene3d, index, now) {
                                eprintln!("restart failed: {e}");
                            }
                        }
                        _ => race.reset_player(),
                    }
                }
            }
            GameKey::Enter if pressed && self.scene == Scene::Race => {
                if self.race.as_ref().is_some_and(|r| r.phase == Phase::Results) {
                    self.sounds.push(SOUND_FORWARD);
                    self.leave_race(now);
                }
            }
            _ => {}
        }
    }

    fn leave_race(&mut self, now: f32) {
        self.race = None;
        self.keys = Keys::default();
        self.scene = Scene::Events;
        self.scene_start = now;
        self.events.reset_input();
    }

    pub fn draws(&self, now: f32) -> Vec<Draw> {
        let mut out = match self.scene {
            Scene::Launch => self.launch.draws(self.launch_time(now), &self.tx),
            Scene::Events => self.events.draws(now - self.scene_start, &self.tx),
            Scene::Tracks => self.tracks.draws(now - self.scene_start, &self.tx),
            Scene::Race => self.race.as_ref().map(|r| r.hud(&self.tx)).unwrap_or_default(),
            Scene::Drive => self.drive.as_ref().map(|d| d.hud(&self.tx)).unwrap_or_default(),
            Scene::Event => self.event.as_ref().map(|e| e.hud(&self.tx)).unwrap_or_default(),
            Scene::Ui => self.flow.as_ref().map(|f| f.draws()).unwrap_or_default(),
        };
        if let Some((text, since)) = &self.notice {
            let age = now - since;
            if (0.0..NOTICE_SECONDS).contains(&age) {
                let alpha = ease(age / 0.2) * ease((NOTICE_SECONDS - age) / 0.4);
                out.push(solid(self.tx.white, [0.0, STAGE_H - 170.0, STAGE_W, 110.0], [0.0, 0.0, 0.0], 0.7 * alpha));
                self.tx.body_font.draw_centered(text, STAGE_W / 2.0, STAGE_H - 115.0, 1.3, [1.0, 1.0, 1.0, alpha], &mut out);
            }
        }
        out
    }

    /// The 3D world to draw under the 2D layer, if the current scene has one.
    pub fn world(&self, aspect: f32) -> Option<World> {
        match self.scene {
            Scene::Race => Some(self.race.as_ref()?.world(self.race_assets.as_ref()?, aspect)),
            Scene::Drive => Some(self.drive.as_ref()?.world(self.drive_assets.as_ref()?, self.track.as_ref(), aspect)),
            Scene::Event => Some(self.event.as_ref()?.world(self.drive_assets.as_ref()?, self.track.as_ref()?, aspect)),
            Scene::Ui => {
                let flow = self.flow.as_ref()?;
                if flow.split() == usize::MAX {
                    return None;
                }
                Some(crate::drive::kart_showroom(self.drive_assets.as_ref()?, self.showroom_time, aspect))
            }
            _ => None,
        }
    }

    /// Returns true when the cursor should be a pointer.
    pub fn cursor_moved(&mut self, x: f32, y: f32, now: f32) -> bool {
        match self.scene {
            Scene::Launch => self.launch.cursor_moved(x, y, self.launch_time(now)),
            Scene::Events => self.events.cursor_moved(x, y),
            Scene::Tracks => self.tracks.cursor_moved(x, y),
            Scene::Ui => self.flow.as_ref().map(|f| f.pointer(x, y)).unwrap_or(false),
            Scene::Drive => {
                if let Some(d) = self.drive.as_mut() {
                    if d.touch.is_some() {
                        d.touch = Some((x, y));
                    }
                }
                false
            }
            Scene::Event => {
                if let Some(d) = self.event.as_mut().map(|e| &mut e.player) {
                    if d.touch.is_some() {
                        d.touch = Some((x, y));
                    }
                }
                false
            }
            Scene::Race => false,
        }
    }

    pub fn press(&mut self, x: f32, y: f32, now: f32) {
        match self.scene {
            Scene::Launch => self.launch.press(x, y, self.launch_time(now)),
            Scene::Events => self.events.press(x, y),
            Scene::Tracks => self.tracks.press(x, y),
            Scene::Ui => {
                if let Some(f) = self.flow.as_mut() {
                    f.press(x, y);
                }
            }
            Scene::Drive => {
                if let Some(d) = self.drive.as_mut() {
                    d.touch = Some((x, y));
                }
            }
            Scene::Event => {
                if let Some(e) = self.event.as_mut() {
                    e.player.touch = Some((x, y));
                }
            }
            Scene::Race => {}
        }
    }

    pub fn release(&mut self, x: f32, y: f32, now: f32) -> Outcome {
        match self.scene {
            Scene::Launch => {
                if self.launch.release(x, y, self.launch_time(now)) == Some(launch::Action::Play) {
                    self.sounds.push(SOUND_FORWARD);
                    self.open_events(now);
                }
                Outcome::None
            }
            Scene::Events => match self.events.release(x, y) {
                Some(menu::Action::Back) => {
                    self.sounds.push(SOUND_BACK);
                    self.back_to_launch(now);
                    Outcome::None
                }
                Some(menu::Action::Start(Event::Race)) if self.dev_race => {
                    self.sounds.push(SOUND_FORWARD);
                    self.open_tracks(now);
                    Outcome::None
                }
                Some(menu::Action::Start(Event::Race)) => {
                    // the tracks are downloaded content that is not in the APK: drive on the baseplate instead
                    self.sounds.push(SOUND_FORWARD);
                    Outcome::StartDrive
                }
                Some(menu::Action::Start(_)) | None => Outcome::None,
            },
            Scene::Tracks => match self.tracks.release(x, y) {
                Some(TrackAction::Back) => {
                    self.sounds.push(SOUND_BACK);
                    self.open_events(now);
                    Outcome::None
                }
                Some(TrackAction::Start(i)) => {
                    self.sounds.push(SOUND_FORWARD);
                    Outcome::StartRace(i)
                }
                None => Outcome::None,
            },
            Scene::Ui => {
                let Some(f) = self.flow.as_mut() else { return Outcome::None };
                let (sound, event) = f.release(x, y);
                if let Some(s) = sound {
                    self.sounds.push(leak(&s));
                }
                if let Some(n) = f.notice.take() {
                    self.notice = Some((n, now));
                }
                match event {
                    FlowEvent::StartDrive => Outcome::StartDrive,
                    FlowEvent::StartEvent(i) => Outcome::StartEvent(i),
                    FlowEvent::None => Outcome::None,
                }
            }
            Scene::Drive => {
                if let Some(d) = self.drive.as_mut() {
                    d.touch = None;
                }
                Outcome::None
            }
            Scene::Event => {
                if let Some(e) = self.event.as_mut() {
                    e.player.touch = None;
                }
                Outcome::None
            }
            Scene::Race => Outcome::None,
        }
    }

    /// Esc: one step back; returns false when there is nothing left to go back to (quit).
    pub fn escape(&mut self, now: f32) -> bool {
        match self.scene {
            Scene::Drive => {
                self.sounds.push(SOUND_BACK);
                self.drive = None;
                self.keys = Keys::default();
                if let Some(f) = self.flow.as_mut() {
                    f.open_map();
                    self.scene = Scene::Ui;
                } else {
                    self.open_events(now);
                }
                true
            }
            Scene::Ui => {
                let back = self.flow.as_mut().is_some_and(|f| f.back());
                if back {
                    self.sounds.push(SOUND_BACK);
                }
                back
            }
            Scene::Event => {
                self.sounds.push(SOUND_BACK);
                self.leave_event();
                true
            }
            Scene::Race => {
                self.sounds.push(SOUND_BACK);
                self.leave_race(now);
                true
            }
            Scene::Tracks => {
                self.sounds.push(SOUND_BACK);
                self.open_events(now);
                true
            }
            Scene::Events => {
                self.sounds.push(SOUND_BACK);
                self.back_to_launch(now);
                true
            }
            Scene::Launch => false,
        }
    }

    /// Screenshot mode: jump straight into a race and run it for `seconds` with a scripted driver.
    pub fn fast_forward_race(&mut self, seconds: f32, keys: Keys) {
        let step = 1.0 / 60.0;
        let mut t = 0.0;
        self.keys = keys;
        while t < seconds {
            self.update(step);
            t += step;
        }
    }

    pub fn open_tracks_for_capture(&mut self) {
        self.open_tracks(0.0);
    }

    pub fn race_mut(&mut self) -> Option<&mut Race> {
        self.race.as_mut()
    }

    pub fn race_ref(&self) -> Option<&Race> {
        self.race.as_ref()
    }
}

/// UI sound names come from the layouts at run time; the sound queue holds `&'static str`, so each distinct name is kept once.
fn leak(name: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut guard = NAMES.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(existing) = set.get(name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

impl Game {
    /// Screenshot mode: show a front-end screen by its global-state name.
    pub fn goto_flow(&mut self, state: &str) {
        if let Some(f) = self.flow.as_mut() {
            f.goto(state);
            self.scene = Scene::Ui;
        }
    }
}

impl Game {
    /// Starts campaign event `index` (the campaign list position) on its real track: loads the track, builds the race
    /// (`EventRun`: rules, grid, AI karts, items, mode) and pays the energy cost.
    pub fn start_event(&mut self, scene3d: &mut Scene3d, index: usize, now: f32) -> Result<(), String> {
        eprintln!("[start_event] index {index} scene {:?} skip_gate {}", self.scene, self.skip_gate);
        if self.drive_assets.is_none() {
            self.drive_assets = Some(DriveAssets::load(scene3d, &self.assets_dir)?);
        }
        let root = self.assets_dir.parent().ok_or("no parent folder")?.join("assets292");
        let now_s = crate::meta::now_secs();
        // one profile for everything: the flow's, or (headless captures) a private one that is never saved
        let flow_has_meta = self.flow.as_ref().is_some_and(|f| f.meta.is_some());
        if !flow_has_meta && self.local_meta.is_none() {
            self.local_meta = crate::meta::Meta::open(&root, None, now_s).ok();
        }
        let meta: Option<&mut crate::meta::Meta> = if flow_has_meta { self.flow.as_mut().and_then(|f| f.meta.as_mut()) } else { self.local_meta.as_mut() };
        // which file: campaign[index].eventIndex -> <Event index> row; the old (episode = index / 11) guess only without a profile
        let file = match meta.as_deref().and_then(|m| crate::eventrun::eventdef_file_for_campaign(m, &root, index)) {
            _ if self.event_file_override.is_some() => root.join("xml_gameplay").join(self.event_file_override.as_deref().unwrap_or("")),
            Some(f) => f,
            None => {
                let (episode, event) = (index / 11, index % 11);
                root.join(format!("xml_gameplay/eventdef_episode{episode:02}/eventdef_episode{episode:02}_event{event:02}_stage00.xml"))
            }
        };
        let xml = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        // pre-check (the energy is only spent once the event is built, so a failed load costs nothing)
        if !self.capture_mode && !self.skip_gate {
            if let Some(m) = meta.as_deref() {
                let why = if !m.is_event_unlocked(index) {
                    Some("EVENT LOCKED")
                } else if m.energy_level(now_s) < m.campaign_energy_cost(index) {
                    Some("NOT ENOUGH ENERGY")
                } else {
                    None
                };
                if let Some(text) = why {
                    self.notice = Some((text.to_string(), now));
                    return Ok(());
                }
            }
        }
        let (theme, run) = crate::eventrun::track_of_eventdef(&xml)?;
        let key = format!("{theme}/{run}");
        if self.track_cache_key != key || self.track.is_none() {
            self.track = Some(TrackWorld::load(scene3d, &root, &theme, &run, 2)?);
            self.track_cache_key = key;
        }
        let meta: Option<&crate::meta::Meta> = if flow_has_meta { self.flow.as_ref().and_then(|f| f.meta.as_ref()) } else { self.local_meta.as_ref() };
        let track = self.track.as_ref().unwrap();
        let assets = self.drive_assets.as_mut().unwrap();
        let mut run = EventRun::new(scene3d, assets, &self.assets_dir, track, &xml, &stem, if self.event_file_override.is_some() { None } else { Some(index) }, meta, self.autopilot)?;
        for p in self.test_powerups.clone() {
            run.choose_powerup(p);
        }
        run.test_ability_at = self.test_ability_at;
        // energy (CPlayerInfo::PlayedCampaignStage + SpentEnergyOnRace); captures and test event files neither spend nor save
        if !self.capture_mode && self.event_file_override.is_none() {
            let m = if flow_has_meta { self.flow.as_mut().and_then(|f| f.meta.as_mut()) } else { self.local_meta.as_mut() };
            if let Some(m) = m {
                let skip = self.skip_gate;
                if let Err(e) = m.start_campaign_event(index, now_s) {
                    if !skip {
                        self.notice = Some((format!("{e:?}").to_uppercase(), now));
                        return Ok(());
                    }
                }
                if let Err(e) = m.save(now_s) {
                    eprintln!("could not save the profile: {e}");
                }
            }
        }
        for line in &run.log {
            println!("{line}");
        }
        self.event = Some(run);
        self.drive = None;
        self.keys = Keys::default();
        self.scene = Scene::Event;
        self.scene_start = now;
        Ok(())
    }

    fn leave_event(&mut self) {
        self.event = None;
        self.keys = Keys::default();
        if let Some(f) = self.flow.as_mut() {
            f.open_map();
            self.scene = Scene::Ui;
        } else {
            self.scene = Scene::Events;
        }
    }

    pub fn event_ref(&self) -> Option<&EventRun> {
        self.event.as_ref()
    }

    /// Screenshot / self-test mode: run the event for `seconds` of simulated time (stops early when the results show).
    pub fn fast_forward_event(&mut self, seconds: f32, keys: Keys) {
        self.keys = keys;
        let mut t = 0.0;
        while t < seconds {
            self.update(1.0 / 60.0);
            t += 1.0 / 60.0;
            if self.event.as_ref().is_some_and(|e| e.stage == Stage::Results && e.summary.is_some()) && self.stop_at_results {
                break;
            }
        }
    }

    fn update_event(&mut self, dt: f32) {
        let (Some(run), Some(track)) = (self.event.as_mut(), self.track.as_ref()) else { return };
        run.update(dt, &self.keys, track);
        if run.stage == Stage::Results && run.summary.is_none() {
            let now_s = crate::meta::now_secs();
            let flow_has_meta = self.flow.as_ref().is_some_and(|f| f.meta.is_some());
            let meta: Option<&mut crate::meta::Meta> = if flow_has_meta { self.flow.as_mut().and_then(|f| f.meta.as_mut()) } else { self.local_meta.as_mut() };
            run.apply_results(meta, now_s, !self.capture_mode);
            self.sounds.push(SOUND_FORWARD);
        }
        for s in run.sounds.drain(..) {
            self.sounds.push(s);
        }
    }
}

impl Game {
    /// One line about the profile in use (the flow's or the capture's private one), for the self tests.
    pub fn profile_summary(&self, index: usize) -> String {
        let flow_has_meta = self.flow.as_ref().is_some_and(|f| f.meta.is_some());
        let meta = if flow_has_meta { self.flow.as_ref().and_then(|f| f.meta.as_ref()) } else { self.local_meta.as_ref() };
        match meta {
            Some(m) => {
                let now = crate::meta::now_secs();
                let t = m.topbar(now);
                let c = m.profile.campaign.get(index);
                format!(
                    "profile: coins {} gems {} energy {}/{} rank {} xp {}; campaign[{index}] played {:?} completed {:?} best_stars {:?} best_score {:?}; save path {:?}",
                    t.coins,
                    t.gems,
                    t.energy,
                    t.energy_max,
                    t.rank,
                    t.xp_total,
                    c.map(|c| c.played),
                    c.map(|c| c.completed),
                    c.map(|c| c.best_stars),
                    c.map(|c| c.best_score),
                    m.save_path
                )
            }
            None => "no profile".to_string(),
        }
    }
}

impl Game {
    /// Capture hook: look at the player's kart from `rel` (kart-local x left, y up, z forward).
    pub fn set_event_camera(&mut self, rel: Option<glam::Vec3>) {
        if let Some(e) = self.event.as_mut() {
            e.player.debug_cam_rel = rel;
        }
    }
}

impl Game {
    /// One line of state for the scripted-input harness (`--script` `say`).
    pub fn debug_line(&self) -> String {
        match self.scene {
            Scene::Event => match &self.event {
                Some(e) => format!(
                    "event stage {:?} phase {:?} pos {} speed {:.1} m/s at {:.0},{:.0},{:.0} clock {:.1} progress {:.2} coins {} yaw {:.0} steer {:.2} wheels {} lateral {:.1}",
                    e.stage,
                    e.player.phase,
                    e.position(),
                    e.player.speed(),
                    e.player.pos().x,
                    e.player.pos().y,
                    e.player.pos().z,
                    e.rs.clock_s,
                    e.rs.cars[0].tracker.progress_fraction(&e.rs.track),
                    e.coins,
                    e.player.car.forward().x.atan2(e.player.car.forward().z).to_degrees(),
                    e.player.car.steer_angle(),
                    e.player.car.wheels_on_ground(),
                    e.rs.track.splines.get(e.rs.cars[0].tracker.spline_id).map(|s| s.lateral_offset(e.rs.cars[0].tracker.pos, e.player.pos())).unwrap_or(0.0)
                ),
                None => "no event".into(),
            },
            Scene::Drive => self.drive.as_ref().map(|d| format!("drive phase {:?} speed {:.1}", d.phase, d.speed())).unwrap_or_default(),
            _ => String::new(),
        }
    }
}
