# Browser spectator

This optional, read-only extension runs **Cinnabar's renderer library directly
through WASM**. Bevy/WGPU owns the canvas and uses the existing chunk, actor and
UI GPU plugins and their unchanged shaders. JavaScript supplies snapshots,
skins and camera choices. It does not render replacement terrain or players.

WebGPU is required. Unsupported browsers receive an explicit error. There is
no flat-color or WebGL approximation fallback. Each viewer belongs in its own
same-origin iframe: destroying that document disposes its Winit event loop,
canvas and GPU resources. No desktop client or Xbox login runs per viewer, and
the renderer never connects to a game server or sends gameplay input.

## Runtime assets

The host builds Cinnabar's pinned vanilla carriers with the existing `assetc`
commands, then mounts them as read-only runtime data. The viewer requires the
world, entity, actor, particle, equipment, HUD, item-icon, JSON-UI and font carriers plus the
matching canonical block registry. Publish content-addressed URLs with
immutable caching and a small separately cached bundle manifest. These are
runtime artifacts; do not commit Mojang images, pack archives or carriers.

`TerrainAssets` verifies the pinned source identity, registry SHA-256, protocol,
visual count and air identity. Arena palette names and scalar states resolve
through that exact registry. Dragonfly boolean bit states normalize to the
registry's integer values. Unknown states fail instead of becoming guessed
cubes. Native cube, partial-model, transparent and liquid streams use the
26-neighbour palette-native mesher and the renderer's compiled materials,
textures, UVs and visibility rules, including invisible barriers.

The native actor path uses compiled player rigs, skin normalization, authored
armor geometry/artwork and the shared fixed 20 Hz Molang animator. Streamed
movement, sneak/sprint/swim flags, swing/hurt observations and held-item use
feed the native motion and pose owners. Third-person equipment and the native
first-person main-hand pass share their existing display transforms and atlas.
The HUD reuses native JSON-UI layout, stat painters,
item icons, font metrics, texture atlas and GPU overlay. Pure presentation
helpers live in shared native owners consumed by both the desktop app and browser adapter.
Public fighter names use the native font-pixel nameplate layout, retained text
atlas and existing billboard GPU pass, including sneaking depth/alpha policy.
The selected POV player's own tag is hidden. Tags and HUD share the compiled
font and bounded text cache; no server-only name metadata is exposed.
The shared nameplate atlas samples the font's exclusive texel-edge UVs. Dirty
cells upload through one mapped GPU staging buffer with aligned rows, bounded
by the atlas size; copy commands own its pixels through GPU completion. This
avoids the observed browser `writeTexture` corruption during actor uploads.
Neither this target nor a successful screenshot closes a vanilla parity gate.
POV camera and hands use the same native walk bob, turn spring and damage tilt
owners. Damage-direction packets are not in the stream, so committed hurt
events use native directionless tilt at their actual age.
Orbit and follow use Cinnabar's current default projection. POV uses a 110-degree
horizontal field of view; the hand pass receives the resulting vertical FOV.
Browser redraws follow the native continuous Winit/AutoVsync path instead of
a 34 ms reactive timer. The 10 Hz stream is sampled between committed poses:
positions stay aligned across actors, follow/orbit targets and nameplates, while
POV yaw takes the shortest turn and pitch stays within its vertical range.
New duels, arenas, rounds and revived fighters start at their current pose.
No movement is extrapolated beyond the latest committed frame. Converted bone
poses retain one cached pair per actor, invalidated by the native animation and
observation ticks, rig/skin replacement and actor/session lifetime changes.
Biome/light/environment streaming and complete vanilla first-person parity
remain open. The shared native main-hand pass has the same
existing offhand and map restrictions as the desktop path. The admission limits
are described below.

Regenerate carriers against the current pinned sources when updating the renderer;
older world/entity schemas and mismatched actor catalogs are rejected. The font
carrier uses `assets/fonts/CinnanglesSans.ttf` with
`assets/cinnangles-sans-source.json`. Compile server-pack glyphs with
`assetc font-assets --glyph-pack <pack-directory> --compact-pages` to include
custom scoreboard separators without unused atlas padding. Publish all ten
carriers together with their actual hashes in the bundle manifest.

## Binding

```js
import init, { TerrainAssets, Viewer } from './web_spectator.js';
await init();
const terrain = new TerrainAssets(worldBytes, registryBytes, protocol);
const limits = JSON.parse(Viewer.gpu_requirements(terrain));
// Check the WebGPU adapter limits before constructing the viewer.
const viewer = new Viewer('#cinnabar-canvas', terrain, entityBytes,
  actorBytes, particleBytes, equipmentBytes, hudBytes, iconBytes, jsonUiBytes, fontBytes);
viewer.set_arena(JSON.stringify(arena));
viewer.set_frame(JSON.stringify(frame), Date.now());
viewer.set_skin(playerId, width, height, rgbaPixels, slim);
viewer.set_camera('pov', playerId); // also orbit and follow
const status = JSON.parse(viewer.status());
```

`orbit(deltaYaw, deltaPitch, deltaZoom)` accepts radians and logarithmic zoom.
`reset_camera()` restores the default orbit framing around the current duel.
Only camera controls are accepted; they do not control fighters. Invalid
snapshots, assets or skins throw descriptive errors. `status()` returns
`{ready,error,arenaReady,renderedFrames,submittedChunks,renderer,diagnostics}`. Readiness
requires a native GPU-completed terrain draw acknowledgement, rather than just
successful WASM initialization.
The bounded diagnostics record all native GPU acknowledgements, their last
allocation/visibility/draw counts and rejection reasons, native visibility
digests, actor GPU acknowledgements, and HUD upload/draw counts. They report
observations without changing readiness. Terrain publications carry the active
native biome table identity so its revision guard admits their GPU uploads.
`Viewer.gpu_requirements(terrain)` derives texture layers from the carrier and
storage-buffer requirements from the native chunk layout for capability checks.

Arena input is `{id,name,palette,blocks,bounds}`. Entries are `{name,states}`;
index zero must resolve to compiled air. Blocks are `[x,y,z,paletteIndex]`.
Bounds are inclusive minimum XYZ then maximum XYZ. Duplicate positions use
their final value, including air removal. Native mesh admission is 32 MiB JSON,
one million block records, `assets::MAX_TEXTURE_LAYERS` palette entries, 4096
subchunks, absolute coordinates up to one million, 1024 blocks per bounding
axis and 64 MiB of packed mesh output. GPU publication uses the native bounded
queue with at most eight uploads/eight MiB per frame. Terrain meshes once per
arena; live frames update fighters and HUD without remeshing it.

The older `mesh_arena(json)` binding still returns nine-float flat triangle
vertices as a diagnostic geometry tool. The website viewer does not use it.

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

Use an optimized `--release` artifact for deployment; the development build
above is for bounded iteration. Deploy `web_spectator.js` and
`web_spectator_bg.wasm` together under a content-addressed website path and pin
the source commit, actual build profile and WASM hash in its manifest. Validate
rendered frames and cold/warm performance with that exact artifact before push.

## Recorded duel playback

Replay windows use the same canonical renderer as live duels, with immutable
arena and appearance assets. Seeking resets actor interpolation, camera smoothing
and particle emitters. Playback time freezes and scales actor/particle animation;
recorded entity transforms, equipment, HUD and block snapshots remain authoritative.
Sound assets are decoded from the pinned pack once and fetched by verified content
hash on demand, after the viewer enables sound.

Remaining parity work is tracked in plan.md; a compiled viewer is not a visual
parity result.

## Server font sheets and compact carriers

The native HUD uses the authenticated font carrier for both ordinary text and
server glyphs. Build its pinned outline sources with
`assetc outline-font-assets --glyph-pack <server-resource-pack> --compact-pages`
alongside the existing font, fallback-font, source-manifest, out and report
arguments. `--glyph-pack` reuses Cinnabar's session sheet extraction and packing
for `font/glyph_XX.png`; it preserves each glyph's native bearing, advance,
drawn size and colour. Source PNGs and generated carriers remain local assets.

`--compact-pages` keeps every code point and UV rectangle, removing only unused
power-of-two atlas padding. It reduces decoded bytes, verification work and GPU
uploads without removing CJK fallback glyphs. The generated carrier remains
MCBEFONT1 with the pinned font source identity and a new verified payload hash.
