# Exact Stained-Glass Cubes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the 16 ordinary protocol-1001 stained-glass cube states with
exact translucent materials, cave-open semantics, and same-colour internal-face
culling.

**Architecture:** Compile each exact stateless stained-glass record into a
six-quad full-cube model marked by one checked immutable template flag. Route
its alpha-blended materials through the existing sorted transparent model phase;
the CPU mesher uses the flag plus exact six-face material identity to suppress
only same-colour shared faces.

**Tech Stack:** Rust, serde/serde_json, MCBEAS04 assets/runtime, palette-native
chunk meshing, Bevy/wgpu transparent model phase, visualcoverage.

## Global Constraints

- Follow
  `docs/superpowers/specs/2026-07-14-stained-glass-cubes-design.md` exactly.
- Admit only the 16 ordinary `minecraft:<colour>_stained_glass` records with
  canonical state `{}`, `ModelFamily::Cube`, and contributor role
  `Primary`.
- Education hard glass, panes, copper grates, slime, invisible bedrock, and
  legacy flags-zero records remain diagnostic.
- Use the existing one-bind-group transparent model phase; add no Bevy
  `Mesh`, material object, render phase, texture page, or per-subchunk object.
- Preserve paletted runtime data and compact model/draw records.
- Never track Mojang assets or the compiled `.mcbea` blob.
- Pinned pack:
  `C:\Users\Hashim\Projects\rust-mcbe\.worktrees\phase2-textures\.local\assets\bedrock-samples\v1.26.30.32-preview\full\resource_pack`.

---

### Task 1: Checked stained-glass asset route

**Files:**

- Modify: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/src/blob.rs`
- Modify: `crates/assets/src/runtime.rs`
- Test: `crates/assets/tests/compiler.rs`
- Test: `crates/assets/tests/blob.rs`
- Test: `crates/assets/tests/runtime.rs`

**Interfaces:**

- Produces:
  `pub const MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE: u32 = 1 << 9`.
- Produces a `VisualKind::Model` with one six-quad unit-cube template and six
  alpha-blended materials for each exact ordinary stained-glass record.
- Preserves zero `AIR | CUBE_GEOMETRY | OCCLUDES_FULL_FACE | LEAF_MODEL`
  flags on the compiled visual.

- [ ] **Step 1: Write the failing exact compiler tests**

Add a literal 16-name allowlist and assert:

```rust
assert_eq!(ordinary_stained_glass.len(), 16);
assert!(ordinary_stained_glass.iter().all(|record| {
    record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
}));
```

Compile synthetic and pinned-pack records and require `VisualKind::Model`,
six unit-cube quads, alpha-blended materials, exact face texture pixels,
cleared occlusion/cube flags, byte-identical reversed-record output, and
diagnostic output for extra state, wrong family/role, hard glass, copper grate,
slime, and invisible bedrock.

- [ ] **Step 2: Run the focused compiler tests and observe RED**

Run:

```powershell
$env:PINNED_VANILLA_PACK='C:\Users\Hashim\Projects\rust-mcbe\.worktrees\phase2-textures\.local\assets\bedrock-samples\v1.26.30.32-preview\full\resource_pack'
cargo test -p assets --test compiler stained_glass_cube --locked -- --nocapture
```

Expected: the exact ordinary states remain `VisualKind::Diagnostic`.

- [ ] **Step 3: Write malformed template-flag tests and observe RED**

Create otherwise valid MCBEAS04 fixtures and reject:

```rust
template.flags = MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE;
template.quad_count = 5; // reject
material.flags = 0; // reject: every referenced material must alpha blend
template.flags |= MODEL_TEMPLATE_FLAG_PANE; // reject incompatible semantics
```

Also require one isolated six-quad, alpha-blended transparent-cube template to
round-trip through both checked blob and runtime decoders.

- [ ] **Step 4: Implement the minimal checked asset route**

Add the isolated flag to the public model flag mask and validate it as a
standalone semantic. In compiler code, use an exact helper:

```rust
fn is_stained_glass_cube(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && ORDINARY_STAINED_GLASS_NAMES.binary_search(&record.name.as_ref()).is_ok()
}
```

The literal list must be sorted and exact. In `descriptor_for`, add
`MATERIAL_FLAG_ALPHA_BLEND` only for this helper. In `compile_visuals`, use
a dedicated `BTreeMap<[u32; 6], u32>` to intern six-quad unit-cube templates:

```rust
let template = push_model_template(
    cuboid_quads(materials, [0, 0, 0], [256, 256, 256]),
    MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    &mut model_templates,
    &mut model_quads,
)?;
visual.flags.remove(
    BlockFlags::AIR
        | BlockFlags::CUBE_GEOMETRY
        | BlockFlags::OCCLUDES_FULL_FACE
        | BlockFlags::LEAF_MODEL,
);
visual.faces = materials;
visual.kind = VisualKind::Model;
visual.model_template = template;
```

Blob/runtime validation must require exactly six quads and alpha-blended,
non-diagnostic materials and reject every combined semantic flag.

- [ ] **Step 5: Run Task 1 tests and commit**

Run:

```powershell
cargo test -p assets --all-targets --locked
cargo clippy -p assets --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all pass. Commit only Task 1 files.

---

### Task 2: Palette-native glass culling and production ratchet

**Files:**

- Modify: `crates/render/src/mesh.rs`
- Test: `crates/render/tests/mesh.rs`
- Modify: `tools/visualcoverage/tests/ratchet.rs`
- Modify: `crates/assets/data/visual-coverage-v1001.json`
- Modify: `plan.md`
- Modify: `docs/phase-2-texture-slice-report.md`
- Modify: `.superpowers/sdd/progress.md`

**Interfaces:**

- Consumes `MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE` and exact
  `ResolvedPaletteEntry.faces`.
- Produces transparent-only draw references with same-material shared-face
  suppression inside and across subchunks.

- [ ] **Step 1: Write failing CPU mesh tests**

Construct production-style stained-glass visuals and require:

```rust
// Equal colour: 12 outer quads minus the two shared faces.
assert_eq!(mesh.model_draw_refs.transparent.len(), 10);
assert!(mesh.model_draw_refs.opaque.is_empty());
```

Add separate tests proving different colours retain 12 faces, an adjacent opaque
cube retains its opaque face while hiding the glass face behind it, glass stays
cave-open, and equal-colour culling works across all six subchunk boundaries.

- [ ] **Step 2: Run the focused mesh tests and observe RED**

Run:

```powershell
cargo test -p render --test mesh stained_glass --locked -- --nocapture
```

Expected: equal-colour glass retains the two internal faces before the new
template-semantic culling rule.

- [ ] **Step 3: Implement the minimal palette-native culling rule**

In the existing model-quad culling loop, preserve full-opaque and pane behavior,
then add:

```rust
let equal_transparent_cube =
    template.flags & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
        && model_template_flags(visuals, neighbour)
            & MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
            != 0
        && neighbour.faces == entry.faces;

if neighbour.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
    || equal_pane
    || equal_transparent_cube
{
    visible_quad_mask &= !bit;
}
```

Do not change connectivity, allocation, upload, sorting, or pipeline code.

- [ ] **Step 4: Prove the exact production delta before refreshing**

Rebuild the ignored blob, ratchet it against the committed 7,722 baseline, and
require 16 removals, zero additions, and only the literal ordinary stained-glass
identities:

```powershell
cargo run -p assets --bin assetc --locked -- compile --pack $env:PINNED_VANILLA_PACK --registry crates/assets/data/block-registry-v1001.bin --biome-registry crates/assets/data/biome-registry-v1001.bin --out .local/assets/compiled/vanilla-v1001.mcbea
cargo run -p visualcoverage --locked -- ratchet --registry crates/assets/data/block-registry-v1001.bin --assets .local/assets/compiled/vanilla-v1001.mcbea --baseline crates/assets/data/visual-coverage-v1001.json --out .local/assets/compiled/pre-stained-glass-ratchet.json
```

- [ ] **Step 5: Refresh the baseline and exact production assertions**

Generate the reviewed baseline using
`crates/assets/data/visual-invisible-v1001.json`. Update the production test to
require 7,706 diagnostics and reconstruct the 7,722 pre-stained-glass set by
adding exactly the 16 admitted IDs. Rerun the ratchet and require zero additions
and zero removals.

- [ ] **Step 6: Update plan/report/progress and verify**

Record exact names/count, culling/material semantics, exclusions, integrated
blob SHA-256, residual 7,706, and cumulative removed count 7,235. Keep earlier
tranche counts explicitly historical.

Run:

```powershell
cargo test -p assets -p render -p visualcoverage --all-targets --locked
$env:CINNABAR_REAL_PACK=(Resolve-Path '.local/assets/compiled/vanilla-v1001.mcbea').Path
cargo test -p visualcoverage --test ratchet production_ratchet_reports_exact_model_removals_for_the_full_real_pack --locked -- --ignored --exact
cargo clippy -p assets -p render -p visualcoverage --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: every command passes; tracked files contain no Mojang payload.

- [ ] **Step 7: Commit Task 2**

Commit the renderer, tests, refreshed baseline, and documentation. Do not push
until independent spec-and-quality review approves the complete design range.
