# Phase 2 bee housing reference

Date: 2026-07-14

Protocol: 1001

No Mojang assets or screenshots are tracked by this manifest.

## Reviewed sources

- Cinnabar base: `6c9da26664cd9ce18c895161dc4bdcfa7d153e9f`.
- Axolotl registry evidence: revision
  `6f6806f6e61130edbb52a087aef4593bb087ab9f`; `BeehiveState` has exact
  `direction` values 0 through 3 and `honey_level` values 0 through 5.
- Dragonfly registry evidence: revision
  `b85c56fb680d6d9a6a829b7ec21b174c84f22f78`; horizontal protocol direction
  is south, west, north, east for values 0, 1, 2, 3 respectively.
- The ignored Bedrock sample resource pack is pinned to tag
  `v1.26.30.32-preview`. Its block maps explicitly route the unrotated front
  to south. `bee_nest` uses distinct bottom and top keys; `beehive` uses its
  top key on both vertical faces.

The local-only pack data was inspected in place. It was not copied into this
repository.

## Exact state and visual contract

- `minecraft:bee_nest`: sequential IDs 10,395 through 10,418.
- `minecraft:beehive`: sequential IDs 12,495 through 12,518.
- Within each family, sequential offset is `honey_level * 4 + direction`.
- Both selectors are typed `int`; aliases, wrong types, missing/extra keys,
  out-of-range values, duplicate product members, and projection disagreement
  reject the complete 48-state admission.
- All states retain the canonical unit-cube collision seed, six-face coverage,
  full-face occlusion, and the eight-byte packed cube path. No model template,
  per-block mesh object, or block-entity renderer is introduced.
- Direction rotates the front south, west, north, east for values 0, 1, 2, 3.
- Honey levels 0 through 4 use the ordinary front; level 5 alone uses the
  honeyed front.

The resource-pack route is also atomic. Exact six-face `blocks.json` maps are
required. Static keys must use the pinned singleton-array terrain form, fronts
must use exactly two ordered variants, all paths must match literally, and
tint, alias/extra metadata, or flipbook participation rejects the family.

## Deterministic results

| Artifact | SHA-256 |
| --- | --- |
| `block-registry-v1001.bin` | `bbd430a773d1d93d772178fc974abef4787b2e15a90754b7bccf98776e821826` |
| `block-light-registry-v1001.bin` | `3144acb7ccafd48eb45c6b25ddce338148f02ad06dd4243b1f1d5eb95182a04f` |
| ignored real-pack `MCBEAS05` witness | `4880fd066be66983a08ba93cdcde60a71b9f61beb705e8b4160705d69c15d14e` |
| `visual-coverage-v1001.json` | `7fdd59e9cd34e87865b651021b033cdc9922c2ce6266c6cfb1aa003e98628d1d` |

The production visual-coverage delta is exactly 48 removals and zero
additions: IDs 10,395..10,418 and 12,495..12,518. Global diagnostics shrink
from 2,448 to 2,400, and neither bee family retains a diagnostic state.

Automated render evidence covers all 48 states in sequential and hashed
network-ID modes, compact cube-only streams, dense greedy six-quad closure,
closed cave connectivity, and culling on all six cross-subchunk boundaries.
Bee occupants, honey inventory NBT, and live block-entity updates are outside
this state-geometry tranche and remain explicitly open in the Phase 2 block-
entity manifest work.

## Verification

- `go test ./tools/registrygen`
- `cargo test -p assets -p render -p visualcoverage --all-targets --locked --no-fail-fast`
- The ignored exact pinned-pack compiler test, explicitly enabled with
  `PINNED_VANILLA_PACK`.
- The ignored full-production ratchet and 2,400-diagnostic gallery-inventory
  tests, explicitly enabled against the ignored `MCBEAS05` witness.
- `cargo clippy -p assets -p render -p visualcoverage --all-targets --all-features --locked -- -D warnings`
- `cargo fmt --all -- --check` and `git diff --check`.
- Independent repeat generation produced byte-identical BREG and LREG hashes.

All listed gates passed. No native screenshot is claimed for this tranche;
visual/native gallery closure remains part of the global Phase 2 acceptance
item rather than evidence for a different block-state mapping.
