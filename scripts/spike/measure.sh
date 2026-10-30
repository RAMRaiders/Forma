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
peak=0
last=0
i=0
while [ "$i" -lt "$seconds" ] && kill -0 "$pid" 2>/dev/null; do
  # RomDex plus every WebKit helper process: what the WebView really costs.
  # pgrep finds nothing until WebKit has started; that is not an error.
  pids="$pid $({ pgrep -f 'WebKitWebProcess|WebKitNetworkProcess|WebKitGPUProcess' || true; } | tr '\n' ' ')"
  last=0
  for p in $pids; do
    kb=$(awk '/^VmRSS:/ {print $2}' "/proc/$p/status" 2>/dev/null || echo 0)
    last=$((last + ${kb:-0}))
  done
  [ "$last" -gt "$peak" ] && peak=$last
  i=$((i + 1))
  sleep 1
done
kill "$pid" 2>/dev/null || true

echo "--- RomDex spike, $(uname -m), $(date)"
grep -E 'ready [0-9]+ ms after start' "$log" || echo "start-up: no ready line (see $log)"
echo "memory: peak $((peak / 1024)) MiB, at the end $((last / 1024)) MiB (RomDex + WebKit processes)"
echo "log: $log"
