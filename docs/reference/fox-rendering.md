# Fox cube bind poses

The pinned adult fox sample omits two cube bind rotations which vanilla inherits
from its base model: body +90 degrees X and tail +80 degrees X. Losing them draws
the body vertically and drops the tail into the floor. These transforms belong to
the cubes; rotating the animated bones instead would also displace the head and
legs. `animation.fox.setup` clears the body's animated X angle through Molang
`this`, and cannot restore a missing cube bind.

## Vanilla rules

For the 1.26.50.26 client:

- Geometry parsing retains the JSON hierarchy of base and replacement models.
- Each bone reads `bind_pose_rotation` through that hierarchy, requires three
  entries, and stores the converted bind radians separately from the ordinary
  rotation.
- The hierarchy lookup continues into older nodes when a field is absent from a
  replacement. A missing bind in a modern replacement therefore retains the base
  model's bind.
- Ordinary rotation becomes the bone default, while bind rotation is applied
  separately to cubes: cube and bind Euler angles combine, and the cube pivot
  rotates about the part pivot. Children retain their authored pivots and
  animation frames.

## Scope

The adult body and tail retain the cube binds above when a replacement omits
them. The baby model in `baby_fox.geo.json` has no inherited adult binds.

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
vanilla side-by-side acceptance were not performed.

Before the final publication request, the pinned compiler regression passed,
and the carrier-to-mesh regression failed against the uncorrected carrier then
passed against the corrected one. Formatting, architecture and affected-crate
compilation also passed. The affected verifier was stopped during test compilation
at the user's request to skip further tests and publish directly to `dev`;
the complete verification gate is not reported green.

The broader animation gate remains incomplete: fixed-tick Molang sampling,
controller blend transitions and nonuniform parent scale still have the
limitations recorded in `plan.md`.
