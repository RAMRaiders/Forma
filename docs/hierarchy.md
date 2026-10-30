# Hierarchy views

> Draft. The view schema lands in #13 and the engine in #14.

EmulationStation organises a library one way: system, then games. In RomDex a **view** is a
tree of nodes you define, resolved against the library index, so a theme only renders nodes.

## Node types

| Node         | What it does                                                                                   |
| ------------ | ---------------------------------------------------------------------------------------------- |
| `groupBy`    | Splits games by `system`, `manufacturer`, `genre`, `year`, `players`, `developer` or `region`. |
| `filter`     | Keeps games matching an expression.                                                            |
| `collection` | A static list, or `favorites` / `recent`.                                                      |
| `games`      | The leaf: a sorted list of games.                                                              |

## Example: manufacturer → system → games

```json
{
  "id": "manufacturer-system",
  "name": "By manufacturer",
  "root": {
    "groupBy": "manufacturer",
    "children": { "groupBy": "system", "children": { "games": { "sort": "name" } } }
  }
}
```

## Bundled views

`systems` (EmulationStation-equivalent), `manufacturer-system`, `genre`, `favorites`, `recent`.

## The view RomDex opens with

Whichever view you choose is what RomDex boots into — for example your own
manufacturer → system view instead of the EmulationStation-style system list. It is a RomDex
setting (`views.default`), not a theme setting, so it survives a theme change. RomDex picks:

1. your `views.default` setting;
2. otherwise the view the theme suggests in `theme.json` (`"hierarchy"`);
3. otherwise the bundled `systems` view.

If the chosen view is deleted or broken, RomDex falls back to the next one and says why. Themes
read and set it through the SDK: `nav.defaultView()` and `nav.setDefaultView(id)`.

## Your own views

Drop JSON files in `/userdata/system/romdex/views/`. A broken view is reported and skipped; it
never takes the others down with it.
