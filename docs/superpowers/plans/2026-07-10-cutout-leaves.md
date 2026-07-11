# Cutout Cube Leaves Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render protocol-v1001 Dragonfly leaf cubes as coverage-preserving alpha cutouts with correct asymmetric culling and open cave connectivity in the existing packed opaque chunk pipeline.

**Architecture:** Versioned Dragonfly metadata separates air, cube geometry, full-face occlusion, and leaf-model facts. The local asset compiler marks leaf face materials with bit 8 and preserves alpha-test coverage per texture-array layer/mip; the palette-native binary-greedy mesher uses independent geometry/occluder/leaf masks. The existing shared `Opaque3d` shader samples once, conditionally discards cutout texels, writes alpha one and depth, and keeps the one bind group/texture array/MDI path.

**Tech Stack:** Rust 1.93.1, Bevy 0.18.1/wgpu/WGSL, `bitflags` 2, `image` 0.25, `sha2` 0.10, Go 1.26.1, Dragonfly `v0.10.15-0.20260709170650-b85c56ffea6b`, PowerShell acceptance tooling.

## Global Constraints

- The approved design is `docs/superpowers/specs/2026-07-10-cutout-leaves-design.md`; implement no broader material/model feature.
- Scope is cutout cube leaves only: no cross plants, general models, blend/water, biome data or tint, lighting, animation, sky, fog, or clouds.
- Keep protocol-v1001 filenames: `block-registry-v1001.bin`, `block-registry-v1001.sha256`, and `vanilla-v1001.mcbea`.
- Use registry magic `BREG1002`, blob magic `MCBEAS02`, and `BLOB_VERSION = 2`; reject the old registry/blob semantics.
- `BlockFlags` bits are exactly: `AIR = 1 << 0`, `CUBE_GEOMETRY = 1 << 1`, `OCCLUDES_FULL_FACE = 1 << 2`, `LEAF_MODEL = 1 << 3`.
- `Material` bit 8 is `MATERIAL_FLAG_ALPHA_CUTOUT`; bits 0 through 3 remain UV rotation/reflection and bits 4 through 7 remain unassigned.
- Keep `Material` and `PackedQuad` exactly 8 bytes. Do not add a per-subchunk Bevy `Mesh`/`StandardMaterial`, second texture array, widened quad, second chunk bind group, or second chunk render pipeline.
- Keep one depth-writing, no-blend `Opaque3d` phase and the existing MDI/direct fallback. True blend remains assigned to Phase 2.6.
- Keep runtime world data palette + packed words. Never materialize a 4,096-element block, flag, or material array.
- Keep `u64` axis-column binary-greedy meshing and split merges on visible material ID.
- Opaque neighbor hides any source face; leaf neighbor does not hide an opaque face; leaf/leaf shared faces are removed; leaf against opaque is removed.
- Cave connectivity treats every non-full-face-occluding voxel, including leaves and diagnostic model fallbacks, as open.
- Compile from the verified local `bedrock-samples-v1.26.30.32-preview-full.zip` only. Its pinned SHA-256 remains `12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c`.
- Commit no Mojang archive, extracted payload, PNG/TGA/JSON source, derived texture pixels, screenshot, or `.mcbea` blob. Commit only code/docs and regenerated Dragonfly registry metadata.
- Validate self-colored cherry, azalea, and flowered-azalea leaves first. Common-leaf foliage tint and color parity are explicitly deferred.
- Preserve the app budgets: mesh dispatch 64/frame, GPU uploads 8/frame, network ingress 8/frame, outbound sends 16/frame; world result capacity 128; render queue 256 items/64 MiB.
- At radius 16, carry the gates: combined client+core RSS at most 650 MB, steady normalized CPU at most 15%, join/teleport settle and full-view remesh at most 2 seconds, modified-subchunk visibility at most 100 ms, dev-MacBook p99 at most 8 ms, and zero unadjudicated decode errors/missing mappings.
- Full-view proof binds the exact 1,089-column radius-16 target cohort, excludes the captured source cohort, and requires two identical GPU-completed presented-frame manifests that each equal an independently frozen exact target `(SubChunkKey, mesh_generation)` manifest for one view generation; loaded counts, upload acknowledgements, or two identical partial frames never settle it.
- Steady resource sampling uses independent triggers: baseline world-ready, near gallery fixture-ready, or far forest bound-present completion. Baseline and near gallery modes never arm far teleport.
- The far forest retargets both BDS `setblock` and app mutation tracking to a loaded target coordinate only after observable fixture-load, teleport, cohort, and presented-frame sequencing.
- The pre-feature evidence base is `5933209fe053aff0f2164262f129635b947a636b`; diagnostic reduction must compare the same deterministic forest fixture against that revision and the final revision.
- Every behavioral task follows RED -> GREEN -> REFACTOR, ends in a focused commit, and receives an independent review before its dependent task begins. Fix all Critical and Important findings before proceeding.
- Every step must retain its concrete assertions and bounds; do not skip assertions, ignore failures, or add unbounded test loops.

## Implementation Map

- `tools/registrygen/main.go`: export independent Dragonfly model facts under `BREG1002`.
- `tools/registrygen/main_test.go`: pin exact registry/leaf counts, flag combinations, and deterministic bytes.
- `crates/assets/src/registry.rs`: decode and validate the new flag schema.
- `crates/assets/src/{compiler,image}.rs`: compile leaf materials and deterministic coverage-preserving mips.
- `crates/assets/src/{blob,runtime}.rs`: encode/decode only `MCBEAS02` version 2 and validate flag/material masks.
- `crates/assets/src/bin/assetc.rs`: identify the new registry schema and report cutout counts.
- `crates/assets/tests/{pack,compiler,blob,runtime}.rs`: schema, compiler, mip, and compatibility regressions.
- `crates/render/src/mesh.rs`: palette facts, independent `u64` masks, asymmetric culling, and open leaf connectivity.
- `crates/render/src/chunk.wgsl`: one-sample bit-8 discard and alpha-one output.
- `crates/render/src/plugin.rs`: retain the one opaque pipeline, freeze the independently expected target generation manifest, and compare it with exact direct/MDI presented-frame manifests after GPU completion.
- `crates/render/tests/{mesh,plugin}.rs`: culling/connectivity, record-size, shader, phase, bind-group, MDI, and present-fence regressions.
- `app/src/world_stream.rs`: retain bounded job plumbing, update leaf connectivity, and report exact target/source cohort status.
- `app/src/{main,metrics}.rs`: bind and serialize exact cohort/presented-frame teleport and forced-remesh proof, then retarget mutation tracking.
- `scripts/acceptance.ps1`: deterministic leaf fixtures, prebuilt-baseline client support, and process resource samples.
- `scripts/tests/acceptance.Tests.ps1`: fixture, path-safety, resource-accounting, and baseline-option contracts.
- `docs/phase-2-cutout-leaves-report.md`: exact hashes/counts/diagnostic reduction/performance/visual limitations.
- `plan.md`: mark Phase 2.4 complete only after every code, evidence, budget, and review gate passes.

---

### Task 1: Version and Export Independent Block Semantics

**Files:**
- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/registry.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/src/blob.rs`
- Modify: `crates/assets/src/runtime.rs`
- Modify: `crates/assets/src/bin/assetc.rs`
- Modify: `crates/assets/tests/pack.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/assets/tests/blob.rs`
- Modify: `crates/assets/tests/runtime.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `app/tests/assets.rs`
- Regenerate: `crates/assets/data/block-registry-v1001.bin`
- Regenerate: `crates/assets/data/block-registry-v1001.sha256`

**Interfaces:**
- Produces Go constants `flagAir`, `flagCubeGeometry`, `flagOccludesFullFace`, `flagLeafModel` and `func classifyFlags(value world.Block) uint8`.
- Produces Rust `BlockFlags::{AIR, CUBE_GEOMETRY, OCCLUDES_FULL_FACE, LEAF_MODEL}` and `BlockFlags::has_valid_semantics(self) -> bool`.
- Produces `BREG1002`, `MCBEAS02`, and `BLOB_VERSION == 2` while preserving the version-1001 filenames and existing record widths.
- Keeps `Record`, `RegistryRecord`, `BlockVisual`, sequential/hash lookup, the 28-byte visual record, and the 88-byte blob header structurally unchanged.

- [ ] **Step 1: Write failing exporter/schema tests**

  Replace old full-cube expectations and add exact pinned assertions:

  ```go
  func TestClassifyFlagsSeparatesSolidLeavesAndOtherModels(t *testing.T) {
      if got := classifyFlags(block.Stone{}); got != flagCubeGeometry|flagOccludesFullFace {
          t.Fatalf("stone flags = %#x", got)
      }
      leaf := block.Leaves{Type: block.CherryLeaves(), Persistent: true}
      if got := classifyFlags(leaf); got != flagCubeGeometry|flagLeafModel {
          t.Fatalf("cherry leaf flags = %#x", got)
      }
      if got := classifyFlags(block.Torch{}); got != 0 {
          t.Fatalf("torch flags = %#x", got)
      }
  }
  ```

  Extend `TestCollectDefaultBlockRegistry` to assert 16,913 total records, 713
  `CUBE_GEOMETRY` states, 669 `OCCLUDES_FULL_FACE` states, 44 `LEAF_MODEL` states,
  one `AIR` state, and 11 distinct leaf names. Assert every leaf has cube+leaf and lacks
  occlusion; all 49 vetted mycelium/mushroom states have cube+occlusion.

  Add `TestEncodeRejectsInvalidFlagSemantics` covering an unknown bit, air+cube,
  occluder-without-cube, leaf-without-cube, and leaf+occluder. Each call to `encode` must fail
  with the offending sequential ID and flag byte.

  In Rust, make `registry_bytes` emit `BREG1002`; test all valid flag combinations and reject:

  ```rust
  for invalid in [
      BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
      BlockFlags::OCCLUDES_FULL_FACE,
      BlockFlags::LEAF_MODEL,
      BlockFlags::CUBE_GEOMETRY
          | BlockFlags::OCCLUDES_FULL_FACE
          | BlockFlags::LEAF_MODEL,
  ] {
      assert!(!invalid.has_valid_semantics(), "accepted {invalid:?}");
  }
  ```

  Add explicit tests that `read_registry(b"BREG1001...")` returns
  `AssetError::InvalidRegistryMagic` and `RuntimeAssets::decode` rejects a resealed
  `MCBEAS01`/version-1 blob.

- [ ] **Step 2: Run RED and capture the expected semantic failures**

  Run:

  ```text
  go -C tools/registrygen test ./... -count=1
  cargo test -p assets --test pack --test blob --test runtime --locked -- --nocapture
  ```

  Expected: Go fails because `classifyFlags` and the three new flags do not exist; Rust fails
  because the new `BlockFlags` names/schema values do not exist and old magic is still accepted.

- [ ] **Step 3: Implement exact Dragonfly classification**

  Replace `fullCube` with:

  ```go
  const (
      registryHeader = "BREG1002"
      flagAir             uint8 = 1 << 0
      flagCubeGeometry    uint8 = 1 << 1
      flagOccludesFullFace uint8 = 1 << 2
      flagLeafModel       uint8 = 1 << 3
      allBlockFlags             = flagAir | flagCubeGeometry | flagOccludesFullFace | flagLeafModel
  )

  func validRecordFlags(flags uint8) bool {
      if flags&^allBlockFlags != 0 {
          return false
      }
      air := flags&flagAir != 0
      cube := flags&flagCubeGeometry != 0
      occludes := flags&flagOccludesFullFace != 0
      leaf := flags&flagLeafModel != 0
      return (!air || flags == flagAir) && (!occludes || cube) && (!leaf || (cube && !occludes))
  }

  func classifyFlags(value world.Block) uint8 {
      name, properties := value.EncodeBlock()
      if name == "minecraft:air" {
          return flagAir
      }
      switch value.Model().(type) {
      case model.Leaves:
          return flagCubeGeometry | flagLeafModel
      case model.Solid:
          return flagCubeGeometry | flagOccludesFullFace
      }
      _, stateHash := value.Hash()
      if stateHash == math.MaxUint64 && approvedUnknownFullCubeState(name, properties) {
          return flagCubeGeometry | flagOccludesFullFace
      }
      return 0
  }
  ```

  Set `Flags: classifyFlags(value)` in `collect`. Keep the approved unknown-state whitelist
  byte-for-byte narrow. In `encode`, reject `!validRecordFlags(record.Flags)` before reserving
  or writing record bytes.

- [ ] **Step 4: Implement Rust flag invariants and schema rejection**

  Define the four flags and:

  ```rust
  impl BlockFlags {
      #[must_use]
      pub const fn has_valid_semantics(self) -> bool {
          let air = self.contains(Self::AIR);
          let cube = self.contains(Self::CUBE_GEOMETRY);
          let occludes = self.contains(Self::OCCLUDES_FULL_FACE);
          let leaf = self.contains(Self::LEAF_MODEL);
          (!air || self.bits() == Self::AIR.bits())
              && (!occludes || cube)
              && (!leaf || (cube && !occludes))
      }
  }
  ```

  Validate the method after `from_bits` in registry, compiler, blob, and runtime paths. Rename
  existing texture-eligibility checks from `FULL_CUBE` to `CUBE_GEOMETRY` so the workspace stays
  compiling; Task 3 supplies the new culling/connectivity behavior.

  Set:

  ```rust
  const REGISTRY_MAGIC: &[u8; 8] = b"BREG1002";
  pub const BLOB_MAGIC: [u8; 8] = *b"MCBEAS02";
  pub const BLOB_VERSION: u32 = 2;
  ```

  Update runtime error/help text, test fixtures, and `assetc`'s registry description. Do not
  rename protocol-v1001 paths.

- [ ] **Step 5: Verify GREEN, determinism, and regenerate tracked metadata**

  Run:

  ```powershell
  go -C tools/registrygen test ./... -count=1
  cargo test -p assets --test pack --test compiler --test blob --test runtime --locked -- --nocapture
  cargo test -p render --test mesh --test plugin --locked -- --nocapture
  cargo test -p bedrock-client --test assets --locked -- --nocapture
  go -C tools/registrygen run . -out ../../crates/assets/data/block-registry-v1001.bin
  go -C tools/registrygen run . -out ../../.local/assets/block-registry-v1001-second.bin
  $first = (Get-FileHash -Algorithm SHA256 crates/assets/data/block-registry-v1001.bin).Hash.ToLowerInvariant()
  $second = (Get-FileHash -Algorithm SHA256 .local/assets/block-registry-v1001-second.bin).Hash.ToLowerInvariant()
  if ($first -ne $second) { throw "registry output changed across identical runs" }
  ```

  Expected: all tests pass; both hashes match; the generated registry has the exact counts from
  Step 1. Use `apply_patch` to replace the sole line of
  `crates/assets/data/block-registry-v1001.sha256` with `$first`, then assert its trimmed content
  equals `$first`.

- [ ] **Step 6: Commit and request independent review**

  ```text
  git add tools/registrygen crates/assets crates/render/src/mesh.rs crates/render/tests app/tests/assets.rs
  git commit -m "feat: export independent block semantics"
  ```

  Review the task's base..HEAD range for exact counts, impossible flag rejection, old-schema
  rejection, filenames, generated bytes, and absence of Mojang payload. Fix all Critical and
  Important findings in focused commits before Task 2.

---

### Task 2: Compile Cutout Materials and Coverage-Preserving Mips

**Files:**
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/src/image.rs`
- Modify: `crates/assets/src/blob.rs`
- Modify: `crates/assets/src/runtime.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/src/bin/assetc.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/assets/tests/blob.rs`
- Modify: `crates/assets/tests/runtime.rs`

**Interfaces:**
- Produces `MATERIAL_FLAG_UV_MASK: u32 = 0x0000_000f`, `MATERIAL_FLAG_ALPHA_CUTOUT: u32 = 1 << 8`, and `MATERIAL_FLAGS_MASK` as their union.
- Changes private `build_texture_array(base_layers, cutout_layers)` to accept `&BTreeSet<u32>` while keeping public `TextureArray` unchanged.
- Keeps `Material { layer: u32, flags: u32 }`, `BlockVisual`, `CompiledAssets`, blob section widths, and `RuntimeAssets` public lookup APIs unchanged.
- Produces cutout-material count in `assetc`'s deterministic summary.

- [ ] **Step 1: Write failing leaf-material and mip-coverage tests**

  Add `compiler_marks_only_leaf_faces_as_alpha_cutout` with one opaque cube and the three
  self-colored leaf names. Assert every leaf face resolves non-diagnostic, every leaf material
  has bit 8, the opaque material does not, UV bits remain unchanged, and:

  ```rust
  assert_eq!(MATERIAL_FLAG_UV_MASK, 0x0f);
  assert_eq!(MATERIAL_FLAG_ALPHA_CUTOUT, 0x100);
  assert_eq!(std::mem::size_of::<Material>(), 8);
  ```

  Add `cutout_mips_preserve_each_layer_coverage_without_cross_layer_bleed`. Define
  `fn alpha_survivors(rgba: &[u8]) -> usize` as the number of four-byte texels whose alpha is at
  least 128. Generate red and blue non-aligned masks with
  `((x * 17 + y * 29 + x * y * 7) & 255) < threshold`, using thresholds 78 and 181. This makes
  the uncorrected chain miss at least one rounded target. For every 16/8/4/2/1 mip, extract each
  layer separately and assert:

  ```rust
  let pixels = usize::try_from(mip.size * mip.size).unwrap();
  let target = (base_survivors * pixels + 128) / 256;
  assert_eq!(alpha_survivors(actual), reference_nearest_survivors(raw_mip, target));
  assert!(red.chunks_exact(4).all(|p| p[2] == 0));
  assert!(blue.chunks_exact(4).all(|p| p[0] == 0));
  ```

  `fn reference_nearest_survivors(raw_rgba: &[u8], target: usize) -> usize` is an independent
  bounded oracle: enumerate scale 0 plus the Q16
  threshold scale and predecessor for each non-zero raw alpha (at most `2 * pixels + 1`
  candidates), choose minimum survivor-count error, then smaller scale. Assert the selected scale
  changes alpha only, and add a no-tie fixture where survivors equal the exact rounded target.

  Compile the same generated pack 100 times with shuffled records and assert identical blob
  bytes. Add blob/runtime cases that reject a material flag outside `0x0000_010f`.

- [ ] **Step 2: Run RED**

  Run:

  ```text
  cargo test -p assets --test compiler --test blob --test runtime --locked -- --nocapture
  ```

  Expected: FAIL because the cutout constants/layer set/coverage correction do not exist and
  leaf descriptors do not set bit 8.

- [ ] **Step 3: Assign leaf material descriptors without widening records**

  In `descriptor_for`, retain UV flags and add:

  ```rust
  let mut flags = if rotate_uv { MATERIAL_FLAG_ROTATE_UV } else { 0 };
  if record.flags.contains(BlockFlags::LEAF_MODEL) {
      flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
  }
  ```

  Track every deduplicated texture layer referenced by a descriptor whose flags contain bit 8;
  pass that `BTreeSet<u32>` into mip construction. Material identity remains `(layer, flags)`, so
  an opaque and cutout use of the same pixels creates distinct materials but not duplicate array
  pixels. Validate `material.flags & !MATERIAL_FLAGS_MASK == 0` in blob/runtime paths.

- [ ] **Step 4: Implement deterministic bounded alpha-coverage correction**

  Use exact integer bounds:

  ```rust
  const ALPHA_TEST_THRESHOLD: u8 = 128;
  const ALPHA_SCALE_FRACTION_BITS: u32 = 16;
  const ALPHA_SCALE_MAX: u32 = 16 << ALPHA_SCALE_FRACTION_BITS;
  const ALPHA_SCALE_SEARCH_STEPS: usize = 21;
  ```

  Preserve the existing linear-light/premultiplied RGB downsample. For each cutout layer, compute
  base survivors at alpha >=128. At every smaller mip, set
  `target = (base_survivors * mip_pixels + 128) / 256`, then binary-search the smallest Q16 scale
  whose survivor count reaches the target. Compare that scale with the immediately lower scale;
  choose the smaller absolute count error, breaking ties toward the smaller scale. Apply
  `min(255, (alpha * scale + 0x8000) >> 16)` to alpha only. Search exactly 21 iterations and do
  not allocate beyond one mip-layer scratch buffer.

- [ ] **Step 5: Verify GREEN and compiler diagnostics**

  Run:

  ```text
  cargo test -p assets --test compiler --test blob --test runtime --locked -- --nocapture
  cargo test -p assets --locked -- --nocapture
  cargo clippy -p assets --all-targets --locked -- -D warnings
  cargo run -p assets --bin assetc -- --help
  ```

  Expected: all tests pass; shuffled outputs are byte-identical; material/record sizes remain
  eight bytes; help still targets `vanilla-v1001.mcbea`; no local Mojang output is tracked.

- [ ] **Step 6: Commit and request independent review**

  ```text
  git add crates/assets
  git commit -m "feat: compile cutout leaf materials"
  ```

  Review bit allocation, descriptor/material deduplication, fixed search bounds, one-texel
  coverage tolerance, layer isolation, malformed flags, and overflow handling. Resolve all
  Critical and Important findings before Task 3.

---

### Task 3: Mesh Asymmetric Leaf Faces and Open Connectivity

**Files:**
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `app/src/world_stream.rs`

**Interfaces:**
- Keeps the existing five-argument `mesh_sub_chunk` signature, `Neighbourhood` builders, `ChunkMesh`, `FaceConnectivity`, and `PackedQuad` APIs.
- Introduces private palette-level `ResolvedPaletteEntry { flags: BlockFlags, faces: [u32; 6] }` and `PaletteFacts<'a>`; no public world-storage change.
- Introduces private `AxisColumns`/`VisibilityMasks { geometry, occluders, leaves }`, each backed only by 16x16 `u64` columns.
- Uses `fn culls_face(source: BlockFlags, neighbour: BlockFlags) -> bool` for both internal and cross-subchunk decisions.

- [ ] **Step 1: Write the asymmetric culling/connectivity matrix tests**

  Extend the synthetic `RuntimeAssets` fixture with opaque, two distinct leaf materials, air,
  and a known unsupported model. For two adjacent blocks assert totals and directed shared faces:

  | Source / neighbor | Source shared face | Total two-block quads |
  |---|---:|---:|
  | opaque / opaque | culled | 10 |
  | opaque / leaf | kept | 11 |
  | leaf / opaque | culled | 11 |
  | leaf / leaf | culled on both cubes | 10 |
  | diagnostic / leaf | kept | 12 |

  Repeat at every `Neighbourhood` boundary. A boundary source emits five faces against opaque;
  opaque emits six against leaf; leaf emits five against leaf or opaque.

  Add:

  ```rust
  #[test]
  fn uniform_leaf_meshes_outer_planes_but_is_cave_open() {
      let leaf = uniform(LEAF_A);
      let mesh = mesh(
          &classifier(),
          NetworkIdMode::Sequential,
          &Neighbourhood::empty(),
          &leaf,
      );
      assert_eq!(mesh.quad_count(), 6);
      assert!(mesh.connectivity().is_all_connected());
      assert_eq!(std::mem::size_of::<PackedQuad>(), 8);
  }
  ```

  Prove a leaf-filled slab does not stop connectivity BFS, an opaque slab does, different leaf
  materials still remove their shared face, and existing face-material greedy splits remain.

- [ ] **Step 2: Run RED**

  Run:

  ```text
  cargo test -p render --test mesh --locked -- --nocapture
  cargo test -p bedrock-client world_stream --locked -- --nocapture
  ```

  Expected: FAIL because every current non-air neighbor occludes and every non-air voxel blocks
  connectivity.

- [ ] **Step 3: Resolve palette facts once and build independent binary masks**

  Build one compact table parallel to each storage palette:

  ```rust
  #[derive(Clone, Copy)]
  struct ResolvedPaletteEntry {
      flags: BlockFlags,
      faces: [u32; 6],
  }

  fn culls_face(source: BlockFlags, neighbour: BlockFlags) -> bool {
      neighbour.contains(BlockFlags::OCCLUDES_FULL_FACE)
          || (source.contains(BlockFlags::LEAF_MODEL)
              && neighbour.contains(BlockFlags::LEAF_MODEL))
  }
  ```

  `PaletteFacts::at(x, y, z)` reads packed indices and existing storage-layer precedence. Air
  emits nothing. `CUBE_GEOMETRY` uses its face material; a known non-air model without cube
  support keeps diagnostic material 0 as the existing visible fallback. The latter has no
  occluder/leaf bit.

  Construct `geometry`, `occluders`, and `leaves` axis columns without a per-block array. For
  each direction compute visible bits as source geometry minus shifted neighbor occluders minus
  source-leaf/shifted-neighbor-leaf pairs. Apply `culls_face` to the 256 cross-boundary samples
  so boundary and internal semantics match exactly. Preserve uniform fast paths and material-ID
  greedy identity.

- [ ] **Step 4: Make cave flood fill depend only on full-face occlusion**

  Change connectivity's open predicate to:

  ```rust
  fn connectivity_open(entry: ResolvedPaletteEntry) -> bool {
      !entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
  }
  ```

  Return `FaceConnectivity::all()` for uniform air, leaf, or non-occluding diagnostic models;
  return `none()` for a uniform full-face occluder; use the existing bounded 4,096-bit visited
  set and queue for mixed data. Do not change collision or lighting code.

- [ ] **Step 5: Verify GREEN, bounds, and integration**

  Run:

  ```powershell
  cargo test -p render --test mesh --locked -- --nocapture
  cargo test -p render --locked -- --nocapture
  cargo test -p bedrock-client --locked -- --nocapture
  cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings
  $flat = & rg -n "\[(u32|BlockFlags|ResolvedPaletteEntry); 4096\]" crates/render app 2>&1
  $rgExit = $LASTEXITCODE
  if ($rgExit -eq 0) { $flat; throw 'forbidden flat 4096-element array found' }
  if ($rgExit -gt 1) { throw "rg flat-array scan failed with exit $rgExit`: $flat" }
  ```

  Expected: tests/clippy pass; the scan prints no matches; `PackedQuad` is eight bytes; queue,
  revision-cancellation, connectivity-generation, and palette tests remain green.

- [ ] **Step 6: Commit and request independent review**

  ```text
  git add crates/render/src/mesh.rs crates/render/tests/mesh.rs app/src/world_stream.rs
  git commit -m "feat: mesh asymmetric leaf faces"
  ```

  Review every ordered culling pair, all six boundaries, multilayer palettes, uniform paths,
  diagnostics, connectivity, packed representation, and binary-greedy preservation. Resolve all
  Critical and Important findings before Task 4.

---

### Task 4: Apply Alpha Cutout in the Existing Opaque Shader

**Files:**
- Modify: `crates/render/src/chunk.wgsl`
- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/tests/plugin.rs`

**Interfaces:**
- Keeps bind-group bindings 0 through 5, `MaterialGpu { layer, flags }`, `ChunkPipeline`, `Opaque3d`, `ChunkIndirectBatches`, and MDI/direct selection unchanged.
- Adds flat `material_flags: u32` to `VertexOutput`; no storage buffer or bind-group addition.
- Fragment cutoff is exactly `sampled.a < 0.5` for bit 8; surviving fragments return alpha 1.

- [ ] **Step 1: Write shader and architecture RED tests**

  Extend `packed_chunk_shader_parses_and_validates` to assert:

  ```rust
  assert_eq!(shader.matches("textureSample(").count(), 1);
  assert!(shader.contains("@interpolate(flat) material_flags: u32"));
  assert!(shader.contains("material_flags & (1u << 8u)"));
  assert!(shader.contains("sampled.a < 0.5"));
  assert!(shader.contains("discard"));
  assert!(shader.contains("vec4(sampled.rgb, 1.0)"));
  ```

  Add a source-level architecture regression over `plugin.rs`: exactly one chunk
  `RenderPipelineDescriptor`, draw commands registered for `Opaque3d`, `blend: None`,
  `depth_write_enabled: true`, bindings 0..5 only, and no `AlphaMask3d`/`Transparent3d` chunk
  path. Keep the existing MDI/direct capability test and exact `Material`/`PackedQuad` sizes.

- [ ] **Step 2: Run RED**

  Run: `cargo test -p render --test plugin --locked -- --nocapture`

  Expected: FAIL because fragment flags, bit-8 discard, and alpha-one output do not exist.

- [ ] **Step 3: Implement the one-sample fragment path**

  Add the flat field, assign `out.material_flags = material.flags`, and implement:

  ```wgsl
  @fragment
  fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
      let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.layer));
      if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) {
          discard;
      }
      return vec4(sampled.rgb, 1.0);
  }
  ```

  Leave texture/material upload, bind-group identity, `Opaque3d` queuing, no-blend target,
  depth state, global arenas, indirect commands, and direct fallback unchanged.

- [ ] **Step 4: Verify GREEN and unchanged renderer architecture**

  Run:

  ```text
  cargo test -p render --test plugin --locked -- --nocapture
  cargo test -p render --locked -- --nocapture
  cargo test -p bedrock-client --locked -- --nocapture
  cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings
  ```

  Expected: shader parses/validates; exactly one sample/discard path exists; one bind group and
  opaque pipeline remain; MDI/direct tests and upload budgets are unchanged.

- [ ] **Step 5: Commit and request independent review**

  ```text
  git add crates/render
  git commit -m "feat: discard cutout leaf texels"
  ```

  Review WGSL interpolation, cutoff edge (`alpha == 0.5` survives), alpha-one output, depth/no
  blend, one phase, material bounds, and direct/MDI parity. Resolve all Critical and Important
  findings before Task 5.

---

### Task 5: Regenerate Local Assets and Pass the Deterministic Live Gate

**Files:**
- Modify: `app/src/args.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/metrics.rs`
- Modify: `app/src/world_stream.rs`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Create: `docs/phase-2-cutout-leaves-report.md`
- Modify: `plan.md`

**Interfaces:**
- Extends `-VisualFixturePose` with near-only `LeafGalleryFront` and `LeafGalleryBack`; adds harness-only `-LeafForestBaseline` and binding `-LeafForestFullView` modes over the same hashed forest plan.
- Adds paired `-ClientExecutable <path>` and `-SkipClientBuild` options for the pinned base comparison; normal acceptance behavior is unchanged when omitted.
- Retains `-UseVsync` for capped baseline/near-gallery sampling; far full-view mode uses the Task 8 `--frame-cap 60` contract.
- Adds `-SteadyResourceTrigger WorldReady|VisualFixtureReady|FullViewPresented`; exactly one trigger owns each 30-second sampler and only `FullViewPresented` requires the far tracker.
- Produces `ViewCohort { dimension: i32, center: [i32; 2], radius: i32 }`; radius 16 expands to exactly 1,089 `ChunkKey`s.
- Produces `WorldStream::cohort_status(&self, target: ViewCohort, source: &BTreeSet<ChunkKey>) -> CohortStatus` with target/committed cohort identity; `expected`, `loaded_target`, `missing_target`, `foreign_loaded`, `foreign_requested`, `foreign_resident`, and `source_leftover`; resident/known-air count+hash identities; and bounded-work emptiness.
- Produces `TargetRenderExpectation { cohort: ViewCohort, manifest: Arc<[(SubChunkKey, u64)]>, view_generation: u64, render_ready_at: Instant }`, frozen from the authoritative main-world mesh-handoff expectation table before `ChunkRenderInstance` extraction.
- Produces `ChunkRenderQueue::freeze_target_expectation(&self, cohort: ViewCohort, view_generation: u64, render_ready_at: Instant) -> TargetRenderExpectation`; LeafForest rejects an empty result.
- Produces `PresentedFrameAck { cohort: ViewCohort, presented_manifest: Arc<[(SubChunkKey, u64)]>, view_generation: u64, present_returned_at: Instant, gpu_completed_at: Instant, missing_target_instances: usize, unexpected_target_instances: usize, source_instances: usize, foreign_instances: usize, stale_generation_instances: usize, orphan_allocations: usize }` after exact comparison with that independent expectation.
- Produces `MetricsReport::{teleport_settle_ms, forced_full_view_remesh_ms}: Option<f64>` from the binding GPU-completion timestamps, not from upload acknowledgement or a quiet timer.
- Produces `AcceptanceRun::retarget_mutation(&mut self, coordinate: [i32; 3], armed_at: Instant)` and a deterministic target-forest mutation coordinate shared with the PowerShell manifest.
- Keeps owned BDS stdin, command-length/newline checks, observable `list` fence, fresh runtime copy, and 64-command/32,768-block fixture bounds.

- [ ] **Step 1: Write failing acceptance-harness tests**

  Add tests that both near gallery poses and the far forest plan produce bounded commands and
  manifest schema `rust-mcbe-visual-fixture-v2`. Gallery/forest manifests must contain:

  ```powershell
  $selfColored = @(
      'minecraft:cherry_leaves',
      'minecraft:azalea_leaves',
      'minecraft:azalea_leaves_flowered'
  )
  $tintDeferred = @('minecraft:oak_leaves', 'minecraft:birch_leaves', 'minecraft:spruce_leaves')
  ```

  Assert each leaf command sets `persistent_bit=true` and `update_bit=false`, opaque backing
  touches leaves, near/far panels exist, front/back camera targets are deterministic, and the
  forest contains multiple bounded canopies plus one target mutation coordinate. Test that
  `-SkipClientBuild` without
  `-ClientExecutable` fails, a missing executable fails, and an explicit prebuilt executable is
  never overwritten. Assert full-view mode supplies `--frame-cap 60` and omits `--no-vsync`.
  Test the CPU formula
  `100 * delta_cpu_seconds / (wall_seconds * logical_processor_count)` and combined RSS sum with
  synthetic samples.

  Add Rust RED tests for the exact binding:

  - radius 16 enumerates 1,089 unique columns around the committed publisher center;
  - an equal loaded count with one missing target and one source/foreign replacement fails;
  - publisher cohort cannot bind before its FIFO-committed update matches the `MovePlayer` target;
  - requested/resident keys outside target or any source-column intersection fail;
  - write-buffer/upload acknowledgement without a present-return plus submitted-work callback
    never settles;
  - an empty `TargetRenderExpectation` fails for the non-empty LeafForest fixture;
  - `identical_subset_presented_manifests_do_not_settle` freezes independently expected entries
    `[(a, 7), (b, 7)]`, submits two consecutive GPU-completed presented manifests that are both
    the identical subset `[(a, 7)]`, asserts `missing_target_instances == 1`, and asserts the
    consecutive-frame pair never advances;
  - two consecutive GPU-completed frames with identical sorted manifest and view generation are
    required only after each actual manifest equals the frozen expectation; expectation,
    manifest, or generation changes reset the pair;
  - source/foreign/stale-generation instances or orphan allocations block completion;
  - a newly prepared generation waits for a later queue/draw pass; direct and MDI manifests match;
  - forced remesh starts after teleport binding, returns an exact key/generation manifest, and
    uses the same presented/GPU fence;
  - target mutation tracking remains disarmed until the forest cohort is bound, then ignores the
    source coordinate and closes only on the target coordinate's presented generation.

  Extend PowerShell tests to prove trigger isolation: `WorldReady` works with the pinned baseline
  client, `VisualFixtureReady` works for near GalleryFront/Back without
  `--full-view-teleport-gate`, and `FullViewPresented` rejects a run without both binding markers.
  Assert baseline/full-view forest modes publish the same canonical fixture-layout hash and cannot
  be selected together.
  Validate `teleport_settle_ms` and `forced_full_view_remesh_ms` independently as finite values at
  or below 2,000 ms.

- [ ] **Step 2: Run RED**

  Run:

  ```text
  cargo test -p render --test plugin --locked -- --nocapture
  cargo test -p bedrock-client --locked -- --nocapture
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
  ```

  Expected: FAIL because exact cohort/source status, independent expected-manifest equality,
  presented-frame GPU fences, leaf evidence modes, target mutation retargeting, and independent
  resource triggers do not exist.

- [ ] **Step 3: Implement deterministic leaf fixtures and the binding full-view proof**

  Keep `LeafGalleryFront`/`LeafGalleryBack` near the initial cohort. Each builds three 2x2x2
  self-colored cubes, three labeled common-leaf cubes, leaf/leaf adjacency, opaque backing, and
  near/far panels. `New-FullViewTeleportPlan -LeafForest` uses the existing 65-chunk offset,
  builds deterministic persistent canopies at the target, and records a mutation coordinate
  inside the target forest. Its sequence is fixed: write all forest commands; issue and observe
  the BDS `list` processing fence; atomically publish the hashed fixture/teleport plan and its
  `VISUAL_FIXTURE_READY=<path>` marker; verify that marker contains the planned target mutation
  coordinate; only then issue `tp`. Persist each transition in the harness event log so fixture
  readiness, teleport issue, `MovePlayer` ingress, target presentation, and mutation arming are
  ordered evidence rather than inferred from wall-clock delay.

  Implement exact cohort identity:

  ```rust
  #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
  pub struct ViewCohort {
      pub dimension: i32,
      pub center: [i32; 2],
      pub radius: i32,
  }

  impl ViewCohort {
      pub fn columns(self) -> BTreeSet<ChunkKey> {
          let mut columns = BTreeSet::new();
          for x in self.center[0] - self.radius..=self.center[0] + self.radius {
              for z in self.center[1] - self.radius..=self.center[1] + self.radius {
                  columns.insert(ChunkKey::new(self.dimension, x, z));
              }
          }
          columns
      }
  }

  pub struct CohortStatus {
      pub target: ViewCohort,
      pub committed: Option<ViewCohort>,
      pub expected: usize,
      pub loaded_target: usize,
      pub missing_target: usize,
      pub foreign_loaded: usize,
      pub foreign_requested: usize,
      pub foreign_resident: usize,
      pub source_leftover: usize,
      pub resident_count: usize,
      pub resident_hash: u64,
      pub known_air_count: usize,
      pub known_air_hash: u64,
      pub bounded_work_empty: bool,
  }
  ```

  Reject a radius other than 16 in this gate and assert `columns().len() == 1_089`. At far
  `MovePlayer` ingress, capture the previously committed radius-16 publisher cohort and set
  `source = source_cohort.columns()`; assert that source also has 1,089 columns and is disjoint
  from the planned 65-chunk-offset target. Bind the target only after the matching publisher
  update commits in FIFO order. `CohortStatus::is_exact()` requires `committed == Some(target)`,
  `expected == loaded_target == 1_089`, every missing/foreign/source-leftover count to be zero,
  and every bounded network/decode/mesh/render queue to be empty. Hash sorted resident and
  known-air `SubChunkKey` identities and report every field even on failure, so equal column
  counts cannot hide a foreign resident, stale known-air identity, or source remainder.

  Carry mesh generation into `ChunkRenderInstance` and increment a monotonic `view_generation`
  when the bound cohort changes and when forced remesh begins. Once the exact cohort is bound and
  all target uploads are committed, freeze `TargetRenderExpectation.manifest` from an
  authoritative main-world expectation map populated when each `WorldMeshChange::Upsert/Remove`
  is accepted by `ChunkRenderQueue` and reconciled with its upload acknowledgement. Sort by
  `(SubChunkKey, mesh_generation)` and reject an empty LeafForest expectation. Never derive this
  expected set from `ChunkRenderInstance` extraction, render-phase items, or a presented frame.
  Independently build
  `PresentedFrameAck.presented_manifest` from instances actually queued/drawn by both direct and
  MDI paths. Compute `missing_target_instances = expected - presented` and
  `unexpected_target_instances = presented - expected` over the full pairs, so a key at the wrong
  generation contributes to the mismatch. Require sorted-array equality and both counts zero.

  In `RenderApp`, add the acknowledgement system to `RenderSystems::Render` after Bevy's
  `render_system`. After present returns, submit an empty sentinel command buffer, attach
  `on_submitted_work_done`, and poll nonblocking. Only then publish `PresentedFrameAck`; an upload
  acknowledgement cannot substitute. A generation prepared this frame is eligible only after a
  later queue/draw pass observes it.

  Bind teleport completion to two consecutive GPU-completed acknowledgements with identical
  cohort, `presented_manifest`, and `view_generation`, each exactly equal to the same frozen
  `TargetRenderExpectation`, with missing/unexpected/source/foreign/stale-generation/orphan counts
  all zero. Two identical empty or partial actual manifests therefore cannot settle the gate.
  `teleport_settle_ms` is
  `second_identical_ack.gpu_completed_at - move_player_ingress_at`. Only after that completion,
  call `WorldStream::remesh_all_resident(started)` and return its exact sorted key/new-generation
  manifest as the independently expected remesh set. Apply the same equality and presented/GPU
  fence; `forced_full_view_remesh_ms` is
  `gpu_completed_at - started`. Emit distinct `RUST_MCBE_TELEPORT_SETTLED` and
  `RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED` markers only after these proofs. Their metrics
  records carry the exact `CohortStatus`, expected and presented manifest hashes and lengths, view
  generation, present-return timestamp, GPU-completion timestamp, and
  missing/unexpected/source/foreign/stale-generation/orphan counts; the harness validates those
  fields instead of treating marker text alone as proof.

  After the binding teleport marker, recompute the deterministic target mutation coordinate in
  Rust and compare it with the already-published PowerShell forest manifest. Only on equality,
  call `AcceptanceRun::retarget_mutation(coordinate, armed_at)` and echo the coordinate in the
  arming event. The harness then stops any pre-teleport mutation loop and begins the alternating
  `setblock` loop at that target coordinate. The source coordinate is no longer mutated, and a
  source upload/present cannot close the target `MutationTracker`.

- [ ] **Step 4: Implement independent steady-resource triggers**

  Resolve an explicit baseline client only when both prebuilt options are supplied and hash it
  before launch. Route exactly one trigger per run:

  - `WorldReady`: pinned opaque baseline, armed by its compatible `RUST_MCBE_WORLD_READY` marker;
  - `VisualFixtureReady`: near GalleryFront/Back, armed after the observable
    `VISUAL_FIXTURE_READY=<path>` sequence and never passed `--full-view-teleport-gate`;
  - `FullViewPresented`: far forest, armed only after both binding teleport and forced-remesh
    markers and passed `--full-view-teleport-gate --frame-cap 60`.

  Sample exact app/core process handles once per second for 30 seconds, call `Refresh()`, use a
  monotonic stopwatch, normalize CPU by logical processors, and retain at most 600 samples. Write
  this bounded shape:

  ```json
  {
    "schema": "rust-mcbe-steady-resources-v2",
    "trigger": "VisualFixtureReady",
    "duration_seconds": 30,
    "processor_count": 12,
    "samples": [
      {"elapsed_seconds": 1.0, "combined_rss_bytes": 2, "cpu_percent": 0.1}
    ],
    "summary": {
      "sample_count": 30,
      "max_combined_rss_bytes": 2,
      "mean_cpu_percent": 0.1,
      "p95_cpu_percent": 0.1
    }
  }
  ```

  Gate the steady-window maximum RSS at 650 MB and both mean/p95 normalized CPU at 15%; record
  whole-run peak RSS separately. Assert the selected trigger in the artifact so a baseline or
  gallery sample cannot be mislabeled as post-full-view evidence.

- [ ] **Step 5: Commit the tested harness**

  Run:

  ```text
  cargo test -p render --test plugin --locked -- --nocapture
  cargo test -p bedrock-client --locked -- --nocapture
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
  ```

  Expected: presented-frame/direct/MDI, cohort/mutation/serialization, and PowerShell trigger
  tests pass. Then:

  ```text
  git add app/src/args.rs app/src/main.rs app/src/metrics.rs app/src/world_stream.rs crates/render/src/lib.rs crates/render/src/plugin.rs crates/render/tests/plugin.rs scripts/acceptance.ps1 scripts/tests/acceptance.Tests.ps1
  git commit -m "test: add deterministic leaf live gate"
  ```

- [ ] **Step 6: Run the clean no-assets code gate**

  Start with no `.local/assets` in a clean verification worktree and run:

  ```text
  cargo fmt --all -- --check
  cargo test --workspace --locked -- --nocapture
  cargo clippy --workspace --all-targets --locked -- -D warnings
  go test ./core/... -count=1
  go vet ./core/...
  go -C tools/registrygen test ./... -count=1
  powershell -NoProfile -File scripts/tests/vanilla-assets.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
  git diff --check
  ```

  Expected: every command exits zero; the diagnostic startup path works; tracked files contain no
  `.png`, `.tga`, `.zip`, or `.mcbea`; no tracked path begins `.local/assets`.

- [ ] **Step 7: Reproduce the registry and compile the ignored final blob**

  Run:

  ```powershell
  go -C tools/registrygen run . -out ../../.local/assets/block-registry-v1001-repro.bin
  if ((Get-FileHash crates/assets/data/block-registry-v1001.bin).Hash -ne
      (Get-FileHash .local/assets/block-registry-v1001-repro.bin).Hash) {
      throw 'tracked registry is not reproducible'
  }
  powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula
  cargo run -p assets --bin assetc -- compile `
    --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack `
    --registry crates/assets/data/block-registry-v1001.bin `
    --out .local/assets/compiled/vanilla-v1001.mcbea
  ```

  Expected: archive hash equals the pinned value; registry has 16,913/713/669/44/1 exact flag
  counts; compiler reports exact visuals/materials/cutout-materials/layers/mip bytes; the blob
  decodes as schema 2 and remains ignored.

- [ ] **Step 8: Reconstruct the same-scene opaque baseline**

  Create an ignored detached worktree at the pinned base, build its client/compiler into ignored
  target paths, and compile its schema-1 local blob from the same verified source:

  ```powershell
  git worktree add --detach .local/comparison/opaque-base 5933209fe053aff0f2164262f129635b947a636b
  cargo build --release -p bedrock-client --locked `
    --manifest-path .local/comparison/opaque-base/Cargo.toml `
    --target-dir .local/comparison/opaque-target
  cargo run --manifest-path .local/comparison/opaque-base/Cargo.toml -p assets --bin assetc -- `
    compile --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack `
    --registry .local/comparison/opaque-base/crates/assets/data/block-registry-v1001.bin `
    --out .local/comparison/opaque-base.mcbea
  ```

  Run the current harness in `-LeafForestBaseline` mode with the base executable/blob and the
  independent `WorldReady` resource trigger. This mode publishes the same canonical forest-layout
  hash as the binding full-view run but never passes the far-tracker flag to the old client.
  Record exact executable/blob/fixture hashes, diagnostic quads, missing mappings, and resources.
  The old schema is consumed only by the old client; the new runtime continues rejecting it.

  ```powershell
  $bds = (Resolve-Path .local/bds/bedrock-server-1.26.32.2).Path
  powershell -NoProfile -File scripts/acceptance.ps1 `
    -DurationSeconds 60 `
    -BdsDir $bds `
    -MetricsOut .local/evidence/opaque-base-leaf-forest.json `
    -Assets .local/comparison/opaque-base.mcbea `
    -LeafForestBaseline `
    -SteadyResourceTrigger WorldReady `
    -ClientExecutable .local/comparison/opaque-target/release/bedrock-client.exe `
    -SkipClientBuild `
    -UseVsync
  ```

  Expected: the base session passes existing radius/error/mutation checks, does not arm full-view
  tracking, and publishes the forest layout hash, diagnostic count, WorldReady-triggered process
  samples, and executable/blob hashes.

- [ ] **Step 9: Run final gallery/forest visual and performance evidence**

  Run fresh 60-second radius-16 near sessions for `LeafGalleryFront`/`LeafGalleryBack`, then a
  separate `LeafForestFullView` session using the final ignored blob. Require zero missing
  mappings and decode errors. Compute forest diagnostic reduction as
  `base_diagnostic - final_diagnostic` and `100 * reduction / base_diagnostic`; require the same
  canonical forest-layout hash.

  Attempt Computer Use first on the gallery. Inspect cutout holes, opaque backing through holes,
  self-colored cherry/azalea correctness, leaf adjacency, opaque/leaf boundaries, near/far mips,
  and explicit grayscale/tint-deferred common leaves. If Computer Use succeeds, focus the
  client, verify forward/back/strafe/vertical fly movement, mouse-look yaw and pitch, cursor
  capture, Escape release, and no stuck movement or rotation. If snapshot fails with
  `SetIsBorderRequired failed: No such interface supported (0x80004002)`, send no app input after
  failure. Passive GDI frames may supplement visual evidence but must be labeled passive and
  cannot claim focus/input/capture/release. Hash every accepted manifest/frame.

  Require the forest artifact to report 1,089 expected/loaded target columns, zero missing,
  foreign loaded/requested/resident, and source-leftover columns; resident and known-air
  count+hash identities; independently expected and actually presented manifest hashes/lengths;
  exact equality for both GPU-completed frames; zero missing/unexpected target and
  source/foreign/stale-generation/orphan render state; and post-teleport target mutation
  visibility at most 100 ms. Record p50/p95/p99/max frame,
  max decode/mesh/remesh/mutation latency, both binding full-view timings,
  resident/visible subchunks, GPU bytes, client/core/combined RSS, normalized steady CPU, and all
  queue peaks. A 14.2--15.1 second full-view remesh was observed before this slice; do not close
  the <=2-second gate unless fresh evidence actually passes it.

  ```powershell
  $bds = (Resolve-Path .local/bds/bedrock-server-1.26.32.2).Path
  foreach ($pose in @('LeafGalleryFront', 'LeafGalleryBack')) {
      powershell -NoProfile -File scripts/acceptance.ps1 `
        -DurationSeconds 60 `
        -BdsDir $bds `
        -MetricsOut ".local/evidence/final-$($pose.ToLowerInvariant()).json" `
        -Assets .local/assets/compiled/vanilla-v1001.mcbea `
        -VisualFixturePose $pose `
        -SteadyResourceTrigger VisualFixtureReady `
        -UseVsync
      if ($LASTEXITCODE -ne 0) { throw "$pose acceptance failed" }
  }
  powershell -NoProfile -File scripts/acceptance.ps1 `
    -DurationSeconds 900 `
    -BdsDir $bds `
    -MetricsOut .local/evidence/final-leafforestfullview.json `
    -Assets .local/assets/compiled/vanilla-v1001.mcbea `
    -LeafForestFullView `
    -FullViewTeleportGate `
    -SteadyResourceTrigger FullViewPresented
  if ($LASTEXITCODE -ne 0) { throw 'LeafForestFullView acceptance failed' }
  ```

  Expected: all runs use fresh BDS runtimes and pass their mode-specific triggers. Missing
  mappings/decode errors are zero; forest cohort/present/mutation proof is exact; final diagnostic
  quads are below the same-layout base count; exact budget results are available for the report.

- [ ] **Step 10: Write the report, run final independent review, and close only proven gates**

  `docs/phase-2-cutout-leaves-report.md` must state exact hashes/counts, same-scene diagnostic
  reduction, performance/budget results, screenshots and capture method, zero/unresolved errors,
  common tint deferral, and every failed or conditional gate. Mark Phase 2.4 in `plan.md` complete
  only if all binding gates pass.

  Request a final read-only review over
  `5933209fe053aff0f2164262f129635b947a636b..HEAD`. The reviewer must verify schema rejection,
  flag counts, mip coverage, asymmetric culling/connectivity, one opaque pipeline/bind group/MDI,
  exact eight-byte records, no flat arrays, no tracked Mojang payload, same-fixture evidence,
  1,089-column/source-exclusion and resident/known-air identities, the independently expected
  target render-generation manifest, identical-subset rejection, direct/MDI actual manifests, the
  present-return/submitted-work GPU fence, target mutation retargeting, mode-specific resource
  triggers, resource budgets, tint deferral, and Computer Use/passive-capture wording. Fix and
  re-review all Critical and Important findings.

  Re-run the full Step 6 gate plus:

  ```powershell
  git diff --check 5933209fe053aff0f2164262f129635b947a636b..HEAD
  $payload = git ls-files | rg "\.(png|tga|zip|mcbea)$|^\.local/assets/" 2>&1
  $rgExit = $LASTEXITCODE
  if ($rgExit -eq 0) { $payload; throw 'forbidden tracked asset payload found' }
  if ($rgExit -gt 1) { throw "rg payload scan failed with exit $rgExit`: $payload" }
  ```

  Expected: `rg` exit 1 is treated as a clean no-match result, exit 0 fails on forbidden content,
  and exit greater than 1 fails as a tool error. The full gate passes and final review says ready.
  Then:

  ```text
  git add docs/phase-2-cutout-leaves-report.md plan.md
  git commit -m "docs: record cutout leaf evidence"
  ```

  If any resource, visual, schema, diagnostic, or review gate is unmet, keep Phase 2.4 open and
  commit an accurate conditional report rather than claiming completion.
