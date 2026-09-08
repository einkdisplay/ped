# PED, Potato E-ink Display

PED is a non-touch Kindle Paperwhite information display. Servo renders
HTML/CSS/JavaScript with SWGL, and FBInk commits grayscale frames to `/dev/fb0`.
External control uses TOML, a restricted Unix socket, and `ped-cli`.

## Quick start

```sh
./target/armv7-unknown-linux-musleabihf/release/ped --config=ped.toml
./target/armv7-unknown-linux-musleabihf/release/ped-cli status
./target/armv7-unknown-linux-musleabihf/release/ped-cli refresh
./target/armv7-unknown-linux-musleabihf/release/ped-cli stop
```

The default configuration maps `dashboard/index.html` and its assets to an
ephemeral loopback HTTP server. Relative script paths resolve beside the TOML
file. The server is loopback-only and rejects traversal.

Supported control methods are `status`, `open`, `eval`, `emit`, `refresh`,
`reload-config`, and `stop`. `open` is origin-allowlisted. `reload-config` hot-applies display/control/origin settings when possible and
returns `restart_required` only for structural changes (socket, page, lifecycle, static server).

KUAL and Scriptlet launch assets are under `kual/` and `scriptlet/`. An independent
lifecycle helper is available as `bin/ped-watchdog.sh`. They use
the same binary and control path. Set `lifecycle.enabled = true` only on a
jailbroken Kindle where `/dev/fb0` and `/sbin/start`/`/sbin/stop` exist. PED
then records a marker, stops `lab126_gui`, and attempts to restore it during
cleanup.

## Browser API

`navigator.kindle.screen` supports `setAutoRefresh`, `lastRefresh`, `refreshNow`,
and `beginRefresh` transactions. `navigator.kindle.device.battery()` /
`network()` return device telemetry when available. Privileged calls are limited
to configured `trusted_origins`.


## Kernel entropy (Kindle)

AWS-LC (via rustls) blocks when `RNDGETENTCNT` reports `< 256` bits. PED:

1. harvests CPU timing jitter with `rand_jitter`
2. credits the kernel with `RNDADDENTROPY` (not a bare write to `/dev/random`)
3. keeps a background top-up thread while PED is running

Look for `ped: seeded kernel entropy ...` near startup. Residual `RNDGETENTCNT` spam should be rare once the maintain thread is alive.

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

## Entropy bootstrap

PED credits the kernel entropy pool at startup with CPU-jitter samples
(`rand_jitter` + `RNDADDENTROPY`) so AWS-LC/rustls does not block on Kindle
kernels that report low `entropy_avail`. This runs before Servo initializes.

## Build and validation

The authoritative build is the ARMv7 MUSL Podman build described in
[`../SERVO_ARMV7_BUILD_NOTES.md`](../SERVO_ARMV7_BUILD_NOTES.md). Host Cargo
checks are not authoritative because the vendored Kindle-oriented
`freetype-sys` build requires the ARM environment.

The verified software path is:

```text
Servo HTML/CSS/JS → SWGL CPU frame → grayscale/diff → FBInk → /dev/fb0
```

Hardware validation remains required for dynamic updates, sleep/wake,
long-running operation, crash recovery, and actual KUAL installation.

- Lifecycle design: [SERVO_KINDLE_PLAN.md](../SERVO_KINDLE_PLAN.md)
- ARMv7 build notes: [SERVO_ARMV7_BUILD_NOTES.md](../SERVO_ARMV7_BUILD_NOTES.md)
- Kindle lessons: [SERVO_KINDLE_LESSONS.md](../SERVO_KINDLE_LESSONS.md)
- TypeScript API draft: [`../kindle.d.ts`](../kindle.d.ts)

## Source layout

PED expects sibling checkouts (path dependencies):

```text
workspace/
  PED/          # this repo
  servo/        # https://github.com/einkdisplay/servo (branch kindle-fontconfigless)
  mozjs/        # https://github.com/einkdisplay/mozjs (branch kindle-armv7-musl)
  FBInk/        # https://github.com/NiLuJe/FBInk (upstream)
```

TypeScript API draft: `kindle.d.ts` in this repository root.
