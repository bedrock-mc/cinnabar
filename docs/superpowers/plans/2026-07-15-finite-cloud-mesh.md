# Finite Vanilla Cloud Mesh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the validated vanilla cloud occupancy texture as one bounded, finite, world-space cloud mesh with visible thickness and physical depth.

**Architecture:** Deterministically greedy-mesh the validated 256×256 alpha mask into eight-byte exposed-face records, upload them once per atmosphere identity, and vertex-pull one periodic mesh through a dedicated custom pipeline instanced 3×3 around the camera. Sky/celestial rendering remains fullscreen; cloud weather, fog, and absolute-time motion move to the finite geometry shader.

**Tech Stack:** Rust, Bevy 0.17 render phases, wgpu/WGSL, `MCBEATM1`, bytemuck, Naga.

## Global Constraints

- Do not commit Mojang assets, generated atmosphere blobs, screenshots, or native renderer binaries.
- Runtime cloud data remains the validated `MCBEATM1` palette of exact RGBA bytes; do not introduce a second asset source or schema.
- Preserve altitude 128, absolute-time +X motion of 0.03 blocks per Bedrock tick, weather response, distance fog, reversed-Z, per-view MSAA/HDR specialization, and identity-stable resources.
- No per-frame mesh rebuild, GPU upload, bind-group creation, per-cell draw, Bevy `Mesh`, or `StandardMaterial`.
- The worst-case mask must be rejected or rendered within an explicit record/byte ceiling before allocation.
- Use strict red-green-refactor TDD for every production behavior.

---

### Task 1: Deterministic periodic cloud mesher

**Files:**
- Create: `crates/render/src/cloud_mesh.rs`
- Modify: `crates/render/src/lib.rs`
- Create: `crates/render/tests/cloud_mesh.rs`

**Interfaces:**
- `pub const CLOUD_MASK_SIZE: u32 = 256;`
- `pub const CLOUD_UNDERSIDE_Y: f32 = 128.0;`
- `pub const CLOUD_TOP_Y: f32 = 132.0;`
- `#[repr(u8)] pub enum CloudFace { Down, Up, North, South, West, East }`
- `#[repr(C)] pub struct PackedCloudQuad { pub bounds: u32, pub face_and_axis: u32 }` with exact size eight and `Pod + Zeroable`.
- `pub fn mesh_cloud_texture(texture: &assets::AtmosphereTexture) -> Result<Box<[PackedCloudQuad]>, CloudMeshError>`.
- `pub fn cloud_instance_origins(camera_xz: [f64; 2], offset_blocks: f64) -> [[f32; 2]; 9]` returns canonical row-major 3×3 period origins.

- [ ] **Step 1: Write failing occupancy, topology, and ABI tests**

Construct synthetic 256×256 RGBA textures in memory. Assert alpha 1 is empty and 255 is occupied; wrong dimensions/role fail; empty emits zero records; one occupied cell emits six faces; two adjacent cells contain no internal face and greedily merge to six rectangular faces; toroidal edge neighbours cull their shared seam; all-filled emits exactly one top and one bottom quad; packed records round-trip exact coordinates/extents/face and remain eight bytes.

- [ ] **Step 2: Run and record red**

Run `cargo test -p render --test cloud_mesh --locked`. Expected: compilation failure because the module and interfaces do not exist.

- [ ] **Step 3: Implement occupancy and unmerged exposed faces**

Validate `AtmosphereRole::Clouds`, 256×256 dimensions, exact RGBA length, and bounded input before allocation. Classify occupancy with `alpha >= 128`; wrap neighbour coordinates with Euclidean modulo. Emit canonical face masks without constructing cubes or a flat expanded world volume.

- [ ] **Step 4: Greedy-merge each face mask**

Use fixed 256-column bit masks and deterministic face/row/column traversal. Merge coplanar rectangles only. Encode `bounds` as `axis0_start | axis1_start << 8 | (axis0_extent - 1) << 16 | (axis1_extent - 1) << 24`; encode only `CloudFace as u32` in bits 0–2 of `face_and_axis` and require the remaining 29 bits to be zero. Top/down axes are X/Z; north/south use an X run plus the fixed four-block Y extent at their Z plane; west/east use a Z run plus the same Y extent at their X plane. Enforce `MAX_CLOUD_QUADS` and `MAX_CLOUD_BYTES` against the checkerboard worst case before producing the boxed result.

- [ ] **Step 5: Add and implement snapped instance-origin tests**

Assert 3×3 origins remain world-anchored across positive/negative coordinates, shift by exactly one period at the snap boundary, preserve fractional time offset modulo 256, and never contain non-finite values.

- [ ] **Step 6: Verify and commit**

Run `cargo test -p render --test cloud_mesh --locked`, `cargo test -p render --locked`, `cargo clippy -p render --all-targets --locked -- -D warnings`, and `cargo fmt --all -- --check`. Commit with `feat: mesh finite periodic clouds`.

### Task 2: Custom finite-cloud GPU pipeline

**Files:**
- Create: `crates/render/src/cloud_render.rs`
- Create: `crates/render/src/cloud.wgsl`
- Modify: `crates/render/src/atmosphere_render.rs`
- Modify: `crates/render/src/atmosphere.wgsl`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/tests/atmosphere.rs`
- Create: `crates/render/tests/cloud_render.rs`

**Interfaces:**
- `CloudGpu` owns the immutable record buffer, record count, prepared atmosphere identity, and identity-cached bind group.
- `CloudPipelineKey { msaa: Msaa, hdr: bool }` specializes the target sample count/format.
- Vertex pulling reconstructs six corners per packed quad from `vertex_index / 6`, applies one of nine period origins from `instance_index`, and adds `cloud_texture_offset * 256` on X.
- The fragment path derives top/side/underside color from `AtmosphereFrame`, then applies the same weather and distance-fog inputs used by the removed plane path.

- [ ] **Step 1: Write failing shader/pipeline/resource tests**

Assert the cloud shader parses and validates after a test `View` substitution; all six faces reconstruct exact fixed bounds; pipeline depth compare is reversed-Z and depth writes are enabled; MSAA/HDR specialize from the view; one immutable storage buffer and one bind group are reused for equal asset identities; identity replacement rebuilds once; one queued cloud item issues one draw with the exact quad and nine-instance counts.

- [ ] **Step 2: Run and record red**

Run `cargo test -p render --test cloud_render --locked` and the cloud-focused atmosphere tests. Expected: failures because the custom cloud pipeline does not exist and the fullscreen shader still samples the cloud texture.

- [ ] **Step 3: Upload the deterministic cloud records once**

During atmosphere asset preparation, call `mesh_cloud_texture` for `AtmosphereRole::Clouds`, create one `STORAGE | COPY_DST` buffer with checked byte count, and cache it by the same 32-byte atmosphere identity. A zero-record mask queues no draw and requires no dummy out-of-range access.

- [ ] **Step 4: Implement vertex pulling and physical depth**

Use a dedicated bind-group layout for the dynamic view uniform, atmosphere uniform, and read-only cloud records. Declare every binding's actual vertex/fragment visibility for Metal validation. Reconstruct world positions without a vertex buffer, project with `view.clip_from_world`, and retain physical depth. Draw `quad_count * 6` vertices and nine instances in one render item.

- [ ] **Step 5: Move weather/fog/face lighting to cloud geometry**

Top faces use the clear/rain cloud color, sides use a fixed lower multiplier, and undersides use the darkest multiplier. Rain and thunder blend toward the existing bounded storm colors. Output alpha is one; distance fog blends color to `fog_color_start.rgb` and reaches full fog before the 3×3 instance edge. Do not fade by sampling alpha-1 empty pixels.

- [ ] **Step 6: Remove the fullscreen cloud plane**

Delete `sample_cloud_layer`, its call, and all `textureSampleLevel(clouds_texture, ...)` use from `atmosphere.wgsl`. Preserve the texture in `MCBEATM1` and CPU preparation because it remains the cloud mesh source. Update binding layouts only where the removed GPU texture is no longer consumed.

- [ ] **Step 7: Verify and commit**

Run `cargo test -p render --locked`, `cargo test -p bedrock-client --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo fmt --all -- --check`. Commit with `feat: render finite depth-aware clouds`.

### Task 3: Release live acceptance and plan evidence

**Files:**
- Modify: `plan.md`
- Modify: the existing Phase 2 acceptance report selected by `scripts/acceptance.ps1`

**Interfaces:**
- Acceptance logs exact client commit, release/debug profile, backend/adapter/driver, atmosphere identity, shader identity, quad/byte counts, per-frame uploads, draw count, and frame/RSS/CPU metrics.
- Screenshots remain temporary and untracked.

- [ ] **Step 1: Run deterministic release acceptance**

Build assets and the stable release client, run BDS/core/client from the approved paths, and collect below/above/within/grazing views at positive and negative period boundaries. Use FIFO first; any no-vsync run is a separately labelled A/B diagnostic.

- [ ] **Step 2: Capture and inspect fresh native images**

Use Windows GDI `CopyFromScreen` to write PNGs under `%TEMP%`, inspect each file, and verify visible top/bottom/side faces, physical terrain occlusion, no periodic seam/pop, no black celestial rectangles, and no fullscreen cloud plane. Do not add the files to git.

- [ ] **Step 3: Check performance/resource gates**

Require stable record/buffer identity, zero steady-state cloud uploads/rebuilds, one cloud draw, no frame spike at 256-block crossings, combined RSS at most 650 MB, steady CPU at most 15%, and full-view teleport remesh at most two seconds. Record actual values without rounding a miss into a pass.

- [ ] **Step 4: Update plan truthfully and commit**

Mark only the finite-cloud acceptance sub-item complete. Leave Phase 2.7 and Phase 2 open if any lighting, visual, platform, or performance gate remains. Commit with `docs: record finite cloud acceptance` and push the reviewed history to `bedrock-mc/cinnabar:phase2-textures`.
