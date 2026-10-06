//! Gacha / toolbox logic (offline), daily race rewards, parts shop, coin shop: ported from `CGachaManager`
//! (`GetRandomPrize @0010d5dc`, `OpenToolbox @0010f8f8`, `BuyTicketSpins @00110088`, `GetCoinSubsitution @0010a4e0`, `AwardPrize @0010a8bc`),
//! `MetagameData::TCoinSubstition::Randomise @0020b514`, `CSoftCurrencyShopManager`.
//!
//! Randomness: the original uses `lrand48()` for the weighted pick (`GetRandomPrize`), our `Lrand48` is the exact generator.

use super::data::*;
use super::rules::MetaError;
use super::Meta;

/// `TGachaPrizeInstance` (12 bytes): item, quantity, coin substitution (when non zero the prize is paid as coins).
#[derive(Clone, Debug, PartialEq)]
pub struct Prize {
    pub pool_id: String,
    pub reward: Reward,
    pub quantity: i32,
    pub coin_subst: i32,
}

impl Meta {
    // ---------------------------------------------------------------------------------------------------
    // toolboxes
    // ---------------------------------------------------------------------------------------------------

    /// `CGachaManager::GetActiveToolbox(type) @00113..`: the last toolbox of `kind` ("Gacha" = type 2) with `requiredRank > -2` and
    /// `requiredRank <= CPlayerInfo::GetRank()` (0 based index; see `OpenToolbox`).
    pub fn active_toolbox(&self, kind: &str) -> Option<usize> {
        let rank = self.rank();
        let mut found = None;
        for (i, t) in self.data.gacha.toolboxes.iter().enumerate() {
            if t.kind == kind && t.required_rank > -2 && t.required_rank <= rank {
                found = Some(i);
            }
        }
        found
    }

    fn is_kart_token(&self, id: &str) -> Option<(String, i32)> {
        if id.len() != 8 {
            return None;
        }
        let (k, d) = id.split_at(4);
        let n: i32 = d.parse().ok()?;
        self.data.kart(k).map(|_| (k.to_string(), n - 1))
    }

    /// `GetRandomPrize(pool, ...) @0010d5dc`: weighted pick over the pool items (sum of `weighting`, `lrand48() % total`, first item whose running
    /// sum exceeds it); kart tokens of karts that are neither unlocked by rank (`unlockRank <= rank + 1`) nor owned are excluded.
    // UNRESOLVED: the original also skips BLUE (blueprint) items when its `param_3` flag is set and re-draws through `GetRandomPrizes @0010e..`
    // (OpenToolbox calls GetRandomPrize(..., 1)); pools PB01.. consist of BLUE items only, so that flag cannot be a plain exclusion here.
    // This port allows every item.
    pub fn roll_pool(&mut self, pool_id: &str) -> Option<Prize> {
        let pool = self.data.gacha.pools.iter().find(|p| p.id == pool_id)?.clone();
        let mut cands: Vec<(usize, i32)> = Vec::new();
        for (i, it) in pool.items.iter().enumerate() {
            if it.reward.kind == "Token" {
                if let Some((k, _)) = self.is_kart_token(&it.reward.sub) {
                    let def = self.data.kart(&k).unwrap();
                    let ok = self.is_kart_owned(&k) || (def.unlock_rank != -1 && def.unlock_rank <= self.rank_display());
                    if !ok {
                        continue;
                    }
                }
            }
            cands.push((i, it.weighting));
        }
        let total: i64 = cands.iter().map(|(_, w)| *w as i64).sum();
        if cands.is_empty() || total <= 0 {
            return None;
        }
        let r = (self.rng.next() as i64) % total;
        let mut acc = 0i64;
        let mut chosen = cands[0].0;
        for (i, w) in &cands {
            acc += *w as i64;
            if acc > r {
                chosen = *i;
                break;
            }
        }
        let it = pool.items[chosen].clone();
        let quantity = self.roll_quantity(&it);
        let mut coin_subst = 0;
        if it.reward.kind == "Token" {
            if let Some((k, part)) = self.is_kart_token(&it.reward.sub) {
                if !self.is_kart_owned(&k) {
                    coin_subst = self.coin_substitution(&k, part);
                }
            }
        }
        Some(Prize { pool_id: pool_id.to_string(), reward: it.reward, quantity, coin_subst })
    }

    /// quantity roll (`GetRandomPrize @0010da20`): type 2 fixed; type 0 "Normal": mean `min + 0.5*range`, sigma `0.25*range` (Box-Muller:
    /// `sqrt(-2 ln u1) * cos(angle)`), `+0.5`, clamped to [min, max]; type 1: same but sigma scaled by an unread constant and folded at `min`.
    // UNRESOLVED: the angle of the cosine (2*pi*u2 assumed) and the type 1 sigma constant (DAT_0010e230) were not read.
    fn roll_quantity(&mut self, it: &GachaItem) -> i32 {
        if it.max_quantity <= it.min_quantity {
            return it.min_quantity;
        }
        if it.quantity_random_type == 2 {
            return it.min_quantity;
        }
        let min = it.min_quantity as f32;
        let range = (it.max_quantity - it.min_quantity) as f32;
        let u1 = self.rng.unit().max(1e-7);
        let u2 = self.rng.unit();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
        let v = min + range * 0.5 + range * 0.25 * z;
        (((v + 0.5) as i32).max(it.min_quantity)).min(it.max_quantity)
    }

    /// `CGachaManager::GetCoinSubsitution @0010a4e0` for one unowned kart token: `TCoinSubstition::Randomise(multiplier)` with
    /// `multiplier = TXP::GetAmountGained(kartRarity, partRarity)` over the `CoinSubstitution` table of metagame.xml.
    pub fn coin_substitution(&mut self, kart: &str, part_rarity: i32) -> i32 {
        let Some(def) = self.data.kart(kart) else { return 0 };
        let rarity = def.rarity;
        let mut v = 1i32;
        for u in &self.data.metagame.coin_subst {
            if (u.kart_rarity == 4 || u.kart_rarity == rarity) && (u.part_rarity == 3 || u.part_rarity == part_rarity) {
                v *= u.multiplier;
            }
        }
        // Randomise(v) @0020b514: v + gauss(0, variance*v), min v*minClamp, +0.5
        let variance = self.data.metagame.coin_subst_variance;
        let min_clamp = self.data.metagame.coin_subst_min_clamp;
        let u1 = self.rng.unit().max(1e-7);
        let u2 = self.rng.unit();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
        let vf = v as f32;
        let mut r = vf + variance * vf * z;
        if r <= vf * min_clamp {
            r = vf * min_clamp;
        }
        (r + 0.5) as i32
    }

    /// One "open": every `<Spin poolID numSpins chance>` of the toolbox is taken with probability `chance` (`OpenToolbox`: `random < chance`),
    /// then `numSpins` prizes are drawn from the pool.
    pub fn roll_toolbox(&mut self, toolbox: usize) -> Vec<Prize> {
        let Some(tb) = self.data.gacha.toolboxes.get(toolbox).cloned() else { return vec![] };
        let mut out = Vec::new();
        for sp in &tb.spins {
            if self.rng.unit() < sp.chance || sp.chance >= 1.0 {
                for _ in 0..sp.num_spins {
                    if let Some(p) = self.roll_pool(&sp.pool_id) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    /// `CGachaManager::AwardPrize @0010a8bc`: a non zero coin substitution pays coins, otherwise the item with its quantity.
    pub fn award_prize(&mut self, p: &Prize, now: u64) {
        if p.coin_subst != 0 {
            self.add_coins(p.coin_subst);
            return;
        }
        let mut r = p.reward.clone();
        r.quantity = p.quantity;
        self.apply_reward(&r, now);
    }

    /// `CGachaManager::BuyTicketSpins(n) @00110088`: pays `n * tokenCost` tickets (`GACH0000`) and opens the active Gacha toolbox `n` times.
    pub fn buy_ticket_spins(&mut self, n: i32, now: u64) -> Result<Vec<Prize>, MetaError> {
        let tb = self.active_toolbox("Gacha").ok_or_else(|| MetaError::Locked("toolbox".into()))?;
        let cost = self.data.gacha.toolboxes[tb].token_cost.max(1) * n;
        let token = if self.data.gacha.token_type.is_empty() { "GACH0000".to_string() } else { self.data.gacha.token_type.clone() };
        let have = self.profile.token(&token);
        if have < cost {
            return Err(MetaError::MissingTokens { token, have, need: cost });
        }
        self.add_tokens(&token, -cost);
        let mut all = Vec::new();
        for _ in 0..n {
            for p in self.roll_toolbox(tb) {
                self.award_prize(&p, now);
                all.push(p);
            }
        }
        Ok(all)
    }

    /// Gem purchase of a toolbox (`CGachaManager::BuyPremiumSpin @0010fe..`): `gemCost` per spin, or `gemMultiSpinCost` for `multiSpinAmount` spins.
    // UNRESOLVED: BuyPremiumSpin @00110fa8 was not read; costs are the toolbox attributes gemCost / gemMultiSpinCost / multiSpinAmount.
    pub fn buy_toolbox_with_gems(&mut self, toolbox: usize, multi: bool, now: u64) -> Result<Vec<Prize>, MetaError> {
        let tb = self.data.gacha.toolboxes.get(toolbox).cloned().ok_or_else(|| MetaError::Locked("toolbox".into()))?;
        let (cost, spins) = if multi { (tb.gem_multi_spin_cost, tb.multi_spin_amount.max(1)) } else { (tb.gem_cost, 1) };
        if !self.spend_gems(cost) {
            return Err(MetaError::NotEnoughGems { missing: cost - self.profile.gems });
        }
        let mut all = Vec::new();
        for _ in 0..spins {
            for p in self.roll_toolbox(toolbox) {
                self.award_prize(&p, now);
                all.push(p);
            }
        }
        Ok(all)
    }

    /// Free "ad" toolbox (offline stand-in: no ad is shown, the `adToolboxSpinInterval` cooldown still applies).
    pub fn free_ad_toolbox(&mut self, now: u64) -> Result<Vec<Prize>, MetaError> {
        let interval = self.data.gacha.ad_toolbox_spin_interval.max(0) as u64;
        if self.profile.last_ad_spin != 0 && now < self.profile.last_ad_spin + interval {
            return Err(MetaError::NotAvailableOffline("ad toolbox cooling down".into()));
        }
        let tb = self.data.gacha.toolboxes.iter().position(|t| t.kind == "Ad").ok_or_else(|| MetaError::Locked("ad toolbox".into()))?;
        self.profile.last_ad_spin = now;
        let prizes = self.roll_toolbox(tb);
        for p in &prizes {
            self.award_prize(p, now);
        }
        Ok(prizes)
    }

    // ---------------------------------------------------------------------------------------------------
    // daily race (offline stand-in)
    // ---------------------------------------------------------------------------------------------------

    /// The `<DailyReward>` entry active at `now`: `(now - startTime) mod repeatTime < duration` (repeatTime 0 = once).
    // The server picked the race; the reward calendar itself is data (dailyraces.xml).
    pub fn daily_reward_now(&self, now: u64) -> Option<(usize, u64)> {
        for (i, d) in self.data.daily_races.rewards.iter().enumerate() {
            if now < d.start_time {
                continue;
            }
            let since = now - d.start_time;
            let cycle = if d.repeat_time > 0 { since / d.repeat_time } else { 0 };
            let into = if d.repeat_time > 0 { since % d.repeat_time } else { since };
            if into < d.duration {
                return Some((i, cycle));
            }
        }
        None
    }
    /// Rewards for a daily race result: the `RaceRewards` plus every milestone whose score <= `total_score` not yet claimed this cycle.
    pub fn claim_daily_race(&mut self, score: i32, now: u64) -> Result<Vec<Reward>, MetaError> {
        let (i, cycle) = self.daily_reward_now(now).ok_or_else(|| MetaError::NotAvailableOffline("no daily race in the calendar".into()))?;
        let d = self.data.daily_races.rewards[i].clone();
        let key = format!("{}-{}", d.start_time, cycle);
        let claimed = self.profile.daily_race_claimed.get(&key).copied().unwrap_or(0);
        let mut out = Vec::new();
        if claimed == 0 {
            out.extend(d.race_rewards.iter().cloned());
        }
        let mut top = claimed;
        for (m, rs) in &d.milestones {
            if *m > claimed && *m <= score {
                out.extend(rs.iter().cloned());
                top = top.max(*m);
            }
        }
        self.profile.daily_race_claimed.insert(key, top.max(1));
        for r in &out {
            self.apply_reward(r, now);
        }
        Ok(out)
    }

    // ---------------------------------------------------------------------------------------------------
    // shop (offline: gems / coins only)
    // ---------------------------------------------------------------------------------------------------

    /// Coin packs `SC01..SC05` of the currency shop: priced in gems (`type="Gems" price`), bundle gives coins. Real-money items are refused.
    pub fn buy_shop_item(&mut self, tag: &str, now: u64) -> Result<Vec<Reward>, MetaError> {
        let item = self.data.store.item(tag).cloned().ok_or_else(|| MetaError::NotAvailableOffline(format!("unknown item {tag}")))?;
        if item.real_money {
            return Err(MetaError::NotAvailableOffline(format!("{tag} is a real-money purchase")));
        }
        let (cur, price) = item.price.clone().ok_or_else(|| MetaError::NotAvailableOffline(format!("{tag} has no price")))?;
        if cur != "Gems" {
            return Err(MetaError::NotAvailableOffline(format!("currency {cur}")));
        }
        if !self.spend_gems(price) {
            return Err(MetaError::NotEnoughGems { missing: price - self.profile.gems });
        }
        let rewards = self.data.store.bundle(item.bundle_index).map(|b| b.items.clone()).unwrap_or_default();
        for r in &rewards {
            self.apply_reward(r, now);
        }
        Ok(rewards)
    }

    /// Parts shop slot price (`TCost::GetCost`): `rarity` "Common"/"Rare"/"Epic", the number of parts of that rarity bought so far.
    pub fn parts_shop_price(&self, rarity: &str) -> Option<i32> {
        let c = self.data.store.parts_cost(rarity)?;
        Some(c.get_cost(self.profile.parts_shop_bought.get(rarity).copied().unwrap_or(0)))
    }
    /// Buy a part (token) from the parts shop for coins (`CSoftCurrencyShopManager::BuyItem @0017b3dc`: SpendSoftCurrency, grant, bought++).
    pub fn buy_parts_item(&mut self, token: &str, rarity: &str, now: u64) -> Result<(), MetaError> {
        let price = self.parts_shop_price(rarity).ok_or_else(|| MetaError::NotAvailableOffline("parts shop".into()))?;
        if !self.spend_coins(price) {
            return Err(MetaError::NotEnoughCoins { missing: price - self.profile.coins });
        }
        self.apply_reward(&Reward { kind: "Token".into(), sub: token.to_string(), quantity: 1, star: 4 }, now);
        *self.profile.parts_shop_bought.entry(rarity.to_string()).or_insert(0) += 1;
        Ok(())
    }
    /// Tokens offered by the parts shop for the player's rank: tier `GetBestTierIndex(rank)`, one random entry per rarity slot (seeded roll).
    // UNRESOLVED: RepopulateShop @0017a9a8 (seed rotation, number of visible slots, duplicates) was not ported; this is a simple stand-in.
    pub fn parts_shop_offer(&mut self) -> Vec<(String, String)> {
        let rank = self.rank();
        let ti = self.data.store.best_parts_tier(rank);
        let Some(tier) = self.data.store.parts_tiers.get(ti).cloned() else { return vec![] };
        let mut out = vec![];
        for (rar, ids) in tier.slots {
            if ids.is_empty() {
                continue;
            }
            let k = (self.rng.next() as usize) % ids.len();
            out.push((rar, ids[k].clone()));
        }
        out
    }
}
