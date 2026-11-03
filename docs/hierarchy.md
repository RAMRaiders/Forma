# Hierarchy views

EmulationStation organises a library one way: system, then games. In Forma a **view** is a
tree of nodes you define, resolved against the library database, so a theme only renders what
it is given. The bundled views are written in exactly the same format as yours
(`crates/forma-core/src/hierarchy/views/`), so any of them is a starting point.

The format is checked by [`view.schema.json`](view.schema.json); point your editor at it for
completion. Forma validates every view on load and says where a problem is
(`root.children.branches[1]: id "a" is used twice`).

## A view

```json
{
  "id": "manufacturer-system",
  "name": "By manufacturer",
  "description": "Nintendo, Sega, Sony… then their systems, then games.",
  "root": {
    "groupBy": "manufacturer",
    "children": {
      "groupBy": "system",
      "children": { "games": { "sort": "name" } }
    }
  }
}
```

| Field         | Meaning                                                            |
| ------------- | ------------------------------------------------------------------ |
| `id`          | Lower-case letters, digits and dashes. Unique.                     |
| `name`        | Shown to the user.                                                 |
| `description` | Optional.                                                          |
| `root`        | The top node. Every branch of the tree must end in a `games` node. |

## Nodes

Every node is exactly one of these three, and may also carry a `filter` that applies to
everything below it.

### `groupBy`: split games by a field

```json
{ "groupBy": "genre", "sort": "count", "children": { "games": {} } }
```

| Key            | Groups by                                       | Natural order      |
| -------------- | ----------------------------------------------- | ------------------ |
| `system`       | System, labelled with its full name             | `es_systems` order |
| `manufacturer` | The system's manufacturer                       | name               |
| `hardware`     | The system's kind: console, portable, computer… | name               |
| `genre`        | Game genre, as scraped                          | name               |
| `developer`    | Game developer                                  | name               |
| `publisher`    | Game publisher                                  | name               |
| `region`       | Game region                                     | name               |
| `year`         | Release year                                    | ascending          |
| `decade`       | Release decade, labelled `1980s`                | ascending          |
| `players`      | Maximum players, labelled `2 players`           | ascending          |

`sort` is `natural` (default), `name` or `count` (largest first). Games with no value for the
field are grouped under **Unknown**, always last. `children` is what each group contains.

### `branches`: a fixed menu

Each branch has an `id`, a `name`, an optional `filter` and its own `children`. This is how
you build a home screen:

```json
{
  "branches": [
    {
      "id": "favorites",
      "name": "Favourites",
      "filter": { "favorite": true },
      "children": { "games": {} }
    },
    {
      "id": "by-decade",
      "name": "By decade",
      "children": { "groupBy": "decade", "children": { "games": {} } }
    }
  ]
}
```

### `games`: the list at the bottom

```json
{ "games": { "sort": "lastPlayed", "order": "desc", "limit": 50 } }
```

| Field   | Values                                                                          |
| ------- | ------------------------------------------------------------------------------- |
| `sort`  | `name` (default), `releaseDate`, `rating`, `lastPlayed`, `playCount`            |
| `order` | `asc` or `desc`. Defaults: ascending for name and date, descending for the rest |
| `limit` | Show at most this many                                                          |

Games with no value for the sort field come last either way.

## Filters

Every condition in a filter must hold. Filters on nested nodes add up.

| Field                                                                             | Matches                                                  |
| --------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `system`, `manufacturer`, `hardware`, `genre`, `developer`, `publisher`, `region` | One value or a list of any; case-insensitive, exact      |
| `year`, `players`                                                                 | A range: `{ "min": 1985, "max": 1995 }`, either end open |
| `favorite`                                                                        | `true` or `false`                                        |
| `played`                                                                          | `true`: played at least once. `false`: never played      |
| `collection`                                                                      | In the user's collection of this name; case-insensitive  |

Forma also makes a `collections` view from the user's collections: a branch per collection
(its id is the name in lower case, dashes between words), its games by name. With no
collection yet, it is an empty list of games.

Hidden games and games whose file is missing never appear in any view.

## Bundled views

| id                    | What it is                                                    |
| --------------------- | ------------------------------------------------------------- |
| `systems`             | System, then games: the EmulationStation layout               |
| `manufacturer-system` | Manufacturer, then system, then games                         |
| `genre`               | Genres across all systems, largest first                      |
| `all`                 | Every game from every system, A to Z                          |
| `favorites`           | Favourite games from every system                             |
| `recent`              | The last 50 games played                                      |
| `home`                | A menu of favourites, recents, systems, manufacturers, genres |

## The view Forma opens with

Whichever view you choose is what Forma boots into — for example your own
manufacturer → system view instead of the EmulationStation-style system list. It is a Forma
setting (`views.default`), not a theme setting, so it survives a theme change. Forma picks:

1. your `views.default` setting;
2. otherwise the view the theme suggests in `theme.json` (`"hierarchy"`);
3. otherwise the bundled `systems` view.

If the chosen view is deleted or broken, Forma falls back to the next one and says why. Themes
read and set it through the SDK: `nav.defaultView()` and `nav.setDefaultView(id)`.

## Your own views

Drop JSON files in `/userdata/system/forma/views/` (Forma creates the folder). They are
reloaded as soon as you save. A file with the same `id` as a bundled view replaces it. A broken
file is reported and skipped; it never takes the others down with it.

## How themes browse a view

A theme asks for a view and a **path**: the keys it has opened so far, from the root down.
Each group's `key` (or branch `id`) is the next path segment. The answer is one page of:

- **groups**: key, label, game count and a representative image (the best-rated game in the
  group that has one);
- **branches**: id, name and game count;
- **games**: the games themselves, with a total for paging.

It also carries a **trail** of labels for a breadcrumb: `Manufacturers › Sega › Mega Drive`.
