# Exhaustive Vanilla Visual Coverage Design

Date: 2026-07-13

## Goal

Prevent any canonical vanilla block state from silently falling back to the pink diagnostic visual, and prove that every supported block state and block entity reaches the correct bounded CPU/GPU route. Structural codec validity alone is insufficient: `MCBEAS04` deliberately permits `VisualKind::Diagnostic`, so a missing family such as `minecraft:vine` can otherwise pass every blob-integrity test.

The protocol-1001 corpus is exact: 1,356 names, 16,913 canonical states, one air state, and 16,912 non-air states. Block entities are a separate namespace and must never be folded into this count.

## 1. Immediate diagnostic ratchet

Add a workspace tool, `visualcoverage`, which uses the production `assets::read_registry` and `RuntimeAssets::decode` implementations. It must not duplicate BREG or MCBEAS parsing.

`visualcoverage ratchet` accepts the exact BREG, MCBEAS, and a reviewed protocol baseline. The baseline binds:

- protocol number and exact BREG SHA-256;
- exact state/name/air counts;
- sorted canonical state identity for every sequential ID;
- sorted diagnostic state identities and diagnostic counts by family and name;
- a source-cited allowlist of intentionally invisible states.

The ratchet fails if a state is added, removed, duplicated, reordered outside a deliberate protocol baseline update, if a previously working state becomes diagnostic, or if a diagnostic is hidden by changing it to `Invisible` without allowlist authority. Diagnostic-set shrinkage is allowed and reported as an exact diff. The first baseline explicitly contains all currently unfinished states, including all 16 `minecraft:vine` masks; the vine implementation flips that family assertion to zero diagnostics.

The checked report is deterministic JSON. It records both input hashes, exact state identity, totals, per-family/name diagnostics, added/removed diagnostics, and invisible-allowlist decisions. Synthetic tests cover missing, duplicate, and non-contiguous IDs; BREG/blob mismatch; regression and shrinkage; invisible laundering; and byte-identical output.

## 2. Final strict state and asset gate

`visualcoverage strict` is a required Phase 2.6 merge gate. It proves:

### Canonical inventory

- exactly 1,356 unique names and 16,913 unique `(name, canonical state)` pairs;
- sequential IDs exactly `0..16_912`;
- exactly one air state and 16,912 non-air states;
- sequential and hashed lookup modes resolve every record to the same visual and light facts;
- no unknown or unsupported model family.

### Visual status

- zero diagnostic non-air states;
- air resolves to explicit no-draw behavior;
- every other invisible state is in the source-cited allowlist;
- a visible state cannot satisfy the gate through missing or empty draw data.

### Transitive graph integrity

- cubes reach six valid nonzero materials;
- crossed/model visuals reach a valid nonempty template and every reached quad has a valid nonzero material;
- liquids reach the required still/flow materials and valid animation descriptors;
- every animated frame reaches a real page/layer;
- every texture reference is within its declared page and layer bounds;
- no non-diagnostic visual transitively reaches diagnostic material/texture slot zero;
- invisible states carry no accidental drawable references;
- expected visual kind, material flags, tint class, animation route, and render-stream mask agree.

The pinned real pack is compiled twice and both MCBEAS bytes and strict JSON reports must be byte-identical. The report binds the BREG and MCBEAS hashes together even before a future blob schema embeds the registry hash directly.

## 3. Paged exact-state GPU galleries

Sort the canonical BREG records by sequential ID and divide them into fixed logical pages of 256 target states: 66 full pages and one 17-state page, exactly 67 pages total.

Each family-aware page builder may add uncounted support/context blocks, but every canonical target appears on exactly one page. Multipart, neighbor-derived, liquid/waterlogged, attachment, crop-support, and invisible families use reviewed builders rather than isolated `setblock` guesses.

Each page manifest binds:

- page and target index;
- absolute target coordinate;
- sequential ID, network hash, name, and canonical state;
- expected visual kind, render stream, and template/material identity;
- BREG, MCBEAS, world, layout, request, and camera hashes.

A successful placement command is not proof. The harness must read back the loaded palette/state and reject BDS normalization. The Rust world store must witness the exact state identity, meshing must witness the expected cube/model/liquid/no-draw route, and the renderer must prove `requested = committed = encoded = presented` for that exact target set. Each page requires two adjacent GPU-completed frames with identical manifests and zero diagnostic material in any submitted drawable range. Explicit invisible states require a no-draw witness.

Family-appropriate fixed camera poses receive fresh native `%TEMP%` screenshots. Screenshots remain untracked and are inspected through Computer Use or the documented native screenshot fallback. GPU presentation proves routing, not visual correctness, so representative vanilla-reference comparisons remain mandatory for each reviewed family generator.

## 4. Block-entity coverage

Block entities use a separate generated inventory and reviewed renderer manifest.

Generate `block-entities-v1001.json` from pinned Dragonfly/BDS registrations plus real protocol fixtures. Each canonical entry includes its NBT `id`, backing block names/states, source/version/hash, required NBT variants, chunk-NBT support, live `BlockEntityDataPacket` update support, renderer class, implementation symbol, gallery builder, and witness IDs.

The renderer manifest classifies each entry as static block model, custom geometry, text overlay, animated, or sourced logical/invisible. Phase deferrals are explicit statuses and cannot satisfy the final strict gate.

Coverage fails for source IDs absent from the renderer manifest, manifest-only IDs, ambiguous aliases, missing chunk/update paths, missing required NBT variants, unsupported renderers, or missing GPU/no-draw evidence. Until this inventory is generated, the proven block-entity renderer count is reported as unknown/zero proven rather than inferred from packet-codec presence.

## 5. Performance and provenance

The tool and gallery operate on bounded production formats and never commit Mojang assets or screenshots. Runtime state remains palette-native. The exhaustive gallery is an acceptance job, not a per-commit unit test; the fast ratchet and synthetic strict tests run in ordinary CI. Real-pack strict coverage runs in the asset-capable required job.

All reports are deterministic, hash-bound, and reject stale BREG/MCBEAS pairings. State totals may only change through the deliberate protocol-bump workflow.

## Acceptance

Phase 2 visual coverage is complete only when:

1. `visualcoverage strict` reports all 16,913 canonical states with zero non-air diagnostics and a reviewed invisible set;
2. all 67 exact-state pages have two-frame GPU-completed evidence and inspected native screenshots;
3. every block-entity source ID is implemented and witnessed according to the reviewed manifest;
4. real-pack compilation/reporting is deterministic and no Mojang payload is tracked.
