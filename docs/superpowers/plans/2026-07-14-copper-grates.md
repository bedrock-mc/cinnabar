# Exact Copper Grates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render all eight canonical copper-grate states with exact cutout
textures, open connectivity, and exact-state internal-face culling.

**Architecture:** Extend the checked transparent-unit-cube semantic to permit
one homogeneous alpha class (all blend or all cutout), compile a literal
eight-name grate allowlist through it, and key shared-face suppression by exact
palette identity. Reuse the existing depth-writing cutout model path.

**Tech Stack:** Rust, MCBEAS04, palette-native meshing, Bevy/wgpu model shader,
visualcoverage.

## Global Constraints

- Follow `docs/superpowers/specs/2026-07-14-copper-grates-design.md`.
- Admit exactly eight stateless primary Cube-family grate records.
- Slime and every other glass/copper/legacy/invisible record stay excluded.
- Add no pipeline, bind group, texture page, Bevy object, or flat block array.
- Never track Mojang assets or the compiled blob.
- Pinned pack:
  `C:\Users\Hashim\Projects\rust-mcbe\.worktrees\phase2-textures\.local\assets\bedrock-samples\v1.26.30.32-preview\full\resource_pack`.

---

### Task 1: Checked cutout grates, exact culling, and production ratchet

**Files:**

- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/src/blob.rs`
- Modify: `crates/assets/src/runtime.rs`
- Test: `crates/assets/tests/compiler.rs`
- Test: `crates/assets/tests/blob.rs`
- Test: `crates/assets/tests/runtime.rs`
- Modify: `crates/render/src/mesh.rs`
- Test: `crates/render/tests/mesh.rs`
- Modify: `tools/visualcoverage/tests/ratchet.rs`
- Modify: `crates/assets/data/visual-coverage-v1001.json`
- Modify: `plan.md`
- Modify: `docs/phase-2-texture-slice-report.md`
- Modify: `.superpowers/sdd/progress.md`

- [ ] **Step 1: Write failing exact compiler and trust-boundary tests**

Bind the literal eight-name inventory and exact `{}`/Cube/Primary registry
contract. Synthetic and pinned compilation must require:

```rust
assert_eq!(visual.kind, VisualKind::Model);
assert_eq!(template.quad_count, 6);
assert!(materials.iter().all(|material| {
    material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
        && material.flags & MATERIAL_FLAG_ALPHA_BLEND == 0
}));
assert!(!visual.flags.intersects(
    BlockFlags::AIR
        | BlockFlags::CUBE_GEOMETRY
        | BlockFlags::OCCLUDES_FULL_FACE
        | BlockFlags::LEAF_MODEL
));
```

Prove all four waxed/unwaxed pairs share exact face material IDs, reversed input
is byte-identical, and every exclusion remains diagnostic.

Extend blob/runtime fixtures before production changes. Accept all-blend glass
and all-cutout grate templates; reject mixed classes, both alpha bits, opaque or
diagnostic materials, incompatible flags, wrong count, and malformed topology.

Run focused tests and observe RED:

```powershell
$env:PINNED_VANILLA_PACK='C:\Users\Hashim\Projects\rust-mcbe\.worktrees\phase2-textures\.local\assets\bedrock-samples\v1.26.30.32-preview\full\resource_pack'
cargo test -p assets copper_grate --locked -- --nocapture
```

- [ ] **Step 2: Implement the minimal compiler and validator generalization**

Add a sorted literal exact-name helper requiring `{}`, Cube, and Primary.
Apply `MATERIAL_FLAG_ALPHA_CUTOUT` only to admitted grate descriptors. Reuse
the existing checked unit-cube template flag and geometry, clear runtime
cube/occlusion flags, and intern deterministic templates by six-face material.

At both checked encoder and runtime decoder boundaries, derive each referenced
material's alpha class:

```rust
let alpha_class = material.flags
    & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT);
if !matches!(alpha_class, MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT) {
    return Err(invalid(...));
}
if expected_alpha_class.replace(alpha_class).is_some_and(|expected| expected != alpha_class) {
    return Err(invalid(...));
}
```

Use equivalent valid Rust that rejects both-bits and mixed templates without
weakening exact topology or semantic-flag validation.

- [ ] **Step 3: Write renderer RED tests, then use exact palette identity**

Production-style tests must prove:

- two identical grate states emit 10 ordinary opaque-model draw refs;
- different oxidation emits 12;
- waxed versus unwaxed emits 12 despite equal six-face materials;
- opaque/grate asymmetry and cave-open connectivity;
- no transparent draw refs for cutout grates;
- exact behavior in sequential and hashed modes;
- identical-state culling across all six subchunk boundaries;
- stained-glass focused tests remain green.

Run and observe the material-equality bug before editing:

```powershell
cargo test -p render --test mesh copper_grate --locked -- --nocapture
```

Then replace equal-material identity only for the checked full transparent-cube
semantic with:

```rust
let equal_transparent_cube =
    template.flags & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
        && model_template_flags(visuals, neighbour)
            & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
            != 0
        && neighbour.network_value == entry.network_value;
```

Do not change pane behavior or generic model culling.

- [ ] **Step 4: Prove and refresh the exact production delta**

Rebuild the ignored blob and ratchet it against the committed 7,706 baseline:

```powershell
cargo run -p asset-compiler --bin assetc --locked -- compile --pack $env:PINNED_VANILLA_PACK --registry crates/assets/data/block-registry-v1001.bin --biome-registry crates/assets/data/biome-registry-v1001.bin --out .local/assets/compiled/vanilla-v1001.mcbea
cargo run -p visualcoverage --locked -- ratchet --registry crates/assets/data/block-registry-v1001.bin --assets .local/assets/compiled/vanilla-v1001.mcbea --baseline crates/assets/data/visual-coverage-v1001.json --out .local/assets/compiled/pre-copper-grate-ratchet.json
```

Require eight removals, zero additions, and only the literal grate identities.
Refresh with `visual-invisible-v1001.json`, require zero post-refresh delta,
update production and gallery assertions to 7,698, and reconstruct 7,706 by
adding the exact eight IDs.

- [ ] **Step 5: Update durable docs and run full verification**

Record exact names, cutout/alias/culling semantics, exclusions, blob hash,
residual 7,698, and cumulative removals 7,243. Keep prior counts historical.

Run:

```powershell
cargo test -p assets -p render -p visualcoverage --all-targets --locked
cargo test -p assets --test compiler --locked
$env:CINNABAR_REAL_PACK=(Resolve-Path '.local/assets/compiled/vanilla-v1001.mcbea').Path
cargo test -p visualcoverage --test ratchet production_ratchet_reports_exact_model_removals_for_the_full_real_pack --locked -- --ignored --exact
cargo test -p visualcoverage --test ratchet current_gallery_inventory_is_non_accepting_with_7698_diagnostics --locked -- --ignored --exact
cargo clippy -p assets -p render -p visualcoverage --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Also require `git ls-files .local` empty and no tracked image/blob payload.

- [ ] **Step 6: Commit and report**

Commit the complete tranche, write the SDD report with RED/ratchet/test evidence,
and do not push until independent spec-and-quality review approves it.
