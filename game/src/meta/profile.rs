//! Player profile / save model (`CPlayerInfo`, `CEnergySystem` state, token store) and its text persistence.
//!
//! The original stores most counters XOR-obfuscated with 0x3e5ab9c (`*(uint*)(this+0x404) ^ 0x3e5ab9c` ...) in an XML save
//! (`CSaveManager`); this port keeps plain integers and writes a simple `key=value` text file next to the exe.
//!
//! Field map (CPlayerInfo offsets, `SetDefaults @00149998`, `ResetAllData @0014a880`):
//!  +0x404 coins, +0x408 gems, +0x40c lifetime coins, +0x410 lifetime gems (XOR), +0x9968 / +0x996c XP (campaign / upgrade XP, XOR; rank
//!  uses the sum), +0x9904/+0x9908 kart state array (0x48 bytes each), +0x9910 selected kart, +0x9920 last kart used per theme,
//!  +0x992c campaign state array (0x18 bytes each), +0x138/+0x13c (+0x28*char) character level / xp, +0x9970 pending rank popup.

use super::data::*;
use std::collections::BTreeMap;

/// Wallet limit: `AddSoftCurrency @0014c6ac` clamps to 999999999.
pub const WALLET_MAX: i32 = 999_999_999;

/// `CEnergySystem` + `CABKEnergyGameState` save data (`CEnergySystem::SaveData @000f45c0`:
/// rechargeCostReset, numRechargesToday, energyFullTimeStamp, maxEnergyIncrease, excessEnergy, lastEnergy).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnergyState {
    /// `energyFullTimeStamp` (CEnergySystem +0x18): unix seconds at which the energy bar is full again (<= now: full)
    pub full_timestamp: u64,
    /// `maxEnergyIncrease` (game state +0xc)
    pub max_increase: i32,
    /// `excessEnergy` (game state +0x10): energy above the maximum (gifts), spent first
    pub excess: i32,
    /// `lastEnergy` (game state +0x20)
    pub last_energy: i32,
    /// `numRechargesToday` (CEnergySystem +0xc)
    pub recharges_today: i32,
    /// `rechargeCostReset` (CEnergySystem +0x10)
    pub recharge_cost_reset: u64,
}

/// Per kart state (0x48 bytes at +0x9904, built by `SetupKartStates @0015075c`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KartState {
    /// +0xc : 0 not owned, 1 owned, 2 (UnlockKart/PurchaseKart mode 10)
    pub owned: i32,
    /// +0x10 : unlock popup pending
    pub unlock_popup: bool,
    /// +0x14 : current tier
    pub tier: i32,
    /// +0x18..+0x28 : cumulative stat levels (index into the tier-concatenated modifier list, `GetKartStat`)
    pub levels: [i32; 5],
    /// +0x2c..+0x3c : levels bought inside the current tier (reset by `UpTierKart`)
    pub tier_levels: [i32; 5],
}

/// `CPlayerInfo` campaign state (0x18 bytes at +0x992c).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CampaignState {
    pub tag: String,
    /// +8 played
    pub played: bool,
    /// +0xc completed
    pub completed: bool,
    /// +0x10 best score
    pub best_score: i32,
    /// +0x14 best stars
    pub best_stars: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharState {
    pub unlocked: bool,
    /// +0x13c
    pub xp: i32,
    /// +0x138 (1 based, `GetLevelFromXP`)
    pub level: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub notifications: bool,
    pub motion_controls: bool,
}
impl Default for Settings {
    fn default() -> Settings {
        // economy.xml DefaultControlSystem MotionControlEnabled=false
        // UNRESOLVED: original default volumes (CSettings) were not located; 1.0 used.
        Settings { music_volume: 1.0, sfx_volume: 1.0, notifications: true, motion_controls: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub version: u32,
    pub created: u64,
    pub last_saved: u64,
    pub coins: i32,
    pub gems: i32,
    pub lifetime_coins: i32,
    pub lifetime_gems: i32,
    /// +0x9968
    pub xp_campaign: i32,
    /// +0x996c
    pub xp_upgrade: i32,
    pub energy: EnergyState,
    /// token id (`SSKM0001`, `BLUE0001`, `GACH0000` ...) -> count (`CTokenManager`)
    pub tokens: BTreeMap<String, i32>,
    /// one per `MetaData.karts` entry (same order, keyed by base id for the save file)
    pub karts: Vec<(String, KartState)>,
    pub selected_kart: String,
    /// last kart used per theme index (`+0x9920`)
    pub last_kart_used: Vec<String>,
    pub characters: BTreeMap<String, CharState>,
    pub selected_character: String,
    pub campaign: Vec<CampaignState>,
    /// `UnlockAll` => every episode; otherwise filled by `UnlockEpisode`
    pub unlocked_episodes: Vec<bool>,
    /// achievement tracked values (`CAchievementsManager::OnModifyTrackedValue`)
    pub tracked: BTreeMap<String, f32>,
    pub achievements_done: Vec<String>,
    pub powerups: BTreeMap<String, i32>,
    pub features: BTreeMap<String, bool>,
    pub settings: Settings,
    pub pending_rank_popup: bool,
    pub ftue_done: Vec<String>,
    pub parts_shop_bought: BTreeMap<String, i32>,
    pub last_ad_spin: u64,
    pub daily_race_claimed: BTreeMap<String, i32>,
    pub extra_ability_uses: i32,
    pub coin_doubler: bool,
    pub gacha_ftue_given: bool,
    pub events_won: i32,
    pub total_races: i32,
}

pub const PROFILE_VERSION: u32 = 1;

impl Profile {
    /// First-run state, as `CPlayerInfo::SetDefaults / ResetAllData / SetupKartStates / ResetCampaignProgress` build it:
    /// wallet = economy `StartingCoins` / `StartingGems`, energy full at rank 0, the FTUE kart owned + selected, the FTUE character unlocked,
    /// all campaign states empty, all episodes unlocked (economy `EpisodeUnlocking/UnlockAll`).
    pub fn new_default(data: &MetaData, now: u64) -> Profile {
        let eco = &data.economy;
        let mut p = Profile {
            version: PROFILE_VERSION,
            created: now,
            last_saved: now,
            coins: eco.starting_coins,
            gems: eco.starting_gems,
            lifetime_coins: eco.starting_coins,
            lifetime_gems: eco.starting_gems,
            xp_campaign: 0,
            xp_upgrade: 0,
            energy: EnergyState::default(),
            tokens: BTreeMap::new(),
            karts: data.karts.iter().map(|k| (k.base_id.clone(), KartState::default())).collect(),
            selected_kart: if eco.ftue_kart.is_empty() { "SSKM".into() } else { eco.ftue_kart.clone() },
            last_kart_used: vec![String::new(); 5],
            characters: BTreeMap::new(),
            selected_character: String::new(),
            campaign: data.events.campaign.iter().map(|c| CampaignState { tag: c.tag.clone(), ..Default::default() }).collect(),
            unlocked_episodes: vec![eco.unlock_all_episodes; 5],
            tracked: BTreeMap::new(),
            achievements_done: vec![],
            powerups: BTreeMap::new(),
            features: BTreeMap::new(),
            settings: Settings::default(),
            pending_rank_popup: false,
            ftue_done: vec![],
            parts_shop_bought: BTreeMap::new(),
            last_ad_spin: 0,
            daily_race_claimed: BTreeMap::new(),
            extra_ability_uses: 0,
            coin_doubler: false,
            gacha_ftue_given: false,
            events_won: 0,
            total_races: 0,
        };
        // FTUE kart owned (SetupKartStates @0015075c sets +0xc = 1 for the kart whose tag == FTUE kart)
        let fk = p.selected_kart.clone();
        if let Some((_, ks)) = p.karts.iter_mut().find(|(id, _)| *id == fk) {
            ks.owned = 1;
        }
        // FTUE character
        let ch = if eco.ftue_character.is_empty() { "red".to_string() } else { eco.ftue_character.to_ascii_lowercase() };
        p.characters.insert(ch.clone(), CharState { unlocked: true, xp: 0, level: 1 });
        p.selected_character = ch;
        // energy: full at rank 0. The base maximum is `StartingAmount`, the rank table gives the real maximum.
        if let (Some(e), Some(r0)) = (&data.energy, data.ranks.first()) {
            p.energy.max_increase = r0.max_energy - e.starting_amount;
        }
        p
    }

    pub fn kart_state(&self, id: &str) -> Option<&KartState> {
        self.karts.iter().find(|(k, _)| k == id).map(|(_, s)| s)
    }
    pub fn kart_state_mut(&mut self, id: &str) -> Option<&mut KartState> {
        self.karts.iter_mut().find(|(k, _)| k == id).map(|(_, s)| s)
    }
    pub fn token(&self, id: &str) -> i32 {
        self.tokens.get(id).copied().unwrap_or(0)
    }
    pub fn xp_total(&self) -> i32 {
        self.xp_campaign + self.xp_upgrade
    }

    // ----------------------------------------------------------------------------------------------------------
    // persistence: simple `key=value` text, one record per line
    // ----------------------------------------------------------------------------------------------------------

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let mut w = |k: &str, v: String| {
            s.push_str(k);
            s.push('=');
            s.push_str(&v);
            s.push('\n');
        };
        w("ABGPROFILE", self.version.to_string());
        w("created", self.created.to_string());
        w("last_saved", self.last_saved.to_string());
        w("coins", self.coins.to_string());
        w("gems", self.gems.to_string());
        w("lifetime_coins", self.lifetime_coins.to_string());
        w("lifetime_gems", self.lifetime_gems.to_string());
        w("xp_campaign", self.xp_campaign.to_string());
        w("xp_upgrade", self.xp_upgrade.to_string());
        let e = &self.energy;
        w("energy", format!("{},{},{},{},{},{}", e.full_timestamp, e.max_increase, e.excess, e.last_energy, e.recharges_today, e.recharge_cost_reset));
        for (k, v) in &self.tokens {
            w(&format!("tok.{k}"), v.to_string());
        }
        for (id, k) in &self.karts {
            let j = |a: &[i32; 5]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
            w(&format!("kart.{id}"), format!("{},{},{},{},{}", k.owned, k.unlock_popup as i32, k.tier, j(&k.levels), j(&k.tier_levels)));
        }
        w("selected_kart", self.selected_kart.clone());
        w("last_kart_used", self.last_kart_used.join(","));
        for (n, c) in &self.characters {
            w(&format!("char.{n}"), format!("{},{},{}", c.unlocked as i32, c.xp, c.level));
        }
        w("selected_character", self.selected_character.clone());
        for (i, c) in self.campaign.iter().enumerate() {
            w(&format!("camp.{i}"), format!("{},{},{},{},{}", c.tag, c.played as i32, c.completed as i32, c.best_score, c.best_stars));
        }
        w("episodes", self.unlocked_episodes.iter().map(|b| (*b as i32).to_string()).collect::<Vec<_>>().join(","));
        for (k, v) in &self.tracked {
            w(&format!("track.{k}"), v.to_string());
        }
        w("achievements_done", self.achievements_done.join(","));
        for (k, v) in &self.powerups {
            w(&format!("powerup.{k}"), v.to_string());
        }
        for (k, v) in &self.features {
            w(&format!("feature.{k}"), (*v as i32).to_string());
        }
        let st = &self.settings;
        w("settings", format!("{},{},{},{}", st.music_volume, st.sfx_volume, st.notifications as i32, st.motion_controls as i32));
        w("pending_rank_popup", (self.pending_rank_popup as i32).to_string());
        w("ftue_done", self.ftue_done.join(","));
        for (k, v) in &self.parts_shop_bought {
            w(&format!("partsbought.{k}"), v.to_string());
        }
        w("last_ad_spin", self.last_ad_spin.to_string());
        for (k, v) in &self.daily_race_claimed {
            w(&format!("dailyclaim.{k}"), v.to_string());
        }
        w("extra_ability_uses", self.extra_ability_uses.to_string());
        w("coin_doubler", (self.coin_doubler as i32).to_string());
        w("gacha_ftue_given", (self.gacha_ftue_given as i32).to_string());
        w("events_won", self.events_won.to_string());
        w("total_races", self.total_races.to_string());
        s
    }

    /// Parse a save. Unknown keys are ignored; missing keys keep the first-run defaults of `base`.
    pub fn from_text(text: &str, base: Profile) -> Result<Profile, String> {
        let mut p = base;
        let mut seen_header = false;
        let ints = |s: &str| -> Vec<i64> { s.split(',').filter(|x| !x.is_empty()).map(|x| x.trim().parse::<i64>().unwrap_or(0)).collect() };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "ABGPROFILE" => {
                    seen_header = true;
                    p.version = v.parse().unwrap_or(PROFILE_VERSION);
                }
                "created" => p.created = v.parse().unwrap_or(p.created),
                "last_saved" => p.last_saved = v.parse().unwrap_or(p.last_saved),
                "coins" => p.coins = v.parse().unwrap_or(p.coins),
                "gems" => p.gems = v.parse().unwrap_or(p.gems),
                "lifetime_coins" => p.lifetime_coins = v.parse().unwrap_or(p.lifetime_coins),
                "lifetime_gems" => p.lifetime_gems = v.parse().unwrap_or(p.lifetime_gems),
                "xp_campaign" => p.xp_campaign = v.parse().unwrap_or(0),
                "xp_upgrade" => p.xp_upgrade = v.parse().unwrap_or(0),
                "energy" => {
                    let a = ints(v);
                    if a.len() >= 6 {
                        p.energy = EnergyState {
                            full_timestamp: a[0] as u64,
                            max_increase: a[1] as i32,
                            excess: a[2] as i32,
                            last_energy: a[3] as i32,
                            recharges_today: a[4] as i32,
                            recharge_cost_reset: a[5] as u64,
                        };
                    }
                }
                "selected_kart" => p.selected_kart = v.to_string(),
                "last_kart_used" => p.last_kart_used = v.split(',').map(|s| s.to_string()).collect(),
                "selected_character" => p.selected_character = v.to_string(),
                "episodes" => p.unlocked_episodes = ints(v).iter().map(|x| *x != 0).collect(),
                "achievements_done" => p.achievements_done = v.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "settings" => {
                    let f: Vec<&str> = v.split(',').collect();
                    if f.len() >= 4 {
                        p.settings = Settings {
                            music_volume: f[0].parse().unwrap_or(1.0),
                            sfx_volume: f[1].parse().unwrap_or(1.0),
                            notifications: f[2] == "1",
                            motion_controls: f[3] == "1",
                        };
                    }
                }
                "pending_rank_popup" => p.pending_rank_popup = v == "1",
                "ftue_done" => p.ftue_done = v.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "last_ad_spin" => p.last_ad_spin = v.parse().unwrap_or(0),
                "extra_ability_uses" => p.extra_ability_uses = v.parse().unwrap_or(0),
                "coin_doubler" => p.coin_doubler = v == "1",
                "gacha_ftue_given" => p.gacha_ftue_given = v == "1",
                "events_won" => p.events_won = v.parse().unwrap_or(0),
                "total_races" => p.total_races = v.parse().unwrap_or(0),
                _ => {
                    if let Some(id) = k.strip_prefix("tok.") {
                        p.tokens.insert(id.to_string(), v.parse().unwrap_or(0));
                    } else if let Some(id) = k.strip_prefix("kart.") {
                        let a = ints(v);
                        if a.len() >= 13 {
                            let ks = KartState {
                                owned: a[0] as i32,
                                unlock_popup: a[1] != 0,
                                tier: a[2] as i32,
                                levels: [a[3] as i32, a[4] as i32, a[5] as i32, a[6] as i32, a[7] as i32],
                                tier_levels: [a[8] as i32, a[9] as i32, a[10] as i32, a[11] as i32, a[12] as i32],
                            };
                            if let Some(slot) = p.kart_state_mut(id) {
                                *slot = ks;
                            } else {
                                p.karts.push((id.to_string(), ks));
                            }
                        }
                    } else if let Some(n) = k.strip_prefix("char.") {
                        let a = ints(v);
                        if a.len() >= 3 {
                            p.characters.insert(n.to_string(), CharState { unlocked: a[0] != 0, xp: a[1] as i32, level: a[2] as i32 });
                        }
                    } else if let Some(i) = k.strip_prefix("camp.") {
                        let f: Vec<&str> = v.split(',').collect();
                        if let (Ok(i), true) = (i.parse::<usize>(), f.len() >= 5) {
                            let cs = CampaignState {
                                tag: f[0].to_string(),
                                played: f[1] == "1",
                                completed: f[2] == "1",
                                best_score: f[3].parse().unwrap_or(0),
                                best_stars: f[4].parse().unwrap_or(0),
                            };
                            if i < p.campaign.len() {
                                p.campaign[i] = cs;
                            } else {
                                p.campaign.push(cs);
                            }
                        }
                    } else if let Some(n) = k.strip_prefix("track.") {
                        p.tracked.insert(n.to_string(), v.parse().unwrap_or(0.0));
                    } else if let Some(n) = k.strip_prefix("powerup.") {
                        p.powerups.insert(n.to_string(), v.parse().unwrap_or(0));
                    } else if let Some(n) = k.strip_prefix("feature.") {
                        p.features.insert(n.to_string(), v == "1");
                    } else if let Some(n) = k.strip_prefix("partsbought.") {
                        p.parts_shop_bought.insert(n.to_string(), v.parse().unwrap_or(0));
                    } else if let Some(n) = k.strip_prefix("dailyclaim.") {
                        p.daily_race_claimed.insert(n.to_string(), v.parse().unwrap_or(0));
                    }
                }
            }
        }
        if !seen_header {
            return Err("not an ABG profile (missing ABGPROFILE header)".into());
        }
        Ok(p)
    }
}
