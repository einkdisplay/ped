# `potatoeinkdisplay-types`

TypeScript types for pages running inside **PED** (Potato E-ink Display),
including `navigator.kindle`.

PED runtime: https://github.com/einkdisplay/ped

## Install

```sh
pnpm add -D potatoeinkdisplay-types
# or: npm / yarn equivalent
```

## Use

Reference the package once so TypeScript picks up the global `Navigator`
augmentation:

```typescript
/// <reference types="potatoeinkdisplay-types" />

await navigator.kindle.screen.refreshNow({ waveform: "quality" });
const battery = await navigator.kindle.device.battery();
```

Deno:

```typescript
/// <reference types="npm:potatoeinkdisplay-types" />
```
