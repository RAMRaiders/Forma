# Fixtures

A small fake Batocera install for tests and `npm run dev`.

| Path                         | Stands in for                                    |
| ---------------------------- | ------------------------------------------------ |
| `usr-share-emulationstation` | `/usr/share/emulationstation` (base systems)     |
| `userdata`                   | `/userdata` (ROMs, gamelists, overlays, config)  |
| `usr-share-batocera`         | `/usr/share/batocera` (shader sets, decorations) |

Point Forma at it with:

```sh
FORMA_USERDATA=fixtures/userdata FORMA_SYSTEMS_CFG=fixtures/usr-share-emulationstation/es_systems.cfg
```

**Never add real ROMs or copyrighted artwork.** ROM files here are empty and images are 1×1
placeholders.

## Quirks it covers on purpose

- `nes/gamelist.xml`: XML entities and a character reference, CDATA, an undefined HTML entity
  (`&nbsp;`), an unknown element, an entry with no `<path>`, an entry for a file that is not on
  disk, a `<folder>` entry, and a ROM on disk (`Homebrew Demo.nes`) the gamelist does not list.
- `snes/gamelist.xml`: truncated mid-entry, as after a power cut during a write.
- `megadrive`: no gamelist at all.
- `psx`: a multi-disc `.m3u` whose `.cue` files sit in a subfolder.
- `es_systems_custom.cfg`: replaces `snes` by name and adds a `homebrew` system.
