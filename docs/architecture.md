# Architecture

## Layers

```
 theme (HTML/CSS/JS)  ──uses──▶  @romdex/sdk  ──invoke/listen──▶  romdex-app (Tauri)  ──▶  romdex-core
                                                                                              │
                                     Batocera: es_systems.cfg, gamelist.xml, batocera.conf ◀──┤
                                     emulatorlauncher                                    ◀──┘
```

- **romdex-core** owns every decision that is not about a window: parsing Batocera's files,
  the SQLite library index, the hierarchy engine, building `emulatorlauncher` commands and
  normalising gamepad input. It does not depend on Tauri, so it is testable anywhere.
- **romdex-app** is deliberately thin: a fullscreen window, the `romdex://theme` and
  `romdex://media` protocols, one command per SDK call, and the permission check in front of
  each privileged command.
- **@romdex/sdk** is the only API themes use. It is versioned (`theme.json` declares
  `"sdk": "^1"`) and ships a mock backend so themes can be built in a normal browser.

## The library database

SQLite (WAL) is RomDex's **primary store**. Boot reads it directly, so the UI appears without
parsing any XML. `gamelist.xml` is not the store, for four reasons: it has to be parsed in full
per system before anything shows, it has no indexes for grouping or search, every change
rewrites the whole file, and it has nowhere to keep RomDex's own data (RetroAchievements ids,
ROM hashes, playtime, custom collections).

Data flows in from three sources, and each field records which one set it:

| Source                | How it arrives                                                     | Precedence |
| --------------------- | ------------------------------------------------------------------ | ---------- |
| The user, in RomDex   | Favourites, collections, settings                                  | highest    |
| External import (#57) | Versioned JSON Lines dropped in a watched folder, e.g. RetroWorker | middle     |
| `gamelist.xml` (#9)   | Whatever Batocera's or any other scraper wrote                     | lowest     |

- **Batocera's files are read, never owned.** Batocera, Skraper and other scrapers keep writing
  `gamelist.xml`; RomDex re-imports only the files that changed (#55 watches for them).
- **RetroWorker feeds the richer route.** It can export what `gamelist.xml` cannot carry —
  notably the ROM hashes it already computes for RetroAchievements, so a device never has to
  re-hash its library. The import format is a documented contract, not shared code, so the
  two projects stay independent.
- **Write-back is optional and later** (#58, M3): favourites and play stats only, atomically,
  so EmulationStation still sees them when it is the fallback.

RomDex still launches through Batocera's own `emulatorlauncher`.

### Measured

`cargo run --release -p romdex-core --example bench_library` generates 10 systems × 2,000 games
with full gamelists and times what a user feels. On an Apple M-series laptop, 2026-10-30:

| Operation                                  |   Time |
| ------------------------------------------ | -----: |
| First import (scan + parse every gamelist) | 1.19 s |
| Refresh with nothing changed (scan only)   |  98 ms |
| Cold boot to the first list (2,000 games)  |  14 ms |
| Search, top 50                             | 2–4 ms |

The first import was 3.2 s while the search index was kept up by triggers; maintaining it from
code, only for games whose searchable text changed, cut it to 1.19 s. Expect a Raspberry Pi to be
several times slower: the spike (#39) measures it on real hardware. Cold boot is the number that
matters most and is independent of library size, since it reads one system's rows.

## Input

Gamepads are read in Rust (gilrs) and delivered to the theme as normalised actions (`up`,
`down`, `a`, `b`, `start`, `hotkey`…), because the Gamepad API in WebKitGTK is not reliable
enough to build a sofa UI on. Themes can still use the browser Gamepad API directly.

## Decisions to revisit

- **Runtime.** Tauri needs WebKitGTK on Linux, which Batocera does not ship. Spike #39 decides
  whether that is viable on x86_64 and RPi4. Findings go in
  [batocera-integration.md](batocera-integration.md).
