//! Unit tests of the meta game port (they read the real definition files from `assets292`; set `ABG_ASSETS` if it is not found automatically).

use super::*;

fn root() -> PathBuf {
    if let Some(r) = find_assets_root() {
        return r;
    }
    let p = PathBuf::from(r"C:\Users\Brady\Desktop\AngryBirdsGo\assets292");
    assert!(p.exists(), "assets292 not found; set ABG_ASSETS");
    p
}

fn game() -> Meta {
    let data = MetaData::load(&root()).expect("load meta data");
    Meta::new_game(data, 1_700_000_000)
}

const T0: u64 = 1_700_000_000;

#[test]
fn data_loads_and_counts() {
    let m = game();
    assert!(m.data.warnings.is_empty(), "warnings: {:?}", m.data.warnings);
    assert_eq!(m.data.ranks.len(), 50);
    assert_eq!(m.data.events.campaign.len(), 55);
    assert!(m.data.karts.len() >= 39);
    assert_eq!(m.data.gacha.toolboxes.len(), 8);
    assert_eq!(m.data.gacha.pools.len(), 24);
    assert_eq!(m.data.map.chapters.len(), 11);
    assert_eq!(m.data.char_levelling.thresholds.len(), 11);
    assert_eq!(m.data.energy.as_ref().unwrap().starting_amount, 20);
    assert_eq!(m.data.energy.as_ref().unwrap().recharge_secs, 300);
    assert!(m.data.upgrade_levels.contains_key("SSKM"));
    assert!(!m.data.achievements.is_empty());
    assert!(!m.data.challenges.is_empty());
    assert!(!m.data.store.items.is_empty());
    assert!(!m.data.store.parts_tiers.is_empty());
    assert_eq!(m.data.daily_races.rewards.len(), 7);
    assert_eq!(m.data.economy.score_multiplier, 150.0);
}

#[test]
fn rank_thresholds() {
    let m = game();
    // ranklist.xml: rank0 0..120, rank1 121..280, rank2 281..400 ...
    assert_eq!(m.rank_for_xp(0), 0);
    assert_eq!(m.rank_for_xp(120), 0);
    assert_eq!(m.rank_for_xp(121), 1);
    assert_eq!(m.rank_for_xp(280), 1);
    assert_eq!(m.rank_for_xp(281), 2);
    assert_eq!(m.rank_for_xp(400), 2);
    assert_eq!(m.rank_for_xp(401), 3);
    assert_eq!(m.rank_for_xp(5001), 49);
    assert_eq!(m.rank_for_xp(99999), 49);
    // GetRank returns 0 when no rank matches (xp above the last max)
    assert_eq!(m.rank_for_xp(100_000), 0);
    assert_eq!(m.rank_min_xp(2), 281);
    assert_eq!(m.rank_max_xp(2), 400);
    assert_eq!(m.rank_max_energy(0), 20);
    assert_eq!(m.rank_max_energy(49), 99);
    assert_eq!(m.max_xp(), 99999);
    // contiguous table
    for i in 1..m.data.ranks.len() {
        assert_eq!(m.data.ranks[i].min_xp, m.data.ranks[i - 1].max_xp + 1, "gap before rank {i}");
    }
}

#[test]
fn first_run_state() {
    let m = game();
    assert_eq!(m.profile.coins, 0);
    assert_eq!(m.profile.gems, 0);
    assert_eq!(m.profile.selected_kart, "SSKM");
    assert!(m.is_kart_owned("SSKM"));
    assert_eq!(m.karts_owned(), 1);
    assert_eq!(m.energy_level(T0), 20);
    assert_eq!(m.energy_max(), 20);
    assert_eq!(m.rank(), 0);
    assert_eq!(m.rank_display(), 1);
    assert!(m.profile.characters["red"].unlocked);
    assert!(m.profile.unlocked_episodes.iter().all(|x| *x));
    assert_eq!(m.next_campaign_level(), 0);
    assert!(m.is_event_unlocked(0));
    assert!(!m.is_event_unlocked(1));
}

#[test]
fn energy_spend_and_regen() {
    let mut m = game();
    assert!(m.spend_energy(3, T0));
    assert_eq!(m.energy_level(T0), 17);
    // 300 s per unit
    assert_eq!(m.energy_level(T0 + 299), 17);
    assert_eq!(m.energy_level(T0 + 300), 18);
    assert_eq!(m.energy_level(T0 + 600), 19);
    assert_eq!(m.energy_level(T0 + 900), 20);
    assert_eq!(m.energy_level(T0 + 100_000), 20);
    // cannot spend more than available
    assert!(!m.spend_energy(18, T0));
    assert_eq!(m.energy_level(T0), 17);
    // spend again while recharging: timestamp keeps moving
    assert!(m.spend_energy(2, T0 + 100));
    assert_eq!(m.energy_level(T0 + 100), 15);
    assert_eq!(m.energy_level(T0 + 100 + 1500), 20);
    // time until next unit
    assert!(m.energy_time_until_next(T0 + 100) <= 300);
    // refill costs the energy.xml gem price and fills the bar
    assert_eq!(m.energy_refill_cost(T0 + 100), 30);
    assert!(m.refill_energy_with_gems(T0 + 100).is_err());
    m.profile.gems = 100;
    m.refill_energy_with_gems(T0 + 100).unwrap();
    assert_eq!(m.profile.gems, 70);
    assert_eq!(m.energy_level(T0 + 100), 20);
    assert_eq!(m.energy_refill_cost(T0 + 100), 0);
    // add_energy at full -> excess, spent first
    assert!(!m.add_energy(T0 + 100));
    assert_eq!(m.energy_level(T0 + 100), 21);
    assert!(m.spend_energy(1, T0 + 100));
    assert_eq!(m.profile.energy.excess, 0);
    assert_eq!(m.energy_level(T0 + 100), 20);
}

#[test]
fn xp_rank_up_rewards_and_energy() {
    let mut m = game();
    let r = m.add_xp(121, 0, T0).expect("rank up");
    assert_eq!((r.from, r.to), (0, 1));
    assert_eq!(r.new_max_energy, 23);
    assert_eq!(m.energy_max(), 23);
    assert_eq!(m.energy_level(T0), 23, "rank up refills the bar to the new maximum");
    assert_eq!(m.profile.coins, 5000, "rank 1 reward = 5000 coins");
    assert!(m.profile.pending_rank_popup);
    m.set_has_seen_rank_popup();
    assert!(!m.profile.pending_rank_popup);
    // rank 2 reward is 20 BLUE0001
    let r = m.add_xp(160, 0, T0).unwrap();
    assert_eq!(r.to, 2);
    assert_eq!(m.token_count("BLUE0001"), 20);
    // xp is clamped at the maximum
    m.add_xp(10_000_000, 0, T0);
    assert_eq!(m.profile.xp_total(), 99999);
    assert_eq!(m.rank(), 49);
    // a kart unlocks at displayed rank == unlockRank (MMKR needs 3)
    let mut m2 = game();
    let ru = m2.add_xp(281, 0, T0).unwrap();
    assert_eq!(ru.to, 2);
    assert!(ru.karts_unlocked.contains(&"MMKR".to_string()));
}

#[test]
fn kart_cc_and_mod_spec_level_zero() {
    let m = game();
    // baseCC 54 + sum(tier0 ccIncrease(1) * 1 level) over 5 stats
    assert_eq!(m.kart_cc("SSKM"), 54 + 5);
    let ms = m.mod_spec("SSKM");
    // kartupgradelevels.xml SSKM tier 0, index 0 of each stat: TopSpeed 1, Acceleration 0.1, Strength 0.8, Handling 0.14, Grip 0.6
    assert!((ms.drag - 1.0).abs() < 1e-6);
    assert!((ms.thrust - 0.1).abs() < 1e-6);
    assert!((ms.fragility - 0.8).abs() < 1e-6);
    assert!((ms.sliding_ang_vel - 0.14).abs() < 1e-6);
    assert!((ms.grip - 0.6).abs() < 1e-6);
    assert_eq!(ms.min_speed, 0.0);
    assert_eq!(ms.tuple(), mod_spec(&m.profile, &m.data, "SSKM"));
    assert_eq!(ms.character_level_ratio, 0.0);
}

#[test]
fn upgrade_cost_lookup_and_upgrade() {
    let mut m = game();
    // SSKM TopSpeed: idx0 (1, 200c), idx1 Cost 2 Coins 300 Rare 0.99, idx2 Cost 10 Coins 400 Common .98, idx3 Cost 2 Coins 600 Rare .97
    let (lv, coin) = m.next_stat_level("SSKM", Stat::TopSpeed).unwrap();
    assert_eq!((lv.cost_tokens, lv.coins, lv.rarity), (2, 300, 1));
    assert_eq!(coin, 300);
    assert_eq!(Meta::upgrade_token_id("SSKM", lv.rarity), "SSKM0002");
    // without tokens
    assert!(matches!(m.upgrade_stat("SSKM", Stat::TopSpeed, T0), Err(MetaError::MissingTokens { .. })));
    m.add_tokens("SSKM0002", 2);
    assert!(matches!(m.upgrade_stat("SSKM", Stat::TopSpeed, T0), Err(MetaError::NotEnoughCoins { .. })));
    m.profile.coins = 1000;
    let r = m.upgrade_stat("SSKM", Stat::TopSpeed, T0).unwrap();
    assert_eq!(r.new_level, 1);
    assert_eq!(r.coins_spent, 300);
    assert_eq!(m.profile.coins, 700);
    assert_eq!(m.token_count("SSKM0002"), 0);
    // xp: kart Common (x2) * part Rare (x1)... TXP: kartRarity Common=2, partRarity Rare=1 -> 2
    assert_eq!(r.xp_gained, 2);
    assert_eq!(m.profile.xp_upgrade, 2);
    assert!((m.mod_spec("SSKM").drag - 0.99).abs() < 1e-6);
    assert_eq!(m.kart_cc("SSKM"), 54 + 5 + 1);
    // cannot go past the end of the tier: levels in tier = 4, so 3 upgrades max
    m.add_tokens("SSKM0001", 100);
    m.add_tokens("SSKM0002", 100);
    m.add_tokens("SSKM0003", 100);
    m.profile.coins = 100_000;
    m.upgrade_stat("SSKM", Stat::TopSpeed, T0).unwrap();
    m.upgrade_stat("SSKM", Stat::TopSpeed, T0).unwrap();
    assert!(matches!(m.upgrade_stat("SSKM", Stat::TopSpeed, T0), Err(MetaError::MaxLevelForTier)));
    assert_eq!(m.profile.kart_state("SSKM").unwrap().levels[0], 3);
    // modifier at the end of tier 0
    assert!((m.mod_spec("SSKM").drag - 0.97).abs() < 1e-6);
    // the other stats are not maxed so the tier cannot be raised yet
    assert!(!m.can_upgrade_tier("SSKM", false));
}

#[test]
fn stat_modifier_walks_tiers() {
    let m = game();
    // MMKR has two tiers; TopSpeed tier0 has 5 entries, so cumulative level 5 is tier 1 index 0 (0.92)
    assert_eq!(m.max_level_for_tier("MMKR", Stat::TopSpeed, 0), 5);
    assert!((m.kart_stat_modifier("MMKR", Stat::TopSpeed, 4) - 0.93).abs() < 1e-6);
    assert!((m.kart_stat_modifier("MMKR", Stat::TopSpeed, 5) - 0.92).abs() < 1e-6);
    assert_eq!(m.level_to_tier("MMKR", Stat::TopSpeed, 5), Some((1, 0)));
    assert_eq!(m.kart_stat_modifier("NOPE", Stat::Grip, 0), 0.0);
}

#[test]
fn kart_purchase_unlock_rules() {
    let mut m = game();
    // MMKR: unlockRank 3, unlockCost 50
    assert!(!m.kart_rank_unlocked("MMKR"));
    assert!(matches!(m.purchase_kart("MMKR"), Err(MetaError::MissingTokens { .. })));
    m.add_tokens("BLUE0001", 60);
    m.purchase_kart("MMKR").unwrap();
    assert!(m.is_kart_owned("MMKR"));
    assert_eq!(m.token_count("BLUE0001"), 10);
    assert_eq!(m.karts_owned(), 2);
    assert!(m.select_kart("MMKR"));
    assert!(!m.select_kart("DRLR"));
    assert_eq!(m.profile.selected_kart, "MMKR");
}

#[test]
fn campaign_stars_and_rewards() {
    let mut m = game();
    assert_eq!(m.campaign_energy_cost(0), 1);
    m.start_campaign_event(0, T0).unwrap();
    assert_eq!(m.energy_level(T0), 19);
    assert!(m.start_campaign_event(1, T0).is_err(), "event 1 is locked until event 0 is completed");
    // C01: OneStar coins 200 + XP 30, TwoStar coins 300, ThreeStar coins 500
    let o = m.complete_campaign_event(0, 3, 20_000, T0).unwrap();
    assert!(o.first_clear);
    assert_eq!(m.profile.coins, 200 + 300 + 500);
    assert_eq!(m.profile.xp_campaign, 30);
    assert_eq!(m.total_stars(), 3);
    assert!(m.is_event_unlocked(1));
    assert_eq!(m.next_campaign_level(), 1);
    // a replay with the same stars gives nothing
    let o = m.complete_campaign_event(0, 3, 30_000, T0).unwrap();
    assert!(o.rewards.is_empty());
    assert_eq!(m.profile.coins, 1000);
    assert_eq!(m.profile.campaign[0].best_score, 30_000);
    // later improvement only pays the missing star
    let o = m.complete_campaign_event(1, 1, 1, T0).unwrap();
    assert!(!o.rewards.is_empty());
    let before = m.profile.coins;
    let o = m.complete_campaign_event(1, 2, 2, T0).unwrap();
    assert!(o.rewards.iter().all(|r| r.star == 1));
    assert!(m.profile.coins >= before);
}

#[test]
fn star_scores_and_difficulty() {
    let m = game();
    // economy.xml Score fMultiplier 150, fAddition 0, stars 1.0 / 1.40 / 1.90
    assert_eq!(m.star_scores(55), [8250, 11550, 15675]);
    assert_eq!(m.stars_for_score(55, 8249), 0);
    assert_eq!(m.stars_for_score(55, 8250), 1);
    assert_eq!(m.stars_for_score(55, 15675), 3);
    assert_eq!(Meta::race_max_score(10), 1500);
    // DifficultyAdjust relativeCC 4, 0, -2, -4, -8
    assert_eq!(m.difficulty_adjust(50, 60), DifficultyAdjust::VeryEasy); // event <= kart - 4
    assert_eq!(m.difficulty_adjust(60, 60), DifficultyAdjust::Easy);
    assert_eq!(m.difficulty_adjust(62, 60), DifficultyAdjust::Medium);
    assert_eq!(m.difficulty_adjust(64, 60), DifficultyAdjust::Hard);
    assert_eq!(m.difficulty_adjust(80, 60), DifficultyAdjust::Extreme);
}

#[test]
fn gacha_weights_and_toolboxes() {
    let m = game();
    for p in &m.data.gacha.pools {
        let sum: i64 = p.items.iter().map(|i| i.weighting as i64).sum();
        assert!(sum > 0, "pool {} has no weight", p.id);
        for it in &p.items {
            assert!(it.min_quantity <= it.max_quantity, "pool {}", p.id);
        }
    }
    // every Spin of every toolbox points to a pool
    for t in &m.data.gacha.toolboxes {
        for s in &t.spins {
            assert!(m.data.gacha.pools.iter().any(|p| p.id == s.pool_id), "toolbox {} -> missing pool {}", t.name, s.pool_id);
            assert!(s.chance > 0.0 && s.chance <= 1.0);
        }
    }
    // PL01 weights: 79 + 20 per kart pair + a few 1s
    let pl01 = m.data.gacha.pools.iter().find(|p| p.id == "PL01").unwrap();
    let sum: i32 = pl01.items.iter().map(|i| i.weighting).sum();
    assert!(sum > 500);
    // the wooden toolbox is the active one at rank 0
    let tb = m.active_toolbox("Gacha").unwrap();
    assert_eq!(m.data.gacha.toolboxes[tb].name, "WOODEN_TOOLBOX");
}

#[test]
fn gacha_roll_follows_weights() {
    let mut m = game();
    // PC01 is a single coin item 90..126
    for _ in 0..50 {
        let p = m.roll_pool("PC01").unwrap();
        assert_eq!(p.reward.sub, "Coins");
        assert!(p.quantity >= 90 && p.quantity <= 126, "{}", p.quantity);
    }
    // empirical weights of PL01 restricted to rank-eligible items: SSKM (owned) 79 vs 20
    let mut common = 0;
    let mut rare = 0;
    for _ in 0..4000 {
        let p = m.roll_pool("PL01").unwrap();
        match p.reward.sub.as_str() {
            "SSKM0001" => common += 1,
            "SSKM0002" => rare += 1,
            _ => {}
        }
    }
    let ratio = common as f64 / rare as f64;
    assert!(ratio > 2.5 && ratio < 6.0, "79:20 expected ~3.95, got {ratio} ({common}/{rare})");
}

#[test]
fn toolbox_open_with_tickets() {
    let mut m = game();
    assert!(matches!(m.buy_ticket_spins(1, T0), Err(MetaError::MissingTokens { .. })));
    m.add_tokens("GACH0000", 3);
    let prizes = m.buy_ticket_spins(2, T0).unwrap();
    assert_eq!(m.token_count("GACH0000"), 1);
    // wooden toolbox: PL01 x2 + PC01 x1 + PP03 x1 (all chance 1.0) per spin
    assert_eq!(prizes.len(), 2 * 4);
    assert!(m.profile.coins > 0 || m.profile.tokens.len() > 1);
}

#[test]
fn shop_coin_pack_with_gems() {
    let mut m = game();
    assert!(m.buy_shop_item("HC02", T0).is_err(), "real money item");
    assert!(matches!(m.buy_shop_item("SC01", T0), Err(MetaError::NotEnoughGems { .. })));
    m.profile.gems = 80;
    let r = m.buy_shop_item("SC01", T0).unwrap();
    assert_eq!(m.profile.gems, 5);
    assert_eq!(m.profile.coins, 7500);
    assert_eq!(r.len(), 1);
    // parts shop cost curve: Common cost 100, +5 per bought, cap 500
    assert_eq!(m.parts_shop_price("Common"), Some(100));
    m.profile.parts_shop_bought.insert("Common".into(), 3);
    assert_eq!(m.parts_shop_price("Common"), Some(115));
    m.profile.parts_shop_bought.insert("Common".into(), 1000);
    assert_eq!(m.parts_shop_price("Common"), Some(500));
}

#[test]
fn daily_race_calendar() {
    let mut m = game();
    // dailyraces.xml: first entry starts 1462752000 (1 day, repeats every 7 days)
    let (i, cycle) = m.daily_reward_now(1462752000 + 100).unwrap();
    assert_eq!((i, cycle), (0, 0));
    let (i, cycle) = m.daily_reward_now(1462752000 + 7 * 86400 + 100).unwrap();
    assert_eq!((i, cycle), (0, 1));
    let (i, _) = m.daily_reward_now(1462752000 + 86400 + 5).unwrap();
    assert_eq!(i, 1);
    let r = m.claim_daily_race(16, 1462752000 + 100).unwrap();
    // race reward 100 coins + milestone 5 (GACH0000 x1) + milestone 15 (BLUE0001 x5)
    assert_eq!(m.profile.coins, 100);
    assert_eq!(m.token_count("GACH0000"), 1);
    assert_eq!(m.token_count("BLUE0001"), 5);
    assert_eq!(r.len(), 3);
    // claiming again gives only new milestones
    let r2 = m.claim_daily_race(26, 1462752000 + 200).unwrap();
    assert_eq!(r2.len(), 1);
    assert_eq!(m.token_count("GACH0000"), 4);
}

#[test]
fn character_levelling() {
    let mut m = game();
    assert_eq!(m.character_level_from_xp(0), 1);
    assert_eq!(m.character_level_from_xp(99), 1);
    assert_eq!(m.character_level_from_xp(100), 2);
    assert_eq!(m.character_level_from_xp(6600), 12);
    m.add_character_xp("red", 300);
    assert_eq!(m.profile.characters["red"].level, 3);
    assert!((m.character_level_ratio() - 2.0 / 11.0).abs() < 1e-6);
}

#[test]
fn profile_text_roundtrip() {
    let mut m = game();
    m.profile.coins = 12345;
    m.profile.gems = 77;
    m.add_tokens("SSKM0002", 9);
    m.add_xp(150, 0, T0);
    m.start_campaign_event(0, T0).unwrap();
    m.complete_campaign_event(0, 2, 12_000, T0).unwrap();
    m.unlock_kart("MMKR").unwrap();
    m.profile.kart_state_mut("SSKM").unwrap().levels = [1, 2, 0, 1, 0];
    m.profile.settings.music_volume = 0.25;
    m.unlock_character("black");
    let text = m.profile.to_text();
    let base = Profile::new_default(&m.data, T0);
    let back = Profile::from_text(&text, base).unwrap();
    assert_eq!(back, m.profile);
    // garbage is rejected
    assert!(Profile::from_text("hello", Profile::new_default(&m.data, T0)).is_err());
}

#[test]
fn save_file_roundtrip() {
    let dir = std::env::temp_dir().join(format!("abg_meta_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(SAVE_FILE_NAME);
    let _ = std::fs::remove_file(&path);
    let mut m = Meta::open(&root(), Some(path.clone()), T0).unwrap();
    m.profile.coins = 4242;
    m.spend_energy(5, T0);
    m.save(T0 + 1).unwrap();
    let m2 = Meta::open(&root(), Some(path.clone()), T0 + 10).unwrap();
    assert_eq!(m2.profile.coins, 4242);
    assert_eq!(m2.energy_level(T0 + 10), 15);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn topbar_and_views() {
    let mut m = game();
    let tb = m.topbar(T0);
    assert_eq!((tb.coins, tb.gems, tb.energy, tb.energy_max, tb.rank), (0, 0, 20, 20, 1));
    assert_eq!(tb.xp_fraction, 0.0);
    m.add_xp(60, 0, T0);
    let tb = m.topbar(T0);
    assert!((tb.xp_fraction - 60.0 / 120.0).abs() < 1e-3);
    let ev = m.map_event(0).unwrap();
    assert_eq!(ev.tag, "C01");
    assert_eq!(ev.cc, 55);
    assert!(ev.unlocked);
    assert_eq!(ev.star_scores, [8250, 11550, 15675]);
    assert_eq!(m.map_events().len(), 55);
    let kv = m.kart_view("SSKM").unwrap();
    assert!(kv.owned);
    assert_eq!(kv.stats.len(), 5);
    assert_eq!(kv.stats[0].levels_in_tier, 4);
    assert_eq!(kv.cc, 59);
    let list = m.kart_list(0);
    assert!(list.iter().any(|k| k.id == "SSKM"));
}

#[test]
fn kart_theme_lists_are_complete() {
    let m = game();
    let total: usize = (0..5).map(|t| m.kart_list(t).len()).sum();
    let non_power = m.data.karts.iter().filter(|k| !k.is_power_up_kart).count();
    assert_eq!(total, non_power);
    // every non power-up kart has upgrade levels
    for k in m.data.karts.iter().filter(|k| !k.is_power_up_kart) {
        assert!(m.data.upgrade_levels.contains_key(&k.base_id), "no upgrade levels for {}", k.base_id);
    }
}
