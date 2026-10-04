# Fox cube bind poses

The pinned adult fox sample omits two cube bind rotations which vanilla inherits
from its base model: body +90 degrees X and tail +80 degrees X. Losing them draws
the body vertically and drops the tail into the floor. These transforms belong to
the cubes; rotating the animated bones instead would also displace the head and
legs. `animation.fox.setup` clears the body's animated X angle through Molang
`this`, and cannot restore a missing cube bind.

## Current client evidence

MCSRC's verified current export is preview 1.26.50.26. Its matching executable
SHA-256 is `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
The named 26.30 reference supplies navigation; the contracts below were checked
against current canonical bodies or the matching executable.

- `01e59760` retains the geometry JSON hierarchy and passes it to `06ae8580`.
- `06ae8580` dispatches bone parsing to `06ae88a0`. That parser's current
  reconstructed body is unavailable, so its bind-field read was verified by
  disassembly of the matching executable. VA `146aea6c5` loads
  `bind_pose_rotation`; `146aea71e` calls hierarchy getter `00b95650`.
  `146aea7d8` requires three array entries. `146aea841` through `146aea84b`
  store the converted bind radians separately at node offsets `0x38..0x40`.
- Canonical `00b95650` walks retained hierarchy nodes, continuing into older
  nodes when a field is absent from a replacement. A missing bind in a modern
  replacement therefore retains the base model's bind.
- `01e61dd0` copies ordinary node rotation at `+0x20` into bone defaults and
  passes bind rotation at `+0x38` separately to `01e62d50`. The latter combines
  cube and bind Euler angles and rotates the cube pivot about the part pivot.
  Children retain their authored pivots and animation frames.

## Native resource witness

The installed iOS vanilla pack is 1.26.51, a near-version resource witness,
not an identical-version executable acceptance artifact. Base model archive
`vanilla/__brarchive/models/entity.brarchive`, SHA-256
`e64e48cdfcdecd587e0af787f4b8493875e788fa23ab0d122e35af7c8c99ef4f`,
has `fox.geo.json` at payload offset 65179, length 1723. It declares the body and
tail binds above. Modern replacements in `vanilla_1.21.90` and `vanilla_1.26.10`
retain the adult identifier, pivots and cube shapes but omit the bind fields.
No later installed pack replaces the fox model. The baby model is independently
authored in `baby_fox.geo.json` and has no inherited adult binds.

The correction applies only in vanilla compilation to the exact adult sample
path, identifier and source SHA-256. Its digest is declared once in
`crates/pack-compiler/src/entity/native_bind_pose.rs`. Changed/custom sources,
session packs and explicit bind overrides remain authoritative. No geometry or
animation defaults other than the two absent cube binds change.

## Regression and preparation

The compiler tests cover both cube binds, custom-source exclusion, explicit
overrides, untouched child fields and the baby model. The carrier-to-mesh
regression checks independent model-space bounds: body
`[-3,5,-3]..[3,11,8]`, tail
`[-2,4.0477008,7.5940995]..[2,10.534573,17.32561]`, plus unchanged head and leg
bounds. Local pinned-source tests require `CINNABAR_VANILLA_ROOT`; the mesh test
requires `CINNABAR_ENTITY_CARRIER`. Mojang payloads remain outside git.

Rebuild entity, actor and equipment carriers together for a development launch:
`make entity-assets actor-assets equipment-assets`. Actor and equipment carriers
embed entity-catalog identity. Packaged first-run preparation already hashes
the asset compiler and invalidates these dependent carriers when it changes;
this geometry correction needs no carrier schema change.

## Local verification

On 2026-10-04 the freshly built client rendered a disposable loopback gallery
on Apple M3 Pro Metal, with adult and baby red/arctic foxes on a flat grass floor
at fixed daytime and clear weather. A current-schema carrier compiled from a
whitespace-modified copy of the sample deliberately bypassed the content pin:
adults reproduced the vertical body and downward tail while babies looked normal.
Rebuilding the entity, actor and equipment carriers from the pinned source
restored the adults' horizontal bodies and elevated tails. The user inspected
the corrected live gallery and accepted the foxes, including the adult tails.
Standing appearance was accepted; extended pose/transition and exact-version
native side-by-side acceptance were not performed.

Before the final publication request, the pinned compiler regression passed,
and the carrier-to-mesh regression failed against the uncorrected carrier then
passed against the corrected one. Formatting, architecture and affected-crate
compilation also passed. The affected verifier was stopped during test compilation
at the user's request to skip further tests and publish directly to `dev`;
the complete verification gate is not reported green.

The broader animation gate remains incomplete: fixed-tick Molang sampling,
controller blend transitions and nonuniform parent scale still have the
limitations recorded in `plan.md`.
