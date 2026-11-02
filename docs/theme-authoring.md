# Theme authoring

A Forma theme is a folder containing a `theme.json` manifest and a web app that talks to
Forma through [`@ramraiders/forma-sdk`](sdk.md).

```ts
import { connect } from '@ramraiders/forma-sdk';

const forma = await connect();
const page = await forma.nav.browse((await forma.nav.defaultView()).id ?? 'systems');
forma.input.onPress('confirm', () => …);
```

Open the theme in a plain browser while you work: outside Forma the SDK serves a mock library,
so you can build without a device.

## `theme.json`

```json
{
  "id": "default",
  "name": "Forma Default",
  "version": "0.0.0",
  "entry": "index.html",
  "sdk": "^1",
  "permissions": [],
  "resolutions": ["1280x720", "1920x1080"]
}
```

| Field         | Meaning                                                                 |
| ------------- | ----------------------------------------------------------------------- |
| `id`          | Unique, kebab-case. Also the folder name under the themes directory.    |
| `entry`       | The HTML page Forma loads.                                              |
| `sdk`         | Caret range of the SDK API the theme needs. A mismatch refuses to load. |
| `permissions` | Privileged capabilities the theme uses, e.g. `system.power`.            |
| `resolutions` | Resolutions the theme was designed for. Informational.                  |
| `settings`    | The theme's own settings, shown in Forma's menu (see below).            |

## Forma's menus, over every theme

**Start** and **Select** belong to Forma. Whatever the theme, Start opens Forma's menu (theme,
where Forma opens, screen margins, music, RetroAchievements, netplay, network, the content
downloader, game and system settings, EmulationStation, power) and Select opens its views list.
A theme never receives those two buttons (F1 and Tab on a keyboard), nor any input while one of
Forma's menus is open. So a user can always change theme, even from a theme with no menu at all.

Forma's menus are drawn by its shell, a script Forma puts first in every theme page's `<head>`.
It lives in a closed shadow root at the top of the page: a theme's CSS cannot reach it, and it
cannot reach the theme's. It acts with Forma's own rights, not the theme's: its menus work
whatever the theme's `permissions` say.

A theme works with the menus in three ways:

- **What is on screen.** `forma.shell.setContext({ view, system, game })` as the selection
  moves; the menu then offers options for that game and system, and the views list marks the
  view shown.
- **Views.** When the user picks a view in Forma's list, the theme receives `nav:open` with its
  id and shows it. Forma keeps which view to open with (`nav.defaultView()`).
- **Its own settings.** Declare them in `theme.json`; Forma's menu shows them under Interface,
  stores them in the theme's private settings (`forma.settings.get(key)`), and tells the theme
  of a change with `settings:changed`:

```json
"settings": [
  {
    "key": "board",
    "label": "Circuit board",
    "description": "The board behind the screens",
    "group": "Look",
    "type": "choice",
    "choices": [
      { "value": "on", "label": "On" },
      { "value": "off", "label": "Off" }
    ],
    "default": "on"
  },
  { "key": "motion", "label": "Animations", "group": "Look", "type": "toggle", "default": true }
]
```

`type` is `toggle` (`true`/`false`) or `choice` (one of `choices`, by `value`). A setting whose
`default` it cannot take, an empty `choices` or a key listed twice keeps the theme from loading,
with the reason in Forma's theme list.

## Where themes live, and installing your own

- Bundled themes ship with Forma.
- User themes go in `/userdata/themes-forma/<id>/`. Installing a theme is copying its folder
  there; Forma lists it and lets you switch to it. A theme with a bad manifest or an
  incompatible `sdk` range is listed as unavailable, with the reason, and never loaded.
- An installer (from a `.zip` or URL, with update and remove) comes in Milestone 4 (#52). It
  shows the permissions a theme asks for before installing it.
- A theme may ship its own hierarchy views and suggest one to boot into; your own boot view
  setting still wins (see [hierarchy.md](hierarchy.md)).

## How a theme is served

Forma serves the selected theme from `forma://localhost/theme/<id>/`, straight from its
folder. Only files inside that folder are reachable: `..`, absolute paths and symlinks leading
out of it are refused.

Game media never comes as a path on disk. The SDK hands out URLs such as
`forma://localhost/media/<game id>/image` (also `video`, `marquee`, `thumbnail`, `fanart`,
`manual`); videos support seeking.

On Linux (Batocera), a `<video>` or `<audio>` cannot play a `forma://` URL directly: WebKitGTK's
media player (GStreamer) loads only http(s) and blob URLs, and reports anything else as an
unsupported format. Fetch the file and play it from a blob, freeing it when done:

```ts
const blob = await (await fetch(game.media.video, { signal })).blob();
video.src = URL.createObjectURL(blob); // later: URL.revokeObjectURL(video.src)
```

The theme CSP allows `blob:` for media. Images are not affected.

### Content Security Policy

Every HTML page gets a CSP that allows the theme's own folder (`forma:`), Tauri's IPC, inline
styles, and `data:`/`blob:` images and media. Nothing else loads, unless the manifest lists it:

```json
{ "permissions": ["network"], "remote": ["https://fonts.example.com"] }
```

Remote origins must be `https://` and need the `network` permission.

## Permissions

| Permission           | Allows                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------- |
| `system.power`       | Quitting Forma; restart and shutdown                                                        |
| `system.settings`    | Batocera's system settings: display, audio, language, region, storage (Batocera only)       |
| `store.manage`       | Installing and removing content from Batocera's content downloader (Batocera only)          |
| `system.network`     | Seeing and changing Wi-Fi and Ethernet (Batocera only)                                      |
| `settings.batocera`  | RetroAchievements and netplay settings, and logging in to RetroAchievements (Batocera only) |
| `settings.emulators` | Emulator, core and game options (Batocera only)                                             |
| `settings.system`    | Reading and writing Forma settings, beyond the theme's own                                  |
| `network`            | Loading from the origins listed in `remote`                                                 |

They are checked in Rust before each command runs; a missing one returns a
`permissionDenied` error naming it. Everything else is open to every theme: browsing the
library, launching games, favourites, the theme's own settings, choosing the boot view and
switching theme. Those are the user's own actions, made in the UI.

A theme asking for a permission Forma does not know is not loaded.

## Starting up

Forma's entry screen comes first, whatever the theme. When Forma starts, Batocera's boot image;
then Forma's logo and a loader, with Batocera's, Forma's and the theme's versions in a corner.
It covers the theme page while it loads (and again on a theme switch), and goes once the theme
reports that it started. `connect()` reports it; a theme that wants the cover up until its first
screen is drawn connects with `{ deferReady: true }` and calls `forma.ready()` then. A theme
needs no loading screen of its own for start-up.

If a theme page has not reported within 30 seconds of starting to load, Forma shows the default
theme instead, for this session only: the user's choice of theme is kept for the next start.

## Developing

`npm run dev` builds the default theme, starts Forma against the fixture library with the
mock launcher, and rebuilds on every change. In development the window reloads by itself when
the theme's files change.

## Rules

- Themes are served from `forma://theme/<id>/`, so use **relative** asset URLs.
- A strict Content Security Policy applies: no remote scripts unless the manifest declares them.
- Everything goes through `@ramraiders/forma-sdk`. There is no private API, even for the bundled theme.

## Screen margins (overscan)

Many TVs enlarge the picture so its edges fall outside the screen. Forma keeps one setting for
it, shared by every theme and by its own start-up screen: `display.margins`, the percentage of
each side to keep clear (0–10), read with `settings.getSystem('display.margins')` (it needs
`settings.system`). The simplest way to honour it is to shrink the whole page evenly:

```ts
document.body.style.transform = margins > 0 ? `scale(${1 - (2 * margins) / 100})` : "";
```

Fixed overlays live inside the body, so they shrink with it. Forma's menu sets it (Start →
Interface → Screen margins), and tells the theme with `settings:changed`
(`{ scope: "system", key: "display.margins", value }`).
