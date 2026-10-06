//! INTRO game modes (`<GameMode name="INTRO"/>`, `EGameMode` ids 0 / 1 / 2) and the first-time-user (FTUE) pop-ups that
//! the in-game HUD fires per game mode, ported from the Angry Birds Go 2.9.1 decompile.
//!
//! Original classes: `CGameModeIntro` (id 0, vtable `0xd91990`), `CGameModeIntro2` (id 1, vtable `0xd919e0`),
//! `CGameModeIntro3` (id 2); per-car data `CGameModeIntroData`/`Intro2Data`/`Intro3Data` (all three are byte-identical to
//! `CGameModeData`). `CGameMode::CreateGameMode @ 0017d1b4` instantiates them for ids 0, 1, 2.
//!
//! # What the shipped data really contains
//! Only `eventdef_intro.xml` uses `<GameMode name="INTRO"/>`; there is no eventdef with `INTRO2`/`INTRO3`
//! (`eventdef_intro_race2_time.xml` is a `TIME_ATTACK`, `eventdef_intro_race3_race.xml` a `RACE`). The real 2.9.x tutorial
//! race is the **RACE** eventdef `EventDef_Intro_Race3_Race.xml` (level 0.2, stars 90000/110000/130000): `FrontendLoadingFunc
//! @ 0027c098` loads it when `CFTUEManager::GetActiveState(0)` is state 0 (`TutorialRace`, first state of
//! `gameftueprerequisites.xml`) and then goes to the landing screen; with state 1 (`Campaign1`) and no campaign level done it
//! launches the first campaign event directly (analytics stage `030_enter_campaign_1`). `EventDefinitionData.xml
//! tutorialLevelCount="3"` marks the first three campaign events as tutorial levels (`IsEventFTUELocked @ 0014f860`, FTUE stage
//! `CPlayerInfo+0x42c` changed by `RequestFTUEStageChange @ 0014f670`). `eventdef_intro.xml` (and any INTRO2/INTRO3 eventdef) is
//! referenced nowhere in the binary (only `EventDef_Intro_Race3_Race` and `EventDef_Dummy` are), so the INTRO modes are
//! (inference) dead legacy code that is still compiled in (`CreateGameMode` ids 0-2). `eventdef_intro.xml` has `<Stars 1="1000" 2="20000" 3="25000"/>`: the reader only looks for
//! `Star1/Star2/Star3`, so all three thresholds stay 0 (faithfully reproduced here).
//!
//! # Rules (identical across the three, differences marked)
//! * `InitialiseGrid` (slot 4, `0017f998` / `0017fdf4` / Intro3 `00180284`): `CGame::RequestStateChange_JumpIntoGameplay`
//!   (`CGame+0xcc = 8`) then the base grid: the race starts at once, no grid / countdown phase.
//! * `InitialiseCarData`: every car gets a plain `CGameModeData`; `CGame+0x3244 = 5.0`.
//! * `Update` (slot 5): **Intro (0)** `0017f77c`: `CCar::FullRepair` + `CheckVisualDamage` on the local car every frame
//!   (the kart cannot be damaged) and, if the kart is in the slingshot (`CCar+0x464 != 0 && +0x468 != -1`), the slingshot
//!   is cancelled: two globals cleared, `CCar::SetInSlingshot(-1)`, chassis axes reset to (0,0,1), body woken, camera type
//!   0 (the intro race has no slingshot launch). **Intro2 (1)** `0017fc74`: `FullRepair` + `CheckVisualDamage` only.
//!   **Intro3 (2)** `001800d0`: plain `ProcessUpdate` (a normal race).
//! * `CheckGameOverCondition` (slot 7): all human players finished or inactive.
//! * `GetEventCompleted` (slot 12, `0017f658`, `0017fb50`, `0017ffac`): `data+0x10 == 1`, i.e. the kart finished FIRST.
//!   There is no "forced" result: win = finishing position 1, otherwise the result state is 3.
//! * `Debug`: `ProcessUpdate` also force-finishes the event with state 2 once `CGame+0xd4 > 2.0` when the debug global
//!   `*DAT_0017d1a4` is set; not part of the shipping game and not ported.
//!
//! # In-game FTUE pop-ups (`CXGSFE_InGameScreen::ShowInGameFTUE @ 003030d0`, `uipopupingameftue.xml`)
//! On the first race of a type the in-game screen shows a "how to play this mode" popup. See [`ingame_ftue_for_mode`].
#![allow(dead_code)]

use super::types::*;

/// `EGameMode` ids (`StringToGameMode`).
pub const MODE_INTRO: u32 = 0;
pub const MODE_INTRO2: u32 = 1;
pub const MODE_INTRO3: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntroVariant {
    Intro,
    Intro2,
    Intro3,
}

impl IntroVariant {
    pub fn mode_id(self) -> u32 {
        match self {
            IntroVariant::Intro => MODE_INTRO,
            IntroVariant::Intro2 => MODE_INTRO2,
            IntroVariant::Intro3 => MODE_INTRO3,
        }
    }
    pub fn from_mode_name(s: &str) -> Option<IntroVariant> {
        match s {
            "INTRO" => Some(IntroVariant::Intro),
            "INTRO2" => Some(IntroVariant::Intro2),
            "INTRO3" => Some(IntroVariant::Intro3),
            _ => None,
        }
    }
    /// Loc keys of `CGameMode::GetGameModeName / GetGameModeDesc @ 0017d4ac / 0017d674`.
    pub fn loc_keys(self) -> (&'static str, &'static str) {
        match self {
            IntroVariant::Intro => ("MODE_INTRO", "MODE_INTRO_DESC"),
            IntroVariant::Intro2 => ("MODE_INTRO2", "MODE_INTRO2_DESC"),
            IntroVariant::Intro3 => ("MODE_INTRO3", "MODE_INTRO3_DESC"),
        }
    }
}

/// Result state values (`CGameModeData+4`).
pub mod data_state {
    pub const RUNNING: u8 = 1;
    pub const COMPLETED: u8 = 2;
    pub const FAILED: u8 = 3;
}

/// `CGame+0x3244` set by `InitialiseCarData` of the intro / slalom / race modes (`0x40a00000`).
pub const CGAME_3244_VALUE: f32 = 5.0;

// ------------------------------------------------------------------------------------------------------------
// Eventdef (INTRO)
// ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct IntroEventDef {
    pub game_mode_name: String,
    pub environment: String,
    pub character: String,
    pub difficulty_level: f32,
    pub stars: [i32; 3],
}

fn tag_attr<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(p) = xml[from..].find(&open) {
        let s = from + p;
        let after = s + open.len();
        match xml.as_bytes().get(after) {
            Some(c) if c.is_ascii_whitespace() || *c == b'/' || *c == b'>' => {
                let e = xml[s..].find('>').map(|e| s + e).unwrap_or(xml.len());
                let body = &xml[s..e];
                let key = format!(" {attr}=\"");
                let k = body.find(&key)?;
                let vs = k + key.len();
                let ve = body[vs..].find('"')?;
                return Some(&body[vs..vs + ve]);
            }
            _ => from = after,
        }
    }
    None
}

impl IntroEventDef {
    pub fn from_xml(xml: &str) -> Result<IntroEventDef, String> {
        let gm = tag_attr(xml, "GameMode", "name").ok_or("no <GameMode name=...>")?;
        if IntroVariant::from_mode_name(gm).is_none() {
            return Err(format!("GameMode is {gm}, not INTRO/INTRO2/INTRO3"));
        }
        let mut stars = [0i32; 3];
        for (i, n) in ["Star1", "Star2", "Star3"].iter().enumerate() {
            if let Some(v) = tag_attr(xml, "Stars", n) {
                stars[i] = v.trim().parse().unwrap_or(0);
            }
        }
        if !(stars[0] == 0 && stars[1] == 0 && stars[2] == 0) {
            if stars[1] < stars[0] {
                stars[1] = stars[0];
            }
            if stars[2] < stars[1] {
                stars[2] = stars[1];
            }
        }
        Ok(IntroEventDef {
            game_mode_name: gm.to_string(),
            environment: tag_attr(xml, "Environment", "pathname").unwrap_or("").to_string(),
            character: tag_attr(xml, "Character", "name").unwrap_or("").to_string(),
            difficulty_level: tag_attr(xml, "Difficulty", "level").and_then(|v| v.parse().ok()).unwrap_or(0.5),
            stars,
        })
    }
}

/// `GetStarsFromScore @ 000fee38`.
pub fn stars_from_score(running: bool, score: i64, stars: [i32; 3]) -> u8 {
    if running {
        0
    } else if score > stars[2] as i64 {
        3
    } else if score > stars[1] as i64 {
        2
    } else {
        1
    }
}

// ------------------------------------------------------------------------------------------------------------
// Mode
// ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct IntroMode {
    pub variant: IntroVariant,
    pub stars: [i32; 3],
    pub data_state: u8,
    /// `data+0x10` (current position) / `data+0x14` (best position, starts at 1000)
    pub data_position: i32,
    pub data_best_position: i32,
    /// `data+8` race time
    pub data_race_time: f32,
    /// `this+0x1c` mode clock
    pub clock: f32,
    /// set by the host: `CCar+0x464 != 0 && CCar+0x468 != -1` for the local kart
    pub player_in_slingshot: bool,
    pub score_total: i64,
}

impl IntroMode {
    pub fn new(variant: IntroVariant, stars: [i32; 3]) -> IntroMode {
        IntroMode {
            variant,
            stars,
            data_state: data_state::RUNNING,
            data_position: 0,
            data_best_position: 1000,
            data_race_time: 0.0,
            clock: 0.0,
            player_in_slingshot: false,
            score_total: 0,
        }
    }

    pub fn from_eventdef(xml: &str) -> Result<IntroMode, String> {
        let d = IntroEventDef::from_xml(xml)?;
        Ok(IntroMode::new(IntroVariant::from_mode_name(&d.game_mode_name).unwrap(), d.stars))
    }

    /// `InitialiseGrid`: the race jumps straight into gameplay (`CGame::RequestStateChange_JumpIntoGameplay @ 00114c18`:
    /// `CGame+0xcc = 8`; `UpdateStateDependentThings` state 8 is "racing").
    pub fn initialise_grid(&self, events: &mut Vec<ModeEvent>) {
        events.push(ModeEvent::Message { tag: "jump_into_gameplay".to_string() });
    }

    /// `GetEventCompleted`: finished first.
    pub fn event_completed(&self) -> bool {
        self.data_position == 1
    }

    pub fn finish(&mut self, events: &mut Vec<ModeEvent>) {
        if self.data_state != data_state::RUNNING {
            return;
        }
        self.data_state = if self.event_completed() { data_state::COMPLETED } else { data_state::FAILED };
        events.push(ModeEvent::Finished {
            won: self.data_state == data_state::COMPLETED,
            score: self.score_total,
            stars: self.stars(),
        });
    }
}

impl GameModeRules for IntroMode {
    fn kind(&self) -> GameModeKind {
        GameModeKind::Intro
    }

    fn update(&mut self, dt: f32, karts: &[KartState], effects: &mut Vec<CarEffect>, _events: &mut Vec<ModeEvent>) {
        self.clock += dt;
        let Some(k) = karts.iter().find(|k| k.is_player) else { return };
        match self.variant {
            IntroVariant::Intro | IntroVariant::Intro2 => {
                // CCar::FullRepair + CheckVisualDamage
                effects.push(CarEffect::Repair { car: k.id, fraction: 1.0 });
            }
            IntroVariant::Intro3 => {}
        }
        if self.variant == IntroVariant::Intro && self.player_in_slingshot {
            // clears two globals, SetInSlingshot(-1), chassis axes (rb+0x10..0x18 and +0xd4..0xdc) = (0,0,1), SetSleep(0),
            // camera type (0,1,0,1)
            effects.push(CarEffect::Other { tag: "IntroCancelSlingshot", car: Some(k.id), vals: [0.0, 0.0, 1.0, 0.0], pos: Vec3::ZERO });
        }
        // CGameModeData::Update
        self.data_position = k.race_position as i32;
        if self.data_position <= self.data_best_position {
            self.data_best_position = self.data_position;
        }
        if self.data_state == data_state::RUNNING && k.race_time > 0.0 {
            self.data_race_time += dt;
        }
    }

    fn on_input(&mut self, input: &ModeInput, events: &mut Vec<ModeEvent>) {
        match input {
            ModeInput::KartFinished { .. } => self.finish(events),
            ModeInput::Other { tag: "initialise_grid", .. } => self.initialise_grid(events),
            ModeInput::Other { tag: "player_in_slingshot", value, .. } => self.player_in_slingshot = *value != 0.0,
            ModeInput::Other { tag: "score_total", value, .. } => self.score_total = *value as i64,
            _ => {}
        }
    }

    fn state(&self) -> ModeState {
        match self.data_state {
            data_state::COMPLETED => ModeState::Won,
            data_state::FAILED => ModeState::Lost,
            _ => ModeState::Running,
        }
    }

    fn score(&self) -> i64 {
        self.score_total
    }

    fn stars(&self) -> u8 {
        if self.state() == ModeState::Won {
            stars_from_score(false, self.score_total, self.stars)
        } else {
            0
        }
    }
}

// ------------------------------------------------------------------------------------------------------------
// In-game FTUE pop-ups
// ------------------------------------------------------------------------------------------------------------

/// One entry of the `switch(CGame::GetGameMode())` in `CXGSFE_InGameScreen::Process @ 0030b57c` that calls
/// `ShowInGameFTUE(ENotificationType, EInGameFTUEStyle, titleKey, bodyKey)` (call sites `0030d84c..0030d90c`), plus the
/// strings of `CNotificationSlalomTutorial::LayoutScreen @ 00377b18`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InGameFtue {
    /// `ENotificationType` passed to `ShowInGameFTUE` (also the index into `CPlayerInfo+0x440[type]`)
    pub notification_type: u32,
    /// `EInGameFTUEStyle`: state of `CStateWindow_Image` in `uipopupingameftue.xml`
    pub style: u32,
    /// the state texture of that style (`uipopupingameftue.xml texture0..7`)
    pub style_texture: &'static str,
    pub title_key: &'static str,
    pub body_key: &'static str,
}

/// `uipopupingameftue.xml` `<CStateWindow_Image numStates="8" texture0..7>`.
pub const FTUE_STYLE_TEXTURES: [&str; 8] = [
    "UIResults/ABK_slalom.png",
    "UIResults/ABK_race.png",
    "UIResults/ABK_fruitsplat.png",
    "UIResults/ABK_timeboom.png",
    "UIResults/ABK_bossbattle.png",
    "UIResults/ABK_dailyrace.png",
    "UIResults/ABK_dailyrace.png",
    "UIResults/ABK_icesplat.png",
];

/// The in-game "how to play" popup shown the first time a player enters a game mode:
/// RACE (4) -> 0x9f, TIME_ATTACK (6) -> 0xa0, SEED_RUSH (7) -> 0xa1, BOSS_BATTLE (0xb) -> 0xa2, SLALOM (0xd) -> 0x42.
/// Other modes (INTRO, VERSUS, JENGA, QMR/TMR/LMR, BOSS_FRUIT_RUSH) show none. `0xa3` (daily race, styles 5/6) is raised
/// from the daily race flow, not from this switch.
///
/// Trigger (`ShowInGameFTUE`): runs from the in-game HUD `Process` once per frame while the race is on; shows only if
/// Popup text comes from `CPopupManager::PopupInGameFTUE @ 003dab20` ([`ingame_ftue_popup_text`]), NOT from the 4th/5th
/// arguments of `ShowInGameFTUE`, which are only analytics strings (e.g. `"ftue"`, `"025a_ftue_race"`, `"025e_win_ftue_race"`).
/// `uipopupftuetext.xml` (`PopupFTUEText @ 003da688`) is a different, channel based text popup fired from `CFTUEManager` update
/// code (`UpdateFTUE @ 003ffc60`: welcome back, unlock texts) and is not part of any game mode.
///
/// `CPlayerInfo+0x440[type] != 0` (the "not shown yet" flag, cleared right after the popup opens, then
/// `CSaveManager::RequestSave`), `CPlayerInfo+0x6d4[type] != 0` (UNRESOLVED: second per-type enable flag), no other popup is
/// active (`CPopupManager::HasActivePopup`), the game is in a racing state (`CGame+200 >= 7`), the HUD notification
/// suppress flag `+0x608` is clear, and the FTUE manager bit of the current stage is set. While the popup is open the
/// slingshot of every human player is disabled (`CPlayer::SetSlingshotEnabled(p, 0)`).
pub fn ingame_ftue_for_mode(mode_id: u32) -> Option<InGameFtue> {
    let (nt, style, title, body) = match mode_id {
        4 => (0x9f, 1, "MODE_RACE", "GAMEMODE_DESC_RACE"),
        6 => (0xa0, 3, "MODE_TIME_ATTACK", "GAMEMODE_DESC_TIMEATTACK"),
        7 => (0xa1, 2, "MODE_FRUIT_RUSH", "MODE_FRUIT_RUSH_DESC"),
        0xb => (0xa2, 4, "BOSS_BATTLE", "BEAT_THE_BOSS"),
        0xd => (0x42, 0, "MODE_SLALOM", "MODE_SLALOM_DESC"),
        _ => return None,
    };
    Some(InGameFtue { notification_type: nt, style, style_texture: FTUE_STYLE_TEXTURES[style as usize], title_key: title, body_key: body })
}

/// `GameUI::CPopupManager::PopupInGameFTUE @ 003dab20`: the title / body loc keys and the `CStateWindow_Image` state of a
/// style. Style 2 (fruit rush) turns into the ice-cream variant (`MODE_ICE_SPLAT`, `GAMEMODE_DESC_ICE_SPLAT`, image state 7,
/// the last state) when `CGame+0x2fc` (the selected episode) is 4. Unknown styles fall back to the slalom text (`default:`).
pub fn ingame_ftue_popup_text(style: u32, selected_episode: i32) -> (&'static str, &'static str, u32) {
    match style {
        0 => ("MODE_SLALOM", "MODE_SLALOM_DESC", 0),
        1 => ("MODE_RACE", "GAMEMODE_DESC_RACE", 1),
        2 if selected_episode == 4 => ("MODE_ICE_SPLAT", "GAMEMODE_DESC_ICE_SPLAT", 7),
        2 => ("MODE_FRUIT_RUSH", "MODE_FRUIT_RUSH_DESC", 2),
        3 => ("MODE_TIME_ATTACK", "GAMEMODE_DESC_TIMEATTACK", 3),
        4 => ("BOSS_BATTLE", "BEAT_THE_BOSS", 4),
        _ => ("MODE_SLALOM", "MODE_SLALOM_DESC", style),
    }
}

// ------------------------------------------------------------------------------------------------------------
// gameftueprerequisites.xml (story FTUE chain; the manager itself is `CFTUEManager`, out of scope here)
// ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FtueState {
    pub name: String,
    pub previous_states: Vec<String>,
    pub prerequisites: Vec<String>,
}

/// Parse `<Prerequisites><State name= previousStates= prerequisites=/>...</Prerequisites>` (comma separated lists).
pub fn parse_ftue_prerequisites(xml: &str) -> Result<Vec<FtueState>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let split = |s: Option<&str>| -> Vec<String> {
        s.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default()
    };
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name("State"))
        .map(|n| FtueState {
            name: n.attribute("name").unwrap_or("").to_string(),
            previous_states: split(n.attribute("previousStates")),
            prerequisites: split(n.attribute("prerequisites")),
        })
        .collect())
}

/// States whose `previousStates` are all completed and whose `prerequisites` are all satisfied (and that are not done).
/// This is the declarative reading of the file; the exact activation logic is `CFTUEManager::GetStateActive`
/// (UNRESOLVED: not ported, it is bit-mask based and belongs to the meta/UI layer).
pub fn available_states<'a>(all: &'a [FtueState], completed: &[&str], satisfied: &[&str]) -> Vec<&'a str> {
    all.iter()
        .filter(|s| !completed.contains(&s.name.as_str()))
        .filter(|s| s.previous_states.iter().all(|p| completed.contains(&p.as_str())))
        .filter(|s| s.prerequisites.iter().all(|p| satisfied.contains(&p.as_str())))
        .map(|s| s.name.as_str())
        .collect()
}

// ------------------------------------------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn load(rel: &str) -> Option<String> {
        let p = Path::new(ASSETS292).join(rel);
        if !p.exists() {
            eprintln!("skip: {} missing", p.display());
            return None;
        }
        std::fs::read_to_string(p).ok()
    }

    #[test]
    fn parse_real_intro_eventdef() {
        let Some(x) = load("xml/gameplay/eventdef_other/eventdef_intro.xml") else { return };
        let d = IntroEventDef::from_xml(&x).unwrap();
        assert_eq!(d.game_mode_name, "INTRO");
        assert_eq!(d.character, "red");
        assert!(d.environment.contains("theme002"));
        // `<Stars 1= 2= 3=>` are not Star1/2/3 => thresholds stay 0 exactly like the original reader
        assert_eq!(d.stars, [0, 0, 0]);
        let m = IntroMode::from_eventdef(&x).unwrap();
        assert_eq!(m.variant, IntroVariant::Intro);
        assert_eq!(m.kind(), GameModeKind::Intro);
        // the other two "intro" files are normal modes
        for f in ["eventdef_intro_race2_time.xml", "eventdef_intro_race3_race.xml"] {
            let x = load(&format!("xml/gameplay/eventdef_other/{f}")).unwrap();
            assert!(IntroEventDef::from_xml(&x).is_err());
        }
    }

    fn kart(pos: u32, in_race: bool) -> KartState {
        KartState { id: 0, is_player: true, race_position: pos, race_time: if in_race { 1.0 } else { 0.0 }, ..Default::default() }
    }

    #[test]
    fn invulnerable_and_slingshot_cancel() {
        let mut m = IntroMode::new(IntroVariant::Intro, [0; 3]);
        let (mut fx, mut ev) = (Vec::new(), Vec::new());
        m.update(0.1, &[kart(1, true)], &mut fx, &mut ev);
        assert_eq!(fx, vec![CarEffect::Repair { car: 0, fraction: 1.0 }]);
        fx.clear();
        m.on_input(&ModeInput::Other { tag: "player_in_slingshot", car: None, value: 1.0 }, &mut ev);
        m.update(0.1, &[kart(1, true)], &mut fx, &mut ev);
        assert!(fx.iter().any(|e| matches!(e, CarEffect::Other { tag: "IntroCancelSlingshot", .. })));
        // Intro3 is a plain race: no repair
        let mut m3 = IntroMode::new(IntroVariant::Intro3, [0; 3]);
        fx.clear();
        m3.update(0.1, &[kart(1, true)], &mut fx, &mut ev);
        assert!(fx.is_empty());
        // Intro2 repairs but never cancels the slingshot
        let mut m2 = IntroMode::new(IntroVariant::Intro2, [0; 3]);
        m2.player_in_slingshot = true;
        m2.update(0.1, &[kart(1, true)], &mut fx, &mut ev);
        assert_eq!(fx.len(), 1);
    }

    #[test]
    fn win_only_when_first() {
        let mut ev = Vec::new();
        for (pos, won) in [(1, true), (2, false), (4, false)] {
            let mut m = IntroMode::new(IntroVariant::Intro, [0; 3]);
            m.update(0.1, &[kart(pos, true)], &mut Vec::new(), &mut ev);
            m.on_input(&ModeInput::KartFinished { car: 0, time: 30.0 }, &mut ev);
            assert_eq!(m.state(), if won { ModeState::Won } else { ModeState::Lost });
        }
    }

    #[test]
    fn grid_jumps_into_gameplay() {
        let m = IntroMode::new(IntroVariant::Intro2, [0; 3]);
        let mut ev = Vec::new();
        m.initialise_grid(&mut ev);
        assert_eq!(ev, vec![ModeEvent::Message { tag: "jump_into_gameplay".into() }]);
    }

    #[test]
    fn ingame_ftue_table_matches_xml() {
        let Some(x) = load("xml/xml/ui/uipopupingameftue.xml") else { return };
        for (i, t) in FTUE_STYLE_TEXTURES.iter().enumerate() {
            assert!(x.contains(&format!("texture{i}=\"{t}\"")), "style {i}");
        }
        let s = ingame_ftue_for_mode(0xd).unwrap();
        assert_eq!((s.notification_type, s.style, s.style_texture), (0x42, 0, "UIResults/ABK_slalom.png"));
        assert_eq!(ingame_ftue_for_mode(4).unwrap().notification_type, 0x9f);
        assert_eq!(ingame_ftue_for_mode(6).unwrap().notification_type, 0xa0);
        assert_eq!(ingame_ftue_for_mode(7).unwrap().notification_type, 0xa1);
        assert_eq!(ingame_ftue_for_mode(0xb).unwrap().notification_type, 0xa2);
        assert!(ingame_ftue_for_mode(10).is_none());
        assert!(ingame_ftue_for_mode(0).is_none());
    }

    #[test]
    fn ftue_prerequisite_chain() {
        let Some(x) = load("xml/xml/global/gameftueprerequisites.xml") else { return };
        let states = parse_ftue_prerequisites(&x).unwrap();
        assert_eq!(states.len(), 16);
        assert_eq!(states[0].name, "TutorialRace");
        assert!(states[0].previous_states.is_empty());
        let c1 = states.iter().find(|s| s.name == "Campaign1").unwrap();
        assert_eq!(c1.previous_states, vec!["TutorialRace"]);
        assert_eq!(c1.prerequisites, vec!["SaveMigrationHandled"]);
        let n = states.iter().find(|s| s.name == "Notifications").unwrap();
        assert_eq!(n.prerequisites, vec!["IsIOS", "CompletedLeaderboardFTUE"]);
        assert_eq!(available_states(&states, &[], &[]), vec!["TutorialRace"]);
        assert_eq!(available_states(&states, &["TutorialRace"], &["SaveMigrationHandled"]), vec!["Campaign1"]);
        assert!(available_states(&states, &["TutorialRace"], &[]).is_empty());
    }

    #[test]
    fn popup_text_per_style() {
        assert_eq!(ingame_ftue_popup_text(0, 0), ("MODE_SLALOM", "MODE_SLALOM_DESC", 0));
        assert_eq!(ingame_ftue_popup_text(2, 1).0, "MODE_FRUIT_RUSH");
        assert_eq!(ingame_ftue_popup_text(2, 4), ("MODE_ICE_SPLAT", "GAMEMODE_DESC_ICE_SPLAT", 7));
        assert_eq!(ingame_ftue_popup_text(4, 0).1, "BEAT_THE_BOSS");
        for m in [4u32, 6, 7, 0xb, 0xd] {
            let f = ingame_ftue_for_mode(m).unwrap();
            let t = ingame_ftue_popup_text(f.style, 0);
            assert_eq!((t.0, t.1), (f.title_key, f.body_key), "mode {m}");
        }
    }
}
