//! META GAME: definition data, profile / save model, rules and a small query API for the UI flow.
//!
//! Standalone module (depends only on `roxmltree` + std). Everything comes from the original definition files
//! (`assets292/xml/...`) and the 2.9.1 decompile; every rule function names its original (`CPlayerInfo::...`, `CMetagameManager::...`,
//! `CEnergySystem::...`, `CKartManager::...`, `CGachaManager::...`) with its Ghidra address. Gaps are marked `// UNRESOLVED:`.
//!
//! Layout:
//!  * `meta/data.rs`    - loaders of ranklist, energy, metagame, kart definitions + upgrade levels, event definitions, campaign map,
//!                        gacha pools, economy, character levelling, achievements, challenges, daily races, tournaments ...
//!  * `meta/store.rs`   - store.pak definitions (currency shop, bundles, parts shop, offers)
//!  * `meta/profile.rs` - `Profile` (wallet, XP, energy timers, karts + upgrade levels, characters, campaign stars ...) and its text save
//!  * `meta/rules.rs`   - rank / XP, energy, wallet, kart stats / CC / upgrade / purchase, campaign stars + rewards, `mod_spec`
//!  * `meta/gacha.rs`   - toolboxes (weighted pools, `lrand48`), coin substitution, daily race rewards, shop (gems / coins only)
//!  * `meta/rng.rs`     - `lrand48`
//!
//! UI entry points (what the screens' data fields need): `Meta::topbar`, `Meta::map_event`, `Meta::map_events`, `Meta::kart_view`,
//! `Meta::kart_list`, plus the actions `start_campaign_event` / `complete_campaign_event` / `upgrade_stat` / `upgrade_tier` /
//! `purchase_kart` / `buy_ticket_spins` / `buy_shop_item` / `refill_energy_with_gems` and `meta::mod_spec(kart)` for the car physics.

#![allow(dead_code)]

pub mod data;
pub mod gacha;
pub mod profile;
pub mod rng;
pub mod rules;
pub mod store;

pub use data::{MetaData, Reward, Stat};
pub use gacha::Prize;
pub use profile::Profile;
pub use rules::{DifficultyAdjust, EventOutcome, MetaError, ModSpec, RankUp, UpgradeResult};

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Name of the save file written next to the exe.
pub const SAVE_FILE_NAME: &str = "abg_profile.sav";

/// Definition data + the player's profile + a random source.
pub struct Meta {
    pub data: MetaData,
    pub profile: Profile,
    pub rng: rng::Lrand48,
    pub save_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------------------------------------------
// view structs for the UI
// ---------------------------------------------------------------------------------------------------------------

/// Top bar: coins / gems / energy / rank / xp bar.
#[derive(Clone, Debug, PartialEq)]
pub struct TopBar {
    pub coins: i32,
    pub gems: i32,
    pub energy: i32,
    pub energy_max: i32,
    /// seconds until the next energy unit (`GetTimeUntilNextRecharge`)
    pub energy_next_secs: i64,
    pub energy_full_secs: i64,
    /// displayed rank (1 based)
    pub rank: i32,
    pub xp_total: i32,
    pub rank_min_xp: i32,
    pub rank_max_xp: i32,
    /// (xp - rankMin) / (rankMax - rankMin), 0..1
    // UNRESOLVED: the xp bar fill formula of CTopBarRender was not read; this is the plain fraction inside the rank.
    pub xp_fraction: f32,
    pub tickets: i32,
    pub blueprints: i32,
    pub pending_rank_popup: bool,
}

/// A campaign map marker (`campaignmapdefinition.xml` EventMarker campaignIndex).
#[derive(Clone, Debug, PartialEq)]
pub struct MapEvent {
    pub index: usize,
    pub tag: String,
    pub event_index: i32,
    pub cc: i32,
    pub energy_cost: i32,
    pub unlocked: bool,
    pub played: bool,
    pub completed: bool,
    pub stars: i32,
    pub best_score: i32,
    pub star_scores: [i32; 3],
    pub rewards: Vec<Reward>,
    pub hidden: bool,
    pub difficulty: DifficultyAdjust,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KartStatView {
    pub stat: Stat,
    /// cumulative level (`CKartData` +0x18+4*stat)
    pub level: i32,
    pub level_in_tier: i32,
    pub levels_in_tier: i32,
    /// the modifier (ratio) currently applied for this stat
    pub modifier: f32,
    /// next upgrade: (token id, tokens, coins)
    pub next: Option<(String, i32, i32)>,
    pub can_upgrade: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KartView {
    pub id: String,
    pub name_key: String,
    pub rarity: String,
    pub theme_index: i32,
    pub owned: bool,
    pub tier: i32,
    pub tier_count: i32,
    pub stars: i32,
    pub visual_model: String,
    pub cc: i32,
    pub unlock_rank: i32,
    pub unlock_cost: i32,
    pub rank_unlocked: bool,
    pub can_purchase: bool,
    pub can_upgrade_tier: bool,
    pub tier_token_cost: i32,
    pub stats: Vec<KartStatView>,
    pub mod_spec: ModSpec,
}

// ---------------------------------------------------------------------------------------------------------------
// construction / persistence
// ---------------------------------------------------------------------------------------------------------------

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Find `assets292`: env `ABG_ASSETS`, else look in the exe dir / current dir and their parents.
pub fn find_assets_root() -> Option<PathBuf> {
    let ok = |p: &Path| p.join("xml/xml/global/ranklist.xml").exists();
    if let Ok(v) = std::env::var("ABG_ASSETS") {
        let p = PathBuf::from(v);
        if ok(&p) {
            return Some(p);
        }
    }
    let mut starts: Vec<PathBuf> = vec![];
    if let Ok(e) = std::env::current_exe() {
        if let Some(d) = e.parent() {
            starts.push(d.to_path_buf());
        }
    }
    if let Ok(c) = std::env::current_dir() {
        starts.push(c);
    }
    for s in starts {
        let mut cur: Option<&Path> = Some(&s);
        while let Some(d) = cur {
            let cand = d.join("assets292");
            if ok(&cand) {
                return Some(cand);
            }
            cur = d.parent();
        }
    }
    None
}

/// Default save location: next to the exe.
pub fn default_save_path() -> PathBuf {
    if let Some(p) = std::env::var_os("ABG_SAVE") {
        return PathBuf::from(p);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(SAVE_FILE_NAME))).unwrap_or_else(|| PathBuf::from(SAVE_FILE_NAME))
}

impl Meta {
    /// Load the definition data from `root` (= `assets292`) and the profile from `save_path` (first-run defaults when the file is absent
    /// or unreadable; an unreadable file is kept as `<save>.bad`).
    pub fn open(root: &Path, save_path: Option<PathBuf>, now: u64) -> Result<Meta, String> {
        let data = MetaData::load(root)?;
        let base = Profile::new_default(&data, now);
        let mut profile = base.clone();
        if let Some(p) = &save_path {
            if let Ok(text) = std::fs::read_to_string(p) {
                match Profile::from_text(&text, base) {
                    Ok(pr) => profile = pr,
                    Err(_) => {
                        let _ = std::fs::rename(p, p.with_extension("sav.bad"));
                    }
                }
            }
        }
        let mut m = Meta { data, profile, rng: rng::Lrand48::new((now as u32) ^ 0x5eed), save_path };
        m.fix_up(now);
        Ok(m)
    }

    /// Fresh profile (first run), no save file.
    pub fn new_game(data: MetaData, now: u64) -> Meta {
        let profile = Profile::new_default(&data, now);
        Meta { data, profile, rng: rng::Lrand48::new((now as u32) ^ 0x5eed), save_path: None }
    }

    /// Keep the profile consistent with the definitions (kart list, campaign length, episode flags).
    fn fix_up(&mut self, now: u64) {
        for k in &self.data.karts {
            if self.profile.kart_state(&k.base_id).is_none() {
                self.profile.karts.push((k.base_id.clone(), profile::KartState::default()));
            }
        }
        while self.profile.campaign.len() < self.data.events.campaign.len() {
            let i = self.profile.campaign.len();
            self.profile.campaign.push(profile::CampaignState { tag: self.data.events.campaign[i].tag.clone(), ..Default::default() });
        }
        if self.profile.unlocked_episodes.len() < 5 {
            self.profile.unlocked_episodes.resize(5, self.data.economy.unlock_all_episodes);
        }
        if self.data.economy.unlock_all_episodes {
            for e in &mut self.profile.unlocked_episodes {
                *e = true;
            }
        }
        // `CPlayerInfo::UpdateMaxEnergy`: the maximum follows the rank (no refill)
        let _ = now;
        let m = self.rank_max_energy(self.rank());
        if m > 0 {
            let cur = self.energy_max();
            if cur != m {
                self.profile.energy.max_increase += m - cur;
            }
        }
    }

    /// Write the profile (atomic: temp file + rename).
    pub fn save(&mut self, now: u64) -> std::io::Result<()> {
        let Some(p) = self.save_path.clone() else { return Ok(()) };
        self.profile.last_saved = now;
        let tmp = p.with_extension("sav.tmp");
        std::fs::write(&tmp, self.profile.to_text())?;
        std::fs::rename(&tmp, &p)
    }

    // ---------------------------------------------------------------------------------------------------
    // UI queries
    // ---------------------------------------------------------------------------------------------------

    pub fn topbar(&self, now: u64) -> TopBar {
        let rank = self.rank();
        let min = self.rank_min_xp(rank);
        let max = self.rank_max_xp(rank);
        let xp = self.profile.xp_total();
        let span = (max - min).max(1) as f32;
        TopBar {
            coins: self.profile.coins,
            gems: self.profile.gems,
            energy: self.energy_level(now),
            energy_max: self.energy_max(),
            energy_next_secs: self.energy_time_until_next(now),
            energy_full_secs: self.energy_seconds_to_full(now),
            rank: rank + 1,
            xp_total: xp,
            rank_min_xp: min,
            rank_max_xp: max,
            xp_fraction: ((xp - min) as f32 / span).clamp(0.0, 1.0),
            tickets: self.profile.token(if self.data.gacha.token_type.is_empty() { "GACH0000" } else { &self.data.gacha.token_type }),
            blueprints: self.profile.token("BLUE0001"),
            pending_rank_popup: self.profile.pending_rank_popup,
        }
    }

    /// One campaign event by campaign index (the `campaignIndex` of the map markers).
    pub fn map_event(&self, index: usize) -> Option<MapEvent> {
        let ev = self.data.events.campaign.get(index)?;
        let st = self.profile.campaign.get(index).cloned().unwrap_or_default();
        let kart_cc = self.kart_cc(&self.profile.selected_kart);
        Some(MapEvent {
            index,
            tag: ev.tag.clone(),
            event_index: ev.event_index,
            cc: ev.campaign_cc,
            energy_cost: ev.energy_cost,
            unlocked: self.is_event_unlocked(index),
            played: st.played,
            completed: st.completed,
            stars: st.best_stars,
            best_score: st.best_score,
            star_scores: self.star_scores(ev.campaign_cc),
            rewards: ev.rewards.clone(),
            hidden: ev.hidden,
            difficulty: self.difficulty_adjust(ev.campaign_cc, kart_cc),
        })
    }
    pub fn map_events(&self) -> Vec<MapEvent> {
        (0..self.campaign_len()).filter_map(|i| self.map_event(i)).collect()
    }

    /// Kart select / garage data for one kart.
    pub fn kart_view(&self, id: &str) -> Option<KartView> {
        let def = self.data.kart(id)?;
        let st = self.profile.kart_state(id).cloned().unwrap_or_default();
        let tier = st.tier.max(0) as usize;
        let mut stats = Vec::new();
        for s in Stat::ALL {
            let level = st.levels[s.idx()];
            let next = self.next_stat_level(id, s).map(|(lv, coin)| (Self::upgrade_token_id(id, lv.rarity), lv.cost_tokens, coin));
            stats.push(KartStatView {
                stat: s,
                level,
                level_in_tier: st.tier_levels[s.idx()],
                levels_in_tier: self.max_level_for_tier(id, s, tier),
                modifier: self.kart_stat_modifier(id, s, level),
                next,
                can_upgrade: st.owned == 1 && self.can_upgrade_stat(id, s, true, true),
            });
        }
        Some(KartView {
            id: id.to_string(),
            name_key: def.base_name.clone(),
            rarity: def.rarity_name.clone(),
            theme_index: def.theme_index,
            owned: st.owned == 1,
            tier: st.tier,
            tier_count: def.tiers.len() as i32,
            stars: def.tiers.get(tier).map(|t| t.stars).unwrap_or(0),
            visual_model: def.tiers.get(tier).map(|t| t.visual_model.clone()).unwrap_or_default(),
            cc: self.kart_cc(id),
            unlock_rank: def.unlock_rank,
            unlock_cost: def.unlock_cost,
            rank_unlocked: self.kart_rank_unlocked(id),
            can_purchase: st.owned != 1 && self.kart_rank_unlocked(id) && self.profile.token("BLUE0001") >= def.unlock_cost,
            can_upgrade_tier: self.can_upgrade_tier(id, true),
            tier_token_cost: def.tiers.get(tier).map(|t| t.token_cost).unwrap_or(0),
            stats,
            mod_spec: self.mod_spec(id),
        })
    }
    /// All karts of a theme/episode (index 0..4) in definition order; power-up karts (`isPowerUpKart`) excluded.
    pub fn kart_list(&self, theme_index: i32) -> Vec<KartView> {
        self.data.karts.iter().filter(|k| k.theme_index == theme_index && !k.is_power_up_kart).filter_map(|k| self.kart_view(&k.base_id)).collect()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// process wide instance (for code that only needs a read, e.g. the car spec)
// ---------------------------------------------------------------------------------------------------------------

static GLOBAL: OnceLock<Mutex<Option<Meta>>> = OnceLock::new();

/// Run `f` on the process wide `Meta` (lazily opened from `find_assets_root()` + `default_save_path()`); `None` if the assets can't be found.
pub fn with_global<R>(f: impl FnOnce(&mut Meta) -> R) -> Option<R> {
    let cell = GLOBAL.get_or_init(|| {
        let m = find_assets_root().and_then(|r| Meta::open(&r, Some(default_save_path()), now_secs()).ok());
        Mutex::new(m)
    });
    let mut g = cell.lock().ok()?;
    g.as_mut().map(f)
}

/// `(grip, fragility, thrust, drag, sliding_ang_vel, min_speed)` for `kart_id` from the global profile - replaces `drive.rs::mods_at_level_zero`.
/// Field order = carsim.rs `CModSpec`. `None` if the data could not be loaded.
pub fn mod_spec_global(kart_id: &str) -> Option<(f32, f32, f32, f32, f32, f32)> {
    with_global(|m| m.mod_spec(kart_id).tuple())
}

/// `fn mod_spec(&Profile, kart_id)` of the task: `(grip, fragility, thrust, drag, sliding_ang_vel, min_speed)`; needs the definition data
/// for the modifier tables.
pub fn mod_spec(profile: &Profile, data: &MetaData, kart_id: &str) -> (f32, f32, f32, f32, f32, f32) {
    rules::mod_spec(profile, data, kart_id).tuple()
}

#[cfg(test)]
mod tests;
