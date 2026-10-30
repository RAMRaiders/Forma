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

## The WebView spike (#39)

Tauri renders through WebKitGTK on Linux, and Batocera does not ship it. Before Milestone 3
builds on Tauri, this measures whether a self-contained build runs well enough on an x86_64
Batocera box and a Raspberry Pi 4. **Not yet run:** it needs the hardware.

### What to build

The **Spike AppImage** workflow (Actions tab → Run workflow) builds both on GitHub's own x86_64
and arm64 runners and attaches them to the run as artifacts. It runs only when started by hand
and publishes nothing. To build locally instead:

`scripts/spike/build-appimage.sh amd64` (and `arm64`) builds the same AppImage in Docker, with
WebKitGTK and the default theme inside, into `dist-spike/<arch>/`. Nothing in the repository's
Tauri config changes; the bundle settings are passed on the command line. arm64 under QEMU is
slow; building on an arm64 machine (a Pi 4 or 5 with Raspberry Pi OS) is faster.

### A library worth scrolling

```sh
cargo run --release -p romdex-core --example bench_library -- 10 2000 --keep ./spike-library
```

Copy `spike-library/` to the device. Its paths are written as `/userdata/...`, so it works
from wherever `ROMDEX_USERDATA` points.

### On each device

1. Copy the AppImage and `scripts/spike/measure.sh` to `/userdata/`, e.g. with `scp`.
2. Stop EmulationStation, so the two do not compete for the screen and the GPU.
3. From the graphical session (X11 or Wayland, depending on the Batocera version and board), run
   `/userdata/measure.sh /userdata/RomDex-*.AppImage 90 /userdata/spike-library/userdata`.
4. While it runs: open a system with 2,000 games, hold down for ten seconds, page with the
   shoulder buttons, open a game and let its video preview play. Note the overlay's figures.
5. Repeat with the fixture library (small) for a baseline, and restart EmulationStation.

`measure.sh` prints the start-up time (process start to the theme reporting ready) and the
peak and settled memory of RomDex plus its WebKit processes.

Things to watch for and note: the AppImage refusing to start (missing libraries, no FUSE; the
script sets `APPIMAGE_EXTRACT_AND_RUN=1`), a black window (GPU or compositor), and whether
gamepad input arrives.

### Results

| Measure                                   | x86_64 | Raspberry Pi 4 | Target   |
| ----------------------------------------- | ------ | -------------- | -------- |
| Batocera version, display server          |        |                |          |
| AppImage size                             |        |                | < 150 MB |
| Start-up to ready (fixture library)       |        |                | < 3 s    |
| Start-up to ready (20,000 games)          |        |                | < 5 s    |
| Peak memory, RomDex + WebKit              |        |                | < 400 MB |
| Scrolling a 2,000-game list: fps / p95 ms |        |                | 60 / 17  |
| Video preview plays smoothly              |        |                | yes      |
| Gamepad input arrives                     |        |                | yes      |

The targets are a proposal, set from what a sofa UI on a Pi 4 should feel like; adjust them
before judging.

### Packaging options to compare

| Option                                    | For                                          | Against                                       |
| ----------------------------------------- | -------------------------------------------- | --------------------------------------------- |
| AppImage in `/userdata`                   | No image changes; easy to try and remove     | Large; FUSE or extract-to-run; per-arch build |
| Batocera userdata package (`.pacman`)     | Batocera's own add-on route; updatable       | Still ships WebKitGTK; packaging work         |
| Custom Batocera image (WebKitGTK in-tree) | Smallest, shared libraries, boot integration | Maintaining an image fork                     |

### Go / no-go

- **Go** if the Pi 4 meets the targets with the AppImage: Milestone 3 packages it.
- **Go, slimmer** if it works but is heavy: try a custom image with WebKitGTK built in-tree.
- **No-go** if WebKitGTK cannot render acceptably on the Pi 4: fall back to a Chromium-based
  runtime (CEF or Electron, as `batocode-arcade` already runs on Batocera) behind the same
  `romdex-core` and SDK. Themes would not change; only `romdex-app` would.
