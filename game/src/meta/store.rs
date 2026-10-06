//! Store data (`assets292/xml/store`): currency shop, bundles, parts shop (soft currency shop), item list, misc / telepod products, offers.
//!
//! Offline stand-in: real-money items (`product_ID` entries, gem packs, telepod karts, IAP offers) are listed but marked
//! `real_money = true` and are never purchasable here. Items priced in gems / coins (coin packs bought with gems `SC01..SC05`,
//! power-ups, parts shop slots) can be bought with the player's own currency.
//!
//! Original code: `CSoftCurrencyShopManager::ParseXML @0017a3a8`, `TCost::GetCost @0017a87c`, `GetBestTierIndex @0017a92c`,
//! `RepopulateShop @0017a9a8`, `BuyItem @0017b3dc`.

use super::data::*;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ShopItem {
    pub tag: String,
    pub category: String,
    pub bundle_index: i32,
    pub product_id: Option<String>,
    /// `type="Gems"` + `price` for items bought with gems (coin packs)
    pub price: Option<(String, i32)>,
    pub free_display_amount: i32,
    pub hidden: bool,
    pub tracked_investment: Option<f32>,
    pub thumbnail: String,
    pub badge: Option<String>,
    /// true when the item can only be bought with real money (not purchasable offline)
    pub real_money: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Bundle {
    pub index: i32,
    pub items: Vec<Reward>,
    /// `ExtraFreeModifier` / `RoundingTolerance` of the first item (gem packs)
    pub extra_free_modifier: Option<i32>,
    pub rounding_tolerance: Option<i32>,
}

/// `CSoftCurrencyShopManager::TCost` (`<Slot rarity cost maxCost progressionMultiplier progressionAdd>`).
#[derive(Clone, Copy, Debug, Default)]
pub struct PartsCost {
    pub cost: i32,
    pub max_cost: i32,
    pub progression_multiplier: f32,
    pub progression_add: f32,
}
impl PartsCost {
    /// `TCost::GetCost(int) @0017a87c`. `bought` = instances already bought.
    pub fn get_cost(&self, bought: i32) -> i32 {
        let start = self.cost;
        let v: i32 = if self.progression_multiplier == 0.0 {
            (bought as f32 * self.progression_add) as i32 + start
        } else {
            let mut f = start as f32;
            for _ in 0..bought.max(0) {
                f = f + self.progression_add + f * self.progression_multiplier;
            }
            f as i32
        };
        // clamp: if start <= v then (v < max ? v : max) else start
        if start <= v {
            if v < self.max_cost {
                v
            } else {
                self.max_cost
            }
        } else {
            start
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PartsShopTier {
    pub id: i32,
    pub required_rank: i32,
    /// (rarity, token ids)
    pub slots: Vec<(String, Vec<String>)>,
}

#[derive(Clone, Debug, Default)]
pub struct Offer {
    pub kind: String,
    pub special_offer_type: String,
    pub kart: Option<String>,
    pub start_time: i64,
    pub duration: i64,
    pub item_name_tag: String,
    pub percent_free: i32,
}

#[derive(Clone, Debug, Default)]
pub struct StoreData {
    pub items: Vec<ShopItem>,
    pub bundles: Vec<Bundle>,
    pub parts_costs: Vec<(String, PartsCost)>, // rarity -> cost
    pub parts_tiers: Vec<PartsShopTier>,
    pub powerup_items: Vec<(String, i32, i32, String)>, // tag, cost (gems), currency id, name
    pub misc_products: Vec<(String, String)>,
    pub telepod_karts: Vec<(String, String)>,
    pub offers: Vec<Offer>,
}

impl StoreData {
    pub fn load(dir: &Path) -> Result<StoreData, String> {
        let mut s = StoreData::default();
        let t = read_text(&dir.join("currencyshop.xml"))?;
        let d = roxmltree::Document::parse(&t).map_err(|e| e.to_string())?;
        // DistributionSharedMain is the generic distribution
        if let Some(main) = d.descendants().find(|n| n.is_element() && n.tag_name().name() == "DistributionSharedMain") {
            for cat in elems(&main, "Category") {
                let ctype = attr(&cat, "type").unwrap_or("").to_string();
                for it in elems(&cat, "Item") {
                    let product_id = attr(&it, "product_ID").map(|s| s.to_string());
                    let price = match (attr(&it, "type"), attr_i32(&it, "price")) {
                        (Some(t), Some(p)) => Some((t.to_string(), p)),
                        _ => None,
                    };
                    s.items.push(ShopItem {
                        tag: attr(&it, "tag").unwrap_or("").to_string(),
                        category: ctype.clone(),
                        bundle_index: attr_i32(&it, "bundleIndex").unwrap_or(-1),
                        real_money: product_id.is_some() && price.is_none(),
                        product_id,
                        price,
                        free_display_amount: attr_i32(&it, "freeDisplayAmount").unwrap_or(0),
                        hidden: attr_bool(&it, "hiddenItem").unwrap_or(false),
                        tracked_investment: attr_f32(&it, "trackedInvestment"),
                        thumbnail: attr(&it, "thumbnail").unwrap_or("").to_string(),
                        badge: attr(&it, "badge").map(|s| s.to_string()),
                    });
                }
            }
        }
        if let Ok(t) = read_text(&dir.join("bundledefinitions.xml")) {
            let d = roxmltree::Document::parse(&t).map_err(|e| e.to_string())?;
            for b in elems(&d.root_element(), "Bundle") {
                let nodes: Vec<_> = elems(&b, "Item").collect();
                s.bundles.push(Bundle {
                    index: attr_i32(&b, "index").unwrap_or(0),
                    items: nodes.iter().map(Reward::parse).collect(),
                    extra_free_modifier: nodes.first().and_then(|n| attr_i32(n, "ExtraFreeModifier")),
                    rounding_tolerance: nodes.first().and_then(|n| attr_i32(n, "RoundingTolerance")),
                });
            }
        }
        if let Ok(t) = read_text(&dir.join("partsshop.xml")) {
            let d = roxmltree::Document::parse(&t).map_err(|e| e.to_string())?;
            let shop = d.root_element();
            if let Some(costs) = child(&shop, "Costs") {
                for sl in elems(&costs, "Slot") {
                    s.parts_costs.push((
                        attr(&sl, "rarity").unwrap_or("").to_string(),
                        PartsCost {
                            cost: attr_i32(&sl, "cost").unwrap_or(0),
                            max_cost: attr_i32(&sl, "maxCost").unwrap_or(0),
                            progression_multiplier: attr_f32(&sl, "progressionMultiplier").unwrap_or(0.0),
                            progression_add: attr_f32(&sl, "progressionAdd").unwrap_or(0.0),
                        },
                    ));
                }
            }
            for tr in elems(&shop, "Tier") {
                s.parts_tiers.push(PartsShopTier {
                    id: attr_i32(&tr, "id").unwrap_or(0),
                    required_rank: attr_i32(&tr, "requiredRank").unwrap_or(0),
                    slots: elems(&tr, "Slot")
                        .map(|sl| {
                            (
                                attr(&sl, "rarity").unwrap_or("").to_string(),
                                sl.text().unwrap_or("").split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                            )
                        })
                        .collect(),
                });
            }
        }
        if let Ok(t) = read_text(&dir.join("itemlist.xml")) {
            let wrapped = format!("<W>{t}</W>");
            let d = roxmltree::Document::parse(&wrapped).map_err(|e| e.to_string())?;
            for it in d.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Item") {
                s.powerup_items.push((
                    attr(&it, "tag").unwrap_or("").to_string(),
                    attr_i32(&it, "cost").unwrap_or(0),
                    attr_i32(&it, "currency").unwrap_or(0),
                    attr(&it, "name").unwrap_or("").to_string(),
                ));
            }
        }
        if let Ok(t) = read_text(&dir.join("miscproducts.xml")) {
            let wrapped = format!("<W>{t}</W>");
            let d = roxmltree::Document::parse(&wrapped).map_err(|e| e.to_string())?;
            s.misc_products = d.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Product").map(|n| (attr(&n, "tag").unwrap_or("").to_string(), attr(&n, "product_ID").unwrap_or("").to_string())).collect();
        }
        if let Ok(t) = read_text(&dir.join("telepodkarts.xml")) {
            let wrapped = format!("<W>{t}</W>");
            let d = roxmltree::Document::parse(&wrapped).map_err(|e| e.to_string())?;
            s.telepod_karts = d.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Kart").map(|n| (attr(&n, "tag").unwrap_or("").to_string(), attr(&n, "product_ID").unwrap_or("").to_string())).collect();
        }
        if let Ok(t) = read_text(&dir.join("offers.xml")) {
            let d = roxmltree::Document::parse(&t).map_err(|e| e.to_string())?;
            s.offers = elems(&d.root_element(), "Offer")
                .map(|o| Offer {
                    kind: attr(&o, "type").unwrap_or("").to_string(),
                    special_offer_type: attr(&o, "specialOfferType").unwrap_or("").to_string(),
                    kart: attr(&o, "kart").map(|s| s.to_string()),
                    start_time: attr_i64(&o, "startTime").unwrap_or(0),
                    duration: attr_i64(&o, "duration").unwrap_or(0),
                    item_name_tag: attr(&o, "itemNameTag").unwrap_or("").to_string(),
                    percent_free: attr_i32(&o, "percentFree").unwrap_or(0),
                })
                .collect();
        }
        Ok(s)
    }

    pub fn item(&self, tag: &str) -> Option<&ShopItem> {
        self.items.iter().find(|i| i.tag == tag)
    }
    pub fn bundle(&self, index: i32) -> Option<&Bundle> {
        self.bundles.iter().find(|b| b.index == index)
    }
    pub fn parts_cost(&self, rarity: &str) -> Option<PartsCost> {
        self.parts_costs.iter().find(|(r, _)| r.eq_ignore_ascii_case(rarity)).map(|(_, c)| *c)
    }
    /// `CSoftCurrencyShopManager::GetBestTierIndex @0017a92c`: the tier with the highest `requiredRank` that is <= the player's rank
    /// (rank = `CPlayerInfo::GetRank`, 0 based).
    pub fn best_parts_tier(&self, rank: i32) -> usize {
        let mut best = 0usize;
        for (i, t) in self.parts_tiers.iter().enumerate() {
            if t.required_rank <= rank && self.parts_tiers[best].required_rank < t.required_rank {
                best = i;
            }
        }
        best
    }
}
