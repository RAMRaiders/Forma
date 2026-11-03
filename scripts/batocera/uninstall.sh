#!/bin/bash
# Gives the box back to EmulationStation and removes Forma's program and service. Forma's data
# (/userdata/system/forma: the library database and settings) is kept unless --purge is given.
set -u

batocera-services disable forma
batocera-services stop forma
rm -f /userdata/system/services/forma
rm -rf /userdata/system/forma/app /userdata/system/forma/bin
# Forma's boot picture: Batocera's own video plays at boot again.
rm -f /userdata/splash/forma.png /userdata/system/forma/boot-picture.svg
if [ "${1:-}" = "--purge" ]; then
    rm -rf /userdata/system/forma
fi
pgrep -f '^emulationstation( |$)' > /dev/null || (cd /userdata && nohup /usr/bin/emulationstation-standalone > /dev/null 2>&1 &)
echo "Forma removed; EmulationStation is back."
