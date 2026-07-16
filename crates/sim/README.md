# Bedrock movement simulation foundation

This crate is the deterministic, protocol-independent base for Phase 3 movement. The app now
samples manual input into this fixed-tick simulation, collides against the packed world, and
interpolates the local camera. This crate still does not emit `PlayerAuthInput`, read the render
camera, or make free-camera movement authoritative. Network authority remains gated until the
parity work below is complete.

## Implemented contract

- One `Simulator::tick` call advances exactly one 20 Hz tick and is transactional on errors.
- The player origin is at the feet. The collision box is 0.6 blocks wide and 1.8 blocks tall,
  with bedsim's `1e-4` horizontal inset.
- Basic walk, sprint, jump, sneak input scaling, ground/air acceleration, gravity, drag,
  runtime-specific surface friction, swept AABB collision, Y-X-Z resolution, and 0.6-block
  stepping follow the pinned bedsim behavior.
- `PaletteWorld` queries the packed `world::ChunkStore` directly. It fails closed for unloaded
  chunks, unknown runtime IDs, invalid bounds, and invalid collision-registry data rather than
  guessing air/full-cube behavior. Queries are capped at 128 blocks per axis, and registered
  local shapes must fit the exact `[-1, 2]` one-block-halo coverage contract.
- `PredictionHistory` retains a bounded tick-keyed input/state history and transactionally
  replays later inputs after a correction. Protocol-specific eye/feet/delta conversion and
  collision-world snapshot versioning deliberately remain outside this crate.
- The strict JSONL conformance reader rejects unknown fields, discontinuous ticks, non-finite
  values, and any state mismatch beyond its caller-provided epsilon.

## Pinned evidence

The behavioral reference is `github.com/oomph-ac/bedsim v0.1.3`, source commit
`5be9149df14e30c0ab14f9e01d51dd2acfee5230`, module checksum
`h1:tWZ7O48DL/SaWIY+0zz0hFln+DXN4vfatqKr8zTHVo8=`. The generated fixture's provenance and
SHA-256 are recorded in `fixtures/bedsim-v0.1.3-basic.provenance.json`.

Regenerate the bounded basic trace from `tools/bedsimtrace`:

```powershell
$env:GOWORK='off'
go run .
```

The Rust conformance test compares its per-tick state against that output at `1e-12` epsilon.

## Remaining Phase 3 work

- Generate and verify the authoritative runtime-ID collision registry for every supported block
  state, including compound shapes and friction/surface semantics.
- Port the rest of bedsim's behavior: sneak edge avoidance, item-use slowdown, climbing,
  cobwebs, liquids/swimming, slime/bed bounce, effects, knockback, teleport handling, gliding,
  and special block/game-mode behavior.
- Add dynamic movement attributes and bounding-box state from
  `ClientMovementPredictionSync`, plus authoritative snapshot identity for correction replay.
- Expand the pinned trace corpus across slabs, stairs, fences/walls, climbables, liquids,
  collisions, and correction scenarios; then validate on vanilla parkour and Lunar-fronted
  servers.
- Prove snapshot-versioned correction replay and the remaining collision/behavior contracts,
  then authorize the already-separated outbound movement scheduler. Freecam must remain
  network-silent.
