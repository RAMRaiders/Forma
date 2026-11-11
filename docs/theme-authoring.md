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

To start a new theme, `packages/create-forma-theme` makes one: a single screen that lists
systems and games and launches one, with tests on the mock and a README on installing it.

```sh
node packages/create-forma-theme/bin/create-forma-theme.js ../my-theme --name "My Theme"
# once the package is published: npm create @ramraiders/forma-theme my-theme
```

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
| `author`      | Who made it. Optional; shown on the theme's page in Interface.          |
| `description` | One or two sentences on what it is like. Optional; shown there too.     |
| `entry`       | The HTML page Forma loads.                                              |
| `sdk`         | Caret range of the SDK API the theme needs. A mismatch refuses to load. |
| `permissions` | Privileged capabilities the theme uses, e.g. `system.power`.            |
| `resolutions` | Resolutions the theme was designed for. Informational.                  |
| `settings`    | The theme's own settings, shown in Forma's menu (see below).            |
| `views`       | The only views the theme shows, by id, e.g. `["systems"]`. Absent, all. |
| `draws`       | What the theme draws itself instead of Forma: `nowPlaying`, `status`.   |
| `logo`        | An `.svg`, `.png` or `.webp` in the theme folder, e.g. `"logo.svg"`.    |
| `scraper`     | What the scraper should fetch for the theme (see below).                |

With a `logo`, Forma's menus and its start-up cover show it in place of the theme's name: in
Interface's theme list, at the top of the Start menu and beside the version on the cover. Draw
it wide and short (about 4:1), legible at 30 px high, on a transparent background.

### What the theme needs from the scraper

A theme that shows more than the usual box, screenshot, marquee, logo and video says so, and
Forma's scraper fetches it too while the theme is in use, on top of what the user chose:

```json
"scraper": {
  "media": ["boxside", "boxback", "cartridge", "fanart", "manual"],
  "fields": ["ageRating"]
}
```

A theme whose views show different things says what each needs on top, under `views`, and
names the setting that holds the view in use:

```json
"scraper": {
  "media": ["fanart"],
  "viewSetting": "gamesView",
  "views": { "collector": { "media": ["boxside", "boxback", "cartridge"] } }
}
```

With no `viewSetting`, the view in use is the one the theme reports in `setContext`.

`media` are media kinds as `forma.media.url` names them. `fields` are details beyond the usual
ones (name, description, developer, publisher, genre, players, date, rating): for now
`ageRating`, which makes a game without age ratings count as missing details. Kinds and fields
Forma does not know are ignored. The scraper's page in Forma's menu shows what the theme asks
for, marks those kinds "for <theme>", and lets the user turn it off.

`shows` says every kind the theme draws, in any view, and `backups` which of them it draws only
when another is missing (since SDK 1.26.0, and a theme using them needs `"sdk": "^1.26"`):

```json
"scraper": {
  "shows": ["thumbnail", "image", "video", "wheel", "marquee"],
  "backups": { "marquee": "wheel" }
}
```

Forma's scraper menus then set every other kind apart under "Not used by <theme>", and a game's
Scrape screen does not pick those by default, nor a backup when what it replaces is on offer.
They can still be fetched: the files serve other themes and programs. A theme with no `shows`
has nothing set apart.

Start → Interface lists the themes you can pick. Forma's plain **Default** theme is a safety net
for when no other theme can start, so it is listed only while it is in use.

## Forma's menus, over every theme

**Start** and **Select** belong to Forma. Whatever the theme, Start opens Forma's menu (theme,
where Forma opens, screen margins, music, RetroAchievements, netplay, network, the content
downloader, game and system settings, EmulationStation, power) and Select opens its views list.
A theme never receives those two buttons (F1 and Tab on a keyboard), nor any input while one of
Forma's menus is open. So a user can always change theme, even from a theme with no menu at all.
Select opens its page when let go; held on a game list, it shows only the favourites, and held
again, all of them (the rest of the list's filter stays). The list changes as any filter
changes it: `library:changed`.

Forma's menus are drawn by its shell, a script Forma puts first in every theme page's `<head>`.
It lives in a closed shadow root at the top of the page: a theme's CSS cannot reach it, and it
cannot reach the theme's. It acts with Forma's own rights, not the theme's: its menus work
whatever the theme's `permissions` say.

A theme works with the menus in three ways:

- **What is on screen.** `forma.shell.setContext({ view, system, game, list })` as the
  selection moves; the menu then offers options for that game and system, the views list marks
  the view shown, and Select opens Forma's options for the `list` shown (for `systems`, their
  order) and the theme's own settings for it (see below). Systems come from `library.systems()` already in the user's order: keep it.
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

A setting can also be offered on **Select** while the theme shows a list, with
`"select": ["games"]` (or `"systems"`). Select's page then has a row with the theme's logo,
opening a page of those settings. Use it for how the theme shows that list, such as its view
of a game list:

```json
{
  "key": "gamesView",
  "label": "View",
  "group": "Games",
  "select": ["games"],
  "type": "choice",
  "choices": [{ "value": "immersive", "label": "Immersive" }],
  "default": "immersive"
}
```

The setting stays under Interface too. `select` needs Forma's SDK 1.23 (`"sdk": "^1.23"`):
an older Forma does not know the field and will not load the theme. `type` is `toggle` (`true`/`false`) or `choice` (one of `choices`, by `value`). A setting whose
`default` it cannot take, an empty `choices` or a key listed twice keeps the theme from loading,
with the reason in Forma's theme list.

A theme that shows the library only some ways lists them in `views`. Forma's views list (Select)
and its "Start on" setting then keep to them; with one view, Forma offers no choice at all.

### The song that just started

When a new song starts, Forma's shell shows its title and artist for the time set in Forma's
menu (Start → Music). When and for how long are Forma's; how it looks can be the theme's:

- restyle Forma's card from the theme's CSS, through its parts:
  `forma-shell::part(now-playing)`, `::part(song-title)`, `::part(song-artist)`,
  `::part(song-note)`;
- or draw it entirely: `"draws": ["nowPlaying"]` in `theme.json`, then show
  `music:song` (`{ title, artist, seconds }`) and hide it after `seconds`.

### Controllers and the connection

Forma shows, top right, an icon per controller connected (by kind, numbered by player) and the
connection, marked when the internet does not answer. Restyle it through
`forma-shell::part(status)`, `::part(status-controller)` (and `status-xbox`, `status-playstation`,
`status-nintendo`, `status-generic`) and `::part(status-network)`; or draw it yourself with
`"draws": ["status"]`, from `forma.status.get()` and `status:changed`.

### What a game just earned

RetroArch shows its own pop-up for each achievement while the game runs. Afterwards, Forma
asks RetroAchievements what the session earned and shows a card for 12 seconds: the game, up to
four achievements with their badges and points, the points in all, and the user's progress in
the game. Restyle it through `forma-shell::part(unlocks)`, `::part(unlock)` and
`::part(unlock-badge)`; or draw it yourself with `"draws": ["achievements"]`, from `ra:unlocked`
(`{ gameId, game, unlocked, points }`). It needs the game's RetroAchievements id and the web API
key; a session that earned nothing sends nothing.

## Where themes live, and installing your own

- Bundled themes ship with Forma: **Cyber**, the default, at the version pinned in
  `themes/cyber/lock.json`, and the plain **Default** theme from `packages/theme-default`.
- A user theme with a bundled theme's id stands in for it only when its `version` is newer
  (`0.3.1`, or a prerelease such as `0.3.1-dev`, over `0.3.0`), so a theme can be updated without
  a new Forma. An older or equal copy is listed as unavailable. If the newer copy fails to
  start, Forma sets it aside and goes back to the bundled version.
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
| `system.update`      | Checking for and installing Batocera updates (Batocera only)                                |
| `system.controllers` | Seeing, pairing, connecting and forgetting Bluetooth controllers (Batocera only)            |
| `settings.batocera`  | RetroAchievements and netplay settings, and logging in to RetroAchievements (Batocera only) |
| `settings.emulators` | Emulator, core and game options (Batocera only)                                             |
| `settings.system`    | Reading and writing Forma settings, beyond the theme's own                                  |
| `themes.manage`      | Installing, updating and removing themes                                                    |
| `addons.manage`      | Installing, turning on or off and removing Forma add-ons                                    |
| `scraper.manage`     | Scraping game details and media, and the scraper's settings and accounts                    |
| `network`            | Loading from the origins listed in `remote`                                                 |

They are checked in Rust before each command runs; a missing one returns a
`permissionDenied` error naming it. Everything else is open to every theme: browsing the
library, launching games, favourites, the theme's own settings, choosing the boot view and
switching theme. Those are the user's own actions, made in the UI.

A theme asking for a permission Forma does not know is not loaded.

## Starting up

Forma's entry screen comes first, whatever the theme: Forma's logo and a loader, with
Batocera's, Forma's and the theme's versions in a corner. It covers the theme page while it
loads (and again on a theme switch), and fades out as soon as the theme reports that it
started. `connect()` reports it; a theme that wants the cover up until its first
screen is drawn connects with `{ deferReady: true }` and calls `forma.ready()` then. A theme
needs no loading screen of its own for start-up.

If a theme page has not reported within 30 seconds of starting to load, Forma shows another
theme instead, for this session only (the user's choice is kept for the next start): the bundled
version of a theme that was standing in for one, else Cyber, else the plain Default theme.

## Developing

Cyber is not kept in this repository. The build (`scripts/fetch-themes.sh`, run by
`npm run dev` and the AppImage workflow) downloads the release of RAMRaiders/forma-theme-cyber
pinned in `themes/cyber/lock.json`, refuses an archive whose checksum differs and unpacks it
into `build/themes/cyber`. To ship a new Cyber with Forma, release it from its repository
(`scripts/release.sh` there), then pin it here: `scripts/pin-cyber.sh <version>` records the
version, commit and checksum in `lock.json`. Fetching needs `gh` signed in to an account that
can read the Cyber repository; CI uses the `CYBER_TOKEN` secret.

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

## Publishing to the theme catalogue

Forma installs and updates themes from a catalogue: a JSON file, by default
`catalogue.json` in [`RAMRaiders/forma-themes`](https://github.com/RAMRaiders/forma-themes)
(the Forma setting `themes.catalogueUrl` points elsewhere, for a catalogue of your own).

```json
{
  "themes": [
    {
      "id": "neon",
      "name": "Neon",
      "version": "1.2.0",
      "author": "You",
      "description": "Bright lines on black.",
      "sdk": "^1",
      "permissions": ["system.power"],
      "previews": ["https://example.com/neon.png"],
      "download": "https://github.com/you/neon/releases/download/v1.2.0/neon-1.2.0.zip",
      "sha256": "…64 hex digits…"
    }
  ]
}
```

The package is a zip of the built theme (`dist/`), with `theme.json` at its root or in one
folder. Its `id` and `version` must match the entry. Forma refuses a package whose SHA-256
differs, which has paths leaving the folder or links, or which cannot load. URLs must be
`https://`. To publish an update, raise `version` in `theme.json`, attach the new zip to a
release, and change `version`, `download` and `sha256` in the catalogue. Users see it in
Interface as "Update to …".
