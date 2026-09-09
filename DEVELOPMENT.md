# PED development

Developer notes for building, deploying, controlling, and validating PED on
jailbroken Kindle hardware.

Product overview and end-user docs: [README.md](./README.md)  
Chinese overview: [README.zh.md](./README.zh.md)  
Agent operational brief: [AGENTS.md](./AGENTS.md)

## Runtime quick start

```sh
./target/armv7-unknown-linux-musleabihf/release/ped --config=ped.toml
./target/armv7-unknown-linux-musleabihf/release/ped-cli status
./target/armv7-unknown-linux-musleabihf/release/ped-cli refresh
./target/armv7-unknown-linux-musleabihf/release/ped-cli stop
```

The default configuration maps `dashboard/index.html` and its assets to an
ephemeral loopback HTTP server. Relative script paths resolve beside the TOML
file. The server is loopback-only and rejects path traversal.

Supported control methods are `status`, `open`, `eval`, `emit`, `refresh`,
`reload-config`, and `stop`. `open` is origin-allowlisted. `reload-config`
hot-applies display/control/origin settings when possible and returns
`restart_required` only for structural changes (socket, page, lifecycle, static
server). Field-level comments live in [`ped.toml`](./ped.toml).

KUAL and Scriptlet launch assets are under `apps/kual/` and `apps/scriptlet/`.
An independent lifecycle helper is available as `scripts/ped-watchdog.sh`. They
use the same binary and control path. Set `lifecycle.enabled = true` only on a
jailbroken Kindle where `/dev/fb0` and `/sbin/start`/`/sbin/stop` exist. PED
then records a marker, stops `lab126_gui`, and attempts to restore it during
cleanup.

## Browser API

`navigator.kindle.screen` supports `setAutoRefresh`, `lastRefresh`, `refreshNow`,
and `beginRefresh` transactions. `navigator.kindle.device.battery()` /
`network()` return device telemetry when available. Privileged calls are limited
to configured `trusted_origins`.

TypeScript package: [`js-binding/`](./js-binding/) (`potatoeinkdisplay-types`).

## Kernel entropy (Kindle)

AWS-LC (via rustls) blocks when `RNDGETENTCNT` reports `< 256` bits. PED:

1. harvests CPU timing jitter with `rand_jitter`
2. credits the kernel with `RNDADDENTROPY` (not a bare write to `/dev/random`)
3. keeps a background top-up thread while PED is running

This runs before Servo initializes. Look for `ped: seeded kernel entropy ...`
near startup. Residual `RNDGETENTCNT` spam should be rare once the maintain
thread is alive.

## Hardware validation page

The default dashboard intentionally mutates the DOM every second so a real
Kindle can prove the live path:

```text
page.js DOM updates → Servo/SWGL re-render → PED auto screenshot (~2.5s)
→ grayscale/diff → FBInk → /dev/fb0
```

What should change on screen without any `navigator.kindle` refresh API:

- large clock and DOM tick counter
- inverted black/white phase panel
- moving bar blocks
- rotating banner text
- `navigator.kindle` status card (`attrs only` is expected today)

Copy the release binaries plus `ped.toml` and the whole `dashboard/` directory
to the device, then run from that directory:

```sh
SERVO_DISABLE_SYSTEM_FONTS=1 ./ped --config=ped.toml
```

Optional control checks from another shell:

```sh
./ped-cli status
./ped-cli emit --type=update --detail='{"runtime":"manual-emit"}'
./ped-cli refresh
./ped-cli stop
```

## Build and validation

Authoritative builds target `armv7-unknown-linux-musleabihf`, typically via the
project Podman cross image (see [AGENTS.md](./AGENTS.md) and
[`Dockerfile.cross`](./Dockerfile.cross)). Host `x86_64` Cargo checks are not
authoritative: the Kindle-oriented `freetype-sys` / MUSL / FBInk path expects
the ARM cross environment.

Minimum ladder:

```sh
cargo check --target armv7-unknown-linux-musleabihf -p ped
cargo build --release --target armv7-unknown-linux-musleabihf -p ped
```

Verified software path:

```text
Servo HTML/CSS/JS → SWGL CPU frame → grayscale/diff → FBInk → /dev/fb0
```

Hardware validation remains required for dynamic updates, sleep/wake,
long-running operation, crash recovery, and actual KUAL installation.

## Source layout

PED pulls engine deps via git (not path members of this workspace):

```text
servo        https://github.com/einkdisplay/servo        (kindle-fontconfigless)
mozjs_sys    https://github.com/einkdisplay/mozjs        (kindle-armv7-musl)
freetype-sys https://github.com/einkdisplay/freetype-sys (master)
```

FBInk C sources used by `fbink-sys` are vendored under
`crates/fbink-sys/vendor/fbink/`.

Workspace layout and hard constraints: [AGENTS.md](./AGENTS.md).
