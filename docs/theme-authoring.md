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

## Where themes live

- Bundled themes ship with RomDex.
- User themes go in `/userdata/themes-romdex/<id>/`.

## Rules

- Themes are served from `romdex://theme/<id>/`, so use **relative** asset URLs.
- A strict Content Security Policy applies: no remote scripts unless the manifest declares them.
- Everything goes through `@romdex/sdk`. There is no private API, even for the bundled theme.
