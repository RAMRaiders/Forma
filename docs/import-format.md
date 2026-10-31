# Import format

How another tool feeds Forma what `gamelist.xml` cannot carry — ROM hashes above all — so
Forma never re-hashes a library on a Raspberry Pi. RetroWorker is the first producer, but the
format is the whole contract: no code is shared, and anything that writes these files works.

The header says `romdex-import`, not `forma-import`, on purpose: identifying ROMs (hashing,
DAT matching, RetroAchievements hashes) is planned as its own project, `romdex`, and this is
its format. Forma reads it today; the specification will move to that repository when it
exists, unchanged.

## Where files go

`/userdata/system/forma/import/*.jsonl` (Forma creates the folder). One file per producer is
the convention, e.g. `retroworker.jsonl`. Forma notices new, changed and deleted files on its
own; write to a temporary name and rename it into place, so it never reads half a file.

## Shape

[JSON Lines](https://jsonlines.org/): one JSON object per line, UTF-8. Blank lines are ignored.

The **first line is a header**:

```json
{
  "format": "romdex-import",
  "formatVersion": 1,
  "source": "retroworker",
  "generatedAt": "2026-10-31T12:00:00Z"
}
```

| Field           | Required | Meaning                                                            |
| --------------- | -------- | ------------------------------------------------------------------ |
| `format`        | yes      | Always `romdex-import`                                             |
| `formatVersion` | yes      | `1`. A file with a higher version is skipped whole, with a warning |
| `source`        | no       | Who wrote it                                                       |
| `generatedAt`   | no       | When, ISO 8601                                                     |

**Every other line is one game:**

```json
{
  "system": "nes",
  "path": "The Legend of Zelda (USA).nes",
  "metadata": {
    "name": "The Legend of Zelda",
    "releaseDate": "1986-02-21",
    "publisher": "Nintendo"
  },
  "media": { "marquee": "./images/zelda-marquee.png" },
  "hashes": {
    "crc32": "3FE272FB",
    "md5": "337bd6f1a1163df31bf2633665589ab0",
    "sha1": "dab79c84934f9aa5db4e7dad390e5d0c12443fa2",
    "ra": "e2a91f8e3e5a7ffc5f8fd9a9dd1b6b1d"
  },
  "raGameId": 1447
}
```

(Shown wrapped; in the file each record is on one line.)

| Field      | Required | Meaning                                                                                                        |
| ---------- | -------- | -------------------------------------------------------------------------------------------------------------- |
| `system`   | yes      | The system id from `es_systems.cfg`, e.g. `nes`                                                                |
| `path`     | yes      | The ROM, relative to the system's folder (`./` optional), or absolute `/userdata/roms/...`                     |
| `metadata` | no       | See below                                                                                                      |
| `media`    | no       | `image`, `thumbnail`, `marquee`, `video`, `fanart`, `manual`: paths relative to the system folder, or absolute |
| `hashes`   | no       | `crc32` (8 hex), `md5` (32), `sha1` (40), `ra` (32): the RetroAchievements hash. Any case                      |
| `raGameId` | no       | The game's RetroAchievements id, a positive number                                                             |

Metadata keys: `name`, `description`, `developer`, `publisher`, `genre`, `players` (text, e.g.
`1-2`), `releaseDate` (`YYYY`, `YYYY-MM` or `YYYY-MM-DD`), `rating` (number, 0.0–1.0),
`region`, `lang`.

There is deliberately no favourite, play count or last played: those belong to the user.

## How Forma applies it

- **Precedence:** what the user sets in Forma wins, then imports, then `gamelist.xml`. A field
  an import does not set keeps the gamelist's value.
- **Several files:** read in name order; where two set the same field, the later file wins.
- **Changes:** any change to the import files (one edited, added or deleted) re-applies them
  all, so deleting a file removes everything it said.
- **Games not in the library yet:** reported, and applied as soon as the file appears.
- **Problems** are reported with the file and line, and never stop the rest:
  - a line that is not a valid record is skipped;
  - a field with an invalid value (a hash of the wrong length, a rating of 7) is dropped and
    the rest of its record kept;
  - an unknown metadata key, media kind or hash is reported and ignored.

## What it is used for

- **RetroAchievements:** `raGameId` links a game to its achievements (`forma.ra.game`). The
  `ra` hash lets Forma match games by hash (#41) without hashing them on the device.
- **Netplay:** lobby rooms are matched to local games by CRC first, then by name.
- **Metadata and media** from a producer that knows better than the scraper.

## Versioning

Additions that older readers can ignore (a new optional field) keep `formatVersion` 1. A change
in meaning bumps it, and older Forma versions then skip the file rather than misread it.

A sample with deliberate mistakes, used by the tests, is
[`fixtures/import/retroworker.jsonl`](../fixtures/import/retroworker.jsonl).
