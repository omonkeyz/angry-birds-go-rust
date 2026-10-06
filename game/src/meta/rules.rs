//! Meta game rules ported from the 2.9.1 decompile (`libABK291`, Ghidra addresses; every function names its original).
//!
//! Original classes: `CMetagameManager` (rank table, scores, difficulty), `CPlayerInfo` (wallet, XP, karts, campaign),
//! `CKartManager` (kart stats / CC), `CEnergySystem` + `CABKEnergyGameState` (energy), `CTokenManager`.
//!
//! Everything takes an explicit `now` (unix seconds, `ITime`): offline there is no server time, the caller passes the system clock.

use super::data::*;
use super::profile::*;
use super::Meta;

/// Result of a rank change (`CPlayerInfo::AddXP @00150db4` -> `DoRankRewards @00151e9c`).
#[derive(Clone, Debug, PartialEq)]
pub struct RankUp {
    pub from: i32,
    pub to: i32,
    pub new_max_energy: i32,
    pub rewards: Vec<Reward>,
    /// karts whose `unlockRank == to + 1` (`AddXP` schedules a `KartUnlock_<id>` special offer for them)
    pub karts_unlocked: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetaError {
    NotEnoughEnergy,
    NotEnoughCoins { missing: i32 },
    NotEnoughGems { missing: i32 },
    MissingTokens { token: String, have: i32, need: i32 },
    MaxLevelForTier,
    NotOwned,
    AlreadyOwned,
    UnknownKart(String),
    UnknownEvent(i32),
    Locked(String),
    NotAvailableOffline(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpgradeResult {
    pub stat: Stat,
    pub new_level: i32,
    pub tokens_spent: (String, i32),
    pub coins_spent: i32,
    pub xp_gained: i32,
    pub rank_up: Option<RankUp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventOutcome {
    pub new_stars: i32,
    pub first_clear: bool,
    pub rewards: Vec<Reward>,
    pub rank_up: Option<RankUp>,
}

/// The six `CModSpec` ratios (`CModSpec + 0x10..0x24`) consumed by `CCarSpec::CopyWithMods @001b5700`, plus the character level ratio at +0xc.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ModSpec {
    pub grip: f32,            // +0x10  <- GetKartStat(Grip=4)
    pub fragility: f32,       // +0x14  <- GetKartStat(Strength=2)
    pub thrust: f32,          // +0x18  <- GetKartStat(Acceleration=1)
    pub drag: f32,            // +0x1c  <- GetKartStat(TopSpeed=0)
    pub sliding_ang_vel: f32, // +0x20  <- GetKartStat(Handling=3)
    /// +0x24: never written by `CGame::AddCar @0011a6c4` (nor by the ghost-car filler @0011bf10): stays at the `CModSpec::CModSpec @001b7480` value 0.
    // UNRESOLVED: no writer of CModSpec+0x24 was found in the decompile; if one exists it is not one of the kart-stat paths.
    pub min_speed: f32,
    /// +0xc: `(characterLevel - 1) / 11` (`CGame::AddCar`), not consumed by `CopyWithMods`.
    pub character_level_ratio: f32,
}
impl ModSpec {
    /// `(grip, fragility, thrust, drag, sliding_ang_vel, min_speed)` in carsim.rs `CModSpec` field order.
    pub fn tuple(&self) -> (f32, f32, f32, f32, f32, f32) {
        (self.grip, self.fragility, self.thrust, self.drag, self.sliding_ang_vel, self.min_speed)
    }
}

/// `CMetagameManager::GetDifficultyAdjust @0012df90` result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyAdjust {
    VeryEasy = 0,
    Easy = 1,
    Medium = 2,
    Hard = 3,
    Extreme = 4,
}

impl Meta {
    // ======================================================================================================
    // ranks / XP  (CMetagameManager @0012dd48..0012de44, CPlayerInfo::AddXP @00150db4)
    // ======================================================================================================

    /// `CMetagameManager::GetRank @0012dd48`: index of the first rank with `min <= xp <= max`, 0 if none.
    pub fn rank_for_xp(&self, xp: i32) -> i32 {
        for (i, r) in self.data.ranks.iter().enumerate() {
            if r.min_xp <= xp && xp <= r.max_xp {
                return i as i32;
            }
        }
        0
    }
    /// `GetRankMinXP @0012dd90`
    pub fn rank_min_xp(&self, rank: i32) -> i32 {
        self.data.ranks.get(rank as usize).filter(|_| rank >= 0).map(|r| r.min_xp).unwrap_or(0)
    }
    /// `GetRankMaxXP @0012ddc0`
    pub fn rank_max_xp(&self, rank: i32) -> i32 {
        self.data.ranks.get(rank as usize).filter(|_| rank >= 0).map(|r| r.max_xp).unwrap_or(0)
    }
    /// `GetRankMaxEnergy @0012de10`
    pub fn rank_max_energy(&self, rank: i32) -> i32 {
        self.data.ranks.get(rank as usize).filter(|_| rank >= 0).map(|r| r.max_energy).unwrap_or(0)
    }
    /// `GetRankRewards @0012de78` / `GetNoofRankRewards @0012de44`
    pub fn rank_rewards(&self, rank: i32) -> Vec<Reward> {
        self.data.ranks.get(rank as usize).filter(|_| rank >= 0).map(|r| r.rewards.clone()).unwrap_or_default()
    }
    /// `GetMaxXP @0012ddf4`: max xp of the last rank
    pub fn max_xp(&self) -> i32 {
        self.data.ranks.last().map(|r| r.max_xp).unwrap_or(0)
    }
    /// `CPlayerInfo::GetRank @00151cec`: rank index (0 based) from campaign XP + upgrade XP.
    pub fn rank(&self) -> i32 {
        self.rank_for_xp(self.profile.xp_total())
    }
    /// The number the UI shows (index + 1). Kart `unlockRank`, `CGachaManager::GetRandomPrize` (`rank + 1`) use this scale.
    pub fn rank_display(&self) -> i32 {
        self.rank() + 1
    }

    /// `CPlayerInfo::AddXP(int amount, int source) @00150db4`. `source` 0 adds to the campaign counter (+0x9968), anything else to the
    /// upgrade counter (+0x996c). The amount is clamped to the XP still missing for the maximum.
    /// On a rank change: pending rank popup, `CEnergySystem::SetMaxEnergy(rankMaxEnergy, 1, 1)` (the bar is refilled) and the rank rewards.
    // UNRESOLVED: the reward visitor object (IVisitor at DAT_00151f88+8) that `AddXP` runs over each rank reward was not identified; it is
    // assumed to grant the reward (as `Type::IVisitor::Visit` of the other award paths does).
    pub fn add_xp(&mut self, amount: i32, source: i32, now: u64) -> Option<RankUp> {
        let total = self.profile.xp_total();
        let old_rank = self.rank_for_xp(total);
        let room = (self.max_xp() - total).max(0);
        let amount = amount.min(room);
        if source == 0 {
            self.profile.xp_campaign += amount;
            if self.profile.xp_campaign < 0 {
                self.profile.xp_campaign = 0;
            }
        } else {
            self.profile.xp_upgrade += amount;
            if self.profile.xp_upgrade < 0 {
                self.profile.xp_upgrade = 0;
            }
        }
        let new_rank = self.rank_for_xp(self.profile.xp_total());
        if new_rank == old_rank {
            return None;
        }
        self.profile.pending_rank_popup = true;
        let max_e = self.rank_max_energy(new_rank);
        self.set_max_energy(max_e, true, now);
        let rewards = self.rank_rewards(new_rank);
        for r in &rewards {
            self.apply_reward(r, now);
        }
        let karts_unlocked = self.data.karts.iter().filter(|k| k.unlock_rank == new_rank + 1).map(|k| k.base_id.clone()).collect();
        Some(RankUp { from: old_rank, to: new_rank, new_max_energy: max_e, rewards, karts_unlocked })
    }

    /// `CPlayerInfo::SetHasSeenRankPopup @00151d88`
    pub fn set_has_seen_rank_popup(&mut self) {
        self.profile.pending_rank_popup = false;
    }
    /// `CPlayerInfo::ResetKartUpgradeXP @00151c7c`: zero the +0x996c counter.
    pub fn reset_kart_upgrade_xp(&mut self) {
        self.profile.xp_upgrade = 0;
    }
    /// `CPlayerInfo::UpdateMaxEnergy @00152108`: set the maximum to the rank maximum without refilling.
    pub fn update_max_energy(&mut self, now: u64) {
        let m = self.rank_max_energy(self.rank());
        self.set_max_energy(m, false, now);
    }

    // ======================================================================================================
    // energy  (CEnergySystem @000f4464.., CABKEnergyGameState @000d8bd8..)
    // ======================================================================================================

    fn energy_cfg(&self) -> (i32, i64) {
        let e = self.data.energy.as_ref();
        (e.map(|e| e.starting_amount).unwrap_or(20), e.map(|e| e.recharge_secs).unwrap_or(300).max(1))
    }
    /// `CABKEnergyGameState::GetMaxEnergy @000d8bf0`: base (`StartingAmount`, +4) + max increase (+0xc)
    pub fn energy_max(&self) -> i32 {
        self.energy_cfg().0 + self.profile.energy.max_increase
    }
    /// `CEnergySystem::GetEnergyLevel @000f4878`: when `now >= energyFullTimeStamp` the bar is full (`max + excess`); otherwise
    /// `max - ((fullTimeStamp - 1 - now + R) / R) + excess` where R = seconds per unit.
    pub fn energy_level(&self, now: u64) -> i32 {
        let (_, r) = self.energy_cfg();
        let e = &self.profile.energy;
        let max = self.energy_max();
        if now >= e.full_timestamp {
            max + e.excess
        } else {
            let t = e.full_timestamp as i64;
            let missing = (r + (t - 1 - now as i64)) / r;
            max - missing as i32 + e.excess
        }
    }
    /// `CABKEnergyGameState::GetEnergyRechargeTimePerUnit` = `energy.xml RechargeTimePerBlock`
    // UNRESOLVED: the decompile of GetEnergyRechargeTimePerUnit @000d8bf4 is empty (returns r0); RechargeTimePerBlock is the only seconds value in energy.xml.
    pub fn energy_recharge_secs(&self) -> i64 {
        self.energy_cfg().1
    }
    /// `CEnergySystem::SpendEnergy(int) @000f46a8`: fails if the level is below `n`; the excess energy is spent first, the rest moves the
    /// full timestamp forward by `R * (n - spentFromExcess)` (starting from `now` if the bar was full).
    pub fn spend_energy(&mut self, n: i32, now: u64) -> bool {
        let (_, r) = self.energy_cfg();
        if n > self.energy_level(now) {
            return false;
        }
        let e = &mut self.profile.energy;
        let spent_excess = if e.excess < 1 {
            0
        } else {
            let s = n.min(e.excess);
            e.excess -= s; // SpendExcessEnergy @000d8c20
            s
        };
        let base = if now >= e.full_timestamp { now } else { e.full_timestamp };
        e.full_timestamp = base + (r as u64) * (n - spent_excess) as u64;
        true
    }
    /// `CEnergySystem::AddEnergy @000f49f0`: one unit; at full it goes to the excess counter (returns false).
    pub fn add_energy(&mut self, now: u64) -> bool {
        let (_, r) = self.energy_cfg();
        if self.energy_level(now) >= self.energy_max() {
            self.profile.energy.excess += 1;
            return false;
        }
        let e = &mut self.profile.energy;
        let base = if now >= e.full_timestamp { now } else { e.full_timestamp };
        e.full_timestamp = base.saturating_sub(r as u64);
        true
    }
    /// `CEnergySystem::GetTimeUntilNextRecharge @000f4b8c`: `(fullTimeStamp - now) mod R` while recharging, else 0.
    pub fn energy_time_until_next(&self, now: u64) -> i64 {
        let (_, r) = self.energy_cfg();
        let t = self.profile.energy.full_timestamp;
        if t > now {
            ((t - now) as i64) % r
        } else {
            0
        }
    }
    /// Seconds until the bar is full (`GetTimeOfFullCharge @000f5034` returns the timestamp).
    pub fn energy_seconds_to_full(&self, now: u64) -> i64 {
        self.profile.energy.full_timestamp.saturating_sub(now) as i64
    }
    /// `CEnergySystem::GetRechargeCost @000f4c24` with the values `CreateEnergySystem @000d9248` installs
    /// (+0x20 base = +0x28 cap = game state +0x14, +0x24 increment = 0): a flat gem price while the bar is not full.
    // UNRESOLVED: game state +0x14 (the value copied to the cost fields) is assumed to be `GemRefillCost` of energy.xml (LoadFromXML @000d8f.. not read).
    pub fn energy_refill_cost(&self, now: u64) -> i32 {
        if self.energy_level(now) >= self.energy_max() {
            return 0;
        }
        self.data.energy.as_ref().map(|e| e.gem_refill_cost).unwrap_or(0)
    }
    /// Buy a full refill with gems: `CEnergySystem::RechargeEnergy @000f4464` (count++ , `rechargeCostReset = now + 86400` on the first,
    /// `energyFullTimeStamp = now`) after paying `GetRechargeCost` with `SpendHardCurrency`.
    pub fn refill_energy_with_gems(&mut self, now: u64) -> Result<(), MetaError> {
        let cost = self.energy_refill_cost(now);
        if cost == 0 {
            return Ok(());
        }
        if !self.spend_gems(cost) {
            return Err(MetaError::NotEnoughGems { missing: cost - self.profile.gems });
        }
        let e = &mut self.profile.energy;
        if e.recharges_today < 1 {
            e.recharge_cost_reset = now + 0x15180;
        }
        e.recharges_today += 1;
        e.full_timestamp = now;
        Ok(())
    }
    /// `CEnergySystem::SetMaxEnergy(new, p2, refill) @000f4f28`: nothing if unchanged; otherwise the game state's increase moves by the
    /// difference and (when `refill`) the bar is filled (`energyFullTimeStamp = now`).
    pub fn set_max_energy(&mut self, new_max: i32, refill: bool, now: u64) {
        let cur = self.energy_max();
        if new_max == cur {
            return;
        }
        self.profile.energy.max_increase += new_max - cur;
        if refill {
            self.profile.energy.full_timestamp = now;
        }
    }

    // ======================================================================================================
    // wallet  (CPlayerInfo::AddSoftCurrency @0014c6ac, SpendSoftCurrency @0014c9f0, SpendHardCurrency @0014ca38)
    // ======================================================================================================

    pub fn add_coins(&mut self, n: i32) {
        let p = &mut self.profile;
        // AddSoftCurrency @0014c6ac: clamp at 999999999 (also lifetime counter)
        p.coins = (p.coins as i64 + n as i64).clamp(0, WALLET_MAX as i64) as i32;
        p.lifetime_coins = (p.lifetime_coins as i64 + n as i64).clamp(0, WALLET_MAX as i64) as i32;
    }
    pub fn add_gems(&mut self, n: i32) {
        let p = &mut self.profile;
        let v = p.gems as i64 + n as i64;
        p.gems = v.clamp(0, WALLET_MAX as i64) as i32;
        let l = p.lifetime_gems as i64 + n as i64;
        p.lifetime_gems = l.clamp(0, WALLET_MAX as i64) as i32;
    }
    /// `HasEnoughSoftCurrency @0014c9d0`
    pub fn has_enough_coins(&self, n: i32) -> bool {
        self.profile.coins - n >= 0
    }
    /// `SpendSoftCurrency @0014c9f0`
    pub fn spend_coins(&mut self, n: i32) -> bool {
        if self.profile.coins - n < 0 {
            return false;
        }
        self.profile.coins -= n;
        true
    }
    /// `SpendHardCurrency @0014ca38`
    pub fn spend_gems(&mut self, n: i32) -> bool {
        if self.profile.gems - n < 0 {
            return false;
        }
        self.profile.gems -= n;
        true
    }

    // ======================================================================================================
    // tokens + generic reward award
    // ======================================================================================================

    pub fn add_tokens(&mut self, id: &str, n: i32) {
        let e = self.profile.tokens.entry(id.to_string()).or_insert(0);
        *e = (*e + n).max(0);
    }
    pub fn token_count(&self, id: &str) -> i32 {
        self.profile.token(id)
    }

    /// Grant one reward entry (`Type::CType` visited by the award visitor): Currency Coins/Gems/Energy, Token, Statistic XP, Character,
    /// Kart, Powerup, Feature/Durable.
    pub fn apply_reward(&mut self, r: &Reward, now: u64) {
        match (r.kind.as_str(), r.sub.as_str()) {
            ("Currency", "Coins") => self.add_coins(r.quantity),
            ("Currency", "Gems") => self.add_gems(r.quantity),
            ("Currency", "Energy") => {
                for _ in 0..r.quantity.max(0) {
                    self.add_energy(now);
                }
            }
            ("Token", id) => self.add_tokens(id, r.quantity),
            ("Statistic", "XP") => {
                self.add_xp(r.quantity, 0, now);
            }
            ("Character", name) => self.unlock_character(name),
            ("Kart", id) => {
                let _ = self.unlock_kart(id);
            }
            ("Powerup", id) | ("Durable", id) | ("Feature", id) => {
                *self.profile.powerups.entry(id.to_string()).or_insert(0) += r.quantity.max(1);
            }
            _ => {}
        }
    }

    /// `CPlayerInfo::UnlockCharacter(char*) @0014eeac`
    pub fn unlock_character(&mut self, name: &str) {
        let key = name.to_ascii_lowercase();
        let c = self.profile.characters.entry(key).or_insert(CharState { unlocked: false, xp: 0, level: 1 });
        c.unlocked = true;
        if c.level < 1 {
            c.level = 1;
        }
    }

    // ======================================================================================================
    // character levelling  (CMetagameManager::GetLevelFromXP @0012d418, GetProgressWithinLevel @0012d4c0, CPlayerInfo::AddCharacterXP @0014c5b4)
    // ======================================================================================================

    /// level = 1 below the first threshold, else the level of the highest threshold reached (`GetLevelFromXP`)
    pub fn character_level_from_xp(&self, xp: i32) -> i32 {
        let mut lvl = 1;
        for (txp, tl) in &self.data.char_levelling.thresholds {
            if xp >= *txp {
                lvl = *tl;
            }
        }
        lvl
    }
    /// `CPlayerInfo::AddCharacterXP @0014c5b4`
    pub fn add_character_xp(&mut self, name: &str, xp: i32) {
        let key = name.to_ascii_lowercase();
        let lvl_xp = {
            let c = self.profile.characters.entry(key.clone()).or_insert(CharState { unlocked: false, xp: 0, level: 1 });
            c.xp += xp;
            c.xp
        };
        let lvl = self.character_level_from_xp(lvl_xp);
        if let Some(c) = self.profile.characters.get_mut(&key) {
            c.level = lvl;
        }
    }
    /// `(level - 1) / 11` : `CModSpec +0xc` (`CGame::AddCar @0011a6c4`; 11 = max level 12 - 1)
    pub fn character_level_ratio(&self) -> f32 {
        let lvl = self.profile.characters.get(&self.profile.selected_character).map(|c| c.level.max(1)).unwrap_or(1);
        (lvl - 1) as f32 / 11.0
    }

    // ======================================================================================================
    // karts: stats, CC, upgrade, purchase  (CKartManager::GetKartStat @00126110, GetKartCC @00125c64, CPlayerInfo::UpgradeKart @0015139c ...)
    // ======================================================================================================

    /// Number of levels of `stat` inside `tier` (`CKartManager::GetMaxLevelForTier @001265e8`: tier +0x30 + stat*0x14).
    pub fn max_level_for_tier(&self, kart: &str, stat: Stat, tier: usize) -> i32 {
        self.data.upgrade_levels.get(kart).and_then(|t| t.get(tier)).map(|t| t.stats[stat.idx()].len() as i32).unwrap_or(0)
    }

    /// Locate (tier, index-in-tier) for a cumulative level: `CKartManager::GetKartStat @00126110`.
    pub fn level_to_tier(&self, kart: &str, stat: Stat, level: i32) -> Option<(usize, usize)> {
        let tiers = self.data.upgrade_levels.get(kart)?;
        if tiers.is_empty() || level < 0 {
            return None;
        }
        let mut before = 0i32;
        for (t, tl) in tiers.iter().enumerate() {
            let n = tl.stats[stat.idx()].len() as i32;
            if level < before + n {
                return Some((t, (level - before) as usize));
            }
            before += n;
        }
        // the original indexes one tier past the end (out of range); clamp to the last entry
        let last = tiers.len() - 1;
        let n = tiers[last].stats[stat.idx()].len();
        if n == 0 {
            None
        } else {
            Some((last, n - 1))
        }
    }

    /// `CKartManager::GetKartStat(stat, nameTag, level)`: the `Modifier` at cumulative level `level` (0.0 for an unknown kart).
    pub fn kart_stat_modifier(&self, kart: &str, stat: Stat, level: i32) -> f32 {
        let Some((t, i)) = self.level_to_tier(kart, stat, level) else { return 0.0 };
        self.data.upgrade_levels[kart][t].stats[stat.idx()][i].modifier
    }

    /// The six ratios `CGame::AddCar @0011a6c4` stores into the `CModSpec` handed to `CCar::CCar` (-> `CCarSpec::CopyWithMods`):
    /// `+0x10 = GetKartStat(Grip)`, `+0x14 = Strength`, `+0x18 = Acceleration`, `+0x1c = TopSpeed`, `+0x20 = Handling`, `+0x24` unset (0).
    pub fn mod_spec(&self, kart: &str) -> ModSpec {
        mod_spec(&self.profile, &self.data, kart)
    }

    /// `CKartManager::GetKartCC @00125c64`: `baseCC + sum over stats and tiers 0..=currentTier of tier.ccIncrease[stat] * clamp(level + 1 - levelsInEarlierTiers, 0, levelsInTier)`.
    pub fn kart_cc(&self, kart: &str) -> i32 {
        let Some(def) = self.data.kart(kart) else { return -1 };
        let Some(st) = self.profile.kart_state(kart) else { return def.base_cc };
        let Some(tiers) = self.data.upgrade_levels.get(kart) else { return def.base_cc };
        let mut cc = 0i32;
        for s in Stat::ALL {
            let mut before = 0i32;
            for t in 0..=(st.tier.max(0) as usize) {
                let Some(tl) = tiers.get(t) else { break };
                let count = tl.stats[s.idx()].len() as i32;
                let v = (1 - before + st.levels[s.idx()]).clamp(0, count);
                let inc = def.tiers.get(t).map(|x| x.cc_increase[s.idx()]).unwrap_or(0);
                cc += inc * v;
                before += count;
            }
        }
        cc + def.base_cc
    }

    /// `CMetagameManager::GetCoinCostForKartUpgrade @0012dcf4`: the entry's `Coins`, or when 0: `(index + 1) * tier.ccIncrease[stat] * 5 + 100`.
    pub fn coin_cost_for_kart_upgrade(&self, kart: &str, stat: Stat, index_in_tier: usize, tier: usize) -> i32 {
        let Some(tl) = self.data.upgrade_levels.get(kart).and_then(|t| t.get(tier)) else { return 0 };
        let Some(lv) = tl.stats[stat.idx()].get(index_in_tier) else { return 0 };
        if lv.coins != 0 {
            return lv.coins;
        }
        let inc = self.data.kart(kart).and_then(|k| k.tiers.get(tier)).map(|t| t.cc_increase[stat.idx()]).unwrap_or(0);
        (index_in_tier as i32 + 1) * inc * 5 + 100
    }

    /// The next purchasable level entry of `stat` (index `levelInTier + 1` of the current tier, as `UpgradeKart` reads it).
    pub fn next_stat_level(&self, kart: &str, stat: Stat) -> Option<(StatLevel, i32)> {
        let st = self.profile.kart_state(kart)?;
        let tl = self.data.upgrade_levels.get(kart)?.get(st.tier.max(0) as usize)?;
        let idx = (st.tier_levels[stat.idx()] + 1) as usize;
        let lv = *tl.stats[stat.idx()].get(idx)?;
        Some((lv, self.coin_cost_for_kart_upgrade(kart, stat, idx, st.tier as usize)))
    }

    /// Token id used by an upgrade entry: `sprintf("%s%04i", kartId, rarity + 1)` e.g. `SSKM0002`.
    pub fn upgrade_token_id(kart: &str, rarity: i32) -> String {
        format!("{kart}{:04}", rarity + 1)
    }

    /// `CPlayerInfo::CanUpgradeKartStat(kart, stat, checkTokens, checkCoins) @00152ec0`
    pub fn can_upgrade_stat(&self, kart: &str, stat: Stat, check_cost: bool, check_coins: bool) -> bool {
        let Some(st) = self.profile.kart_state(kart) else { return false };
        let Some(tl) = self.data.upgrade_levels.get(kart).and_then(|t| t.get(st.tier.max(0) as usize)) else { return false };
        let count = tl.stats[stat.idx()].len() as i32;
        if count - 1 <= st.tier_levels[stat.idx()] {
            return false;
        }
        if check_cost {
            let Some((lv, coin)) = self.next_stat_level(kart, stat) else { return false };
            if self.profile.token(&Self::upgrade_token_id(kart, lv.rarity)) < lv.cost_tokens {
                return false;
            }
            if check_coins && self.profile.coins < coin {
                return false;
            }
        }
        true
    }

    /// `CPlayerInfo::UpgradeKart(kart, stat, ...) @0015139c`. Needs the kart's tokens and coins; bumps both the cumulative and the in-tier
    /// level, then `AddXP(TXP::GetAmountGained(kartRarity, partRarity), 1)`.
    pub fn upgrade_stat(&mut self, kart: &str, stat: Stat, now: u64) -> Result<UpgradeResult, MetaError> {
        let def = self.data.kart(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?.clone();
        let st = self.profile.kart_state(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?.clone();
        let tl = self.data.upgrade_levels.get(kart).and_then(|t| t.get(st.tier.max(0) as usize)).ok_or(MetaError::MaxLevelForTier)?;
        let count = tl.stats[stat.idx()].len() as i32;
        if count <= st.tier_levels[stat.idx()] + 1 {
            return Err(MetaError::MaxLevelForTier); // POPUP_MAX_LEVEL_FOR_TIER
        }
        let (lv, coin) = self.next_stat_level(kart, stat).ok_or(MetaError::MaxLevelForTier)?;
        let tok = Self::upgrade_token_id(kart, lv.rarity);
        let have = self.profile.token(&tok);
        if have < lv.cost_tokens {
            return Err(MetaError::MissingTokens { token: tok, have, need: lv.cost_tokens });
        }
        if self.profile.coins < coin {
            return Err(MetaError::NotEnoughCoins { missing: coin - self.profile.coins });
        }
        self.add_tokens(&tok, -lv.cost_tokens);
        self.profile.coins -= coin;
        {
            let s = self.profile.kart_state_mut(kart).unwrap();
            s.levels[stat.idx()] += 1;
            s.tier_levels[stat.idx()] += 1;
        }
        let xp = self.xp_amount_gained(def.rarity, lv.rarity);
        let rank_up = self.add_xp(xp, 1, now);
        if matches!(kart, "SSKM" | "BBUG" | "HGDR" | "MMBL") {
            self.track(&format!("{kart}Upgrades"), true);
        }
        Ok(UpgradeResult { stat, new_level: self.profile.kart_state(kart).unwrap().levels[stat.idx()], tokens_spent: (tok, lv.cost_tokens), coins_spent: coin, xp_gained: xp, rank_up })
    }

    /// `MetagameData::TXP::GetAmountGained @0020b404`: product of every matching multiplier (wildcards: kart rarity 4, part rarity 3), start 1.
    pub fn xp_amount_gained(&self, kart_rarity: i32, part_rarity: i32) -> i32 {
        let mut v = 1;
        for u in &self.data.metagame.xp {
            if (kart_rarity == 4 || u.kart_rarity == kart_rarity || u.kart_rarity == 4) && (part_rarity == 3 || u.part_rarity == part_rarity || u.part_rarity == 3) {
                v *= u.multiplier;
            }
        }
        v
    }

    /// `CPlayerInfo::CanUpgradeTier(kart, check) @0015252c`: owned, not the last tier, and every stat maxed inside the current tier;
    /// with `check` also needs the tier's blueprint tokens (BLUE0001) and `GetCoinCostForTierUpgrade @0012dd40` (500) coins.
    pub fn can_upgrade_tier(&self, kart: &str, check_cost: bool) -> bool {
        let Some(def) = self.data.kart(kart) else { return false };
        let Some(st) = self.profile.kart_state(kart) else { return false };
        if st.owned != 1 || st.tier == def.tiers.len() as i32 - 1 {
            return false;
        }
        for s in Stat::ALL {
            if st.tier_levels[s.idx()] < self.max_level_for_tier(kart, s, st.tier as usize) - 1 {
                return false;
            }
        }
        if check_cost {
            let need = def.tiers[st.tier as usize].token_cost;
            if self.profile.token("BLUE0001") < need || self.profile.coins < 500 {
                return false;
            }
        }
        true
    }

    /// `CPlayerInfo::UpTierKart @00151964`: pays `tier.tokenCost` BLUE0001 tokens; an owned kart moves to the next tier and its in-tier levels reset
    /// (the +0xc != 1 branch of the original marks the kart owned instead - use `purchase_kart` for that).
    // UNRESOLVED: UpTierKart does not charge the 500 coins `CanUpgradeTier` checks (GetCoinCostForTierUpgrade); nothing in UpTierKart spends coins.
    pub fn upgrade_tier(&mut self, kart: &str, now: u64) -> Result<(), MetaError> {
        let def = self.data.kart(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?.clone();
        let st = self.profile.kart_state(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?.clone();
        let need = def.tiers.get(st.tier.max(0) as usize).map(|t| t.token_cost).unwrap_or(0);
        let have = self.profile.token("BLUE0001");
        if have < need {
            return Err(MetaError::MissingTokens { token: "BLUE0001".into(), have, need });
        }
        self.add_tokens("BLUE0001", -need);
        self.add_xp(0, 1, now);
        let s = self.profile.kart_state_mut(kart).unwrap();
        if s.owned == 1 {
            s.tier += 1;
            s.tier_levels = [0; 5];
        } else {
            s.owned = 1;
        }
        Ok(())
    }

    /// `CPlayerInfo::PurchaseKart @0014d1f0`: costs `unlockCost` BLUE0001 blueprint tokens, marks the kart owned (+0xc = 1) and records it as the
    /// last kart used for its theme if none is set.
    pub fn purchase_kart(&mut self, kart: &str) -> Result<(), MetaError> {
        let def = self.data.kart(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?.clone();
        let have = self.profile.token("BLUE0001");
        if have < def.unlock_cost {
            return Err(MetaError::MissingTokens { token: "BLUE0001".into(), have, need: def.unlock_cost });
        }
        self.add_tokens("BLUE0001", -def.unlock_cost);
        self.unlock_kart(kart)
    }

    /// `CPlayerInfo::UnlockKart @0014d610`: free unlock (rewards, gacha, ranks).
    pub fn unlock_kart(&mut self, kart: &str) -> Result<(), MetaError> {
        let def = self.data.kart(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?.clone();
        let theme = def.theme_index.max(0) as usize;
        if self.profile.last_kart_used.len() <= theme {
            self.profile.last_kart_used.resize(theme + 1, String::new());
        }
        if self.profile.last_kart_used[theme].is_empty() {
            self.profile.last_kart_used[theme] = kart.to_string();
        }
        let s = self.profile.kart_state_mut(kart).ok_or_else(|| MetaError::UnknownKart(kart.into()))?;
        s.owned = 1;
        s.unlock_popup = true;
        self.track("KartsUnlocked", true);
        self.track("UnlockKart", true);
        Ok(())
    }

    /// `CPlayerInfo::IsKartOwned @0014d974`
    pub fn is_kart_owned(&self, kart: &str) -> bool {
        self.profile.kart_state(kart).map(|s| s.owned == 1).unwrap_or(false)
    }
    /// `CPlayerInfo::GetNumberKartsOwned @001508c4`
    pub fn karts_owned(&self) -> usize {
        self.profile.karts.iter().filter(|(_, s)| s.owned == 1).count()
    }
    /// Kart can be bought (UI gate): rank reached (`unlockRank <= rank + 1`, -1 = never), not owned.
    pub fn kart_rank_unlocked(&self, kart: &str) -> bool {
        self.data.kart(kart).map(|k| k.unlock_rank != -1 && k.unlock_rank <= self.rank_display()).unwrap_or(false)
    }
    /// `CPlayerInfo::SetSelectedKart`/`ValidateSelectedKart @00151db0`: selecting an unowned kart falls back to the last valid one.
    pub fn select_kart(&mut self, kart: &str) -> bool {
        if self.is_kart_owned(kart) {
            self.profile.selected_kart = kart.to_string();
            true
        } else {
            false
        }
    }

    /// Achievement tracked value (`CAchievementsManager::OnModifyTrackedValue`): +1 on `inc`.
    // UNRESOLVED: CAchievementsManager internals (platform reporting, grades) not ported; offline the tracker only counts.
    pub(crate) fn track(&mut self, name: &str, inc: bool) {
        if inc {
            *self.profile.tracked.entry(name.to_string()).or_insert(0.0) += 1.0;
            self.check_achievements();
        }
    }

    /// Mark achievements whose `ValueTracker` reached `MaxValue`.
    pub fn check_achievements(&mut self) {
        let defs = self.data.achievements.clone();
        for a in defs {
            let Some(tr) = &a.value_tracker else { continue };
            let v = self.profile.tracked.get(tr).copied().unwrap_or(0.0);
            if v >= a.max_value as f32 && !self.profile.achievements_done.contains(&a.game_center_id) {
                self.profile.achievements_done.push(a.game_center_id.clone());
            }
        }
    }

    // ======================================================================================================
    // campaign  (CPlayerInfo @00150a0c.., CMetagameManager @0012df14.., TCampaignEventData)
    // ======================================================================================================

    pub fn campaign_len(&self) -> usize {
        self.data.events.campaign.len()
    }
    /// `CPlayerInfo::GetNextCampaignLevel @00150a0c`: first non-hidden event not yet completed; if all are completed, the last non-hidden one.
    pub fn next_campaign_level(&self) -> usize {
        let mut ret = 0;
        for (i, c) in self.data.events.campaign.iter().enumerate() {
            if !c.hidden {
                ret = i;
                if !self.profile.campaign.get(i).map(|s| s.completed).unwrap_or(false) {
                    return i;
                }
            }
        }
        ret
    }
    /// `CPlayerInfo::GetCampaignProgress @00150a9c`: number of completed non-hidden events.
    pub fn campaign_progress(&self) -> usize {
        self.data.events.campaign.iter().enumerate().filter(|(i, c)| !c.hidden && self.profile.campaign.get(*i).map(|s| s.completed).unwrap_or(false)).count()
    }
    /// An event can be played when everything before it is done (`index <= next campaign level`).
    // UNRESOLVED: the UI lock rule (CMapScreen) was not read; this is the rule implied by GetNextCampaignLevel / GetCampaignRaceWithAvailableReward.
    pub fn is_event_unlocked(&self, index: usize) -> bool {
        index < self.campaign_len() && index <= self.next_campaign_level()
    }
    /// `CEventDefinitionManager::GetCampaignEnergyCost @000ffc30` (1 when out of range).
    pub fn campaign_energy_cost(&self, index: usize) -> i32 {
        self.data.events.campaign.get(index).map(|c| c.energy_cost).unwrap_or(1)
    }
    /// `CMetagameManager::GetRaceEnergyCost(type, difficulty) @0012deac` for the non campaign modes (daily race / tournament); 1 if no entry.
    pub fn race_energy_cost(&self, kind: &str, difficulty: &str) -> i32 {
        self.data.economy.energy_cost.iter().find(|(t, d, _)| t == kind && d == difficulty).map(|(_, _, c)| *c).unwrap_or(1)
    }
    /// `CPlayerInfo::PlayedCampaignStage @00150b1c` + energy payment (`SpentEnergyOnRace`).
    pub fn start_campaign_event(&mut self, index: usize, now: u64) -> Result<(), MetaError> {
        if index >= self.campaign_len() {
            return Err(MetaError::UnknownEvent(index as i32));
        }
        if !self.is_event_unlocked(index) {
            return Err(MetaError::Locked(format!("event {index}")));
        }
        let cost = self.campaign_energy_cost(index);
        if !self.spend_energy(cost, now) {
            return Err(MetaError::NotEnoughEnergy);
        }
        self.ensure_campaign_len();
        self.profile.campaign[index].played = true;
        Ok(())
    }
    fn ensure_campaign_len(&mut self) {
        while self.profile.campaign.len() < self.data.events.campaign.len() {
            let i = self.profile.campaign.len();
            self.profile.campaign.push(CampaignState { tag: self.data.events.campaign[i].tag.clone(), ..Default::default() });
        }
    }

    /// `CMetagameManager::CalculateScoreFromCC @0012df14`: score needed for 1 / 2 / 3 stars: `fAddition + CC * fMultiplier * starMultiplier[i]`.
    pub fn star_scores(&self, cc: i32) -> [i32; 3] {
        let e = &self.data.economy;
        let f = |m: f32| (e.score_addition + cc as f32 * e.score_multiplier * m) as i32;
        [f(e.star_multipliers[0]), f(e.star_multipliers[1]), f(e.star_multipliers[2])]
    }
    /// `CMetagameManager::GetRaceMaxScore @0012df04`: `cc * 150`.
    pub fn race_max_score(cc: i32) -> i32 {
        cc * 0x96
    }
    pub fn stars_for_score(&self, cc: i32, score: i32) -> i32 {
        self.star_scores(cc).iter().filter(|&&s| score >= s).count() as i32
    }
    /// `CMetagameManager::GetDifficultyAdjust(eventCC, kartCC) @0012df90` with the economy `DifficultyAdjust relativeCC` values.
    pub fn difficulty_adjust(&self, event_cc: i32, kart_cc: i32) -> DifficultyAdjust {
        let d: Vec<i32> = self.data.economy.difficulty_adjust.iter().map(|(_, v)| *v).collect();
        let g = |i: usize| d.get(i).copied().unwrap_or(0);
        if event_cc <= kart_cc - g(0) {
            DifficultyAdjust::VeryEasy
        } else if event_cc <= kart_cc - g(1) {
            DifficultyAdjust::Easy
        } else if event_cc <= kart_cc - g(2) {
            DifficultyAdjust::Medium
        } else if kart_cc - g(3) < event_cc {
            DifficultyAdjust::Extreme
        } else {
            DifficultyAdjust::Hard
        }
    }

    /// `CPlayerInfo::CompletedCampaignStage(index, score, stars) @00150b38` plus the campaign rewards of the event.
    /// A reward with `RewardType` OneStar/TwoStar/ThreeStar is granted when that star level is reached for the first time
    /// (`TCampaignEventData::GetRewards @000ffec0` filters by a bit mask `1 << rewardType`).
    // UNRESOLVED: the caller that builds the bit mask (race results) was not read; "newly reached star levels" is the natural reading. Rewards with no
    // RewardType (type 4) are granted on every clear only when economy `campaignRepeatReward` is set, otherwise on the first clear.
    pub fn complete_campaign_event(&mut self, index: usize, stars: i32, score: i32, now: u64) -> Result<EventOutcome, MetaError> {
        if index >= self.campaign_len() {
            return Err(MetaError::UnknownEvent(index as i32));
        }
        self.ensure_campaign_len();
        let ev = self.data.events.campaign[index].clone();
        let (prev_stars, was_done) = {
            let s = &self.profile.campaign[index];
            (s.best_stars, s.completed)
        };
        {
            let s = &mut self.profile.campaign[index];
            s.completed = true;
            if s.best_score < score {
                s.best_score = score;
            }
            if s.best_stars < stars {
                s.best_stars = stars;
            }
        }
        let repeat = self.data.economy.campaign_repeat_reward;
        let mut rewards = Vec::new();
        for r in &ev.rewards {
            let give = match r.star {
                0..=2 => (r.star as i32) < stars && (r.star as i32) >= prev_stars,
                _ => !was_done || repeat,
            };
            if give {
                rewards.push(r.clone());
            }
        }
        let mut rank_up: Option<RankUp> = None;
        for r in &rewards {
            if r.is("Statistic", "XP") {
                if let Some(ru) = self.add_xp(r.quantity, 0, now) {
                    rank_up = Some(ru);
                }
            } else {
                self.apply_reward(r, now);
            }
        }
        self.profile.total_races += 1;
        if stars > 0 {
            self.profile.events_won += 1;
        }
        Ok(EventOutcome { new_stars: stars.max(prev_stars), first_clear: !was_done, rewards, rank_up })
    }

    /// Total stars earned over the campaign.
    pub fn total_stars(&self) -> i32 {
        self.profile.campaign.iter().map(|c| c.best_stars).sum()
    }
}

/// Free function form of `Meta::mod_spec` (the profile and data are all it needs).
pub fn mod_spec(profile: &Profile, data: &MetaData, kart: &str) -> ModSpec {
    let lvl = profile.kart_state(kart).map(|s| s.levels).unwrap_or([0; 5]);
    let m = |stat: Stat| -> f32 {
        // `CKartManager::GetKartStat` walk over tiers
        let Some(tiers) = data.upgrade_levels.get(kart) else { return 0.0 };
        let mut before = 0i32;
        let level = lvl[stat.idx()];
        for tl in tiers {
            let n = tl.stats[stat.idx()].len() as i32;
            if level < before + n {
                return tl.stats[stat.idx()][(level - before) as usize].modifier;
            }
            before += n;
        }
        tiers.last().and_then(|t| t.stats[stat.idx()].last()).map(|l| l.modifier).unwrap_or(0.0)
    };
    let char_lvl = profile.characters.get(&profile.selected_character).map(|c| c.level.max(1)).unwrap_or(1);
    ModSpec {
        grip: m(Stat::Grip),
        fragility: m(Stat::Strength),
        thrust: m(Stat::Acceleration),
        drag: m(Stat::TopSpeed),
        sliding_ang_vel: m(Stat::Handling),
        min_speed: 0.0,
        character_level_ratio: (char_lvl - 1) as f32 / 11.0,
    }
}
