# Batocera integration

> Design only. Nothing here is packaged or deployed yet; that is Milestone 3.

## What RomDex reads

| Batocera file                                                | Used for                               |
| ------------------------------------------------------------ | -------------------------------------- |
| `/usr/share/emulationstation/es_systems.cfg`                 | System definitions                     |
| `/userdata/system/configs/emulationstation/es_systems_*.cfg` | User system overlays                   |
| `/userdata/roms/<system>/gamelist.xml`                       | Game metadata and media paths          |
| `/userdata/system/batocera.conf`                             | RetroAchievements and netplay settings |

For development, `ROMDEX_USERDATA` and `ROMDEX_SYSTEMS_CFG` point these at
`fixtures/userdata`.

## Launching

Games launch with the system's own `<command>` template from `es_systems.cfg`, as
EmulationStation does, so emulator choice, per-game settings and RetroArch achievements are
still decided by Batocera's configgen. RomDex fills the same placeholders:

| Placeholder           | Becomes                                                |
| --------------------- | ------------------------------------------------------ |
| `%ROM%`, `%ROM_RAW%`  | Full path of the game file                             |
| `%BASENAME%`          | File name without extension                            |
| `%SYSTEM%`            | System id, e.g. `nes`                                  |
| `%SYSTEMNAME%`        | System full name                                       |
| `%GAMEINFOXML%`       | A gamelist-style file with the game's metadata         |
| `%HOME%`              | `/userdata/system`                                     |
| `%CONTROLLERSCONFIG%` | `-pNindex`, `-pNguid`, `-pNname` per connected gamepad |

Two deliberate differences from EmulationStation:

- **No shell.** The template is split into arguments first and each placeholder fills its own
  argument, so a ROM named `$(rm -rf ~).nes` is just a file name.
- **Overrides and netplay are appended** as `-emulator`, `-core`, and
  `-netplaymode host|client -netplayip … -netplayport …`.

The emulator's output goes to `/userdata/system/romdex/run/launch.log`. After a session RomDex
records play count, last played and time played as the user's own data; a launch that fails
within 10 seconds is not counted.

**Not yet matched:** EmulationStation also passes each pad's button, hat and axis counts and
its `/dev/input` path. gilrs does not expose them; configgen matches controllers by GUID and
name, which RomDex does pass. Whether that is enough on real hardware is part of the spike
(#39).

For development without Batocera, `ROMDEX_LAUNCHER=mock` logs the command it would run and
"plays" for two seconds.

## RetroAchievements and netplay

RetroAchievements unlocking stays in RetroArch, set up by Batocera from `batocera.conf`. RomDex
only shows progress, through the Web API, which needs the user's **web API key**: Batocera stores
the account password for RetroArch, not that key. It is the RomDex setting `ra.apiKey`; the
username comes from `global.retroachievements.username` unless `ra.username` overrides it.

The netplay lobby is read from `lobby.libretro.com`. Answers from both are cached under
`/userdata/system/romdex/cache/`, and served marked stale when offline.

## Open question: the WebView runtime

Tauri renders through WebKitGTK on Linux, and Batocera does not ship it. Spike #39 measures a
bundled build on x86_64 and RPi4 (RAM, cold start, scrolling) and recommends a packaging
route — or a different runtime if WebKitGTK is not viable. Results will be recorded here.
