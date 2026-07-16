# Phase 4.2 Actor Rendering Design

**Scope:** Render remote player actors from the bounded Phase 4.1 store with delayed, time-based interpolation and server-supplied classic skin pixels. Persona geometry, Molang, name tags, equipment, and non-player entities remain out of scope.

## Chosen architecture

`PlayerList` is the only protocol-1001 packet in the current generated model that exposes player skin bytes; `AddPlayer` supplies the UUID that joins the actor to that roster entry. The protocol layer will normalize a bounded classic skin payload or an explicit unavailable reason. Persona skins are retained only as `UnsupportedPersona`; malformed dimensions/lengths and per-packet budget overflow are retained as deterministic unavailable states rather than silently treated as classic skins.

The app actor store keeps the normalized skin status beside the bounded player profile. It exposes a deterministic, runtime-ID-ordered list of player render sources. A render-owned scene keeps at most 128 remote-player histories with at most two timed poses per actor. Samples are evaluated 100 ms behind the current render time, clamped at the ends, with shortest-path angle interpolation. Teleports replace both endpoints and therefore never smear. This state is driven only by actor packets and `Time<Real>`; free-camera and local movement state are not inputs.

Rendering remains in the compact custom architecture. One `Opaque3d` non-mesh draw expands six Bedrock biped cuboids in WGSL, instances all visible players from one storage buffer, and samples one 64x64 texture-array layer per instance. The main world publishes a bounded frame; the render world uploads one instance buffer and one skin array only when their content identities change. There is no `StandardMaterial`, no Bevy `Mesh` per actor, and no per-frame entity/mesh churn.

## Geometry and skin contract

The landed geometry is the standard static base-layer biped: head (8x8x8), torso (8x12x4), two arms (4x12x4), and two legs (4x12x4), at 1/16 block per model pixel. UVs use the 64x64 classic layout. Supported server images are square classic atlases whose side is 64, 128, or 256 pixels; larger atlases are deterministically nearest-sampled to 64x64. Legacy 64x32 and custom geometry/persona forms are explicit fallbacks in this slice.

When usable server pixels are absent, the renderer uses a source-authored `Cinnabar Default` skin generated locally from named colour regions. It is bounded to one 64x64 RGBA layer, contains no Mojang bytes, and is not a diagnostic/checkerboard texture.

## Failure and reset behavior

- Non-finite render sources are excluded.
- More than 128 player actors are truncated after stable runtime-ID ordering.
- Roster removal, actor removal, dimension reset, and session reset remove their render history on the next publication.
- Missing, malformed, legacy, over-budget, or persona skin data maps to the documented default layer.
- A texture-array upload or pipeline preparation failure leaves the actor draw absent; it cannot mutate chunk meshes or camera authority.

## Verification

Focused tests cover protocol skin normalization and bounds, store profile joining, bounded histories, delayed position/angle interpolation, teleport snapping, stable truncation, standard biped cuboid/UV descriptors, skin normalization/default provenance, plugin idempotence, and GPU shader validation. Strict formatting and workspace Clippy run before the local commit.

## Landed implementation status

The standard remote-player slice above is implemented. Automated verification covers the packet-to-roster-to-render-source chain and the compact pipeline contract. No live two-client/BDS visual capture was performed in this isolated tranche, so visual evidence remains an explicit follow-up rather than an implied acceptance result.
