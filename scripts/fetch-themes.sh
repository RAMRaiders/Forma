#!/usr/bin/env bash
# Fetches the pinned themes in themes/*/lock.json from their repositories' GitHub releases
# (release v<version>, asset <file>), checks each against its checksum and unpacks it into
# build/themes/<id>, for the AppImage to carry (and `tauri dev` to find). Downloads are kept in
# build/downloads, so a pinned theme is fetched once.
#
# The theme repositories are private: `gh` needs a token that can read them (locally, your
# `gh auth login`; in CI, GH_TOKEN).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cache="$here/build/downloads"
mkdir -p "$cache"

field() { node -p "require('$1')['$2']"; }

for lock in "$here"/themes/*/lock.json; do
  id="$(field "$lock" id)"
  repo="$(field "$lock" repo)"
  version="$(field "$lock" version)"
  file="$(field "$lock" file)"
  want="$(field "$lock" sha256)"
  archive="$cache/$file"

  if [ ! -f "$archive" ] || [ "$(sha256sum "$archive" | cut -d' ' -f1)" != "$want" ]; then
    rm -f "$archive"
    echo "fetching $id $version from $repo"
    gh release download "v$version" --repo "$repo" --pattern "$file" --dir "$cache"
  fi
  have="$(sha256sum "$archive" | cut -d' ' -f1)"
  if [ "$have" != "$want" ]; then
    rm -f "$archive"
    echo "fetch-themes: $repo v$version $file is $have, lock.json says $want" >&2
    exit 1
  fi

  dest="$here/build/themes/$id"
  rm -rf "$dest" && mkdir -p "$dest"
  tar -C "$dest" -xzf "$archive"
  echo "unpacked $id $version"
done
