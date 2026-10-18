# Retained acceptance requirements

[Roadmap](../../plan.md)

These requirements retain their recorded scope. They are not new evidence of completion.
Use current repository instructions for commands and ownership.

## Sequencing and program rules

- Order is 0 → 1 → 2 → 3 → (4 ∥ 5) → 6 → 7 → 8. Phases 4 and 5 can run in parallel worktrees once 3 lands (disjoint crates; both consume `crates/world` + `crates/protocol` which are stable by then).
- Each phase starts by converting its scope block into a full task-by-task plan (superpowers:writing-plans), gets brainstorm-level review if its scope shifted, and ends with the requesting-code-review flow. PR-bot adjudication rules apply throughout.
- Every phase must leave `main` in a runnable state (`app` launches, joins BDS, does everything prior phases delivered) — CI runs the Phase 0 acceptance connect as a smoke test forever.
- Protocol bumps during the program: deliberate, one task, lockstep — regenerate valentine defs, run conformance, bump core, bump `registrygen` exports, fix findings. Never mid-phase.

## v1 Definition of Done

From a clean machine: install → sign in with Xbox (device code) → join a third-party RakNet
server, a Realm, and a friend's world (NetherNet) → play survival basics (move, build, mine,
chest, craft, chat, forms) with vanilla look and feel at 60fps on the dev MacBook → create a
local dragonfly world, play it offline, reload it. Server resource packs render. No Rust-side
auth/transport code exists.

---

## Appendix: Rendering Performance Playbook (binding for Phases 0 and 2)

FPS and memory in this client are dominated by chunk meshes; these techniques stack
multiplicatively and are the required approach, not suggestions:

1. **Paletted chunk data stays paletted at runtime.** Mesh directly from palette + packed
   indices; never expand to flat per-block arrays (the naeast2 lesson, client-side). Uniform
   subchunks (all air/all one block) store one palette entry and skip meshing entirely.
2. **Binary greedy meshing.** Per-axis-column `u64` bitmasks; face culling and coplanar
   merging via bitwise ops (target: tens of µs per subchunk, making remesh-on-update ~free).
   Merges split where baked AO/light values differ. References: `block-mesh` crate and
   TanTanDev binary-greedy-meshing demos.
3. **Packed vertices / per-quad vertex pulling.** Local position 5+5+5 bits, face ID 3 bits
   (normal from LUT), texture-array layer index, AO 2 bits, light 8 bits → 1–2 `u32` per
   vertex, subchunk origin as a per-draw push constant. Preferred form: one ~8-byte record
   per quad in a storage buffer, corners reconstructed in the vertex shader, and one shared
   static index buffer for all chunks. This targets roughly 20–40× less mesh memory than
   naive 32-byte vertices.
4. **Custom Bevy render phase for chunks.** No per-subchunk `Mesh`/`StandardMaterial`; use
   one chunk pipeline family with at most two immutable state variants
   (opaque/cutout with depth writes, blend without depth writes) and one shared bind group,
   with `multi_draw_indirect` where available.
5. **Visibility culling.** Per-subchunk frustum culling + cave/connectivity culling
   (Checchi-style: face-to-face connectivity flood-filled at mesh time, then BFS from the
   camera through the chunk graph—the approach used by vanilla).
6. **Budget spiky work.** Decode/mesh/light only on Rayon workers; GPU uploads capped per
   frame and nearest-first; light updates deduplicated and queued; block + sky light baked
   per vertex at mesh time so lighting cost rides the remesh budget.
7. **2D texture arrays, not a stitched atlas.** This avoids mip bleeding, permits greedy-quad
   UV wrapping, and implements flipbooks as layer swaps; mipmaps are generated per layer.
   Use one measured physical array when the reachable deduplicated layer inventory fits the
   minimum target adapter, otherwise at most two equal-format array pages in the same shared
   bind group. More pages, frame dropping, or silent animation degradation are forbidden.

Explicitly deferred past v1: GPU
occlusion queries (cave culling suffices), and mesh shaders.

Resource budget (tracked from Phase 2 onward; reference machine class = Ryzen 5 3600 / mid
Apple Silicon, 16-chunk radius, capped 60fps): combined RSS (client + core) ≤ 650MB
steady-state; steady-state CPU ≤ 15% total; join/teleport bursts may saturate cores but must
settle within ~2 seconds. Baseline for comparison: vanilla Bedrock client on the same
machine runs at 800MB–2GB and 30%+ CPU.

Binding Phase 2 scope: block registry + block-state → model/texture mapping (generated
export from Dragonfly's registry via `tools/registrygen`, shipped as a binary asset, with
pinned PMMP BedrockData as the exact protocol-1001 canonical palette/property/biome
cross-check, Axolotl Valentine's typed state catalog as a versioned selector reference, and
Axolotl's exact pinned PrismarineJS Bedrock collision shapes as reviewed cuboid-template and
occlusion inputs rather than render/UV authority); vanilla
asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version
(terrain textures, `blocks.json`, flipbooks, and biome colors). The pinned samples have no
block-render model JSON, so deterministic reviewed family generators combine collision
bounds, Dragonfly behavior rules, Mojang texture mappings, and vanilla-reference evidence.
Zuri is not a rendering or asset-system input. BDS
`resource_packs/vanilla` is server-minimal (`blocks.json` + texts only): it is a data
reference, not the texture source. Use the bounded one-or-two-page 2D texture-array scheme
above with per-layer mipmaps; meshing per
this playbook with opaque/cutout/blend layers; a client-side block + sky flood-fill light
engine with per-vertex light baked at mesh time and day/night; biome tinting for
grass/foliage/water; sky, fog, and clouds; chunk streaming/eviction tied to
`ChunkRadiusUpdated` + `SubChunk` request flow. Custom block-entity renderers remain
deferred; chests/signs receive static models in this phase. The Phase 0 performance budget
carries forward, with full remesh of view distance after teleport ≤ 2 seconds.
