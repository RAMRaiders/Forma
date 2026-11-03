#!/usr/bin/env bash
# Pins the Cyber theme Forma bundles: builds a checkout of RAMRaiders/forma-theme-cyber, packs
# its dist reproducibly into themes/cyber/cyber-<version>.tar.gz and records the version, the
# commit and the checksum in themes/cyber/lock.json. The build checks that checksum.
#
#   scripts/pin-cyber.sh ../forma-theme-cyber
set -euo pipefail

cyber="$(cd "${1:?usage: pin-cyber.sh <forma-theme-cyber checkout>}" && pwd)"
here="$(cd "$(dirname "$0")/.." && pwd)"
out="$here/themes/cyber"

if [ -n "$(git -C "$cyber" status --porcelain)" ]; then
  echo "pin-cyber: $cyber has uncommitted changes; pin a commit" >&2
  exit 1
fi
commit="$(git -C "$cyber" rev-parse HEAD)"
version="$(node -p "require('$cyber/public/theme.json').version")"

(cd "$cyber" && npm run build >/dev/null)

rm -f "$out"/cyber-*.tar.gz
file="cyber-$version.tar.gz"
tar -C "$cyber/dist" --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -cf - . \
  | gzip -9n > "$out/$file"
sha256="$(sha256sum "$out/$file" | cut -d' ' -f1)"

cat > "$out/lock.json" <<JSON
{
  "id": "cyber",
  "version": "$version",
  "commit": "$commit",
  "file": "$file",
  "sha256": "$sha256"
}
JSON
echo "pinned Cyber $version ($commit), $sha256"
