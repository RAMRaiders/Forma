# `@ramraiders/forma-sdk` reference

The SDK is the only way a theme talks to Forma — the bundled default theme included. Every
call returns a promise; every failure rejects with a `FormaError` that has a `code` to branch
on and a `message` to show. The types (`Game`, `Browse`, `InputEvent`…) are exported from the
package and documented in [`types.ts`](../packages/sdk/src/types.ts).

## Installing

The SDK is published to GitHub Packages. While it is private, a theme repo needs an `.npmrc`
that sends the `@ramraiders` scope there, with a token that can read packages (a classic
personal access token with `read:packages`, kept out of the repo):

```ini
@ramraiders:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_TOKEN}
```

```sh
npm install @ramraiders/forma-sdk
```

`@ramraiders/forma-sdk/mock` exports `mockBackend` for tests:
`connect({ backend: mockBackend({ permissions: ['system.network'] }) })`.

The package version follows the API version (`SDK_API_VERSION` in `forma-core`), so a theme's
`"sdk": "^1"` and its dependency range say the same thing. Publishing is the manual
**Publish SDK** workflow; a version can only be published once, so a change raises the version
first.

## Connecting

```ts
import { connect, FormaError } from "@ramraiders/forma-sdk";

const forma = await connect();
```

`connect()` checks that Forma speaks a compatible SDK version (it rejects with code
`incompatible` otherwise), then reports the theme as started, which lifts Forma's cover. A theme
that never reports is replaced by another theme after 30 seconds (see theme-authoring.md,
Starting up). With `deferReady`, the
theme reports itself, with `forma.ready()`, e.g. once its first screen is drawn.

Outside Forma, in a plain browser, `connect()` uses a **mock**: the real engine's answers for
the repository's fixture library, with favourites, settings and play stats kept in memory and
placeholder images for media. `forma.backendKind` says which one you got.

| Option       | Default          | Meaning                                            |
| ------------ | ---------------- | -------------------------------------------------- |
| `keyboard`   | `true`           | Deliver arrows, Enter, Escape… as input events too |
| `deferReady` | `false`          | Leave reporting started to `forma.ready()`         |
| `mock`       | `{}`             | `{ playMs, permissions }` for the mock             |
| `backend`    | Tauri, else mock | A custom backend, for tests                        |

## `library`

| Method                      | Returns                                                       |
| --------------------------- | ------------------------------------------------------------- |
| `systems()`                 | Systems with visible games, in Batocera's order               |
| `game(id)`                  | One game, or `null`                                           |
| `search(query, limit = 50)` | Games matching every word as a prefix, accents ignored        |
| `setFavorite(id, favorite)` | Records the user's choice; emits `library:changed`            |
| `mediaUrl(gameId, kind)`    | A URL for a game's `image`, `video`, `marquee`… (synchronous) |

```ts
const [hit] = await forma.library.search("super mar");
await forma.library.setFavorite(hit.id, true);
img.src = hit.media.image ?? "";
```

Media always comes as URLs a page can load, never as paths on disk. Pass `width` for images
shown small, e.g. `mediaUrl(id, 'image', 480)` for a card: Forma serves a resized copy, made
in the background and cached, and the original until it is ready. Full-size box art on every
card is the easiest way to make a Raspberry Pi stutter.

## `nav`

Browsing [hierarchy views](hierarchy.md). A **path** is the group keys (or branch ids) opened so
far, from the root down.

| Method                       | Returns                                                   |
| ---------------------------- | --------------------------------------------------------- |
| `views()`                    | Every view: bundled and the user's                        |
| `browse(view, path?, page?)` | One page: `groups`, `branches` or `games`, plus a `trail` |
| `defaultView()`              | The boot view, and notes on any choice passed over        |
| `setDefaultView(id \| null)` | Makes `id` the boot view; `null` clears it                |

```ts
const { id } = await forma.nav.defaultView();
const page = await forma.nav.browse(id ?? "systems", [], { offset: 0, limit: 50 });
if (page.listing.kind === "groups") {
  for (const group of page.listing.items) {
    const art = group.imageGame ? forma.library.mediaUrl(group.imageGame) : null;
    render(group.label, group.gameCount, art);
  }
}
const deeper = await forma.nav.browse(page.view, [...page.path, page.listing.items[0].key]);
```

## `launch`

| Method               | Returns                                                   |
| -------------------- | --------------------------------------------------------- |
| `game(id, request?)` | Resolves once started; `game:start` and `game:end` follow |

`request` may override the emulator or core, or start netplay:
`{ netplay: { mode: 'host', password? } }`,
`{ netplay: { mode: 'client', host, port, password?, session?, spectate? } }`; `netplay.join()`
and `host()` build these. While a game
runs the window is hidden and input is paused. A second launch meanwhile rejects with `busy`.

## `input`

| Method                     | Does                                                          |
| -------------------------- | ------------------------------------------------------------- |
| `on(handler)`              | Every input event, gamepads and keyboard; returns unsubscribe |
| `onPress(action, handler)` | Presses and repeats of one action                             |
| `controllers()`            | Connected gamepads                                            |

```ts
forma.input.onPress("down", () => select(+1));
forma.input.onPress("confirm", () => open(selected));
forma.input.onPress("back", () => goUp());
```

Each event has an `action` (`up`, `down`, `left`, `right`, `confirm`, `back`, `west`, `north`,
`pageUp`, `pageDown`, `l2`, `r2`, `l3`, `r3`, `start`, `select`, `hotkey`), the physical
`button` (`south`, `east`… or `null` for sticks and keys), `pressed`, and `repeat` for
held directions.

Start and Select (F1 and Tab on a keyboard) are Forma's: they open its menu and views list, and
never reach the theme. While one of Forma's menus is open, no input reaches the theme.

## `shell`

Forma's own menus, over every theme (see [Theme authoring](theme-authoring.md)).

| Method                | Does                                                                     |
| --------------------- | ------------------------------------------------------------------------ |
| `setContext(context)` | Tells the menu what is on screen: `{ view, system, game }`, each or null |

`system` is `{ name, fullname }`, `game` is `{ id, name }`. The menu offers options for them
(Start → "Super Metroid options…") and marks `view` in the views list.

## `events`

```ts
forma.events.on("library:changed", ({ systems }) => refreshIfShowing(systems));
forma.events.on("views:changed", () => reloadViewList());
forma.events.on("game:start", ({ name }) => showLaunching(name));
forma.events.on("game:end", ({ seconds, error }) => (error ? showError(error) : refresh()));
// A few seconds later, if the session earned any RetroAchievements:
forma.events.on("ra:unlocked", ({ game, unlocked, points }) => celebrate(game, unlocked, points));
forma.events.on("store:progress", ({ name, stage, percent }) => showProgress(name, stage, percent));
forma.events.on("nav:open", ({ view }) => open(view)); // picked in Forma's views list
forma.events.on("settings:changed", ({ scope, key, value }) => apply(scope, key, value));
forma.events.on("music:song", ({ title, artist, seconds }) => showSong(title, artist, seconds));
```

`music:song` comes when a new song starts (not on a resume): its title without the file's
extension, its artist (from its tags, else from an `Artist - Title` file name), and how long to
show it, Forma's setting (Start → Music → Show each new song, 8 seconds unless changed; at 0 the
event never comes). Forma's shell shows it; a theme that wants its own look either restyles the
shell's (below) or draws it itself, declaring `"draws": ["nowPlaying"]` in `theme.json`.

`settings:changed` follows a change made in Forma's menu: one of the theme's own settings
(`scope: "theme"`, declared in `theme.json`) or one of Forma's (`scope: "system"`, e.g.
`display.margins`).

`on` returns a function that unsubscribes.

## `status`

```ts
const { controllers, network } = await forma.status.get();
forma.events.on("status:changed", (status) => redraw(status));
```

What Forma shows in the top-right corner: each controller connected, in player order, with its
`kind` (`xbox`, `playstation`, `nintendo` or `generic`, from its maker or its name), and the
connection (`kind`: `ethernet`, `wifi` or `none`; `internet`: whether the internet answers, by
connman's own check). `network` is `null` off a Batocera box. No permission: it holds no address
and no network name. Forma's shell draws it; a theme restyles it or draws its own (see
[theme authoring](theme-authoring.md)).

## `settings`

| Method                                    | Scope                                     |
| ----------------------------------------- | ----------------------------------------- |
| `get(key)`, `set(key, value)`             | The theme's own settings, private to it   |
| `getSystem(key)`, `setSystem(key, value)` | Forma's settings; needs `settings.system` |

Values are any JSON. `set(key, null)` removes a setting.

## `themes`

| Method       | Does                                                             |
| ------------ | ---------------------------------------------------------------- |
| `list()`     | Every theme found, with `unavailable` saying why one cannot load |
| `select(id)` | Switches to that theme (the window reloads into it)              |

## `ra`

RetroAchievements, read-only: unlocking happens inside RetroArch, which Batocera already logs in.

| Method      | Returns                                                       |
| ----------- | ------------------------------------------------------------- |
| `user()`    | The user's profile, points, rank and recently played games    |
| `game(id)`  | A local game's achievements and progress (`id` is Forma's id) |
| `matches()` | Per system: games, hashed, matched to RetroAchievements       |

```ts
const { data, stale } = await forma.ra.game(game.id);
for (const a of data.achievements) {
  badge(a.badge, a.title, a.earnedAt !== null);
}
if (stale) note("Offline: showing the last known progress");
```

Both answer with `{ data, stale, fetchedAt }`: offline, Forma serves the last answer it got,
with `stale: true`. Badge, icon and avatar URLs are served by Forma itself, so a theme needs no
`network` permission to show them.

They need the user's **web API key** (retroachievements.org → Settings → Keys) in the Forma
setting `ra.apiKey`; without it they reject with `notConfigured` and a message saying so. The
username defaults to the one Batocera logs in with. `game()` rejects with `notFound` when no
RetroAchievements id is known for the game yet.

Forma finds the ids itself: in the background it hashes every game on a system RetroAchievements
supports, as rcheevos does, and looks each hash up in RetroAchievements' list of that console's
games. Hashing reads a file again only when it changes, never runs while a game does, and the
lists are cached for a week. Ids and hashes from an import file win over these. `matches()`
says how far it got (`running` while it works); the mock reports three systems.

### Logging in

`ra.login(username, password)` checks the account with RetroAchievements and saves it in
Batocera's settings with the session token the emulators use, as EmulationStation does; it
resolves with the user name as RetroAchievements spells it, and rejects with `invalid` (saving
nothing) when the login is refused. `ra.logout()` forgets it. Both need `settings.batocera`.

## `batocera`

Batocera's RetroAchievements and netplay settings (`global.retroachievements.*`,
`global.netplay.*` in `batocera.conf`), as EmulationStation's menus set them. Needs the
`settings.batocera` permission; Batocera only.

| Method              | Returns                                                                      |
| ------------------- | ---------------------------------------------------------------------------- |
| `settings(prefix?)` | `{ key, value, kind, set, choices }` for each setting starting with `prefix` |
| `set(key, value)`   | Sets one (an empty string clears it), checked against its kind               |

`kind` is `bool` (`'0'`/`'1'`), `text`, `port`, `choice` (one of `choices`, e.g. the netplay
relays or the unlock sounds) or `secret`. A secret's `value` is always `null`: a theme only learns
whether one is `set`, and sets it through `ra.login`.

## `store`

Batocera's content downloader: free games, ports, decorations (bezels) and music from Batocera's
package store, as EmulationStation offers them. Through `batocera-store`, so a package installed
from Forma is installed for EmulationStation too; games land in the ROM folders and show up in the
library by themselves. EmulationStation themes are left out. Needs the `store.manage`
permission; Batocera only.

| Method          | Returns                                                                               |
| --------------- | ------------------------------------------------------------------------------------- |
| `list()`        | Every package: `name`, `description`, `group`, `system`, sizes, `installed`, `update` |
| `refresh()`     | Reads the catalogue again from Batocera's server                                      |
| `install(name)` | Resolves once installed; `store:progress` reports `download`, then `install`          |
| `remove(name)`  | Uninstalls it                                                                         |
| `update()`      | Updates every installed package that has a newer version                              |

`group` is `sys-<system>` for a system's games (`system` then names it), or `bezel`, `music`,
`ports`, `misc`. Sizes are in bytes. One operation runs at a time: another is refused as
`unsupported` until it ends.

## `options`

Batocera's emulator, core and game options: what EmulationStation shows in its game settings
(global), a system's advanced options and a game's. Read from `es_features.cfg` and
`es_systems.cfg`, saved in `batocera.conf` (`global.<key>`, `<system>.<key>`,
`<system>["<file>"].<key>`), and used by the next launch. Needs the `settings.emulators`
permission; Batocera only.

| Method                   | Returns                                                          |
| ------------------------ | ---------------------------------------------------------------- |
| `get(scope)`             | `{ options, flags }`: the options at that level                  |
| `set(scope, key, value)` | Sets one, checked against its choices or range; `null` unsets it |

`scope` is `{ level: 'global' }`, `{ level: 'system', system: 'snes' }` or
`{ level: 'game', game: 42 }`. A game's value overrides its system's, which overrides the
global one; each option says what is set at this level (`value`) and, when nothing is, what it
inherits and from where (`inherited`).

A system or game page starts with `emulator` and `core`; the options after them are those of the
chosen core, so get the page again after changing either. Changing the emulator unsets a core set
at the same level. `kind` is `choice` (one of `choices`), `number` (`min`…`max` by `step`) or
`text`. `group` and `submenu` say where ES files an option: a section (`ADVANCED OPTIONS`) or a
page of its own (`DECORATIONS`). `flags` say what the emulator supports (`netplay`, `cheevos`).

## `netplay`

| Method                   | Returns                                                                |
| ------------------------ | ---------------------------------------------------------------------- |
| `lobby(filter?)`         | Open rooms on the libretro lobby, joinable first, as `{ data, stale }` |
| `lan(filter?)`           | RetroArch hosts on the local network (listens for 1.5 s)               |
| `join(room, options?)`   | Starts the local game with the room's core, connected to it            |
| `host(gameId, options?)` | Starts a game others can join                                          |

Each room says what this box needs to join it: `localGameId` (the local game with the same name
or CRC), `core` (the installed core RetroArch calls `coreName`) and `join` (the host, or its relay
with the room's `session`). `join()` rejects when one is missing; `options` are `password` and
`spectate`. `filter` keeps rooms that are `owned`, `joinable`, `open` (no password), of one
`coreName`, or answering within `maxPingMs` (which measures `pingMs`, a TCP connect to the host
or relay, for up to 40 rooms). LAN discovery is RetroArch's own (`RANQ` broadcast to UDP 55435);
hosts behind a relay do not answer it.

Forma's shell offers all of this on its Netplay page: hosting the selected game, rooms online
(those for your games by default) and rooms on this network.

## `network`

The box's own Wi-Fi and Ethernet. Every method needs the `system.network` permission and
answers `unsupported` anywhere but Batocera.

| Method                        | Returns                                                                                |
| ----------------------------- | -------------------------------------------------------------------------------------- |
| `status()`                    | `{ ethernet, wifi }`: connection, address, and what is saved for each                  |
| `wifiScan()`                  | Networks in range, in use and saved first, then by strength (seconds)                  |
| `joinWifi(ssid, passphrase?)` | `'connected'`, or `'saved'` when a cable keeps Ethernet in use                         |
| `setWifiEnabled(on)`          | Turns Wi-Fi on (joining the saved network) or off                                      |
| `configureEthernet(config)`   | `{ method: 'dhcp' }` or `{ method: 'manual', address, netmask, gateway, nameservers }` |

`joinWifi` waits up to 30 seconds. It rejects with `invalid` for a password that cannot be
right (WPA needs 8–63 characters), `network` when the network refuses it or never answers (it
stays saved either way), and `unsupported` for 802.1X networks. `configureEthernet` restarts
connman, so the connection drops for a few seconds; read `status()` again after that. The mock
answers from recorded connman output, has a cable plugged in, and refuses the password
`wrong-password`.

## `updates`

Batocera's own updates, as EmulationStation's Updates menu has them. Both need the
`system.update` permission and answer `unsupported` anywhere but Batocera.

| Method      | Returns                                                                                  |
| ----------- | ---------------------------------------------------------------------------------------- |
| `check()`   | `{ current, latest, channel, available }`, from Batocera's update server for this board  |
| `install()` | Downloads and unpacks the update with `batocera-upgrade`; it takes effect after a reboot |

`install` takes a long while (gigabytes) and never runs unless called: ask the user first.
`updates:progress` carries `{ message, percent }` on the way; `percent` is `null` when Batocera
does not say. A second `install` while one runs is refused as `unsupported`. The mock offers
43.1 → 44.

## `bluetooth`

Bluetooth controllers, through Batocera's own `batocera-bluetooth`, as EmulationStation's
Controllers menu offers them. Every method needs the `system.controllers` permission and
answers `unsupported` anywhere but Batocera.

| Method                | Returns                                                                       |
| --------------------- | ----------------------------------------------------------------------------- |
| `status()`            | `{ enabled, devices }`: each paired device's address, name, whether connected |
| `setEnabled(on)`      | Turns Bluetooth on or off (Batocera restarts its Bluetooth services)          |
| `pair()`              | Looks for a minute for pads in pairing mode; the devices it paired            |
| `connect(address)`    | Connects a paired device that is on                                           |
| `disconnect(address)` | Disconnects it, keeping it paired                                             |
| `forget(address)`     | Unpairs it                                                                    |

While `pair()` looks, `bluetooth:pairing` carries `{ message }`, Batocera's word on what it is
doing. The pads in use, by player, are in `forma.status.get()`. The mock has two pads paired and
finds a third.

## `system`

`quit()`, `restart()` and `shutdown()` need the `system.power` permission. Restart and
shutdown answer `unsupported` outside Batocera.

Batocera's system settings, as EmulationStation's System settings menu offers them, need the
`system.settings` permission; Batocera only.

| Method            | Returns                                                                    |
| ----------------- | -------------------------------------------------------------------------- |
| `info()`          | `{ label, value }` lines: Batocera's version, model, CPU, memory, address… |
| `settings()`      | `{ key, label, section, description, kind, choices, value, applies }` each |
| `set(key, value)` | Sets one, checked against its choices; `null` unsets it (the default)      |

The settings are the video mode, output and rotation; the audio output; the language, time zone
and keyboard layout; the name on the network, security and update checks; and the storage
device. `applies` says when a change takes effect: `now`, `restart` (Forma's), or `reboot`
(Batocera applies display and name settings as it starts). Choices come from Batocera's own tools,
so they are the box's: its display modes, its audio devices, its disks.

## Errors

| `code`             | Meaning                                                       |
| ------------------ | ------------------------------------------------------------- |
| `notFound`         | No such game, view, theme or path                             |
| `permissionDenied` | The theme's manifest lacks a permission; the message names it |
| `invalid`          | A malformed request                                           |
| `busy`             | A game is already running                                     |
| `unsupported`      | Not available on this machine yet                             |
| `launch`           | The game could not start; the message says why                |
| `network`          | A service or network could not be reached or refused          |
| `incompatible`     | Forma and the theme disagree on the SDK version               |
| `internal`         | Anything else                                                 |

## Keeping the mock honest

The mock's data is generated from the engine by
`cargo run -p forma-core --example mock_snapshot > packages/sdk/src/mock/library.json`. A Rust
test fails if the committed file is stale, and a TypeScript test checks every type here against
it, field by field.
