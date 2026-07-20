# Visual Coverage Strict Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the final deterministic `visualcoverage strict` library and CLI gate that rejects every non-air diagnostic, unsupported family, invisible-state laundering, empty visible route, and transitive diagnostic material/texture reference in the exact protocol-1001 corpus.

**Architecture:** Reuse `assets::read_registry`, `RuntimeAssets::decode`, the existing exact inventory analyzer, and the reviewed baseline. The strict path first runs the production ratchet/inventory checks, then validates each resolved visual's complete draw graph and emits one deterministic hash-bound report. `RuntimeAssets::decode` remains the authority for bounded MCBEAS04 layout and cross-reference validity; the strict gate adds semantic reachability requirements rather than duplicating the codec.

**Tech Stack:** Rust 2024 workspace, `assets`, `visualcoverage`, Clap, Serde, SHA-256, integration tests.

## Global Constraints

- The protocol-1001 corpus is exactly 1,356 names, 16,913 canonical states, one air state, and 16,912 non-air states.
- Use production `assets::read_registry` and `RuntimeAssets::decode`; do not duplicate BREG1003 or MCBEAS04 parsing.
- The checked baseline binds the exact registry hash, state inventory, and source-cited invisible allowlist.
- A strict success contains zero diagnostic non-air states, no unsupported model family, explicit no-draw air, and no visible empty route.
- No non-diagnostic drawable may transitively reach material or texture slot zero.
- Every material, template, quad, animation, frame, page, and layer traversal is bounded by the already-decoded production tables.
- Reports are deterministic JSON and bind both BREG and MCBEAS hashes.
- No Mojang assets, screenshots, or generated real-pack blobs are committed.

---

### Task 1: Strict semantic graph validator and CLI

**Files:**
- Modify: `tools/visualcoverage/src/lib.rs`
- Modify: `tools/visualcoverage/src/main.rs`
- Modify: `tools/visualcoverage/tests/ratchet.rs`

**Interfaces:**
- Consumes: `Baseline`, `RegistryRecord`, `RuntimeAssets`, `ratchet_protocol_1001`, and the public material/template/quad/animation/frame/page accessors.
- Produces: `STRICT_REPORT_SCHEMA`, `StrictReport`, `StrictStateRoute`, `RenderStream`, `strict_bytes(&[u8], &[u8], &Baseline) -> Result<StrictReport, CoverageError>`, and CLI subcommand `strict --registry ... --assets ... --baseline ... --out ...`.

- [ ] **Step 1: Write failing strict-route tests**

Add fixture builders that can create valid cube, model/cross, liquid, invisible, and diagnostic visuals with real nonzero materials and bounded templates. Add tests named:

```rust
strict_rejects_non_air_diagnostics_and_unknown_families
strict_requires_air_no_draw_and_source_cited_invisibles
strict_rejects_empty_or_diagnostic_cube_model_and_liquid_routes
strict_reports_exact_render_stream_material_template_and_animation_routes
strict_json_is_hash_bound_sorted_and_byte_identical
strict_cli_rejects_the_current_real_pack_until_zero_diagnostics
```

Synthetic fixtures may call a non-protocol helper `strict_records` so individual graph failures stay small. The production `strict_bytes` and CLI must enforce `PROTOCOL_1001_COUNTS` and the exact reviewed baseline. The real-pack CLI test must assert failure and no output file; it must not copy or commit the pack.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```powershell
cargo test -p visualcoverage --test ratchet strict_ --locked -- --nocapture
```

Expected: compilation/test failure because `strict_bytes`, strict report types, error variants, and CLI command do not exist.

- [ ] **Step 3: Add strict report types and semantic errors**

Add deterministic public types with ordered vectors/maps only:

```rust
pub const STRICT_REPORT_SCHEMA: &str = "cinnabar-visual-coverage-strict-v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStream { NoDraw, Cube, Model, Liquid }

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StrictStateRoute {
    pub state: StateIdentity,
    pub visual_kind: String,
    pub render_stream: RenderStream,
    pub material_ids: Vec<u32>,
    pub model_template: Option<u32>,
    pub animation_ids: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StrictReport {
    pub schema: &'static str,
    pub protocol: u32,
    pub registry_sha256: String,
    pub assets_sha256: String,
    pub counts: Counts,
    pub routes: Vec<StrictStateRoute>,
    pub invisible_decisions: Vec<InvisibleDecision>,
    pub states_by_stream: BTreeMap<RenderStream, usize>,
}
```

Add precise `CoverageError` variants for diagnostics, unsupported families, invalid air/invisible routes, empty visible routes, and diagnostic transitive references. Errors must include the exact `StateIdentity` and offending ID/kind where applicable.

- [ ] **Step 4: Implement the strict traversal without duplicating codecs**

Implement an internal validator with this flow:

```rust
fn strict_records(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
    enforce_protocol_1001: bool,
) -> Result<StrictReport, CoverageError>
```

1. Run `ratchet` for synthetic tests or `ratchet_protocol_1001` for production; retain its exact invisible decisions.
2. Sort records by sequential ID and pair each record with both sequential and hashed resolution; reject any mismatch.
3. Reject `ModelFamily::Unknown` and any `VisualKind::Diagnostic` for a non-air state.
4. Require the single AIR-flagged state to resolve as `Invisible`/`ContributorRole::Air`, with no model or animation and only sentinel-zero face references. Apply the same no-draw-reference rule to reviewed non-air invisibles.
5. For `Cube`, require six nonzero face materials and record the deduplicated sorted material IDs.
6. For `Cross`/`Model`, require one nonempty template, traverse its checked quad range, and require every quad material to be nonzero.
7. For `Liquid`, require six nonzero face materials, a valid depth variant, and material routes that are consistently either blend-water or depth-writing lava; reject mixed/unsupported liquid material families.
8. For every reached material, require a nonzero texture reference. If animated, traverse the checked animation frame range, require at least one frame, and require each frame texture reference to be nonzero. Runtime decode already proves page/layer bounds; record the reached animation IDs.
9. Map visible kinds to exactly one stream (`Cube`, `Model`, `Liquid`) and invisibles to `NoDraw`; emit routes in sequential-ID order and stream counts in a `BTreeMap`.

Implement:

```rust
pub fn strict_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
    baseline: &Baseline,
) -> Result<StrictReport, CoverageError>
```

It must decode each production input once, compute lowercase SHA-256 identities, call `analyze_records`, and invoke `strict_records(..., true)`.

- [ ] **Step 5: Add the CLI command**

Add a `Strict` Clap variant with `--registry`, `--assets`, `--baseline`, and `--out`. Use the existing bounded readers and `parse_baseline`, call `strict_bytes`, serialize through `deterministic_json`, and write the output only after complete success. Do not create or truncate `--out` on failure.

- [ ] **Step 6: Run focused and full verification**

Run:

```powershell
cargo test -p visualcoverage --test ratchet strict_ --locked -- --nocapture
cargo test -p visualcoverage --locked
cargo clippy -p visualcoverage --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all synthetic strict tests and the existing 11 ratchet tests pass; Clippy, formatting, and diff checks are clean. The current pinned real pack remains an intentional strict failure until the residual diagnostic count reaches zero.

- [ ] **Step 7: Commit**

```powershell
git add tools/visualcoverage/src/lib.rs tools/visualcoverage/src/main.rs tools/visualcoverage/tests/ratchet.rs docs/superpowers/plans/2026-07-13-visualcoverage-strict.md
git commit -m "feat(tools): add strict vanilla visual coverage gate"
```
