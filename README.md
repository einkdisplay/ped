# PED, Potato E-Ink Display

<a href="https://ferris.love/einkdisplay/ped"><img src="https://ferris.love/badge/einkdisplay/ped?variant=mini" alt="Badge showing this repository's Rust code analysis"></a>

PED (Potato E-Ink Display) is a tool that makes it easy to turn a Kindle into a
custom screen.

## Introduction

PED is a web browser. It opens a single tab, renders pages on the CPU, and
refreshes the result straight onto the e-ink panel with
[NiLuJe's FBInk](https://github.com/NiLuJe/FBInk), bypassing Kindle's X11
display server.

PED is also an experiment. Built with Rust and Servo, it explores how Servo, a still-emerging browser engine, behaves under long-running
use on embedded devices (namely Kindles).

### Why not just use the Experimental Browser?

Compared with PED, Kindle's built-in Experimental Browser has several problems.

#### A chrome you cannot hide

The stock browser has a permanent top chrome that takes roughly one sixth of the
screen. That is not catastrophic, but it is ugly for a dedicated information
display.

PED bypasses Kindle OS UI and paints the page directly to the panel, so it can
run truly full screen.

#### Ancient

On my PW3 (firmware 5.16.2.1.1), the built-in browser engine is WebKit 531.2.
That system firmware shipped around 2019, yet community research dates that
WebKit release to 2009. It does not even support Flexbox. Unless you enjoy
polyfilling everything, you will not want the stock browser.

Servo is still experimental and early-stage, but it already covers most features
modern pages need. That should be enough to build a comfortable information
display with contemporary web tech.

#### Restrictions

The stock browser can detect dynamic page elements and pop up a dialog refusing
to show the page. PED does not impose that restriction.

#### Coarse screen refresh control

PED exposes a `window.navigator.kindle` JS API
(*[details](./js-binding)*) so you can control when, how, and which regions of
the screen refresh.

The stock browser has no equivalent; refresh behavior is entirely out of your
hands.

## Install

- **Prerequisite**: a jailbroken Kindle.

Coming soon!

## Usage

After install, open PED from the KUAL menu or your library and it should just
work.

Before you rely on PED day to day, we recommend installing the
[USBNetwork](https://wiki.mobileread.com/wiki/USBNetwork) package and setting up
SSH from your computer to the Kindle. Test thoroughly before launching PED as
your main display path.

## Configuration

PED uses a TOML config file. Edit it on your computer and place it at the
expected path before starting PED.

See the [comments in the TOML file](./ped.toml).

## Building

You will need:

- A Rust toolchain
- Podman or Docker
- [Cross](https://github.com/cross-rs/cross)

See [DEVELOPMENT.md](./DEVELOPMENT.md).

## Authoring pages for PED

### TypeScript types

[![NPM Version](https://img.shields.io/npm/v/potatoeinkdisplay-types)](https://npmjs.com/package/potatoeinkdisplay-types)

You can use PED's Kindle browser API (`window.navigator.kindle`) to read device
info (network, screen, battery) and control screen refresh. Matching TypeScript
definitions are published on npm as `potatoeinkdisplay-types`.

Install it as a dev dependency:

```bash
pnpm add -D potatoeinkdisplay-types
```

Then add this to your entry file so TypeScript picks up the globals:

```typescript
/// <reference types="potatoeinkdisplay-types" />
```

### Testing

PED uses the Servo browser engine, which may not support every Web feature you
use during development.

Before shipping page code, download
[Servo Shell](https://github.com/servo/servo/releases/tag/v0.5.0) and verify
how your page renders there.

> PED is based on a development snapshot of Servo 0.6.0. The Servo Shell build
> above (Servo 0.5.0) is usually good enough, but if you want closer 1:1 parity
> you can build Servo Shell yourself from
> [that upstream Servo revision](https://github.com/servo/servo/commit/55964f7d6d50872b51d8f94ef03ad10ac0bbcf1e)
> (commit SHA-1: `55964f7`).

## License

MIT

> This repository vendors [FBInk code under `crates/fbink-sys/`](./crates/fbink-sys/),
> which remains licensed under GPLv3.
