#!/bin/sh
set -eu
SOCKET=${PED_SOCKET:-/tmp/ped.sock}
"$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/../target/armv7-unknown-linux-musleabihf/release/ped-cli" \
  --socket "$SOCKET" stop || true
