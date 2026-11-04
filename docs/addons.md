# Add-ons (design)

Add-ons extend Forma itself: more scraper sources, integrations, new menu pages. They are not
Batocera's store packages and not themes. This is the design for #76; follow-up tickets build
it.

## What an add-on is

A folder with an `addon.json` manifest and JavaScript (ES2020 modules; written in TS and built
like a theme). Forma runs it in an embedded **QuickJS** engine (`rquickjs`, about 1 MB), one
runtime per add-on, on its own thread, with a memory limit and a time limit per call. QuickJS
has no file system, network or process access of its own: an add-on reaches only what Forma
hands it.

```json
{
  "id": "example-scraper",
  "name": "Example scraper",
  "version": "1.0.0",
  "author": "You",
  "description": "Box art from example.com",
  "forma": "^1",
  "entry": "main.js",
  "provides": ["scraper"],
  "permissions": ["network"],
  "hosts": ["https://api.example.com", "https://cdn.example.com"],
  "settings": [{ "key": "apiKey", "label": "API key", "type": "secret" }]
}
```

## What it may do

Through a `forma` object that Forma injects, gated by the manifest:

- `forma.http.get/post(url, options)`: only to the `hosts` it declares, with `network`.
- `forma.settings.get(key)`: its own settings, which Forma's menu shows (Start → Add-ons →
  the add-on) from `settings`, secrets included.
- `forma.log(...)`, to Forma's log.
- What it `provides` decides what Forma calls. A `scraper` exports
  `lookup(query) → { metadata, media }`, the same shape Forma's built-in sources return, and
  joins the scraper's source list, ordered and switched on and off by the user like the others.

No file access: Forma downloads the media an add-on names and writes them itself.

## Lifecycle

- Installed from the catalogue (`RAMRaiders/forma-themes`, an `addons` list beside `themes`)
  with the same checks as themes: SHA-256, id and version match, the manifest valid, the
  `forma` range satisfied. Before installing, the menu shows the permissions and hosts it asks
  for.
- Start → Add-ons: installed add-ons with their version, what they ask for, enable or disable,
  update (when the catalogue has a newer one) and remove; and Get more add-ons.
- Folders in `/userdata/system/forma/addons/<id>/`. A disabled add-on is not loaded. One that
  throws or exceeds its limits is stopped and marked failed, and Forma carries on.

## Follow-ups

1. The runtime: rquickjs, limits, the injected `forma` API, a scraper add-on joining the scraper.
2. The add-on manager in the menu and the catalogue's `addons` list.
3. `create-forma-addon`, as `create-forma-theme` is for themes.
