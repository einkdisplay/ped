# `ped-types`

TypeScript types for pages running inside **PED** (Potato E-ink Display),
including `navigator.kindle`.

PED runtime: https://github.com/einkdisplay/ped

## Install

```sh
pnpm add -D ped-types
# or: npm / yarn equivalent
```

## Use

Reference the package once so TypeScript picks up the global `Navigator`
augmentation:

```typescript
/// <reference types="ped-types" />

await navigator.kindle.screen.refreshNow({ waveform: "quality" });
const battery = await navigator.kindle.device.battery();
```

Deno:

```typescript
/// <reference types="npm:ped-types" />
```
