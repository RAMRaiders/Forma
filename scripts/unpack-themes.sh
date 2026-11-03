#!/usr/bin/env bash
# Unpacks the pinned themes in themes/*/lock.json into build/themes/<id>, after checking each
# archive against its checksum, for the AppImage to carry (and `tauri dev` to find).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
for lock in "$here"/themes/*/lock.json; do
  dir="$(dirname "$lock")"
  id="$(node -p "require('$lock').id")"
  file="$(node -p "require('$lock').file")"
  want="$(node -p "require('$lock').sha256")"
  have="$(sha256sum "$dir/$file" | cut -d' ' -f1)"
  if [ "$have" != "$want" ]; then
    echo "unpack-themes: $file is $have, lock.json says $want" >&2
    exit 1
  fi
  dest="$here/build/themes/$id"
  rm -rf "$dest" && mkdir -p "$dest"
  tar -C "$dest" -xzf "$dir/$file"
  echo "unpacked $id $(node -p "require('$lock').version")"
done
