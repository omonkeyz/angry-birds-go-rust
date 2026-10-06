# Angry Birds Go! — native Rust port

A native, windowed Rust port of **Angry Birds Go!** (Rovio, Exient XGS engine), built by reading the original game's data formats
and its decompiled native code (`libABK.so`) rather than recreating it. Stack: `wgpu` 30, `winit` 0.30, `glam`, `rodio`.

> **Status: work in progress, not a finished game.** A campaign event is playable from the map to the results screen on the real
> tracks, with the real menus, textured karts and AI. A large part of the rest is ported as tested modules but is not wired in yet.
> Details and honest gaps are below and in [`PORT_STATUS.md`](PORT_STATUS.md).

This repository contains **code only**. The game's art, audio, models and the decompile are Rovio's and are not included
(see [Getting the data](#getting-the-data)).

## What is playable today

Landing screen → map → pick an unlocked event → slingshot launch → race on the real track against AI karts → finish → results
(stars, score, coins) → back to the map. The profile (energy, coins, ranks, unlocks) is saved next to the exe.

| Controls | |
|---|---|
| Left / Right | steer |
| Down | brake |
| Mouse drag down + release (or Space) | slingshot |
| E | ability |
| R | respawn |
| Esc | leave |

Run with `Play Angry Birds Go.bat` (add `--landing` to skip the intro). Delete `abg_profile.sav` to reset progress.

## How complete is it

Percentages are rough, by feature area, and measure "ported from the original and working in the game", not lines of code.

| Area | State | ~Done |
|---|---|---|
| Asset formats (paks, textures, atlases, fonts, strings, models, tracks, animations decode) | decoded and used | 90% |
| Front-end menus (UI layout system, SDF text, localisation, landing, map, settings, shop/garage screens) | real layouts rendered; many screens are shells, online features stubbed offline | 60% |
| Real tracks (15 themed tracks, textured, sky, items placed from event defs) | rendering and item placement work; water/special shaders and effects missing | 65% |
| Karts (textured chassis, wheels, parts, drivers) | rendering works; driver seating/animation is not right yet | 55% |
| Car physics (original `CCar` integrator, wheels, steering, slingshot, thermals) | ported; chassis/wall/kart-kart collision missing | 55% |
| Driving camera | original chase camera ported; intro fly-in and several terms unresolved | 60% |
| Race rules, positions, finish, stars, rewards | ported and wired | 70% |
| AI drivers | ported and wired; jump/collision-avoidance parts partial, abilities unused | 55% |
| Game modes | Race, Time Attack, Fruit Rush, Boss Battle, Slalom run; Jenga/Versus/intro/multiplayer ported as modules but not started from the game | 55% |
| Abilities | framework + Red/Blue/Yellow/Overtake speed abilities wired; Bomb/Terence/King Pig partly ported, rest are stubs | 20% |
| Damage / bodywork | ported as a module; only partly fed by collisions | 35% |
| Power-ups, pickups, challenges | ported as modules; speed booster/king sling/auto-repair wired behind a test flag, no shop selection | 35% |
| Meta game (XP, ranks, energy, economy, upgrades, gacha) | ported, drives the map/top bar and rewards | 65% |
| Audio | music/UI sounds play; race sounds mostly not hooked up | 30% |
| Effects / particles / post-process | not ported | 5% |
| 3D front-end scene, driver animation playback, UI animations | not ported | 10% |
| Online features (multiplayer, tournaments, cloud) | servers are gone; stubbed | n/a |

**Overall: roughly 45–50% of a complete 1:1 port**, with the core "start an event and race it" loop working.

### Known problems
- The driver bird is not properly visible in the kart.
- No chassis, wall or kart-vs-kart collision, so on some tracks the kart falls off or gets stuck; the autopilot finishes about
  half of the 14 tested events (0, 4, 11, 19 cleanly; theme004 thermal gaps and several long tracks fail).
- The 3-2-1-GO countdown shows before every event; the original only shows it for multiplayer and the tutorial.
- AI karts never use abilities; boss weapons have no effects yet.
- Jenga, tutorial intro and multiplayer modes refuse to start.

## Repository layout

| Path | What |
|---|---|
| `game/` | the game (`abg-game`): renderer, UI, physics, race, modes, meta |
| `abgtool/` | asset tool: unpacks paks and decodes the formats |
| `FORMATS.md` | the decoded file-format specs |
| `PORT_STATUS.md` | detailed, honest per-update status log |
| `tools/` | test helpers (window-message based input; no real mouse) |

## Building

Rust (stable), Windows.

```
cd abgtool && cargo build --release          # asset tool
cd game && cargo build --profile fast        # the game
cd game && cargo test --profile fast         # ~260 tests
```

## Getting the data

The game expects the original data unpacked next to the repo (not distributed here):

- `assets/` — v1.0.1 APK data, `assets291/` — v2.9.1, `assets292/` — the full v2.9.2 build (tracks, kart textures, drivers, event defs).
- Unpack with `abgtool` (`abgtool unpack`, plus the texture/xgm/stm commands; see `FORMATS.md`).

You need your own legitimate copy of the game. The cloud-delivered content (track geometry, kart textures) is only in the later builds.

## Method

- Port from the original data and the decompile; never recreate. Every ported routine cites its original address in a comment.
- Anything that is not original is marked `UNRESOLVED` / `NOT original` in the code and listed in `PORT_STATUS.md`.
- Decompiles and Ghidra projects are never committed.

## Legal

Angry Birds Go! is © Rovio Entertainment. This is an unofficial, non-commercial research/preservation project and is not
affiliated with or endorsed by Rovio or Exient. No game assets are included.
