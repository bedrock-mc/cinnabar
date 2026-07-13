# FlowerBed / Petals Vanilla-Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render `minecraft:wildflowers` and `minecraft:pink_petals` with compact, state-correct flowerbed geometry that is measured against the pinned native Bedrock client and never uses the generic full-height Cross fallback.

**Architecture:** Add a typed `FlowerBed` registry family, compile immutable additive patch templates into the existing packed model buffer, and select them using preserved growth/cardinal state. A deterministic BDS/native-client gallery adjudicates every ambiguous coordinate and command-only state before the family is accepted.

**Tech Stack:** Go registry generator, Rust assets/compiler/runtime, existing packed Bevy model renderer, PowerShell BDS acceptance harness, native Windows GDI `%TEMP%` screenshots.

## Global Constraints

- Never commit Mojang textures, native-client files, diagnostic resource packs, worlds, or screenshots.
- Runtime geometry stays packed and immutable; no per-block Bevy `Mesh` or material.
- Normal and command-only Bedrock states must be explicit; never clamp, wrap, or guess growth values.
- Use the pinned protocol/registry and pinned Mojang Bedrock asset source already recorded by Phase 2.
- Every behavior change follows RED → GREEN → full affected-suite verification.
- Native visual evidence is required before the family is marked complete.

---

### Task 1: Preserve a dedicated FlowerBed family in BREG1003

**Files:**
- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/registry.rs`
- Test: `tools/registrygen/main_test.go`
- Test: `crates/assets/tests/compiler.rs`

**Interfaces:**
- Consumes: canonical names, `ModelStateGrowth`, and `ModelStateOrientation` already emitted by `recordFromState`.
- Produces: `ModelFamilyFlowerBed = 31` in Go and `ModelFamily::FlowerBed = 31` in Rust; both flowerbed names use it for all 32 states.

- [x] **Step 1: Write the failing Go classification test**

```go
for _, name := range []string{"minecraft:wildflowers", "minecraft:pink_petals"} {
    state := sourceState(name, intState("growth", 2), stringState("minecraft:cardinal_direction", "east"))
    record := recordFromState(state)
    if record.ModelFamily != ModelFamilyFlowerBed {
        t.Fatalf("%s family=%v, want FlowerBed", name, record.ModelFamily)
    }
    if got, _ := record.ModelState.Get(ModelStateGrowth); got != 2 { t.Fatalf("growth=%d", got) }
}
```

- [x] **Step 2: Run RED**

Run: `go test ./tools/registrygen -run 'Test.*FlowerBed' -count=1`

Expected: FAIL because `ModelFamilyFlowerBed` is absent and `wildflowers` is `Cross`.

- [x] **Step 3: Add the family without renumbering existing values**

Append `ModelFamilyFlowerBed` after `ModelFamilyInvisible`, classify both exact names before `isCrossName`, and remove `wildflowers` from `isCrossName`. Append Rust value `FlowerBed = 31` and decode raw value 31 in `ModelFamily::read`.

- [x] **Step 4: Prove all state cardinalities and BREG decode parity**

Add assertions that each name has exactly 32 records, growth 0–7 × four directions, and no record is `Cross` or `Unknown`.

Run:

```powershell
go test ./tools/registrygen -count=1
cargo test -p assets --test compiler --locked
```

Expected: PASS.

- [x] **Step 5: Regenerate only the committed registry binary**

Use the repository's pinned `tools/registrygen` command recorded in the Phase 2 asset docs. Verify deterministic SHA-256 across two generations and inspect `git status` to ensure no Mojang assets entered git.

---

### Task 2: Compile compact additive FlowerBed templates for normal states

**Files:**
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/assets/tests/blob.rs`
- Modify: `docs/phase-2-family-inventory.md`

**Interfaces:**
- Consumes: `ModelFamily::FlowerBed`, growth/orientation, the two terrain variants for the block, and `ModelQuad`'s 1/256 positions plus 1/4096 UVs.
- Produces: immutable model templates selected by `(block texture pair, growth 0..3, direction)`; growth 4..7 remains attributable diagnostic until Task 4 records native evidence.

- [x] **Step 1: Write failing normal-state compiler tests**

For each block, compile growth 0–3 facing north and assert:

```rust
assert_eq!(visual.kind, VisualKind::Model);
assert_ne!(visual.model_template, NO_MODEL_TEMPLATE);
assert_eq!(template_patch_count(&assets, visual.model_template), growth + 1);
assert!(template_quads(&assets, visual.model_template)
    .iter().all(|quad| quad.positions.iter().all(|p| p[1] < 64)));
assert_eq!(distinct_template_materials(&assets, visual.model_template).len(), 2);
```

Also assert `growth=4` remains diagnostic before Task 4, proving no implicit clamp.

- [x] **Step 2: Run RED**

Run: `cargo test -p assets --test compiler flowerbed --locked -- --nocapture`

Expected: FAIL because both names lack a dedicated compiled model.

- [x] **Step 3: Implement one checked flowerbed template builder**

Add a small `FlowerBedPatch` data table using the exact Mojang flowerbed model positions/UVs. Build an additive prefix of one through four horizontal flower planes plus their stem planes. Resolve terrain variant 0 as flower and variant 1 as stem; fail closed if either is absent. Apply cardinal rotation around the block center with checked 1/256 integer coordinates.

- [x] **Step 4: Deduplicate templates by full material/state identity**

Use a bounded cache key containing both material IDs, growth, and direction. Assert template and quad counts fit existing `u32`/asset-manifest limits and that identical states reuse template indices.

- [x] **Step 5: Verify encode/decode and all normal states**

Run:

```powershell
cargo test -p assets --locked
cargo clippy -p assets --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Expected: PASS; 32 normal states (two blocks × four growth × four directions) are non-diagnostic and bounded; command-only states remain explicit diagnostics.

---

### Task 3: Add an exhaustive deterministic FlowerBed gallery and local diagnostic pack builder

**Files:**
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Create: `scripts/flowerbed-reference-pack.ps1`
- Create: `scripts/tests/flowerbed-reference-pack.Tests.ps1`
- Modify: `docs/superpowers/plans/2026-07-12-flowerbed-parity.md`

**Interfaces:**
- Consumes: pinned local-only Mojang resource pack and BDS console command channel.
- Produces: a 64-state BDS fixture manifest and an ignored diagnostic resource pack with uniquely coloured flower quadrants/stem texels.

- [x] **Step 1: Write failing fixture-manifest tests**

Assert the plan contains exactly 64 unique `setblock` commands, every growth 0–7 and cardinal direction for both names, fixed camera commands for top/north/east/oblique views, a layout hash, and cleanup/ticking-area commands.

- [x] **Step 2: Run RED**

Run: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1`

Expected: FAIL because `FlowerBedGallery*` poses do not exist.

- [x] **Step 3: Implement `New-FlowerBedGalleryPlan`**

Add `FlowerBedGalleryTop`, `FlowerBedGalleryNorth`, `FlowerBedGalleryEast`, and `FlowerBedGalleryOblique` to the validated pose set. Build the exact 64-state grid from typed states, not string ordinal assumptions. Reuse existing fenced BDS command/result proof and source-world identity protections.

- [x] **Step 4: Test the local-only diagnostic pack builder**

The builder must refuse output outside `.local`, copy only the two flower and two stem PNGs plus the minimum manifest/terrain routing, replace texels deterministically, and emit SHA-256 evidence. Tests inspect generated images but never add them to git.

- [x] **Step 5: Run script verification**

Run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/flowerbed-reference-pack.Tests.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
git status --short
```

Expected: PASS and no Mojang/diagnostic assets tracked.

Implementation notes (Task 3):

- The exhaustive gallery reads raw family 31 directly from the committed protocol-1001 `BREG1003` registry and rejects any matrix other than the exact two names × growth 0–7 × South/West/North/East set. Its 8×8, four-by-three-spaced layout pairs every typed state with a polished-andesite reference cube and uses the existing schema-v2 ticking-area, command-result fence, source-world identity, and cleanup lifecycle.
- `scripts/flowerbed-reference-pack.ps1` accepts only an output rooted below this checkout's ignored `.local` directory, rejects overlap and reparse traversal, copies the pack manifest, emits minimal two-entry block/terrain routing, and writes only four deterministic opaque quadrant/stripe PNGs with per-file input/output SHA-256 evidence. The builder validates required route tokens without treating Mojang's comment-bearing pinned `terrain_texture.json` as strict JSON.
- Production builds verify exact SHA-256 values for all seven required files before claiming the pinned Mojang tag/archive identity. Synthetic fixtures must provide an explicit `rust-mcbe-flowerbed-source-identity-v1` document through `-SourceIdentityPath`; its complete file-hash map is verified against actual bytes and bound into the generated v2 evidence manifest.
- Independent Task 3 re-review at `07eab74` found the implementation spec-compliant and approved, with no Critical, Important, or Minor findings.

---

### Task 4: Measure native Bedrock and close command-only growth semantics

**Files:**
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Create: `docs/evidence/phase-2-flowerbed-native-reference.md`
- Modify: `docs/superpowers/specs/2026-07-12-flowerbed-parity-design.md`

**Interfaces:**
- Consumes: installed native Bedrock client, exact gallery, diagnostic pack, fixed camera poses, native Windows `%TEMP%` screenshots.
- Produces: measured plane coordinates/UV orientation and an explicit growth 4–7 mapping backed by screenshot hashes and client version.

- [x] **Step 1: Record the native-client version and fixture identities**

Record `MICROSOFT.MINECRAFTUWP` version, BDS version, asset tag/SHA, gallery layout SHA, diagnostic-pack SHA, FOV, graphics mode, and camera commands. If the client version is not the pinned target, record the mismatch and do not call the evidence exact until the matching build is tested.

- [ ] **Step 2: Capture every reference pose through `%TEMP%`**

Use the diagnostic pack in the native client, place all 64 states, and capture top/north/east/two oblique images. Do not use Computer Use/WGC for Cinnabar; use native GDI screenshots and inspect every fresh image.

Progress: five native GDI analysis captures are recorded and inspected. The generated world's terrain obstructs some external fixed camera positions, so the fixed-pose pixel-parity capture remains open for Task 5's terrain-safe fixture.

- [x] **Step 3: Derive and record command-only behavior**

Segment unique colours, calibrate against adjacent full cubes, and compare growth 4–7 masks to 0–3. Record whether each aliases an existing layout or has distinct geometry. Never infer an unobserved mapping.

- [x] **Step 4: Write RED tests for measured differences**

Add exact template position/UV hashes and growth 4–7 expected template selection from recorded evidence. Run the focused tests and verify they fail before updating compiler tables.

- [x] **Step 5: Implement and verify all 64 states**

Update compact template data only. Run full assets tests/Clippy/fmt and regenerate the ignored compiled vanilla blob. Confirm all 64 flowerbed states are non-diagnostic.

---

### Task 5: Render, performance, and live parity acceptance

**Files:**
- Modify: `crates/render/tests/plugin.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Modify: `docs/phase-2-texture-slice-report.md`
- Modify: `plan.md`

**Interfaces:**
- Consumes: complete flowerbed asset family and native reference evidence.
- Produces: deterministic Cinnabar gallery captures, packed-path performance evidence, and honest plan completion state.

- [ ] **Step 1: Add packed-render contract tests**

Assert FlowerBed visuals use existing `PackedModelRef`/lighting records, two-sided cutout, direct/MDI identical addressing, conservative connectivity, and no new bind group/pipeline/per-subchunk mesh.

- [ ] **Step 2: Run full affected verification**

```powershell
go test ./tools/registrygen -count=1
cargo test -p assets -p render -p bedrock-client --locked
cargo clippy -p assets -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
```

- [ ] **Step 3: Run deterministic BDS gallery acceptance**

Capture every fixed Cinnabar pose with native `%TEMP%` screenshots, compare against native reference pixels, and record diagnostic counters, FPS/frame-time, CPU, RSS, upload bytes, and template/ref counts.

- [ ] **Step 4: Run representative streamed-world verification**

Inspect the same BDS terrain that previously showed floating shrubs. Confirm no full-height wildflower planes, no pink-petals diagnostic cubes, correct patch count/direction, stable chunk streaming, and no new flicker.

- [ ] **Step 5: Independent review and plan update**

Request correctness and performance reviews. Mark the FlowerBed/model-family item complete in `plan.md` only when every accepted state has native visual evidence, all tests/gates pass, and no required work remains.
