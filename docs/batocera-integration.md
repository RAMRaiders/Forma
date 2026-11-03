# Batocera integration

> Design only. Nothing here is packaged or deployed yet; that is Milestone 3.

## Installing on Batocera

On the box (e.g. over SSH), with a Forma AppImage built by the **Spike AppImage** workflow and
this repository's `scripts/batocera/`:

```sh
./install.sh /userdata/Forma_0.0.0_aarch64.AppImage   # extract, install the service, enable it
batocera-services start forma                          # take over now, or reboot
```

- The AppImage is **extracted once** to `/userdata/system/forma/app`. Batocera has no FUSE 2,
  and running the AppImage directly unpacks ~260 MB into `/tmp` (RAM) at every start.
- `forma` is a **Batocera user service** (`/userdata/system/services/forma`), the supported way
  to run something at boot; Batocera's own menu lists it. It starts after EmulationStation, so
  anything failing before Forma takes over leaves EmulationStation on screen.
- `forma-standalone` replaces EmulationStation's restart loop with Forma's in the same labwc
  session, and gives the screen back to EmulationStation when Forma quits (the menu's
  "Switch to EmulationStation") or fails three times in two minutes. Never becoming ready
  within 30 seconds counts as failing.
- Restart and Shut down run `shutdown -r now` / `shutdown -P -h now`, where EmulationStation's
  path ends. `batocera-es-swissknife --reboot` would do nothing: it only acts while
  EmulationStation is running.
- The log is `/userdata/system/logs/forma.log`.
- `./uninstall.sh` disables the service and removes the program; `--purge` removes Forma's data
  too. Gamelists are never written; network settings are, as below.

### What the AppImage carries

Batocera has no WebKitGTK, so Forma brings the runtime Tauri needs: the AppImage bundles
WebKitGTK 4.1 and JavaScriptCore with the GTK libraries they need, and GStreamer with its base
and good plugins for video previews and the start-up animation. The libraries come from the
build machine's Ubuntu 22.04 packages (`ubuntu-22.04-arm` for the Pi).

| Measured on the Pi 5 (aarch64)       | Size   |
| ------------------------------------ | ------ |
| The AppImage                         | 130 MB |
| Unpacked in `/userdata/system/forma` | 382 MB |
| WebKitGTK and JavaScriptCore         | 115 MB |
| GStreamer plugins                    | 19 MB  |

**Licences.** WebKitGTK and JavaScriptCore are LGPL-2+ and BSD-2-Clause, with parts under
Apache-2.0 and AFL-2.0; GStreamer and its plugins are LGPL-2+ and LGPL-2.1+. They are
dynamically linked, unmodified, and each package's Debian `copyright` file ships in the
AppImage's `usr/share/doc`, so the LGPL's terms (the user can replace the libraries) are met by
the AppImage's layout itself.

## What Forma reads

| Batocera file                                                | Used for                               |
| ------------------------------------------------------------ | -------------------------------------- |
| `/usr/share/emulationstation/es_systems.cfg`                 | System definitions                     |
| `/userdata/system/configs/emulationstation/es_systems_*.cfg` | User system overlays                   |
| `/userdata/roms/<system>/gamelist.xml`                       | Game metadata and media paths          |
| `/userdata/system/batocera.conf`                             | RetroAchievements and netplay settings |

For development, `FORMA_USERDATA` and `FORMA_SYSTEMS_CFG` point these at
`fixtures/userdata`.

## RetroAchievements and netplay settings

The menus change the same `batocera.conf` keys as EmulationStation's, through
`batocera-settings-set`: `global.retroachievements.*` and `global.netplay.*`, and nothing else.
Logging in calls RetroAchievements' `dorequest.php?r=login`, as ES does, and saves the account
with its token (the emulators use the token; RetroArch clears the password). The netplay relay is
one of `none`, `nyc`, `madrid`, `montreal`, `saopaulo` or `custom` (with
`global.netplay.customserver`).

## Updates

System settings → Batocera updates (`forma.updates` in the SDK, `system.update` permission)
does what EmulationStation's Updates menu does:

- **Check:** `https://updates.batocera.org/<board>/<channel>/last/batocera.version` against
  `/usr/share/batocera/batocera.version`. The board is `/boot/boot/batocera.board` (`rpi4`
  downloads `rpi464`, as `batocera-upgrade` does), the channel `updates.type`, the server
  `updates.url` when set.
- **Install:** `batocera-upgrade`, only after the user confirms on a second page. Its
  `… >>> 13%` lines become progress. The box restarts when the user chooses.

## Controllers

The menu's Controllers screen (`forma.bluetooth` in the SDK, `system.controllers` permission)
shows the pads in use by player and drives Batocera's `batocera-bluetooth`, so EmulationStation
sees the same pairings:

| Change          | How                                                                                      |
| --------------- | ---------------------------------------------------------------------------------------- |
| Paired devices  | `batocera-bluetooth list`: trusted devices from `/var/lib/bluetooth`                     |
| Pair            | `batocera-bluetooth trust input`: Batocera's agent looks for a minute, trusting each pad |
| Connect, forget | `batocera-bluetooth connect`, `remove` (saved to `/userdata/system/bluetooth`)           |
| On, off         | `batocera-bluetooth enable`/`disable`: `controllers.bluetooth.enabled` and its services  |

## Network settings

The menu's Network screen (`forma.network` in the SDK, `system.network` permission) changes the
box's connections the way Batocera does, so EmulationStation sees the same settings:

| Change                 | How                                                                                                                             |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Join a Wi-Fi network   | `batocera-wifi enable <ssid> <key> <country>`: saved in `batocera.conf`                                                         |
| Wi-Fi off              | `wifi.enabled=0`, then `S08connman reload` (`batocera-wifi disable` waits for every address to go, ten seconds with a cable in) |
| Static Ethernet / DHCP | Forma's own `[service_forma_ethernet]` section in `/userdata/system/network-connman.config`, then `S08connman restart`          |

- `/var` is in memory, so `connmanctl config` alone is forgotten at boot. The provisioning file
  is linked into `/var/lib/connman` by Batocera and read when connman starts; Forma rewrites
  only its own section of it.
- Batocera runs connman with `SingleConnectedTechnology` and Ethernet preferred. With a cable
  in, joining Wi-Fi saves the network and the box switches to it once the cable is unplugged.
- 802.1X (enterprise) networks cannot be joined: `batocera-wifi` has no way to set them up.
- Status and scans come from `connmanctl technologies`, `services` and `services <id>`.

## Launching

Games launch with the system's own `<command>` template from `es_systems.cfg`, as
EmulationStation does, so emulator choice, per-game settings and RetroArch achievements are
still decided by Batocera's configgen. Forma fills the same placeholders:

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

The emulator's output goes to `/userdata/system/forma/run/launch.log`. After a session Forma
records play count, last played and time played as the user's own data; a launch that fails
within 10 seconds is not counted.

**Not yet matched:** EmulationStation also passes each pad's button, hat and axis counts and
its `/dev/input` path. gilrs does not expose them; configgen matches controllers by GUID and
name, which Forma does pass. Whether that is enough on real hardware is part of the spike
(#39).

For development without Batocera, `FORMA_LAUNCHER=mock` logs the command it would run and
"plays" for two seconds.

## RetroAchievements and netplay

RetroAchievements unlocking stays in RetroArch, set up by Batocera from `batocera.conf`. Forma
only shows progress, through the Web API, which needs the user's **web API key**: Batocera stores
the account password for RetroArch, not that key. It is the Forma setting `ra.apiKey`; the
username comes from `global.retroachievements.username` unless `ra.username` overrides it.

**Matching games (#41).** Forma links [romdex](https://github.com/RAMRaiders/romdex), vendored in
`vendor/romdex` at the commit in `PINNED` (`scripts/pin-romdex.sh` updates it), which hashes ROMs
with rcheevos' own code, archives and discs included. After start-up, a background thread hashes
one system at a time, waiting while a game runs; its cache (`romdex.sqlite` in
`/userdata/system/forma`) keeps path, size and time, so after the first pass only changed files
are read (11 minutes for 12,000 cartridge and arcade games on a Pi 5 the first time, 2 seconds
after). Hashes are matched through `API_GetGameList.php?i=<console>&h=1&f=1`, once per console a
week, when the web API key is set. CHD discs are not hashed yet (romdex R2).

**Netplay (#43, #44).** Forma launches through Batocera's launcher as EmulationStation does:
`-netplaymode host|client|spectator`, `-netplayip`, `-netplayport`, `-netplaypass` and, for a room
behind a relay (`host_method` 3), the relay's address and `-netplaysession`. Joining picks the
installed core whose `.info` `corename` (in `/usr/share/libretro/info`) is the room's core name,
with `-emulator libretro`. Nickname, relay and port stay Batocera's `global.netplay.*`. Rooms on
the local network are found with RetroArch's own discovery: a `RANQ` broadcast to UDP 55435,
answered by every host not behind a relay with its `ad_packet` (`netplay_frontend.c`, RetroArch
1.22).

The netplay lobby is read from `lobby.libretro.com`. Answers from both are cached under
`/userdata/system/forma/cache/`, and served marked stale when offline.

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
cargo run --release -p forma-core --example bench_library -- 10 2000 --keep ./spike-library
```

Copy `spike-library/` to the device. Its paths are written as `/userdata/...`, so it works
from wherever `FORMA_USERDATA` points.

### On each device

1. Copy the AppImage and `scripts/spike/measure.sh` to `/userdata/`, e.g. with `scp`.
2. Stop EmulationStation, so the two do not compete for the screen and the GPU.
3. From the graphical session (X11 or Wayland, depending on the Batocera version and board), run
   `/userdata/measure.sh /userdata/Forma-*.AppImage 90 /userdata/spike-library/userdata`.
4. While it runs: open a system with 2,000 games, hold down for ten seconds, page with the
   shoulder buttons, open a game and let its video preview play. Note the overlay's figures.
5. Repeat with the fixture library (small) for a baseline, and restart EmulationStation.

`measure.sh` prints the start-up time (process start to the theme reporting ready) and the
peak and settled memory of Forma plus its WebKit processes.

Things to watch for and note: the AppImage refusing to start (missing libraries, no FUSE; the
script sets `APPIMAGE_EXTRACT_AND_RUN=1`), a black window (GPU or compositor), and whether
gamepad input arrives.

### Results

Run on 2026-10-31 over SSH, driven by `wtype` (real key events through the compositor) and read
back with `grim` screenshots and the theme's own frame statistics. EmulationStation was left
running underneath. The x86_64 box has not been measured yet.

| Measure                                     | x86_64 | Raspberry Pi 5 (4 GB)                                                                                                  | Target   |
| ------------------------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------- | -------- |
| Batocera version, display server            |        | 43.1, Wayland (labwc)                                                                                                  |          |
| AppImage size                               |        | 82 MB; 259 MB extracted                                                                                                | < 150 MB |
| Launch to ready, AppImage (unpacks to /tmp) |        | 2.5 s                                                                                                                  | < 3 s    |
| Launch to ready, extracted, cold / warm     |        | 2.6 s / 0.55 s                                                                                                         | < 3 s    |
| Inside Forma: process start to ready        |        | 0.4–0.6 s warm, 1.4 s cold                                                                                             |          |
| First import of 20,000 games                |        | done before the first screenshot (< 20 s)                                                                              |          |
| Memory, Forma + WebKit (PSS / RSS)          |        | 234 MiB / 345 MiB                                                                                                      | < 400 MB |
| Idle on the systems grid                    |        | 60 fps, p95 18 ms                                                                                                      | 60 / 17  |
| Holding down in a 2,000-game list           |        | 20 fps, p95 66 ms → **57–60 fps, p95 19–27 ms** after the fix                                                          | 60 / 17  |
| Paging (shoulder button)                    |        | 58 fps, p95 25 ms                                                                                                      | 60 / 17  |
| Video preview plays smoothly                |        | crashed WebKit's page process (no GStreamer playback plugins in the AppImage); `bundleMediaFramework` added, to retest | yes      |
| Gamepad input arrives                       |        | not tested: needs someone holding a pad                                                                                | yes      |

What it found:

- **WebKitGTK works on Batocera 43 on a Pi 5**, under Wayland, with the GPU: rendering, layout
  and key input through the compositor all behaved.
- **Holding a direction was the one real problem.** The game list rebuilt its detail panel and
  loaded its art on every step, 20 times a second. The panel now waits for movement to settle
  (it still updates at once on a single press): 20 fps became 57–60.
- **Do not ship it as a self-unpacking AppImage.** Batocera has FUSE 3 only, so the AppImage
  unpacks itself on every start into `/tmp`, which is RAM: 259 MB gone before Forma starts.
  Extracted once onto `/userdata`, it costs nothing in RAM and starts in 0.55 s warm.
- On a cold cache most of the start is reading 259 MB of libraries from `/userdata` (an
  external disk here); a smaller bundle, or WebKitGTK in the image, would cut it.
- The Pi 5 is the stronger board; a Pi 4 run would still be worth doing before calling the
  targets met for both.

**Recommendation: go, with packaging changed.** Ship the extracted bundle in `/userdata` (or a
Batocera package that installs it there), not the AppImage. Re-measure on x86_64, on a Pi 4 if
one is at hand, and with a gamepad and real video before Milestone 3 closes.

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
  `forma-core` and SDK. Themes would not change; only `forma-app` would.
