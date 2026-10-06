//! Unit tests against the real `assets292` character xml (skipped when the data is missing) plus synthetic xml.

use super::*;

fn char_xml(n: u32) -> Option<String> {
    let p = format!("{}/xml/characters/charxml/char_{:03}.xml", ASSETS292, n);
    std::fs::read_to_string(p).ok()
}

fn kart(player: bool, ch: u8) -> KartState {
    KartState { id: 3, is_player: player, character: ch, wheels_on_ground: 4, ..Default::default() }
}

fn make(n: u32) -> Option<Box<dyn Ability>> {
    let xml = char_xml(n)?;
    let p = AbilityParams::from_character_xml(&xml)?;
    let mut a = create_ability(&p.name, &p, 0.0)?;
    let mut env = a.base().env.clone();
    env.effects = p.effects.clone();
    a.set_env(env);
    Some(a)
}

fn body_force_z(out: &[CarEffect]) -> Option<f32> {
    out.iter().rev().find_map(|e| if let CarEffect::BodyForce { force_local, .. } = e { Some(force_local.z) } else { None })
}

#[test]
fn enum_mapping_matches_original() {
    assert_eq!(BirdAbility::from_name("SpeedBoost"), Some(BirdAbility::SpeedBoost));
    assert_eq!(BirdAbility::from_name("RedSpeedBoost").unwrap().id(), 7);
    assert_eq!(BirdAbility::from_name("BlueSpeedBoost").unwrap().id(), 8);
    assert_eq!(BirdAbility::from_name("OvertakeSpeedBoost").unwrap().id(), 10);
    assert_eq!(BirdAbility::from_name("KingPigAbility").unwrap().id(), 0xb);
    assert_eq!(BirdAbility::from_name("MatildaBossAbility").unwrap().id(), 0x16);
    assert_eq!(BirdAbility::from_name("nope"), None);
    for i in 1..=22 {
        assert_eq!(BirdAbility::from_id(i).unwrap().id(), i);
    }
    // GetChargesPerPurchase @001baf14: 3 for 8, 0xe, 3
    for i in 1..=22 {
        let want = if i == 8 || i == 0xe || i == 3 { 3 } else { 1 };
        assert_eq!(BirdAbility::from_id(i).unwrap().charges_per_purchase(), want, "id {}", i);
    }
    assert!(BirdAbility::Bomb.should_ghost_use_ai() && !BirdAbility::RedSpeedBoost.should_ghost_use_ai());
}

#[test]
fn red_values_from_xml() {
    let Some(a) = make(1) else { return };
    assert_eq!(a.id(), BirdAbility::RedSpeedBoost);
    assert_eq!(a.name(), "RedSpeedBoost");
    let b = a.base();
    assert_eq!(b.duration, 1.66);
    assert_eq!(b.effect_duration, 1.66);
    assert_eq!(b.charges, 1);
    assert_eq!(a.reuse_delay(), 5.0);
    assert_eq!(b.cam_behind_mod, 5.0); // level fraction 0 -> MinLevel
    assert_eq!((b.ai_min_time, b.ai_max_time), (10.0, 60.0));
    assert_eq!(a.on_set_steering(1.0), 0.75);
    assert_eq!(b.env.effects.start, "RedAbility"); // <AbilityEffectName>
}

#[test]
fn red_lifecycle_and_force() {
    let Some(mut a) = make(1) else { return };
    let k = kart(true, 0);
    let mut out = Vec::new();
    assert!(a.can_trigger(&k));
    a.trigger_player(&k, &mut out);
    assert!(a.base().active && a.is_active());
    assert_eq!(a.base().charges, 0);
    assert_eq!(a.base().uses, 1);
    assert_eq!(a.base().time_remaining, 1.66);
    assert_eq!(a.base().effect_timer, 1.66);
    // grace floor is requested for players
    assert!(out.iter().any(|e| matches!(e, CarEffect::Other { tag, .. } if tag.starts_with("CGame+0x3298"))));
    // no charges left: cannot retrigger
    assert!(!a.can_trigger(&k));
    // integrate: force = rb+0x98 * 5500 along +Z
    out.clear();
    a.on_integrate(&k, &mut out);
    let z = body_force_z(&out).unwrap();
    assert!((z - 5500.0 / 60.0).abs() < 1e-3);
    let dt = 1.0 / 60.0;
    let mut n = 0;
    while a.is_active() && n < 1000 {
        tick_ability(a.as_mut(), dt, &k, &mut out);
        n += 1;
    }
    assert!(!a.base().active);
    // 1.66 s at 60 Hz = 99.6 frames -> 100 (+-1 for float accumulation)
    assert!((99..=101).contains(&n), "frames {}", n);
    assert!(!a.is_active());
    // after finish no more force
    out.clear();
    a.on_integrate(&k, &mut out);
    assert!(body_force_z(&out).is_none());
}

#[test]
fn red_spent_latch_only_without_retrigger_flag() {
    let Some(mut a) = make(1) else { return };
    let k = kart(true, 0);
    let mut out = Vec::new();
    // with every ability debug bool off a player cannot trigger at all (base CanTriggerAbility)
    let mut env = a.base().env.clone();
    env.debug.buy_in_race = false;
    env.debug.buy_pre_race = false;
    a.set_env(env.clone());
    assert!(!a.can_trigger(&k));
    // shipped flag on: trigger; then drop the flags so CanRetrigger() is false when FinishAbility runs
    env.debug.buy_in_race = true;
    a.set_env(env.clone());
    a.trigger_player(&k, &mut out);
    env.debug.buy_in_race = false;
    a.set_env(env.clone());
    a.finish(&k, &mut out);
    env.debug.buy_in_race = true;
    a.set_env(env);
    a.on_player_purchase(); // charges back to 1
    assert_eq!(a.charges(), 1);
    assert!(!a.can_trigger(&k), "spent latch must block the next trigger");
}

#[test]
fn blue_three_charges_and_force() {
    let Some(mut a) = make(5) else { return };
    assert_eq!(a.id(), BirdAbility::BlueSpeedBoost);
    assert_eq!(a.charges(), 3);
    assert_eq!(a.base().duration, 2.0);
    assert_eq!(a.reuse_delay(), DEBUG_FLOAT_ABILITY_HOLDOFF_TIME); // <ReuseDelay> is not read by any loader
    assert_eq!(a.on_set_steering(2.0), 1.5);
    let k = kart(true, 4);
    let mut out = Vec::new();
    a.trigger_player(&k, &mut out);
    assert_eq!(a.charges(), 2);
    assert!(!a.can_trigger(&k), "not while running");
    out.clear();
    a.on_integrate(&k, &mut out);
    assert!((body_force_z(&out).unwrap() - 2000.0 / 60.0).abs() < 1e-3);
    a.finish(&k, &mut out);
    assert!(a.can_trigger(&k));
    a.trigger_player(&k, &mut out);
    a.finish(&k, &mut out);
    a.trigger_player(&k, &mut out);
    a.finish(&k, &mut out);
    assert_eq!(a.charges(), 0);
    assert_eq!(a.uses_this_race(), 3);
    assert!(!a.can_trigger(&k));
    a.on_player_purchase();
    assert_eq!(a.charges(), 3);
    // CamBehindMod: Blue's override decays CamBehindMod (5.0 at level 0) with the remaining time
    a.trigger_player(&k, &mut out);
    assert!((a.on_cam_behind_mod(0.0) - 5.0).abs() < 1e-5);
    a.update(1.0, &k, &mut out);
    assert!((a.on_cam_behind_mod(0.0) - 2.5).abs() < 1e-5);
}

#[test]
fn yellow_time_scale_and_slowmo() {
    for (n, dur) in [(12u32, 1.0f32), (13, 1.5), (14, 1.5)] {
        let Some(mut a) = make(n) else { return };
        assert_eq!(a.id(), BirdAbility::SpeedBoost);
        let k = kart(true, 11);
        let mut out = Vec::new();
        a.trigger_player(&k, &mut out);
        assert!(out.iter().any(|e| *e == CarEffect::SetTimeScale { car: 3, scale: 3.0, integrate_scalar: 3 }));
        let slow = out.iter().find_map(|e| if let CarEffect::SlowMo { scale, seconds } = e { Some((*scale, *seconds)) } else { None }).unwrap();
        assert!((slow.0 - 0.4).abs() < 1e-6, "{:?}", slow);
        assert!((slow.1 - 2.5 * dur).abs() < 1e-5, "{:?}", slow);
        // pilot animation runs at SlowMoScalar
        assert!(out.iter().any(|e| matches!(e, CarEffect::Other { tag, vals, .. } if *tag == "CPilotAnimationHandler::SetAnimRate" && vals[0] == 2.5)));
        out.clear();
        let mut frames = 0;
        while a.is_active() && frames < 2000 {
            tick_ability(a.as_mut(), 1.0 / 60.0, &k, &mut out);
            frames += 1;
        }
        assert!(!a.base().active);
        assert!(out.iter().any(|e| *e == CarEffect::SetTimeScale { car: 3, scale: 1.0, integrate_scalar: 1 }));
        assert!(((frames as f32) / 60.0 - dur).abs() < 0.05, "frames {}", frames);
    }
}

#[test]
fn yellow_ai_gets_no_slowmo_but_time_scale() {
    let Some(mut a) = make(12) else { return };
    let k = kart(false, 11);
    let mut out = Vec::new();
    a.trigger(&k, &mut out);
    assert!(!out.iter().any(|e| matches!(e, CarEffect::SlowMo { .. })));
    assert!(out.iter().any(|e| matches!(e, CarEffect::SetTimeScale { .. })));
}

#[test]
fn level_interpolation() {
    let xml = r#"<Character><Ability name="RedSpeedBoost"><Duration><MinLevel>1.0</MinLevel><MaxLevel>2.0</MaxLevel></Duration>
        <SpeedForce><MinLevel>1000</MinLevel><MaxLevel>3000</MaxLevel></SpeedForce>
        <CamBehindMod><MinLevel>5.00</MinLevel><MaxLevel>6.00</MaxLevel></CamBehindMod></Ability></Character>"#;
    let p = AbilityParams::from_character_xml(xml).unwrap();
    assert_eq!(p.float_for_level("SpeedForce", 0.0, 0.5), 2000.0);
    assert_eq!(p.float_for_level("SpeedForce", 0.0, -1.0), 1000.0);
    assert_eq!(p.float_for_level("SpeedForce", 0.0, 7.0), 3000.0);
    assert_eq!(p.float_for_level("Missing", 4.0, 0.3), 4.0);
    let mut a = create_ability("RedSpeedBoost", &p, 0.5).unwrap();
    assert_eq!(a.base().duration, 1.5);
    assert_eq!(a.base().effect_duration, 1.5); // EffectDuration defaults to the loaded Duration
    assert_eq!(a.base().cam_behind_mod, 5.5);
    let k = kart(true, 0);
    let mut out = Vec::new();
    a.trigger_player(&k, &mut out);
    a.on_integrate(&k, &mut out);
    assert!((body_force_z(&out).unwrap() - 2000.0 / 60.0).abs() < 1e-3);
    // int loader rounds up (ceilf)
    let p2 = AbilityParams::from_character_xml(
        r#"<Character><Ability name="SpeedBoost"><IntegrateScalar><MinLevel>2</MinLevel><MaxLevel>3</MaxLevel></IntegrateScalar></Ability></Character>"#,
    )
    .unwrap();
    assert_eq!(p2.int_for_level("IntegrateScalar", 0.0), 2);
    assert_eq!(p2.int_for_level("IntegrateScalar", 0.2), 3);
}

#[test]
fn reuse_delay_table_and_charge_count_default() {
    let xml = r#"<Character><Ability name="BlueSpeedBoost"><Duration><MinLevel>2</MinLevel><MaxLevel>2</MaxLevel></Duration>
        <ReuseDelays><Delay number="1" value="1.5"/><Delay number="2" value="9"/><Default value="4"/></ReuseDelays>
        <AIActivationTime/></Ability></Character>"#;
    let p = AbilityParams::from_character_xml(xml).unwrap();
    let mut a = create_ability("BlueSpeedBoost", &p, 0.0).unwrap();
    assert_eq!(a.charges(), 3); // ChargeCount default for id 8
    assert_eq!(a.reuse_delay(), 1.5); // number == uses+1 == 1
    assert_eq!(a.base().ai_min_time, AI_ACTIVATION_TIME_DEFAULT);
    let k = kart(true, 4);
    let mut out = Vec::new();
    a.trigger_player(&k, &mut out); // uses 0 -> 1: next delay is number 2
    assert_eq!(a.reuse_delay(), 9.0);
    a.finish(&k, &mut out);
    a.trigger_player(&k, &mut out); // next number 3 -> default
    assert_eq!(a.reuse_delay(), 4.0);
}

#[test]
fn overtake_synthetic() {
    let xml = r#"<Character><Ability name="Overtake" type="OvertakeSpeedBoost"><StartDistance>12</StartDistance><EndDistance>2</EndDistance>
        <SpeedForce>3000</SpeedForce><DamageMultiplier>0.5</DamageMultiplier><BoostAI>1.3</BoostAI></Ability></Character>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let node = doc.root_element().first_element_child().unwrap();
    let p = AbilityParams::from_node(node);
    let mut a = create_boss_ability(&p.ty, &p, 0.0).unwrap();
    assert_eq!(a.name(), "Overtake");
    assert!(a.base().is_boss);
    assert!(!a.can_trigger_at_distance(0.0, -5.0));
    assert!(a.can_trigger_at_distance(0.0, -12.0));
    assert_eq!(a.on_impact_damage(10.0), 5.0);
    let k = kart(false, 20);
    let mut env = a.base().env.clone();
    env.ai_boost_factors = (0.9, 0.8);
    a.set_env(env);
    let mut out = Vec::new();
    assert!(a.can_trigger(&k)); // boss ability
    a.trigger(&k, &mut out);
    assert!(a.base().active);
    // distance below EndDistance: stays active even though the base timer would have expired
    a.can_trigger_at_distance(0.0, -12.0);
    for _ in 0..600 {
        tick_ability(a.as_mut(), 0.1, &k, &mut out);
    }
    assert!(a.base().active);
    a.can_trigger_at_distance(0.0, 2.5);
    tick_ability(a.as_mut(), 0.1, &k, &mut out);
    assert!(!a.base().active);
    assert!(out.iter().any(|e| matches!(e, CarEffect::Other { tag, vals, .. } if tag.contains("restore") && vals[0] == 0.9 && vals[1] == 0.8)));
    // the bird factory returns null for it
    assert!(create_ability("OvertakeSpeedBoost", &p, 0.0).is_none());
}

fn ctx(eco: &AbilityEconomy) -> RaceAbilityCtx<'_> {
    RaceAbilityCtx {
        game_state: 3,
        game_state_aux: 0,
        event_allows_abilities: true,
        multiple_abilities_enabled: false,
        num_players: 1,
        mp_game_active: false,
        dbg77: false,
        economy: eco,
    }
}

#[test]
fn car_slot_gating_and_ready_time() {
    let Some(a) = make(1) else { return };
    let eco = AbilityEconomy::default();
    let ctx = ctx(&eco);
    let mut slot = CarAbilitySlot::new(Some(a));
    assert_eq!(slot.ready_time, 5.0); // GetReuseDelay of Red after Init
    let k = kart(true, 0);
    let mut out = Vec::new();
    assert!(!slot.can_trigger(&k, &ctx));
    assert!(!slot.trigger(&k, &mut out));
    slot.tick(5.1, &k, &mut out);
    // HUD polls the fraction every frame: before the trigger it is (reuse - (ready - clock)) / reuse
    assert!((slot.charged_fraction(false) - (5.0 - (5.0 - 5.1)) / 5.0).abs() < 1e-4);
    assert!(slot.can_trigger(&k, &ctx));
    assert!(slot.trigger(&k, &mut out));
    assert!(slot.is_active());
    // ready = clock + GetDuration + GetReuseDelay (after the use: the Default 5.0)
    assert!((slot.ready_time - (5.1 + 1.66 + 5.0)).abs() < 1e-4);
    assert_eq!(slot.since_trigger, 0.0);
    assert!(!slot.can_trigger(&k, &ctx)); // running / no charge left
    // charged fraction while active is remaining/duration (the cache from the previous poll is >= 0.05)
    assert!((slot.charged_fraction(false) - 1.0).abs() < 1e-4);
    // the event mode must allow abilities
    let ctx2 = RaceAbilityCtx { event_allows_abilities: false, ..ctx.clone() };
    slot.tick(20.0, &k, &mut out);
    assert!(!slot.can_trigger(&k, &ctx2));
    // game state 6 blocks unless aux == 8
    let ctx3 = RaceAbilityCtx { game_state: 6, ..ctx.clone() };
    slot.on_player_purchase();
    let mut s2 = CarAbilitySlot::new(make(1));
    s2.tick(6.0, &k, &mut out);
    assert!(s2.can_trigger(&k, &ctx));
    assert!(!s2.can_trigger(&k, &ctx3));
    assert!(s2.can_trigger(&k, &RaceAbilityCtx { game_state_aux: 8, ..ctx3 }));
}

#[test]
fn ai_trigger_has_no_use_bookkeeping() {
    let Some(mut a) = make(1) else { return };
    let mut env = a.base().env.clone();
    env.driver_ability_flag = true;
    a.set_env(env);
    let mut slot = CarAbilitySlot::new(Some(a));
    let k = kart(false, 0);
    let mut out = Vec::new();
    slot.tick(6.0, &k, &mut out);
    let charges = slot.charges();
    assert!(slot.trigger(&k, &mut out));
    // TriggerAbility (slot 0x24) decrements charges but never bumps `uses`
    assert_eq!(slot.charges(), charges - 1);
    assert_eq!(slot.uses_this_race(), 0);
}

#[test]
fn economy_from_real_xml() {
    let path = format!("{}/xml/gameplay/misc/economy.xml", ASSETS292);
    let Ok(xml) = std::fs::read_to_string(path) else { return };
    let e = AbilityEconomy::from_economy_xml(&xml);
    assert_eq!(e.cost_before_race, 3);
    assert_eq!(e.max_ability, 3);
    assert_eq!(e.max_uses, 5);
    assert_eq!(e.costs, vec![2, 3, 5]);
    assert_eq!(bird_ability_cost_pre_race(&e), 3);
    let red = BirdAbility::RedSpeedBoost;
    assert_eq!(bird_ability_cost_in_race(Some((0, red)), &e), Some(0));
    assert_eq!(bird_ability_cost_in_race(Some((1, red)), &e), Some(2));
    assert_eq!(bird_ability_cost_in_race(Some((2, red)), &e), Some(3));
    assert_eq!(bird_ability_cost_in_race(Some((3, red)), &e), Some(5));
    assert_eq!(bird_ability_cost_in_race(Some((5, red)), &e), Some(5));
    assert_eq!(bird_ability_cost_in_race(Some((6, red)), &e), None); // > MaxUses
    assert_eq!(bird_ability_cost_in_race(None, &e), None);
    // Blue buys 3 charges per purchase
    let blue = BirdAbility::BlueSpeedBoost;
    assert_eq!(bird_ability_cost_in_race(Some((2, blue)), &e), Some(0));
    assert_eq!(bird_ability_cost_in_race(Some((3, blue)), &e), Some(2));
    assert!(can_use_ability(0, red, &e));
    assert!(can_use_ability(5, red, &e));
    assert!(!can_use_ability(6, red, &e));
    assert!(can_use_ability(6, red, &AbilityEconomy { max_uses: -1, ..e.clone() }));
    assert!(!can_use_ability(1, red, &AbilityEconomy::default()), "no cost table -> false");
}

#[test]
fn boss_xml_object_spawn_params() {
    let path = format!("{}/xml/characters/charxml/boss_001.xml", ASSETS292);
    let Ok(xml) = std::fs::read_to_string(path) else { return };
    assert!(AbilityParams::boss_ability_count(&xml) >= 1);
    let p = AbilityParams::from_boss_xml(&xml, 0).unwrap();
    assert_eq!(p.ty, "ObjectSpawn");
    assert_eq!(BirdAbility::from_name(&p.ty), Some(BirdAbility::ObjectSpawn));
    assert_eq!(p.scalar_str("ObjectType"), Some("bombs_bomb"));
    assert_eq!(p.scalar_f32("MaxDistance", 0.0), 55.0);
    // ObjectSpawn is a boss-only weapon: the boss factory builds it
    assert!(create_boss_ability_by_enum(BirdAbility::ObjectSpawn, &p, 0.0).is_some());
    assert!(create_ability_by_enum(BirdAbility::ObjectSpawn, &p, 0.0).is_none());
}

#[test]
fn every_character_xml_parses() {
    for n in 1..=16u32 {
        let Some(xml) = char_xml(n) else { return };
        if let Some(p) = AbilityParams::from_character_xml(&xml) {
            assert!(BirdAbility::from_name(&p.name).is_some(), "char_{:03} ability {}", n, p.name);
        }
    }
}
