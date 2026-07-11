# Vanilla Texture Vertical Slice Progress

Plan: `docs/superpowers/plans/2026-07-10-vanilla-texture-vertical-slice.md`
Branch base: `d7711e0`

Task 1: complete (commits 6e951ae..ee1b1ce, review clean; offline PowerShell/Git Bash contracts green; no Mojang payload tracked)

Task 2: complete (code integrated at e024dc4; exporter tests/vet green; 16,913 records and deterministic SHA-256 f3b8689d7be5189d503a26f47224939d83a441a05d9259806c1fd08d1753e07c independently verified; no PNG or asset JSON payload markers)

Task 8 diagnostic checkpoint: complete (commit ba0b39d; binding `teleport_settle_ms` remains independent from secondary `forced_full_view_remesh_ms`; stage diagnostics and deferred resource sampling reviewed clean; exact cohort/GPU fence still open)

Task 8A exact cohort: complete (commit 33889d9; 117 tests + strict clippy/fmt green; independent review clean; exact 1,089-column target/source exclusion and blocker progress evidence; GPU/present fence still open)

Task 8B presented-frame gate: complete (render commit 2f27eed, client commit d84ca15; 32+14+9 render and 102+17+10 client tests, strict clippy/fmt green; exact manifest, adjacent GPU-complete frames, and FIFO source capture independently reviewed clean; live Task 8C evidence still open)

Task 8C acceptance validator: complete (commit 734fa57; target-prefixed exact proof parsing, cohort/manifest continuity, independent 2s gates, interval frame cap, and recomputed schema-v2 30-s resource evidence; PowerShell suite and independent review clean; Rust proof emission remains open)

Task 8C Rust proof emission: complete (commit 16fc4bb; exact resident-plus-known-air forced manifest, independent teleport/remesh proof JSON and markers, adjacent GPU-complete frames, deterministic hashes/null stages, and interrupted-probe reset; client 116+19+10, render 32+14+9, acceptance, strict clippy/fmt/diff green; schema and runtime reviews clean; fresh live evidence remains open)

Cutout-leaves implementation plan: complete/integrated (commits 502b738, e729603, c81af2e; final independent review clean; implementation intentionally follows Task 8 acceptance closure)

Task 8E omitted-SubChunk lifecycle: complete (commit db24278; per-Y two-second deadlines, two exact-Y retries, terminal no-air completion, FIFO-preserving bounded direct/deferred scheduling, eviction/inline cleanup, exact progress stats; 110+17+10 client tests, acceptance dry-run, and strict clippy/fmt/diff green; first review FIFO Important plus explicit-transient equivalent fixed, focused final re-review clean)

Task 8E live retry correction: complete (commit a9cbb12; transport-confirmed deadline arming, inbound-first network fairness, submit-time reply protection, bounded overlapping-attempt correlation, and first-fatal preservation; client 151, render 55, protocol 94, acceptance, strict clippy/fmt/diff green; independent review clean). Live runs 20260711T102936Z-8188 and 20260711T103334Z-39712 proved the pre-fix retry storm and local batch >1600 failure; a defensive Go producer cap remains open before rerun.

Task 8E local relay batch cap: complete (commit 1434bd6; upstream-BDS to local-Rust batches are flushed at an exact 1,600-packet ceiling without changing the reverse Lunar relay path; full Go tests, vet, gofmt, diff checks, and independent review clean).

Task 8E live network round-robin: complete (commit 02d9e4b; connection-local deterministic inbound/command alternation prevents both inbound and outbound starvation while preserving strict shutdown priority and command FIFO; focused network tests, full 151-test client suite, strict clippy/fmt/diff, live world readiness, and independent review clean).

Task 8G bounded network control liveness: complete (commits 180ea07 and f4e214c; controls use an independent exact-64 FIFO drained before admission-gated world data, and one bounded pending world event keeps the actual single worker able to consume commands and publish ACKs while the exact-4 world FIFO remains full; exact deadlock RED, focused 11-test network gate, full 156-test client suite, strict clippy/fmt/diff, and final independent rereview clean). Fresh capped live acceptance remains open.

Task 8G BDS readiness harness: complete (commit 7c35d71; primary log-marker readiness now has a BDS-only strict RakNet unconnected-pong fallback for child-side buffered stdout, with wrong-ID/wrong-magic regression coverage, preserved continuous log capture and cleanup, fresh full dry-run verification, and independent review clean).

Cutout leaves Task 1 independent block semantics: complete (commit f768cfa; BREG1002/MCBEAS02 version 2, exact independent air/cube/occluder/leaf flags with invalid and old-schema rejection, deterministic registry SHA 8a27e1389f5ffa2e2ab032563a45660dc31f5d708fdacb2225b344a49aa15bfc and exact 16913/713/669/44/1 counts; focused and broad Go/Rust tests, strict hygiene, no Mojang payload, and independent review clean).

Cutout leaves Task 2 materials and mips: complete (commit 4d23356; leaf-only bit-8 materials, exact 0x10f mask validation, deduplicated layers, raw RGB-preserving 21-step Q16 alpha coverage correction with global smaller-scale ties, deterministic cutout reporting, focused 32/full 57 asset tests, strict hygiene, and independent review clean). Ignored schema-2 blob SHA a626666c540c88b9457578e9f030bf4ee116f8b7b20891e5fce905b976f6c7c9 contains 405 materials, 11 cutout materials, and 372 layers.

Cutout leaves Task 3 palette-native meshing: complete (commit f33b71c; storage-palette facts, independent geometry/occluder/leaf u64 masks, exact ordered internal/all-boundary culling, diagnostic non-occlusion, classifier-authoritative air, non-occluder cave connectivity, world-graph integration, unchanged eight-byte quads; render 32+22+9 and client 128+19+10 tests, strict hygiene/flat-array scan, and final independent review clean).

Cutout leaves Task 4 opaque-shader cutout: complete (commit 8391a58; flat bit-8 material flags, exactly one texture sample, strict alpha-below-0.5 conditional discard, opaque alpha ignored, surviving alpha forced to one, unchanged eight-byte records and single depth-writing no-blend Opaque3d pipeline/bind group/MDI-direct architecture; render 32+22+10 and client 128+19+10 tests, strict hygiene, and independent review clean).

Phase 2.5 biome asset rules: complete (commits 4e7179b..578ec80; BIOREG01, MCBEAS03, eight tint maps, six resolved tint classes plus flags, fallback-zero live resolution, strict synthetic error/classification coverage; assets 69 tests + docs, strict clippy/fmt/workspace checks, independent re-review clean).

Phase 2.5 live biome runtime integration: complete (commits b55cdc5..6694383; lossless custom IDs, palette-native dense records, collision-free stream/revision identity, stale-work rejection, six-class WGSL routing, actual direct/MDI/upload/ack/bind-group tests; protocol/render/client suites and strict hygiene green, independent re-review clean). Real water-material production remains Phase 2.6.

Phase 2.6 Task 5 MCBEAS04 schema foundation: complete (commit 09f5831; range 2fd8b20..09f5831). The checked v4 codec/runtime now carries typed visual kind plus BREG1003 contributor role, render/tint/animation materials, fixed-point model templates/quads with UV and face/cull metadata, complete flipbook selector metadata and frame TextureRefs, one/two independently hashed texture pages, textures, and biomes. Every new section has canonical bounds/cross-reference/malformed decoder coverage; selected flipbooks and static strip aliases remain distinct by texture-key identity. Fresh evidence: assets 14+10+8+24+40+15 tests, strict assets Clippy, locked workspace all-target check, fmt/diff checks, and exact pinned-pack compile green (16,913 visuals, 377 materials, 397 layers, 88 biomes, 2,936,632 bytes, SHA-256 2e345f2993a5d68f5d1c8cdd06f32e270021e2c898b1a0686e2e3d7e3187fb). Independent final review reported no Critical/Important findings. Broader Phase 2.6 visual-family/template population remains open, so no plan checkbox is closed by this schema-only checkpoint.
