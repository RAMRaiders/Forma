# Forma

A modern frontend for [Batocera](https://batocera.org/), built to replace EmulationStation.

> **Status: early development.** Milestone 1 (Core MVP) is in progress. Track it on the
> [Forma Roadmap](https://github.com/orgs/RAMRaiders/projects/5).

## Why

EmulationStation themes are XML layouts with a fixed set of elements, and the library is
organised one way: system, then games. Forma keeps everything Batocera already does well —
its systems, `gamelist.xml` metadata and `emulatorlauncher` — and replaces the part you look at:

- **Themes are web apps.** HTML, CSS and JavaScript, with any layout, animation or framework.
- **A public SDK.** `@ramraiders/forma-sdk` exposes the library, navigation, launching, input, settings,
  RetroAchievements and netplay. The bundled theme uses nothing else, so anything it does, your
  theme can do.
- **Your own hierarchy.** Browse by manufacturer → system, genre, year, collections, or a view
  you define in JSON — not only system → games.
- **RetroAchievements and netplay** as first-class parts of the frontend.

## What it is not

- **Not an emulator.** Games still launch through Batocera's `emulatorlauncher`, so emulator
  settings, per-game overrides and RetroArch achievements keep working.
- **Not a ROM source.** It reads the library you already have.
- **Not a replacement for Batocera's system menus** (Wi-Fi, Bluetooth, updates) — yet.

## Architecture

| Path                     | What it is                                                       |
| ------------------------ | ---------------------------------------------------------------- |
| `crates/forma-core`      | Rust core: library, hierarchy engine, launcher, input. No Tauri. |
| `crates/forma-app`       | Tauri 2 shell that hosts themes and exposes the core to them.    |
| `packages/sdk`           | `@ramraiders/forma-sdk`, the public TypeScript API for themes.   |
| `packages/theme-default` | The reference theme, built only on the SDK.                      |
| `fixtures/userdata`      | A fake Batocera `/userdata` for tests and development.           |

More in [docs/architecture.md](docs/architecture.md).

## Development

Requirements: Node.js 24 (see `.nvmrc`) and Rust (pinned in `rust-toolchain.toml`). On Linux,
Tauri also needs the WebKitGTK development packages.

```sh
npm install
npm run dev          # Tauri window with the default theme, hot reloading
npm run verify       # every quality gate: format, lint, types, tests, build, clippy, knip
```

Commits follow [Conventional Commits](https://www.conventionalcommits.org/), enforced by a hook.

## Documentation

- [Architecture](docs/architecture.md)
- [Theme authoring](docs/theme-authoring.md)
- [SDK reference](docs/sdk.md)
- [Hierarchy views](docs/hierarchy.md)
- [Import format](docs/import-format.md) (for RetroWorker and other tools)
- [Batocera integration](docs/batocera-integration.md)

## Licence

[Apache-2.0](LICENSE).
