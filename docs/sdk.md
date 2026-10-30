# `@romdex/sdk` reference

The SDK is the only way a theme talks to RomDex — the bundled default theme included. Every
call returns a promise; every failure rejects with a `RomDexError` that has a `code` to branch
on and a `message` to show. The types (`Game`, `Browse`, `InputEvent`…) are exported from the
package and documented in [`types.ts`](../packages/sdk/src/types.ts).

## Connecting

```ts
import { connect, RomDexError } from "@romdex/sdk";

const romdex = await connect();
```

`connect()` checks that RomDex speaks a compatible SDK version (it rejects with code
`incompatible` otherwise), then reports the theme as started. A theme that never connects is
replaced by the default theme after 15 seconds.

Outside RomDex, in a plain browser, `connect()` uses a **mock**: the real engine's answers for
the repository's fixture library, with favourites, settings and play stats kept in memory and
placeholder images for media. `romdex.backendKind` says which one you got.

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
const [hit] = await romdex.library.search("super mar");
await romdex.library.setFavorite(hit.id, true);
img.src = hit.media.image ?? "";
```

Media always comes as URLs a page can load, never as paths on disk.

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
const { id } = await romdex.nav.defaultView();
const page = await romdex.nav.browse(id ?? "systems", [], { offset: 0, limit: 50 });
if (page.listing.kind === "groups") {
  for (const group of page.listing.items) {
    const art = group.imageGame ? romdex.library.mediaUrl(group.imageGame) : null;
    render(group.label, group.gameCount, art);
  }
}
const deeper = await romdex.nav.browse(page.view, [...page.path, page.listing.items[0].key]);
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
romdex.input.onPress("down", () => select(+1));
romdex.input.onPress("confirm", () => open(selected));
romdex.input.onPress("back", () => goUp());
```

Each event has an `action` (`up`, `down`, `left`, `right`, `confirm`, `back`, `west`, `north`,
`pageUp`, `pageDown`, `l2`, `r2`, `l3`, `r3`, `start`, `select`, `hotkey`), the physical
`button` (`south`, `east`… or `null` for sticks and keys), `pressed`, and `repeat` for
held directions.

## `events`

```ts
romdex.events.on("library:changed", ({ systems }) => refreshIfShowing(systems));
romdex.events.on("views:changed", () => reloadViewList());
romdex.events.on("game:start", ({ name }) => showLaunching(name));
romdex.events.on("game:end", ({ seconds, error }) => (error ? showError(error) : refresh()));
```

`on` returns a function that unsubscribes.

## `settings`

| Method                                    | Scope                                      |
| ----------------------------------------- | ------------------------------------------ |
| `get(key)`, `set(key, value)`             | The theme's own settings, private to it    |
| `getSystem(key)`, `setSystem(key, value)` | RomDex's settings; needs `settings.system` |

Values are any JSON. `set(key, null)` removes a setting.

## `themes`

| Method       | Does                                                             |
| ------------ | ---------------------------------------------------------------- |
| `list()`     | Every theme found, with `unavailable` saying why one cannot load |
| `select(id)` | Switches to that theme (the window reloads into it)              |

## `system`

`quit()`, `restart()` and `shutdown()` need the `system.power` permission. Restart and
shutdown answer `unsupported` until Batocera integration (Milestone 3).

## Errors

| `code`             | Meaning                                                       |
| ------------------ | ------------------------------------------------------------- |
| `notFound`         | No such game, view, theme or path                             |
| `permissionDenied` | The theme's manifest lacks a permission; the message names it |
| `invalid`          | A malformed request                                           |
| `busy`             | A game is already running                                     |
| `unsupported`      | Not available on this machine yet                             |
| `launch`           | The game could not start; the message says why                |
| `incompatible`     | RomDex and the theme disagree on the SDK version              |
| `internal`         | Anything else                                                 |

## Keeping the mock honest

The mock's data is generated from the engine by
`cargo run -p romdex-core --example mock_snapshot > packages/sdk/src/mock/library.json`. A Rust
test fails if the committed file is stale, and a TypeScript test checks every type here against
it, field by field.
