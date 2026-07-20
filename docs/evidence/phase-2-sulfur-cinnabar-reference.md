# Sulfur and cinnabar visual reference

This tranche covers the two protocol-1001 states that live diagnostic telemetry identified as the dominant emitted diagnostic geometry:

| Sequential ID | Network hash | State |
| ---: | ---: | --- |
| 12638 | `0xbda02665` | `minecraft:cinnabar {}` |
| 14658 | `0x2d658dd8` | `minecraft:sulfur {}` |

The committed registry records are exact PMMP + Dragonfly + Prismarine joins. Each has one canonical state, a primary contributor, no projected model state, and a unit collision box with collision-only confidence. Dragonfly does not yet provide a concrete visual model for either state, so both records deliberately remain `unknown` with zero render flags. The compiler admission therefore validates the complete two-record identity product instead of treating arbitrary unknown blocks as cubes.

## Pinned Bedrock visual contract

The ignored, EULA-gated Bedrock `v1.26.30.32-preview` resource pack used by the protocol-1001 build supplies a plain scalar block-texture route for each identifier:

- `cinnabar` selects terrain key `cinnabar`, whose only static source is `textures/blocks/cinnabar`.
- `sulfur` selects terrain key `sulfur`, whose only static source is `textures/blocks/sulfur`.

Neither route has a face selector, texture array, tint declaration, flipbook, render method, or geometry override. Both physical sources are 16×16 and every texel has alpha 255. In the Bedrock built-in block route this is one opaque unit cube using the same unrotated texture on all six faces. The pinned PMMP properties independently report opacity 1.0 and brightness 0.0 for both blocks, while the registry collision seed is exactly the unit block.

The compiled contract is therefore:

- `VisualKind::Cube` with no model template;
- `CUBE_GEOMETRY | OCCLUDES_FULL_FACE`;
- one material on all six faces;
- material flags `0` (no UV rotation, tint, cutout, blend, overlay, or animation);
- identical sequential-ID and network-hash resolution.

## Fail-closed boundary

The pair is admitted atomically. Either state is kept diagnostic if the registry identity, canonical state, provenance, collision seed, pack selector, terrain path, static scalar cardinality, flipbook exclusion, dimensions, or fully opaque alpha contract differs. No Mojang texture payload is committed; tests synthesize color tiles and the ignored pinned-pack test reads the local EULA-gated sources.
