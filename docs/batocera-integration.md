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

Games launch through `emulatorlauncher` with the same arguments EmulationStation passes, so
emulator choice, per-game settings and RetroArch achievements behave exactly as before.

## Open question: the WebView runtime

Tauri renders through WebKitGTK on Linux, and Batocera does not ship it. Spike #39 measures a
bundled build on x86_64 and RPi4 (RAM, cold start, scrolling) and recommends a packaging
route — or a different runtime if WebKitGTK is not viable. Results will be recorded here.
