# Imports

Forma reads the **romdex import format**: files that tools such as `romdex` and RetroWorker
write, carrying what `gamelist.xml` cannot — ROM hashes above all — so Forma never hashes a
library on a Raspberry Pi. The format itself (header, record fields, validation rules,
versioning) is specified in romdex:
[`docs/import-format.md`](https://github.com/RAMRaiders/romdex/blob/main/docs/import-format.md).

This page is about what Forma does with it.

## Where files go

`/userdata/system/forma/import/*.jsonl` (Forma creates the folder). Forma notices new, changed
and deleted files on its own.

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

Forma's copy of the sample file, [`fixtures/import/retroworker.jsonl`](../fixtures/import/retroworker.jsonl),
is the one its tests use; romdex holds the canonical one.
