# Add-ons

Add-ons extend Forma itself; they are not Batocera's store packages and not themes. For now an
add-on can provide one thing, a **scraper source**: it joins Forma's scraper beside the built-in
sources, ordered, switched on and off and set up like them (see [the scraper](scraper.md)).
Start → Add-ons installs them from the catalogue, shows what each may do and reach, turns them
on and off, updates and removes them.

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
- More things to provide, as Forma grows places for them.
