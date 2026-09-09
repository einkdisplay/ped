#!/bin/sh
# Independent recovery helper for PED lifecycle markers.
# Intended for a Kindle Upstart/cron job. Acts only when a marker exists.

MARKER="${PED_MARKER:-/var/run/ped/session.json}"
SERVICE="${PED_SERVICE:-lab126_gui}"

if [ ! -f "$MARKER" ]; then
  exit 0
fi

now=$(date +%s)
heartbeat=$(sed -n 's/.*"heartbeat_at"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$MARKER" | head -n 1)
timeout=$(sed -n 's/.*"watchdog_timeout_s"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$MARKER" | head -n 1)
pid=$(sed -n 's/.*"pid"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$MARKER" | head -n 1)
timeout=${timeout:-30}
heartbeat=${heartbeat:-0}
age=$((now - heartbeat))

alive=0
if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
  alive=1
fi

if [ "$alive" -eq 1 ] && [ "$age" -le "$timeout" ]; then
  exit 0
fi

echo "ped-watchdog: recovering stale marker (pid=$pid age=${age}s timeout=${timeout}s)"
if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
  kill "$pid" 2>/dev/null || true
  sleep 1
  kill -9 "$pid" 2>/dev/null || true
fi
/sbin/start "$SERVICE" >/dev/null 2>&1 || true
rm -f "$MARKER"
exit 0
