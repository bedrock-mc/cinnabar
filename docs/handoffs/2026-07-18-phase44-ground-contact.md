# Phase 4.4 presented ground-contact handoff

Branch: `phase44-presented-ground-contact`

Base: `origin/phase2-textures` at `d026417`

Status: incomplete work-in-progress checkpoint.

## Composed dependencies

- `eb82812` / `0c0217c`: equivalents of the independently approved actor-store
  interpolation witness (`3 -> 2 -> 1 -> 0`) and its preservation fix.
- `592453f` / `ad0a8fd`: equivalents of reviewed Phase 4.3 production rig
  publication and local-avatar preservation.

## Current implementation

A red test proved the former actor acknowledgement could treat skipped draw/frame
generations as consecutive. Work in `crates/render/src/actor/gpu.rs` and
`crates/render/src/actor/rig.rs` is extending the fixed-size draw acknowledgement
with manifest-aligned, bit-exact feet/world-root position and partial-tick data,
plus stronger lifetime, completed-tick, ingress, and adjacent-generation checks.

## Remaining work

- Finish the render-contract implementation and compile it.
- Add the bounded app-side correlator/resource and injection tests.
- Preserve session, dimension, revision, lifetime, teleport, and replacement
  isolation and reject non-finite or mismatched evidence.
- Run client-world/render/app suites, strict Clippy, formatting, architecture,
  independent review, and resource-bound checks.
- Run authenticated LBSG evidence for spawn, ordinary movement, rotation, and
  teleport, including two consecutive presented frames with both feet on the
  same ground plane.

The dirty render changes are not green and must not be merged as-is.

