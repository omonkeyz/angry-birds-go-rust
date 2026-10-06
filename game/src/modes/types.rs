//! Shared plain-data types for the gameplay-feature modules (`powerups.rs`, `abilities.rs`, `damage.rs`, `modes/*`).
//!
//! Standalone: depends only on `std` + `glam` (+ `roxmltree` in the modules that parse XML). The lead adapts the
//! race/car state into [`KartState`] each tick and applies the returned [`CarEffect`] requests to the car sim
//! (`carsim.rs`) and the world. Nothing here is an original struct; original symbol names are quoted in the modules.
#![allow(dead_code)]

pub use glam::Vec3;

/// Index of a kart in the race (`CGame` car list index; `CCar+0x30` is the player/camera slot, `0x1a5c` the character byte).
pub type CarId = usize;

/// Where the repo's decoded original data lives (tests only; every test skips when absent).
pub const ASSETS292: &str = "C:/Users/Brady/Desktop/AngryBirdsGo/assets292";

/// Read-only snapshot of one kart that the feature modules need. The lead fills it from the car sim + race state.
#[derive(Clone, Debug)]
pub struct KartState {
    pub id: CarId,
    /// `CCar+0x1af0 != 0` in the original: the car is driven by a local human (has a `CPlayer`).
    pub is_player: bool,
    /// Character byte (`CCar+0x1a5c`), index into the character table (`char_001.xml` is index 0, bosses follow).
    pub character: u8,
    /// Chassis position (`rb+0x38..0x40`) and the chassis axes (`rb+0x10..0x18` forward).
    pub pos: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub velocity: Vec3,
    /// |velocity| in m/s.
    pub speed: f32,
    /// `CCar::GetNumWheelsOnGround`.
    pub wheels_on_ground: u32,
    /// 1 = leading. 0 = unknown.
    pub race_position: u32,
    /// Distance along the race spline (m) and the lap number (0-based), from the race rules.
    pub spline_distance: f32,
    pub lap: u32,
    /// Seconds since the race started (race clock, after the launch).
    pub race_time: f32,
}

impl Default for KartState {
    fn default() -> Self {
        KartState {
            id: 0,
            is_player: false,
            character: 0,
            pos: Vec3::ZERO,
            forward: Vec3::Z,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            speed: 0.0,
            wheels_on_ground: 4,
            race_position: 0,
            spline_distance: 0.0,
            lap: 0,
            race_time: 0.0,
        }
    }
}

/// Things the feature modules ask the host (car sim, world, camera, audio, VFX) to do. The module never mutates a car.
/// Variants carry the original quantity names so the host can map them onto `carsim.rs`.
#[derive(Clone, Debug, PartialEq)]
pub enum CarEffect {
    /// `CXGSRigidBody::ApplyBodyForce(force_local, at_local)` impulse (already multiplied by dt where the original does).
    BodyForce { car: CarId, force_local: Vec3, at_local: Vec3 },
    /// `CXGSRigidBody::ApplyWorldForce`.
    WorldForce { car: CarId, force: Vec3, at: Vec3 },
    /// Per-car physics time dilation: `rb+0xc` (time scale float) and `rb+0x114` (integrate-scalar int). Yellow bird "SpeedBoost".
    SetTimeScale { car: CarId, scale: f32, integrate_scalar: i32 },
    /// Steering authority multiplier (ability `SteeringMultiplier`). 1.0 = normal.
    SetSteeringMultiplier { car: CarId, mul: f32 },
    /// Gravity / downforce multipliers (pink bird `GravityMultiplier`, `DownforceMultiplier`). 1.0 = normal.
    SetGravityMultiplier { car: CarId, mul: f32 },
    SetDownforceMultiplier { car: CarId, mul: f32 },
    /// Car becomes invulnerable to smackable / car damage for `seconds`.
    SetInvulnerable { car: CarId, seconds: f32 },
    /// Spin the car out (`spins` full rotations over `time` seconds).
    SpinOut { car: CarId, time: f32, spins: f32 },
    /// Hit points of damage to a car (ability `Damage`, boss weapon damage) -> `damage.rs`.
    Damage { car: CarId, amount: f32, source: Option<CarId> },
    /// Radial explosion (bomb, matilda egg, terence rage ...): impulse `force` falling off to `radius`, `damage` to cars inside.
    Explosion { center: Vec3, radius: f32, force: f32, damage: f32, source: Option<CarId> },
    /// Spawn a world object (projectile / dropped smackable / minion shield ...). `kind` is the smackable/xgm name.
    SpawnObject { kind: String, pos: Vec3, velocity: Vec3, owner: Option<CarId> },
    /// Camera: extra "cam behind" distance and how long (`CamBehindMod`, `CamBehindTime`).
    CameraBehind { car: CarId, modifier: f32, time: f32 },
    /// Global slow motion (`CGame::EnterSlowMo`).
    SlowMo { scale: f32, seconds: f32 },
    /// Particle effect by the original name (`CXGSParticleEffectManager::FindEffect(name)`), attached to a car or at a point.
    Particle { name: String, car: Option<CarId>, pos: Vec3 },
    /// Sound by the original event name (`ABKSound::Core::CController::Play(name)`).
    Sound { name: String, car: Option<CarId> },
    /// Repair `fraction` (0..1) of the car's lost bodywork / reset damage counters.
    Repair { car: CarId, fraction: f32 },
    /// Escape hatch for something not covered above: tag = original function/field name, vals = its numbers.
    Other { tag: &'static str, car: Option<CarId>, vals: [f32; 4], pos: Vec3 },
}

/// A decoded `<MinLevel>/<MaxLevel>` parameter (`CBaseAbility::GetAbilityFloatForLevel`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LevelValue {
    pub min: f32,
    pub max: f32,
}

/// Game-mode ids as the eventdef `GameMode name=` strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameModeKind {
    Race,
    TimeAttack,
    SeedRush,
    BossBattle,
    BossFruitRush,
    Versus,
    Slalom,
    Jenga,
    Intro,
    Other,
}

// ------------------------------------------------------------------------------------------------------------
// Game-mode hook interface (what differs between a RACE and the other modes)
// ------------------------------------------------------------------------------------------------------------

/// Things the race/world tells a mode about (the lead translates its own events into these).
#[derive(Clone, Debug, PartialEq)]
pub enum ModeInput {
    /// A kart collected `count` seeds / fruit (`pickup_seed`, fruit pickups).
    SeedCollected { car: CarId, count: u32 },
    /// A kart crossed the finish (or completed its last lap) at race time `time`.
    KartFinished { car: CarId, time: f32 },
    /// Passed a slalom gate; `clean` = through the gap.
    GatePassed { car: CarId, gate: u32, clean: bool },
    /// A smackable was destroyed by a kart (`points` = its score value if known).
    Smashed { car: CarId, object: String },
    /// Car hit car.
    CarHitCar { attacker: CarId, victim: CarId, speed: f32 },
    /// A kart's boss-projectile / ability hit another kart.
    ProjectileHit { owner: CarId, victim: CarId, kind: String },
    /// Player requested a restart/quit etc.
    Other { tag: &'static str, car: Option<CarId>, value: f32 },
}

/// Things a mode tells the HUD / flow (the lead maps them to UI + `racerules.rs`).
#[derive(Clone, Debug, PartialEq)]
pub enum ModeEvent {
    /// Race clock / countdown the HUD should show (seconds).
    Timer { remaining: f32 },
    ScoreChanged { car: CarId, score: i64 },
    /// Mode finished; `won` for the local player.
    Finished { won: bool, score: i64, stars: u8 },
    /// Boss phase / health changed (0..1).
    BossHealth { fraction: f32 },
    Message { tag: String },
    Other { tag: &'static str, value: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeState {
    Running,
    Won,
    Lost,
    /// Finished with no win/lose distinction (e.g. time attack just produces a time).
    Done,
}

/// Common hook set of a game mode over a race (the original's `CGameMode` virtuals, see each module for addresses).
pub trait GameModeRules {
    fn kind(&self) -> GameModeKind;
    /// Per-frame update. `karts` = all karts incl. AI/bosses. Effects are requests to the host.
    fn update(&mut self, dt: f32, karts: &[KartState], effects: &mut Vec<CarEffect>, events: &mut Vec<ModeEvent>);
    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>);
    fn state(&self) -> ModeState;
    /// Score used for star thresholds / leaderboard (mode specific; time modes return milliseconds).
    fn score(&self) -> i64;
    /// Stars earned from the current result (`<Stars Star1= Star2= Star3=>` of the eventdef).
    fn stars(&self) -> u8;
}
