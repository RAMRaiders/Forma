# Add-ons

Add-ons extend Forma itself; they are not Batocera's store packages and not themes. An add-on
provides one or more of:

- **`scraper`**: a source beside the scraper's built-in ones, ordered, switched on and off and
  set up like them (see [the scraper](scraper.md));
- **`music`**: web radios, listed in Music under "From add-ons" and played like the user's own;
- **`menu`**: pages in Forma's own menu, which Forma draws (Start → Add-ons → Open), so they work
  with the pad and the keyboard over every theme. Core owns function: an add-on gives rows, never
  HTML.

And an add-on from Forma's own catalogue may have a **system part**: programs of its own,
installed with it, removed with it, and run when its pages ask (see below).

Start → Add-ons installs them from the catalogue, shows what each may do and reach, opens their
pages, takes their settings, turns them on and off, updates and removes them.

## What an add-on is

A folder named after its id in `/userdata/system/forma/addons/`, with an `addon.json` and one
JavaScript module (ES2020; write it in TypeScript and bundle it to one file, as a theme is
built). Forma runs it in an embedded **QuickJS** engine (`rquickjs`): a fresh runtime for each
call, with a 64 MB memory limit and a 60 s deadline (its requests included), so nothing carries
over and a runaway script is stopped. QuickJS has no file system, network or process access of
its own, and there is no module loader: the add-on reaches only what Forma hands it.

```json
{
  "id": "example-scraper",
  "name": "Example scraper",
  "version": "1.0.0",
  "author": "You",
  "description": "Box art from example.com",
  "sdk": "^1",
  "entry": "main.js",
  "provides": ["scraper"],
  "permissions": ["network"],
  "hosts": ["https://api.example.com", "https://cdn.example.com"],
  "settings": [{ "key": "apiKey", "label": "API key", "secret": true, "required": true }],
  "scraper": { "gives": ["thumbnail", "image"], "metadata": true }
}
```

Forma checks it before running anything: a kebab-case id matching the folder, an `sdk` range
this Forma satisfies, only `provides` and `permissions` it knows, `hosts` that are plain
`https://` origins (and only with `network`), and an `entry` file inside the folder (4 MB at
most). One that fails is listed with the reason and never run.

## What it may do

A frozen `forma` object, and standard JavaScript:

- `forma.http.get(url, headers?)`, `forma.http.post(url, body, headers?)` and
  `forma.http.request({ method, url, headers, body })`: synchronous; they return
  `{ status, body, json() }` (an HTTP error is a `status`, not an exception) and throw for a
  URL that is not on one of its `hosts` (same scheme, host and port) or a network failure.
- `forma.settings.get(key)`: what the user entered for one of its `settings`, or `null`.
- `forma.log(...)`, to Forma's log.
- `forma.system.run(name, args?)`: runs one of its system part's `commands` and returns what it
  printed (it throws when the command fails). Only an add-on with a system part, installed from
  Forma's catalogue; for any other it throws.

### A scraper

The module exports `lookup(query)` (it may be `async`). The query:

```ts
{
  system: string;         // the Batocera system, e.g. "snes"
  platforms: string[];    // its es_systems.cfg <platform> names
  fileName: string;       // "Super Metroid (USA).sfc"
  stem: string;           // "Super Metroid (USA)"
  name: string;           // a name to search with: "Super Metroid"
  size: number;
  hashes: { crc32: string; md5: string; sha1: string } | null;
  wanted: MediaKind[];    // the kinds still wanted: ask only for these
  wantsMetadata: boolean;
}
```

It returns `null` when it does not know the game, or:

```ts
{
  metadata?: { name?, description?, developer?, publisher?, genre?, players?, releaseDate?, rating? };
  media?: { kind: MediaKind; url: string }[];  // best first; several per kind are tried in order
}
```

`releaseDate` is `YYYY`, `YYYY-MM` or `YYYY-MM-DD`; `rating` from 0 to 1. Forma downloads the
media itself and checks each is what it claims; URLs not on the add-on's `hosts` are dropped.
Its settings are entered in Forma's menu, on its page or as its source in the scraper.

### Web radios

With `"provides": ["music"]`, the module exports `stations()` (it may be `async`), returning
`[{ name, url }]`: `http(s)://` streams, 500 at most, played by Forma's player (mpv). Forma asks
again when the add-on is installed, turned on or off, or one of its settings or actions changes
something.

### Pages in the menu

With `"provides": ["menu"]` and a `"menu": { "label": "Radio box", "description": "…" }`
section, it appears under Start → Add-ons → Open. The module exports:

- `page({ path })`: `path` is `[]` for its first page, then what its rows `open`. It returns
  `{ title?, rows }`, 200 rows at most, each:

  ```ts
  {
    label: string;
    detail?: string;
    heading?: string;           // starts a group
    open?: string[];            // A opens this page
    action?: string;            // A calls action() with it…
    ask?: { title: string; secret?: boolean; value?: string };  // …after the keyboard, with what was typed
    adjust?: string;            // left and right call action() with it, and -1 or 1
    alt?: { label: string; action: string };  // Y
  }
  ```

- `action({ action, input })`, returning `{ notify?, open?, back? }`: a message to show, a page
  to open, or back a page. Forma then draws the page again.

### A system part

Some add-ons need more than the sandbox: a driver, a daemon, a tool. Those declare it:

```json
{
  "permissions": ["network", "system"],
  "system": {
    "install": "system/install.sh",
    "remove": "system/remove.sh",
    "commands": { "tune": "system/tune.sh" }
  }
}
```

The scripts are files in the add-on's folder, run with `sh` from it, **with Forma's rights (root
on Batocera)**: `install` after the add-on is unpacked (if it fails, the add-on is taken out
again), `remove` before its folder is deleted (if it fails, the folder stays, to try again), and
each of `commands` when its script calls `forma.system.run(name, args)`, with `args` as the
script's arguments.

That is full trust, so only add-ons from **Forma's own catalogue**, reviewed there and installed
with their SHA-256 checked, may have one. Installing one from any other catalogue is refused; one
copied in by hand is listed with the reason and never runs. Install says first that it gets full
control of the machine.

## Lifecycle

- The catalogue (`RAMRaiders/forma-themes`) has an `addons` list beside `themes`, with the same
  fields (`sdk` being the add-on's range) and the same checks on install: SHA-256, the package
  holding that id and version, and the manifest valid. Installing asks for a second press after
  saying what it may do.
- Turned off (Forma setting `addons.disabled`), an add-on is not loaded. Updates come from the
  catalogue; removing deletes its folder.
- A theme can do the same through `forma.addons` with the `addons.manage` permission.

## Still to come

- `create-forma-addon`, as `create-forma-theme` is for themes (#109).
- More things to provide, as Forma grows places for them (a theme's widgets, a game's page).
