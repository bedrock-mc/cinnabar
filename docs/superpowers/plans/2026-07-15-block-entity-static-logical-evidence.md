# Block-entity static/logical evidence implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove six protocol-1001 block-entity producers without adding a new geometry pipeline: Barrel, BlastFurnace, Furnace, Smoker, Jukebox, and id-less Note.

**Architecture:** A hash-bound evidence catalog replaces arbitrary manifest witness strings. A bounded typed NBT scan and pure backing-state classifier adjudicate the six runtime routes. A block-entity-specific acceptance request then binds exact palette/NBT targets to two adjacent packed-renderer GPU receipts before the manifest can report six proven and sixteen deferred entries.

**Tech Stack:** Rust 1.93.1, Bevy 0.18/WGPU, Go 1.25 modules, PowerShell acceptance harness, canonical JSON/SHA-256.

## Global Constraints

- Runtime paletted chunk data stays palette-native; never add a flat 4,096-block staging array.
- Do not add per-block-entity Bevy meshes, materials, bind groups, GPU buffers, or render phases.
- Existing packed chunk cube/model streams remain the only backing-block draw authority.
- Mojang assets, BDS payloads, screenshots, and local acceptance artifacts remain untracked.
- `Jukebox` and `Note` mean zero **additional** block-entity references while their backing cubes remain GPU-presented; they are not invisible blocks.
- An absent NBT `id` identifies Note only with the exact noteblock backing identity and typed `note` in `0..=24` plus typed `powered`. Same-named fields on explicitly identified other block entities are extension data and must not make their records malformed.
- Reviewed output after this tranche is exactly six proven and sixteen deferred; global strict-final remains red.
- Every production change follows red-green-refactor and receives one focused independent review cycle.

---

### Task 1: Hash-bound evidence catalog join

**Files:**
- Create: `tools/blockentitygen/evidence.go`
- Create: `tools/blockentitygen/evidence_test.go`
- Create: `docs/evidence/block-entity-render-evidence-v1001.json`
- Modify: `tools/blockentitygen/main.go`
- Modify: `tools/blockentitygen/main_test.go`
- Modify: `tools/blockentitygen/testdata/block-entities-v1001.json`
- Modify: `tools/blockentitygen/testdata/block-entity-coverage-v1001-report.json`
- Modify: `.gitattributes`

**Interfaces:**
- Produces: `readEvidenceCatalog(path string) (evidenceCatalog, error)`.
- Produces: `joinEvidence(manifest rendererManifest, catalog evidenceCatalog, identities evidenceIdentities) (rendererManifest, error)`.
- Produces CLI flag: `-evidence-catalog`, defaulting to `docs/evidence/block-entity-render-evidence-v1001.json`.
- Catalog records use `source_key`, `variant_id`, `kind`, `witness_id`, exact artifact hashes, target palette/NBT identity, and two adjacent `frameReceipt` values.

- [ ] **Step 1: Write failing catalog-boundary tests**

Add table-driven tests that construct one canonical six-route catalog and mutate one boundary at a time. Required failures are: oversized file, unknown witness, duplicate witness, wrong source, wrong variant, wrong kind, wrong BREG/MCBEAS/inventory/manifest/request hash, wrong state identity, wrong NBT digest, non-adjacent frame generations, frame identity drift, wrong stream, wrong backing count, and nonzero additional-reference count.

```go
func TestEvidenceCatalogRejectsCrossIdentityAndReceiptDrift(t *testing.T) {
    valid := validEvidenceCatalog(t)
    tests := []struct {
        name string
        edit func(*evidenceCatalog)
        want string
    }{
        {"wrong source", func(c *evidenceCatalog) { c.Records[0].SourceKey = "Chest" }, "evidence source mismatch"},
        {"stale assets", func(c *evidenceCatalog) { c.MCBEASSHA256 = strings.Repeat("0", 64) }, "MCBEAS hash mismatch"},
        {"non-adjacent", func(c *evidenceCatalog) { c.Records[0].Frames[1].FrameGeneration += 1 }, "frames are not adjacent"},
        {"extra draw", func(c *evidenceCatalog) { c.Records[0].Frames[1].AdditionalRefCount = 1 }, "additional block-entity references"},
    }
    // Each mutation must fail joinEvidence with the named diagnostic.
}
```

- [ ] **Step 2: Run the focused tests and observe RED**

Run from `tools/blockentitygen`:

```powershell
$env:GOWORK='off'
go test ./... -run EvidenceCatalog -count=1 -v
```

Expected: compile failure because `evidenceCatalog`, `readEvidenceCatalog`, and `joinEvidence` do not exist.

- [ ] **Step 3: Implement bounded canonical catalog decoding and joins**

Use explicit structs, `json.Decoder.DisallowUnknownFields`, a 4 MiB file limit, exact 64-lowercase-hex validation, sorted unique witness ownership, and checked integer conversions. Derive canonical `source_contract_sha256` and `renderer_contract_sha256` projections that exclude renderer status, implementation/gallery symbols, evidence IDs, and catalog hashes; raw final inventory/manifest hashes would be circular once evidence is promoted and must not be used as inputs. A receipt is valid only when both frames bind the same target/state/NBT/stream/count/digest and `second.frame_generation == first.frame_generation + 1`.

```go
type frameReceipt struct {
    FrameGeneration          uint64 `json:"frame_generation"`
    ViewGeneration           uint64 `json:"view_generation"`
    BackingStream            string `json:"backing_stream"`
    BackingRefCount          uint64 `json:"backing_ref_count"`
    AdditionalRefCount       uint64 `json:"additional_block_entity_ref_count"`
    PresentedDigestSHA256    string `json:"presented_digest_sha256"`
}

type evidenceRecord struct {
    WitnessID       string         `json:"witness_id"`
    SourceKey       string         `json:"source_key"`
    VariantID       string         `json:"variant_id"`
    Kind            string         `json:"kind"`
    NBTSHA256       string         `json:"nbt_sha256"`
    CanonicalState  string         `json:"canonical_state"`
    SequentialID    uint32         `json:"sequential_id"`
    NetworkHash     uint32         `json:"network_hash"`
    Position        [3]int32       `json:"position"`
    Frames          [2]frameReceipt `json:"frames"`
}
```

Keep the initially tracked catalog canonical and empty. Reviewed generation must continue to report 0 proven/22 deferred until real receipts land. Synthetic fixtures exist only in tests.

- [ ] **Step 4: Run focused and full isolated Go gates**

```powershell
$env:GOWORK='off'
gofmt -w .
go test ./... -count=1
go vet ./...
```

Expected: all tests and vet pass; deterministic existing artifacts remain byte-identical.

- [ ] **Step 5: Commit Task 1**

```powershell
git add .gitattributes tools/blockentitygen docs/evidence/block-entity-render-evidence-v1001.json
git commit -m "feat: bind block entity claims to render evidence"
```

---

### Task 2: Typed Note identity and six-route runtime classifier

**Files:**
- Modify: `crates/world/src/block_entity.rs`
- Modify: `crates/world/tests/block_entity.rs`
- Create: `app/src/block_entity_visuals.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/world_stream.rs`
- Modify: `app/tests/assets.rs`

**Interfaces:**
- Produces a small `RootByteCandidate`-style value (`Absent`, one typed byte, or `Invalid`) through `BlockEntityNbt::note_candidate()` and `BlockEntityNbt::powered_candidate()`; it is discriminator metadata, not a general decoded NBT tree.
- Produces: `adjudicate_block_entity_visual(source: &BlockEntityNbt, backing: BackingBlockIdentity) -> BlockEntityVisualRoute`.
- `BlockEntityVisualRoute` variants are `ExistingBlockState`, `LogicalNoAdditionalDraw`, `Deferred`, and `Unknown` and carry a stable route digest.

- [ ] **Step 1: Write failing NBT scalar tests**

Cover Note pitches `0`, `24`, `25`, typed powered false/true, wrong tag types, duplicates, unrelated absent-id compounds, explicitly identified non-Note compounds with same-named extension fields, and exact-byte retention. The bounded decoder records invalid candidates without globally rejecting those extension fields; the Note classifier is where wrong types, duplicates, and out-of-range values fail closed.

```rust
#[test]
fn idless_note_requires_bounded_typed_note_fields() {
    let nbt = decode_root(&idless_note_nbt(24, true));
    assert_eq!(nbt.id(), None);
    assert_eq!(nbt.note_candidate(), RootByteCandidate::Value(24));
    assert_eq!(nbt.powered_candidate(), RootByteCandidate::Value(1));
    assert!(matches!(
        adjudicate_block_entity_visual(&decode_root(&idless_note_nbt(25, true)), noteblock_identity()),
        BlockEntityVisualRoute::Unknown { .. }
    ));
}
```

- [ ] **Step 2: Run the world test and observe RED**

```powershell
cargo test -p world --test block_entity idless_note_requires_bounded_typed_note_fields --locked
```

Expected: compile failure because the accessors do not exist.

- [ ] **Step 3: Extend the root scan minimally**

Record NetworkLittleEndian root `note` and `powered` candidates without globally reserving their names. One exact tag-1 byte becomes `Value`; absence becomes `Absent`; duplicate or wrong-typed occurrences become `Invalid` after their bounded payloads are consumed. Do not retain a general decoded NBT tree. The pure classifier, and only that classifier, requires `note` in `0..=24` and `powered` in `0..=1` when `id` is absent and the backing identity is the exact noteblock. Explicitly identified other producers ignore these candidate fields for identity and remain decodable.

- [ ] **Step 4: Write failing classifier tests**

Build exact `BackingBlockIdentity` fixtures for the six allowed source/backing pairs and mismatches. Assert all four static sources return `ExistingBlockState`, Jukebox and exact id-less Note return `LogicalNoAdditionalDraw`, reviewed other sources return `Deferred`, and spoofed/mismatched inputs return `Unknown`.

```rust
assert_eq!(
    adjudicate_block_entity_visual(&note, noteblock_identity()),
    BlockEntityVisualRoute::LogicalNoAdditionalDraw { additional_refs: 0, .. }
);
assert!(matches!(
    adjudicate_block_entity_visual(&note, stone_identity()),
    BlockEntityVisualRoute::Unknown { .. }
));
```

- [ ] **Step 5: Run classifier tests and observe RED**

```powershell
cargo test -p bedrock-client block_entity_visuals --locked
```

Expected: compile failure because the module and route types do not exist.

- [ ] **Step 6: Implement and wire the pure classifier**

Create a focused module with no Bevy render resources. Wire it into normal block-entity commit diagnostics so each committed entity can be adjudicated when its backing palette entry is resident. Do not dirty a mesh for any of these six NBT-only changes. Add counters for adjudicated static, adjudicated logical, deferred, and unknown routes; reset them on session/dimension replacement.

- [ ] **Step 7: Verify zero-remesh and lifecycle behavior**

Add app tests for inline/request/live updates, malformed retention, eviction, and session replacement. Record the render-queue generation before and after each six-route NBT update and assert it does not change.

```powershell
cargo test -p world --test block_entity --locked
cargo test -p bedrock-client block_entity --locked
cargo clippy -p world -p bedrock-client --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```

- [ ] **Step 8: Commit Task 2**

```powershell
git add crates/world app
git commit -m "feat: adjudicate static block entity visuals"
```

---

### Task 3: Block-entity-specific GPU evidence request

**Files:**
- Create: `crates/render/src/block_entity_witness.rs`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/block_entity_visuals.rs`

**Interfaces:**
- Produces CLI flag `--block-entity-witness-request <path>`.
- Produces startup-validated `BlockEntityWitnessRequest` with at most 256 targets.
- Produces adjacent markers `RUST_MCBE_BLOCK_ENTITY_WITNESS` and an atomic local candidate evidence catalog.

- [ ] **Step 1: Write failing request-validation tests**

Cover exact source/variant/NBT/palette identity, duplicate position/witness, over-limit targets, unknown fields, stale asset hashes, wrong stream, and nonzero additional refs.

- [ ] **Step 2: Run request tests and observe RED**

```powershell
cargo test -p render block_entity_witness --locked
```

Expected: compile failure because `BlockEntityWitnessRequest` does not exist.

- [ ] **Step 3: Implement the bounded request and frame probe**

Reuse the existing selected-view identity, packed allocation lookup, direct/MDI submission recording, `PresentedFrameAck`, and generation fences. Bind each receipt to absolute block position, subchunk allocation generation, exact backing stream/ref count, classifier route digest, NBT digest, and zero additional refs. Never infer a target from a subchunk-wide nonzero count.

- [ ] **Step 4: Write and pass direct/MDI receipt tests**

Test two adjacent identical completions, stale allocation rejection, target contamination, wrong view, direct/MDI parity, and shutdown frames that never publish a receipt.

```powershell
cargo test -p render block_entity_witness --locked
cargo test -p bedrock-client block_entity_witness --locked
cargo clippy -p render -p bedrock-client --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```

- [ ] **Step 5: Commit Task 3**

```powershell
git add crates/render app
git commit -m "feat: capture block entity GPU witnesses"
```

---

### Task 4: Deterministic gallery, real receipts, and six proven entries

**Files:**
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Create: `docs/evidence/block-entity-static-logical-gallery-v1001.json`
- Modify: `docs/evidence/block-entity-render-evidence-v1001.json`
- Modify: `assets/block-entity-renderers-v1001.json`
- Modify: `crates/assets/data/block-entities-v1001.json`
- Modify: `docs/block-entity-coverage-v1001-report.json`
- Modify: `tools/blockentitygen/testdata/block-entities-v1001.json`
- Modify: `tools/blockentitygen/testdata/block-entity-coverage-v1001-report.json`
- Modify: `plan.md`

**Interfaces:**
- Produces acceptance pose `BlockEntityStaticLogicalGallery`.
- Produces one canonical request covering every declared variant plus Note pitch `0..24 × powered {false,true}`.
- Produces reviewed report counts `proven_renderer_count=6`, `deferred_renderer_count=16`, `final_gate_passed=false`.

- [ ] **Step 1: Write failing PowerShell gallery tests**

Assert deterministic isolated-subchunk placement, exact target count/domain, camera fencing, request/catalog hashes, stable BDS identity, no auto-fly, two adjacent receipt requirement, atomic publication, and rejection of stale/wrong/contaminated receipts.

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
```

Expected: failure because the new pose/plan/request does not exist.

- [ ] **Step 2: Implement the deterministic harness path**

Reuse existing stable BDS/client executable paths and fixture publication helpers. Make the block-entity gallery independent of the generic mutation/world-ready marker: publish from a deterministic spawn-relative anchor, teleport the player facing the gallery, then arm the request after the camera fence.

- [ ] **Step 3: Pass dry-run and focused repository tests**

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
$env:GOWORK='off'; Push-Location tools/blockentitygen; go test ./... -count=1; go vet ./...; Pop-Location
cargo test -p render -p bedrock-client -p world --locked
```

- [ ] **Step 4: Run the live gallery and validate candidate evidence**

Run DX12 on the approved stable paths with local compiled assets. Require all target receipts and clean process teardown. Store screenshots only under `%TEMP%` and inspect them only when presentation capture succeeds.

```powershell
$env:WGPU_BACKEND='dx12'
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance.ps1 `
  -DurationSeconds 60 `
  -BdsDir 'C:\Users\Hashim\projects\rust-mcbe\.local\bds\bedrock-server-1.26.32.2' `
  -BdsRuntimeDirectory 'C:\Users\Hashim\Documents\Codex\2026-07-09\computer-plugin-computer-use-openai-bundled\cinnabar-work\.local\bds-runtime\bedrock-server-1.26.32.2' `
  -MetricsOut '.local\acceptance\block-entity-static-logical-metrics.json' `
  -Assets '.local\assets\compiled\vanilla-v1001.mcbea' `
  -VisualFixturePose BlockEntityStaticLogicalGallery `
  -ClientExecutable 'target\debug\bedrock-client.exe' `
  -SkipClientBuild
```

Expected: all six source keys and all requested variants receive two adjacent exact GPU receipts; additional block-entity refs remain zero; no process remains.

- [ ] **Step 5: Promote validated evidence and regenerate artifacts**

Copy only the validated hash/receipt catalog into the tracked evidence JSON, set the six manifest entries to `implemented` with real implementation/gallery symbols and catalog witness IDs, and regenerate inventory/report/goldens. Verify reviewed counts are exactly 6/16 and strict-final still fails because 16 entries remain deferred.

- [ ] **Step 6: Run complete branch gates**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
go test ./core/... ./tools/fixturegen/... ./tools/registrygen/... -count=1
go vet ./core/... ./tools/fixturegen/... ./tools/registrygen/...
foreach($m in @('tools/chunkfix','tools/blockentitygen','tools/bedsimtrace')) { Push-Location $m; $env:GOWORK='off'; go test ./... -count=1; go vet ./...; Pop-Location }
git diff --check
```

Expected: every command passes; tracked payload scan finds no Mojang asset or screenshot.

- [ ] **Step 7: Commit Task 4**

```powershell
git add scripts docs/evidence assets/block-entity-renderers-v1001.json crates/assets/data/block-entities-v1001.json docs/block-entity-coverage-v1001-report.json tools/blockentitygen/testdata plan.md
git commit -m "feat: prove static and logical block entities"
```

After focused independent review, cherry-pick the task commits onto `render-integration`, rerun the complete gates, push history-preservingly to `phase2-textures`, and inspect GitHub CI.
