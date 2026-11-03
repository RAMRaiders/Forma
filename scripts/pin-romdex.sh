#!/usr/bin/env bash
# Pins the romdex library Forma builds with: copies its library crate and the rcheevos source
# it compiles from a clean checkout of RAMRaiders/romdex into vendor/romdex, and records the
# commit in vendor/romdex/PINNED. A copy, not a git dependency, so building Forma needs no
# access to the private romdex repository.
#
#   scripts/pin-romdex.sh ../romdex
set -euo pipefail

src="$(cd "${1:?usage: pin-romdex.sh <romdex checkout>}" && pwd)"
dest="$(cd "$(dirname "$0")/.." && pwd)/vendor/romdex"

if [ -n "$(git -C "$src" status --porcelain)" ]; then
  echo "pin-romdex: $src has uncommitted changes; pin a commit" >&2
  exit 1
fi
commit="$(git -C "$src" rev-parse HEAD)"

rm -rf "$dest" && mkdir -p "$dest/crates" "$dest/vendor"
cp -R "$src/crates/romdex" "$dest/crates/romdex"
cp -R "$src/vendor/rcheevos" "$dest/vendor/rcheevos"
rm -rf "$dest/crates/romdex/target"
[ -f "$src/LICENSE" ] && cp "$src/LICENSE" "$dest/"
# romdex's workspace with its library alone: the crate inherits its settings from it.
sed -e 's#^members = .*#members = ["crates/romdex"]#' \
  -e '/^clap = /d' "$src/Cargo.toml" > "$dest/Cargo.toml"
printf '%s\n' "$commit" > "$dest/PINNED"
echo "pinned romdex $commit"
