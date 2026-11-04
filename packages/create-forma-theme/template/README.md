# **NAME**

A theme for [Forma](https://github.com/RAMRaiders/Forma), the HTML frontend for Batocera.
It is a small web app: Forma shows it full screen and gives it the library through
`@ramraiders/forma-sdk`.

## Develop

The SDK comes from GitHub Packages. Sign in to it once, with a GitHub token that has
`read:packages`:

```sh
npm login --scope=@ramraiders --registry=https://npm.pkg.github.com
npm install
npm run dev      # opens in a browser, on the SDK's mock library
npm test         # drives the theme like a gamepad, against the mock
npm run build    # the theme, ready to install, in dist/
```

In a browser, the arrow keys, Enter and Escape stand in for the pad.

## What is where

| File                | What it is                                                          |
| ------------------- | ------------------------------------------------------------------- |
| `public/theme.json` | The manifest Forma reads: id, name, version, SDK range, permissions |
| `src/main.ts`       | Connects to Forma (or to the mock in a browser)                     |
| `src/app.ts`        | The one screen: systems, then a system's games; A plays             |
| `src/app.test.ts`   | Tests on the mock                                                   |

Forma owns the Start and Select menus, settings and the start-up cover: a theme only draws
the library. Tell Forma what is selected with `forma.shell.setContext`, so its menus offer the
right options. Ask for any permission the theme needs in `theme.json` (e.g. `system.power`); the
list is in Forma's `docs/theme-authoring.md`.

## Install on Batocera

Copy `dist/` to `/userdata/themes-forma/__ID__/` on the box, e.g.:

```sh
npm run build
scp -r dist root@batocera.local:/userdata/themes-forma/__ID__
```

Then pick it in Forma: Start → Interface. To update it, raise `version` in `theme.json`, build
and copy it again.
