#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CONFIG=${PED_CONFIG:-"$ROOT/ped.toml"}
PIDFILE=${PED_PIDFILE:-/tmp/ped.pid}
if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
  exit 0
fi
nohup "$ROOT/target/armv7-unknown-linux-musleabihf/release/ped" "--config=$CONFIG" \
  >/tmp/ped.log 2>&1 &
echo $! >"$PIDFILE"
