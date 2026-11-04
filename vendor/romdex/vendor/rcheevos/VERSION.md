# Vendored rcheevos

- Upstream: https://github.com/RetroAchievements/rcheevos
- Version: **v12.5.0** (commit 1433173, released 2026-09-14)
- Taken: `include/*.h`, `src/rc_compat.{c,h}`, `src/rhash/*` — the hashing only. The runtime,
  API client and Lua support are not used and not copied.

To update: replace these files from the new tag, update this file, and run the RetroAchievements
hash tests (`cargo test -p romdex ra`). Files here are unmodified.
