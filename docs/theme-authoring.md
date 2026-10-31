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

A theme reports that it started by calling `theme_ready` (the SDK does it on start). If the
selected theme has not done so within 15 seconds, Forma switches to the default theme rather
than leaving a blank screen.

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

Fixed overlays live inside the body, so they shrink with it. The default theme offers the
setting under Interface → Screen margins.
