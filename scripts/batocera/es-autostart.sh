#!/bin/sh
# es-autostart.sh add|remove: the line in labwc's autostart that keeps EmulationStation from
# starting at boot while the Forma service is enabled, so the screen goes from Batocera's boot
# straight to Forma's, never through EmulationStation's loading screen. With the service
# disabled (`batocera-services disable forma`), or Forma gone, EmulationStation starts as ever.
# Forma's start script hands the screen back to EmulationStation itself when Forma fails.
#
# Installed into Forma's bin; run by install.sh, uninstall.sh and at every start (Batocera may
# write the autostart afresh).

AUTOSTART=/userdata/system/.config/labwc/autostart
MARK='# forma: skip EmulationStation'
GUARD="batocera-settings-get system.services | grep -qw forma \
&& [ -x /userdata/system/forma/bin/forma-standalone ] && exit 0 $MARK"
ES_LINE='^/usr/bin/emulationstation-standalone'

[ -f "$AUTOSTART" ] || exit 0
case "$1" in
    add)
        grep -qF "$MARK" "$AUTOSTART" && exit 0
        grep -q "$ES_LINE" "$AUTOSTART" || exit 0
        # Right before EmulationStation: Batocera's device quirks above it still run.
        sed -i "\#$ES_LINE#i $GUARD" "$AUTOSTART"
        ;;
    remove)
        grep -qF "$MARK" "$AUTOSTART" || exit 0
        grep -vF "$MARK" "$AUTOSTART" > "$AUTOSTART.forma" && cat "$AUTOSTART.forma" > "$AUTOSTART"
        rm -f "$AUTOSTART.forma"
        ;;
    *)
        echo "usage: $0 add|remove" >&2
        exit 1
        ;;
esac
