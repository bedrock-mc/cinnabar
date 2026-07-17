# Phase 2 Baseline Ownership and Dependency Map

This planning-only subordinate index is rooted at canonical functional base `d8e4699` and the approved amended Phase 2 plan. It records requirement ownership, producing and validating plan tasks, and task order only. All evidence fields for every sub-gate are open — not recorded.

## Requirement ownership

| Sub-gate | Master requirement |
|---|---|
| P2-BIOME-KERNEL; P2-BIOME-LIVE | P2.5-NATIVE-BIOME |
| P2-CHECKPOINT0-LUNAR; P2-CHECKPOINT0-ZEQA; P2-LUNAR-SPAWN; P2-LUNAR-REMESH; P2-ZEQA-SPAWN; P2-ZEQA-REMESH; P2-UI-PUBLICATION-PRESSURE | P2-CHUNK-PUBLICATION |
| P2-MOTION-AB; P2-LIGHT-PARITY; P2-FOG-AIR; P2-FOG-WATER; P2-FOG-LAVA; P2-PRECIPITATION; P2-CELESTIAL; P2-CLOUD-CALIBRATION; P2-CLOUD-LIVE; P2-FINAL | P2.7-ATMOSPHERE |

Every future evidence row names exactly one master requirement. Biome rows use `P2.5-NATIVE-BIOME`; streaming and publication rows use `P2-CHUNK-PUBLICATION`; lighting, atmosphere, weather, celestial, cloud, and motion rows use `P2.7-ATMOSPHERE`.

## Plan task and dependency map

The rows below are planning records, not evidence rows.

| Sub-gate | Master requirement | Producing and validating Phase 2 plan tasks | Prerequisites and order | Evidence state |
|---|---|---|---|---|
| P2-BIOME-KERNEL | P2.5-NATIVE-BIOME | Task 4 derives and fixes the native biome kernel; Task 13 reruns the final Biome matrix. | Tasks 1–2 establish the index and comparator, then Task 3 integrates the canonical gallery surface before Task 4. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-BIOME-LIVE | P2.5-NATIVE-BIOME | Task 4 establishes the kernel and matching-view gallery contract; Task 13 runs the final live/native Biome matrix. | Task 4 follows the Tasks 1–3 setup. Task 13 consumes the Task 4 output after the remaining Task 5–12 work. | Open — not recorded |
| P2-CHECKPOINT0-LUNAR | P2-CHUNK-PUBLICATION | Task 3 captures the Lunar diagnostic checkpoint with exact status `Non-final diagnostic`. | Task 3 starts only after Task 1 creates this index and Task 2 supplies the bounded comparator/workspace setup; Task 3 first integrates the frozen diagnostics and runner, then runs Lunar before Zeqa. | Open — not recorded |
| P2-CHECKPOINT0-ZEQA | P2-CHUNK-PUBLICATION | Task 3 captures the Zeqa diagnostic checkpoint with exact status `Non-final diagnostic`. | Tasks 1–2 precede Task 3. Within Task 3, Zeqa follows Lunar and consumes the hashed Lunar diagnostic-completeness manifest. | Open — not recorded |
| P2-LUNAR-SPAWN | P2-CHUNK-PUBLICATION | Task 7 runs the Lunar publication candidate and records its row with exact status `Non-final diagnostic`; Task 13 runs the final integrated Lunar validation. | Task 7 consumes Task 3 diagnostics, the Task 5 measured correction or evidence-directed no-change branch, and the Task 6 service controller. Lunar runs before Zeqa. Task 13 follows all prior Phase 2 evidence work and integrated Phase 5 settings. | Open — not recorded |
| P2-LUNAR-REMESH | P2-CHUNK-PUBLICATION | Task 7 runs the Lunar forced-remesh candidate and records its row with exact status `Non-final diagnostic`; Task 13 runs the final integrated Lunar validation. | Task 7 follows Tasks 3, 5, and 6, with Task 5 permitted to select `none`. Lunar runs before Zeqa. Task 13 follows all prior Phase 2 evidence work and integrated Phase 5 settings. | Open — not recorded |
| P2-ZEQA-SPAWN | P2-CHUNK-PUBLICATION | Task 7 runs the Zeqa publication candidate and records its row with exact status `Non-final diagnostic`; Task 13 runs the final integrated Zeqa validation. | Task 7 consumes Tasks 3, 5, and 6 and runs Zeqa only after the Lunar publication candidate gate. In Task 13, final Zeqa follows final Lunar and the integrated UI pressure check. | Open — not recorded |
| P2-ZEQA-REMESH | P2-CHUNK-PUBLICATION | Task 7 runs the Zeqa forced-remesh candidate and records its row with exact status `Non-final diagnostic`; Task 13 runs the final integrated Zeqa validation. | Task 7 consumes Tasks 3, 5, and 6 and runs Zeqa only after the Lunar publication candidate gate. In Task 13, final Zeqa follows final Lunar and the integrated UI pressure check. | Open — not recorded |
| P2-UI-PUBLICATION-PRESSURE | P2-CHUNK-PUBLICATION | Task 6 establishes the deterministic pressure/controller contract; Task 13 runs the integrated live pressure gate. | Task 6 consumes the Task 3 frozen service and quality interfaces. Task 13 requires the Task 6 integration handoff, all prior Phase 2 evidence work, and Phase 5 settings integrated through `EnvironmentQualitySettings`; its order is final Lunar, UI pressure, then final Zeqa. | Open — not recorded |
| P2-MOTION-AB | P2.7-ATMOSPHERE | Task 12 classifies and addresses the motion artifact with paired modes; Task 13 reruns the final motion matrix from one binary. | Task 12 consumes the Task 3 paired-runner surface and follows the Task 8–11 visual tranches. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-LIGHT-PARITY | P2.7-ATMOSPHERE | Task 8 calibrates solved lighting; Task 13 reruns the final LightingAtmosphere matrix. | Task 8 consumes the Task 2 comparator and Task 3 integrated gallery surface, after the publication candidate tranche. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-FOG-AIR | P2.7-ATMOSPHERE | Task 8 calibrates air fog; Task 13 reruns the final LightingAtmosphere matrix. | Task 8 consumes the Task 2 comparator and Task 3 integrated gallery surface, after the publication candidate tranche. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-FOG-WATER | P2.7-ATMOSPHERE | Task 8 calibrates water fog; Task 13 reruns the final LightingAtmosphere matrix. | Task 8 consumes the Task 2 comparator and Task 3 integrated gallery surface, after the publication candidate tranche. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-FOG-LAVA | P2.7-ATMOSPHERE | Task 8 calibrates lava fog; Task 13 reruns the final LightingAtmosphere matrix. | Task 8 consumes the Task 2 comparator and Task 3 integrated gallery surface, after the publication candidate tranche. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-PRECIPITATION | P2.7-ATMOSPHERE | Task 9 implements and runs the native-referenced precipitation tranche; Task 13 reruns the final Precipitation matrix. | Task 9 follows Task 8, uses the Task 3 quality-interface boundary, and requires its own integration export/startup handoff before the live gallery. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-CELESTIAL | P2.7-ATMOSPHERE | Task 10 runs the celestial border/filter-edge tranche; Task 13 reruns the final Celestial matrix. | Task 10 follows Task 9 and consumes the Task 2 comparator plus Task 3 gallery surface. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-CLOUD-CALIBRATION | P2.7-ATMOSPHERE | Task 11 derives the evidence-fixed native cloud configuration; Task 13 reruns the final Cloud matrix. | Task 11 follows Task 10 and consumes the Task 3 `CloudQuality` carrier, Task 2 comparator, and integrated Cloud gallery surface. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-CLOUD-LIVE | P2.7-ATMOSPHERE | Task 11 runs the live native cloud gallery; Task 13 reruns the final Cloud matrix. | Task 11 follows its calibration work and the Cloud gallery integration handoff. Task 13 follows Tasks 4–12. | Open — not recorded |
| P2-FINAL | P2.7-ATMOSPHERE | Task 13 alone runs the complete deterministic, local native, motion, and integrated remote matrix and may prepare the only `Final candidate` handoff for independent review. | Task 13 consumes the future artifacts from Tasks 4–12. Its integrated live portion also requires Phase 5 settings through `EnvironmentQualitySettings`; final Lunar precedes UI pressure, which precedes final Zeqa. | Open — not recorded |

## Future evidence-row contract

Task 1 records no commands, reviewed-run commits, run-directory manifest hashes, metrics hashes, server/native builds, backend/adapter/driver identities, requested/effective present modes, asset identities, results, unresolved failures, timings, counts, or outcomes.

When a later producing task creates a complete row, that row must include the sub-gate, its single master ID, exact command, reviewed commit, create-new run-directory manifest SHA-256, metrics SHA-256, server/native build, backend/adapter/driver, requested/effective present mode, asset identities, result, and unresolved failure. Every Task 3 checkpoint row and every Task 7 candidate row has exact status `Non-final diagnostic`. Only Task 13 may use the exact status `Final candidate`. Until those tasks run, no evidence row exists.

## Task 5A deterministic client blob-cache implementation

The deterministic implementation now provides an opt-in, process-persistent verified blob cache with a fresh pending resolver per Play session. It negotiates cache support only through the enabled login route, reconstructs cached LevelChunk and SubChunk packets before the stateless world normalizer, preserves world FIFO across split miss responses, and resets pending state at session, transfer, disconnect, and ordered dimension boundaries. Cache storage, individual blobs, packet hash lists, pending transactions, and retained/reconstructed bytes are bounded; miss payloads are verified with seed-zero xxHash64 before atomic admission. Aggregate counters contain no blob payloads or server secrets.

The cache publication surface is part of the strict `PHASE2_PUBLICATION` schema. The app forwards live Play-session enablement and aggregate counters into `ClientWorld`, publishes only secret-safe totals, and uses a bounded non-cancellable final control flush before network failure or ordinary stop. The acceptance parser requires the exact cache schema, exact Boolean enablement, unsigned integral counters, `hits + misses = hashes_classified`, zero counters while disabled, stable enablement, and non-regressing cumulative counters across a diagnostic sequence. Lunar terminal evidence must prove enabled cache-backed hash activity with zero rejected blobs and zero pending transactions/bytes; Zeqa explicitly records either `ordinary_payload` or `cache_backed` while enforcing the same clean terminal state.

Deterministic checks completed locally:

- `cargo test -p protocol --locked --test blob_cache -- --nocapture` — 26 passed.
- `cargo test -p protocol --locked --test login_state -- --nocapture` — 7 passed, including encrypted enabled negotiation and cached-world FIFO.
- `cargo test -p bedrock-client --lib --locked` — 202 passed, including live cache-stat forwarding, shutdown-final flushing, and exact publication serialization.
- `Invoke-Pester scripts/tests/remote-acceptance.Tests.ps1` under Windows PowerShell — 19 passed, including exact cache schema, server-specific terminal routes, and sequence validation.
- `cargo test -p jolyne --features client client_cache -- --nocapture` — 2 passed for vendored enabled/disabled negotiation.
- `go test ./... -run ClientBlobCache -count=1` and `go vet ./...` under `core` — passed with the pinned gophertunnel/xxHash fixture.
- `cargo clippy -p protocol -p client-world --all-targets --locked -- -D warnings` and `cargo clippy -p bedrock-client --lib --bins --locked -- -D warnings` — passed.

The combined `bedrock-client --all-targets` gate remains blocked outside Task 5A by the pre-existing `app/tests/assets.rs` `CompiledEntityAssets` initializer missing newly added animation fields. No live Lunar or Zeqa evidence was run or recorded here; Task 5A remains non-final until the ordered live gate is executed independently.
