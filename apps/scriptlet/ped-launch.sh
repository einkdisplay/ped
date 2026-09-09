#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export PED_CONFIG=${PED_CONFIG:-"$ROOT/ped.toml"}
exec "$ROOT/bin/ped-start.sh"
