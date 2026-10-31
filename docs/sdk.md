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
`incompatible` otherwise), then reports the theme as started. A theme that never connects is
replaced by the default theme after 15 seconds.

Outside Forma, in a plain browser, `connect()` uses a **mock**: the real engine's answers for
the repository's fixture library, with favourites, settings and play stats kept in memory and
placeholder images for media. `forma.backendKind` says which one you got.

| Option     | Default          | Meaning                                            |
| ---------- | ---------------- | -------------------------------------------------- |
| `keyboard` | `true`           | Deliver arrows, Enter, Escape… as input events too |
| `mock`     | `{}`             | `{ playMs, permissions }` for the mock             |
| `backend`  | Tauri, else mock | A custom backend, for tests                        |

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
`{ netplay: { mode: 'host' } }`, `{ netplay: { mode: 'client', host, port } }`. While a game
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

## `events`

```ts
forma.events.on("library:changed", ({ systems }) => refreshIfShowing(systems));
forma.events.on("views:changed", () => reloadViewList());
forma.events.on("game:start", ({ name }) => showLaunching(name));
forma.events.on("game:end", ({ seconds, error }) => (error ? showError(error) : refresh()));
```

`on` returns a function that unsubscribes.

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

| Method     | Returns                                                       |
| ---------- | ------------------------------------------------------------- |
| `user()`   | The user's profile, points, rank and recently played games    |
| `game(id)` | A local game's achievements and progress (`id` is Forma's id) |

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
RetroAchievements id is known for the game yet (matching by hash arrives in Milestone 2).

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

| Method    | Returns                                                                |
| --------- | ---------------------------------------------------------------------- |
| `lobby()` | Open rooms on the libretro lobby, joinable first, as `{ data, stale }` |

Each room has `localGameId` when a local game has the same name, so a theme can offer to join
with `launch.game(room.localGameId, { netplay: { mode: 'client', host: room.host, port: room.port } })`.
Hosting and joining from the default theme arrive in Milestone 2.

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

## `system`

`quit()`, `restart()` and `shutdown()` need the `system.power` permission. Restart and
shutdown answer `unsupported` outside Batocera.

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
