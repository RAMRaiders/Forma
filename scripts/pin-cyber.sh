#!/usr/bin/env bash
# Pins the Cyber theme Forma bundles to a release of RAMRaiders/forma-theme-cyber (made there
# with scripts/release.sh): records its version, commit and the checksum of its archive in
# themes/cyber/lock.json. The build (scripts/fetch-themes.sh) downloads that release and refuses
# an archive whose checksum differs.
#
#   scripts/pin-cyber.sh 0.7.12     # or no version for the latest release
set -euo pipefail

repo="RAMRaiders/forma-theme-cyber"
here="$(cd "$(dirname "$0")/.." && pwd)"

if [ -n "${1:-}" ]; then
  tag="v${1#v}"
else
  tag="$(gh release view --repo "$repo" --json tagName --jq .tagName)"
fi
version="${tag#v}"
file="cyber-$version.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
gh release download "$tag" --repo "$repo" --pattern "$file" --dir "$tmp"
sha256="$(sha256sum "$tmp/$file" | cut -d' ' -f1)"
commit="$(gh api "repos/$repo/commits/$tag" --jq .sha)"

cat > "$here/themes/cyber/lock.json" <<JSON
{
  "id": "cyber",
  "repo": "$repo",
  "version": "$version",
  "commit": "$commit",
  "file": "$file",
  "sha256": "$sha256"
}
JSON
echo "pinned Cyber $version ($commit), $sha256"
