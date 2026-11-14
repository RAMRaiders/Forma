#!/bin/bash
# Gives the box back to EmulationStation and removes Forma's program and service. Forma's data
# (/userdata/system/forma: the library database and settings) is kept unless --purge is given.
set -u

batocera-services disable forma
batocera-services stop forma
rm -f /userdata/system/services/forma
# Batocera starts its session, with EmulationStation, at boot again; and labwc's autostart is
# Batocera's own again (it would be from the next boot anyway).
batocera-settings-set system.es.atstartup 1
cp /usr/share/labwc/autostart /userdata/system/.config/labwc/autostart
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
if ! pgrep -x labwc > /dev/null; then
    /etc/init.d/S31emulationstation start
elif ! pgrep -f '^emulationstation( |$)' > /dev/null; then
    (cd /userdata && nohup /usr/bin/emulationstation-standalone > /dev/null 2>&1 &)
fi
echo "Forma removed; EmulationStation is back."
