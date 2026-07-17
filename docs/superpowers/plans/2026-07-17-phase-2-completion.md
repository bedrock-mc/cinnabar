# Phase 2 Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close Phase 2.5 biome blending, diagnose and fix canonical Lunar then Zeqa chunk publication, and finish every remaining Phase 2.7 lighting, atmosphere, celestial, precipitation, cloud, motion-artifact, live, and performance gate.

**Architecture:** Preserve the canonical palette-native world, compact mesh, shared atmosphere-uniform, and identity-cached GPU-resource designs. Add bounded evidence contracts first, select behavioral changes only from those contracts, and keep native-reference inputs and captures ignored-local. World streaming remains owned by `client-world`; the app owns scheduling and acceptance; `render` and `meshing` own visual/GPU behavior; integration-lane wiring is limited to declared shared interfaces.

**Tech Stack:** Rust 1.93.1, Bevy 0.18.1/wgpu, WGSL/Naga, PowerShell and Bash acceptance harnesses, Go `bedrock-core`, local BDS, authenticated Lunar/Zeqa, matching native Bedrock, JSON evidence, SHA-256 provenance.

## Global Constraints

- Execute only in `C:\Users\Hashim\Projects\rust-mcbe\.worktrees\completion-phase2` on branch `completion-phase2`, after the integration owner creates it from the reviewed `full-client-design` tip. Both `b2940086fa2bc8d7089ae065da575086557cbd67` and `d8e469979a0ec6c4798bb2ffc1dc45d3a9891eeb` must be ancestors. Never execute this plan in `full-client-design` or an archival Phase 2 worktree.
- Never commit Mojang payloads, generated asset carriers, native binaries, credentials, token contents, screenshots, or captured world databases. Store them below ignored `.local/phase2/` or the OS temporary directory.
- Completion is strict: unavailable native evidence, missing live witnesses, unexplained diagnostics, failed performance gates, or an unreviewed change remains blocking.
- Preserve palette-native immutable biome/light neighbourhoods, eight-byte packed cube and cloud records, bounded queues/arenas, generation and source-identity validation, and fail-closed unknown boundaries.
- Retain one immutable cloud resource family and one cloud draw; do not add per-frame or per-subchunk buffers, textures, bind groups, meshes, materials, or uploads.
- Measure Lunar `pvp.lunarbedrock.com:19134` first. Do not run the binding Zeqa `zeqa.net:19132` publication gate until Lunar has no persistent current-position hole and its stage evidence is clean.
- Live release evidence uses normal FIFO presentation. Immediate is used only for the explicit identical-scene motion A/B and must prove its effective present mode.
- The authoritative performance manifest requires a 30-second warmup followed by one uninterrupted 120-second steady sample. During that sample, p95 and p99 frame time are each at most `16.6666666667` ms, maximum frame time is at most `50` ms, and exactly 120 one-second resource samples prove maximum combined client/core RSS at most 650 MB and mean and p95 combined CPU each at most 15 percent. Binding join, teleport settle, and forced full-view remesh are each at most 2,000 ms. Every report records processor count, OS, build, backend, adapter, driver, present mode, and asset identities.
- Every behavior change begins with a focused failing test, ends with focused/full verification and independent review, and lands as a small non-squashed commit.
- Phase 2 owns biome blending, client-world streaming/publication, lighting, atmosphere, and a subordinate Phase 2 evidence index. Root `Cargo.toml`, `Cargo.lock`, `plan.md`, the master ledger, app module/plugin/schedule assembly, render and asset module-root exports, asset startup, architecture allowlists, protocol/core shared dispatch, and acceptance entry points are integration-owned. The lane records exact handoff hunks and tests; it never stages those shared files.
- Before any behavior change, the lane pins the reviewed carrier commit as `completion-phase2-interface`; the integration owner then merges that immutable producer history and applies the reviewed exports/runner handoff; canonical diagnostic-only Lunar then Zeqa runs complete in that order.

---

## File and Interface Map

### New files

- `crates/client-world/src/stream/diagnostics.rs`: bounded stage/outcome counters and cohort-qualified publication snapshots.
- `crates/client-world/src/stream/request_queue.rs`: stable bounded priority ordering for player/visible/prefetch initial work and retries.
- `crates/client-world/src/publication_config.rs`: frozen elapsed-time service configuration with explicit minimum, target, frame, and burst ceilings.
- `crates/assets/src/environment_settings.rs`: Bevy/wgpu-free canonical `CloudQuality`, `PrecipitationQuality`, and `EnvironmentQualitySettings` carrier shared by settings and render.
- Integration handoff, not a lane edit: `app/src/runtime/phase2_evidence.rs` emits parser-stable Phase 2 evidence without paths, payloads, or credentials.
- Integration handoff, not a lane edit: `scripts/remote-acceptance.ps1` and `scripts/tests/remote-acceptance.Tests.ps1` provide authenticated Lunar/Zeqa release runs with create-new run directories and secret-safe evidence.
- `tools/phase2-evidence/Cargo.toml`, `tools/phase2-evidence/src/lib.rs`, `tools/phase2-evidence/src/main.rs`: bounded PNG/evidence comparison CLI operating in linear colour.
- `tools/phase2-evidence/tests/cli.rs`: CLI bounds/path-redaction tests that generate tiny synthetic PNG/JSON inputs in a temporary directory; no checked-in image bytes.
- `crates/meshing/tests/fixtures/native-biome-kernel-v1.json`: accepted kernel literals, sample labels, expected linear RGB values, comparator-report hash, and native build identity; no image bytes or paths.
- `crates/render/tests/fixtures/native-lighting-fog-v1.json`: accepted light/fog input-output literals, epsilons, comparator-report hashes, and native build identity; no image bytes or paths.
- `crates/render/tests/fixtures/native-cloud-calibration-v1.json`: accepted per-quality cloud origins/counts/distances, matching-view report hash, asset hash, and native build identity; no image bytes or paths.
- `crates/assets/src/precipitation.rs`, `crates/asset-compiler/src/precipitation.rs`: schema-isolated `MCBEPRC1` precipitation carrier; it does not change `MCBEATM1`.
- `crates/render/src/precipitation.rs`, `crates/render/src/precipitation.wgsl`, `crates/render/tests/precipitation.rs`: bounded weather geometry/pipeline selected from native evidence.
- `docs/phase-2-completion-report.md`: subordinate secret-safe evidence index whose rows map to master IDs; it is not a second requirement ledger and never closes `plan.md`.

### Existing ownership

- `crates/client-world/src/stream/{requests,retries,decode,lighting,meshing,publication,cohort,polling,sequencing}.rs`: request lifecycle, work stages, and exact cohort state.
- `app/src/runtime/{network,world,publication,telemetry}.rs`: transport acknowledgement, main-world application, adaptive allowance, and logging.
- `app/src/acceptance/{world_ready,teleport,remesh,proofs}.rs` and `app/src/metrics{,/report}.rs`: exact live proof and final JSON.
- `crates/meshing/src/{biome,cloud}.rs`: palette-native biome records and compact periodic cloud mesh.
- `crates/render/src/chunk/biome_tints.rs`, `crates/render/src/biome_tint.wgsl`, `crates/render/src/lighting.wgsl`, `crates/render/src/atmosphere.rs`, `crates/render/src/atmosphere.wgsl`, `crates/render/src/atmosphere_render.rs`, `crates/render/src/cloud_config.rs`, `crates/render/src/cloud_render.rs`, and `crates/render/src/cloud.wgsl`: Phase 2 GPU behavior.
- `crates/assets/src/atmosphere.rs` and `crates/asset-compiler/src/atmosphere.rs`: versioned atmosphere inputs. Any precipitation-carrier change is an integration-owned schema checkpoint.
- `scripts/acceptance.ps1`, `scripts/acceptance/**`, and `scripts/tests/acceptance/**`: deterministic BDS/GDI/presentation gates.

### Frozen Phase 2 interfaces

```rust
pub struct Phase2PublicationSnapshot {
    pub session_generation: u64,
    pub player_column: world::ChunkKey,
    pub publisher_radius_chunks: Option<i32>,
    pub required_cohort_hash: u64,
    pub required_columns: usize,
    pub loaded_required_columns: usize,
    pub stages: PublicationStageCounters,
    pub outcomes: SubChunkOutcomeCounters,
    pub max_queue_wait: StageDurations,
    pub max_worker_time: StageDurations,
}

pub struct CohortManifestIdentity {
    pub session_generation: u64,
    pub required_cohort_hash: u64,
    pub generation_manifest_hash: u64,
    pub entry_count: usize,
}

pub struct Phase2PresentationSnapshot {
    pub build_profile: BuildProfileIdentity,
    pub graphics_identity_sha256: [u8; 32],
    pub requested_present_mode: PresentModeIdentity,
    pub effective_present_mode: PresentModeIdentity,
    pub assets_manifest_sha256: [u8; 32],
    pub publisher_disk: CohortManifestIdentity,
    pub resident: CohortManifestIdentity,
    pub allocation: CohortManifestIdentity,
    pub visible: CohortManifestIdentity,
    pub submitted: CohortManifestIdentity,
    pub gpu_presented: CohortManifestIdentity,
}

pub enum RequestClass {
    PlayerRetry,
    PlayerInitial,
    VisibleRetry,
    VisibleInitial,
    PrefetchRetry,
    PrefetchInitial,
}

pub struct PublicationServiceConfig {
    pub minimum_items_per_second: u32,
    pub minimum_bytes_per_second: u64,
    pub target_items_per_second: u32,
    pub target_bytes_per_second: u64,
    pub maximum_frame_items: usize,
    pub maximum_frame_bytes: u64,
    pub maximum_burst_items: usize,
    pub maximum_burst_bytes: u64,
    pub maximum_zero_byte_operations_per_frame: usize,
}

impl PublicationServiceConfig {
    pub const PHASE2_GATE: Self = Self {
        minimum_items_per_second: 4_096,
        minimum_bytes_per_second: 64 * 1024 * 1024,
        target_items_per_second: 8_192,
        target_bytes_per_second: 128 * 1024 * 1024,
        maximum_frame_items: 512,
        maximum_frame_bytes: 64 * 1024 * 1024,
        maximum_burst_items: 8_192,
        maximum_burst_bytes: 128 * 1024 * 1024,
        maximum_zero_byte_operations_per_frame: 256,
    };
}

pub enum CloudQuality { Low, Medium, High, Ultra }

pub enum PrecipitationQuality { Off, Low, High }

pub struct EnvironmentQualitySettings {
    pub clouds: CloudQuality,
    pub precipitation: PrecipitationQuality,
}
```

`WorldStream::poll(camera_position, max_mesh_jobs)`, `WorldStream::collision_store`, `ChunkRenderQueue`, `ChunkUploadBudget`, `AtmosphereFrame`, and `CloudRenderConfig` remain source-compatible through integration checkpoint 1. `assets::environment_settings` is the sole canonical, Bevy/wgpu-free owner of `CloudQuality`, `PrecipitationQuality`, and `EnvironmentQualitySettings`; render consumes those types and may preserve a source-compatible re-export. Phase 5 depends on `assets`, never `render`, and stores/submits these enums rather than `u8`, while Phase 2 owns validation and renderer semantics.

---

### Task 1: Lock the Canonical Baseline and Phase 2 Requirement Ledger

**Files:**
- Create during execution: `docs/phase-2-completion-report.md`
- Integration handoff only: `docs/evidence/phases-2-5-completion-ledger.md`, `plan.md`

**Interfaces:**
- Consumes: canonical `d8e4699` and the approved amended completion spec.
- Produces: a subordinate evidence index. Every row names exactly one master ID: biome rows map to `P2.5-NATIVE-BIOME`, streaming/publication rows map to `P2-CHUNK-PUBLICATION`, and lighting/atmosphere/weather/celestial/cloud/motion rows map to `P2.7-ATMOSPHERE`.

- [ ] **Step 1: Prove the implementation worktree starts at the approved lineage**

Run:

```powershell
$expectedRoot = 'C:\Users\Hashim\Projects\rust-mcbe\.worktrees\completion-phase2'
$actualRoot = (git rev-parse --show-toplevel).Trim() -replace '/', '\'
$actualBranch = (git branch --show-current).Trim()
if ($actualRoot -cne $expectedRoot) { throw "wrong worktree: $actualRoot" }
if ($actualBranch -cne 'completion-phase2') { throw "wrong branch: $actualBranch" }
git merge-base --is-ancestor b2940086fa2bc8d7089ae065da575086557cbd67 HEAD
if ($LASTEXITCODE -ne 0) { throw 'approved spec commit is not an ancestor' }
git merge-base --is-ancestor d8e469979a0ec6c4798bb2ffc1dc45d3a9891eeb HEAD
if ($LASTEXITCODE -ne 0) { throw 'canonical functional base is not an ancestor' }
git merge-base --is-ancestor completion-integration HEAD
if ($LASTEXITCODE -ne 0) { throw 'lane does not contain the current integration checkpoint' }
$dirty = @(git status --short)
if ($dirty.Count -ne 0) { throw "implementation worktree is not clean:`n$($dirty -join "`n")" }
```

Expected: every guard passes only in the clean fresh Phase 2 lane. `full-client-design`, the root worktree, and every archival branch fail before any mutation.

- [ ] **Step 2: Run the deterministic pre-change baseline**

Run:

```powershell
cargo test -p world -p meshing -p client-world -p render -p bedrock-client --locked
cargo test -p client-world --release --locked release_full_view_known_air_lighting_completes_within_two_seconds -- --ignored --nocapture
cargo clippy -p world -p meshing -p client-world -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p devtool --locked -- verify-affected --base d8e4699 --dry-run
```

Expected: all commands pass; the ignored lighting test reports 26,136 current known-air subchunks below 2,000 ms; the devtool prints the affected verification set without changing files.

- [ ] **Step 3: Write the ledger using actual baseline results**

Create `docs/phase-2-completion-report.md` with this exact two-column ownership map before any evidence rows:

```markdown
| Sub-gate | Master requirement |
|---|---|
| P2-BIOME-KERNEL; P2-BIOME-LIVE | P2.5-NATIVE-BIOME |
| P2-CHECKPOINT0-LUNAR; P2-CHECKPOINT0-ZEQA; P2-LUNAR-SPAWN; P2-LUNAR-REMESH; P2-ZEQA-SPAWN; P2-ZEQA-REMESH; P2-UI-PUBLICATION-PRESSURE | P2-CHUNK-PUBLICATION |
| P2-MOTION-AB; P2-LIGHT-PARITY; P2-FOG-AIR; P2-FOG-WATER; P2-FOG-LAVA; P2-PRECIPITATION; P2-CELESTIAL; P2-CLOUD-CALIBRATION; P2-CLOUD-LIVE; P2-FINAL | P2.7-ATMOSPHERE |
```

Each later row records the sub-gate, master ID, exact command, reviewed commit, create-new run-directory manifest SHA-256, metrics SHA-256, server/native build, backend/adapter/driver, requested/effective present mode, asset identities, result, and unresolved failure. `P2-CHECKPOINT0-*` and Task 7 candidate rows are explicitly `Non-final diagnostic`; only Task 13 may emit a `Final candidate` handoff. Do not create empty evidence rows or edit the master ledger in this lane.

- [ ] **Step 4: Commit the baseline ledger**

```powershell
git add docs/phase-2-completion-report.md
git commit -m "docs: map phase 2 evidence to master requirements"
```

### Task 2: Add the Bounded Phase 2 Evidence Tool

**Files:**
- Create: `tools/phase2-evidence/Cargo.toml`
- Create: `tools/phase2-evidence/src/lib.rs`
- Create: `tools/phase2-evidence/src/main.rs`
- Create: `tools/phase2-evidence/tests/cli.rs`
- Integration handoff only: root `Cargo.toml`, `Cargo.lock`, `tools/architecture/policy.toml`, `tools/architecture/tests/policy.rs`

**Interfaces:**
- Consumes: PNG captures and parser-stable JSON/marker files from ignored-local runs.
- Produces: `phase2-evidence compare --kind biome --manifest .local/phase2/reference/manifest.json --native .local/phase2/reference/native.png --cinnabar .local/phase2/reference/cinnabar.png --out .local/phase2/reference/comparison.json`; the `--kind` value also accepts `lighting`, `fog-air`, `fog-water`, `fog-lava`, `celestial`, or `cloud`. Output contains hashes, crop identity, linear-RGB error statistics, and pass/fail thresholds, never source paths.

- [ ] **Step 1: Write failing parser, bounds, and linear-colour tests**

```rust
#[test]
fn comparison_rejects_mismatched_dimensions_and_hashes_pixels_in_linear_rgb() {
    let request = ComparisonRequest::synthetic(
        EvidenceKind::Biome,
        rgba_png(2, 1, &[[128, 64, 32, 255], [255, 255, 255, 255]]),
        rgba_png(2, 1, &[[128, 64, 32, 255], [254, 255, 255, 255]]),
    );
    let report = compare(request).expect("bounded comparison");
    assert_eq!(report.sample_count, 2);
    assert_eq!(report.maximum_channel_error_rgb8, 1);
    assert!(report.mean_squared_error_linear.is_finite());
    assert!(compare(ComparisonRequest::dimension_mismatch()).is_err());
}
```

Run: `cargo test -p phase2-evidence --locked comparison_rejects -- --nocapture`

Expected: FAIL because the package and comparison API do not exist.

- [ ] **Step 2: Implement the exact CLI and bounded report**

Use `image = { version = "0.25", default-features = false, features = ["png"] }`, `clap`, `serde`, `serde_json`, `sha2`, and `thiserror`. Reject inputs over 32 MiB, dimensions above 8192×8192, crop coordinates outside either image, alpha mismatch unless the manifest permits it, duplicate sample labels, non-finite thresholds, and output aliases to either input. Convert sRGB channels with the standard piecewise sRGB transfer function before computing mean, maximum, and per-labelled-sample error.

- [ ] **Step 3: Commit the isolated tool source without staging shared files**

```powershell
git add tools/phase2-evidence
git commit -m "test: add bounded phase 2 evidence comparator"
```

Expected: the commit contains only `tools/phase2-evidence/**`. It does not stage a root manifest, lockfile, architecture policy, app file, acceptance entry point, roadmap, or master ledger.

- [ ] **Step 4: Hand workspace registration to integration and fast-forward back**

The integration owner merges the reviewed tool commit, adds only `tools/phase2-evidence` to the workspace, refreshes `Cargo.lock`, adds the exact architecture allowlist/test entry if required, and runs:

```powershell
cargo test -p phase2-evidence --locked
cargo test -p architecture --locked
cargo clippy -p phase2-evidence --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all synthetic comparisons pass and no fixture or report contains an absolute path. The integration owner commits the root/policy wiring, then the lane runs:

```powershell
git merge --ff-only completion-integration
git status --short
```

Expected: the Phase 2 lane is clean and contains the reviewed integration wiring without recreating its hunks.

### Task 3: Freeze Phase 2 Interfaces, Integrate Evidence Runners, and Capture Checkpoint 0

**Files:**
- Create: `crates/client-world/src/publication_config.rs`
- Create: `crates/client-world/src/stream/diagnostics.rs`
- Modify: `crates/client-world/src/stream.rs`, `crates/client-world/src/stream/model.rs`
- Modify: `crates/client-world/src/stream/requests.rs`, `crates/client-world/src/stream/retries.rs`, `crates/client-world/src/stream/decode.rs`, `crates/client-world/src/stream/lighting/jobs.rs`, `crates/client-world/src/stream/meshing/jobs.rs`, `crates/client-world/src/stream/publication.rs`, `crates/client-world/src/stream/cohort.rs`, `crates/client-world/src/stream/sequencing.rs`
- Create: `crates/assets/src/environment_settings.rs`
- Create: `crates/assets/tests/environment_settings.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: `crates/client-world/src/lib.rs`, `crates/assets/src/lib.rs`, `crates/render/src/lib.rs`, `crates/render/src/cloud_config.rs`, `app/src/runtime/phase2_evidence.rs`, `app/src/runtime/mod.rs`, `app/src/runtime/network.rs`, `app/src/runtime/world.rs`, `app/src/runtime/publication.rs`, `app/src/runtime/telemetry.rs`, `app/src/app.rs`, `app/src/args.rs`, `app/src/metrics.rs`, `app/src/metrics/report.rs`
- Integration handoff only: `scripts/remote-acceptance.ps1`, `scripts/phase2-gallery.ps1`, `scripts/phase2-motion-ab.ps1`, `scripts/tests/remote-acceptance.Tests.ps1`, `scripts/tests/phase2-gallery.Tests.ps1`, `scripts/tests/phase2-motion-ab.Tests.ps1`, `scripts/acceptance/Load.ps1`, `scripts/acceptance/Orchestrator.ps1`, `scripts/acceptance/Orchestration/Validate.ps1`, `scripts/acceptance/Orchestration/Execute.ps1`
- Integration handoff only: Phase 5 `VideoSettings` and `RuntimeSettingsUpdate` fields that must import `CloudQuality` and `PrecipitationQuality` from `assets`, never depend on `render` or store `u8`

**Interfaces:**
- Produces the frozen types shown under “Frozen Phase 2 interfaces,” `WorldStream::phase2_publication_snapshot(player_column) -> Phase2PublicationSnapshot`, and one `PHASE2_PUBLICATION` line per changed combined snapshot identity.
- `scripts/remote-acceptance.ps1 -Server Lunar|Zeqa -Mode Diagnostic|Candidate|Final -RunId <id> -DurationSeconds <n> -AuthCache <ignored-path> -InitialRadius 16 -PresentMode Fifo|Immediate -FullViewTeleportGate -OpenSettingsOverlay -Assets <blob> -ClientExecutable <path> -SkipClientBuild` creates `.local/phase2/remote/<run-id>/` with create-new semantics. `-OpenSettingsOverlay` is valid only after the Phase 5 adapter is integrated; diagnostic/candidate runs omit it.
- `scripts/phase2-gallery.ps1 -Gallery Biome|LightingAtmosphere|Precipitation|Celestial|Cloud -RunId <id> -BdsDir <path> -Assets <blob> -NativeRoot <path> -PresentMode Fifo -ClientExecutable <path> -SkipClientBuild` creates `.local/phase2/galleries/<run-id>/` and runs every manifested `phase2-evidence compare` command. `-ClientExecutable` is optional unless `-SkipClientBuild` is present.
- Remote, gallery, and motion manifests reject a requested duration too short to contain the 30-second warmup and uninterrupted 120-second steady sample; their performance records include p95, p99, maximum frame time, all three ≤2,000 ms lifecycle gates where applicable, and exactly 120 one-second resource samples.
- No interface or runner changes client scheduling, request ordering, retry policy, render constants, or asset schema before checkpoint-0 captures finish.

- [ ] **Step 1: Write failing bounded-interface tests**

```rust
#[test]
fn phase2_gate_has_explicit_minimum_frame_and_burst_bounds() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    assert_eq!(config.minimum_items_per_second, 4_096);
    assert_eq!(config.target_items_per_second, 8_192);
    assert_eq!(config.maximum_frame_items, 512);
    assert_eq!(config.maximum_burst_items, 8_192);
    assert_eq!(config.maximum_zero_byte_operations_per_frame, 256);
    assert!(config.minimum_bytes_per_second <= config.target_bytes_per_second);
    assert!(config.maximum_frame_bytes <= config.maximum_burst_bytes);
}

#[test]
fn environment_quality_rejects_numeric_surrogates() {
    let settings = EnvironmentQualitySettings {
        clouds: CloudQuality::High,
        precipitation: PrecipitationQuality::Low,
    };
    assert_eq!(settings.clouds, CloudQuality::High);
    assert_eq!(settings.precipitation, PrecipitationQuality::Low);
}
```

Run:

```powershell
cargo test -p client-world --locked phase2_gate_has_explicit -- --nocapture
cargo test -p assets --locked environment_quality_rejects -- --nocapture
```

Expected: FAIL because the frozen modules and exports are absent.

- [ ] **Step 2: Write failing stage and identity tests**

```rust
#[test]
fn publication_snapshot_separates_every_stage_and_subchunk_outcome() {
    let mut stream = request_mode_stream();
    drive_success_all_air_unavailable_malformed_stale_and_timeout(&mut stream);
    let snapshot = stream.phase2_publication_snapshot(player_chunk());
    assert_eq!(snapshot.outcomes.success, 1);
    assert_eq!(snapshot.outcomes.all_air, 1);
    assert_eq!(snapshot.outcomes.unavailable, 1);
    assert_eq!(snapshot.outcomes.malformed, 1);
    assert_eq!(snapshot.outcomes.stale, 1);
    assert_eq!(snapshot.outcomes.timed_out, 1);
    assert!(snapshot.stages.requests_sent <= snapshot.stages.requests_constructed);
}
```

Run: `cargo test -p client-world --locked publication_snapshot_separates_every_stage -- --nocapture`

Expected: FAIL because the snapshot API is absent.

- [ ] **Step 3: Implement behavior-neutral bounded diagnostics and carriers**

Use saturating cumulative counters and current gauges with no retained per-key log. Qualify identities with session generation, dimension, player column, required-cohort hash, fixed-size build/present enums, graphics SHA-256, and asset SHA-256. Implement the literal service bounds above and the Bevy/wgpu-free `assets::environment_settings::{CloudQuality, PrecipitationQuality, EnvironmentQualitySettings}` carrier; do not connect the service controller, change any queue policy, or make Phase 5 depend on render.

- [ ] **Step 4: Commit lane-owned carrier source and request the integration handoff**

```powershell
cargo test -p client-world --locked publication_snapshot_separates_every_stage -- --nocapture
rustfmt --edition 2024 crates/client-world/src/publication_config.rs crates/client-world/src/stream/diagnostics.rs crates/assets/src/environment_settings.rs crates/assets/tests/environment_settings.rs
git diff --check
git add crates/client-world/src/publication_config.rs crates/client-world/src/stream crates/assets/src/environment_settings.rs crates/assets/tests/environment_settings.rs
git commit -m "feat: freeze phase 2 publication and quality interfaces"
git branch completion-phase2-interface HEAD
git show-ref --verify refs/heads/completion-phase2-interface
git status --short
```

Expected: the diagnostics test passes. The two new assets files are syntax-checked but remain inactive until the integration-owned module export; the commit contains no module root, root, app, script-entry, architecture, roadmap, master-ledger, protocol, or core file. The branch command succeeds once and pins exactly this reviewed carrier history; it is never force-moved. The commit message lists the exact integration-only exports/wiring and the typed Phase 5 replacement of cloud/weather `u8` fields.

- [ ] **Step 5: Integrate and test the shared runner/app handoff**

The integration owner verifies and merges `completion-phase2-interface`, exports the assets carrier, removes render's duplicate `CloudQuality` definition in favor of consuming/re-exporting `assets::environment_settings::CloudQuality`, applies only the other handoff files listed above, and makes every runner reject an existing `RunId`, a path outside `.local/phase2`, a duration below 150 seconds for a performance-bearing run, a non-release build, an unproved effective present mode, or an auth path outside ignored `.local`. No Bevy or wgpu dependency may be added to `assets`. The remote runner maps `-InitialRadius 16` to the existing authoritative radius-16 client contract and rejects every other value; it does not pass an unsupported client CLI flag. Run in the integration worktree:

```powershell
cargo test -p client-world -p assets -p render -p bedrock-client --locked phase2 -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/remote-acceptance.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/phase2-gallery.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/phase2-motion-ab.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
cargo clippy -p client-world -p assets -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all tests pass; each synthetic run receives a distinct create-new directory; logs redact auth paths and never read token contents. After the integration commit, fast-forward the lane without moving the immutable interface branch:

```powershell
git merge --ff-only completion-integration
git show-ref --verify refs/heads/completion-phase2-interface
git status --short
```

Expected: the immutable branch still points at the reviewed carrier commit, `HEAD` contains the integration-owned exports/runner handoff, and the Phase 2 lane is clean.

- [ ] **Step 6: Capture canonical Lunar diagnostic completeness first**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Lunar -Mode Diagnostic -RunId checkpoint0-lunar-attempt-01 -DurationSeconds 180 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -Assets $env:RUST_MCBE_ASSETS
```

Expected: `.local/phase2/remote/checkpoint0-lunar-attempt-01/manifest.json` proves release/FIFO identity and contains a complete `PHASE2_PUBLICATION` stage sequence. This diagnostic gate requires attributable counters and coherent identities, including the raw publisher radius in blocks as well as its derived retention radius; it records holes/stalls as findings and does not require them to be fixed.

- [ ] **Step 7: Capture canonical Zeqa only after Lunar diagnostics are complete**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Zeqa -Mode Diagnostic -RunId checkpoint0-zeqa-attempt-01 -DurationSeconds 180 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -Assets $env:RUST_MCBE_ASSETS
```

Expected: the runner rejects this command unless the Lunar diagnostic-completeness manifest exists and hashes successfully. Zeqa records the same stage/outcome surface without requiring Lunar’s publication behavior to have passed.

- [ ] **Step 8: Commit non-final checkpoint-0 evidence**

Record only the two run-directory manifest hashes, metrics hashes, stage classifications, identities, and unresolved findings under `P2-CHECKPOINT0-LUNAR` and `P2-CHECKPOINT0-ZEQA`, both marked `Non-final diagnostic`.

```powershell
git add docs/phase-2-completion-report.md
git commit -m "docs: record canonical phase 2 server diagnostics"
```

### Task 4: Adjudicate and Close the Phase 2.5 Biome Kernel

**Files:**
- Modify: `crates/meshing/src/biome.rs`
- Modify: `crates/meshing/tests/biome.rs`
- Create from the accepted report: `crates/meshing/tests/fixtures/native-biome-kernel-v1.json`
- Modify: `crates/render/src/biome_tint.wgsl`
- Modify: `crates/render/tests/biome_shader.rs`
- Modify: `crates/client-world/src/stream/dirty.rs`, `crates/client-world/src/stream/meshing/jobs.rs`, `crates/client-world/src/stream/publication.rs`, `crates/client-world/src/stream/tests/mesh_dependency.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: `app/src/runtime/telemetry.rs`, `app/src/tests/finish.rs`, master-ledger row `P2.5-NATIVE-BIOME`, `plan.md`

**Interfaces:**
- Consumes: ignored-local native and Cinnabar captures for straight, corner, island, and alternating biome layouts across grass, generic foliage, birch/evergreen/dry foliage, and water.
- Produces: one evidence-fixed `BiomeBlendKernel` shared by CPU record construction and WGSL; retains the current nine-source identity/stale-rejection contract unless evidence proves another bounded radius.

- [ ] **Step 1: Run the exact release/FIFO matching-view gallery**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Biome -RunId biome-kernel-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/biome-boundary-v1 -PresentMode Fifo
```

Expected: the create-new run directory contains straight, corner, island, and alternating manifests; matching native/Cinnabar crops; linear-colour reports; a 30-second warmup followed by a 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms and maximum frame time ≤`50` ms; two exact presented witnesses; and no unexplained diagnostic. Conflicting patterns block implementation and require `biome-kernel-attempt-02`, never a guessed kernel.

- [ ] **Step 2: Write the failing evidence-derived CPU test**

```rust
#[test]
fn native_boundary_kernel_matches_every_accepted_evidence_pattern() {
    let evidence: NativeBiomeKernelEvidence = serde_json::from_str(include_str!(
        "fixtures/native-biome-kernel-v1.json"
    ))
    .expect("reviewed native biome evidence fixture");
    let kernel = BiomeBlendKernel::native();
    assert_eq!(kernel.radius(), evidence.radius);
    assert_eq!(kernel.denominator(), evidence.denominator);
    for sample in evidence.samples {
        assert_eq!(kernel.blend(sample.neighbourhood), sample.expected_linear_rgb);
    }
}
```

Create the fixture from only accepted numeric values, report SHA-256, native build identity, and manifest SHA-256. Reject duplicate/missing labels, non-finite RGB, zero denominators, out-of-radius offsets, invalid hashes, and empty samples.

Run: `cargo test -p meshing --locked native_boundary_kernel_matches_every_accepted_evidence_pattern -- --nocapture`

Expected: FAIL against a disproved provisional kernel or because the evidence-fixed API is absent.

- [ ] **Step 3: Implement and verify one CPU/WGSL kernel contract**

Add `BiomeBlendKernel::native()`, exact integer offsets/weights/denominator, checked maximum record size, and a WGSL literal contract. Preserve uniform fast paths, missing-neighbour clamping, special foliage, custom-biome fallback, source identities, cross-chunk dirtying, replacement/eviction invalidation, and stale rejection.

```powershell
cargo test -p meshing -p client-world -p render --locked biome -- --nocapture
cargo test -p render --locked biome_shader -- --nocapture
cargo clippy -p meshing -p client-world -p render --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: every abrupt-boundary, corner, missing-neighbour, replacement, eviction, teleport, stale-job, shader, byte-ceiling, and uniform-record test passes.

- [ ] **Step 4: Record reviewed evidence and commit lane-owned files**

```powershell
git add crates/meshing crates/client-world/src/stream/dirty.rs crates/client-world/src/stream/meshing/jobs.rs crates/client-world/src/stream/publication.rs crates/client-world/src/stream/tests/mesh_dependency.rs crates/render/src/biome_tint.wgsl crates/render/tests/biome_shader.rs docs/phase-2-completion-report.md
git commit -m "render: close native biome blend parity"
```

Expected: the evidence row maps to `P2.5-NATIVE-BIOME`. The integration owner applies telemetry/test and master-ledger/roadmap handoffs only after review; the lane does not edit those files.

### Task 5: Correct Only the Measured First Publication Stall

**Files:**
- Required-cohort-identity branch only: modify `crates/client-world/src/stream/model.rs`, `crates/client-world/src/stream/diagnostics.rs`, `crates/client-world/src/stream/tests/cases_03.rs`, `crates/client-world/src/stream/tests/cases_05.rs`; integration handoff adds the raw-radius witness to `app/src/runtime/phase2_evidence.rs` and the remote manifest
- Request-order branch only: create `crates/client-world/src/stream/request_queue.rs`; modify `crates/client-world/src/stream.rs`, `crates/client-world/src/stream/model.rs`, `crates/client-world/src/stream/requests.rs`, `crates/client-world/src/stream/retries.rs`, `crates/client-world/src/stream/polling.rs`, `crates/client-world/src/stream/residency.rs`, `crates/client-world/src/stream/tests/cases_02.rs`, `crates/client-world/src/stream/tests/cases_05.rs`
- Decode/light/mesh branch only: modify the exact owner and test row in the decision table below
- Integration handoff only: `app/src/runtime/world.rs`, `app/src/runtime/network.rs`, `app/src/tests/finish.rs`, `crates/protocol/src/lib.rs`, `crates/protocol/src/world.rs`, `core/proxy/proxy.go`, `core/proxy/proxy_test.go`
- Modify: `docs/phase-2-completion-report.md`

**Interfaces:**
- Produces exactly one measured correction. Required-cohort geometry is corrected only when checkpoint-0 evidence proves the client-derived cohort contains positions the server's raw block radius excludes. Request ordering is added only when checkpoint-0 evidence instead proves ordering or starvation among actual cohort members. A protocol/core change requires a failing cross-language fixture and an integration-owned handoff.

- [ ] **Step 1: Select the branch from immutable checkpoint-0 evidence**

```powershell
$manifest = Get-Content -Raw -LiteralPath .local/phase2/remote/checkpoint0-lunar-attempt-01/manifest.json | ConvertFrom-Json
if ($manifest.schema -cne 'rust-mcbe-phase2-remote-v1') { throw 'wrong checkpoint-0 schema' }
if ($manifest.mode -cne 'Diagnostic' -or $manifest.server -cne 'Lunar') { throw 'wrong checkpoint-0 identity' }
if (-not $manifest.diagnostic_complete) { throw 'Lunar diagnostic sequence is incomplete' }
$manifest.first_stalled_stage
```

Expected: exactly one of `none`, `required_cohort_identity`, `request_order`, `transport`, `wire_contract`, `response_semantics`, `decode`, `lighting`, `meshing`, `main_apply`, `gpu_upload`, `extraction`, `submission`, or `presentation`. `none` skips this task without a behavior commit. Any missing or ambiguous value blocks work.

- [ ] **Step 2: Write the red regression in the selected owner only**

Use this exhaustive decision contract:

| First stalled stage | Red regression | Allowed lane owner |
|---|---|---|
| `required_cohort_identity` | `crates/client-world/src/stream/tests/cases_03.rs::raw_publisher_block_radius_defines_exact_required_cohort` and `crates/client-world/src/stream/tests/cases_05.rs::request_mode_announcements_complete_exact_required_cohort` | cohort model/diagnostics; ceiling chunk radius remains retention-only |
| `request_order` | `crates/client-world/src/stream/tests/cases_05.rs::player_and_visible_retries_precede_far_initial_prefetch_without_losing_fifo_ties` | request queue/requests/retries/residency |
| `transport` | `app/src/tests/finish.rs::phase2_transport_pending_and_sent_ack_remain_fifo` | integration-owned app handoff |
| `wire_contract` | `crates/protocol/tests/world_packets.rs::checkpoint0_subchunk_fixture_matches_core_bytes` and `core/proxy/proxy_test.go::TestCheckpoint0SubChunkFixture` | integration-owned protocol/core handoff |
| `response_semantics` | `crates/client-world/src/stream/tests/cases_05.rs::checkpoint0_outcomes_retry_only_semantically_retryable_entries` | retries/sequencing |
| `decode` | `crates/client-world/src/stream/tests/cases_02.rs::checkpoint0_decode_cohort_makes_bounded_progress` | decode/admission |
| `lighting` | `crates/client-world/src/stream/tests/light_scheduler/cases_02.rs::checkpoint0_light_cohort_makes_bounded_progress` | lighting jobs/state |
| `meshing` | `crates/client-world/src/stream/tests/mesh_dependency.rs::checkpoint0_mesh_cohort_makes_bounded_progress` | meshing jobs/dependencies |
| `main_apply` | `app/src/tests/publication.rs::checkpoint0_main_apply_preserves_cohort_identity` | integration-owned app handoff |
| `gpu_upload` | `crates/render/src/chunk/gpu/queue_tests.rs::checkpoint0_upload_reservation_preserves_cohort_identity` | render queue/upload/layout |
| `extraction` | `crates/render/src/chunk/presentation/tests.rs::checkpoint0_extraction_preserves_generation_manifest` | render extraction/presentation |
| `submission` | `crates/render/src/chunk/presentation/command_tests.rs::checkpoint0_submission_matches_visible_manifest` | render draw/commands |
| `presentation` | `crates/render/src/chunk/presentation/tests.rs::checkpoint0_gpu_completion_matches_submitted_manifest` | render frame probe/presentation |

Run the selected test by its exact final path/name. Expected: FAIL with the checkpoint-0 counter transition reproduced; every preceding and succeeding stage remains healthy.

- [ ] **Step 3: Implement only the selected branch**

For `required_cohort_identity`, first add a behavior-neutral raw `publisher_radius_blocks` witness and rerun the Lunar diagnostic. Once the raw value and stage counts prove the classification, preserve exact block-radius geometry in `ViewCohort`: raw 120 produces 177 columns, raw 128 produces 197, and raw 256 preserves 797. Cover negative/unaligned centers, exclude the exact 20-position outer annulus introduced by rounding 120 blocks up to eight chunks, and prove 177 request-mode LevelChunk announcements complete that Lunar cohort while any actual member missing fails. Retain the ceiling chunk radius only for bounded active-retention calculations. Do not add `RequestClass` or change retry ordering in this branch.

For `request_order`, use this red test and then implement stable `RequestClass` priority, squared horizontal distance within class, and FIFO sequence ties:

```rust
#[test]
fn player_and_visible_retries_precede_far_initial_prefetch_without_losing_fifo_ties() {
    let mut queue = RequestQueue::with_capacity(64);
    queue.push(prefetch_initial(chunk(16, 16), 1)).unwrap();
    queue.push(visible_initial(chunk(2, 0), 2)).unwrap();
    queue.push(player_retry(chunk(0, 0), 3)).unwrap();
    queue.push(player_retry(chunk(0, 0), 4)).unwrap();
    assert_eq!(queue.pop().unwrap().sequence(), 3);
    assert_eq!(queue.pop().unwrap().sequence(), 4);
    assert_eq!(queue.pop().unwrap().sequence(), 2);
    assert_eq!(queue.pop().unwrap().sequence(), 1);
}
```

Also add `continuous_prefetch_cannot_starve_exact_retry` in the same test file. Classify from the last finite player chunk supplied to `WorldStream::poll`; preserve packet, chunk, base Y, count, sequence, retry attempt, reservation, transport-pending, sent-ack, and timeout identities. One failed vertical batch schedules only its expected Y retries within existing ceilings.

For `wire_contract`, do not edit protocol/core in the lane. Store the minimum secret-safe packet bytes below `.local/phase2/wire/`, prove the two red fixture tests above, and hand the exact fixture hashes plus proposed `crates/protocol/src/world.rs` and `core/proxy/proxy.go` hunks to integration. The integration owner runs:

```powershell
cargo test -p protocol --locked --test world_packets checkpoint0_subchunk_fixture -- --nocapture
Push-Location core; try { go test ./proxy/... -run TestCheckpoint0SubChunkFixture -count=1; if ($LASTEXITCODE -ne 0) { throw 'core fixture test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
```

No wire/core commit is permitted when the fixture tests pass before a change.

- [ ] **Step 4: Verify, rerun Lunar, review, and commit the selected owner**

```powershell
cargo test -p client-world -p bedrock-client --locked request -- --nocapture
cargo test -p client-world -p bedrock-client --locked retry -- --nocapture
cargo test -p render --locked publication -- --nocapture
cargo clippy -p client-world -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Lunar -Mode Candidate -RunId measured-fix-lunar-attempt-01 -DurationSeconds 180 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -Assets $env:RUST_MCBE_ASSETS
git add crates/client-world crates/render docs/phase-2-completion-report.md
git commit -m "fix: correct measured chunk publication stall"
```

Expected: the selected stage now progresses, no preceding/succeeding stage regresses, and the row remains `Non-final candidate`. Remove unmodified crate paths from `git add`; app/protocol/core changes, if selected, land only through the integration handoff and its full fixtures.

### Task 5A: Implement bounded Bedrock client blob caching

**Files:**
- Create: `crates/protocol/src/blob_cache.rs`
- Create: `crates/protocol/tests/blob_cache.rs`
- Modify: `crates/protocol/src/login.rs`, `crates/protocol/src/world.rs`, `crates/protocol/src/lib.rs`, `crates/protocol/Cargo.toml`
- Modify the vendored login seam only: `crates/protocol/vendor/jolyne/src/stream/client.rs` and its focused tests
- Modify for live evidence only: `app/src/runtime/phase2_evidence.rs`, `scripts/acceptance/Load.ps1`, `scripts/tests/remote-acceptance.Tests.ps1`
- Modify: `docs/phase-2-completion-report.md`

**Interfaces:**
- Produces a bounded `ClientBlobCache` and pending-transaction resolver owned by `PlaySession`. The cache may survive a reconnect in the same client process because entries are verified content-addressed blobs; pending LevelChunk/SubChunk transactions never survive a session, transfer, or dimension reset.
- Negotiation advertises `ClientCacheStatus { enabled: true }` only when the resolver is installed. Zeqa and deterministic tests retain an explicit disabled route.
- Uses protocol Bedrock blob IDs exactly as xxHash64 payload hashes. A mismatched, duplicate-conflicting, unsolicited, oversized, or unrequested miss response fails closed and cannot poison the cache.

- [ ] **Step 1: Write cache negotiation, bounds, and reconstruction RED tests**

Cover enabled/disabled login packets; stable deduplicated hit/miss lists; repeated hashes; exact byte/entry/transaction ceilings; LRU eviction that never evicts a blob pinned by a pending transaction; disconnect/dimension/transfer pending-state reset; malformed and hash-mismatched miss responses; and FIFO completion when miss responses arrive across multiple packets.

For cached inline LevelChunk, require hash order to reconstruct the sub-chunk blobs followed by the biome blob and then the packet's uncached payload tail. For request-mode cached SubChunk success, reconstruct `blob || entry.payload` so block-entity NBT remains attached; `SuccessAllAir` must not request or read a blob. No event may be published until every referenced miss for that transaction is resolved.

Run:

```powershell
cargo test -p protocol --locked --test blob_cache -- --nocapture
cargo test -p protocol --locked --test login_state client_cache -- --nocapture
```

Expected: FAIL because the client currently sends `enabled=false` and rejects both cached packet families.

- [ ] **Step 2: Implement bounded cache and protocol sequencing**

Use checked accounting and literal ceilings for entry count, total bytes, one blob, hashes per packet, and pending transactions/bytes. Send one `ClientCacheBlobStatus` per accepted cached packet with every unique hash classified exactly once as hit or miss. Validate each miss payload before insertion, resolve every transaction referencing it, retain packet order among simultaneously completed transactions, and expose counters for hits, misses, admitted/rejected blobs, evictions, pending transactions, and reconstructed LevelChunk/SubChunk events.

Do not make `into_world_event` stateful. Resolve cache packets inside `PlaySession`, then pass reconstructed ordinary packets through the existing bounded normalizer. The disabled route must remain byte-for-byte compatible with current Zeqa behavior.

- [ ] **Step 3: Verify deterministic and local cross-language fixtures**

Use the pinned Dragonfly/gophertunnel xxHash64 blob construction only as a fixture generator, not a runtime dependency. Cross-check at least one inline chunk and one request-mode subchunk fixture in Rust and Go, including a cache hit, a cache miss, a repeated hash, block-entity tail bytes, and an invalid hash.

```powershell
cargo test -p protocol -p client-world -p bedrock-client --locked blob_cache -- --nocapture
Push-Location core; try { go test ./... -run ClientBlobCache -count=1; if ($LASTEXITCODE -ne 0) { throw 'core blob-cache fixture failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
cargo clippy -p protocol -p client-world -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

- [ ] **Step 4: Prove Lunar cache behavior before Zeqa comparison**

Run a create-new Lunar Diagnostic after the raw-radius witness and cohort correction. Require `client_blob_cache_enabled=true`, at least one attributable hash, exact `hits + misses = hashes_classified`, every miss resolved, zero rejected/poisoned blobs, zero pending transactions at the coherent terminal sample, and ordinary publication counters continuing from reconstructed events. Then run Zeqa and record whether the server used cache-backed or ordinary payloads; either is accepted only when its observed route completes without stale Lunar pending state.

Commit deterministic implementation separately from ignored-local live evidence. Do not claim this task fixes the independently measured meshing/publication backlog.

### Task 6: Make Publication Service Elapsed-Time and Pressure Aware

**Files:**
- Modify: `crates/render/src/chunk/api.rs`
- Modify: `crates/render/src/chunk/queue.rs`, `crates/render/src/chunk/gpu/upload.rs`, `crates/render/src/chunk/gpu/layout.rs`
- Modify: `crates/render/src/chunk/gpu/queue_tests.rs`, `crates/render/src/chunk/presentation/tests.rs`
- Integration handoff only: `app/src/runtime/publication.rs`, `app/src/tests/publication.rs`, `app/src/runtime/world.rs`, `app/src/acceptance/remesh.rs`, `app/src/acceptance/proofs.rs`, `app/src/acceptance/world_ready.rs`, `app/src/metrics.rs`, `app/src/metrics/report.rs`, `scripts/tests/ui-publication-pressure.Tests.ps1`

**Interfaces:**
- Produces: `PublicationServiceConfig` and a token-based `PublicationController::begin_frame(elapsed)` that guarantees bounded service over wall time while still reducing genuine pressure.
- Preserves: `ChunkUploadBudget` as the shared handoff/application/extraction/GPU-preparation allowance.

- [ ] **Step 1: Write low-FPS and pressure red tests**

```rust
#[test]
fn eight_hz_frames_receive_two_seconds_of_bounded_service_without_runaway_burst() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::new(config);
    let mut serviced = 0usize;
    for _ in 0..16 {
        controller.begin_frame(Duration::from_millis(125));
        serviced += controller.budget().max_per_frame;
        controller.finish_frame(PublicationFrameWork::healthy());
    }
    assert!(serviced >= 6_951);
    assert!(controller.budget().max_per_frame <= config.maximum_frame_items);
    assert!(controller.accrued_items() <= config.maximum_burst_items);
}
```

Add tests for FIFO jitter, one 80 ms pressure frame, recovery without positive feedback, byte tokens, zero-byte removal cap 256, whole-arena copy reservation, and no service accumulation above the one-second burst ceiling.

Run: `cargo test -p bedrock-client --locked eight_hz_frames_receive_two_seconds -- --nocapture`

Expected: FAIL because current service is capped primarily per frame.

- [ ] **Step 2: Implement token accrual and pressure control**

Accrue item and byte tokens from real elapsed time with checked integer arithmetic. Spend the same allowance across `WorldStream::poll`, main-world mesh-change application, render extraction, and GPU preparation. Pressure may lower the target toward `minimum_items_per_second` and `minimum_bytes_per_second` after proven over-target frame time or GPU backlog, but never below either minimum. Clamp each frame and accumulated burst to the literal `PHASE2_GATE` ceilings. Known-air/packed-empty removals spend `maximum_zero_byte_operations_per_frame` and never consume non-empty byte tokens.

- [ ] **Step 3: Prove the exact deterministic cohort**

Extend the production-path test to use 6,951 populated allocations, exact generation manifests, bounded non-empty bytes, known-air removals, and adjacent presented acknowledgements. Assert zero pending/in-flight/stale/duplicate/unacknowledged work and a simulated wall-clock completion at or below two seconds at 8 Hz.

- [ ] **Step 4: Commit lane-owned render accounting**

```powershell
cargo test -p render --locked publication -- --nocapture
cargo test -p client-world --release --locked release_full_view_known_air_lighting_completes_within_two_seconds -- --ignored --nocapture
cargo clippy -p client-world -p render --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/render
git commit -m "perf: account elapsed-time chunk publication service"
```

- [ ] **Step 5: Apply and verify the integration-owned controller handoff**

The integration owner merges the render commit, applies the exact app/controller/acceptance files listed above, registers `PublicationController::new(PublicationServiceConfig::PHASE2_GATE)`, and adds a Phase 5-facing pressure test contract that later opens the settings overlay during a forced full-view remesh. Run:

```powershell
cargo test -p render -p bedrock-client --locked publication -- --nocapture
cargo test -p bedrock-client --locked eight_hz_frames_receive_two_seconds -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/ui-publication-pressure.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
cargo clippy -p client-world -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: deterministic publication tests pass; the Pester contract is green using a synthetic Phase 5 overlay marker and remains a live-open gate until Task 13. The lane then fast-forwards with `git merge --ff-only completion-integration` before continuing.

### Task 7: Run Non-Final Lunar, Then Zeqa, Publication Candidates

**Files:**
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only for validator corrections: `scripts/remote-acceptance.ps1`, `app/src/runtime/phase2_evidence.rs`

**Interfaces:**
- Consumes: the frozen publication snapshot, request priority, and service controller.
- Produces: non-final Lunar and Zeqa release/FIFO publication candidate evidence with exact coherent stage and presented identities. It cannot close the master ledger because Phase 2.7 and integrated UI pressure are not complete.

- [ ] **Step 1: Run the non-final Lunar publication candidate**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Lunar -Mode Candidate -RunId publication-candidate-lunar-attempt-01 -DurationSeconds 300 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -FullViewTeleportGate -Assets $env:RUST_MCBE_ASSETS
```

Expected: current-position spawn region complete without holes/stalls; the initial radius-16 Euclidean publisher disk publishes without a visible stall; publisher-disk, resident, allocation, visible, submitted, and GPU-presented identities agree; no unexplained outcome/error counter; join, teleport settle, and forced full-view remesh each ≤2,000 ms; after the 30-second warmup, the uninterrupted 120-second steady sample has p95 and p99 frame time each ≤`16.6666666667` ms and maximum frame time ≤`50` ms; two adjacent exact presented witnesses; exactly 120 one-second steady resource samples with maximum RSS ≤650 MB and mean/p95 CPU ≤15%; clean shutdown.

- [ ] **Step 2: Run Zeqa only after Lunar passes**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Zeqa -Mode Candidate -RunId publication-candidate-zeqa-attempt-01 -DurationSeconds 300 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -FullViewTeleportGate -Assets $env:RUST_MCBE_ASSETS
```

Expected: authenticated transfer completes, the same publication/presentation/resource gates pass, and the transfer session does not retain stale Lunar cohort or retry state.

- [ ] **Step 3: Record and commit actual evidence**

Write the create-new run-directory manifest hashes, metrics hashes, exact durations, max queue/worker times, outcome counters, backend/adapter/driver, FIFO proof, and presented cohort hashes into `docs/phase-2-completion-report.md`. Mark both rows `Non-final candidate` under `P2-CHUNK-PUBLICATION`; do not edit `plan.md` or the master ledger.

```powershell
git add docs/phase-2-completion-report.md
git commit -m "docs: record phase 2 publication candidates"
```

### Task 8: Calibrate Solved Lighting and Air/Water/Lava Fog

**Files:**
- Modify: `crates/render/src/lighting.wgsl`, `crates/render/tests/plugin.rs`
- Modify: `crates/render/src/atmosphere.rs`, `crates/render/tests/atmosphere.rs`
- Create from accepted reports: `crates/render/tests/fixtures/native-lighting-fog-v1.json`
- Verify unchanged or modify only after a new invariant regression fails: `crates/client-world/src/stream/lighting/types.rs`, `crates/client-world/src/stream/lighting/jobs.rs`, `crates/client-world/src/stream/tests/light_scheduler/cases_01.rs`, `crates/client-world/src/stream/tests/light_scheduler/cases_02.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: `app/src/environment.rs`, `scripts/acceptance/Galleries/Common.ps1`, `scripts/acceptance/Galleries/LightingAtmosphere.ps1`, `scripts/acceptance/Load.ps1`, `scripts/acceptance/Orchestrator.ps1`, `scripts/acceptance/Orchestration/Validate.ps1`, `scripts/acceptance/Orchestration/Execute.ps1`, `scripts/tests/acceptance/Galleries.Tests.ps1`, `scripts/tests/acceptance/Orchestration.Tests.ps1`, `scripts/tests/acceptance/Paths.Tests.ps1`

**Interfaces:**
- Produces: one deterministic lighting/fog gallery manifest and native-fixed constants/curves; does not change `AtmosphereFrame`'s 96-byte ABI unless a failing ABI test proves it cannot express the evidence.

- [ ] **Step 1: Add a failing deterministic gallery-plan test**

The manifest must enumerate light levels 0–15 for block and sky independently, AO corner/edge/face cases, true night/day/rain/thunder, Overworld/Nether/End, and eye samples in air, still/flowing/waterlogged water, and lava. It must include fixed native/Cinnabar crops and environment-profile identities.

Run: `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/acceptance/Galleries.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'`

Expected: FAIL because `New-LightingAtmosphereGalleryPlan` is absent.

- [ ] **Step 2: Apply the reviewed gallery handoff and capture matching views**

The integration owner implements the exact script files listed above, reruns the red Pester test to green, and commits only the shared app/script handoff. Then fast-forward the Phase 2 lane and run:

```powershell
git merge --ff-only completion-integration
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery LightingAtmosphere -RunId lighting-atmosphere-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/lighting-atmosphere-v1 -PresentMode Fifo
```

Expected: the wrapper executes separate complete `cargo run -p phase2-evidence --locked -- compare` commands for `lighting`, `fog-air`, `fog-water`, and `fog-lava`. Every stratum has a report SHA-256, a 30-second warmup followed by a 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms and maximum frame time ≤`50` ms, two exact presented witnesses, and no unexplained diagnostic; no aggregate pass hides a failed medium.

- [ ] **Step 3: Write failing evidence-derived rendering tests**

```rust
#[test]
fn native_light_transfer_and_medium_fog_match_accepted_evidence() {
    let evidence: NativeLightingFogEvidence = serde_json::from_str(include_str!(
        "fixtures/native-lighting-fog-v1.json"
    ))
    .expect("reviewed native lighting/fog evidence fixture");
    for sample in evidence.light_samples {
        assert_close(light_transfer(sample.input), sample.expected_linear, sample.epsilon);
    }
    for sample in evidence.fog_samples {
        assert_eq!(resolve_fog(sample.profile, sample.medium), sample.expected);
    }
}
```

Create the fixture in the same commit from the accepted reports. It contains only literal inputs, literal expected outputs, bounded epsilons, native build identity, manifest hashes, and separate comparator-report hashes for lighting, air fog, water fog, and lava fog. Its parser rejects non-finite values, epsilons outside `0.0..=0.05`, absent or duplicate required labels, invalid SHA-256 strings, and empty strata. Required labels cover light zero, first emitted-light step, full brightness, midnight skylight floor, biome-profile precedence, missing-media fallback, water/lava surface transition, and non-finite/missing-world fail-closed behavior. Re-run the existing `dirty_or_stale_boundary_light_is_untrusted`, `changed_light_face_requeues_exact_resident_neighbour`, `stale_light_value_mesh_is_requeued_but_provenance_identity_is_ignored`, and `eviction_purges_light_ownership_and_stale_completion_cannot_restore_it` tests; add a red case beside the matching invariant before changing a client-world owner.

- [ ] **Step 4: Implement the smallest evidence-proven calibration**

Preserve independent block/sky/AO/daylight channels, face/directional material response, alpha/fog ordering, profile precedence, medium detection, explicit unknown-light boundaries, stale-generation rejection, and lossless exact-neighbour requeueing. Change only named transfer/fog constants or curves disproved by evidence. Malformed profiles and non-finite server/environment inputs fail closed with bounded attributable counters; all shader and uniform floats remain finite and bounded.

- [ ] **Step 5: Verify, review, and commit**

```powershell
cargo test -p render -p bedrock-client --locked atmosphere -- --nocapture
cargo test -p meshing -p client-world -p render --locked lighting -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/acceptance/Galleries.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
cargo clippy -p meshing -p client-world -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/render crates/client-world docs/phase-2-completion-report.md
git commit -m "render: calibrate native lighting and fog"
```

Expected: the report rows map to `P2.7-ATMOSPHERE`; the lane commit contains no app or script entry-point file.

### Task 9: Implement Native-Referenced Precipitation

**Files:**
- Create: `crates/assets/src/precipitation.rs`, `crates/assets/tests/precipitation.rs`
- Create: `crates/asset-compiler/src/precipitation.rs`, `crates/asset-compiler/tests/precipitation.rs`
- Create: `crates/render/src/precipitation.rs`, `crates/render/src/precipitation.wgsl`, `crates/render/tests/precipitation.rs`
- Modify: `crates/render/src/atmosphere_render.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: `crates/assets/src/lib.rs`, `crates/asset-compiler/src/lib.rs`, `crates/render/src/lib.rs`, `app/src/app.rs`, `app/src/environment.rs`, `app/tests/assets.rs`, `Makefile`, root `Cargo.toml`, `Cargo.lock`

**Interfaces:**
- Produces: `PrecipitationFrame { kind, intensity, wind, camera_origin, session_identity }`, immutable precipitation textures/resources, and one bounded instanced draw.
- Integration boundary: precipitation uses independent magic/schema `MCBEPRC1` and never changes the existing `MCBEATM1` atmosphere carrier. Phase 4/5 asset carriers therefore do not need to rebase for a precipitation schema revision; only module exports/startup wiring are integration-owned.

- [ ] **Step 1: Record exact native source and behavior evidence**

Capture rain and snow at intensity 0, partial, and full; camera above/below cover; biome/dimension suppression; wind/motion; fog interaction; and day/night response. Store exact source hashes and decoded dimensions in ignored `.local/phase2/precipitation/manifest.json`. No compiler constant is added before this manifest passes bounds and matching-version checks.

- [ ] **Step 2: Write failing asset and frame tests**

```rust
#[test]
fn precipitation_assets_and_frame_fail_closed_and_remain_bounded() {
    assert!(compile_precipitation(wrong_hash_png()).is_err());
    let frame = PrecipitationFrame::new(PrecipitationKind::Rain, 2.0, [f32::NAN, 0.0]);
    assert_eq!(frame.intensity(), 1.0);
    assert_eq!(frame.wind(), [0.0, 0.0]);
    assert!(frame.instance_count() <= MAX_PRECIPITATION_INSTANCES);
}
```

Run: `cargo test -p assets -p asset-compiler -p render --locked precipitation -- --nocapture`

Expected: FAIL because the types and carrier records are absent.

- [ ] **Step 3: Implement assets, bounded geometry, and pipeline**

Compile only exact matching-version rain/snow inputs into `MCBEPRC1` with canonical logical paths and independent encoded/decoded hashes. `RuntimePrecipitationAssets::decode` rejects `MCBEATM1`, wrong schema/version/hash, duplicate roles, oversized dimensions/bytes, trailing bytes, and noncanonical paths. Render camera-relative bounded cells with deterministic negative-coordinate anchoring, alpha blending, reversed-Z depth testing, no color depth write, weather/fog modulation, and no per-frame resource creation. Suppress or switch kind only from authoritative environment/biome/dimension facts; missing facts fail to no precipitation with an attributable counter.

- [ ] **Step 4: Format, review, and commit the inactive lane-owned modules**

```powershell
rustfmt --edition 2024 --check crates/assets/src/precipitation.rs crates/assets/tests/precipitation.rs crates/asset-compiler/src/precipitation.rs crates/asset-compiler/tests/precipitation.rs crates/render/src/precipitation.rs crates/render/tests/precipitation.rs
git diff --check
git add crates/assets/src/precipitation.rs crates/assets/tests/precipitation.rs crates/asset-compiler/src/precipitation.rs crates/asset-compiler/tests/precipitation.rs crates/render/src/precipitation.rs crates/render/src/precipitation.wgsl crates/render/src/atmosphere_render.rs crates/render/tests/precipitation.rs docs/phase-2-completion-report.md
git commit -m "feat: render bounded native precipitation"
```

Expected: formatting/diff checks pass; the reviewed producer commit contains no module-root export, app, root manifest, lockfile, Makefile, or acceptance entry-point edit. It remains inactive until integration exports it in Step 5.

- [ ] **Step 5: Apply integration exports/startup and run the exact live/native gate**

The integration owner merges the reviewed module commit, applies only the listed exports/startup/root handoff, and runs:

```powershell
cargo test -p assets -p asset-compiler -p render -p bedrock-client --locked precipitation -- --nocapture
cargo test -p bedrock-client --test assets --locked precipitation -- --nocapture
cargo clippy -p assets -p asset-compiler -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all isolated-carrier, renderer, startup, bounds, and no-churn tests pass. After the integration commit, run:

```powershell
git merge --ff-only completion-integration
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Precipitation -RunId precipitation-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/precipitation-v1 -PresentMode Fifo
```

Expected: rain and snow pass intensity 0/partial/full, above/below cover, biome/dimension suppression, wind/motion, fog, day/night, and clear/rain/thunder views. Metrics prove one steady draw, zero steady uploads, bounded instances, a 30-second warmup followed by a 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms and maximum frame time ≤`50` ms, exactly 120 one-second resource samples within 650 MB/15 percent, and two exact presented witnesses. Record the hashes under `P2-PRECIPITATION` → `P2.7-ATMOSPHERE` in a follow-up evidence-only lane commit.

### Task 10: Close Celestial Border and Filter-Edge Parity

**Files:**
- Modify only if evidence fails: `crates/render/src/atmosphere.wgsl`, `crates/render/src/atmosphere_render.rs`
- Modify: `crates/render/tests/atmosphere.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: `scripts/acceptance/Galleries/Celestial.ps1`, `scripts/acceptance/Load.ps1`, `scripts/acceptance/Orchestrator.ps1`, `scripts/acceptance/Orchestration/Validate.ps1`, `scripts/acceptance/Orchestration/Execute.ps1`, `scripts/tests/acceptance/Galleries.Tests.ps1`, `scripts/tests/acceptance/Orchestration.Tests.ps1`, `scripts/tests/acceptance/Paths.Tests.ps1`, master-ledger row `P2.7-ATMOSPHERE`, `plan.md`

**Interfaces:**
- Consumes: decoded pinned sun and all eight moon tiles already carried by `MCBEATM1`.
- Produces: native/GDI proof for bright sky, dark sky, horizon, and minification/filter-edge views without RGB-keyed rectangles.

- [ ] **Step 1: Add the exact gallery manifest and decoded-pixel red test**

Require 18 views: sun against bright/dark skies, plus eight moon phases against both bright and dark skies. Each body includes center, exact tile border, one-texel-outside, horizon, and minified crops. The Rust test decodes the actual pinned texture fixture and verifies additive composition for near-black border texels rather than string-inspecting WGSL.

Run: `cargo test -p render --locked celestial -- --nocapture`

Expected: the existing decoded additive unit contract passes; the new complete gallery-manifest test fails until all 18 views exist.

- [ ] **Step 2: Capture and compare release/native views**

After the integration owner applies the reviewed gallery handoff and the Pester manifest test passes, run:

```powershell
git merge --ff-only completion-integration
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Celestial -RunId celestial-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/celestial-v1 -PresentMode Fifo
```

Expected: for every manifested view, the wrapper substitutes that view's concrete create-new run paths into a complete `cargo run -p phase2-evidence --locked -- compare` invocation with `--kind celestial`, `--manifest`, `--native`, `--cinnabar`, and `--out`. All 18 views and their center/border/outside/horizon/minified crops pass in release/FIFO after a 30-second warmup and 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms, maximum frame time ≤`50` ms, and two exact frames. If a crop fails, add a red shader/sampler test for the proven blend, UV, sampler, mip, or composition-order discrepancy only.

- [ ] **Step 3: Verify and commit evidence**

```powershell
cargo test -p render -p bedrock-client --locked atmosphere -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/acceptance/Galleries.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/render docs/phase-2-completion-report.md
git commit -m "test: close native celestial edge parity"
```

Expected: the lane commit contains no script, roadmap, or master-ledger file; its evidence maps to `P2.7-ATMOSPHERE`.

### Task 11: Calibrate and Finish Native Cloud Geometry

**Files:**
- Modify: `crates/render/src/cloud_config.rs`, `crates/render/tests/cloud_config.rs`
- Create from accepted reports: `crates/render/tests/fixtures/native-cloud-calibration-v1.json`
- Modify: `crates/meshing/src/cloud.rs`, `crates/meshing/tests/cloud_mesh.rs`
- Modify: `crates/render/src/cloud_render.rs`, `crates/render/src/cloud.wgsl`
- Modify: `crates/render/tests/{cloud_render,atmosphere}.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: `scripts/acceptance/Galleries/Cloud.ps1`, `scripts/acceptance/Load.ps1`, `scripts/acceptance/Orchestrator.ps1`, `scripts/acceptance/Orchestration/Validate.ps1`, `scripts/acceptance/Orchestration/Execute.ps1`, `scripts/tests/acceptance/Galleries.Tests.ps1`, `scripts/tests/acceptance/Orchestration.Tests.ps1`, `scripts/tests/acceptance/Paths.Tests.ps1`, master-ledger row `P2.7-ATMOSPHERE`, `plan.md`

**Interfaces:**
- Consumes: existing exact native `CloudQuality` records and local-only 1.26.33.1 cloud asset identity.
- Produces: a fully calibrated `CloudCalibrationReport`; `CloudRenderConfig::native(quality)` drives bounded world coverage, while one immutable packed mesh and one draw remain unchanged.

- [ ] **Step 1: Capture all discriminating native views**

For Low/Medium/High/Ultra, record fixed camera/world coordinates below, above, within, grazing, at positive/negative period crossings, and at the distance-fog edge. Capture clear, rain, thunder, day, and night. Feed exact matching-view and occupancy semantics to the existing `CloudCalibrationHarness`.

- [ ] **Step 2: Write failing calibration tests**

```rust
#[test]
fn calibrated_native_configs_cover_matching_views_without_guessing_grid_semantics() {
    let evidence: NativeCloudCalibrationEvidence = serde_json::from_str(include_str!(
        "fixtures/native-cloud-calibration-v1.json"
    ))
    .expect("reviewed native cloud calibration fixture");
    let report = CloudCalibrationHarness::from_evidence(evidence).publish().unwrap();
    for quality in CloudQuality::ALL {
        let record = report.record(quality);
        assert!(record.coverage_contains(record.matching_view()));
        assert_eq!(record.config().mesh_size(), 64);
    }
}
```

The fixture contains literal origin/count/distance records for every quality, including negative-coordinate and seam samples, plus matching-view report SHA-256, native build identity, and the exact cloud asset SHA-256. Its parser rejects missing qualities, duplicate labels, invalid hashes, non-finite distances, counts above the checked instance ceiling, and mesh sizes other than 64. Run: `cargo test -p render -p meshing --locked cloud -- --nocapture`.

Expected: FAIL while `CLOUD_GEOMETRY_EVIDENCE` remains `calibrated=false` and provisional 256-period 3×3 coverage is used.

- [ ] **Step 3: Implement config-driven bounded coverage**

Replace provisional origins/period/height only with calibrated values. Preserve eight-byte records, exact occupancy, toroidal seam culling, transparent sorted phase, directional sunlight, exact rain/thunder contributions, fog, negative-coordinate determinism, one draw, immutable GPU records, and checked quad/byte/instance ceilings. Emit `calibrated=true` only when runtime config, asset identity, matching-view report hash, uploaded records, and bounds all agree.

- [ ] **Step 4: Verify and run the live gallery**

```powershell
cargo test -p meshing -p render -p bedrock-client --locked cloud -- --nocapture
cargo clippy -p meshing -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

After the integration owner applies the reviewed Cloud gallery handoff and Pester tests pass, run:

```powershell
git merge --ff-only completion-integration
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Cloud -RunId cloud-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/cloud-v1 -PresentMode Fifo
```

Expected: Low/Medium/High/Ultra each pass above/below/within/grazing, positive/negative period crossing, and fog-edge views across clear/rain/thunder/day/night. Metrics prove no edge pop, accepted thickness/density/silhouette/face shading/motion/fog/seams, one steady draw, zero steady uploads, a 30-second warmup followed by a 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms and maximum frame time ≤`50` ms, exactly 120 one-second resource samples within 650 MB/15 percent, and two exact frames.

- [ ] **Step 5: Review and commit**

```powershell
git add crates/meshing crates/render docs/phase-2-completion-report.md
git commit -m "render: calibrate native finite clouds"
```

Expected: the lane commit contains no script, roadmap, or master-ledger file; its evidence maps to `P2.7-ATMOSPHERE`.

### Task 12: Classify and Eliminate the Motion Artifact

**Files:**
- Integration handoff only, already established in Task 3: `scripts/phase2-motion-ab.ps1`, `scripts/tests/phase2-motion-ab.Tests.ps1`
- Modify only in the decision-table branch proved by the A/B report:
  - cave visibility, integration handoff only: `app/src/runtime/visibility.rs`, `app/src/tests/finish.rs`
  - frustum/draw submission: `crates/render/src/chunk/draw.rs`, `crates/render/src/chunk/pipeline/commands.rs`, `crates/render/src/chunk/presentation/command_tests.rs`
  - GPU-completion acknowledgement: `crates/render/src/chunk/presentation/frame_probe.rs`, `crates/render/src/chunk/presentation/tests.rs`
  - requested/effective present mode: lane-owned `crates/render/src/chunk/gpu/types.rs`, `crates/render/src/chunk/presentation/tests.rs`; integration handoff `app/src/app.rs`, `app/src/acceptance/markers.rs`, `app/src/tests/core.rs`
- Modify: `docs/phase-2-completion-report.md`
- Integration handoff only: master-ledger row `P2.7-ATMOSPHERE`, `plan.md`

**Interfaces:**
- Produces: paired FIFO/Immediate evidence with identical scene/camera path, build, adapter, driver, assets, resident/cave/frustum/submitted/GPU-completed identities, and native video/capture hashes.

- [ ] **Step 1: Write the A/B runner red test**

The test must reject pairs with different scene hash, camera path, build hash, adapter/driver, assets, duration, resolution, or unproven effective present mode. It must require coherent per-frame stage identities and temporary capture hashes for both runs.

Run: `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/phase2-motion-ab.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'`

Expected: FAIL because the paired runner is absent.

- [ ] **Step 2: Implement and run the identical-scene pair**

Run the exact create-new paired capture:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-motion-ab.ps1 -RunId motion-ab-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/motion-v1 -DurationSeconds 180
```

Expected: the script runs FIFO first and Immediate second on one release binary and identical scene/camera/build/adapter/driver/assets/duration/resolution, proves each effective present mode, records the exact artifact frame, and compares resident, cave-visible, frustum-visible, submitted, and GPU-completed sets for that coherent frame. Each leg contains a 30-second warmup followed by an uninterrupted 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms and maximum frame time ≤`50` ms. It rejects an existing `.local/phase2/motion/motion-ab-attempt-01/` directory.

- [ ] **Step 3: Bind the fix to the proven class**

Use this decision contract:

- Identical complete manifests, artifact only in Immediate: fix/disable unsupported Immediate presentation behavior; do not alter culling.
- Missing frustum key with resident/cave key present in both modes: add a red `runtime/visibility` or render visibility test and correct the exact transform/bounds decision.
- Submitted key missing with frustum key present: add a red render queue test and correct queue/application identity.
- GPU-completed key missing with submitted key present: add a red presentation/acknowledgement test and correct render-graph or callback identity.
- Every identity complete but both captures show the band: add a shader/depth/clear/load-operation regression for the isolated pixel region; do not relabel it presentation tearing.

No other subsystem changes in this task.

- [ ] **Step 4: Verify, rerun, review, and commit**

```powershell
cargo test -p render -p bedrock-client --locked visibility -- --nocapture
cargo test -p render -p bedrock-client --locked presentation -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/phase2-motion-ab.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/render docs/phase-2-completion-report.md
git commit -m "fix: eliminate coherent-frame motion artifact"
```

Expected live result: the FIFO release capture has no moving void/static band; the A/B report explains Immediate behavior without unexplained stage divergence. If the selected fix is app-owned, the lane makes an evidence-only commit and the integration owner applies/tests the app handoff before that result is accepted.

### Task 13: Run the Complete Phase 2 Integration Matrix

**Files:**
- Modify: `docs/phase-2-completion-report.md`
- Modify only if its validator is proven incorrect: lane-owned `tools/phase2-evidence/src/lib.rs`, `tools/phase2-evidence/tests/cli.rs`; do not weaken a gate to make a run pass
- Integration handoff only for validator corrections: `scripts/remote-acceptance.ps1`, `scripts/tests/remote-acceptance.Tests.ps1`, `scripts/phase2-gallery.ps1`, `scripts/tests/phase2-gallery.Tests.ps1`, `scripts/phase2-motion-ab.ps1`, `scripts/tests/phase2-motion-ab.Tests.ps1`, `scripts/acceptance/Galleries/LightingAtmosphere.ps1`, `scripts/acceptance/Galleries/Celestial.ps1`, `scripts/acceptance/Galleries/Cloud.ps1`, `scripts/tests/acceptance/Galleries.Tests.ps1`
- Integration handoff only for closure: `docs/evidence/phases-2-5-completion-ledger.md`, `plan.md`

**Interfaces:**
- Consumes: every accepted deterministic/live/native artifact above.
- Produces: a review-clean Phase 2 evidence handoff. Only the integration lane produces the integrated closure commit and updates master IDs/roadmap.

- [ ] **Step 1: Run full deterministic verification**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo test -p client-world --release --locked release_full_view_known_air_lighting_completes_within_two_seconds -- --ignored --nocapture
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
cargo run -p devtool --locked -- verify-affected --base d8e4699
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
Push-Location tools/chunkfix; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'chunkfix go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'chunkfix go vet failed' } } finally { Pop-Location }
Push-Location tools/bedsimtrace; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'bedsimtrace go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'bedsimtrace go vet failed' } } finally { Pop-Location }
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
bash scripts/tests/acceptance_test.sh
git diff --check
```

Expected: zero failures, warnings, formatting differences, policy failures, or diff errors.

- [ ] **Step 2: Run exact final local BDS/native galleries from one binary**

```powershell
cargo build --workspace --release --locked
$client = (Resolve-Path target/release/bedrock-client.exe).Path
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Biome -RunId final-biome-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/biome-boundary-v1 -PresentMode Fifo -ClientExecutable $client -SkipClientBuild
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery LightingAtmosphere -RunId final-lighting-atmosphere-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/lighting-atmosphere-v1 -PresentMode Fifo -ClientExecutable $client -SkipClientBuild
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Precipitation -RunId final-precipitation-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/precipitation-v1 -PresentMode Fifo -ClientExecutable $client -SkipClientBuild
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Celestial -RunId final-celestial-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/celestial-v1 -PresentMode Fifo -ClientExecutable $client -SkipClientBuild
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-gallery.ps1 -Gallery Cloud -RunId final-cloud-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/cloud-v1 -PresentMode Fifo -ClientExecutable $client -SkipClientBuild
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/phase2-motion-ab.ps1 -RunId final-motion-ab-attempt-01 -BdsDir $env:RUST_MCBE_BDS_DIR -Assets $env:RUST_MCBE_ASSETS -NativeRoot .local/phase2/native/motion-v1 -DurationSeconds 180 -ClientExecutable $client -SkipClientBuild
```

Expected: every create-new directory proves the same client SHA-256, release profile, FIFO for binding captures, accepted native reports, two exact frames, the required 30-second warmup plus uninterrupted 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms and maximum frame time ≤`50` ms, no diagnostic texture, and no unexplained decode/normalization/stale/capacity/retry/fallback failure.

- [ ] **Step 3: Hand the integrated UI-publication-pressure and final Lunar/Zeqa gates to integration**

After Phase 5 settings is integrated with `EnvironmentQualitySettings`, the integration owner uses the same release binary and runs Lunar first:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Lunar -Mode Final -RunId final-integrated-lunar-attempt-01 -DurationSeconds 300 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -FullViewTeleportGate -OpenSettingsOverlay -Assets $env:RUST_MCBE_ASSETS -ClientExecutable $client -SkipClientBuild
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/ui-publication-pressure.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/remote-acceptance.ps1 -Server Zeqa -Mode Final -RunId final-integrated-zeqa-attempt-01 -DurationSeconds 300 -AuthCache .local/auth/microsoft-token.json -InitialRadius 16 -PresentMode Fifo -FullViewTeleportGate -OpenSettingsOverlay -Assets $env:RUST_MCBE_ASSETS -ClientExecutable $client -SkipClientBuild
```

Expected: Zeqa is rejected unless the final Lunar manifest passes. Both prove zero persistent holes/stalls, radius-16 publisher coherence, join, teleport settle, and forced full-view remesh each ≤2,000 ms, two exact frames, and—after a 30-second warmup—one uninterrupted 120-second steady sample with p95 and p99 each ≤`16.6666666667` ms, maximum frame time ≤`50` ms, and exactly 120 one-second resource samples whose maximum RSS is ≤650 MB and mean/p95 CPU are ≤15 percent. `P2-UI-PUBLICATION-PRESSURE` additionally proves the settings overlay remains open throughout the forced remesh, the minimum service rates are maintained, the overlay remains responsive, and no UI code mutates client-world queue ownership.

- [ ] **Step 4: Self-audit the requirement ledger**

For every Phase 2 sub-gate in `docs/phase-2-completion-report.md`, verify its master ID, concrete test, run, capture/report hash, metric, reviewed commit, and outcome. The handoff for `P2.5-NATIVE-BIOME`, `P2-CHUNK-PUBLICATION`, and `P2.7-ATMOSPHERE` lists exact rows; a missing artifact keeps the master ledger and roadmap open.

- [ ] **Step 5: Request independent review and commit final evidence**

```powershell
git add docs/phase-2-completion-report.md
git commit -m "docs: publish final phase 2 evidence handoff"
```

Do not mark a master ID complete until independent review approves the complete Phase 2 behavior range and the integration owner validates the shared handoff, integrated UI pressure run, and final Lunar/Zeqa/native evidence.

---

## Integration Boundaries and Cross-Lane Dependencies

1. **Phase 3 camera and collision:** Phase 2 must preserve `WorldStream::collision_store`, current-dimension/session identity, and `WorldStream::poll(camera_position, max_mesh_jobs)`. If publication needs a richer camera/cohort input, add a new value object at integration checkpoint 1 rather than changing these signatures under Phase 3.
2. **Phase 3 semantic input and app scheduling:** Phase 2 evidence emission is a separate system/resource. The lane never edits `app/src/app.rs`, `app/src/runtime/world.rs`, or camera-derived environment ordering; it hands exact hunks/tests to integration after the Phase 3 action/camera interface freezes.
3. **Phase 4/5 asset work:** Precipitation uses independent `MCBEPRC1`; it never revs `MCBEATM1` or another lane’s carrier. New module exports, startup, root manifest, lockfile, and Makefile changes are integration handoffs, so Phase 4/5 asset lanes do not rebase for its schema.
4. **Phase 5 settings:** Phase 2 owns the Bevy/wgpu-free `assets::environment_settings::{CloudQuality, PrecipitationQuality, EnvironmentQualitySettings}` carrier, `CloudRenderConfig`, fog calibration, and renderer bounds. Phase 5 depends on `assets`, never `render`, and persists/submits those enums through `EnvironmentQualitySettings`; numeric `u8` surrogates, duplicated renderer constants, and direct GPU mutation are forbidden.
5. **Phase 5 UI/render performance:** Phase 2 publication budgets and the settings overlay share frame time, not queue ownership. The final integration gate holds the overlay open during Lunar radius-16 forced remesh and proves minimum service, ≤2,000 ms join/teleport/remesh, a 30-second warmup plus 120-second steady sample with p95/p99 ≤`16.6666666667` ms and max ≤`50` ms, and UI responsiveness without a client-world scheduling edit from Phase 5.
6. **Acceptance scripts:** Every gallery/remote/motion entry or shared library change is an integration handoff. Task 3 integrates one canonical create-new run-directory and validation surface before any live capture; later Phase 2 tasks consume it without staging scripts.
7. **Protocol/core:** The lane changes neither vendored packet dispatch nor Go relay behavior by default. Only the `wire_contract` decision branch may request an integration-owned change, and both `world_packets` and Go proxy fixtures must fail on the same secret-safe checkpoint-0 bytes before that handoff exists.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-17-phase-2-completion.md`. Execute with `superpowers:subagent-driven-development` for one fresh worker and two-stage review per task, or with `superpowers:executing-plans` in checkpointed batches. Do not run independent tasks concurrently when they touch an integration boundary listed above.
