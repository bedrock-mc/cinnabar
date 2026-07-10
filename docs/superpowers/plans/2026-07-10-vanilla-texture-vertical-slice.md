# Vanilla Texture Vertical Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render ordinary full-cube blocks from the live protocol-1001 world with the matching Minecraft Bedrock vanilla textures, correct per-face mapping, repeating greedy UVs, and isolated mipmaps.

**Architecture:** A pinned Dragonfly tool exports sequential IDs, network hashes, canonical block states, and full-cube facts. A new Rust `assets` crate compiles the user's verified local `bedrock-samples` pack into an ignored runtime blob containing compact block/material tables and a 16x16 mipmapped 2D texture array. The existing packed renderer replaces the second quad word's debug runtime value with a material ID and samples the one shared texture array without introducing per-subchunk Bevy meshes or materials.

**Tech Stack:** Rust 1.93.1, Bevy 0.18.1/wgpu, serde/serde_json, image, sha2, Go 1.26.1, Dragonfly `v0.10.15-0.20260709170650-b85c56ffea6b`.

## Global Constraints

- Keep runtime chunk data palette + packed indices; never expand a subchunk into a flat per-block array.
- Keep `PackedQuad` exactly 8 bytes and retain the custom global-buffer render phase, one pipeline, one bind group, and MDI/direct fallback.
- Use a 2D texture array with per-layer mipmaps; never build a stitched atlas.
- The base input is `bedrock-samples-v1.26.30.32-preview-full.zip` from tag `v1.26.30.32-preview`, commit `020f1cf4b2baef78e635d4ce7498eb16a429dcbb`, SHA-256 `12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c`.
- No raw or derived Mojang asset payload is committed to Git. Archives, extracted files, generated texture pixels, and compiled runtime blobs stay in ignored `.local/assets` or `target` paths.
- Git may contain importer/compiler code, provenance hashes, the Dragonfly-derived registry export, and programmatically generated synthetic test images only.
- The first slice supports opaque axis-aligned full cubes. Non-cube, cutout, blend, tint, and animated visuals resolve to a diagnostic material until their later Phase 2 tasks.
- Unknown or malformed mappings never perform unchecked GPU indexing; they resolve to the diagnostic material and increment a bounded/rate-limited counter.
- Asset decode, mapping, mip generation, and mesh work run off the render thread. GPU creation/upload remains on Bevy's render thread and respects Task 7 bounds.
- Combined client + core RSS remains at or below 650 MB steady state at radius 16; full-view remesh after teleport remains at or below 2 seconds.
- Every behavioral change follows RED -> GREEN -> REFACTOR, and each task receives an independent spec/quality review before the next integration-dependent task.

---

### Task 1: Pinned Local-Only Vanilla Source Contract

**Files:**
- Create: `assets/vanilla-source.json`
- Create: `scripts/fetch-vanilla-assets.ps1`
- Create: `scripts/fetch-vanilla-assets.sh`
- Create: `scripts/tests/vanilla-assets.ps1`
- Modify: `.gitignore`

**Interfaces:**
- Produces ignored source directory `.local/assets/bedrock-samples/v1.26.30.32-preview/full/`.
- Produces exact dry-run/fetch commands:
  - `scripts/fetch-vanilla-assets.ps1 -AcceptEula [-DryRun]`
  - `scripts/fetch-vanilla-assets.sh --accept-eula [--dry-run]`
- Does not produce or stage any Mojang-owned file under a tracked path.

- [ ] **Step 1: Write the provenance manifest and failing script contract test**

  Add this exact checked-in metadata:

  ```json
  {
    "schema": 1,
    "tag": "v1.26.30.32-preview",
    "commit": "020f1cf4b2baef78e635d4ce7498eb16a429dcbb",
    "archive": "bedrock-samples-v1.26.30.32-preview-full.zip",
    "url": "https://github.com/Mojang/bedrock-samples/releases/download/v1.26.30.32-preview/bedrock-samples-v1.26.30.32-preview-full.zip",
    "sha256": "12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c",
    "artifact_policy": "local-only",
    "cache_dir": ".local/assets/bedrock-samples/v1.26.30.32-preview/full"
  }
  ```

  `scripts/tests/vanilla-assets.ps1` invokes the PowerShell script in dry-run mode and asserts:

  ```powershell
  $dry = & $fetch -AcceptEula -DryRun 2>&1 | Out-String
  if ($LASTEXITCODE -ne 0) { throw "dry-run failed: $dry" }
  foreach ($needle in @($source.url, $source.sha256, $source.cache_dir)) {
      if ($dry -notmatch [regex]::Escape($needle)) { throw "missing '$needle'" }
  }
  & $fetch -DryRun *> $null
  if ($LASTEXITCODE -eq 0) { throw "EULA gate unexpectedly succeeded" }
  if (git ls-files -- '.local/assets/*' | Select-String '.') {
      throw "Mojang cache path is tracked"
  }
  ```

- [ ] **Step 2: Verify RED**

  Run: `powershell -NoProfile -File scripts/tests/vanilla-assets.ps1`

  Expected: FAIL because `scripts/fetch-vanilla-assets.ps1` does not exist.

- [ ] **Step 3: Implement local-only fetch scripts**

  Both scripts must:

  1. refuse to proceed without the explicit EULA flag;
  2. read all source values from `assets/vanilla-source.json`;
  3. download to `.local/assets/downloads/<archive>.partial`;
  4. verify the complete SHA-256 before atomic rename;
  5. extract into a sibling temporary directory and atomically rename to `cache_dir`;
  6. normalize the archive's single top-level directory so `cache_dir/resource_pack/blocks.json` exists;
  7. leave partial/extracted files ignored and print the exact resolved paths;
  8. in dry-run mode, print commands and perform no filesystem mutation.

  PowerShell hashing/extraction uses:

  ```powershell
  $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
  if ($actual -ne $source.sha256) { throw "SHA-256 mismatch: expected $($source.sha256), got $actual" }
  Expand-Archive -LiteralPath $archivePath -DestinationPath $temporaryExtract
  ```

  Bash uses `curl --fail --location`, `sha256sum` (or `shasum -a 256` on macOS), `unzip`, and `mv` within the same parent directory.

- [ ] **Step 4: Verify GREEN and the no-assets boundary**

  Run:

  ```powershell
  powershell -NoProfile -File scripts/tests/vanilla-assets.ps1
  git check-ignore .local/assets/example
  git status --short
  ```

  Expected: the test passes, `.local/assets/example` is ignored, and no downloaded/generated asset appears in Git status.

- [ ] **Step 5: Commit**

  ```text
  git add .gitignore assets/vanilla-source.json scripts/fetch-vanilla-assets.ps1 scripts/fetch-vanilla-assets.sh scripts/tests/vanilla-assets.ps1
  git commit -m "chore: pin local vanilla asset source"
  ```

---

### Task 2: Deterministic Dragonfly Block Registry Export

**Files:**
- Create: `tools/registrygen/go.mod`
- Create: `tools/registrygen/go.sum`
- Create: `tools/registrygen/main.go`
- Create: `tools/registrygen/main_test.go`
- Create: `crates/assets/data/block-registry-v1001.bin`
- Create: `crates/assets/data/block-registry-v1001.sha256`

**Interfaces:**
- CLI: `go run ./tools/registrygen -out <path>`.
- Binary header: `BREG1001`, little-endian `u32 record_count`, followed by sorted records.
- Each record contains `sequential_id: u32`, `network_hash: u32`, `flags: u8`, `name_len: u16`, `state_len: u32`, UTF-8 name, and canonical UTF-8 JSON state properties.
- Flag bit 0 is air; bit 1 is Dragonfly `model.Solid` full cube.

- [ ] **Step 1: Write deterministic exporter tests**

  Tests call a pure `encode(records []Record) ([]byte, error)` and assert exact bytes for two out-of-order records, duplicate sequential/hash rejection, canonical property-key ordering, and stable output across 100 shuffled inputs. An integration test finalizes `world.DefaultBlockRegistry`, imports `github.com/df-mc/dragonfly/server/block` for registrations, and asserts:

  ```go
  records := collect(world.DefaultBlockRegistry)
  if len(records) < 1000 { t.Fatalf("registry too small: %d", len(records)) }
  air := findByName(records, "minecraft:air")
  if air.Flags&flagAir == 0 { t.Fatal("air flag missing") }
  if air.NetworkHash != 0xdbf44120 { t.Fatalf("air hash = %#x", air.NetworkHash) }
  ```

- [ ] **Step 2: Verify RED**

  Run: `go -C tools/registrygen test ./... -count=1`

  Expected: FAIL because the package and encoder do not exist.

- [ ] **Step 3: Implement the pinned exporter**

  Pin:

  ```go
  require github.com/df-mc/dragonfly v0.10.15-0.20260709170650-b85c56ffea6b
  ```

  Finalize and enumerate the registry:

  ```go
  world.DefaultBlockRegistry.Finalize()
  blocks := world.DefaultBlockRegistry.Blocks()
  records := make([]Record, 0, len(blocks))
  for rid, value := range blocks {
      name, properties := value.EncodeBlock()
      networkHash, ok := world.DefaultBlockRegistry.RuntimeIDToHash(uint32(rid))
      if !ok { return nil, fmt.Errorf("runtime ID %d has no network hash", rid) }
      flags := uint8(0)
      if name == "minecraft:air" { flags |= flagAir }
      if _, ok := value.Model().(model.Solid); ok { flags |= flagFullCube }
      records = append(records, Record{
          SequentialID: uint32(rid), NetworkHash: networkHash,
          Flags: flags, Name: name, StateJSON: canonicalJSON(properties),
      })
  }
  ```

  Use checked length conversions and reject duplicate sequential IDs, hashes, names longer than `u16::MAX`, state payloads over 1 MiB, or more than 65,536 records.

- [ ] **Step 4: Generate and verify the committed registry metadata**

  Run:

  ```text
  go -C tools/registrygen run . -out ../../crates/assets/data/block-registry-v1001.bin
  go -C tools/registrygen test ./... -count=1
  go -C tools/registrygen run . -out ../../.local/assets/registry-second.bin
  ```

  Expected: both generated files have identical SHA-256, the checked-in `.sha256` matches, and the binary contains no Mojang PNG/JSON asset payload.

- [ ] **Step 5: Commit**

  ```text
  git add tools/registrygen crates/assets/data/block-registry-v1001.bin crates/assets/data/block-registry-v1001.sha256
  git commit -m "feat: export pinned block registry"
  ```

---

### Task 3: Pack Source Parsing and Face Resolution

**Files:**
- Create: `crates/assets/Cargo.toml`
- Create: `crates/assets/src/lib.rs`
- Create: `crates/assets/src/error.rs`
- Create: `crates/assets/src/pack.rs`
- Create: `crates/assets/src/registry.rs`
- Create: `crates/assets/tests/pack.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- `pub struct RegistryRecord { pub sequential_id: u32, pub network_hash: u32, pub name: Box<str>, pub canonical_state: Box<str>, pub flags: BlockFlags }`
- `pub enum BlockFace { West, East, Down, Up, North, South }` with discriminants matching render `Face` order.
- `pub fn read_registry(bytes: &[u8]) -> Result<Box<[RegistryRecord]>, AssetError>`.
- `pub fn read_pack(root: &Path) -> Result<PackSources, AssetError>`.
- `pub fn resolve_texture_key(blocks: &BlockTextureMap, record: &RegistryRecord, face: BlockFace) -> TextureKey`.

- [ ] **Step 1: Write synthetic parser tests**

  Tests create JSON in a temporary directory and cover:

  - leading `//` comment lines in `terrain_texture.json` and `flipbook_textures.json`;
  - namespace stripping from `minecraft:stone` to the `blocks.json` key `stone`;
  - scalar block textures on all faces;
  - `up/down/side` and six explicit face keys;
  - horizontal `pillar_axis=x|y|z` face permutation;
  - terrain values as string, `{"path": "..."}`, and arrays;
  - deterministic first-variant selection for this slice;
  - rejection of `..`, absolute paths, missing keys, invalid UTF-8, duplicate registry IDs/hashes, oversized counts, and truncated registry records.

  The expected face order is:

  ```rust
  [
      BlockFace::West, BlockFace::East, BlockFace::Down,
      BlockFace::Up, BlockFace::North, BlockFace::South,
  ]
  ```

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p assets --test pack --locked -- --nocapture`

  Expected: FAIL because the `assets` crate and parsers do not exist.

- [ ] **Step 3: Implement bounded source models**

  Add `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `bitflags = "2"`, and `thiserror.workspace = true`.

  Model Bedrock's texture union without unbounded eager allocations:

  ```rust
  #[derive(Deserialize)]
  #[serde(untagged)]
  enum TextureValue {
      Key(String),
      Faces(FaceKeys),
  }

  #[derive(Deserialize)]
  #[serde(untagged)]
  enum TerrainValue {
      Path(String),
      Entry { path: String, overlay_color: Option<String> },
      Variants(Vec<TerrainVariant>),
  }
  ```

  Enforce: at most 65,536 registry records, 8,192 texture keys, 256 variants per key, 4 KiB path length, and 16 MiB per JSON file. Strip only complete leading comment lines before `serde_json`; do not implement a permissive JavaScript parser.

- [ ] **Step 4: Implement face fallback and pillar permutation**

  Strip only the exact `minecraft:` namespace when looking up vanilla `blocks.json` keys. Resolution order is explicit face -> `side` for horizontal faces -> `up`/`down` -> scalar -> diagnostic. For records whose canonical state contains `pillar_axis` or `axis`, map the original up/down texture pair onto the selected axis while preserving a deterministic UV-rotation flag.

- [ ] **Step 5: Verify GREEN**

  Run:

  ```text
  cargo test -p assets --test pack --locked -- --nocapture
  cargo test -p assets --locked
  cargo fmt --all -- --check
  ```

  Expected: all bounded parser, registry, face, and path-safety tests pass.

- [ ] **Step 6: Commit**

  ```text
  git add Cargo.toml Cargo.lock crates/assets
  git commit -m "feat: parse vanilla block textures"
  ```

---

### Task 4: Deterministic Texture Array Compiler and Runtime Blob

**Files:**
- Create: `crates/assets/src/compiler.rs`
- Create: `crates/assets/src/image.rs`
- Create: `crates/assets/src/blob.rs`
- Create: `crates/assets/src/bin/assetc.rs`
- Create: `crates/assets/tests/compiler.rs`
- Create: `crates/assets/tests/blob.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/Cargo.toml`

**Interfaces:**
- CLI: `cargo run -p assets --bin assetc -- compile --pack <resource_pack> --registry <registry.bin> --out <ignored-dir>/vanilla-v1001.mcbea`.
- `pub struct CompiledAssets { pub visuals: Box<[BlockVisual]>, pub hashed: Box<[(u32, u32)]>, pub materials: Box<[Material]>, pub textures: TextureArray }`.
- Blob magic `MCBEAS01`, format version 1, tile size 16, mip count 5.
- `Material` is two `u32` GPU words: layer and flags.

- [ ] **Step 1: Write compiler and mip RED tests**

  Generate synthetic 16x16 RGBA images in test memory; do not check in PNGs. Assert:

  - identical pixel+flags inputs deduplicate to one layer/material;
  - different pixels or UV flags do not deduplicate incorrectly;
  - diagnostic checkerboard is layer 0/material 0;
  - a red layer and blue layer generate independent 8x8, 4x4, 2x2, and 1x1 mips with no cross-layer colour;
  - alpha filtering uses premultiplied-alpha accumulation before unpremultiplication;
  - output is byte-identical across shuffled input maps;
  - full-cube records compile six face materials while non-full-cubes map to diagnostic;
  - sequential and high-hash lookup tables resolve to the same `BlockVisual`;
  - missing PNG, malformed PNG, wrong dimensions, more than 2,048 layers, or more than 65,536 materials fails with the source key/path.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p assets --test compiler --test blob --locked -- --nocapture`

  Expected: FAIL because compiler/blob APIs do not exist.

- [ ] **Step 3: Implement the compiler**

  Add `image = { version = "0.25", default-features = false, features = ["png"] }`, `sha2 = "0.10"`, and `clap = { version = "4", features = ["derive"] }`.

  Use these compact runtime types:

  ```rust
  pub const DIAGNOSTIC_MATERIAL: u32 = 0;
  pub const MAX_TEXTURE_LAYERS: usize = 2_048;
  pub const MAX_MATERIALS: usize = 65_536;

  #[repr(C)]
  #[derive(Clone, Copy, Debug, Eq, PartialEq)]
  pub struct Material {
      pub layer: u32,
      pub flags: u32,
  }

  #[derive(Clone, Copy, Debug, Eq, PartialEq)]
  pub struct BlockVisual {
      pub faces: [u32; 6],
      pub flags: BlockFlags,
  }
  ```

  Resolve only referenced full-cube texture paths. Require static images to be exactly 16x16 for this slice; recognize flipbook paths and map them to diagnostic rather than treating a vertical strip as a static layer. Sort canonical texture-path + UV/material flags before assigning IDs.

- [ ] **Step 4: Implement per-layer mip generation and checked blob encoding**

  Convert sRGB channels to linear floats, average each 2x2 footprint with premultiplied alpha, unpremultiply, convert back to sRGB, and clamp. Serialize every integer little-endian with checked section sizes. Append a SHA-256 of all preceding blob bytes and validate it before any large runtime allocation.

- [ ] **Step 5: Verify GREEN and local-only output**

  Run:

  ```text
  cargo test -p assets --test compiler --test blob --locked -- --nocapture
  cargo run -p assets --bin assetc -- --help
  cargo clippy -p assets --all-targets --locked -- -D warnings
  git status --short
  ```

  Expected: all tests pass, help documents the compile inputs, and no runtime blob is tracked.

- [ ] **Step 6: Commit**

  ```text
  git add Cargo.lock crates/assets
  git commit -m "feat: compile vanilla texture arrays"
  ```

---

### Task 5: Immutable Runtime Lookup and Diagnostic Counters

**Files:**
- Create: `crates/assets/src/runtime.rs`
- Create: `crates/assets/tests/runtime.rs`
- Modify: `crates/assets/src/lib.rs`

**Interfaces:**
- `pub fn RuntimeAssets::decode(bytes: &[u8]) -> Result<Self, AssetError>`.
- `pub enum NetworkIdMode { Sequential, Hashed }`.
- `pub fn RuntimeAssets::resolve(&self, mode: NetworkIdMode, network_value: u32) -> ResolvedBlock`.
- `pub fn RuntimeAssets::material(&self, id: u32) -> Material`.
- `pub fn RuntimeAssets::texture_array(&self) -> &TextureArray`.
- Sequential IDs use direct indexed lookup; hashes use a sorted slice and binary search.

- [ ] **Step 1: Write malformed/runtime lookup tests**

  Cover bad magic/version/hash, truncated sections, non-monotonic hash keys, out-of-range material/layer IDs, mismatched mip byte lengths, allocation limits, direct sequential lookup, binary-search hash lookup, mode isolation for numerically colliding sequential/hash keys, and unknown-value diagnostic counting. Prove 10,000 repeated misses keep one atomic counter and no per-ID unbounded map.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p assets --test runtime --locked -- --nocapture`

  Expected: FAIL because `RuntimeAssets` does not exist.

- [ ] **Step 3: Implement immutable lookup**

  ```rust
  pub struct RuntimeAssets {
      visuals: Box<[BlockVisual]>,
      hashed: Box<[(u32, u32)]>,
      materials: Box<[Material]>,
      textures: TextureArray,
      missing: AtomicU64,
  }

  pub fn resolve(&self, mode: NetworkIdMode, value: u32) -> ResolvedBlock {
      let resolved = match mode {
          NetworkIdMode::Sequential => self.visuals.get(value as usize).copied(),
          NetworkIdMode::Hashed => self.hashed
              .binary_search_by_key(&value, |entry| entry.0)
              .ok()
              .map(|index| self.visuals[self.hashed[index].1 as usize]),
      };
      resolved.map_or_else(|| {
          self.missing.fetch_add(1, Ordering::Relaxed);
          ResolvedBlock::diagnostic()
      }, ResolvedBlock::known)
  }
  ```

  Avoid cloning texture bytes after decode: retain one boxed backing allocation or move decoded sections directly into their final boxed slices.

- [ ] **Step 4: Verify GREEN**

  Run: `cargo test -p assets --locked -- --nocapture`

  Expected: all source, compiler, blob, malformed, and lookup tests pass.

- [ ] **Step 5: Commit**

  ```text
  git add crates/assets
  git commit -m "feat: load compact runtime assets"
  ```

---

### Task 6: Material-Aware Binary Greedy Meshing

**Files:**
- Modify: `crates/render/Cargo.toml`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `crates/protocol/src/world.rs`
- Modify: `crates/protocol/src/lib.rs`
- Modify: `crates/protocol/tests/world_packets.rs`
- Modify: `app/src/world_stream.rs`

**Interfaces:**
- `PackedQuad` remains `#[repr(C)]` and 8 bytes; its second word/accessor becomes `material_id: u32`.
- `mesh_sub_chunk(classifier, visuals, neighbours, sub_chunk) -> ChunkMesh` receives `&assets::RuntimeAssets`.
- Occupancy/air decisions still use the live `BlockClassifier`; face merge identity uses resolved material ID.
- `WorldBootstrap` exposes `block_network_ids_are_hashes: bool`; `WorldStream` converts it once to `assets::NetworkIdMode`.

- [ ] **Step 1: Write mesher RED tests**

  Build a synthetic `RuntimeAssets` fixture and assert:

  - `size_of::<PackedQuad>() == 8`;
  - high-bit network value `0xdbf44120` resolves in hashed mode without truncation;
  - a deliberately colliding low hash never resolves through the sequential table;
  - two adjacent blocks with the same face material merge;
  - two adjacent blocks with different material IDs split;
  - different runtime IDs that resolve to the same material may merge;
  - top/side/bottom faces receive their exact material IDs;
  - unknown and non-full-cube entries use diagnostic material 0;
  - all existing uniform-air, uniform-solid, neighbour culling, connectivity, and packed-position tests remain green.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p render --test mesh --locked -- --nocapture`

  Expected: FAIL because `PackedQuad` and `mesh_sub_chunk` still use runtime IDs.

- [ ] **Step 3: Implement face-material masks without flat blocks**

  Resolve each palette entry once per face into a small palette-material table, then read packed palette indices while building the existing `u64` occupancy/face masks. Replace only the merge-key value:

  ```rust
  #[repr(C)]
  pub struct PackedQuad {
      geometry: u32,
      material_id: u32,
  }

  let material_id = visuals
      .resolve(network_id_mode, runtime_id)
      .face(BlockFace::from(face))
      .material_id();
  ```

  Do not materialize `[u32; 4096]`, do not allocate per visible face, and preserve uniform-air/solid fast paths.

- [ ] **Step 4: Thread immutable assets through bounded mesh jobs**

  Add `block_network_ids_are_hashes` to `WorldBootstrap::from_game_data` and its protocol tests. Store `NetworkIdMode` plus `Arc<RuntimeAssets>` in `WorldStream`; clone only the `Arc` into Rayon jobs. Scope/revision cancellation and queue bounds remain unchanged.

- [ ] **Step 5: Verify GREEN and performance invariants**

  Run:

  ```text
  cargo test -p render --test mesh --locked -- --nocapture
  cargo test -p render --locked
  cargo test -p bedrock-client --locked
  cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings
  ```

  Expected: all existing and material-aware meshing/streaming tests pass; `PackedQuad` remains 8 bytes.

- [ ] **Step 6: Commit**

  ```text
  git add crates/protocol crates/render app/src/world_stream.rs Cargo.lock
  git commit -m "feat: mesh compact block materials"
  ```

---

### Task 7: Shared GPU Texture Array and Vertex-Pulled UVs

**Files:**
- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/src/chunk.wgsl`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `crates/render/Cargo.toml`
- Modify: `app/Cargo.toml`

**Interfaces:**
- One global chunk bind group:
  - binding 0: dynamic view uniform;
  - binding 1: packed quads;
  - binding 2: chunk origins;
  - binding 3: read-only `MaterialGpu { layer, flags }` storage buffer;
  - binding 4: `texture_2d_array<f32>`;
  - binding 5: filtering repeat sampler.
- The texture array is uploaded once per asset revision; chunk uploads never duplicate it.

- [ ] **Step 1: Write shader/plugin RED tests**

  Assert WGSL parses and contains bindings 3-5, `textureSample`, flat array-layer interpolation, and block-scale greedy UV reconstruction. Add pure tests for every face's four UV corners, 1x1 versus 16x16 repetition, 90/180/270 rotation, U/V reflection, array-layer limit rejection, exact mip upload offsets, bind-group rebuild only on resource identity change, and unchanged MDI/direct capability selection.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p render --test plugin --locked -- --nocapture`

  Expected: FAIL because the current pipeline exposes only view/quad/origin buffers and debug colours.

- [ ] **Step 3: Add the one global texture/material resources**

  Add Bevy's `bevy_image` feature. Extract an immutable `ChunkTextureAssets` resource backed by `Arc<RuntimeAssets>` to the render world, so extraction clones only the `Arc` and never the texture bytes. During prepare:

  ```rust
  render_device.limits().max_texture_array_layers >= texture.layers()
  render_device.limits().max_texture_dimension_2d >= texture.tile_size()
  ```

  Create one `Rgba8UnormSrgb` 2D array with all five mip levels, upload each layer/mip with correctly padded `bytes_per_row`, create one repeat/filtering sampler, and upload the two-word material table once. Account these bytes in texture metrics, not per-frame chunk upload bytes.

- [ ] **Step 4: Replace debug colour with vertex-pulled texture sampling**

  WGSL output:

  ```wgsl
  struct VertexOutput {
      @builtin(position) clip_position: vec4<f32>,
      @location(0) uv: vec2<f32>,
      @location(1) @interpolate(flat) layer: u32,
      @location(2) normal: vec3<f32>,
  }

  @fragment
  fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
      return textureSample(block_textures, block_sampler, vec3(in.uv, f32(in.layer)));
  }
  ```

  Derive UV magnitude from greedy width/height so repeat addressing tiles once per block. Apply rotation/reflection from material flags in the vertex shader. Remove shader debug-colour code only after the diagnostic checker texture is bound.

- [ ] **Step 5: Verify GREEN**

  Run:

  ```text
  cargo test -p render --test plugin --locked -- --nocapture
  cargo test -p render --locked
  cargo clippy -p render --all-targets --locked -- -D warnings
  ```

  Expected: shader/bind-group/upload tests pass, one pipeline/bind group remains, and MDI/direct tests are unchanged.

- [ ] **Step 6: Commit**

  ```text
  git add app/Cargo.toml crates/render Cargo.lock
  git commit -m "feat: sample shared block texture array"
  ```

---

### Task 8: App Asset Selection, Local Compilation, and Live Visual Gate

**Files:**
- Modify: `app/src/args.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/metrics.rs`
- Create: `app/tests/assets.rs`
- Create: `docs/phase-2-texture-slice-report.md`
- Modify: `README.md`

**Interfaces:**
- New flag: `--assets <path-to-vanilla-v1001.mcbea>`.
- Environment fallback: `RUST_MCBE_ASSETS`.
- Final fallback: ignored default `.local/assets/compiled/vanilla-v1001.mcbea`.
- If no valid local blob exists, the app starts with the synthetic diagnostic texture and prints the exact fetch/compile commands; it never embeds or downloads Mojang content silently.

- [ ] **Step 1: Write app selection/readiness RED tests**

  Assert CLI-over-environment-over-default precedence, malformed blob failure with exact path, missing blob diagnostic startup, and metrics fields:

  ```rust
  pub struct AssetMetrics {
      pub source_tag: String,
      pub source_sha256: String,
      pub blob_sha256: String,
      pub texture_layers: u32,
      pub texture_bytes_including_mips: u64,
      pub material_count: u32,
      pub missing_mapping_count: u64,
      pub diagnostic_quad_count: u64,
  }
  ```

  Add a world-ready log marker that includes asset source/blob hashes and resident/visible counts.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p bedrock-client --test assets --locked -- --nocapture`

  Expected: FAIL because `--assets` and asset metrics do not exist.

- [ ] **Step 3: Implement startup selection and diagnostic fallback**

  Load and validate the blob before constructing `WorldStream` and `DebugWorldPlugin`. Never read Mojang JSON/PNG files from the frame loop. The fallback uses a programmatically generated 16x16 magenta/black checker and material 0 only.

- [ ] **Step 4: Verify the full codebase before local Mojang ingestion**

  Run:

  ```text
  cargo fmt --all -- --check
  cargo test --workspace --locked -- --nocapture
  cargo clippy --workspace --all-targets --locked -- -D warnings
  go test ./core/... -count=1
  go vet ./core/...
  go -C tools/registrygen test ./... -count=1
  git diff --check
  ```

  Expected: every command exits zero with no warnings and no Mojang payload appears in `git status`.

- [ ] **Step 5: Fetch and compile the user's local vanilla copy**

  Run:

  ```powershell
  powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula
  cargo run -p assets --bin assetc -- compile `
    --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack `
    --registry crates/assets/data/block-registry-v1001.bin `
    --out .local/assets/compiled/vanilla-v1001.mcbea
  ```

  Expected: source/archive hashes match the manifest, compiler reports counts/hashes, output remains ignored, and `git status --short` contains no asset payload.

- [ ] **Step 6: Run the live visual/performance pass**

  Launch BDS/core and:

  ```text
  cargo run -p bedrock-client --locked -- --socket-dir .local/run --assets .local/assets/compiled/vanilla-v1001.mcbea
  ```

  Using Computer Use, inspect stone, dirt, grass, logs on all axes, planks, ores, sand, glass diagnostic handling, and at least one unknown/non-cube fallback. Check near/far mip transitions, all six face orientations, greedy UV repetition, keyboard/mouse capture and release, and no blank/debug-colour output for supported opaque cubes.

  Record client+core RSS, steady CPU, texture bytes, resident/visible counts, missing/diagnostic counts, screenshots, exact hashes, and any parity gaps in `docs/phase-2-texture-slice-report.md`.

- [ ] **Step 7: Independent review and commit**

  Generate a review package from the task base through current HEAD. The renderer reviewer must specifically verify: no flat block arrays, exact 8-byte quads, no per-subchunk GPU assets, one texture array/bind group, per-layer mips, bounded adapter checks/uploads, no tracked Mojang payload, and correct fallback accounting.

  After Critical/Important findings are fixed and re-reviewed:

  ```text
  git add app README.md docs/phase-2-texture-slice-report.md
  git commit -m "feat: render vanilla full-cube textures"
  ```

  Expected: branch is review-approved and ready to rebase onto the accepted Phase 0/Phase 1 head; the full Phase 2 gate remains open for models, material layers, animation, tint, lighting, sky, fog, and clouds.
