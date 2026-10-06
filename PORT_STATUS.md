# Port status (honest)

Source: Angry Birds Go! v1.0.1 APK (`com.rovio.angrybirdsgo`, Exient XGS engine, native code in `libABK.so`).
Standard: 1:1 with the original. Rip from the original data and code, never recreate.

## 1:1 (original data, decoded and used as-is)
| Item | How |
|---|---|
| Textures (`.xgt`, 93 files) | `abgtool` decodes RGBA4444 / RGBA8888 / LA88 / ETC1 to PNG |
| Archives (`.pak`, 7 files, 3,166 entries) | `abgtool unpack` (KPX v1 + the 64-bit v0x41 variant) |
| Sprite atlases (`.atlas`) | exact sprite rectangles and names, JSON in `assets/atlas` |
| Bitmap fonts (`.fnt` + glyph sheets) | used for all text |
| Tokenised XML (`XOX1`, 517 files) | decoded; kart `CarSpec`, track definitions, `sound.xml` |
| Models (`.xgm`, 2,221 files) | geometry + node transforms parse with zero failures |
| Audio (888 mp3) | played as-is |
| Launch sequence art, landing and event-select art | original sprites |

## NOT 1:1 yet (my own placeholders - to be replaced by the decompiled game code)
| Item | Why |
|---|---|
| Screen layouts (button positions, scales, transitions, sounds) | positions were estimated by eye; the real values are in `libABK.so` |
| Event-select -> track-select flow | invented; original flow is in the code |
| Everything under `--dev-race` | procedural track, my own tyre/engine model, AI, pickups, HUD placement. Off by default. |

## Cannot be 1:1 from this APK
`assets.xal` lists 20 archives; 13 were downloaded from Rovio's cloud on first launch and are not in the APK:
`models.pak` + `textures.pak` (track geometry/textures, per theme), `cartextures.pak`, `animation.pak`, `effects.pak`,
`eventdef_*.pak`, `misc.pak`, `smackables.pak`, `store.pak`. The servers are shut down.
The game caches them on the device (`/sdcard/com.exient.ABK`, `XGSCache:assets.xal_remote`).
**Real tracks need that cached data from a device where the game had been run.**

## Next (decompile-driven)
Ghidra project: `C:\AngryBirdsRef` (not part of the repo; derived from the original binary).
Port, in this order, from the decompile: screen layouts and flow, the vehicle dynamics, race rules, AI, HUD.

## Update: second APK (v2.9.1, `angry-birds-go-2-9-1.apk`) extracted to `apk291_extracted`, archives in `assets291/pak`
- Still NO race track geometry (only `track.xml` definitions, 15 of them). Its `theme002/models.pak` is the main-menu 3D scene
  (`frontend.xgm`, showroom, skybox, podium, gift box / pig rigs, animations), not a circuit.
- NEW and usable for a 1:1 port: `xml/ui.pak` = the real screen layouts (`uilandingscreen.xml`, `uikartgaragescreen.xml`,
  `uikartselectscreen.xml`, `uimapscreen.xml`, `uilmpselecttrackscreen.xml`, ...), `eventdef_*` = every race/event definition,
  `ui_core.pak` (17 MB UI textures), `misc.pak`, `store.pak`, 68 `envobjects`.
- Archive format v2 (magic byte 2, 0x34 header, directory records) is read by `abgtool`. Two archives (`localisation`,
  `sounds_core`) use a second compression scheme that is not decoded yet.
- Port base should move to 2.9.1 (final release): its `libABK.so` is 17.7 MB (arm) and 18.3 MB (x86).

## Update 2: 2.9.1 data is readable, port base moved to 2.9.1
- XOX2 (2.9.1 tokenised xml) decodes: 899 of 912 files (the rest are event definitions with comment-like names). Rules, from
  `CXGSXmlReader::CreateXmlDoc` / `NodeDeobfuscate` in the 2.9.1 decompile: same container as XOX1, a different fixed 115-byte
  token table, the document is wrapped in one throw-away element, table slot 0 is a per-file alias (first use takes the next
  string, later uses repeat it), every other token is numbered by first appearance.
  `assets291/xml/xml/ui/*.xml` now holds the real screen layouts, e.g. `uilandingscreen.xml` (window tree, positions in
  %/pt, textures, text labels, fonts, click actions such as `LandingScreen_NewUser`).
- XGST textures of 2.9.1: header word 0x001C0020, format codes 0x03 RGBA4444, 0x04 RGBA8888, 0x18 DXT1, 0x23 ETC1
  (PVRTC / ATC variants are skipped; every texture ships in several of them). 148 textures converted, 0 failures.
- KPX archive v2 (magic byte 2) is read; `localisation` and `sounds_core` use a second compression flag (not decoded yet).
- Decompiles in `C:\AngryBirdsRef`: `decomp/` (1.0.1) and `decomp291/` (2.9.1, 33,661 functions, class names present).
  `CCarSpec::Read` (2.9.1 @ 001b58d4) shows the real kart-spec struct offsets (e.g. m_fDrag +0x42c, m_fTorqueScalar +0x43c).
- NOT done yet: the UI interpreter that draws these layouts, screen flow / global states, the 3D frontend scene, the vehicle
  dynamics port, the slingshot, tracks (still not in either APK).

## Update 3: the real 2.9.1 screens run in the game (landing -> map -> settings/shop/garage -> drive)
- `game/src/uix.rs` interprets the original layouts (`assets291/xml/xml/ui/*.xml`): pt/%/px units, pivots, `style=`,
  `Include block=` parameter expansion, sprite atlases (`.atlas` files read by abgtool), 3-slice / 9-slice panels, SDF text,
  `visibility` / `alpha`, `CBehaviourTouchInput` click actions and `CBehaviourSound` click sounds. Every sprite, position,
  string (from `locdb.xlc`) and font (`*_sdf_32`) is the game's own data.
- `game/src/flow.rs` routes the global-state names the layouts emit (`LandingScreen_NewUser`, `settings`, `shopScreen`,
  `kartGarage`, `topbarBackButton`, `NextCampaignPage`, `CampaignMarkerSelected` ...). The flow table itself is MY reading of
  those names (the original state machine is in the decompile and has not been ported): anything online (daily race,
  tournament, facebook, gacha ...) shows "NOT AVAILABLE OFFLINE".
- Campaign map: chapters / event markers come from `campaignmapdefinition.xml`. The island path tiles (`ABK_Map_N`) are not in the
  atlas, so they are not drawn. Event icons use `abk_map_<theme>` with my episode -> theme guess (index / 11).
- abgtool now reads the LZ4 pak scheme (`localisation`, `sounds_core`), `locdb.xlc` (`abgtool loc`), the 2-channel SDF font sheets
  (format 0x08: red = inner distance, alpha = outer) and decodes the XGSF v2 font records.
- NOT done: dynamic data in the screens (player stats, energy, ranks), screen animations (`CBehaviourAnimation`), the 3D frontend
  (2.9.1 `.xgm` is a newer model format than abgtool reads), outline colour of the bold SDF font, tracks.
- Driving physics: a background port of `CCar` / `CWheel` from the decompile is being written to `game/src/carsim.rs`;
  until it is wired in, `kart.rs` (my placeholder) is still what drives the baseplate.
- Test: `tools/click_test.ps1` posts window messages (no real mouse): landing -> GO -> map -> settings -> back -> campaign -> baseplate.

## Update 4: driving runs on the ported CCar / CWheel code (`game/src/carsim.rs`), slingshot launch
- `carsim.rs` (ported from the 2.9.1 decompile, every fn commented with its symbol + address): CarSpec read/defaults/CopyWithMods,
  CWheel (tyre load sensitivity, slip curves, Integrate), CXGSRigidBody dynamics, CCar Update/Integrate (thrust toward
  MinDesiredSpeed, drag/downforce, steering, brakes, anti-roll, boost), slingshot launch velocity (GetLaunchVelocity: 30..50 m/s).
  10 unit tests pass (`cargo test --profile fast carsim`). There is no engine/gearbox in the 2.9.1 car: thrust is automatic.
- `kart.rs` (my old bicycle model) is no longer used by the baseplate drive. `drive.rs`: slingshot phase (hold Up/Space to pull back to
  at most 8.4 m, release to launch), then the CarSim on a flat plate. Steer left/right, Down = brake.
- NOT 1:1 / unresolved (also listed in carsim.rs as `UNRESOLVED`):
  * the 2.9.1 base car spec (mass, suspension) is in cloud-only `carSpec.pak`; I use the 1.0.1 `kart_base.xml` chassis block with
    the 2.9.1 `kart_red_min/max.xml`. The six upgrade ratios come from `kartupgradelevels.xml` (SSKM level 0); the stat -> ratio
    assignment is my reading of the names.
  * the touch-steering pipeline (`Steer_Touch_Width`, `Steer_Exponent`) was not ported. With full input the ported steering spins
    the car out at speed, so `Drive::steer_limit` (mine) caps the keyboard input by speed.
  * `CPlayer::UpdateSlingshotLaunch` (touch drag -> pull ratio, camera) not ported: the pull is a keyboard bar, not the drag gesture.
  * chassis collision / contact solver, tyre damage, track-spline dependent behaviour.

## Update 5: the original player-input code is wired in (`game/src/playerinput.rs`)
- Ported from `CPlayer::ProcessInput @001470e8` (push-button steering ramp with `m_fSteerAdjRate` 12 / `m_fCentreSteerAdjRate` 16,
  the touch-drag branch, brake gates) and `CPlayer::UpdateSlingshotLaunch @00147a90` (pull curve table read from the binary,
  0.27 launch threshold, pull offset in the camera frame). 8 unit tests; tilt steering, multiplayer early-launch, game mode 8 not ported.
- Slingshot in the drive: mouse drag down (touch path) or hold Down (the original pad path: a held key reads as full deflection) and
  release / tap Space. The pull ratio from the original curve feeds `slingshot_launch`.
- Honest gap: with the 1.0.1 chassis block the ported arcade steering (yaw target ~1.6 rad/s at full input) spins the car out at speed,
  so `Drive::steer_limit` (NOT original) limits the keyboard steering by speed. The real 2.9.1 base spec (cloud-only carSpec.pak) is
  probably what keeps the original stable.
- The slingshot prop (`slingshot.xgm`) is the original mesh but its scale/placement is eyeballed.

## Update 6: full karts
- All 62 kart variants of `pak/cargeom` (v1.0.1 geometry) are available: chassis + wheels + every body part (nose, wood panels, milk carton,
  straw end ...), each part's `attach_1` node placed on the chassis `attach_<Part>_1` node. Garage: Left / Right pages through them.
- Missing from the APKs: kart textures (cloud `cartextures`), the driver birds (`attach_pilot_*`), animation and effects, so karts are flat
  colour per character (my palette), without pilots.
- Game modes: the event definitions (`eventdef_*`, race / time attack / fruit rush / boss / VS ...) are decoded, but every mode needs track
  geometry that is not in either APK, so only the baseplate is playable.

## Update 7: FULL GAME DATA FOUND (itch.io "Angry Birds Go!" 2.9.2 build, `apk292/`, unpacked to `assets292/`)
- Source: refurbish-entertainment.itch.io/angry-birds-go (free download, 275 MB APK). It ships the data the cloud used to serve:
  `data/environments/theme002..006/{models,textures}.pak`, `tracks/run*/track.stm` (15 tracks, 3-21 MB each, `STME` static mesh)
  + `track.xml`, `cars/theme00N/{cargeom,cartextures}.pak`, `cars/carspec.pak` (the missing base car spec), `characters/{models,animation}.pak`
  (the bird drivers), `effects.pak`, `smackables.pak`, `screens.pak`, `ui_core.pak`. All 48 paks unpack with abgtool (zlib + LZ4), 472 MB.
- (A 1.8.7 "Full" APK from archive.org, `apk187/`, has only theme002 + effects + characters models + carspec; its 265 MB `data.save` is encrypted.)
- Next: decode `STME` track meshes + 2.9.x XGSM models, textured 3D pass, real tracks / karts with textures / drivers, the real car spec.

## Update 8: real tracks, models and textures decoded; gameplay systems being ported
- abgtool now reads `track.stm` (STME track meshes + collision + splines), 2.9.x `.xgm` models (incl. skeletons/skin), `.xga` animations,
  all texture variants (837 PNGs in `assets292/textures`), XOX2 (all 1855 xml, `assets292/xml`), `shaders.xmat`. FORMATS.md has the specs.
- game: `trackworld.rs` (textured track + collision `Ground` + start pose + race line), `xmodel.rs` / `fullkart.rs` (textured models; a full
  kart = chassis + wheels + body parts + driver bird), `render3d.rs` textured mesh pipeline. `--capture-track`, `--capture-model`,
  `--capture-kart`, `--capture-drive --event N` render the real data headlessly. A campaign event now loads its real track.
- Open problem (agent working): the kart leaves the steep start slope and falls off; stable driving on the track (launch, ground contact, steering).
- In progress as standalone modules (agents): items.rs (track items, pickups, smackables, structures), racerules.rs + raceai.rs, meta.rs
  (profile / economy / upgrades), powerups.rs + abilities.rs + modes/ (bosses, fruit rush, time attack, slalom, jenga, versus ...).
- Not wired into the game yet; the textured karts are not yet used by the drive scene.

## Update 9: the gameplay modules are integrated; a campaign event now runs as a real race (2026-10-06)
New: `game/src/eventrun.rs` (EventRun: one event on its real track), `game/src/eventextras.rs` (ability / damage / power-up glue),
`game/src/modes/mod.rs`. `main.rs` now declares `abilities, damage, eventextras, eventrun, modes, powerups` (racerules + raceai already were).
Build `cargo build --profile fast` clean, `cargo test --profile fast`: 229 passed, 0 failed.
Headless check: `abg-game --capture-event <campaign index> <seconds> out.png [--autopilot] [--to-results] [--use-flow] [--event-file <path below xml_gameplay>]
[--powerup speedbooster|kingsling|autorepair] [--ability-at <race seconds>]`. It never spends the player's energy or writes their save unless `--use-flow`, which uses
the profile next to the exe that is run (e.g. `target_int/fast/abg_profile.sav`). Screenshots: `game/captures/update9_*.png`.

### Wired (runs in the game: Map -> campaign marker -> Scene::Event)
* Event selection: `campaign[index].eventIndex` -> `<Event index>` row -> `eventdef_episodeEE_eventNN_stageSS.xml` (NOT the old `index / 11` guess, which picked the
  wrong file: campaign 0 is `episode00_event04_stage00`). The track comes from that file's `<Environment>`.
* Rules (`racerules`): `RaceTrack::from_stm` on the real `track.stm` splines / helpers (TrackWorld now keeps the parsed Stm + track.xml), `RaceState` built from the
  event def (kart count, timer / seed thresholds, difficulty adjust from the event CC vs the profile kart CC), positions, finish-line detection, game over, results
  (score from scoreconfig, stars with the event's thresholds), `bind_campaign_event`.
* Grid: `RaceState::grid` slots (slot 1 = player) on the main race line, karts dropped onto the collision surface there (not `track.start`).
* Player: the existing `Drive` (CarSim, slingshot, chase camera) with the profile's kart (`kart_view(selected_kart).visual_model` -> geometry folder, textured via
  `FullKart`) and the profile's `mod_spec`. One profile: the flow's `Meta` (`meta::with_global` is not used by the race).
* AI: `raceai::RaceAi` on real `CarSim` karts with `FullKart` models: skill from economy SkillBase + variance inside the event's AI range, spline choice by weights,
  catch-up flag, slingshot pull / release with `Process`' delays, the speed controller `accelerations`, spline switching, stuck respawn. The full `AiCtx` (phase,
  neighbours, player obs, race clock) is filled every frame; the ported steering needs the same sign flip as the player's (car frame is mirrored).
* Countdown 3-2-1-GO with the original `ingame` atlas sprites. NOT original for single player: the original shows it only when `CGame::ShouldDoCountdownStart
  @0011b158` (more than one human, or the first-race FTUE). The race clock starts at GO, each kart's own clock at its slingshot release.
* Results screen (layout is mine, atlas sprites are the original ones): standings, stars, score, coins, reward lines. Applied to the profile through `Meta`:
  `start_campaign_event` (energy cost; checked before the load, paid once the event is built), `complete_campaign_event` (stars, best score, rewards, XP / rank-up),
  coins picked up + bonus coins, then `save`. Verified through the real flow profile: energy 20 -> 19, coins 0 -> 1024, xp 30, campaign[0] 3 stars / score 20087, file written.
* Items (`items::ItemWorld` on the real track): coins / mega coins / gems / gift boxes, boost pads (-> `CarInput::pad_boost`, the same push as the pad materials, no
  double push), seed-rush tokens (-> fruit counter and the fruit score), smackables (the hit response `kart_dv` is applied to the player), slalom gates (a missed gate costs
  its time penalty on the race timer). Item models are drawn through `XModel` (range culled at 450 m). Coins and fruit picked up by the player are counted.
* Modes: RACE, TIME_ATTACK, SEED_RUSH, BOSS_BATTLE (race + boss AI kart) and SLALOM run through `racerules`, with the matching `modes/*` object beside it where one is wired
  (`TimeAttackMode`, `SeedRushMode`, `VersusMode`, `BossBattle` get per-frame kart states + `SeedCollected` / `KartFinished` / `Smashed` inputs; their state / score is
  printed by the capture, `racerules` stays the authority for finish, score, stars). HUD: mode name, race / timer clock, fruit count, position badge, coins, a progress
  bar with every kart (the tracks are point to point, no laps), speed, ability bar, damage bar, countdown, FINISH banner. Final-binary sweep of campaign 0..54 with the autopilot (`--to-results`, 500 s of simulated time each):
  47 reached the results screen (Race, Time Attack, Fruit Rush with 131 fruit collected and the goal reached, Boss Battle); 2 and 37 are intro / tutorial events that name no track;
  41, 44, 48, 52, 53, 54 did not reach the results because the autopilot (CRaceAI at skill 0.1, 30 m/s) gets stuck or keeps falling at one spot of those tracks (it respawns, then falls
  again; the normal AI karts pass). Slalom and Versus were run through `--event-file`. The map marker index (`CampaignMarkerSelected:N`, `EventMarker campaignIndex`) is the
  position in `meta.data.events.campaign`, the index `start_event` expects.
* Ability button (key E, `CarAbilitySlot::can_trigger / trigger`, per-frame `tick`, `integrate` once per 1/60 s physics step): only what `abilities.rs` has is playable, i.e.
  the Red / Yellow / Blue speed boosts. Verified: `RedSpeedBoost` triggers after its reuse delay, its `BodyForce` is applied to the car, speed 30 -> 35.7 against the AI speed
  governor. `SetSteeringMultiplier` reaches the player's steering.
* Damage (`damage::Bodywork`, no parts): per-frame wear from the wheel materials' `wear_rate`, smackable hits through `Bodywork::on_collision`, pilot-ejection flag, AutoRepair.
* Power-ups (`powerups`): `integrate_powerups` -> `CarInput::boost` (SpeedBooster, boost flag verified), KingSling launch scale, AutoRepair flag. There is no pre-race selection
  screen, so they are only chosen by the `--powerup` test flag.
* Respawn: key R (`CCar::Respawn(-1)` analogue, +1 death for the score) and an automatic one when there is no ground below for 1.5 s (AI karts too).

### Partly wired
* `modes/*`: the objects are driven and printed but not authoritative. Slalom uses `items::slalom` gates + the racerules timer (`modes/slalom.rs` is not driven). Boss abilities and
  the boss weapons (`CarEffect`s of `BossBattle`) are not applied. Versus has no rules beyond its mode object.
* Abilities: effects other than forces / steering multiplier are only collected (particles, sounds, `SetTimeScale`, gravity / downforce multipliers end up in
  `PlayerExtras::unsupported`). Only the player has an ability; AI karts never trigger one (`obs.ability = None`).
* Damage: the impact impulse of a smackable hit is rebuilt from the item's `kart_dv` (carsim has no chassis contacts, so there is no real collision impulse). No kart-kart,
  kart-wall or ground-contact damage, no body parts / pilot-detach visuals; the hp meter has no feeder (as in the decompile).
* Items: AI hotspots are placed but their `HotspotTaken` events are not routed to a specific AI (`set_hotspot_target` is never called); structures / env objects of the item
  world are placed but only pickups and smackables are drawn; the gift box / power box grants are not applied (`PowerBoxSmashed` has no grant in the decompile).
* Meta / data choices that are mine (marked UNRESOLVED in the code): `selected_character` empty -> driver from the kart folder, the AI kart roster, the AI karts use the
  player's mods, the default kart count when `kartcount` is absent (TA / Slalom 1, boss 2, else 4), a fixed 2.5 s from game over to the results.

### Not wired
* JENGA, INTRO / tutorials (campaign 2 names no track), LMR / QMR / TMR: `EventRun::new` refuses them with a message (the race scene has no tower / tutorial rules).
* Kart-kart collisions, the original finish camera / fireworks timing, the original HUD and results layouts (`uiresultsscreen.xml` is not rendered), race sounds (countdown /
  ability / item sounds are `.xopus`, not played), the power-up shop selection, bus / skydive / glide / emote (on hold).
* Driving a whole event with the keyboard was not exercised (only the autopilot: `CRaceAI` at skill 0.1 drives the player; it often loses, so "Failed" results in the headless runs
  are the autopilot, not the rules). Episode 2 events 8 / 24 make the autopilot fall off the track repeatedly (41 respawns before it finishes); the chase camera has no collision
  and can sit inside scenery at the start (seen in `update9_countdown.png`, `update9_seedrush_hud.png`).
* Update 9 addendum: the windowed path (Map -> marker click -> event) was not clicked through; the same code runs under `--capture-event ... --use-flow`. Locked markers are not
  clickable on the map (`campaign_markers`), so the "EVENT LOCKED" notice is only a safety net; with a fresh profile only the first marker starts an event (before Update 9 every
  marker started a baseplate drive). The unlock rule (`Meta::is_event_unlocked`) is UNRESOLVED in meta. `--cam x,y,z` places the camera for captures (`update9_race_ai_kart.png`
  shows a textured AI kart ahead of the player). No git commits were made (no-git rule), so "commit each step" is not applicable.

## Update 10: camera (2026-10-06)
The invented yaw-lagged chase camera (6.4 m behind, 2.5 m up, no collision) is replaced by a port of `CCamera` (`game/src/camera.rs`, `ChaseCam`), used by `Drive`
(baseplate drive and the event player) and the legacy `race.rs` scene. `cargo test --profile fast`: 239 passed (10 new camera tests). Captures: `game/captures/update10_{before,after}_ev*_{start,drive}.png`.
* Ported (decompile addresses): `CCamera::SetRearCam [clone .part.31] @ 000df91c` (offsets in the car frame: camera `(0, (1 - CamHeightMod) * Height + 0.8, (-4 + 2 * COMz - CamBehindMod) * Behind)`,
  look-at `(0, 1.8, 2.5)`, `Lateral_Move_With_Slide`, forward axis blended to the velocity direction `Blend_To_Move_Dir`, right axis re-derived `fwd x up`, FOV `0.84823 * FOV * CamZoomMod`),
  `CCar::GetCamHeightMod @ 001a3fa4 / GetCamZoomMod @ 001a4070 / GetCamBehindMod @ 001a4210`, `CCamera::DoSmoothing @ 000e3b70` (case 0, `k = Smooth_* / 60 / dt`), `CCamera::Process @ 000e46d4`
  (type 0 / 9 tail: upside-down correction `UpsideDownCorrection @ 000e60e0`, minimum distance 8 m, up-vector nudge, look-at shake offset, slingshot -> rear hand-over of `this+0x170`),
  `CCamera::DoCollisionCheck @ 000e3fcc` (inlined in `Process`: ray look-at -> camera, sphere radius 0.75, lift by a quarter of the pull-in), `CCamera::ApplyCameraShake @ 000e1740` (impact part),
  `CCamera::SetSlingshotCam @ 000e3050` (frame, distance `-10 - 7.4 * pull`, look-at 10 m ahead), `SetSlingshotCamPitch/Yaw` clamps. Settings come from the `<Camera>` block of
  `assets292/xml_gameplay/misc/debugtweakables.xml` (`CamTweaks`; indices 5..0x19 and 0x2a..0x2c verified against how the camera code uses them).
* Collision: `Ground::cam_ray / cam_sphere` (two-sided ray and sphere against the track triangles with the camera surface filter `_FilterCameraCollision @ 000df7f4`: ids 0x1d 0x1e 0x21 0x25 0x26 ignored,
  ids 7 and 9 are kept, unlike the car's ray); `TrackGround` implements them with its own triangle grid.
* Wiring: `Drive` owns `cam: ChaseCam` (slingshot camera while in the slingshot, rear camera after the release with the original hand-over; `cam.cut()` on respawn), the slingshot touch ray now uses the
  real camera frame, `EventRun` feeds the impact shake (`Bodywork::cam_shake_mod`). `--cam x,y,z` / `debug_cam_rel` still override the camera for captures.
* Result: the event starts that sat inside terrain (`update9_countdown.png`, `update9_seedrush_hud.png`) now show the kart and the track (ev 0, 5, 8, 12, 20, 25, 33, 40, 47 start frames).
* UNRESOLVED: intro / fly-in camera (`SetIntroCam @ 000e2844`, `UpdateIntroCam`: types 3 / 2 need the spline-cam data, the countdown uses the slingshot camera instead); touch-driven slingshot yaw / pitch
  (arguments lost in the decompile, angles are 0 so the camera looks along the launch direction) and the angle easing of `SetSlingshotCam`; the `SetSlingshotCam` `this+0x120` blend-in timer; shake time base
  and the phase of its second `sinf` (same phase used) and `CCamera::Shake(a, b, c)` (`this+0x90..0x9c`); the engine shake (`Engine_Shake` is false); `CCar::GetCamBehindMod`'s SpeedBooster branch
  (needs `CCar+0x1c44`; `behind_boost` is an input, `None` for now) and the ability hooks at `car+0x1c2c`; `CCar+0x1b78` (acceleration term) is only ever written with 0; the spline up vector for the
  upside-down correction (`track_up` input, `None` = no correction); `car+0x4ac` penalty and the pilot-detached branch (assumed not active); whether the engine FOV is the full vertical angle (assumed; the
  frontend default 0.848 rad = 48.6 deg suggests it) -- the on-screen FOV is now 36..48 deg vertical instead of the old 62..80; `CCamera::ApplyDriftRoll @ 000e16e4` and `GetCarHPR @ 000e1f60`
  have no callers in the decompile (the roll accumulator `this+0xd0` is only ever reset to 0), not ported. AI karts and items are not part of the camera collision (as in the original filter).

## Update 14: stop point
Work stopped here at the user's request. Four fix streams were cut mid-way and are NOT done: driver seating/animation (`fullkart.rs`),
the full gameplay playtest/fix pass, chassis/wall/kart-kart collision (`carsim.rs`), and the remaining abilities (`abilities/*`:
Bomb, Terence and King Pig are ported as modules, Hal/Matilda/Moustache/Stella/Bubbles/Minion/ObjectSpawn are still stubs).
`abilityrun.rs` (in-race ability glue) was left partly integrated; `ABILITY_OBJECT_MODELS` is empty and boss/ability objects are not spawned.
Build is clean, 262 tests pass, and a headless event capture runs. See README.md for the overall completeness table.
