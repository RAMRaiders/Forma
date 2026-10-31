#!/bin/sh
# Runs RomDex on the device and reports start-up time and memory, for the spike (#39).
#
#   measure.sh /userdata/RomDex.AppImage [seconds] [userdata-folder]
#
# Run it from Batocera's graphical session (see docs/batocera-integration.md for how), with
# EmulationStation stopped. RomDex starts with the frame-time overlay on and the mock launcher,
# so nothing is launched; scroll a long list while it runs and note the overlay's numbers.
set -eu
app="$1"
seconds="${2:-60}"
userdata="${3:-}"
log=/tmp/romdex-spike.log

export ROMDEX_PERF=1 ROMDEX_LAUNCHER=mock ROMDEX_FULLSCREEN=1 APPIMAGE_EXTRACT_AND_RUN=1
if [ -n "$userdata" ]; then
  export ROMDEX_USERDATA="$userdata" ROMDEX_SYSTEMS_CFG="$userdata/../es_systems.cfg"
fi

"$app" >"$log" 2>&1 &
pid=$!

# The AppImage runtime starts RomDex as a child, and RomDex starts WebKit's helpers: measure
# all of them. PSS counts shared pages once, split between the processes using them.
procs() {
  { pgrep -x romdex; pgrep -f 'WebKitWebProcess|WebKitNetworkProcess|WebKitGPUProcess'; } 2>/dev/null || true
}
peak=0
last=0
i=0
while [ "$i" -lt "$seconds" ] && kill -0 "$pid" 2>/dev/null; do
  last=0
  for p in $(procs); do
    kb=$(awk '/^Pss:/ {print $2}' "/proc/$p/smaps_rollup" 2>/dev/null || echo 0)
    last=$((last + ${kb:-0}))
  done
  [ "$last" -gt "$peak" ] && peak=$last
  i=$((i + 1))
  sleep 1
done
pkill -x romdex 2>/dev/null || true
kill "$pid" 2>/dev/null || true
# An AppImage run with APPIMAGE_EXTRACT_AND_RUN unpacks into /tmp, which is RAM on Batocera.
rm -rf /tmp/appimage_extracted_* 2>/dev/null || true

echo "--- RomDex spike, $(uname -m), $(date)"
grep -E 'ready [0-9]+ ms after start' "$log" || echo "start-up: no ready line (see $log)"
echo "memory (PSS): peak $((peak / 1024)) MiB, at the end $((last / 1024)) MiB (RomDex + WebKit)"
echo "log: $log"
