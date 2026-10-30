# Theme authoring

> Draft. The SDK API lands in #28 and the reference documentation in #30.

A RomDex theme is a folder containing a `theme.json` manifest and a web app.

## `theme.json`

```json
{
  "id": "default",
  "name": "RomDex Default",
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
| `entry`       | The HTML page RomDex loads.                                             |
| `sdk`         | Caret range of the SDK API the theme needs. A mismatch refuses to load. |
| `permissions` | Privileged capabilities the theme uses, e.g. `system.power`.            |
| `resolutions` | Resolutions the theme was designed for. Informational.                  |

## Where themes live, and installing your own

- Bundled themes ship with RomDex.
- User themes go in `/userdata/themes-romdex/<id>/`. Installing a theme is copying its folder
  there; RomDex lists it and lets you switch to it. A theme with a bad manifest or an
  incompatible `sdk` range is listed as unavailable, with the reason, and never loaded.
- An installer (from a `.zip` or URL, with update and remove) comes in Milestone 4 (#52). It
  shows the permissions a theme asks for before installing it.
- A theme may ship its own hierarchy views and suggest one to boot into; your own boot view
  setting still wins (see [hierarchy.md](hierarchy.md)).

## How a theme is served

RomDex serves the selected theme from `romdex://localhost/theme/<id>/`, straight from its
folder. Only files inside that folder are reachable: `..`, absolute paths and symlinks leading
out of it are refused.

Game media never comes as a path on disk. The SDK hands out URLs such as
`romdex://localhost/media/<game id>/image` (also `video`, `marquee`, `thumbnail`, `fanart`,
`manual`); videos support seeking.

### Content Security Policy

Every HTML page gets a CSP that allows the theme's own folder (`romdex:`), Tauri's IPC, inline
styles, and `data:`/`blob:` images and media. Nothing else loads, unless the manifest lists it:

```json
{ "permissions": ["network"], "remote": ["https://fonts.example.com"] }
```

Remote origins must be `https://` and need the `network` permission.

## Permissions

| Permission        | Allows                                                      |
| ----------------- | ----------------------------------------------------------- |
| `system.power`    | Quitting RomDex; restart and shutdown (with M3)             |
| `settings.system` | Reading and writing RomDex settings, beyond the theme's own |
| `network`         | Loading from the origins listed in `remote`                 |

They are checked in Rust before each command runs; a missing one returns a
`permissionDenied` error naming it. Everything else is open to every theme: browsing the
library, launching games, favourites, the theme's own settings, choosing the boot view and
switching theme. Those are the user's own actions, made in the UI.

A theme asking for a permission RomDex does not know is not loaded.

## Starting up

A theme reports that it started by calling `theme_ready` (the SDK does it on start). If the
selected theme has not done so within 15 seconds, RomDex switches to the default theme rather
than leaving a blank screen.

## Developing

`npm run dev` builds the default theme, starts RomDex against the fixture library with the
mock launcher, and rebuilds on every change. In development the window reloads by itself when
the theme's files change.

## Rules

- Themes are served from `romdex://theme/<id>/`, so use **relative** asset URLs.
- A strict Content Security Policy applies: no remote scripts unless the manifest declares them.
- Everything goes through `@romdex/sdk`. There is no private API, even for the bundled theme.
