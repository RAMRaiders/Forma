#!/bin/bash
# Gives the box back to EmulationStation and removes Forma's program and service. Forma's data
# (/userdata/system/forma: the library database and settings) is kept unless --purge is given.
set -u

batocera-services disable forma
batocera-services stop forma
rm -f /userdata/system/services/forma
# Forma's boot picture: Batocera's own video comes back at boot.
rm -f /userdata/splash/forma.png /userdata/system/forma/boot-picture.svg
rm -rf /userdata/system/forma/app /userdata/system/forma/bin
# The labwc rule that kept Forma's video player under its window.
RC=/userdata/system/.config/labwc/rc.xml
if [ -f "$RC" ] && grep -q 'identifier="forma-video"' "$RC"; then
    sed -i '/identifier="forma-video"/d' "$RC"
    pkill -HUP -x labwc || true
fi
if [ "${1:-}" = "--purge" ]; then
    rm -rf /userdata/system/forma
fi
pgrep -f '^emulationstation( |$)' > /dev/null || (cd /userdata && nohup /usr/bin/emulationstation-standalone > /dev/null 2>&1 &)
echo "Forma removed; EmulationStation is back."
