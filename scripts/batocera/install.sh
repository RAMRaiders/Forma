#!/bin/bash
# Installs (or updates) Forma on a Batocera box, run on the box itself:
#
#   ./install.sh /userdata/Forma_aarch64.AppImage
#
# Extracts the AppImage to /userdata/system/forma/app (no FUSE needed, and no RAM spent
# unpacking it at every start), installs the `forma` user service and enables it. Forma
# replaces EmulationStation from the next boot; `batocera-services start forma` starts it now.
set -eu

appimage="$(readlink -f "${1:?usage: install.sh <Forma AppImage>}")"
here="$(cd "$(dirname "$0")" && pwd)"
dest=/userdata/system/forma

mkdir -p "$dest/bin" /userdata/system/services
work="$(mktemp -d "$dest/.install.XXXXXX")"
trap 'rm -rf "$work"' EXIT

echo "Extracting $(basename "$appimage")…"
(cd "$work" && "$appimage" --appimage-extract > /dev/null)
[ -x "$work/squashfs-root/AppRun" ] || { echo "not a Forma AppImage" >&2; exit 1; }

# A Forma running from the old copy would keep the library open: stop it first, with the new
# service script (older ones missed a Forma whose binary had been replaced).
install -m 755 "$here/forma-service" /userdata/system/services/forma
if /userdata/system/services/forma status | grep -q "is running"; then
    echo "Stopping the running Forma…"
    /userdata/system/services/forma stop
    sleep 2
fi

# Swap in the new version only once it is fully extracted.
rm -rf "$dest/app.old"
[ -d "$dest/app" ] && mv "$dest/app" "$dest/app.old"
mv "$work/squashfs-root" "$dest/app"
rm -rf "$dest/app.old"

install -m 755 "$here/forma-standalone" "$dest/bin/forma-standalone"
install -m 755 "$here/forma-boot-picture" "$dest/bin/forma-boot-picture"
batocera-services enable forma

echo "Installed. Forma replaces EmulationStation from the next boot."
echo "Now: batocera-services start forma   Undo: ./uninstall.sh"
