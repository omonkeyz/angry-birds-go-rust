//! `ItemWorld`: the in-track items of one event, built from an event definition and a track (`Stm`), updated every frame with the kart state.
//!
//! Ported pieces (see the module docs of `place`, `smackables`, `slalom`, `smackdefs`, `structures`, `eventdef`):
//!  * pickups: `CEnvObjectManager::InvokePickupsInRadius @001e22a0` (+ the per-class `IsInRadius` / `CanBePicked` / `OnCarInRadius`):
//!    a sphere of radius `2.0 + model radius` around the pickup, tested against the kart's chassis position; AI karts only trigger hotspots;
//!  * `CCar::AddCoin @001b0c38`: 1 coin (2 with the coin doubler); `CPickupMegaCoin::OnCarInRadius @001ee3b8`: 50 coins (static default of the
//!    global at 0xDAF740); `CPickupGem`: 1 gem; boost pad: sets the car's boost flag for as long as the kart overlaps it (the car applies a
//!    36000 N push along the spline, see `carsim.rs`);
//!  * seed rush tokens (`CPickupSeedRushToken @001e84f0`): fruit count, switch to coin mode when the goal is reached.
use super::envobjs::{self, ENV_OBJECTS, NO_SMACKABLE};
use super::eventdef::{self, ItemRecord, Pattern};
use super::ground::ItemGround;
use super::models::{load_model_info, ModelInfo, ModelLibrary};
use super::place::{self, SnapResult};
use super::slalom::{self, Gate, GateResult};
use super::smackables::{self, SmackState, Smackable};
use super::smackdefs;
use super::spline::{self, CSpline};
use abgtool::stm::Stm;
use glam::{Mat4, Quat, Vec3};
use std::path::{Path, PathBuf};

/// The radius `CEnvObjectManager::Update` passes to `InvokePickupsInRadius` (0x40000000 = 2.0); the kart's own radius is not used for pickups.
pub const PICKUP_RADIUS: f32 = 2.0;
/// `CPickupMegaCoin::OnCarInRadius`: coins per mega coin (static int at 0xDAF740 = 0x32).
pub const MEGA_COIN_VALUE: i32 = 50;
/// AI hotspot sizes (`CPickupAIHotspot{Small,Medium,Large,ExtraLarge}` constructors): radius in metres.
pub const HOTSPOT_RADII: [f32; 4] = [25.0, 35.0, 50.0, 65.0];
/// Pick-up fly-to-kart animation (`CPickupCoin::Update @001e7944`): the coin deactivates 0.2 s after the pick.
pub const PICK_ANIM_TIME: f32 = 0.2;
/// Seed rush coin-mode transition (`UpdateCoinTransformation @001ea8d8`): state 1 -> 2 after 0.3 s, 2 -> 3 after 0.6 s.
pub const COIN_MODE_T1: f32 = 0.3;
pub const COIN_MODE_T2: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Coin,
    MegaCoin,
    Gem,
    GiftBox,
    /// `kind` 0..2 = banana / melon / strawberry, 3..5 = wafer / lolly / ice cream (snow variant)
    SeedToken { fruit: u8 },
    SeedTokenLarge,
    BoostPad,
    /// 0 small .. 3 extra large
    AiHotspot { size: u8 },
    /// `pickup_special_item_marker`: no object of its own, a slot that is replaced by a mega coin / gift box / gem (see `replace_markers`)
    SpecialMarker,
    /// breakable / blocking env object (index into the 126-entry smackable table is in `Smackable::type_id`)
    Smackable,
    SlalomPost,
    /// helper names the original has no object for (`pickup_egg`, `pickup_token`, `tower`)
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub record: usize,
    pub helper: String,
    pub kind: ItemKind,
    pub world: Mat4,
    pub snap: SnapResult,
    /// `+0x7c` / `+0x80` of the pickup classes: still collectable
    pub active: bool,
    pub picked: bool,
    pub pick_timer: f32,
    /// index into `ItemWorld::smackables` for smackable-like items
    pub smackable: Option<usize>,
    /// slalom gate this post belongs to
    pub gate: Option<usize>,
    pub radius: f32,
    pub phase: f32,
}

impl Item {
    pub fn pos(&self) -> Vec3 {
        self.world.w_axis.truncate()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KartState {
    /// chassis position (`*(car+0x104)+0x38..0x40`)
    pub pos: Vec3,
    pub vel: Vec3,
    /// collision sphere radius used against smackables
    pub radius: f32,
    /// kart mass in the units of the smackable table (the smackable `mass` column); used for the hit response
    pub mass: f32,
    pub is_ai: bool,
    /// the kart belongs to a player (car+0x1af0 != 0)
    pub has_player: bool,
    /// `CPlayerInfo+0x420`: coin doubler active
    pub coin_doubler: bool,
    /// `car+0x42c > 0`: the kart passes through smackables except `always_collide` ones
    pub non_collide: bool,
    /// race clock for slalom (`time_left`), progress along the race spline (fractional point index); None = computed by the world
    pub time_left: f32,
    pub progress: Option<f32>,
}

impl Default for KartState {
    fn default() -> Self {
        KartState { pos: Vec3::ZERO, vel: Vec3::ZERO, radius: 1.2, mass: 1000.0, is_ai: false, has_player: true, coin_doubler: false, non_collide: false, time_left: 1.0e9, progress: None }
    }
}

#[derive(Debug, Clone)]
pub enum ItemEvent {
    CoinCollected { item: usize, value: i32 },
    MegaCoinCollected { item: usize, value: i32 },
    GemCollected { item: usize },
    GiftBoxCollected { item: usize, first_of_race: bool },
    /// a fruit / ice token; `count` is the new fruit count, `goal_reached` when this pick switched the tokens to coin mode
    TokenCollected { item: usize, fruit: u8, count: u32, goal_reached: bool },
    /// a token picked while in coin mode (state >= 2): worth one coin
    TokenCoin { item: usize, value: i32 },
    TokenLargeCollected { item: usize, count: u32 },
    /// the kart overlaps a boost pad this frame (`car+0x1be0 = 1`): the car applies its boost push while this is reported; `rising` on the first frame
    BoostPadHit { item: usize, rising: bool },
    HotspotTaken { item: usize, size: u8 },
    SmackableHit { item: usize, type_id: u32, impact: f32, smashed: bool, kart_dv: Vec3, bounty: u32, index: usize, kart_pos: Vec3 },
    SmackableSmashed { item: Option<usize>, type_id: u32, pos: Vec3, fragments: usize, explosion: f32 },
    Explosion { pos: Vec3, strength: f32 },
    GatePassed { gate: usize },
    GateMissed { gate: usize, penalty: f32 },
    /// a random power box was smashed (type 89); the power-up grant is not reachable in the decompile
    PowerBoxSmashed { pos: Vec3 },
}

/// One thing to draw: the model (path under `assets292/pak`), its world matrix and a tint.
#[derive(Debug, Clone)]
pub struct DrawInstance {
    pub model: String,
    pub world: Mat4,
    pub tint: [f32; 4],
}

/// A model + textures the world needs loaded.
#[derive(Debug, Clone)]
pub struct ModelNeed {
    pub model: String,
    pub textures: Vec<String>,
}

pub struct ItemWorld {
    pub records: Vec<ItemRecord>,
    pub items: Vec<Item>,
    pub smackables: Vec<Smackable>,
    pub gates: Vec<Gate>,
    /// records that could not be placed (missing spline / name): (record index, reason)
    pub unplaced: Vec<(usize, String)>,
    pub game_mode: String,
    pub time: f32,
    /// seed rush state: fruit count, goal (`GetTokenThreshold`, UNRESOLVED: set by the caller), coin-mode state 0..3 and its timer
    pub fruit_count: u32,
    pub token_goal: Option<u32>,
    pub coin_state: u8,
    coin_timer: f32,
    boost_last: bool,
    first_giftbox_done: bool,
    models: ModelLibrary,
    ground: ItemGround,
    splines: Vec<CSpline>,
    race_spline: Option<usize>,
    kart_progress: f32,
    assets: Option<PathBuf>,
    rng: u32,
}

fn rand_u32(state: &mut u32) -> u32 {
    // xorshift32 (the original uses CXGSRandom through a vtable; seed/sequence UNRESOLVED)
    let mut x = (*state).max(1);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// `GetPickupTypeFromHelperName @001e2000`: exact name first, then prefix match, over the 13 pickup classes.
pub fn pickup_kind_from_helper(name: &str) -> Option<ItemKind> {
    const TABLE: [(&str, ItemKind); 13] = [
        ("pickup_coin", ItemKind::Coin),
        ("pickup_megacoin", ItemKind::MegaCoin),
        ("pickup_gem", ItemKind::Gem),
        ("pickup_giftbox", ItemKind::GiftBox),
        ("pickup_seedrushtoken", ItemKind::SeedToken { fruit: 0 }),
        ("pickup_seedrushtoken_snow", ItemKind::SeedToken { fruit: 3 }),
        ("pickup_seedrushtoken_large", ItemKind::SeedTokenLarge),
        ("pickup_special_item_marker", ItemKind::SpecialMarker),
        ("boost_pad", ItemKind::BoostPad),
        ("ai_hotspot_small", ItemKind::AiHotspot { size: 0 }),
        ("ai_hotspot_medium", ItemKind::AiHotspot { size: 1 }),
        ("ai_hotspot_large", ItemKind::AiHotspot { size: 2 }),
        ("ai_hotspot_extralarge", ItemKind::AiHotspot { size: 3 }),
    ];
    if let Some((_, k)) = TABLE.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*k);
    }
    TABLE.iter().find(|(n, _)| envobjs::partial_match_nocase(name, n)).map(|(_, k)| *k)
}

fn fruit_model(f: u8) -> &'static str {
    ["fruit_banana", "fruit_melon", "fruit_strawberry", "ice_wafer", "ice_lolly", "ice_cream"][(f as usize).min(5)]
}

impl ItemWorld {
    /// Builds the item world with the assets folder found automatically (`ABG_ASSETS`, `../assets292`, `assets292`); without assets the
    /// smackables use a 2 m box and pickups the radii table of `pickup_model_radius`.
    pub fn build(eventdef_xml: &str, stm: &Stm) -> Result<ItemWorld, String> {
        Self::build_with_assets(eventdef_xml, stm, default_assets_root().as_deref())
    }

    pub fn build_with_assets(eventdef_xml: &str, stm: &Stm, assets: Option<&Path>) -> Result<ItemWorld, String> {
        let ev = eventdef::parse(eventdef_xml)?;
        let records = eventdef::expand(&ev.defs, ev.difficulty_level);
        let ground = ItemGround::build(stm);
        let splines = spline::splines_of(stm);
        let race_spline = splines.iter().position(|s| s.name.to_ascii_lowercase().starts_with("race_"));
        let mut w = ItemWorld {
            records,
            items: Vec::new(),
            smackables: Vec::new(),
            gates: Vec::new(),
            unplaced: Vec::new(),
            game_mode: ev.game_mode.clone(),
            time: 0.0,
            fruit_count: 0,
            token_goal: None,
            coin_state: 0,
            coin_timer: 0.0,
            boost_last: false,
            first_giftbox_done: false,
            models: ModelLibrary::default(),
            ground,
            splines,
            race_spline,
            kart_progress: 0.0,
            assets: assets.map(|p| p.to_path_buf()),
            rng: 0x1234_5678,
        };
        w.populate();
        Ok(w)
    }

    fn model(&mut self, sub: &str, file: &str) -> ModelInfo {
        let key = format!("{sub}/{}", file.to_ascii_lowercase());
        if let Some(m) = self.models.by_name.get(&key) {
            return m.clone();
        }
        let info = match &self.assets {
            Some(a) => load_model_info(a, sub, file),
            None => ModelInfo { rel_path: key.clone(), ..Default::default() },
        };
        self.models.by_name.insert(key, info.clone());
        info
    }

    fn populate(&mut self) {
        let mut gate_posts: Vec<(usize, usize, bool)> = Vec::new(); // (item idx, gate number, is_left)
        for ri in 0..self.records.len() {
            let r = self.records[ri].clone();
            // helper names that resolve neither to a pickup class nor an env object have no object
            let pickup = pickup_kind_from_helper(&r.helper);
            let env = if pickup.is_none() { envobjs::env_object_for_helper(&r.helper) } else { None };
            let Some(placed) = place::place(&r, &self.splines, &self.ground) else {
                self.unplaced.push((ri, format!("spline '{}' not found in the track", r.spline)));
                continue;
            };
            let mut item = Item {
                record: ri,
                helper: r.helper.clone(),
                kind: ItemKind::Unknown,
                world: placed.world,
                snap: placed.snap,
                active: true,
                picked: false,
                pick_timer: 0.0,
                smackable: None,
                gate: None,
                radius: 0.0,
                phase: 0.1 * (placed.world.w_axis.x + placed.world.w_axis.y + placed.world.w_axis.z),
            };
            if let Some(k) = pickup {
                item.kind = k;
                match k {
                    ItemKind::SeedToken { fruit } => {
                        // `CPickupSeedRushToken` ctor: random type 0..2 (snow: 3..5), never the same twice in a row
                        let base = if fruit >= 3 { 3u8 } else { 0 };
                        let mut f = (rand_u32(&mut self.rng) % 3) as u8;
                        if let Some(last) = self.items.iter().rev().find_map(|i| match i.kind {
                            ItemKind::SeedToken { fruit } if (fruit >= 3) == (base == 3) => Some(fruit - base),
                            _ => None,
                        }) {
                            if f == last {
                                f = (f + 1 + (rand_u32(&mut self.rng) % 2) as u8) % 3;
                            }
                        }
                        item.kind = ItemKind::SeedToken { fruit: base + f };
                        item.radius = self.model("envobjects", &format!("{}.xgm", fruit_model(base + f))).radius;
                    }
                    ItemKind::AiHotspot { size } => item.radius = HOTSPOT_RADII[size as usize],
                    ItemKind::Coin => item.radius = self.model("envobjects", "coin.xgm").radius,
                    ItemKind::MegaCoin => item.radius = self.model("envobjects", "megacoin.xgm").radius,
                    ItemKind::Gem => item.radius = self.model("envobjects", "diamond.xgm").radius,
                    ItemKind::GiftBox => item.radius = self.model("envobjects", "giftbox.xgm").radius,
                    ItemKind::SeedTokenLarge => item.radius = self.model("envobjects", "fruit_banana.xgm").radius,
                    ItemKind::BoostPad => item.radius = self.model("envobjects", "boost_pad.xgm").radius,
                    _ => {}
                }
                if matches!(k, ItemKind::Coin | ItemKind::MegaCoin | ItemKind::Gem | ItemKind::GiftBox | ItemKind::SeedToken { .. } | ItemKind::SeedTokenLarge | ItemKind::BoostPad)
                    && item.radius == 0.0
                {
                    item.radius = pickup_model_radius(k);
                }
            } else if let Some(ei) = env {
                let e = &ENV_OBJECTS[ei];
                if e.smackable != NO_SMACKABLE {
                    item.kind = if r.slalom.is_some() { ItemKind::SlalomPost } else { ItemKind::Smackable };
                    let idx = self.add_smackable_from_item(e.smackable, placed.world, e.w6, Some(self.items.len()), false);
                    item.smackable = Some(idx);
                }
            }
            let item_idx = self.items.len();
            if let (Some(sp), true) = (r.slalom, item.kind == ItemKind::SlalomPost) {
                gate_posts.push((item_idx, sp.gate_index as usize, r.index == 0));
            }
            self.items.push(item);
        }
        // slalom gates: two posts each (record index 0 = left, 1 = right)
        use std::collections::BTreeMap;
        let mut by_gate: BTreeMap<usize, (Option<usize>, Option<usize>)> = BTreeMap::new();
        for (i, g, left) in gate_posts {
            let e = by_gate.entry(g).or_default();
            if left {
                e.0 = Some(i);
            } else {
                e.1 = Some(i);
            }
        }
        for (g, (l, r)) in by_gate {
            let (Some(l), Some(r)) = (l, r) else { continue };
            let sp = self.records[self.items[l].record].slalom.unwrap();
            let (a, b) = (self.items[l].pos(), self.items[r].pos());
            let mid = a + (b - a) * 0.5;
            let spline_pos = self.race_spline.map_or(0.0, |s| closest_param(&self.splines[s], mid, None));
            let idx = self.gates.len();
            self.items[l].gate = Some(idx);
            self.items[r].gate = Some(idx);
            self.gates.push(Gate {
                index: g,
                penalty: sp.time_penalty,
                post_a: a,
                post_b: b,
                cols: sp.cols,
                rows: sp.rows,
                col_spacing: sp.col_spacing,
                row_spacing: sp.row_spacing,
                passed: false,
                spline_pos,
            });
        }
    }

    /// `CSmackableManager::AddSmackable`: creates a body of `type_id` at `world`.
    pub fn add_smackable_from_item(&mut self, type_id: u32, world: Mat4, _w6: bool, item: Option<usize>, temporary: bool) -> usize {
        let def = smackdefs::SMACKABLES[type_id as usize];
        let info = self.model("smackables", &format!("{}.xgm", def.name));
        let (center, half) = if info.loaded { ((info.bbox_min + info.bbox_max) * 0.5, (info.bbox_max - info.bbox_min) * 0.5) } else { (Vec3::ZERO, Vec3::splat(1.0)) };
        let fixed = item.map_or(false, |i| {
            let helper = &self.records[self.items.get(i).map_or(0, |it| it.record)].helper;
            envobjs::env_object_for_helper(helper).map_or(false, |e| ENV_OBJECTS[e].w6)
        }) || def.always_collide && !temporary;
        self.smackables.push(Smackable {
            type_id,
            world,
            pivot: info.pivot,
            bbox_center: center,
            half_extents: half,
            radius: info.radius,
            state: SmackState::Idle,
            temporary,
            fixed,
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            accum: 0.0,
            age: 0.0,
            item,
            no_gravity: false,
            scale: 1.0,
            asleep: false,
            ability_owner: None,
            explosion_override: None,
        });
        self.smackables.len() - 1
    }

    /// Spawns a smackable that is not part of the track (slalom miss wall, random power box ...).
    pub fn spawn_smackable(&mut self, type_id: u32, world: Mat4) -> usize {
        let i = self.add_smackable_from_item(type_id, world, false, None, false);
        self.smackables[i].state = SmackState::Active;
        i
    }

    /// Replaces the `pickup_special_item_marker` slots like `CGame::SetupEnvironmentMarkup @0011db98` / `GetReplaceableItemIndices @000fa658`:
    /// each slot becomes a mega coin or a gift box (coin flip), the first `free_gems` slots a gem.
    /// UNRESOLVED: the original's quotas (`GetNumSplinesForPickups`, megacoin/giftbox counts per condition) and its random source; the
    /// replacement set also includes plain coins / seed tokens, which are left as they are here.
    pub fn replace_markers(&mut self, seed: u32, free_gems: usize) {
        self.rng = seed.max(1);
        let mut gems = free_gems;
        for i in 0..self.items.len() {
            if self.items[i].kind != ItemKind::SpecialMarker {
                continue;
            }
            let kind = if gems > 0 {
                gems -= 1;
                ItemKind::Gem
            } else if rand_u32(&mut self.rng) & 1 == 0 {
                ItemKind::MegaCoin
            } else {
                ItemKind::GiftBox
            };
            let model = match kind {
                ItemKind::Gem => "diamond.xgm",
                ItemKind::MegaCoin => "megacoin.xgm",
                _ => "giftbox.xgm",
            };
            let r = self.model("envobjects", model).radius;
            self.items[i].kind = kind;
            self.items[i].radius = if r > 0.0 { r } else { pickup_model_radius(kind) };
        }
    }

    pub fn update(&mut self, dt: f32, kart: &KartState) -> Vec<ItemEvent> {
        self.update_karts(dt, std::slice::from_ref(kart))
    }

    pub fn update_karts(&mut self, dt: f32, karts: &[KartState]) -> Vec<ItemEvent> {
        let mut ev = Vec::new();
        self.time += dt;
        // seed rush coin mode timers (UpdateCoinTransformation)
        if self.coin_state == 1 || self.coin_state == 2 {
            self.coin_timer += dt;
            if self.coin_state == 1 && self.coin_timer > COIN_MODE_T1 {
                self.coin_state = 2;
            }
            if self.coin_state == 2 && self.coin_timer > COIN_MODE_T2 {
                self.coin_state = 3;
            }
        }
        let mut boost_now = false;
        for ki in 0..karts.len() {
            let k = karts[ki];
            self.update_pickups(dt, &k, &mut ev, &mut boost_now);
        }
        for k in karts.iter().filter(|k| !k.is_ai) {
            self.update_smackables(dt, k, &mut ev);
            self.update_gates(k, &mut ev);
        }
        self.boost_last = boost_now;
        ev
    }

    fn update_pickups(&mut self, dt: f32, k: &KartState, ev: &mut Vec<ItemEvent>, boost_now: &mut bool) {
        for idx in 0..self.items.len() {
            let (kind, pos, active, picked) = {
                let it = &self.items[idx];
                (it.kind, it.pos(), it.active, it.picked)
            };
            if picked {
                self.items[idx].pick_timer += dt;
                if self.items[idx].pick_timer > PICK_ANIM_TIME {
                    self.items[idx].active = false;
                }
                continue;
            }
            if !active {
                continue;
            }
            let is_pickup = !matches!(kind, ItemKind::Smackable | ItemKind::SlalomPost | ItemKind::Unknown | ItemKind::SpecialMarker);
            if !is_pickup {
                continue;
            }
            // AI karts: only hotspots (CanBePicked is false for AI on every other class); player karts never take hotspots
            match kind {
                ItemKind::AiHotspot { .. } => {
                    if !k.is_ai {
                        continue;
                    }
                }
                _ => {
                    if k.is_ai {
                        continue;
                    }
                }
            }
            let r = PICKUP_RADIUS + self.items[idx].radius;
            let r = if let ItemKind::AiHotspot { .. } = kind { self.items[idx].radius + PICKUP_RADIUS } else { r };
            if (k.pos - pos).length_squared() > r * r {
                continue;
            }
            match kind {
                ItemKind::Coin => {
                    self.pick(idx);
                    ev.push(ItemEvent::CoinCollected { item: idx, value: if k.coin_doubler { 2 } else { 1 } });
                }
                ItemKind::MegaCoin => {
                    if k.has_player {
                        self.pick(idx);
                        ev.push(ItemEvent::MegaCoinCollected { item: idx, value: MEGA_COIN_VALUE * if k.coin_doubler { 2 } else { 1 } });
                    }
                }
                ItemKind::Gem => {
                    if k.has_player {
                        self.pick(idx);
                        ev.push(ItemEvent::GemCollected { item: idx });
                    }
                }
                ItemKind::GiftBox => {
                    if k.has_player {
                        self.pick(idx);
                        let first = !self.first_giftbox_done;
                        self.first_giftbox_done = true;
                        ev.push(ItemEvent::GiftBoxCollected { item: idx, first_of_race: first });
                    }
                }
                ItemKind::SeedToken { fruit } => {
                    if k.has_player {
                        self.pick(idx);
                        if self.coin_state >= 2 {
                            ev.push(ItemEvent::TokenCoin { item: idx, value: if k.coin_doubler { 2 } else { 1 } });
                        } else {
                            self.fruit_count += 1;
                            // below state 2 the token is gone at once (no fly-to-kart)
                            self.items[idx].active = false;
                            let mut goal = false;
                            if self.token_goal.map_or(false, |g| self.fruit_count >= g) && self.coin_state == 0 {
                                self.coin_state = 1; // ShowCoins
                                self.coin_timer = 0.0;
                                goal = true;
                            }
                            ev.push(ItemEvent::TokenCollected { item: idx, fruit, count: self.fruit_count, goal_reached: goal });
                        }
                    }
                }
                ItemKind::SeedTokenLarge => {
                    if k.has_player {
                        self.pick(idx);
                        self.fruit_count += 1;
                        ev.push(ItemEvent::TokenLargeCollected { item: idx, count: self.fruit_count });
                    }
                }
                ItemKind::BoostPad => {
                    // never consumed: the flag is set while the kart overlaps the pad
                    *boost_now = true;
                    ev.push(ItemEvent::BoostPadHit { item: idx, rising: !self.boost_last });
                }
                ItemKind::AiHotspot { size } => {
                    self.items[idx].active = false; // consumed by the first AI that takes it (SetHotspotTarget accepts)
                    ev.push(ItemEvent::HotspotTaken { item: idx, size });
                }
                _ => {}
            }
        }
    }

    fn pick(&mut self, idx: usize) {
        self.items[idx].picked = true;
        self.items[idx].pick_timer = 0.0;
    }

    fn update_smackables(&mut self, dt: f32, k: &KartState, ev: &mut Vec<ItemEvent>) {
        let n = self.smackables.len();
        // activation (CEnvObject::UpdateVisibility): distance < 50 + model radius
        for i in 0..n {
            let s = &mut self.smackables[i];
            if s.state == SmackState::Idle {
                let r = smackables::ACTIVATION_RADIUS + s.radius;
                if (s.world.w_axis.truncate() - k.pos).length_squared() < r * r {
                    s.state = SmackState::Active;
                }
            }
        }
        // contacts
        let mut smash: Vec<usize> = Vec::new();
        for i in 0..n {
            if self.smackables[i].state != SmackState::Active {
                continue;
            }
            let def = *self.smackables[i].def();
            // CCar::CollisionEnabledCallback: a kart with its non-collide timer running passes through everything but AlwaysCollide bodies
            if k.non_collide && !def.always_collide {
                continue;
            }
            if self.smackables[i].ability_owner.is_some() {
                continue;
            }
            let Some(c) = smackables::kart_contact(&self.smackables[i], k.pos, k.vel, k.radius) else { continue };
            if c.closing_speed <= 0.0 {
                continue;
            }
            let counts = !def.car_only || k.has_player;
            let mag = smackables::impact_magnitude(&def, c.closing_speed);
            let fixed = self.smackables[i].fixed;
            let dv = smackables::kart_response(&def, fixed, &c, k.mass);
            if counts {
                self.smackables[i].accum += mag;
            }
            if !fixed {
                // the body takes the opposite momentum
                let push = -c.normal * (c.closing_speed * (1.0 + def.restitution) * k.mass / (k.mass + def.mass));
                self.smackables[i].vel += push;
            }
            let smashed = self.smackables[i].should_smash();
            let item = self.smackables[i].item;
            ev.push(ItemEvent::SmackableHit { item: item.unwrap_or(usize::MAX), type_id: def_type(&self.smackables[i]), impact: mag, smashed, kart_dv: dv, bounty: def.bounty, index: i, kart_pos: k.pos });
            if smashed {
                smash.push(i);
            }
        }
        for i in smash {
            self.smash(i, dt, ev);
        }
        // simple ballistic motion for loose bodies / debris (rigid-body solver not ported)
        for i in 0..self.smackables.len() {
            let s = &mut self.smackables[i];
            s.age += dt;
            if s.state != SmackState::Active || s.fixed || s.asleep || (s.vel == Vec3::ZERO && s.ang_vel == Vec3::ZERO) {
                continue;
            }
            if !s.no_gravity {
                s.vel.y += smackables::GRAVITY * dt;
            }
            let mut p = s.world.w_axis.truncate() + s.vel * dt;
            if let Some(h) = self.ground.below(p + Vec3::Y * 1.0) {
                if p.y < h.point.y {
                    p.y = h.point.y;
                    if s.vel.y < 0.0 {
                        s.vel.y *= -s.def_restitution() * 0.5;
                    }
                    s.vel.x *= 0.9;
                    s.vel.z *= 0.9;
                    if s.vel.length_squared() < 0.05 {
                        s.vel = Vec3::ZERO;
                    }
                }
            }
            s.world.w_axis = p.extend(1.0);
        }
        // temporary debris far from the kart is removed (GetSpawnUnspawn: UNRESOLVED radius; 150 m here)
        for s in self.smackables.iter_mut() {
            if s.temporary && s.state == SmackState::Active && (s.world.w_axis.truncate() - k.pos).length_squared() > 150.0 * 150.0 {
                s.state = SmackState::Smashed;
            }
        }
    }

    /// `CSmackable::Update` smash branch: fragments from the model's helper nodes, explosion, removal.
    fn smash(&mut self, i: usize, dt: f32, ev: &mut Vec<ItemEvent>) {
        if self.smackables[i].state == SmackState::Smashed {
            return;
        }
        let parent = self.smackables[i].clone();
        let def = *parent.def();
        self.smackables[i].state = SmackState::Smashed;
        let info = self.model("smackables", &format!("{}.xgm", def.name));
        let mm = parent.model_matrix();
        let mut frags = 0;
        for (name, pos) in info.nodes.clone() {
            let Some(t) = smackable_type_from_name(&name) else { continue };
            // permanent cap / temporary cap (AddSmackable): debris counts against the 40 temporary slots, oldest is dropped first
            let live_tmp = self.smackables.iter().filter(|s| s.temporary && s.state == SmackState::Active).count();
            if live_tmp >= smackables::MAX_TEMPORARY {
                if let Some(old) = self.smackables.iter_mut().filter(|s| s.temporary && s.state == SmackState::Active).max_by(|a, b| a.age.total_cmp(&b.age)) {
                    old.state = SmackState::Smashed;
                }
            }
            let wp = mm.transform_point3(pos);
            let basis = Mat4::from_cols(parent.world.x_axis, parent.world.y_axis, parent.world.z_axis, wp.extend(1.0));
            let fi = self.add_smackable_from_item(t, basis, false, None, true);
            let r = wp - parent.world.w_axis.truncate();
            self.smackables[fi].state = SmackState::Active;
            self.smackables[fi].vel = smackables::fragment_velocity(parent.vel, parent.ang_vel, r, dt);
            frags += 1;
        }
        let pos = parent.center();
        let expl = parent.explosion_override.unwrap_or(def.explosion);
        if expl > 0.0 {
            ev.push(ItemEvent::Explosion { pos, strength: expl });
            for j in 0..self.smackables.len() {
                let sj = &self.smackables[j];
                if j == i || sj.state != SmackState::Active || sj.fixed || (0x6d..=0x7c).contains(&sj.type_id) {
                    continue;
                }
                if let Some(v) = smackables::explosion_velocity(expl, sj.center() - pos) {
                    self.smackables[j].vel += v;
                }
            }
        }
        if parent.type_id == 89 {
            ev.push(ItemEvent::PowerBoxSmashed { pos });
        }
        ev.push(ItemEvent::SmackableSmashed { item: parent.item, type_id: parent.type_id, pos, fragments: frags, explosion: expl });
    }

    fn update_gates(&mut self, k: &KartState, ev: &mut Vec<ItemEvent>) {
        if self.gates.is_empty() {
            return;
        }
        let progress = match (k.progress, self.race_spline) {
            (Some(p), _) => p,
            (None, Some(s)) => {
                let hint = if self.kart_progress > 0.0 { Some(self.kart_progress) } else { None };
                let p = closest_param(&self.splines[s], k.pos, hint);
                self.kart_progress = p;
                p
            }
            _ => return,
        };
        for g in 0..self.gates.len() {
            let res = self.gates[g].evaluate(k.pos, progress, k.time_left);
            match res {
                Some(GateResult::Passed) => ev.push(ItemEvent::GatePassed { gate: g }),
                Some(GateResult::Missed { penalty }) => {
                    ev.push(ItemEvent::GateMissed { gate: g, penalty });
                    self.spawn_miss_wall(g, k, progress);
                }
                None => {}
            }
        }
    }

    /// `CGameModeSlalom::Update` miss branch: a `cols x rows` wall of smackable 125 (TNT crates) ahead of the kart (1.5 s of travel).
    fn spawn_miss_wall(&mut self, g: usize, k: &KartState, _progress: f32) {
        let Some(si) = self.race_spline else { return };
        let gate = self.gates[g].clone();
        let sp = &self.splines[si];
        let speed = k.vel.length();
        let start = closest_param(sp, k.pos, None);
        // lookahead: walk the spline `speed * 1.5` metres from the kart's point
        let mut t = start;
        let mut left = speed * slalom::TNT_LOOKAHEAD_SECONDS;
        while left > 0.0 && (t as usize) + 1 < sp.count() {
            let i = t as usize;
            let seg = sp.points[i].seg_len.max(1e-3);
            let take = (seg * (1.0 - (t - i as f32))).min(left);
            t += take / seg;
            left -= take;
        }
        let centre = sp.position(t);
        let pi = sp.clamp_index(t);
        let right = sp.points[pi].right;
        let fwd = sp.points[pi].tangent;
        let rows_half = gate.rows as f32 * 0.5 * gate.row_spacing;
        let ofs = (k.pos - centre).dot(right.normalize_or_zero());
        // lateral clamp so the wall stays on the road (widths are positive here; see slalom.rs UNRESOLVED about the left edge)
        let lat = if ofs > 0.0 { ofs.min(sp.right_width(t) - rows_half) } else { ofs.max(-(sp.left_width(t) - rows_half)) };
        for (_, _, lateral, forward) in slalom::wall_offsets(&gate) {
            let mut p = centre + right * lat + right * lateral + fwd * forward;
            let mut ok = false;
            for probe in [0.0f32, 10.0] {
                let q = p + Vec3::Y * probe;
                if let Some(h) = self.ground.below(q) {
                    if (h.point - q).length_squared() < slalom::GROUND_SNAP_DIST_SQ {
                        p = h.point;
                        ok = true;
                        break;
                    }
                }
            }
            let _ = ok;
            p.y += slalom::TNT_RAISE;
            self.spawn_smackable(slalom::MISS_WALL_SMACKABLE, Mat4::from_translation(p));
        }
    }

    /// Everything to draw this frame.
    pub fn draw_instances(&self) -> Vec<DrawInstance> {
        let mut out = Vec::new();
        let white = [1.0, 1.0, 1.0, 1.0];
        for it in &self.items {
            let t = self.time;
            let (model, extra): (Option<String>, Mat4) = match it.kind {
                ItemKind::Coin => (Some("envobjects/coin.xgm".into()), Mat4::from_rotation_y(3.0 * std::f32::consts::PI * t + it.phase)),
                ItemKind::MegaCoin => (Some("envobjects/megacoin.xgm".into()), Mat4::from_rotation_y(3.0 * std::f32::consts::PI * t + it.phase)),
                ItemKind::Gem => (Some("envobjects/diamond.xgm".into()), Mat4::from_rotation_y(3.0 * std::f32::consts::PI * t + it.phase)),
                ItemKind::GiftBox => (Some("envobjects/giftbox.xgm".into()), Mat4::from_rotation_y(1.5 * std::f32::consts::PI * t + it.phase)),
                ItemKind::SeedToken { fruit } => {
                    let m = if self.coin_state >= 2 { "coin".to_string() } else { fruit_model(fruit).to_string() };
                    (Some(format!("envobjects/{m}.xgm")), Mat4::from_rotation_y(1.5 * std::f32::consts::PI * t + it.phase))
                }
                ItemKind::SeedTokenLarge => (Some("envobjects/fruit_banana.xgm".into()), Mat4::from_rotation_y(3.0 * std::f32::consts::PI * t + it.phase)),
                ItemKind::BoostPad => (Some("envobjects/boost_pad.xgm".into()), Mat4::IDENTITY),
                _ => (None, Mat4::IDENTITY),
            };
            if let Some(model) = model {
                if it.active {
                    let mut world = it.world * extra;
                    if it.picked {
                        let s = (1.0 - it.pick_timer / PICK_ANIM_TIME).clamp(0.0, 1.0);
                        world = it.world * extra * Mat4::from_scale(Vec3::splat(s));
                    }
                    out.push(DrawInstance { model, world, tint: white });
                }
            }
        }
        for s in &self.smackables {
            if s.state == SmackState::Smashed {
                continue;
            }
            out.push(DrawInstance { model: format!("smackables/{}.xgm", s.def().name.to_ascii_lowercase()), world: s.model_matrix(), tint: white });
        }
        out
    }

    /// Models and textures the placed items need (paths under `assets292/pak`; texture names as stored in the models, converted to PNG
    /// under `assets292/textures`). Includes the fragment models of every smackable.
    pub fn required_models(&mut self) -> Vec<ModelNeed> {
        let mut names: Vec<(String, String)> = Vec::new(); // (sub, file)
        fn add(names: &mut Vec<(String, String)>, sub: &str, f: String) {
            if !names.iter().any(|(s, n)| s == sub && n == &f) {
                names.push((sub.to_string(), f));
            }
        }
        for it in &self.items {
            match it.kind {
                ItemKind::Coin => add(&mut names, "envobjects", "coin.xgm".into()),
                ItemKind::MegaCoin => add(&mut names, "envobjects", "megacoin.xgm".into()),
                ItemKind::Gem => add(&mut names, "envobjects", "diamond.xgm".into()),
                ItemKind::GiftBox => {
                    add(&mut names, "envobjects", "giftbox.xgm".into());
                    add(&mut names, "envobjects", "coin.xgm".into());
                }
                ItemKind::SeedToken { .. } => {
                    for f in 0..6u8 {
                        add(&mut names, "envobjects", format!("{}.xgm", fruit_model(f)));
                    }
                    add(&mut names, "envobjects", "coin.xgm".into());
                }
                ItemKind::SeedTokenLarge => add(&mut names, "envobjects", "fruit_banana.xgm".into()),
                ItemKind::BoostPad => add(&mut names, "envobjects", "boost_pad.xgm".into()),
                _ => {}
            }
        }
        let types: Vec<u32> = self.smackables.iter().map(|s| s.type_id).collect();
        for t in types {
            add(&mut names, "smackables", format!("{}.xgm", smackdefs::SMACKABLES[t as usize].name.to_ascii_lowercase()));
        }
        // fragments of every smackable model
        let frag_models: Vec<String> = names
            .iter()
            .filter(|(s, _)| s == "smackables")
            .flat_map(|(_, f)| {
                let info = self.model("smackables", f);
                info.nodes.iter().filter_map(|(n, _)| smackable_type_from_name(n)).map(|t| smackdefs::SMACKABLES[t as usize].name.to_ascii_lowercase()).collect::<Vec<_>>()
            })
            .collect();
        for f in frag_models {
            add(&mut names, "smackables", format!("{f}.xgm"));
        }
        names
            .into_iter()
            .map(|(sub, f)| {
                let info = self.model(&sub, &f);
                ModelNeed { model: format!("{sub}/{f}"), textures: info.textures }
            })
            .collect()
    }

    /// counts per helper name of placed items (diagnostics)
    pub fn count_by_helper(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for it in &self.items {
            *m.entry(it.helper.clone()).or_default() += 1;
        }
        m
    }
}

fn def_type(s: &Smackable) -> u32 {
    s.type_id
}

impl Smackable {
    fn def_restitution(&self) -> f32 {
        self.def().restitution
    }
}

/// `GetSmackableTypeFromHelperName @001d9a20`: hash match first, then case-insensitive substring; here the exact name, then substring.
pub fn smackable_type_from_name(name: &str) -> Option<u32> {
    if let Some(i) = smackdefs::SMACKABLES.iter().position(|d| d.name.eq_ignore_ascii_case(name)) {
        return Some(i as u32);
    }
    let l = name.to_ascii_lowercase();
    smackdefs::SMACKABLES.iter().position(|d| l.contains(&d.name.to_ascii_lowercase())).map(|i| i as u32)
}

/// Half-diagonal radii of the pickup models (`CXGSModel+0xCC`), measured from the shipped .xgm files; used when no assets folder is available.
pub fn pickup_model_radius(k: ItemKind) -> f32 {
    match k {
        ItemKind::Coin => 1.428,
        ItemKind::MegaCoin => 2.852,
        ItemKind::Gem => 2.205,
        ItemKind::GiftBox => 2.746,
        ItemKind::SeedToken { .. } | ItemKind::SeedTokenLarge => 1.485,
        ItemKind::BoostPad => 3.537,
        _ => 0.0,
    }
}

pub fn default_assets_root() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("ABG_ASSETS") {
        c.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut d = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..6 {
            if let Some(dir) = d {
                c.push(dir.join("assets292"));
                d = dir.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }
    c.push(PathBuf::from("assets292"));
    c.push(PathBuf::from("../assets292"));
    c.into_iter().find(|p| p.join("pak").is_dir())
}

/// Closest point of the spline to `p` as a fractional point index (`CSpline::GetClosestPos`-like); `hint` restricts the search to a window of
/// 40 points around the previous result (the kart's progress cannot jump).
pub fn closest_param(sp: &CSpline, p: Vec3, hint: Option<f32>) -> f32 {
    let n = sp.count();
    if n < 2 {
        return 0.0;
    }
    let (lo, hi) = match hint {
        Some(h) => ((h as i64 - 40).max(0) as usize, ((h as i64 + 40) as usize).min(n - 2)),
        None => (0, n - 2),
    };
    let mut best = (f32::MAX, 0.0f32);
    for i in lo..=hi {
        let a = sp.points[i].pos;
        let b = sp.points[i + 1].pos;
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        let d = (a + ab * t - p).length_squared();
        if d < best.0 {
            best = (d, i as f32 + t);
        }
    }
    best.1
}

#[allow(dead_code)]
fn _unused(_: Quat, _: Pattern) {}

impl ItemWorld {
    #[doc(hidden)]
    pub fn race_progress_for_test(&self) -> Option<usize> {
        self.race_spline
    }
}

impl ItemWorld {
    /// Nearest collision surface below `p`: (point, normal).
    pub fn ground_below(&self, p: Vec3) -> Option<(Vec3, Vec3)> {
        self.ground.below(p).map(|h| (h.point, h.normal))
    }

    /// `CSmackable::Explode(power, centre)`: every loose smackable gets the explosion impulse of `smackables::explosion_velocity`
    /// (`ApplyExplodeForce @001d4ff0`; route blockers / fixed bodies are skipped).
    pub fn explode(&mut self, center: Vec3, power: f32) {
        for s in self.smackables.iter_mut() {
            if s.state != SmackState::Active || s.fixed || (0x6d..=0x7c).contains(&s.type_id) {
                continue;
            }
            if let Some(v) = smackables::explosion_velocity(power, s.center() - center) {
                s.vel += v;
                s.asleep = false;
            }
        }
    }

    /// Smash smackable `i` now (fragments, explosion force, events) - the host uses it for ability objects.
    pub fn smash_index(&mut self, i: usize, dt: f32) -> Vec<ItemEvent> {
        let mut ev = Vec::new();
        self.smash(i, dt, &mut ev);
        ev
    }
}

impl ItemWorld {
    /// The model file + textures of smackable type `name` (for objects spawned at run time, e.g. by abilities).
    pub fn smackable_model_need(&mut self, name: &str) -> ModelNeed {
        let f = format!("{}.xgm", name.to_ascii_lowercase());
        let info = self.model("smackables", &f);
        ModelNeed { model: format!("smackables/{f}"), textures: info.textures }
    }
}
