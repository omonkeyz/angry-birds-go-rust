//! Meta game definition data: loaders for the original XML definition files (decoded copies in `assets292/xml`).
//!
//! Every value here is read from the original data; nothing is invented. Where the original code supplies a default for a
//! missing attribute the default is quoted with the address of the parser that supplies it.
//!
//! Parsers in the original (all `CXGSXmlReaderNode` based; addresses are from `libABK291` / Ghidra):
//!  * `CMetagameManager::ReadRankDataFromXML @00132058`, `ReadEconomyDataFromXML`, `ReadCharacterLevellingDataFromXML @0013194c`
//!  * `TKartInfo::Parse @00124ec0`, `TKartTier::Parse @00124dbc`, `TKartStatLevels::Parse @00124abc`, `TKartLevel::Parse @00124984`
//!  * `CEventDefinitionManager` campaign parser (CampaignEvent / Reward / RewardType) around @00102c00..001035b0
//!  * `CGachaPool::Parse @00111b..`, `CGachaToolbox::Parse`, `CGachaManager::ParsePools`, `ParseGachaXML @00111bb0`
//!  * `CSoftCurrencyShopManager::ParseXML @0017a3a8` (parts shop)

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------------------------
// tiny XML helpers (roxmltree)
// ---------------------------------------------------------------------------------------------------------------

pub type Node<'a, 'b> = roxmltree::Node<'a, 'b>;

pub fn read_text(path: &Path) -> Result<String, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(s.trim_start_matches('\u{feff}').to_string())
}

pub fn attr<'a>(n: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute(name)
}
pub fn attr_i32(n: &Node, name: &str) -> Option<i32> {
    n.attribute(name).and_then(|s| s.trim().parse::<i32>().ok())
}
pub fn attr_i64(n: &Node, name: &str) -> Option<i64> {
    n.attribute(name).and_then(|s| s.trim().parse::<i64>().ok())
}
pub fn attr_f32(n: &Node, name: &str) -> Option<f32> {
    n.attribute(name).and_then(|s| s.trim().parse::<f32>().ok())
}
pub fn attr_bool(n: &Node, name: &str) -> Option<bool> {
    n.attribute(name).map(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}
/// `<Name value="..."/>` child helper (used all over `energy.xml` / `economy.xml`).
pub fn child<'a, 'b>(n: &Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}
pub fn child_value_f32(n: &Node, name: &str) -> Option<f32> {
    let c = child(n, name)?;
    c.attribute("value").and_then(|s| s.trim().parse().ok()).or_else(|| c.text().and_then(|t| t.trim().parse().ok()))
}
pub fn child_value_i32(n: &Node, name: &str) -> Option<i32> {
    let c = child(n, name)?;
    c.attribute("value").and_then(|s| s.trim().parse().ok()).or_else(|| c.text().and_then(|t| t.trim().parse().ok()))
}
pub fn elems<'a, 'b>(n: &Node<'a, 'b>, name: &'static str) -> impl Iterator<Item = Node<'a, 'b>> {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}

/// Duration strings used by `dailyraces.xml` / tournaments: `1d`, `7d`, `6h`, `30m`, `45s` (seconds if bare).
pub fn parse_duration(s: &str) -> u64 {
    let s = s.trim();
    let (num, unit) = match s.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => (&s[..i], &s[i..]),
        None => (s, "s"),
    };
    let n: u64 = num.parse().unwrap_or(0);
    n * match unit.trim() {
        "d" => 86400,
        "h" => 3600,
        "m" => 60,
        _ => 1,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Types shared with the profile
// ---------------------------------------------------------------------------------------------------------------

/// `EKartStat` (order proven by `CPlayerInfo::UpgradeKart @0015139c` analytics switch: 0 Upgrade_TopSpeed, 1 Upgrade_Acceleration,
/// 2 Upgrade_Strength, 3 Upgrade_Handling, 4 Upgrade_Grip) and by `TKartInfo::Parse` base-level offsets +0x40..+0x50.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stat {
    TopSpeed = 0,
    Acceleration = 1,
    Strength = 2,
    Handling = 3,
    Grip = 4,
}
impl Stat {
    pub const ALL: [Stat; 5] = [Stat::TopSpeed, Stat::Acceleration, Stat::Strength, Stat::Handling, Stat::Grip];
    pub fn name(self) -> &'static str {
        match self {
            Stat::TopSpeed => "TopSpeed",
            Stat::Acceleration => "Acceleration",
            Stat::Strength => "Strength",
            Stat::Handling => "Handling",
            Stat::Grip => "Grip",
        }
    }
    pub fn from_name(s: &str) -> Option<Stat> {
        Stat::ALL.iter().copied().find(|st| st.name().eq_ignore_ascii_case(s))
    }
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// `EKartRarity` (Common 0, Rare 1, Epic 2, Legendary 3; 4 = wildcard in `TXP::GetAmountGained @0020b404`).
/// Upgrade part rarity (`TKartLevel::Parse @00124984`): Common 0, Rare 1, Epic 2; 3 = wildcard.
pub fn rarity_index(s: &str) -> Option<i32> {
    match s.to_ascii_lowercase().as_str() {
        "common" => Some(0),
        "rare" => Some(1),
        "epic" => Some(2),
        "legendary" => Some(3),
        _ => None,
    }
}

/// One `<Reward .../>` / `<Item .../>` entry (`CTypeManager::ParseType`): `Type` / `SubType` / `Quantity`.
#[derive(Clone, Debug, PartialEq)]
pub struct Reward {
    /// "Currency", "Token", "Statistic", "Character", "Kart", "Powerup", "Feature", "Durable" ...
    pub kind: String,
    pub sub: String,
    pub quantity: i32,
    /// `RewardType` of campaign rewards: 0 OneStar, 1 TwoStar, 2 ThreeStar, 3 (4th string, not present in the data), 4 = none/always
    /// (`TCampaignEventData` rewards +0x18, parser @00102c..; default 4 when the attribute is missing).
    pub star: u8,
}
impl Reward {
    pub fn parse(n: &Node) -> Reward {
        let star = match attr(n, "RewardType").map(|s| s.to_ascii_lowercase()) {
            Some(s) if s == "onestar" => 0,
            Some(s) if s == "twostar" => 1,
            Some(s) if s == "threestar" => 2,
            _ => 4,
        };
        Reward {
            kind: attr(n, "Type").unwrap_or("").to_string(),
            sub: attr(n, "SubType").unwrap_or("").to_string(),
            quantity: attr_i32(n, "Quantity").unwrap_or(1),
            star,
        }
    }
    pub fn is(&self, kind: &str, sub: &str) -> bool {
        self.kind == kind && self.sub == sub
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ranklist.xml
// ---------------------------------------------------------------------------------------------------------------

/// `CMetagameManager` rank table (stride 0x14 at +0x45fc, count +0x45f8): `ReadRankDataFromXML @00132058`.
#[derive(Clone, Debug)]
pub struct RankDef {
    pub min_xp: i32,
    pub max_xp: i32,
    pub max_energy: i32,
    pub rewards: Vec<Reward>,
}

// ---------------------------------------------------------------------------------------------------------------
// energy.xml
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EnergyCfg {
    pub starting_amount: i32,
    /// seconds per energy unit (`RechargeTimePerBlock`)
    pub recharge_secs: i64,
    pub gem_refill_cost: i32,
    pub advert_recharge_absolute: i32,
    pub advert_recharge_ratio: f32,
}

// ---------------------------------------------------------------------------------------------------------------
// metagame.xml
// ---------------------------------------------------------------------------------------------------------------

/// `MetagameData::TXP` entry (kart rarity, part rarity, multiplier); wildcard kart rarity 4, wildcard part rarity 3.
#[derive(Clone, Copy, Debug)]
pub struct XpUpgrade {
    pub kart_rarity: i32,
    pub part_rarity: i32,
    pub multiplier: i32,
}
#[derive(Clone, Debug, Default)]
pub struct MetagameCfg {
    pub xp: Vec<XpUpgrade>,
    pub coin_subst_min_clamp: f32,
    pub coin_subst_variance: f32,
    pub coin_subst: Vec<XpUpgrade>,
    pub leaderboard_segments: Vec<(i32, i32, i32)>,
}

// ---------------------------------------------------------------------------------------------------------------
// kart definitions + upgrade levels
// ---------------------------------------------------------------------------------------------------------------

/// `TKartTier` (0x90 bytes) as read by `TKartTier::Parse @00124dbc` from kartdefinition_*.xml `<Tier>`.
#[derive(Clone, Debug, Default)]
pub struct KartTierDef {
    pub name: String,
    pub stars: i32,
    pub visual_model: String,
    /// BLUE0001 blueprint tokens needed to go to the next tier (+0x24)
    pub token_cost: i32,
    /// `spdCCIncrease` / `accCCIncrease` / `strCCIncrease` / `hndCCIncrease` / `grpCCIncrease` per stat level (stat order TopSpeed, Acc, Str, Hnd, Grp)
    pub cc_increase: [i32; 5],
}

/// `TKartInfo` (0x60 bytes) as read by `TKartInfo::Parse @00124ec0`.
#[derive(Clone, Debug, Default)]
pub struct KartDef {
    pub base_id: String,
    pub base_name: String,
    /// +0x30 (0 Common .. 3 Legendary)
    pub rarity: i32,
    pub rarity_name: String,
    pub theme: String,
    /// +0x2c, index of the theme/episode (Seedway 0, RockyRoad 1, Air 2, Stunt 3, SubZero 4 - file order)
    // UNRESOLVED: the theme string -> index order (strcasecmp chain @00124f30..) was not decoded; this is the kartdefinition_* file order.
    pub theme_index: i32,
    /// +0x34, -1 when absent (displayed rank, 1 based)
    pub unlock_rank: i32,
    /// +0x38 blueprint tokens (BLUE0001) needed by `CPlayerInfo::PurchaseKart @0014d1f0`
    pub unlock_cost: i32,
    /// +0x3c
    pub base_cc: i32,
    /// +0x40 spd, +0x44 acc, +0x48 str, +0x4c hnd, +0x50 grp (stat enum order)
    pub base_levels: [i32; 5],
    pub is_power_up_kart: bool,
    pub tiers: Vec<KartTierDef>,
}

/// One entry of the per-stat level list (`TKartLevel`, 0x14 bytes: +0 Modifier, +4 Cost (tokens), +8 Coins, +0xc Rarity).
#[derive(Clone, Copy, Debug, Default)]
pub struct StatLevel {
    pub modifier: f32,
    pub cost_tokens: i32,
    pub coins: i32,
    pub rarity: i32,
}
/// One tier of kartupgradelevels.xml: the five stat lists.
#[derive(Clone, Debug, Default)]
pub struct TierLevels {
    pub stats: [Vec<StatLevel>; 5],
}

// ---------------------------------------------------------------------------------------------------------------
// eventdefinitiondata.xml
// ---------------------------------------------------------------------------------------------------------------

/// `TCampaignEventData` (0x2c bytes): <CampaignEvent tag eventIndex campaignCC energyCost hidden aiSkillMin aiSkillMax .../>
#[derive(Clone, Debug, Default)]
pub struct CampaignEvent {
    pub tag: String,
    pub event_index: i32,
    pub campaign_cc: i32,
    pub energy_cost: i32,
    pub hidden: bool,
    pub ai_skill_min: Option<f32>,
    pub ai_skill_max: Option<f32>,
    pub disable_catchup: bool,
    pub rewards: Vec<Reward>,
}
#[derive(Clone, Debug, Default)]
pub struct EventDef {
    pub index: i32,
    pub episode: String,
    pub game_mode: String,
    pub tier: i32,
    pub event: i32,
    pub stage: i32,
    pub energy_cost: i32,
}
#[derive(Clone, Debug, Default)]
pub struct EventDefData {
    pub random_cc_range_max: i32,
    pub random_cc_range_min: i32,
    pub random_cc_range_increment: i32,
    pub tutorial_level_count: i32,
    pub default_xp_reward: i32,
    pub default_coin_reward: i32,
    pub max_range_to_search_for_rewards: i32,
    pub campaign: Vec<CampaignEvent>,
    pub events: Vec<EventDef>,
    pub daily_race_ftue_level: Option<i32>,
}

// ---------------------------------------------------------------------------------------------------------------
// campaignmapdefinition.xml
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct MapTile {
    pub x: i32,
    pub y: i32,
    pub tex_index: i32,
    pub rotation: i32,
    pub event_marker: Option<i32>,
}
#[derive(Clone, Debug, Default)]
pub struct MapChapter {
    pub title: String,
    pub tiles: Vec<MapTile>,
}
#[derive(Clone, Debug, Default)]
pub struct CampaignMap {
    pub width: i32,
    pub height: i32,
    pub chapters: Vec<MapChapter>,
}

// ---------------------------------------------------------------------------------------------------------------
// gachapools.xml
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GachaItem {
    pub weighting: i32,
    pub reward: Reward,
    pub min_quantity: i32,
    pub max_quantity: i32,
    /// `QuantityRandomType` 0 Normal, 1 (second string), 2 fixed; absent -> 0.
    // UNRESOLVED: the three strings compared at CGachaPool::Parse @00110d5c..00110d98 and the default were not read; only "Normal" occurs in the data.
    pub quantity_random_type: i32,
}
#[derive(Clone, Debug, Default)]
pub struct GachaPool {
    pub id: String,
    pub items: Vec<GachaItem>,
}
#[derive(Clone, Debug)]
pub struct GachaSpin {
    pub pool_id: String,
    pub num_spins: i32,
    pub chance: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Toolbox {
    /// "Gacha" (type 2 in code), "Premium", "Ad"
    pub kind: String,
    pub required_rank: i32,
    pub name: String,
    pub image: String,
    pub token_cost: i32,
    pub gem_cost: i32,
    pub gem_multi_spin_cost: i32,
    pub multi_spin_amount: i32,
    pub daily_reward_weighting: i32,
    pub spins: Vec<GachaSpin>,
}
#[derive(Clone, Debug, Default)]
pub struct GachaData {
    pub toolboxes: Vec<Toolbox>,
    pub pools: Vec<GachaPool>,
    pub token_type: String,
    pub ad_toolbox_spin_interval: i64,
    pub ftue_reward: Option<GachaItem>,
}

// ---------------------------------------------------------------------------------------------------------------
// economy.xml (gameplay/misc)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Gradient {
    pub m: f32,
    pub c: f32,
    pub t: f32,
}
#[derive(Clone, Debug, Default)]
pub struct GiftChance {
    pub chance: f32,
    pub rewards: Vec<(String, String)>, // (type, specific)
}
#[derive(Clone, Debug, Default)]
pub struct Economy {
    pub ftue_kart: String,
    pub ftue_character: String,
    pub ftue_first_ability_timer: f32,
    pub ftue_cc_variation_percent: f32,
    pub tournament_unlock_rank: i32,
    pub daily_race_unlock_rank: i32,
    pub parts_shop_unlock_rank: i32,
    pub buy_toolbox_rank: i32,
    pub powerup_ftue_unlock: i32,
    pub powerup_level_requirement: i32,
    pub luxury_toolbox_ftue_unlock: i32,
    pub loading_ad_unlock_campaign_level: i32,
    pub shop_toolbox_ad_unlock_rank: i32,
    pub weekly_retry_cost: Vec<(i32, i32)>,
    pub edit_license_cost: i32,
    pub missing_materials_gems: Vec<(String, i32, bool)>,
    pub upgrade_cost_gradients: Vec<Gradient>,
    pub upgrade_cost_weights: HashMap<String, f32>,
    pub challenge_skip_cost: Gradient,
    pub bird_ability_cost: Gradient,
    pub bird_ability_cost_before_race: i32,
    pub bird_ability_max: i32,
    pub gems_rewards: HashMap<String, f32>,
    pub earnings_gradients: Vec<Gradient>,
    pub earnings: HashMap<String, f32>,
    pub starting_coins: i32,
    pub starting_gems: i32,
    pub roulette: Vec<(String, f32, f32)>, // (name, weight, loyalty_bonus)
    pub unlock_all_episodes: bool,
    pub campaign_repeat_reward: bool,
    pub cc_diff_limit: i32,
    pub gift_chances: BTreeMap<String, Vec<GiftChance>>,
    /// `<CurrencyConversion id="SoftToHard">` rate list (sourceAmount, targetAmount)
    pub soft_to_hard: Vec<(i32, i32)>,
    pub energy_cost: Vec<(String, String, i32)>, // (type, difficulty, cost)
    pub telepod_lockout: i64,
    /// `<Score fMultiplier fAddition fOneStarMultiplier fTwoStarMultiplier fThreeStarMultiplier>` (CMetagameManager +0x46f4..+0x4704)
    pub score_multiplier: f32,
    pub score_addition: f32,
    pub star_multipliers: [f32; 3],
    pub starter_bundle_unlock_rank: i32,
    pub blueprint_conversion_multiplier: f32,
    /// `<DifficultyAdjust><Difficulty value relativeCC>` in order VERYEASY, EASY, MEDIUM, HARD, EXTREME (CMetagameManager +0x4708..)
    pub difficulty_adjust: Vec<(String, i32)>,
    pub ai_skill_base: Vec<(String, f32)>,
    pub ai_skill_race: (f32, f32),
    pub ai_skill_variance: (f32, f32),
    pub mega_coin: i32,
    pub end_of_session_ad: (i64, i32, i32), // rewardDelay, energyReward, energyTrigger
    pub race_ability_costs: Vec<i32>,
}

// ---------------------------------------------------------------------------------------------------------------
// misc gameplay files
// ---------------------------------------------------------------------------------------------------------------

/// characterlevelling.xml: `ReadCharacterLevellingDataFromXML @0013194c`
#[derive(Clone, Debug, Default)]
pub struct CharacterLevelling {
    /// (xp, level) thresholds; level 1 below the first one (`GetLevelFromXP @0012d418`)
    pub thresholds: Vec<(i32, i32)>,
    pub race: Vec<(i32, i32)>,       // (position, xp)
    pub time_trial: Vec<(f32, i32)>, // (time_diff, xp)
    pub fruit_rush: Vec<(f32, i32)>, // (remaining_fruit_proportion, xp)
    pub boss_battle: Vec<(i32, i32)>,
    pub challenge_mode: Vec<(i32, i32)>, // (stars, xp)
}

#[derive(Clone, Debug, Default)]
pub struct AchievementDef {
    pub game_center_id: String,
    pub google_play_id: String,
    pub value_tracker: Option<String>,
    pub max_value: i32,
    pub grade: i32,
}

#[derive(Clone, Debug, Default)]
pub struct ChallengeDef {
    pub class: String,
    pub name: String,
    pub category: String,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct EpisodeConfig {
    pub name: String,
    pub kart_type: String,
    pub powerup_kart: String,
    pub energy_cost: i32,
    pub kart_pack: String,
    pub tiers: Vec<Vec<String>>, // event definition files per tier
}

#[derive(Clone, Debug, Default)]
pub struct MpRankCfg {
    pub sp_rank: Vec<(String, i32)>,
    pub tbm: Vec<(String, i32)>,
    pub sbm: Vec<(String, i32)>,
}

#[derive(Clone, Debug, Default)]
pub struct DailyReward {
    pub start_time: u64,
    pub duration: u64,
    pub repeat_time: u64,
    pub race_rewards: Vec<Reward>,
    pub milestones: Vec<(i32, Vec<Reward>)>,
}
#[derive(Clone, Debug, Default)]
pub struct DailyRaces {
    pub mega_coin_value: i32,
    pub rewards: Vec<DailyReward>,
    pub multiplier_easy_medium_hard: (i32, i32, i32),
}

#[derive(Clone, Debug, Default)]
pub struct KartTypeFile {
    pub id: String,
    pub karts: Vec<KartTypeKart>,
}
#[derive(Clone, Debug, Default)]
pub struct KartTypeKart {
    pub id: String,
    pub name: String,
    pub tier: i32,
    pub upgrades_to: String,
    pub energy: i32,
    pub num_upgrade_levels: i32,
    pub cost_type: String,
    pub cost_amount: i32,
}

#[derive(Clone, Debug, Default)]
pub struct FtueState {
    pub name: String,
    pub previous_states: Vec<String>,
    pub prerequisites: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct MigrationRewards {
    /// (name, reward currency, conversionRate, cap, min)
    pub currency: Vec<(String, String, f32, i32, i32)>,
    pub kart_gems: Vec<(String, i32)>,
}

#[derive(Clone, Debug, Default)]
pub struct TournamentDef {
    pub id: String,
    pub start: u64,
    pub duration: u64,
    pub kind: String,
    pub sub_type: i32,
}

/// Everything. `MetaData::load` fails only if a required core file (ranks, energy, kart definitions) is missing; the
/// optional files simply stay empty and a note is added to `warnings`.
#[derive(Clone, Debug, Default)]
pub struct MetaData {
    pub root: PathBuf,
    pub warnings: Vec<String>,
    pub ranks: Vec<RankDef>,
    pub energy: Option<EnergyCfg>,
    pub metagame: MetagameCfg,
    pub karts: Vec<KartDef>,
    /// kart base id -> tiers -> five stat lists
    pub upgrade_levels: HashMap<String, Vec<TierLevels>>,
    pub events: EventDefData,
    pub map: CampaignMap,
    pub gacha: GachaData,
    pub economy: Economy,
    pub char_levelling: CharacterLevelling,
    pub achievements: Vec<AchievementDef>,
    pub challenges: Vec<ChallengeDef>,
    pub episodes: Vec<EpisodeConfig>,
    pub mp_rank: MpRankCfg,
    pub daily_races: DailyRaces,
    pub kart_types: Vec<KartTypeFile>,
    pub ftue_states: Vec<FtueState>,
    pub migration: MigrationRewards,
    pub features: Vec<(String, bool)>,
    pub tracks: Vec<(i32, Vec<i32>)>, // (theme num, run nums)
    pub track_times: Vec<Vec<f32>>,
    pub news_feed: Vec<(String, i32)>,
    pub tournaments: Vec<TournamentDef>,
    pub tournament_types: Vec<String>,
    pub tournament_adjectives: usize,
    pub unlock_info: Vec<(Vec<String>, i32)>,
    pub score_config_present: bool,
    pub store: crate::meta::store::StoreData,
    pub type_images: HashMap<(String, String), String>,
    pub lmp_tracks: Vec<(String, Vec<(String, i32)>)>,
    pub powerup_tweakable_names: Vec<String>,
    pub gameplay_tweakable_sections: Vec<String>,
}

impl MetaData {
    /// `root` is the `assets292` directory.
    pub fn load(root: &Path) -> Result<MetaData, String> {
        let mut d = MetaData { root: root.to_path_buf(), ..Default::default() };
        let global = root.join("xml/xml/global");
        let misc = root.join("xml/gameplay/misc");

        // ---- required ----
        let t = read_text(&global.join("ranklist.xml"))?;
        d.ranks = parse_ranks(&t)?;
        let t = read_text(&global.join("energy.xml"))?;
        d.energy = Some(parse_energy(&t)?);
        let mut karts = Vec::new();
        // file order = episode order (Seedway, RockyRoad, Air, Stunt, SubZero)
        for (i, f) in ["seedway", "rockyroad", "air", "stunt", "subzero"].iter().enumerate() {
            let t = read_text(&global.join(format!("kartdefinition_{f}.xml")))?;
            karts.extend(parse_kart_defs(&t, i as i32)?);
        }
        d.karts = karts;
        let t = read_text(&global.join("kartupgradelevels.xml"))?;
        d.upgrade_levels = parse_upgrade_levels(&t)?;

        // ---- optional (best effort) ----
        macro_rules! opt {
            ($path:expr, $f:expr) => {
                match read_text(&$path).and_then(|t| $f(&t).map_err(|e| format!("{}: {e}", $path.display()))) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        d.warnings.push(e);
                        None
                    }
                }
            };
        }
        if let Some(v) = opt!(global.join("metagame.xml"), parse_metagame) {
            d.metagame = v;
        }
        if let Some(v) = opt!(global.join("eventdefinitiondata.xml"), parse_event_defs) {
            d.events = v;
        }
        if let Some(v) = opt!(global.join("campaignmapdefinition.xml"), parse_map) {
            d.map = v;
        }
        if let Some(v) = opt!(global.join("gachapools.xml"), parse_gacha) {
            d.gacha = v;
        }
        if let Some(v) = opt!(misc.join("economy.xml"), parse_economy) {
            d.economy = v;
        }
        if let Some(v) = opt!(misc.join("characterlevelling.xml"), parse_char_levelling) {
            d.char_levelling = v;
        }
        if let Some(v) = opt!(misc.join("achievements.xml"), parse_achievements) {
            d.achievements = v;
        }
        if let Some(v) = opt!(misc.join("challenges.xml"), parse_challenges) {
            d.challenges = v;
        }
        if let Some(v) = opt!(misc.join("episode_config.xml"), parse_episode_config) {
            d.episodes = v;
        }
        if let Some(v) = opt!(misc.join("mprank_config.xml"), parse_mp_rank) {
            d.mp_rank = v;
        }
        if let Some(v) = opt!(misc.join("tracktimes.xml"), parse_track_times) {
            d.track_times = v;
        }
        if let Some(v) = opt!(global.join("dailyraces.xml"), parse_daily_races) {
            d.daily_races = v;
        }
        for f in ["ep1speedway", "ep2offroad", "ep3flying", "ep4stunt", "ep5snow"] {
            if let Some(v) = opt!(misc.join(format!("karttype_{f}.xml")), parse_kart_type) {
                d.kart_types.push(v);
            }
        }
        if let Some(v) = opt!(global.join("gameftueprerequisites.xml"), parse_ftue) {
            d.ftue_states = v;
        }
        if let Some(v) = opt!(global.join("savemigrationrewards.xml"), parse_migration) {
            d.migration = v;
        }
        if let Some(v) = opt!(global.join("featureconfig.xml"), parse_features) {
            d.features = v;
        }
        if let Some(v) = opt!(global.join("tracklist.xml"), parse_tracklist) {
            d.tracks = v;
        }
        if let Some(v) = opt!(global.join("newsfeed.xml"), parse_newsfeed) {
            d.news_feed = v;
        }
        if let Some(v) = opt!(global.join("types.xml"), parse_types) {
            d.type_images = v;
        }
        if let Some(v) = opt!(global.join("lmptracks.xml"), parse_lmp) {
            d.lmp_tracks = v;
        }
        if let Some(v) = opt!(root.join("xml/unlockdata/unlockinfo.xml"), parse_unlock_info) {
            d.unlock_info = v;
        }
        d.score_config_present = global.join("scoreconfig.xml").exists();
        if let Some(v) = opt!(root.join("xml/xml/tournament/tournament.xml"), parse_tournaments) {
            d.tournaments = v;
        }
        if let Some(v) = opt!(root.join("xml/xml/tournament/tournamenttypes.xml"), parse_tournament_types) {
            d.tournament_types = v;
        }
        if let Some(v) = opt!(global.join("tournamentnames.xml"), |t: &str| -> Result<usize, String> {
            let doc = roxmltree::Document::parse(t).map_err(|e| e.to_string())?;
            Ok(doc.descendants().filter(|n| n.tag_name().name() == "Adjective").count())
        }) {
            d.tournament_adjectives = v;
        }
        if let Some(v) = opt!(misc.join("poweruptweakables.xml"), |t: &str| -> Result<Vec<String>, String> {
            let doc = roxmltree::Document::parse(t).map_err(|e| e.to_string())?;
            Ok(doc.root_element().children().filter(|n| n.is_element()).map(|n| n.tag_name().name().to_string()).collect())
        }) {
            d.powerup_tweakable_names = v;
        }
        if let Some(v) = opt!(misc.join("gameplaytweakables.xml"), |t: &str| -> Result<Vec<String>, String> {
            let doc = roxmltree::Document::parse(t).map_err(|e| e.to_string())?;
            Ok(doc.root_element().children().filter(|n| n.is_element()).map(|n| n.tag_name().name().to_string()).collect())
        }) {
            d.gameplay_tweakable_sections = v;
        }
        match crate::meta::store::StoreData::load(&root.join("xml/store")) {
            Ok(s) => d.store = s,
            Err(e) => d.warnings.push(e),
        }
        Ok(d)
    }

    pub fn kart(&self, base_id: &str) -> Option<&KartDef> {
        self.karts.iter().find(|k| k.base_id == base_id)
    }
    pub fn kart_index(&self, base_id: &str) -> Option<usize> {
        self.karts.iter().position(|k| k.base_id == base_id)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// parsers
// ---------------------------------------------------------------------------------------------------------------

fn doc(t: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse(t).map_err(|e| e.to_string())
}

pub fn parse_ranks(t: &str) -> Result<Vec<RankDef>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Rank")
        .map(|r| RankDef {
            min_xp: attr_i32(&r, "iMinXP").unwrap_or(0),
            max_xp: attr_i32(&r, "iMaxXP").unwrap_or(0),
            max_energy: attr_i32(&r, "maxEnergy").unwrap_or(0),
            rewards: elems(&r, "Reward").map(|x| Reward::parse(&x)).collect(),
        })
        .collect())
}

pub fn parse_energy(t: &str) -> Result<EnergyCfg, String> {
    let d = doc(t)?;
    let r = d.root_element();
    Ok(EnergyCfg {
        starting_amount: child_value_i32(&r, "StartingAmount").ok_or("energy.xml: StartingAmount")?,
        recharge_secs: child_value_i32(&r, "RechargeTimePerBlock").ok_or("energy.xml: RechargeTimePerBlock")? as i64,
        gem_refill_cost: child_value_i32(&r, "GemRefillCost").unwrap_or(0),
        advert_recharge_absolute: child_value_i32(&r, "AdvertRechargeAbsolute").unwrap_or(0),
        advert_recharge_ratio: child_value_f32(&r, "AdvertRechargeRatio").unwrap_or(0.0),
    })
}

fn pct(s: &str) -> f32 {
    s.trim().trim_end_matches('%').parse::<f32>().unwrap_or(0.0) / 100.0
}

pub fn parse_metagame(t: &str) -> Result<MetagameCfg, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut m = MetagameCfg::default();
    let read_ups = |n: &Node| -> Vec<XpUpgrade> {
        elems(n, "Upgrade")
            .map(|u| XpUpgrade {
                kart_rarity: attr(&u, "kartRarity").and_then(rarity_index).unwrap_or(4),
                part_rarity: attr(&u, "partRarity").and_then(rarity_index).unwrap_or(3),
                multiplier: attr_i32(&u, "amountMultiplier").or_else(|| attr_f32(&u, "amountMultiplier").map(|f| f as i32)).unwrap_or(1),
            })
            .collect()
    };
    if let Some(x) = child(&r, "XP") {
        m.xp = read_ups(&x);
    }
    if let Some(c) = child(&r, "CoinSubstitution") {
        m.coin_subst_min_clamp = attr(&c, "minClamp").map(pct).unwrap_or(0.0);
        m.coin_subst_variance = attr(&c, "randomVariance").map(pct).unwrap_or(0.0);
        m.coin_subst = read_ups(&c);
    }
    if let Some(l) = child(&r, "Leaderboard") {
        m.leaderboard_segments = elems(&l, "Segment")
            .map(|s| (attr_i32(&s, "rankMin").unwrap_or(0), attr_i32(&s, "rankMax").unwrap_or(0), attr_i32(&s, "segmentID").unwrap_or(0)))
            .collect();
    }
    Ok(m)
}

pub fn parse_kart_defs(t: &str, theme_index: i32) -> Result<Vec<KartDef>, String> {
    let d = doc(t)?;
    let mut out = Vec::new();
    for k in elems(&d.root_element(), "Kart") {
        let rarity_name = attr(&k, "rarity").unwrap_or("Common").to_string();
        let mut kd = KartDef {
            base_id: attr(&k, "baseID").unwrap_or("").to_string(),
            base_name: attr(&k, "baseName").unwrap_or("").to_string(),
            rarity: rarity_index(&rarity_name).unwrap_or(0),
            rarity_name,
            theme: attr(&k, "theme").unwrap_or("").to_string(),
            theme_index,
            unlock_rank: attr_i32(&k, "unlockRank").unwrap_or(-1), // @00125.. default 0xffffffff
            unlock_cost: attr_i32(&k, "unlockCost").unwrap_or(0),
            base_cc: attr_i32(&k, "baseCC").unwrap_or(0),
            base_levels: [0; 5],
            is_power_up_kart: attr_bool(&k, "isPowerUpKart").unwrap_or(false),
            tiers: Vec::new(),
        };
        kd.base_levels = [
            attr_i32(&k, "baseSpdLvl").unwrap_or(0),
            attr_i32(&k, "baseAccLvl").unwrap_or(0),
            attr_i32(&k, "baseStrLvl").unwrap_or(0),
            attr_i32(&k, "baseHndLvl").unwrap_or(0),
            attr_i32(&k, "baseGrpLvl").unwrap_or(0),
        ];
        for tn in elems(&k, "Tier") {
            kd.tiers.push(KartTierDef {
                name: attr(&tn, "name").unwrap_or("").to_string(),
                stars: attr_i32(&tn, "stars").unwrap_or(0),
                visual_model: attr(&tn, "visualModel").unwrap_or("").to_string(),
                token_cost: attr_i32(&tn, "tokenCost").unwrap_or(0),
                cc_increase: [
                    attr_i32(&tn, "spdCCIncrease").unwrap_or(0),
                    attr_i32(&tn, "accCCIncrease").unwrap_or(0),
                    attr_i32(&tn, "strCCIncrease").unwrap_or(0),
                    attr_i32(&tn, "hndCCIncrease").unwrap_or(0),
                    attr_i32(&tn, "grpCCIncrease").unwrap_or(0),
                ],
            });
        }
        out.push(kd);
    }
    Ok(out)
}

pub fn parse_upgrade_levels(t: &str) -> Result<HashMap<String, Vec<TierLevels>>, String> {
    let d = doc(t)?;
    let mut out = HashMap::new();
    for k in elems(&d.root_element(), "Kart") {
        let name = attr(&k, "name").unwrap_or("").to_string();
        let mut tiers = Vec::new();
        for tn in elems(&k, "Tier") {
            let mut tl = TierLevels::default();
            for s in elems(&tn, "Stats") {
                let Some(stat) = attr(&s, "Stat").and_then(Stat::from_name) else { continue };
                tl.stats[stat.idx()].push(StatLevel {
                    modifier: attr_f32(&s, "Modifier").unwrap_or(0.0),
                    cost_tokens: attr_i32(&s, "Cost").unwrap_or(0),
                    coins: attr_i32(&s, "Coins").unwrap_or(0),
                    rarity: attr(&s, "Rarity").and_then(rarity_index).unwrap_or(0),
                });
            }
            tiers.push(tl);
        }
        out.insert(name, tiers);
    }
    Ok(out)
}

pub fn parse_event_defs(t: &str) -> Result<EventDefData, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut e = EventDefData {
        random_cc_range_max: attr_i32(&r, "randomCCRangeMax").unwrap_or(0),
        random_cc_range_min: attr_i32(&r, "randomCCRangeMin").unwrap_or(0),
        random_cc_range_increment: attr_i32(&r, "randomCCRangeIncrement").unwrap_or(0),
        tutorial_level_count: attr_i32(&r, "tutorialLevelCount").unwrap_or(0),
        ..Default::default()
    };
    if let Some(c) = child(&r, "Campaign") {
        e.default_xp_reward = attr_i32(&c, "defaultXPReward").unwrap_or(0);
        e.default_coin_reward = attr_i32(&c, "defaultCoinReward").unwrap_or(0);
        e.max_range_to_search_for_rewards = attr_i32(&c, "iMaxRangeToSearchForRewards").unwrap_or(0);
        for ce in elems(&c, "CampaignEvent") {
            e.campaign.push(CampaignEvent {
                tag: attr(&ce, "tag").unwrap_or("").to_string(),
                event_index: attr_i32(&ce, "eventIndex").unwrap_or(0),
                campaign_cc: attr_i32(&ce, "campaignCC").unwrap_or(0),
                energy_cost: attr_i32(&ce, "energyCost").unwrap_or(1), // GetCampaignEnergyCost @000ffc30 falls back to 1
                hidden: attr_bool(&ce, "hidden").unwrap_or(false),
                ai_skill_min: attr_f32(&ce, "aiSkillMin"),
                ai_skill_max: attr_f32(&ce, "aiSkillMax"),
                disable_catchup: attr_bool(&ce, "disableCatchup").unwrap_or(false),
                rewards: elems(&ce, "Reward").map(|x| Reward::parse(&x)).collect(),
            });
        }
    }
    if let Some(dr) = child(&r, "DailyRace") {
        e.daily_race_ftue_level = attr_i32(&dr, "ftueLevel");
    }
    if let Some(ed) = child(&r, "EventData") {
        for ev in elems(&ed, "Event") {
            e.events.push(EventDef {
                index: attr_i32(&ev, "index").unwrap_or(0),
                episode: attr(&ev, "episode").unwrap_or("").to_string(),
                game_mode: attr(&ev, "gameMode").unwrap_or("").to_string(),
                tier: attr_i32(&ev, "tier").unwrap_or(0),
                event: attr_i32(&ev, "event").unwrap_or(0),
                stage: attr_i32(&ev, "stage").unwrap_or(0),
                energy_cost: attr_i32(&ev, "energyCost").unwrap_or(1),
            });
        }
    }
    Ok(e)
}

pub fn parse_map(t: &str) -> Result<CampaignMap, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut m = CampaignMap { width: attr_i32(&r, "width").unwrap_or(0), height: attr_i32(&r, "height").unwrap_or(0), chapters: vec![] };
    for c in elems(&r, "Chapter") {
        let mut ch = MapChapter { title: attr(&c, "title").unwrap_or("").to_string(), tiles: vec![] };
        for tl in elems(&c, "MapTile") {
            ch.tiles.push(MapTile {
                x: attr_i32(&tl, "x").unwrap_or(0),
                y: attr_i32(&tl, "y").unwrap_or(0),
                tex_index: attr_i32(&tl, "texIndex").unwrap_or(0),
                rotation: attr_i32(&tl, "rotation").unwrap_or(0),
                event_marker: child(&tl, "EventMarker").and_then(|e| attr_i32(&e, "campaignIndex")),
            });
        }
        m.chapters.push(ch);
    }
    Ok(m)
}

fn gacha_item(n: &Node) -> GachaItem {
    let mut reward = Reward::parse(n);
    let min = attr_i32(n, "MinQuantity").or_else(|| attr_i32(n, "Quantity")).unwrap_or(1);
    let max = attr_i32(n, "MaxQuantity").or_else(|| attr_i32(n, "Quantity")).unwrap_or(min);
    reward.quantity = min;
    GachaItem {
        weighting: attr_i32(n, "weighting").unwrap_or(1),
        reward,
        min_quantity: min,
        max_quantity: max,
        quantity_random_type: match attr(n, "QuantityRandomType").map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("normal") => 0,
            Some("fixed") | Some("constant") => 2,
            Some(_) => 1,
            None => 0,
        },
    }
}

pub fn parse_gacha(t: &str) -> Result<GachaData, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut g = GachaData::default();
    if let Some(tbs) = child(&r, "Toolboxes") {
        for tb in elems(&tbs, "Toolbox") {
            g.toolboxes.push(Toolbox {
                kind: attr(&tb, "type").unwrap_or("").to_string(),
                required_rank: attr_i32(&tb, "requiredRank").unwrap_or(-2),
                name: attr(&tb, "name").unwrap_or("").to_string(),
                image: attr(&tb, "imageTexture").unwrap_or("").to_string(),
                token_cost: attr_i32(&tb, "tokenCost").unwrap_or(0),
                gem_cost: attr_i32(&tb, "gemCost").unwrap_or(0),
                gem_multi_spin_cost: attr_i32(&tb, "gemMultiSpinCost").unwrap_or(0),
                multi_spin_amount: attr_i32(&tb, "multiSpinAmount").unwrap_or(0),
                daily_reward_weighting: attr_i32(&tb, "dailyRewardWeighting").unwrap_or(0),
                spins: elems(&tb, "Spin")
                    .map(|s| GachaSpin {
                        pool_id: attr(&s, "poolID").unwrap_or("").to_string(),
                        num_spins: attr_i32(&s, "numSpins").unwrap_or(1),
                        chance: attr_f32(&s, "chance").unwrap_or(1.0),
                    })
                    .collect(),
            });
        }
    }
    if let Some(gp) = child(&r, "GachaPools") {
        g.token_type = attr(&gp, "tokenType").unwrap_or("").to_string();
        g.ad_toolbox_spin_interval = attr_i64(&gp, "adToolboxSpinInterval").unwrap_or(0);
        g.ftue_reward = child(&gp, "FTUEReward").map(|n| gacha_item(&n));
        for p in elems(&gp, "Pool") {
            g.pools.push(GachaPool { id: attr(&p, "id").unwrap_or("").to_string(), items: elems(&p, "Item").map(|i| gacha_item(&i)).collect() });
        }
    }
    Ok(g)
}

fn gradient(n: &Node) -> Gradient {
    let f = |name: &str| child(n, name).and_then(|c| c.text()).and_then(|t| t.trim().parse::<f32>().ok()).unwrap_or(0.0);
    Gradient { m: f("m"), c: f("c"), t: f("t") }
}

pub fn parse_economy(t: &str) -> Result<Economy, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut e = Economy::default();
    if let Some(f) = child(&r, "FTUE") {
        e.ftue_kart = attr(&f, "kart").unwrap_or("SSKM").to_string();
        e.ftue_character = attr(&f, "character").unwrap_or("red").to_string();
        e.ftue_first_ability_timer = attr_f32(&f, "firstAbilityTimer").unwrap_or(0.0);
        e.ftue_cc_variation_percent = attr_f32(&f, "ccVariationPercent").unwrap_or(0.0);
        e.tournament_unlock_rank = child_value_i32(&f, "TournamentUnlockRank").unwrap_or(0);
        e.daily_race_unlock_rank = child_value_i32(&f, "DailyRaceUnlockRank").unwrap_or(0);
        e.parts_shop_unlock_rank = child_value_i32(&f, "PartsShopUnlockRank").unwrap_or(0);
        e.buy_toolbox_rank = child_value_i32(&f, "BuyToolboxRank").unwrap_or(0);
        e.powerup_ftue_unlock = child_value_i32(&f, "PowerupFTUEUnlock").unwrap_or(0);
        e.powerup_level_requirement = child_value_i32(&f, "PowerupLevelRequirement").unwrap_or(0);
        e.luxury_toolbox_ftue_unlock = child_value_i32(&f, "LuxuryToolboxFTUEUnlock").unwrap_or(0);
        e.loading_ad_unlock_campaign_level = child_value_i32(&f, "LoadingAdUnlockCampaignLevel").unwrap_or(0);
        e.shop_toolbox_ad_unlock_rank = child_value_i32(&f, "ShopToolboxAdUnlockRank").unwrap_or(0);
    }
    if let Some(w) = child(&r, "WeeklyRetryCost") {
        e.weekly_retry_cost = elems(&w, "repetition").map(|x| (attr_i32(&x, "value").unwrap_or(0), attr_i32(&x, "cost").unwrap_or(0))).collect();
    }
    e.edit_license_cost = child(&r, "EditLicenseCost").and_then(|c| attr_i32(&c, "value")).unwrap_or(0);
    if let Some(m) = child(&r, "MissingMaterialsConverter") {
        e.missing_materials_gems = elems(&m, "GemValue")
            .map(|g| (attr(&g, "type").unwrap_or("").to_string(), attr_i32(&g, "gems").unwrap_or(0), attr_bool(&g, "roundUp").unwrap_or(false)))
            .collect();
    }
    if let Some(u) = child(&r, "UpgradeCost") {
        e.upgrade_cost_gradients = elems(&u, "Gradient").map(|g| gradient(&g)).collect();
        for c in u.children().filter(|c| c.is_element() && c.tag_name().name().ends_with("CostWeight")) {
            if let Some(v) = c.text().and_then(|t| t.trim().parse::<f32>().ok()) {
                e.upgrade_cost_weights.insert(c.tag_name().name().to_string(), v);
            }
        }
    }
    if let Some(c) = child(&r, "ChallengeSkipCost").and_then(|c| child(&c, "Gradient")) {
        e.challenge_skip_cost = gradient(&c);
    }
    if let Some(b) = child(&r, "BirdAbilityCost") {
        if let Some(g) = child(&b, "Gradient") {
            e.bird_ability_cost = gradient(&g);
        }
        e.bird_ability_cost_before_race = child(&b, "CostBeforeRace").and_then(|c| c.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        e.bird_ability_max = child(&b, "MaxAbility").and_then(|c| c.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    }
    if let Some(g) = child(&r, "GemsRewards") {
        for c in g.children().filter(|c| c.is_element()) {
            if let Some(v) = c.text().and_then(|t| t.trim().parse::<f32>().ok()) {
                e.gems_rewards.insert(c.tag_name().name().to_string(), v);
            }
        }
    }
    if let Some(en) = child(&r, "Earnings") {
        e.earnings_gradients = elems(&en, "Gradient").map(|g| gradient(&g)).collect();
        for c in en.children().filter(|c| c.is_element() && c.tag_name().name() != "Gradient") {
            if let Some(v) = c.text().and_then(|t| t.trim().parse::<f32>().ok()) {
                e.earnings.insert(c.tag_name().name().to_string(), v);
            }
        }
        e.starting_coins = e.earnings.get("StartingCoins").copied().unwrap_or(0.0) as i32;
        e.starting_gems = e.earnings.get("StartingGems").copied().unwrap_or(0.0) as i32;
    }
    if let Some(ro) = child(&r, "Roulette") {
        for c in ro.children().filter(|c| c.is_element() && attr(c, "weight").is_some()) {
            e.roulette.push((c.tag_name().name().to_string(), attr_f32(&c, "weight").unwrap_or(0.0), attr_f32(&c, "loyalty_bonus").unwrap_or(0.0)));
        }
    }
    e.unlock_all_episodes = child(&r, "EpisodeUnlocking").and_then(|c| child(&c, "UnlockAll")).and_then(|c| c.text()).map(|t| t.trim() == "true").unwrap_or(false);
    if let Some(c) = child(&r, "Campaign") {
        e.campaign_repeat_reward = attr_bool(&c, "campaignRepeatReward").unwrap_or(false);
        e.cc_diff_limit = attr_i32(&c, "ccDiffLimit").unwrap_or(0);
    }
    if let Some(gb) = child(&r, "GiftBoxProbabilities").and_then(|g| child(&g, "Chances")) {
        for sect in gb.children().filter(|c| c.is_element()) {
            let list = elems(&sect, "Gift")
                .map(|g| GiftChance {
                    chance: attr_f32(&g, "chance").unwrap_or(0.0),
                    rewards: elems(&g, "Reward").map(|x| (attr(&x, "type").unwrap_or("").to_string(), attr(&x, "specific").unwrap_or("").to_string())).collect(),
                })
                .collect();
            e.gift_chances.insert(sect.tag_name().name().to_string(), list);
        }
    }
    if let Some(cc) = child(&r, "CurrencyConversion") {
        for conv in elems(&cc, "Conversion").filter(|c| attr(c, "id") == Some("SoftToHard")) {
            if let Some(rates) = child(&conv, "Rates") {
                e.soft_to_hard = elems(&rates, "Rate").map(|x| (attr_i32(&x, "sourceAmount").unwrap_or(0), attr_i32(&x, "targetAmount").unwrap_or(0))).collect();
            }
        }
    }
    if let Some(ec) = child(&r, "EnergyCost") {
        e.energy_cost = elems(&ec, "Cost")
            .map(|c| (attr(&c, "type").unwrap_or("").to_string(), attr(&c, "difficulty").unwrap_or("").to_string(), attr_i32(&c, "energyCost").unwrap_or(1)))
            .collect();
    }
    e.telepod_lockout = child(&r, "Telepods").and_then(|c| attr_i64(&c, "telepodLockOut")).unwrap_or(0);
    if let Some(s) = child(&r, "Score") {
        e.score_multiplier = attr_f32(&s, "fMultiplier").unwrap_or(0.0);
        e.score_addition = attr_f32(&s, "fAddition").unwrap_or(0.0);
        e.star_multipliers = [attr_f32(&s, "fOneStarMultiplier").unwrap_or(1.0), attr_f32(&s, "fTwoStarMultiplier").unwrap_or(1.0), attr_f32(&s, "fThreeStarMultiplier").unwrap_or(1.0)];
    }
    e.starter_bundle_unlock_rank = child(&r, "StarterBundle").and_then(|c| attr_i32(&c, "unlockRank")).unwrap_or(0);
    e.blueprint_conversion_multiplier = child(&r, "BlueprintConversion").and_then(|c| attr_f32(&c, "multiplier")).unwrap_or(1.0);
    if let Some(da) = child(&r, "DifficultyAdjust") {
        e.difficulty_adjust = elems(&da, "Difficulty").map(|x| (attr(&x, "value").unwrap_or("").to_string(), attr_i32(&x, "relativeCC").unwrap_or(0))).collect();
    }
    if let Some(ai) = child(&r, "AISkill") {
        if let Some(rc) = child(&ai, "Race") {
            e.ai_skill_race = (attr_f32(&rc, "min").unwrap_or(0.0), attr_f32(&rc, "max").unwrap_or(0.0));
        }
        if let Some(v) = child(&ai, "SkillVariance") {
            e.ai_skill_variance = (attr_f32(&v, "min").unwrap_or(0.0), attr_f32(&v, "max").unwrap_or(0.0));
        }
        if let Some(sb) = child(&ai, "SkillBase") {
            e.ai_skill_base = elems(&sb, "Difficulty").map(|x| (attr(&x, "value").unwrap_or("").to_string(), attr_f32(&x, "skillBase").unwrap_or(0.0))).collect();
        }
    }
    e.mega_coin = child(&r, "MegaCoin").and_then(|c| attr_i32(&c, "value")).unwrap_or(0);
    if let Some(s) = child(&r, "EndOfSessionAd") {
        e.end_of_session_ad = (attr_i64(&s, "rewardDelay").unwrap_or(0), attr_i32(&s, "energyReward").unwrap_or(0), attr_i32(&s, "energyTrigger").unwrap_or(0));
    }
    if let Some(c) = child(&r, "RaceAbilityCosts") {
        e.race_ability_costs = elems(&c, "Cost").filter_map(|x| attr_i32(&x, "value")).collect();
    }
    Ok(e)
}

pub fn parse_char_levelling(t: &str) -> Result<CharacterLevelling, String> {
    // the file has two top-level elements (<Levels> and <Rewards>): wrap them in one root
    let wrapped = format!("<W>{t}</W>");
    let d = doc(&wrapped)?;
    let mut c = CharacterLevelling::default();
    let list = |name: &str| d.descendants().find(|n| n.is_element() && n.tag_name().name() == name);
    if let Some(l) = list("Levels") {
        c.thresholds = elems(&l, "Threshold").map(|n| (attr_i32(&n, "xp").unwrap_or(0), attr_i32(&n, "level").unwrap_or(0))).collect();
    }
    if let Some(n) = list("Race") {
        c.race = elems(&n, "Reward").map(|x| (attr_i32(&x, "position").unwrap_or(0), attr_i32(&x, "xp").unwrap_or(0))).collect();
    }
    if let Some(n) = list("TimeTrial") {
        c.time_trial = elems(&n, "Reward").map(|x| (attr_f32(&x, "time_diff").unwrap_or(0.0), attr_i32(&x, "xp").unwrap_or(0))).collect();
    }
    if let Some(n) = list("FruitRush") {
        c.fruit_rush = elems(&n, "Reward").map(|x| (attr_f32(&x, "remaining_fruit_proportion").unwrap_or(0.0), attr_i32(&x, "xp").unwrap_or(0))).collect();
    }
    if let Some(n) = list("BossBattle") {
        c.boss_battle = elems(&n, "Reward").map(|x| (attr_i32(&x, "position").unwrap_or(0), attr_i32(&x, "xp").unwrap_or(0))).collect();
    }
    if let Some(n) = list("ChallengeMode") {
        c.challenge_mode = elems(&n, "Reward").map(|x| (attr_i32(&x, "stars").unwrap_or(0), attr_i32(&x, "xp").unwrap_or(0))).collect();
    }
    Ok(c)
}

pub fn parse_achievements(t: &str) -> Result<Vec<AchievementDef>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Achievement")
        .map(|a| AchievementDef {
            game_center_id: attr(&a, "GameCenterID").unwrap_or("").to_string(),
            google_play_id: attr(&a, "GooglePlayID").unwrap_or("").to_string(),
            value_tracker: attr(&a, "ValueTracker").map(|s| s.to_string()),
            max_value: attr_i32(&a, "MaxValue").unwrap_or(1),
            grade: attr_i32(&a, "Grade").unwrap_or(0),
        })
        .collect())
}

pub fn parse_challenges(t: &str) -> Result<Vec<ChallengeDef>, String> {
    let d = doc(t)?;
    let Some(c) = child(&d.root_element(), "Challenges") else { return Ok(vec![]) };
    Ok(c.children()
        .filter(|n| n.is_element())
        .map(|n| ChallengeDef {
            class: n.tag_name().name().to_string(),
            name: attr(&n, "Name").unwrap_or("").to_string(),
            category: attr(&n, "Category").unwrap_or("").to_string(),
            description: attr(&n, "Description").unwrap_or("").to_string(),
        })
        .collect())
}

pub fn parse_episode_config(t: &str) -> Result<Vec<EpisodeConfig>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Episode")
        .map(|e| EpisodeConfig {
            name: attr(&e, "name").unwrap_or("").to_string(),
            kart_type: attr(&e, "kart_type").unwrap_or("").to_string(),
            powerup_kart: attr(&e, "powerup_kart").unwrap_or("").to_string(),
            energy_cost: attr_i32(&e, "energy_cost").unwrap_or(1),
            kart_pack: attr(&e, "kart_pack").unwrap_or("").to_string(),
            tiers: elems(&e, "Tier").map(|tr| tr.children().filter(|c| c.is_element()).filter_map(|c| attr(&c, "m_cEventDefinitionFile").map(|s| s.to_string())).collect()).collect(),
        })
        .collect())
}

pub fn parse_mp_rank(t: &str) -> Result<MpRankCfg, String> {
    // three top level elements (SPRank_Config, TBMRank_Config, SBMRank_Config) - parse them one by one
    let mut m = MpRankCfg::default();
    for (tag, dst) in [("SPRank_Config", 0), ("TBMRank_Config", 1), ("SBMRank_Config", 2)] {
        let (Some(a), Some(b)) = (t.find(&format!("<{tag}>")), t.find(&format!("</{tag}>"))) else { continue };
        let frag = &t[a..b + tag.len() + 3];
        let d = doc(frag)?;
        let list: Vec<(String, i32)> = d
            .root_element()
            .children()
            .filter(|c| c.is_element())
            .map(|c| (attr(&c, "name").unwrap_or_else(|| c.tag_name().name()).to_string(), attr_i32(&c, "value").unwrap_or(0)))
            .collect();
        match dst {
            0 => m.sp_rank = list,
            1 => m.tbm = list,
            _ => m.sbm = list,
        }
    }
    Ok(m)
}

pub fn parse_track_times(t: &str) -> Result<Vec<Vec<f32>>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Episode").map(|e| elems(&e, "Tier").filter_map(|x| attr_f32(&x, "time")).collect()).collect())
}

pub fn parse_daily_races(t: &str) -> Result<DailyRaces, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut dr = DailyRaces::default();
    if let Some(rw) = child(&r, "Rewards") {
        dr.mega_coin_value = attr_i32(&rw, "megaCoinValue").unwrap_or(0);
        for x in elems(&rw, "DailyReward") {
            let mut item = DailyReward {
                start_time: attr_i64(&x, "startTime").unwrap_or(0) as u64,
                duration: attr(&x, "duration").map(parse_duration).unwrap_or(0),
                repeat_time: attr(&x, "repeatTime").map(parse_duration).unwrap_or(0),
                ..Default::default()
            };
            if let Some(rr) = child(&x, "RaceRewards") {
                item.race_rewards = elems(&rr, "Reward").map(|y| Reward::parse(&y)).collect();
            }
            if let Some(mr) = child(&x, "MilestoneRewards") {
                for m in elems(&mr, "Milestone") {
                    item.milestones.push((attr_i32(&m, "score").unwrap_or(0), elems(&m, "Reward").map(|y| Reward::parse(&y)).collect()));
                }
            }
            dr.rewards.push(item);
        }
    }
    if let Some(m) = child(&r, "Multiplier") {
        dr.multiplier_easy_medium_hard = (attr_i32(&m, "easy").unwrap_or(1), attr_i32(&m, "medium").unwrap_or(2), attr_i32(&m, "hard").unwrap_or(3));
    }
    Ok(dr)
}

pub fn parse_kart_type(t: &str) -> Result<KartTypeFile, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut f = KartTypeFile { id: attr(&r, "ID").unwrap_or("").to_string(), karts: vec![] };
    for k in elems(&r, "Kart") {
        f.karts.push(KartTypeKart {
            id: attr(&k, "ID").unwrap_or("").to_string(),
            name: attr(&k, "name").unwrap_or("").to_string(),
            tier: attr_i32(&k, "tier").unwrap_or(0),
            upgrades_to: attr(&k, "upgrades_to").unwrap_or("").to_string(),
            energy: attr_i32(&k, "energy").unwrap_or(0),
            num_upgrade_levels: child(&k, "NumUpgradeLevels").and_then(|c| c.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0),
            cost_type: child(&k, "CostType").and_then(|c| c.text()).unwrap_or("").trim().to_string(),
            cost_amount: child(&k, "CostAmount").and_then(|c| c.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0),
        });
    }
    Ok(f)
}

pub fn parse_ftue(t: &str) -> Result<Vec<FtueState>, String> {
    let d = doc(t)?;
    let split = |s: Option<&str>| -> Vec<String> { s.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default() };
    Ok(elems(&d.root_element(), "State")
        .map(|s| FtueState { name: attr(&s, "name").unwrap_or("").to_string(), previous_states: split(attr(&s, "previousStates")), prerequisites: split(attr(&s, "prerequisites")) })
        .collect())
}

pub fn parse_migration(t: &str) -> Result<MigrationRewards, String> {
    let d = doc(t)?;
    let r = d.root_element();
    let mut m = MigrationRewards::default();
    if let Some(cc) = child(&r, "CurrencyConversion") {
        for c in cc.children().filter(|c| c.is_element()) {
            m.currency.push((
                c.tag_name().name().to_string(),
                attr(&c, "reward").unwrap_or("").to_string(),
                attr_f32(&c, "conversionRate").unwrap_or(1.0),
                attr_i32(&c, "cap").unwrap_or(0),
                attr_i32(&c, "min").unwrap_or(0),
            ));
        }
    }
    if let Some(kc) = child(&r, "KartConversion") {
        m.kart_gems = elems(&kc, "Kart").map(|k| (attr(&k, "old").unwrap_or("").to_string(), attr_i32(&k, "gems").unwrap_or(0))).collect();
    }
    Ok(m)
}

pub fn parse_features(t: &str) -> Result<Vec<(String, bool)>, String> {
    let d = doc(t)?;
    Ok(d.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Setting").map(|s| (attr(&s, "feature").unwrap_or("").to_string(), attr_bool(&s, "enabled").unwrap_or(false))).collect())
}

pub fn parse_tracklist(t: &str) -> Result<Vec<(i32, Vec<i32>)>, String> {
    let d = doc(t)?;
    let r = d.root_element();
    Ok(elems(&r, "Theme").map(|th| (attr_i32(&th, "num").unwrap_or(0), elems(&th, "Run").filter_map(|x| attr_i32(&x, "num")).collect())).collect())
}

pub fn parse_newsfeed(t: &str) -> Result<Vec<(String, i32)>, String> {
    // spacingFormat="<<  %s  <<" has raw `<` inside an attribute value (the original reader accepts it)
    let fixed = t.replace("<<", "&lt;&lt;");
    let d = doc(&fixed)?;
    Ok(elems(&d.root_element(), "String").map(|s| (attr(&s, "string").unwrap_or("").to_string(), attr_i32(&s, "weighting").unwrap_or(1))).collect())
}

pub fn parse_types(t: &str) -> Result<HashMap<(String, String), String>, String> {
    let d = doc(t)?;
    let mut m = HashMap::new();
    for e in elems(&d.root_element(), "Entry") {
        if let Some(img) = elems(&e, "Images").find_map(|i| attr(&i, "Medium").map(|s| s.to_string())) {
            m.insert((attr(&e, "Type").unwrap_or("").to_string(), attr(&e, "SubType").unwrap_or("").to_string()), img);
        }
    }
    Ok(m)
}

pub fn parse_lmp(t: &str) -> Result<Vec<(String, Vec<(String, i32)>)>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Episode")
        .map(|e| (attr(&e, "theme").unwrap_or("").to_string(), elems(&e, "Track").map(|x| (attr(&x, "trackName").unwrap_or("").to_string(), attr_i32(&x, "trackNumber").unwrap_or(0))).collect()))
        .collect())
}

pub fn parse_unlock_info(t: &str) -> Result<Vec<(Vec<String>, i32)>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Unlock")
        .map(|u| {
            let items = child(&u, "UnlockItems").and_then(|c| c.text()).unwrap_or("").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let n = child(&u, "IntNumberOFUnlocks").and_then(|c| c.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0);
            (items, n)
        })
        .collect())
}

pub fn parse_tournaments(t: &str) -> Result<Vec<TournamentDef>, String> {
    let d = doc(t)?;
    Ok(d.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "Tournament" && attr(n, "ID").is_some())
        .map(|n| TournamentDef {
            id: attr(&n, "ID").unwrap_or("").to_string(),
            start: attr_i64(&n, "Start").unwrap_or(0) as u64,
            duration: attr(&n, "Duration").map(parse_duration).unwrap_or(0),
            kind: attr(&n, "Type").unwrap_or("").to_string(),
            sub_type: attr_i32(&n, "SubType").unwrap_or(0),
        })
        .collect())
}

pub fn parse_tournament_types(t: &str) -> Result<Vec<String>, String> {
    let d = doc(t)?;
    Ok(elems(&d.root_element(), "Tournament").map(|n| attr(&n, "type").unwrap_or("").to_string()).collect())
}
