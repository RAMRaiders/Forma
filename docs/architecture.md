# Architecture

## Layers

```
 theme (HTML/CSS/JS)  ──uses──▶  @forma/sdk  ──invoke/listen──▶  forma-app (Tauri)  ──▶  forma-core
                                                                                              │
                                     Batocera: es_systems.cfg, gamelist.xml, batocera.conf ◀──┤
                                     emulatorlauncher                                    ◀──┘
```

- **forma-core** owns every decision that is not about a window: parsing Batocera's files,
  the SQLite library index, the hierarchy engine, building `emulatorlauncher` commands and
  normalising gamepad input. It does not depend on Tauri, so it is testable anywhere.
- **forma-app** is deliberately thin: a fullscreen window, the `forma://theme` and
  `forma://media` protocols, one command per SDK call, and the permission check in front of
  each privileged command.
- **@forma/sdk** is the only API themes use. It is versioned (`theme.json` declares
  `"sdk": "^1"`) and ships a mock backend so themes can be built in a normal browser.

## The library database

SQLite (WAL) is Forma's **primary store**. Boot reads it directly, so the UI appears without
parsing any XML. `gamelist.xml` is not the store, for four reasons: it has to be parsed in full
per system before anything shows, it has no indexes for grouping or search, every change
rewrites the whole file, and it has nowhere to keep Forma's own data (RetroAchievements ids,
ROM hashes, playtime, custom collections).

Data flows in from three sources, and each field records which one set it:

| Source                                       | How it arrives                                                     | Precedence |
| -------------------------------------------- | ------------------------------------------------------------------ | ---------- |
| The user, in Forma                           | Favourites, collections, settings                                  | highest    |
| External import ([format](import-format.md)) | Versioned JSON Lines dropped in a watched folder, e.g. RetroWorker | middle     |
| `gamelist.xml` (#9)                          | Whatever Batocera's or any other scraper wrote                     | lowest     |

- **Batocera's files are read, never owned.** Batocera, Skraper and other scrapers keep writing
  `gamelist.xml`; Forma re-imports only the files that changed (#55 watches for them).
- **RetroWorker feeds the richer route.** It can export what `gamelist.xml` cannot carry —
  notably the ROM hashes it already computes for RetroAchievements, so a device never has to
  re-hash its library. The import format is a documented contract, not shared code, so the
  two projects stay independent.
- **Write-back is optional and later** (#58, M3): favourites and play stats only, atomically,
  so EmulationStation still sees them when it is the fallback.

Forma still launches through Batocera's own `emulatorlauncher`.

### Staying current

Boot reads the database as it is; a background thread (`forma_core::sync`) then reconciles it
with the disk on its own connection, which WAL lets run alongside the UI's reads. After that it
follows changes:

- A file watcher (inotify on Batocera) says **where** something changed, so only that system's
  folder is rescanned. A change to `es_systems*.cfg` or a new folder in `roms/` triggers a full
  refresh and re-watches.
- Changes are merged until 500 ms of quiet (at most 5 s), so a scraper writing thousands of
  images causes one refresh, not thousands.
- Every refresh reports `changed_systems`; an empty list means the UI has nothing to redraw.
- A full rescan every 5 minutes catches what no watcher reported. Network shares rarely
  report changes made by another machine.

### Artwork

Themes ask for images at the width they show them (`?w=480`). Forma snaps that up to one of a
few sizes (160–1280 px), makes a resized copy on a single background thread (JPEG, or PNG when
the art has transparency), and serves the original until the copy exists. Copies are keyed by
the source's path, size and modification time, capped at 512 MB with the oldest evicted, and
live in `/userdata/system/forma/cache/thumbs`, safe to delete. WebP would be smaller, but the
pure-Rust encoder only writes lossless WebP, which is larger than JPEG for box art.

### Measured

`cargo run --release -p forma-core --example bench_library` generates 10 systems × 2,000 games
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

Gamepads are read in Rust (gilrs, using SDL's controller database) and delivered to the theme as
normalised events, because the Gamepad API in WebKitGTK is not reliable enough to build a sofa
UI on. Each event carries a semantic **action** (`up`, `down`, `confirm`, `back`, `pageUp`,
`start`, `hotkey`…) and the physical **button** by position (`south`, `east`…), so a theme can
follow conventions or draw the right glyph. `confirm`/`back` swap for a Nintendo layout.

The left stick acts as a D-pad, with a press threshold above its release threshold so a stick
resting near the edge does not flicker. Held directions repeat after 400 ms, every 80 ms.
Input pauses while a game runs, since the emulator owns the controllers then. Themes can still
use the browser Gamepad API directly.

## Decisions to revisit

- **Runtime.** Tauri needs WebKitGTK on Linux, which Batocera does not ship. Spike #39 decides
  whether that is viable on x86_64 and RPi4. Findings go in
  [batocera-integration.md](batocera-integration.md).
