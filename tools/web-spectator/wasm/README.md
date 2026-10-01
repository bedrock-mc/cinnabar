# Browser spectator terrain

This optional Cinnabar extension turns a duel arena snapshot into triangle
geometry through `world::ChunkStore`, `assets::RuntimeAssets` and
`meshing::mesh_sub_chunk`. It does not launch a desktop client, authenticate an
Xbox account, or send game commands. The website owns its camera, rendering
surface and live player presentation.

Visual support is **diagnostic**: named flat colors and solid unit cubes replace
texture, transparency, liquid and partial-block shapes. The shared Cinnabar
visibility classifier maps vanilla invisible blocks to air, so barriers, light
blocks, structure voids, invisible bedrock and moving blocks neither draw nor
hide neighboring faces. These approximations are
recorded as incomplete in the master plan. This target closes no vanilla parity
gate. No Mojang assets are bundled.

## Binding

```js
import init, { mesh_arena } from './web_spectator.js';
await init();
const vertices = mesh_arena(JSON.stringify(arena));
```

`vertices` is a `Float32Array`, with interleaved position XYZ, outward normal XYZ,
and linear color RGB for each triangle vertex. Invalid input throws a descriptive
error. World coordinates are preserved, including negative coordinates.

The input is `{ id, name, palette, blocks, bounds }`. Palette entries are
`{ name, states }`; index zero must be `air` or `minecraft:air`. Blocks are
`[x, y, z, paletteIndex]`. Bounds are inclusive minimum XYZ followed by maximum
XYZ. Duplicate positions use their final palette value; zero or an invisible
block removes a block from the visual mesh.
Unrelated metadata is ignored.

The browser admission limits are 32 MiB JSON, one million block records,
`assets::MAX_TEXTURE_LAYERS` palette entries, 4096 occupied subchunks,
coordinates within one million blocks of the origin, 1024 blocks per bounding
axis and two million output vertices. Meshing is a one-time arena operation;
the live pose stream does not rebuild terrain every tick.

## Build

Use the repository's shared-host verification wrapper and one target directory
for this worktree. Install Rust's `wasm32-unknown-unknown` target and the
`wasm-bindgen` CLI matching `Cargo.lock` first.

```sh
CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p web-spectator --lib \
  --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir target/web-spectator \
  target/wasm32-unknown-unknown/debug/web_spectator.wasm
```

Deploy `web_spectator.js` and `web_spectator_bg.wasm` together under the website's
`public/cinnabar/`. The debug build is deliberate during bounded development;
production optimization is separate from this initial acceptance slice.
