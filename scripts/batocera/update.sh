#!/bin/bash
# Updates an installed Forma from a test build's forma-aarch64.tar.gz: the program, the shell
# and the themes, swapped into /userdata/system/forma/app in place. The libraries it runs with
# (WebKitGTK, GStreamer…) stay the installed AppImage's, so a build needing new ones needs the
# full AppImage and install.sh instead. The version before is kept in app.prev, to go back:
#
#   update.sh forma-aarch64.tar.gz
#   update.sh --back
set -eu

dest=/userdata/system/forma
app="$dest/app"
prev="$dest/app.prev"
[ -x "$app/usr/bin/forma" ] || { echo "no Forma installed: run install.sh with an AppImage first" >&2; exit 1; }

stop() {
    if /userdata/system/services/forma status | grep -q "is running"; then
        echo "Stopping the running Forma…"
        /userdata/system/services/forma stop
        sleep 2
    fi
}

if [ "${1:-}" = --back ]; then
    [ -d "$prev" ] || { echo "nothing to go back to" >&2; exit 1; }
    stop
    rm -rf "$dest/app.old"
    mv "$app" "$dest/app.old" && mv "$prev" "$app" && rm -rf "$dest/app.old"
    echo "Back to the version before. Now: batocera-services start forma"
    exit 0
fi

update="$(readlink -f "${1:?usage: update.sh <forma-aarch64.tar.gz> | --back}")"
work="$(mktemp -d "$dest/.update.XXXXXX")"
trap 'rm -rf "$work"' EXIT
tar -xzf "$update" -C "$work"
[ -x "$work/usr/bin/forma" ] && [ -d "$work/usr/lib/Forma/shell" ] \
    || { echo "not a Forma update" >&2; exit 1; }

stop
# The version before, as hard links: a copy in a moment, taking no room but what changes.
rm -rf "$prev"
cp -al "$app" "$prev"
# Replaced, not written over: the files in app.prev keep their contents.
rm -f "$app/usr/bin/forma"
cp "$work/usr/bin/forma" "$app/usr/bin/forma"
for dir in shell themes; do
    rm -rf "$app/usr/lib/Forma/$dir"
    cp -r "$work/usr/lib/Forma/$dir" "$app/usr/lib/Forma/$dir"
done
echo "Updated. Now: batocera-services start forma   Back: update.sh --back"
