# Phase 4 — Entities and other players

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


Scope: actor lifecycle packets, metadata/attributes, movement interpolation, biped rendering
with standard + persona skins (skin data arrives via PlayerList/AddPlayer), name tags, vanilla
mob geometry + textures from bedrock-samples, **molang subset** for vanilla animation
controllers (walk cycles, look-at; documented cut-line, static pose fallback), item entities
and dropped-item rendering, paper-doll first-person arm/held item.

**Phase 4 progress (kept current as work lands):**

- [x] **4.1 Bounded actor lifecycle ingestion.** The protocol layer now materializes and
  normalizes `AddPlayer`, `AddActor`/`AddEntity`, `RemoveActor`/`RemoveEntity`, absolute and
  delta actor movement, `SetActorData`/`SetEntityData`, `UpdateAttributes`, and `PlayerList`
  into vendor-neutral events. All retained identifiers, names, metadata/NBT, attributes,
  modifiers, properties, and roster collections have explicit limits and finite-number
  validation. The app owns a sparse runtime-ID actor store with a unique-ID removal index,
  atomic duplicate replacement, bounded player roster, FIFO/session/dimension rejection, and
  dimension/session reset semantics. Foreign `MovePlayer` packets route to this actor stream
  without entering the local-camera path, and actor updates do not dirty chunk/light/mesh
  state. This substep deliberately does **not** implement entity rendering, interpolation,
  skins/persona data, Molang, name tags, or item/first-person visuals; those remain open Phase 4
  work.

- [x] **4.2 Standard remote-player render slice.** `PlayerList` now retains explicitly bounded
  classic 64x64, 128x128, or 256x256 RGBA skin images, with deterministic unavailable states for
  persona, malformed, or retained-budget-exhausted data; `AddPlayer` UUIDs join those roster
  profiles to the sparse actor store, whose cumulative retained skin bytes remain capped across
  incremental roster packets. Foreign `MovePlayer` metadata preserves head yaw, ground state,
  mode, and signed source tick while retaining explicit packet-coordinate origin. `AddPlayer`
  and `AddActor` remain feet-space, while `MovePlayer` and absolute actor movement are normalized
  exactly once after retained actor kind and metadata are known. Standing players use Bedrock's
  `1.62001` offset; sleeping players use `0.2` only when retained sleeping flags prove the pose;
  retained item actors, falling blocks, and the reviewed vanilla minecart identifiers use `0.5`;
  `minecraft:boat` uses `0.375`; and primed TNT uses half its retained bounding-box height with
  the vanilla `0.49` default. The
  separate `AddItemEntity` spawn packet still awaits ingestion. Partial/delta movement and actors
  without a reviewed offset remain unchanged. The client-world store performs
  Oomph-style three-20-Hz-tick player convergence;
  teleports and actor replacement snap immediately, and the renderer performs the distinct
  adjacent-tick frame interpolation with shortest-path angles. Camera-frustum and conservative
  192-block distance culling happen before the runtime-ID-ordered 128-instance upload cap, so
  invisible players cannot displace visible ones. This path consumes no free-camera or local
  movement state and sends no free-camera position updates. One
  custom `Opaque3d` instanced draw expands a shared six-cuboid standard
  Bedrock biped vertex buffer and samples a bounded 64x64 texture array, with no `StandardMaterial`
  or per-actor Bevy mesh. Missing/unsupported/invalid skins use the documented, locally generated
  `Cinnabar Default` skin (no Mojang or diagnostic bytes). Focused protocol/app/render tests,
  shader parsing, no-op-backend binding-layout validation, pipeline specialization, format, and
  warnings-denied workspace Clippy are green. Persona/custom geometry,
  legacy 64x32 skins, outer skin layers, limb animation/Molang, name tags, equipment, mobs/items,
  first-person visuals, live render-pipeline creation on a hardware backend, and multi-client
  visual evidence remain open Phase 4 work.
  The invisible-player capture fixes retain PlayerSkin updates, polygon bodies, inflated planes,
  named classic models and native skin texels, and apply vanilla classic alpha validation.
  Persona face/body atlases now retain their own geometry and texture and follow the pack
  animation rate and blink controller. Player appearance parity remains incomplete: local
  piece/tint assembly, repository trust/fallback decisions, geometry version upgrades and persona
  atlases above the admitted size still need validation. Authored skin visibility bounds now reach
  render, cave and animation culling; complete transformed-bounds parity still needs evidence.
  Offline capture renders establish coverage; they do not close native visual parity.
  The complete absolute-movement origin correction, regression suite, independent review, and
  post-merge protocol/client-world/app verification are green through `e7c85ea`; the LBSG live
  ground-contact witness remains open under 4.4.

- [ ] **4.3 Data-driven Bedrock entity rigs and animation.** `P4.3-RIGS` Ingest the pinned
  vanilla resource pack's `entity`, `models/entity`, `animations`,
  `animation_controllers`, `render_controllers`, and `textures/entity` trees as
  bounded compiled assets. Evaluate only the reviewed Molang subset needed by
  those controllers, drive poses from protocol metadata/attributes and the
  20-Hz actor state, then perform the distinct adjacent-tick frame
  interpolation in the renderer. Preserve shared geometry/material/texture
  storage and bounded per-frame actor work.
  **Bounded asset-catalog tranche complete (2026-07-16):** the pinned vanilla
  `entity`, geometry, animation, animation-controller, render-controller, and
  entity-texture trees now compile into the deterministic `MCBEENT3` carrier
  with exact source-manifest provenance. The real reviewed pack produces 3,247
  source records, 2,993 symbols, and 3,071 dependency edges (2,929 internal and
  142 explicitly external); duplicate identifiers remain selectable candidates
  rather than silently choosing a generation. Texture identifiers and
  conditional render-controller keys resolve canonically, startup fails closed
  on stale provenance, and generated carriers/reports and Mojang payloads remain
  ignored. Independent review and post-merge assets/compiler/client tests and
  strict checks are green. **Geometry payload tranche complete (2026-07-16):**
  the carrier now preserves bounded, deterministic geometry, bone, cube,
  pivot/rotation, mirror, inflate, and UV payloads. Legacy inheritance resolves
  sparse overlays through the selected parent chain, exact reviewed legacy/modern
  schema versions fail closed, and geometry JSON rejects duplicate semantic keys
  recursively. The complete `105107d..d84667d` behavior range received fresh APPROVE
  after all three Important review findings were fixed and landed as merge `1e4ba3c`.
  The subsequent behavior-preserving compiler split through `4a6696b` independently
  passed review and architecture enforcement and landed as merge `73b8de7`.
  Animation clip payloads, the reviewed Molang/controller evaluator, runtime rig
  consumption and skeletal GPU skinning/posing, and native animated-actor evidence
  remain open.
- [ ] **4.4 Live actor ground-contact and interpolation witness.** `P4.4-LIVE-ACTOR` Join
  `play.lbsg.net:19132` with the normal authenticated core, observe at least one
  remote player's spawn, ordinary movement, rotation, and teleport, and prove
  that AddPlayer/MovePlayer origins, three-tick convergence, frame
  interpolation, and the shared biped model keep both feet on the same ground
  plane without a 1.6-block jump. Keep the visual standing eye height (`1.62`)
  distinct from Bedrock's standing-player movement network offset (`1.62001`).
  Capture bounded native visual and packet/pose evidence.

- [ ] **4.5 Held items, actions, dropped items, and viewmodel.** `P4.5-ITEM-ACTIONS`
  - [ ] Pickup sound and the three-tick copied-item flight survive immediate server
    removal. The squared trajectory and collector offset follow the current vanilla
    rules; a version-matched rendered comparison remains incomplete.
  - [x] Render supported ordinary opaque full-cube held blocks using the current
    selected stack, world materials, and session-bound local presentation authority.
    A live controlled check covered Stone, distinct crafting-table faces, empty-slot
    and unsupported-item fallback, resize, and reconnect. Current GPU completion
    suppresses only the matching CPU fallback; rejected or stale submissions retain it.
    This is a static geometry slice, not complete item, pose, animation, lighting,
    or matched retail visual parity. Tinted, animated, partial, and unsupported
    items remain on the existing fallback path.
  - [ ] Server custom opaque cubes now use the shared block-item face sheets for
    inventory and held rendering. Preserving higher-resolution custom face textures
    is incomplete: the sheet builder currently selects the mip matching
    `BLOCK_ITEM_FACE_SIDE`. Custom shapes, tinting, animation, and matched retail
    visual parity remain incomplete as well.
