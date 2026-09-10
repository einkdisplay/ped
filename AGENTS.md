# AGENTS.md

Instructions for coding agents working on **PED** (Potato E-ink Display).

Human-oriented product docs live in `README.md` (Chinese: `README.zh.md`).
Build, deploy, control-plane, and hardware-validation notes live in
`DEVELOPMENT.md`. This file is the operational brief for automated changes.

## Project overview

PED is a non-touch side display for jailbroken Kindle Paperwhite. It embeds
Servo, rasterizes with SWGL (CPU), converts frames to grayscale, and commits
through FBInk to `/dev/fb0`.

```text
HTML/CSS/JS
  → Servo (einkdisplay fork) + SWGL
  → PED grayscale / dirty-region union
  → FBInk (vendored in fbink-sys)
  → /dev/fb0
```

Control plane: TOML config, Unix NDJSON socket, `ped-cli`.
Page API: `navigator.kindle` (WebIDL in the Servo fork + embedder bridge here).

**Target device:** Kindle Paperwhite 3-class ARMv7, `armv7-unknown-linux-musleabihf`,
no reliable GPU/EGL path. Do not reintroduce X11/GLX/Mesa as the primary display
path; that approach was tried and rejected.

## Repository layout

```text
PED/                          # this git repo (https://github.com/einkdisplay/ped)
  Cargo.toml                  # workspace root + [patch.crates-io]
  crates/
    ped/                      # binaries: ped, ped-cli
      src/main.rs             # main loop, Servo embed, FBInk commit, control drain
      src/{config,control,display,device,entropy,kindle_lifecycle,static_server,runtime}.rs
      src/bin/ped-cli.rs
    fbink-sys/                # FFI + vendored FBInk C subset
      vendor/fbink/           # do not assume a sibling ../../FBInk checkout
  dashboard/                  # hardware validation page (DOM mutates every 1s)
  apps/kual/ apps/scriptlet/  # launcher assets
  scripts/                    # start/stop/status/watchdog helpers
  js-binding/                 # npm package potatoeinkdisplay-types
  ped.toml                    # default config
  .cargo/config.toml          # ARMv7 linker flags
  Dockerfile.cross            # cross image notes (see build section)
```

Related **separate** git repos (not path members of this workspace):

| Repo | Branch | Role |
|---|---|---|
| https://github.com/einkdisplay/servo | `kindle-fontconfigless` | Engine + `navigator.kindle` WebIDL/DOM |
| https://github.com/einkdisplay/mozjs | `kindle-armv7-musl` | `mozjs_sys` ARMv7 MUSL patches (`153.0.0-0` pin) |
| https://github.com/einkdisplay/freetype-sys | `master` | FreeType cross-build patches |
| https://github.com/NiLuJe/FBInk | upstream | Source of vendored C; not a runtime path dep |

`crates/ped` pulls Servo crates via **git**. Workspace `[patch.crates-io]` redirects
`mozjs_sys` and `freetype-sys` to the forks above.

## Hard constraints

1. **Authoritative builds are ARMv7 MUSL**, typically via Podman image
   `localhost/ped-servo-cross:armv7-musl` (or equivalent). Host `x86_64` cargo
   check is not authoritative for Kindle linkage (freetype/sysroot/fbink).
2. **Display path is SWGL + FBInk.** Prefer `CpuRenderingContext` / memory frames.
   Do not make PED depend on Surfman opening X11/Wayland on device.
3. **UI takeover is Upstart-friendly:** stop/start the `x` job (not only
   `lab126_gui`); never raw-kill Amazon GUI as the recovery strategy. Restore with
   `start x` **and** `start lab126_gui`. Always restore GUI after tests when you
   stopped it.
4. **Privileged page APIs** must stay origin-gated (`trusted_origins` in config).
5. **Entropy:** Kindle kernels often sit under AWS-LC’s 256-bit `RNDGETENTCNT`
   floor. Keep startup + background `RNDADDENTROPY` jitter seeding
   (`crates/ped/src/entropy.rs`) before Servo/rustls init.
6. **Do not vendor full Cargo crate trees** into git (old `vendor/surfman` mistake).
   Use git deps/forks or minimal C vendor under `fbink-sys`.
7. **BusyBox on device:** `sleep` may not accept fractional seconds; use whole seconds.
8. **rsync to Kindle:** use `--no-owner --no-group --no-perms` when chown fails.

## Setup

```sh
git clone https://github.com/einkdisplay/ped.git
cd ped
# Cargo will fetch git deps (servo/mozjs/freetype-sys) on first build.
```

Cross toolchain expectations (container):

- Target: `armv7-unknown-linux-musleabihf`
- MUSL sysroot roughly at `/usr/local/musl/armv7-unknown-linux-musleabihf`
- `CC_armv7_unknown_linux_musleabihf` / `CXX_...` point at the musl cross gcc/g++
- ccache optional under `/opt/ccache`

Environment used by builds:

```sh
export CFLAGS_armv7-unknown-linux-musleabihf='-march=armv7-a -mfpu=vfpv3-d16 -mfloat-abi=hard'
export CXXFLAGS_armv7-unknown-linux-musleabihf='-march=armv7-a -mfpu=vfpv3-d16 -mfloat-abi=hard'
```

`.cargo/config.toml` already sets linker + `-lgcc` + `--allow-multiple-definition`
and `RUST_FONTCONFIG_DLOPEN=1`.

## Build commands

From repo root:

```sh
# Fast iteration (in cross container / with toolchain)
cargo check --target armv7-unknown-linux-musleabihf -p ped

# Release binaries
cargo build --release --target armv7-unknown-linux-musleabihf -p ped

# Outputs
# target/armv7-unknown-linux-musleabihf/release/ped
# target/armv7-unknown-linux-musleabihf/release/ped-cli
```

Only FBInk FFI:

```sh
cargo check --target armv7-unknown-linux-musleabihf -p fbink-sys
```

### Podman pattern (authoritative)

When using the project cross image, mount this repo and Cargo caches, set the
env vars above, and run cargo inside. Host network may be required the first
time git deps are fetched; afterward `--offline` can work with a warm
`~/.cargo/{git,registry}`.

Parallelism tip: nested `mozjs_*` build scripts can race unpacking crates.io
into a shared registry (`File exists` on `.cargo-ok`). If that happens, lower
`-j`, clean the bad unpack dirs, `cargo fetch` on the host, retry offline.

### mozjs patch version

Servo pins **`mozjs_sys` 153.0.0-0** (via crates.io `mozjs` 0.25.0). The
`einkdisplay/mozjs` branch `kindle-armv7-musl` **must stay on that version**.
A newer `153.0.0-1` main tip will show up as `[[patch.unused]]` and silently
fall back to unpatched crates.io (broken ARMv7).

## Runtime / device workflow

Deploy payload under `/mnt/us/ped/` typically: `ped`, `ped-cli`, `ped.toml`,
`dashboard/`.

```sh
# On device (after stopping stock GUI if taking over fb0)
SERVO_DISABLE_SYSTEM_FONTS=1 ./ped --config=/mnt/us/ped/ped.toml

# Another shell
./ped-cli status
./ped-cli refresh
./ped-cli emit --type=update --detail='{"k":"v"}'
./ped-cli stop
```

Control methods: `status`, `open`, `eval`, `emit`, `refresh`, `reload-config`, `stop`.
`open` is origin-allowlisted. `reload-config` hot-applies non-structural settings;
structural changes return `restart_required`.

Lifecycle (`lifecycle.enabled = true`): marker + stop `x` (cascades GUI) +
heartbeat; cleanup must `start x` then `start lab126_gui` (and best-effort wait
for framework). Independent helper: `scripts/ped-watchdog.sh`.

Config paths: `Config::load` should canonicalize so relative `page.url` / asset
roots resolve when cwd differs from the toml location.

## Architecture notes for code changes

### Threads

- **Main thread:** Servo event pump, screenshot, grayscale/diff, FBInk,
  Kindle embedder command execution, control command application.
- **Background only:** control accept loop, static HTTP, lifecycle heartbeat,
  entropy maintain thread.

Do not drive FBInk from arbitrary worker threads.

### `navigator.kindle`

- WebIDL + DOM live in the **servo** fork (`Kindle.webidl`, `kindle*.rs`).
- Embedder messages / delegate hooks live in servo shared embedder + PED main.
- Prefer async embedder callback patterns (no blocking script `recv` on the JS
  thread).
- Keep TypeScript defs in `js-binding/index.d.ts` aligned with WebIDL when the
  API changes (`width`/`height`, `lastRefresh(): number | null` epoch ms, etc.).

### Display

- Grayscale + changed-region union in `display.rs`.
- FBInk: `print_raw_data`, `refresh_rect`, optional flash / wait-for-complete.
- Default dashboard is a high-contrast hardware proof page, not a product UI.

### fbink-sys

- Builds only the Kindle IMAGE subset with
  `FBINK_FOR_KINDLE`, `FBINK_MINIMAL`, `FBINK_WITH_DRAW`, `FBINK_WITH_IMAGE`.
- Sources under `crates/fbink-sys/vendor/fbink/` (includes `eink/`, `stb/stb_image.h`).
- FBInk uses `#\tinclude "..."` forms; when re-vendoring, scan with `#\s*include`
  and pull submodules (`stb`) as needed.
- Do not invoke FBInk’s top-level Makefile (pulls Kobo i2c-tools submodule).

## TypeScript package (`js-binding/`)

- npm name: **`potatoeinkdisplay-types`** (unscoped on purpose).
- Pure types; no runtime entry.
- After API changes, update `index.d.ts` and keep `package.json` `types`/`exports`.
- Consumers:

```ts
/// <reference types="potatoeinkdisplay-types" />
```

Publish from `js-binding/` with pnpm when intentionally releasing types.

## Testing

There is no full unit-test suite for the embedder yet.

Minimum validation ladder:

1. `cargo check --target armv7-unknown-linux-musleabihf -p ped`
2. `cargo build --release --target armv7-unknown-linux-musleabihf -p ped`
3. On-device smoke: start PED, confirm first frame log, dirty-region refreshes,
   `ped-cli status` counters, clean `stop`, GUI restored.
4. Exercise page API only after DOM auto-refresh path is healthy.
5. Watch for `RNDGETENTCNT` spam; entropy guard should keep it near zero.

Promise-returning JS checked via `eval` often needs a window latch + sleep;
`evaluate_javascript` does not await Promises.

## Code style

- Rust edition **2024** (workspace).
- Match existing module split and naming in `crates/ped/src/`.
- Prefer small, boring diffs; no drive-by refactors.
- Keep comments sparse and only for non-obvious Kindle/Servo constraints.
- Do not commit `target/`, logs, device sysroots, or probe tarballs.
- Do not commit `node_modules` or `js-binding/pnpm-lock.yaml` (ignored).

## PR / commit guidelines

- Prefer focused commits: runtime vs packaging vs types vs docs.
- Mention device impact when touching FBInk, lifecycle, entropy, or display.
- If changing Servo/WebIDL, coordinate the **servo** fork commit/branch and
  bump/lock as needed in this repo’s `Cargo.lock`.
- Before claiming green: ARMv7 check at minimum; release build for binary-size
  or link-sensitive changes.

## Common failure modes

| Symptom | Likely cause |
|---|---|
| `invalid page.url: relative URL without a base` | Config path not absolute/canonical; fix load-side resolve |
| `RNDGETENTCNT` spam / stall | Entropy pool drained; ensure `EntropyGuard` runs pre-Servo |
| `[[patch.unused]]` mozjs_sys | Fork version ≠ 153.0.0-0 |
| fbink missing `eink/mxcfb-kindle.h` / `stb/stb_image.h` | Incomplete vendor set |
| Host build fails freetype/zlib | Use ARMv7 cross env, not host-as-authority |
| GUI gone after crash | Lifecycle/watchdog; manually `start x && start lab126_gui` |

## What not to do

- Do not add Mesa/X11 as required runtime for PED mainline.
- Do not publish whole `cargo vendor` crate dumps into this repo.
- Do not force-push shared forks (`servo`/`mozjs`) without explicit instruction.
- Do not leave the device with `x` / `lab126_gui` stopped after a session you started.
- Do not expand FBInk vendor to “entire upstream tree” without need; keep the
  Kindle IMAGE subset.

## Quick command card

```sh
cargo check --target armv7-unknown-linux-musleabihf -p ped
cargo build --release --target armv7-unknown-linux-musleabihf -p ped
# device
SERVO_DISABLE_SYSTEM_FONTS=1 ./ped --config=ped.toml
./ped-cli status && ./ped-cli stop
```
