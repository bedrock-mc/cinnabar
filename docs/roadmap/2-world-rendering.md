# Phase 2 — World rendering (textured, lit, real)

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


**Goal:** the spike renderer becomes the real world pipeline. Deliverable: fly through any
live server world and it *looks like Minecraft*.

Scope: block registry + block-state → model/texture mapping (generated export from Dragonfly's registry via `tools/registrygen`, shipped as a binary asset, pinned PMMP BedrockData as the exact protocol-1001 canonical palette/property/biome cross-check, Axolotl Valentine's versioned typed-state approach as a state-selector/catalog reference, and Axolotl's exact pinned PrismarineJS Bedrock collision shapes as reviewed cuboid-template/occlusion inputs—not render/UV authority); vanilla asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version (terrain textures, `blocks.json`, flipbooks, and biome colors) — NOTE: the pinned samples contain no block-render model JSON, so deterministic reviewed family generators combine these sources and vanilla-reference evidence; BDS `resource_packs/vanilla` is server-minimal (blocks.json + texts only), a data reference rather than the texture source; 2D texture array pages + per-layer mipmaps; greedy/culled meshing with transparency layers (opaque/cutout/blend) and per-face culling; **client-side light engine** (block + sky flood-fill, per-vertex light, day/night); biome tinting (grass/foliage/water); sky, fog, clouds; chunk streaming/eviction tied to `ChunkRadiusUpdated` + `SubChunk` request flow; block entities with custom renderers deferred (chests/signs get static models in this phase). Zuri is not a rendering or asset-system input.

**Phase 2 progress (kept current as work lands):**

- [x] **Sequential/hash third-party block identity compatibility.** Runtime
  classification derives canonical air from the validated compiled registry
  instead of the stale protocol bootstrap constant. The checked-in production
  path proves sequential air `13094` and hashed air `0xdbf44120`, rejects
  ambiguous/decoy AIR records, and covers both `WorldStream` modes. This closes
  the all-pink-air failure seen on sequential-ID third-party servers while
  preserving hashed servers.
- [x] **2.1 Local-only vanilla source and deterministic asset pipeline.** Pinned
  `bedrock-samples` provenance, Dragonfly registry export, pack parsing, bounded
  compiler, per-layer mips, versioned runtime blob, and diagnostic fallback are
  implemented; Mojang payloads remain ignored.
- [x] **2.2 Opaque full-cube render path.** Material-aware binary greedy meshing,
  exact eight-byte quads, one shared material buffer/texture array/bind group,
  vertex-pulled repeating UVs, oriented faces, and live asset selection are
  implemented. Two current-HEAD 60-second Windows radius-16 runs passed with
  p99 4.1 ms and zero errors; see `docs/phase-2-texture-slice-report.md`.
- [ ] **2.3 Close the opaque texture slice.** The deterministic named-block BDS
  gallery now passes with all faces/log axes, greedy repetition, mips, supported
  and diagnostic cases recorded, and the clean no-assets full gate passes. The
  fail-closed material path, local relay 1,600-packet ceiling, and deterministic
  inbound/command network arbitration are implemented and independently reviewed.
  A 2026-07-11 interactive radius-16 run at `00b7a32` reached world-ready with
  zero missing mappings, but is diagnostic rather than acceptance evidence:
  849,117 rendered quads used material zero across 9,040 resident/7,093 visible
  subchunks, and exact inspection of blob SHA-256
  `1fbd361c489d3cf90edb49c0056b83ffd9a2a114a36ac1eaf28cfd1103ecf508`
  found only 661 of 16,913 registry visuals
  mapped to real materials. Evidence is in
  `.local/acceptance/20260711T192110Z-16912/app.stdout.log`. Most of that visible gap
  belongs to Tasks 2.4–2.7 (leaves, tint/grass, water/blend, and models). The exact
  two-second teleport/full-view remesh gate and fresh combined RSS/steady-CPU
  evidence remain open; close those findings before completing Task 8.
- [ ] **2.4 Cutout cube materials and leaves.** Preserve independent geometry,
  occlusion, and cave-connectivity semantics; keep the packed subchunk/quad and
  shared GPU architecture. Tasks 1–4/5 are complete at `f768cfa`, `4d23356`,
  `f33b71c`, and `8391a58`: the versioned
  registry now exports independent air, cube-geometry, full-face-occlusion,
  and leaf-model facts with exact pinned counts, and leaf-only cutout materials
  now use coverage-preserving per-layer mips. Palette-native `u64` meshing now
  applies ordered leaf/opaque culling and non-occluder cave connectivity without
  widening the eight-byte quad. The existing single opaque shader now applies
  bit-8 alpha cutout with depth writes and no blending. No Mojang payload is
  tracked. The deterministic live-evidence task remains open.
- [ ] **2.5 Biome palettes and tinting.** `P2.5-NATIVE-BIOME` Decode/store biome data and apply
  grass/foliage/water tint without widening the eight-byte quad record.
  - [x] Palette-native v1001 biome storage/column decoding, including padded
    Bedrock words, `0xff` previous-storage reuse, strict malformed-input
    rejection, atomic inline block+biome commits, and biome-only column
    lifetime independent of all-air block subchunks.
  - [x] Carry request-mode and inline `LevelChunk` biome payloads through the
    Rayon/FIFO streaming path, decode the full dimension column independently
    of requested block count, and commit it before subchunk requests.
  - [x] Retain the live biome definition mapping needed to resolve palette IDs
    to climate and vanilla tint rules, including bounded custom-biome fallback.
  - [x] Remove the grass-block diagnostic fallback: compile bottom/top/side
    independently, preserve grass-side alpha as an opaque tint mask through
    mip generation, and apply the pinned pack's deterministic default grass
    tint until live per-biome color lookup replaces it.
  - [x] Compile grass/foliage/water tint classifications and biome color rules,
    upload palette-native biome/tint tables, and apply them in the chunk shader
    without widening the eight-byte quad record. Grass plus generic/birch/
    evergreen/dry foliage are now resolved from `MCBEAS03`, revision-gated,
    and applied palette-natively. **Complete (2026-07-13):** Task 13's real
    animated water route applies the live palette-native water tint in the
    liquid shader without widening the eight-byte cube quad. Native run
    `20260712T203607Z-7596` proved five runtime water tints, consecutive exact
    GPU witnesses, generation 518 presented, p99 14.0 ms, and zero decode
    errors; fresh assets/render/client focused suites remain green.
  - [ ] Add vanilla-reference biome tint blending for grass, foliage, and
    water. Determine the matching Bedrock radius/kernel from native reference
    evidence, sample the bounded palette-native biome neighbourhood across
    chunk boundaries without flattening columns, preserve special foliage
    rules and custom-biome fallback, and cover abrupt-boundary, missing-neighbour,
    teleport/eviction, GPU, and performance cases.
    **Bounded implementation landed; parity evidence remains open (2026-07-14):**
    grass, generic foliage, and water now share a radius-one horizontal 3x3
    linear-colour box blend across self-contained palette-native neighbour
    snapshots. The descriptor deduplicates equal packed payloads, clamps a
    missing neighbour to the center's nearest edge, records a uniform fast path,
    retains direct birch/evergreen/dry-foliage selection, and validates all nine
    immutable source identities before publication so neighbour replacement,
    eviction, and teleport churn fail stale. Request-mode biome-only commits now
    dirty resident cross-column consumers. The eight-byte cube quad and packed
    Bedrock storages remain unchanged; a uniform record is 52 bytes, or
    1,359,072 bytes for 33x33x24 subchunks at radius 16, while the exact
    descriptor ceiling and the existing GPU arena cap bound adversarial palettes.
    The 3x3 kernel is explicitly provisional because no reviewed native Bedrock
    radius/weight evidence was available; keep this checkbox open until a native
    abrupt-boundary reference fixes or confirms the kernel and the live
    performance/visual gate passes.
    **Committed diagnostic contract complete (2026-07-16):** the provisional
    radius, nine-sample count, and denominator are now one explicit CPU/WGSL
    contract over fixed-size packed records, with cross-chunk diagonal samples
    and exact missing-neighbour edge clamping. Acceptance-only
    `BIOME_BLEND_COMMITTED stage=app_committed` telemetry is sourced from the
    immutable camera-subchunk `ChunkRenderInstance` record after world apply and
    binds its key, chunk generation, tint identity/revision, full-record hash,
    local coordinate, and exact sample set. Unchanged identities are deduplicated
    so stationary runs cannot grow logs. This is deliberately not labeled
    GPU-presented evidence. Independent review and full affected tests/strict
    checks are green through merge `a0f08d9`; the native abrupt-boundary and live
    performance/visual adjudication remain the only completion gate.
- [ ] **2.6 Static/non-cube models, blend/water, and flipbooks.** Complete the
  remaining block visual classes and animation path per
  `docs/superpowers/specs/2026-07-11-phase-2-6-noncube-water-design.md`.
  - [x] Pin and securely acquire the exact local-only PMMP, PrismarineJS,
    Axolotl, and Dragonfly evidence bundle. Whole-bundle atomic publication,
    byte/hash/time bounds, junction rejection, concurrent-winner handling,
    exact license notices, and the no-tracked-payload contract are complete at
    `c44de03`.
  - [x] Preserve complete bounded flipbook metadata and compile the real pinned
    pack's physical frames into deterministic page-aware staging data without
    changing the v3 runtime schema. Commits `143c68d` and `e6e49e1` cover 83
    animations, 1,209 physical frames, 1,323 timeline references, and 1,901
    deduplicated layers on one 2,048-layer page.
    Active pack overlays retain base flipbook source descriptions and material
    destinations in the world sidecar, including frame order, timing, blending,
    and replication. Raster-only replacement and removal have synthetic regression
    coverage; rendered acceptance remains incomplete.
  - [x] Export and strictly decode `BREG1003` typed model/contributor/selectors,
    face coverage, collision seeds, and per-state provenance at `e58d083`.
    PMMP/Dragonfly/Prismarine form a full 16,913-state/1,356-name bijection;
    Valentine is an exact ordered 15,845-state subset with 1,068 attributable
    missing states across 35 wholly absent names and zero extra/mismatched
    states. The deterministic registry SHA-256 is
    `3669be82850824af8592276afe864d903495e743b8af81dfcf1d3aa1586231a4`.
  - [x] Version the bounded runtime asset schema to `MCBEAS04`; compile the
    typed registry selectors, template tables, page-aware flipbook data, and
    attributable per-family diagnostics without committing Mojang payloads.
  - [x] Upload the bounded one/two-page `MCBEAS04` texture resource, immutable
    material/animation/frame tables, and a stable 16-byte animation clock in
    one shared chunk bind group. Commit `a30a0ef` adds page-aware current/next
    frame selection, cross-page interpolation and wraparound, a real diagnostic
    second-page fallback, atomic asset-revision replacement, derivative-safe
    WGSL sampling, and no per-frame texture upload; 82 render tests, strict
    Clippy, and independent spec/quality review are green.
  - [x] Generalize bounded chunk rendering to named cube, model,
    model-lighting, liquid, and liquid-lighting streams while preserving the
    eight-byte greedy cube record. Commit `5734872` adds exact combined byte
    accounting, one consolidated word-addressed geometry arena, transactional
    all-stream allocation/rollback/retry, generation/tint gates, expected/drawn
    presentation masks, and identical direct/MDI addressing. The projected
    vertex storage-binding count including future templates is seven of the
    common minimum eight; 89 render tests, strict Clippy, and independent
    re-review are green.
  - [x] Produce stable eight-byte face-specific model/liquid lighting sidecars
    from a palette-native center-plus-26-neighbour snapshot. Commit `8b5c5a6`
    bakes exact Phase 2.6 block-light 0 / sky-light 15 values plus per-vertex AO,
    registers generation-scoped diagonal AO/liquid dependency masks, and covers
    inline columns, known-air replacement, stale rejection, and conservative
    unknown targets. Render 93/93, world 51/51, client 187/187, strict combined
    Clippy, and independent re-review are green; Phase 2.7 will replace only the
    light inputs, not this format or addressing.
  - [x] Add palette-native multi-layer contributor resolution, retaining the
    eight-byte greedy cube record and adding compact model/liquid streams with
    atomic queue/GPU generation accounting and direct/MDI parity. Task 10 now
    resolves up to 16 packed storage layers without a flat 4,096-block array,
    fails closed on contributor conflicts, and retains simultaneous primary and
    liquid contributors. All three seagrass states and all 26 kelp ages compile
    with exact animated material identities; kelp head/body selection is driven
    only by the primary block above, including across subchunk boundaries. The
    deterministic 29-state BDS water-tank gallery passed from both directions
    at current HEAD with zero target diagnostics/decode errors, p99 15.5 ms,
    377,843,712-byte peak combined RSS, 5.78% mean combined CPU, and native temp
    screenshots confirming real green cutout models. Water geometry remains
    deliberately invisible until Tasks 11–13.
  - [x] Track the exact bounded liquid mesh neighbourhood and invalidation set.
    Task 11 keeps the shared palette-native `world::MeshNeighbourhood` as the
    render API boundary, exposes the deduplicated 23-subchunk liquid sample set
    (current/upper horizontal 3x3 plus lower center and four cardinals),
    applies its checked inverse for liquid-only diagonal dirtying, preserves
    ordinary six-face cube invalidation, coalesces duplicate rapid updates, and
    rejects stale dependency masks. World, client, formatting, strict Clippy,
    and independent review are green; no flat block or whole-column snapshot was
    introduced.
  - [x] Compile crossed cutout plants/crops with exact variants and biome tint;
    compile all physical flipbook frames into texture-array layers and animate
    them from immutable descriptors without per-frame texture uploads. Commit
    `a24370b` covers all 443 terrestrial Cross/Crop states (279 Cross, 164 Crop)
    with zero diagnostics, reusable two-quad templates, compact model refs,
    face-specific lighting, a shared bounded/no-cull direct+MDI GPU path, and an
    exhaustive hash-bound gallery; assets 112, render 102, world 51, client 187,
    acceptance, strict Clippy, and final independent re-review are green.
    Wheat's later farmland reproduction exposed the generic cross as incorrect:
    its eight stages now use four native quarter-position rows with a -1/16 Y
    offset and their original pinned sprites. Compiler and subchunk-boundary
    regressions pass; see `docs/reference/farmland-rendering.md`. This does not
    establish native geometry parity for the other generic Crop families.
  - [x] Mesh animated, biome-tinted water from the shared bounded palette
    snapshot. Task 12 preserves all 16 water depth/falling states, vanilla-like
    weighted four-corner surfaces, diagonal and cross-subchunk influence,
    same-water/solid culling, clipped sides and bottoms, signed flow gradients,
    waterlogging, still/flow per-face materials, stable stream order, and one
    face-specific eight-byte lighting sidecar per 16-byte liquid quad. The
    liquid dependency set was corrected to the exact 23 samples needed by
    lower-cardinal waterfall flow. Only all-face alpha-blended, water-tinted
    families enter this stream; lava remains attributable diagnostic until its
    Task 19 depth-writing route. Seventeen liquid integration tests, full locked
    affected-crate suites, exact strict Clippy, real 16,913-visual asset compile,
    and final independent re-review are green. Water remains deliberately
    invisible until Task 13 installs the transparent GPU path.
  - [x] Mesh animated, biome-tinted water with same-liquid culling, vanilla-like
    corner heights, diagonal invalidation, and a correctly ordered transparent
    phase with depth testing and no depth writes. Its deterministic BDS gallery
    requires one real water tint referenced by the committed/presented liquid
    snapshot (BDS commands cannot assign fixture biomes); a separate end-to-end
    app integration test proves two raw biome IDs with distinct map-water
    colours preserve dense lookup and distinct renderer tint records. Water
    visibility churn and generation-only remeshes retain the last ordered
    snapshot only while every absolute address stays physically resident: same
    key/metadata, active tint, valid lighting, and a same-start liquid range
    containing the old range. Moved/shrunk ranges, eviction, metadata reuse, or
    asset/tint replacement use bounded copy-on-write quarantine: retired spans
    remain drawable and unreusable until an empty-or-nonempty replacement frame
    is submitted and its independent GPU retirement epoch completes. Cap
    exhaustion backpressures the update/removal rather than risking stale reads
    or a blank transparent frame.
    Gallery freezes its 60-second duration/frame metrics at the original
    deadline, then permits at most two unmeasured seconds for a non-empty
    committed=encoded=GPU-presented transparent generation; timeout is a
    logged nonzero failure. Its manifested p99 frame-time gate is exactly
    1000/60 ms.
    **Task 13 complete (2026-07-12):** after fixing bounded GPU-upload
    starvation and four-word liquid-stream arena alignment, exact four-key GPU
    witnesses passed repeatedly across `WaterGalleryFront` and
    `WaterGalleryBack`. Commit `38c1f5d` moved the block-constant packed biome
    tint lookup from the fill-heavy liquid fragment stage to a flat vertex
    varying without changing tint, alpha, ordering, uploads, or lifecycle
    bounds. Native run `20260712T203607Z-7596` froze exactly 60 seconds and
    passed at p99 14.0 ms (limit 16.667 ms), with 38,214 transparent refs,
    five runtime water tints, consecutive exact four-key GPU witnesses,
    request=result=committed=encoded=presented generation 518, zero ceiling
    rejects, zero decode errors, radius 16, 414,187,520-byte peak combined RSS,
    and 6.438% mean combined CPU. Full render/client tests, strict Clippy,
    acceptance tests, and independent re-review through test-hardening commit
    `ba3ea3f` are green with no findings.
    **Camera-motion regression closed (2026-07-13):** exact floating-point view
    keys no longer discard a safe same-address transparent snapshot midway
    through its bounded inactive-slot upload. The staged generation now commits
    atomically before the latest pose is requested, while allocation, asset,
    tint, or stream-address changes still cancel immediately. This prevents
    newly streamed water from starving and appearing or disappearing with
    camera movement (`a4f7da5`; regression-first test, full 278-test render
    suite, strict Clippy/formatting, and diff check green). A fresh native BDS
    camera-motion capture is required below before closing live evidence.
    **Native rerun integration follow-up (2026-07-13):** the first DX12 water
    rerun exposed two independent GPU setup defects introduced by the new
    transparent-model path. The opaque packed-model pipeline now explicitly
    selects `fragment` after `model.wgsl` gained the separate
    `fragment_blend` entry point. Debug DX12 also uses the equivalent direct
    cube/model/depth-liquid draw path because wgpu 27's indirect validator
    expands a 20-byte indexed command to 32 bytes for D3D12 special constants
    while its debug batching assertion still advances by 20; release DX12 and
    unaffected backends retain multi-draw indirect. Both failures have
    regression-first coverage, the full 279-test render suite and strict
    Clippy/formatting are green, and independent review found no blocking
    issues. The corrected native run stayed alive and continuously committed
    transparent snapshots through generation 187 / 31,915 refs instead of
    panicking or starving. It still timed out at the 180-second fixture-ready
    marker while loading 7,934/9,132 debug-path subchunks, and the required
    fresh GDI capture remained pure black under the already-isolated RX 570
    Bevy/wgpu presentation failure, so this is correctness evidence rather
    than visual closure.
  - [ ] Add compact static templates in impact order: slabs/stairs,
    wall-attached vines/lichen/sculk-vein and related thin faces,
    doors/trapdoors, connection-aware panes/fences/gates, then static
    chest/sign models; retain conservative culling/connectivity for partial
    models until exact face-coverage optimization is separately verified.
    - [x] Slab asset templates: all 272 BREG1003 bottom/top/double states now
      compile through opaque six-quad packed model templates with exact
      face-specific materials, UV crops, boundary cull flags, deterministic
      deduplication, and zero pinned-pack slab diagnostics (`c64330b`; assets
      tests, strict Clippy/formatting, and independent review green).
    - [x] Slab packed rendering and occlusion: lower/upper/double slabs remain
      compact model references with six lighting sidecars and no cube stream;
      double slabs provide full-face cave/cull occlusion while partial slabs
      remain conservatively cave-open. Internal and all six cross-subchunk
      model/cube boundaries are covered without lighting reindexing
      (`6df380d`, `09279a1`; 122 asset tests, 44 render-mesh tests, strict
      Clippy/formatting, and independent review green). Gallery acceptance
      remains part of Task 14 before the parent item can close.
    - [x] Stair templates and neighbor-derived straight/inner/outer selection:
      all 512 BREG1003 states across 64 names compile through compact five-shape
      groups per material/upside signature, with exact S/W/N/E transforms,
      both Dragonfly side-isolation guards, same-half matching, all four
      horizontal cross-subchunk boundaries, selected-template lighting, and
      conservative cave connectivity (`859fb13`, `0475516`, `e1732eb`,
      `1766a56`, `469695b`). The exact pinned pack has zero stair diagnostics;
      real-pack assets/render tests, the 43-witness/five-pose deterministic
      gallery, strict MCBEAS04 integrity/tamper gates, full PowerShell dry-run
      acceptance, strict Clippy/formatting, and independent re-review are green.
    - [x] Door and trapdoor templates: all 672 door states and 336 trapdoor
      states compile through compact six-quad alpha-cutout cuboids with exact
      typed open/orientation/hinge/half selection, native door thickness and
      3/16-block trapdoor thickness,
      lower/upper door materials, conservative partial-model culling, and
      deterministic template reuse. Trapdoor templates also retain facing/top-half
      artwork orientation and the top three sprite rows on thin edges, including
      the opened north/south cap texture override. Legacy oak-through-iron door texture arrays
      and modern bamboo/cherry/mangrove/pale-oak/nether/copper/waxed aliases are
      covered. Dragonfly's rotated door-state encoding is inverted before its
      logical-facing/open-hinge transform; an independent review caught and
      corrected the initial direct-orientation interpretation before push.
      Missing or out-of-range selectors fail closed, collision-only seeds are
      not used as render authority, and the real-pack exhaustive gate removes
      exactly 1,008 diagnostics with no additions (`1a69fca`; 145 assets tests,
      strict Clippy/formatting, full 16,913-state ratchet, and independent review
      green). Deterministic gallery/native GPU evidence remains in the shared
      residual-family live gate.
    - [ ] Paired door rendering acceptance: world meshing resolves lower
      direction/open and upper hinge within the same door class, including
      vertical subchunk neighbors; missing pairs emit no door geometry.
      Highlights and mining cracks use the same paired model surfaces; crack
      caches retain the resolved pose so neighbor updates change their geometry.
      Focused regressions cover different stored selectors. Source-bound
      before/after frames, native-server toggle/picking input, directional
      server artwork and final independent review remain incomplete.
    - [ ] Barrel and adjacent trapdoor render parity: barrel facing/open artwork,
      grain direction, material ownership and carried-item rendering have regression
      and rendered coverage. The reported texture leakage was not reproduced;
      trapdoor edge UV and state orientation have compiler regressions across
      registered materials and 16/32 pixel textures. Rendered trapdoor acceptance
      and broader graphics parity remain incomplete.
    - [x] Connected wall templates: all 5,184 states across 32 wall materials
      decode the exact 9-bit north/east/south/west none/short/tall selector and
      center-post bit into deterministic zero-to-30-quad packed models. Visible
      bounds come from the local vanilla `template_wall_post`,
      `template_wall_side`, and `template_wall_side_tall` render models rather
      than Dragonfly/Prismarine collision boxes: post-off states omit the post,
      short arms reach 14/16, tall arms reach full height, and UV projection
      follows the vanilla UV-locked blockstate contract. Invalid selectors fail
      closed, partial-model culling remains conservative, collision-seed removal
      leaves output byte-identical, and the real-pack ratchet removes exactly
      5,184 diagnostics with no additions (`09ba163`; 148 assets tests, strict
      Clippy/formatting, full 16,913-state ratchet, and independent correction
      review green). Deterministic gallery/native GPU evidence remains in the
      shared residual-family live gate.
    - [x] Pressure-plate templates and typed pressed selector: BREG1003 now
      preserves `redstone_signal` only for the 256 pressure-plate states as an
      explicit unpressed/pressed flag, without affecting redstone wire or any
      other record. All 16 material families compile through two deterministic
      opaque templates using the vanilla `pressure_plate_up/down` bounds and
      exact UV crops, including the pressed model's half-texel side strip;
      missing/invalid selectors fail closed and collision data is not render
      authority. The selector-only registry regeneration is byte-reproducible,
      changes exactly those 256 records, and the real-pack ratchet removes all
      256 pressure-plate diagnostics with no additions (`4c83afd`; registry SHA
      `fda4b40335c24b0019049ce572668b03f8ddb9a705de88abb4d724aa7ff81106`,
      152 assets tests, 23 strict-coverage tests with one real-blob gate ignored
      by default, registrygen tests/vet, strict Clippy/formatting, and independent
      correction review green). Deterministic gallery/native GPU evidence
      remains in the shared residual-family live gate.
    - [x] Fence-gate templates and bounded compound model references: all 192
      states across 12 materials require the exact typed `Orientation`, `Open`,
      and in-wall flag mask, fail closed on missing, invalid, or additional
      selectors, and compile from the vanilla render-model oracle rather than
      collision boxes. Closed/open and normal/in-wall forms preserve exact
      UV-locked geometry; bamboo uses its distinct custom 38/40-quad topology
      and reversed/rotated UV rectangles. Because exact gates exceed the
      existing 32-quad mask, one visual now selects a validated pair of
      consecutive 24+16 (bamboo closed 22+16) templates, emitted as two
      independent packed model references with contiguous lighting and draw
      records while preserving the 16-byte reference, `u32` visible mask,
      MCBEAS04 field widths, and GPU shader contract. Encoder/runtime trust
      boundaries reject empty, truncated, nested, incompatible, or directly
      referenced continuations. The production 16,913-state ratchet removes
      exactly 192 gate diagnostics with no additions and now holds 8,301
      diagnostics including air (`f4bcfe0`, `1aaf952`; full assets/render and
      visualcoverage suites, strict Clippy/formatting, real pinned-pack run, and
      independent review/re-review green). Deterministic gallery/native GPU
      evidence remains in the shared residual-family live gate.
    - [x] Connection-aware pane and fence templates plus transparent model
      streaming: all 43 pane/bar states select one of 16 exact post-and-arm
      masks, and all 13 fence states select compact post plus connection-arm
      templates while preserving wood/nether connection classes. Internal and
      all four horizontal cross-subchunk seams suppress only true pane joins;
      fences connect to full occluders, matching fences, and only the sides of
      axis-aligned gates. Mixed connected-template flags fail closed. Alpha
      admission is descriptor-scoped so stained panes retain blend materials
      without accidentally admitting full stained-glass cubes that share the
      same texture path; reviewed beacon and liquid routes remain intact.
      Alpha-blended model quads now reuse the same packed model references and
      lighting sidecars but enter a dedicated no-depth-write phase, sorted
      back-to-front by retained view and face. Sorting runs through a
      latest-wins Rayon worker cache keyed by exact CPU/GPU generation and
      stream identity; water and model uploads share one whole-subchunk,
      per-frame transparent-reference budget. The production 16,913-state
      ratchet removes exactly 56 diagnostics with zero additions and leaves
      8,066 diagnostics including air (`a2c3a5a`, `5024f21`; full assets/render
      suites, strict Clippy/formatting, pinned-pack ratchet, and independent
      review/re-review green). Deterministic gallery/native GPU evidence
      remains in the shared residual-family live gate.
    - [x] Carpet and stateful pale-moss-carpet templates: all 17 ordinary
      stateless carpets compile as exact opaque 1/16-block cuboids with the
      pinned wool/moss aliases, while all 162 pale-moss states enforce the
      exact four `none`/`short`/`tall` side properties and upper-bit contract.
      Pale bases stay opaque; side planes use the pinned two-entry cutout pair
      in its verified tall/short order, render two-sided with conservative
      connectivity, preserve the isolated-upper base-plus-four-tall special
      case, and quantize vanilla's unrepresentable 1.6/256 inset symmetrically
      to 2/254. Missing, invalid, extra, or mismatched typed selectors fail
      closed; collision seeds do not affect rendering. The production ratchet
      removes exactly 179 carpet diagnostics with no additions and now holds
      8,122 diagnostics including air (`8087b6a`, `9323093`, `9e99a5e`; exact
      opposing and direction-specific Java UV corner orders, two byte-identical
      pinned builds, full assets/visualcoverage suites, renderer regressions,
      strict workspace Clippy/formatting, and independent final re-review green).
      Deterministic gallery/native GPU evidence remains in the shared
      residual-family live gate.
    - [x] Button templates and exact wall UV locking: all 168 states across 14
      materials enforce the exact `Orientation` plus pressed-flag mask and
      canonical schema, fail closed on missing/extra/invalid selectors, and map
      Bedrock's six outward-facing values to deterministic floor, ceiling, and
      four wall transforms. Unpressed and pressed forms use the exact vanilla
      bounds and face rectangles, with the unrepresentable 1.02-pixel pressed
      depth deliberately quantized to one pixel. Wall faces derive UV-locked
      rectangles from rotated target bounds; independent literal six-face
      goldens cover all four directions and both pressed states after review
      caught and corrected the initial source-space projection. Materials stay
      opaque, partial models advertise no boundary culling/coverage, and
      collision seeds are not render authority. The production ratchet removes
      exactly 168 button diagnostics with no additions and now holds 7,898
      diagnostics including air after integrating the already-landed 56-state
      pane/fence tranche (`8b427eb`, `fe55779`; deterministic pinned
      builds, full assets/render/visualcoverage suites, strict Clippy/formatting,
      and independent final re-review green). Deterministic gallery/native GPU
      evidence remains in the shared residual-family live gate.
    - [x] Canonical huge-mushroom cube states: all 48 states across brown
      mushroom blocks, red mushroom blocks, and mushroom stems now select the
      pinned pack's exact six-face material aliases from the canonical tagged
      `huge_mushroom_bits` integer. Missing, extra, untagged, mistyped,
      noncanonical, or out-of-range selectors fail closed. The focused
      production-pack gate preserves diagnostics for all 43 legacy flags-zero
      cube records, all 25 stained-glass/copper-grate/slime transparency-family
      cubes, and `minecraft:invisible_bedrock`; record reordering remains
      byte-deterministic. The production ratchet removes exactly 48 intended
      diagnostics with zero additions. After integrating the already-landed
      128-state glow-lichen/sculk-vein tranche, that historical checkpoint held
      7,722 diagnostics including air (full assets/visualcoverage suites, pinned
      compiler tests, strict Clippy/formatting, and zero-delta refreshed ratchet
      green).
    - [x] Ordinary stained-glass cubes: the exact stateless
      `minecraft:black_stained_glass`, `minecraft:blue_stained_glass`,
      `minecraft:brown_stained_glass`, `minecraft:cyan_stained_glass`,
      `minecraft:gray_stained_glass`, `minecraft:green_stained_glass`,
      `minecraft:light_blue_stained_glass`,
      `minecraft:light_gray_stained_glass`, `minecraft:lime_stained_glass`,
      `minecraft:magenta_stained_glass`, `minecraft:orange_stained_glass`,
      `minecraft:pink_stained_glass`, `minecraft:purple_stained_glass`,
      `minecraft:red_stained_glass`, `minecraft:white_stained_glass`, and
      `minecraft:yellow_stained_glass` records now render as alpha-blended
      six-quad models. Palette-native meshing suppresses a shared face only for
      an equal six-face material identity under the checked transparent-cube
      semantic, preserves both cross-colour boundary faces, culls glass behind
      full opaque neighbours without hiding the opaque face, stays cave-open,
      and applies across all six subchunk boundaries. The production ratchet
      removes exactly these 16 IDs with zero additions, leaving 7,706
      diagnostics and 7,235 cumulative removals; the ignored integrated blob is
      SHA-256
      `61025bb3e8e1b9ca0d5e2ec1cd7847433333a20f99948c6193fbb370a0d4900f`.
    - [x] Copper grates: the exact stateless `minecraft:copper_grate`,
      `minecraft:exposed_copper_grate`, `minecraft:weathered_copper_grate`,
      `minecraft:oxidized_copper_grate`, `minecraft:waxed_copper_grate`,
      `minecraft:waxed_exposed_copper_grate`,
      `minecraft:waxed_weathered_copper_grate`, and
      `minecraft:waxed_oxidized_copper_grate` records now use homogeneous
      alpha-cutout six-quad transparent-cube templates. Waxed variants retain
      the exact unwaxed face-material aliases, while shared-face culling uses
      exact network identity so wax and oxidation boundaries remain visible in
      sequential and hashed modes. Grates stay cave-open, route only through
      ordinary depth-writing model draws, cull against identical states across
      all six subchunk boundaries, and preserve opaque-neighbour asymmetry.
      Slime, stained glass, panes, copper bars/bulbs/doors/trapdoors,
      unrelated grate names, legacy flags-zero records, and
      `minecraft:invisible_bedrock` remain outside this admission. The
      production ratchet removes exactly eight IDs with zero additions, leaving
      7,698 diagnostics and 7,243 cumulative removals; the ignored integrated
      blob SHA-256 is
      `20cd1b4301f40736468a3249acf21fdea0544d74fa238d8faae04aaee1af9940`.
    - [x] Chiseled bookshelves: all 256 canonical
      `minecraft:chiseled_bookshelf` states (sequential IDs 1,605–1,860) now
      compile from the exact `books_stored:int 0..63 × direction:int 0..3`
      product into 64 immutable north-facing templates and four opaque source
      materials. Each template has five ordinary full faces plus six coplanar
      front-slot quads; native 1.26.33.1 evidence fixes bit order to top-left,
      top-middle, top-right, bottom-left, bottom-middle, bottom-right and fixes
      directions 0/1/2/3 to south/west/north/east. Exact pair/static terrain
      access, unit collision, flags, face coverage, typed state, ID formula,
      and complete-family cardinality all fail closed. Ordinary and six-quad
      front faces cull across every subchunk boundary, the full model closes
      cave connectivity, and the dense-subchunk fixture emits exactly 1,352
      model refs and 2,816 visible quad refs with stable 11-record lighting
      spans. The exact production ratchet removes only IDs 1,605–1,860 with
      zero additions, leaving 2,570 diagnostics including air. Registry SHA-256
      is `3e0a67718b6368d8b5f7755e9e49a1241233f21bcea8724a9163febb4f1b1d92`;
      the ignored compiled pack SHA-256 is
      `df82f3408ee5805bcd536a484b6d0e8831eb972d76225c17eda005695e4d982c`.
      - [ ] Live presentation acceptance: capture Cinnabar from the matching
        native-gallery viewpoints and require two consecutive exact
        GPU-completed model-stream witnesses. Keep both native and Cinnabar
        screenshots local-only; do not commit Mojang-derived imagery.
    - [x] Resin clumps: all 64 canonical `minecraft:resin_clump` states
      (sequential IDs 2,930–2,993) now require the exact typed
      `multi_face_direction_bits:int 0..63` product, formula IDs, empty flags and
      face coverage, empty collision, and the exact scalar/static
      `resin_clump` texture route. Native 1.26.33.1 support-removal/readback
      fixes bits 1/2/4/8/16/32 to down/up/south/west/north/east, matches the
      glow-lichen UV projection, and proves that a written zero mask reloads as
      63. The compiler preserves every protocol record while aliasing mask 0 to
      mask 63, emitting one static alpha-cutout material, 63 templates, and 192
      quads. Sequential and hashed mesh gates cover every mask, all six
      boundaries, cave openness, opaque-support visibility, layered water, and
      the dense 4,096-reference/24,576-draw-light bound. The exact production
      ratchet removes only IDs 2,930–2,993 with zero additions, leaving 2,506
      diagnostics including air. Registry SHA-256 is
      `33a31ec89a04fe638a4f59ab315561c1c0d897e04f2041d5643262d3de56d30c`;
      the ignored compiled pack SHA-256 is
      `91998c61a9f8c40a72e73e45167d7448e9ad18271b561bc61f8d839584603e19`.
      - [ ] Live presentation acceptance: reproduce the native resin viewpoints
        in Cinnabar and require two consecutive exact GPU-completed model-stream
        witnesses. Native and Cinnabar screenshots remain local-only.
    - [x] Reviewed selector-alias opaque cubes: validate all 38 records in the
      complete hay, bone, quartz-block, smooth-quartz, chiseled-quartz, purpur,
      and TNT products, then promote exactly 27 compatibility states. Exact
      typed wrappers, raw keys, values, formula IDs, Primary/Cube facts, shape 1
      CollisionOnly unit bounds, complete-product cardinality, and exact
      static/non-tinted/opaque vanilla pack descriptors fail closed. Native
      1.26.33.1 evidence preserves Y caps down/up, X caps west/east, Z caps
      north/south, and a quarter-turn on all four non-cap faces for X/Z;
      `deprecated=0..3` and `explode_bit=0..1` are static visual aliases.
      Sequential/hash rendering covers every state, all six cross-subchunk
      culls, dense six-quad greedy output, cave closure, and zero model,
      transparent, and liquid streams. The exact production ratchet removes
      only IDs 2,908-2,910, 2,912-2,914, 2,916-2,918, 5,443-5,444,
      6,466-6,468, 6,470-6,472, 6,474-6,476, 7,082-7,083, 13,113,
      14,686-14,687, and 15,345-15,346 with zero additions, leaving 2,479
      diagnostics including air. Registry SHA-256 is
      `9f67a14d73cf958b53557cc31c601168aa0eb95c5d46dfac1299f8412a0cb74f`;
      ignored compiled-pack SHA-256 is
      `18a4718d6fd03a66c0eb30e0a28444dcf80159c658cf4f7712e5ff342f7740ca`.
      - [ ] Live presentation acceptance: reproduce the matching native axis,
        TNT, and deprecated-state viewpoints and require two consecutive exact
        GPU-completed cube-stream witnesses. Screenshots remain local-only.
    - [x] Exact cactus family implementation: all 16 canonical
      `minecraft:cactus` states (sequential IDs 13,606-13,621) now require the
      complete exact `age:int 0..15` product, formula IDs, Primary/Cuboid
      ownership, empty flags and face coverage, exact shape-84 collision, and
      exact static side/down/up pack routes. Native 1.26.33.1 evidence fixes the
      visible X/Z inset to 1/16, full Y height, and side UV crop to source
      columns 1..14. Every age reuses one six-quad template and three static
      alpha-cutout materials. Sequential/hash, all-boundary, opaque-adjacency,
      cave-open, additional-water, dense 4,096-reference/24,576-draw-light,
      deterministic-registry, deterministic-pack, and exact 2,479 -> 2,463
      visual-coverage gates are green. No Mojang payload or screenshot is
      tracked.
      - [ ] Live presentation acceptance: reproduce the matching native cactus
        overview, stack, grazing, and top-inset viewpoints in Cinnabar and
        require two consecutive exact GPU-completed model-stream witnesses with
        stable generation/ref counts and zero contamination counters.
    - [x] Exact cake family implementation: all seven canonical
      `minecraft:cake` states (sequential IDs 14,055-14,061) now require the
      complete exact `bite_counter:int 0..6` product, formula IDs,
      Primary/Cuboid ownership, empty flags/coverage, exact collision shapes
      89-95, exact six-face block routing, and literal untinted terrain pairs.
      Native 1.26.33.1 evidence fixes west as the advancing cut plane and binds
      bite zero to `cake_side` versus bites one through six to `cake_inner`.
      Seven immutable six-quad templates use
      `[16+32*b,0,16]..[240,128,240]`; sequential/hash, all-boundary,
      opaque-adjacency, cave-open, additional-water, dense
      4,096-reference/24,576-draw-light, deterministic-registry,
      deterministic-pack, and exact 2,463 -> 2,456 visual-coverage gates are
      green. No Mojang payload or screenshot is tracked.
      - [ ] Live presentation acceptance: reproduce representative bite 0, 1,
        and 6 native viewpoints in Cinnabar and require two consecutive exact
        GPU-completed model-stream witnesses with stable generation/ref counts
        and zero contamination counters.
    - [x] Candle-cake composite implementation: all seventeen colors and both
      typed `lit:byte` states retain an unbitten cake, centered six-pixel candle,
      and crossed wick. The corresponding candle block supplies its color and
      lit texture, including static texture aliases. Carrier regressions cover
      sequential/hash lookup and reject malformed selectors or collision data.
      - [ ] Full presentation parity: ambient flame particles and matching native
        world captures remain unverified; isolated GPU model captures establish
        the composite geometry and texture routing only.
    - [x] Exact farmland family implementation: all eight canonical
      `minecraft:farmland` states now require the
      complete exact `moisturized_amount:int 0..7` product and unique identities
      from the target registry (see `docs/reference/farmland-rendering.md`),
      Primary/Cuboid ownership, empty flags/coverage, exact shape-43 collision,
      and literal untinted side/top routes. Native 1.26.33.1 evidence binds
      amount zero to dry terrain-array index 1 and amounts one through seven to
      wet index 0. Two immutable six-quad templates use full X/Z and 15/16 Y;
      sequential/hash, all-boundary, opaque-adjacency, cave-open,
      additional-water, uniform/mixed dense 4,096-reference/24,576-draw-light,
      deterministic-registry, deterministic-pack, and exact 2,456 -> 2,448
      visual-coverage gates are green. No Mojang payload or screenshot is
      tracked.
      - [ ] Live presentation acceptance: reproduce representative moisture 0,
        1, and 7 native viewpoints in Cinnabar and require two consecutive
        exact GPU-completed model-stream witnesses with stable generation/ref
        counts and zero contamination counters.
        The October 4 macOS Metal manual check confirms that farmland and the
        corrected wheat model render correctly; the user approved this fix.
    - [x] Common farmland crop rows: wheat, carrots, potatoes and beetroot use
      four axis-aligned, two-sided planes lowered to the farmland surface.
      All eight growth states preserve their selected artwork and upright UVs
      in sequential and hashed lookup; ordinary flowers retain crossed planes.
    - [x] Remaining farmland crop templates: fruit stems retain
      growth height, stage colour and a facing-oriented connector. Torchflowers
      preserve their two crop sprites and flower sprite at the plant width.
      Pitcher crops retain buried pods, distinct lower/upper flower stages and
      invisible early upper halves. Both block-ID modes cover every admitted state.
      Sweet berry bushes select each growth stage's face artwork, preserve the
      mature sprite and use the narrower plant geometry.
      - [ ] Full crop presentation parity: exact native-client comparison,
        sub-unit sprite geometry, atlas-edge UV calibration and torchflower
        positional variation remain open.
    - [ ] Nether wart geometry and selectors remain incomplete independently
      of the farmland crop families.
    - [ ] Slab/stair native and packed-GPU live acceptance: capture all five
      fixed Cinnabar poses through native `%TEMP%` screenshots and require two
      consecutive exact GPU-completed model-stream witnesses. Automated gallery
      construction is complete. The Top pose now reaches two consecutive exact
      7-key GPU witnesses with 276 model references and zero missing, stale,
      wrong-stream, zero-reference, or draw-mismatch counters after moving the
      camera teleport ahead of the synthetic fixture-update flood. The five
      inspectable native captures and a clean performance-gate run remain open;
      the first repaired live run was rejected at 138.3439 ms mutation-to-visible
      against the 100 ms gate. Audit found that the already-complete exact model
      witness remained armed throughout the later timed session, rebuilding a
      full frame probe over thousands of instances every frame. The probe now
      disarms immediately after its exact two-frame pair, and gallery publication
      now waits for an exact Rust-side committed-camera marker before sending the
      fixture-update flood. Unit, full-workspace, acceptance dry-run, and runtime
      safety regressions are green; a fresh native five-pose rerun remains open.
      North run `20260714T011758Z-840` then passed its exact camera fence,
      77-command result fence, and consecutive GPU model witnesses (sequences
      907/908, seven keys, 277 refs, all contamination counters zero), but the
      timed gate still failed at p50 41.7 ms / p99 47.6 ms and 140.1161 ms
      mutation-to-visible. Its 55-second camera delay was a bounded Rust ingress
      bottleneck (four queued packets and eight admissions per rendered frame),
      not relay reordering; the channel and per-frame admission window are now
      coherently 32, matching the existing heavy-event cap while preserving FIFO
      order and decode/mesh worker budgets. GPU cost and fresh native visual
      evidence remain open. The first GPU-side correction now rejects all padded
      and neighbour-masked slots in the model vertex stage before template,
      lighting, texture, tint, and fragment work; the fixed 32-quad/reference
      storage contract remains bounded while a live A/B measures the reduction.
      Optimized North run `20260714T013915Z-6480` reduced teleport
      acknowledgement-to-ingress/commit to 8.18 seconds (sequence 2,128, with
      ingress and commit in the same update) and again passed the exact model
      witness. Vertex culling improved p50 from 41.7 to 39.6 ms despite 8,699
      versus 5,679 resident subchunks, but p99 remained 47.7 ms and the 100 ms
      mutation gate still failed at 139.9718 ms. Structural exact-count model
      drawing is now complete in `fcb1989` and ownership-hardening `b07e194`:
      one exact eight-byte visible-quad indirection record replaces the fixed
      32-quad vertex launch while preserving 16-byte model refs, ordered
      lighting, one direct/MDI command per subchunk, arena/COW bounds, and model
      witness semantics. Full render/client tests, strict Clippy/format/shader
      validation, release build, and independent review/re-review are green.
      Live VineGallery run `20260714T030538Z-22388` passed exact GPU witnesses
      at sequences 191/192 (four keys, 95 stable refs, all contamination
      counters zero) but measured p50 40.6 ms, p99 47.7 ms, and 142.6 ms
      mutation-to-visible with 8,345 resident subchunks—neutral versus the prior
      p50 40.3 / p99 47.8 / 138.9454 ms run. Exact drawing therefore closes the
      required packed per-quad architecture but not the performance gate; GPU
      stage timestamps/workload counters must identify the remaining cost before
      considering a one-sided/two-sided pipeline split. Resident and
      frustum-visible model workload counters are now implemented: acceptance
      JSON distinguishes 16-byte model refs from exact eight-byte quad draw refs
      and reports the former fixed 32-quad slot invocations avoided. Full
      render/client suites are green. Acceptance/profiling runs now also enable
      Bevy's asynchronous DX12 timestamp recorder and report paired, deduplicated
      p50/p95/p99/max timings for the chunk-containing opaque and transparent 3D
      passes without blocking the GPU. Live VineGallery North run
      `20260714T032404Z-12360` recorded 1,296 GPU samples: combined opaque plus
      transparent was 4.9 ms p50 / 10.2 ms p99 (10.54048 ms max), while full
      frame time remained 40.2 ms p50 / 47.6 ms p99. Its 29,083 visible model
      refs issued 80,233 exact quad draws and avoided 850,423 of the former
      930,656 fixed-slot invocations (91.38%); resident totals were 63,327 refs,
      161,125 draws, and 1,865,339 avoided invocations. The exact-draw path is
      therefore effective and model shader work is not the remaining frame-time
      bottleneck; do not add a speculative one/two-sided model pipeline split.
      The run again passed adjacent exact GPU witnesses (sequences 801/802,
      four keys, 92 refs, zero contamination), stayed within the RSS/CPU budget
      at 638,537,728 bytes and 2.82% mean CPU, and failed only the shared 100 ms
      mutation gate at 142.5286 ms. The next performance investigation must
      target frame scheduling/presentation and mutation-to-frame latency.
      Two process-scoped pacing A/Bs on the same approved DX12 runtime ruled
      out unsafe workarounds: AutoNoVsync run `20260714T033621Z-14584` never
      reached the world-ready/mutation fence, while FIFO with
      `WGPU_DX12_USE_FRAME_LATENCY_WAITABLE_OBJECT=dontwait` in run
      `20260714T034035Z-22400` reached the clean gallery/camera fence but never
      produced a GPU-completed model witness and resisted graceful shutdown.
      Keep wgpu's default waitable-object behavior for correctness; surface
      acquisition/presentation remains the evidence-backed external blocker.
      The backend/presentation investigation is now
      conclusive: five direct swapchain captures from Cinnabar, minimal Bevy
      Camera3d and Camera2d clear-only probes, a camera-local red clear, and
      DX12/FXC were byte-identical pure black. Vulkan exposes no surface present
      modes and GL exposes no adapter on this machine. This isolates the native
      black-window symptom below Cinnabar to Bevy 0.18.1/wgpu DX12 on the RX 570
      driver `31.0.21924.61`; chunk, camera, shader, and custom render-phase code
      must not be changed to mask it. A driver or isolated Bevy/wgpu A/B plus a
      tiny startup clear-color smoke gate remains required before native visual
      evidence can close, while deterministic GPU witnesses can continue.
      **Visibility investigation checkpoint (2026-07-15; telemetry only):**
      acceptance/metrics runs now publish one coherent, fixed-size diagnostic
      snapshot for each rendered frame. It fingerprints the extracted camera
      identity, pose, and frustum with monotonic pose/view generations; records
      resident-mesh, cave-visible, Bevy-frustum-visible opaque, and actually
      submitted opaque Direct/MDI key counts plus deterministic hashes; and
      reports the three adjacent stage-loss count/hash deltas. Collection is
      disabled outside the existing acceptance/metrics diagnostics path, emits
      at most one aggregate marker per second, retains no per-key history, and
      caps the transient submitted-key set at 65,536 entries; overflow marks
      the submitted digest and its adjacent loss unavailable instead of
      publishing a truncated prefix as exact. Deterministic/empty/mutation,
      generation-coherence, Direct/MDI-parity, and bound tests are present. This
      checkpoint changes no culling, meshing, shader, draw-order, or presentation
      behavior and does not establish a visibility fix. A fresh affected live
      run must still capture consecutive `CINNABAR_VISIBILITY_SNAPSHOT`
      markers spanning the symptom and identify the first nonzero adjacent loss
      before any repair is proposed.
      **Live diagnostic-geometry attribution (2026-07-16):** every actually
      emitted diagnostic cube quad now carries its exact raw network identity
      and resolved protocol-1001 sequential identity through bounded meshing,
      resident replacement/eviction, JSON, `WORLD_READY`, and revision-gated
      live telemetry. Per-mesh/global top sets are deterministic and bounded;
      omitted contributions remain exactly reversible under capacity churn;
      identical remeshes do not resort or relog unchanged data. Independent
      review and post-merge meshing/app/assets tests are green through
      `eda5ba9`. A native BDS run then proved the dominant visible magenta
      geometry is `minecraft:sulfur` (14658/`0x2d658dd8`) and
      `minecraft:cinnabar` (12638/`0xbda02665`), with `minecraft:leaf_litter`
      far behind, replacing screenshot-based family guesses with exact live
      evidence.
      **Sulfur/cinnabar exact route complete (2026-07-16):** both single-state
      minerals now compile as exact opaque full cubes for sequential and hashed
      protocol identities. Admission proves the reviewed collision, selector,
      sound, static texture, tint, flipbook, alias, and extension-metadata
      boundaries and rejects the pair atomically on any mismatch. Independent
      review, the ignored pinned-pack witness, full affected suites, and strict
      checks are green through the `6aeb8c8` merge. Production visual diagnostics
      fell exactly from 2,400 to 2,398 with zero additions. **Native gate passed
      on `3bdb317`:** fresh ignored carriers joined BDS and both minerals were
      absent from the day and forced-night diagnostic top sets, replacing their
      former ~328k/~322k quad dominance with leaf-litter states at roughly 2.5k
      quads each. A residual magenta rectangle and remaining 2,398 compile-time
      diagnostics keep the broader zero-magenta Phase 2.6 gate open.
      **Fixed-pose diagnostic attempt (2026-07-15):** run
      `20260715T164233Z-14908` disabled auto-fly and retained one stable render
      view generation, but the client never emitted its mutation/world-ready
      marker, so the harness timed out before publishing and facing the requested
      `Front` fixture. The resulting 62 stable-pose snapshots therefore do not
      constitute the required aimed-camera reproduction. They still narrow the
      pipeline: 61/62 had exact frustum-visible/submitted counts and hashes; the
      sole mismatch was the final snapshot taken as the failed harness entered
      cleanup. No culling repair is authorized by this evidence. The next probe
      must establish a fixture-facing camera independently of the world-ready
      marker and reproduce the user's visible disappearance before changing
      cave, frustum, or submission behavior.
    - [ ] Wall-attached vine family: replace the diagnostic pink-cube fallback
      for every `minecraft:vine` direction-bit state with compact cutout face
      templates selected from its exact attachment mask, including conservative
      cave connectivity, cross-subchunk neighbours, texture/UV parity, and a
      deterministic gallery plus native screenshot/GPU evidence. Extend the
      same reviewed thin-face route to glow lichen and sculk vein separately;
      do not collapse their distinct state/property contracts into vine logic.
      **Implementation complete; live gate open (2026-07-13):** all 16 masks
      compile to foliage-tinted two-sided attachment planes with exact UV axes,
      zero diagnostics, zero-mask no-draw behavior, and all-mask/all-boundary
      CPU mesh coverage (`ff7066b`; focused Go/assets/render tests and two
      independent reviews green). Deterministic acceptance is now complete in
      `489af26` and `748438c`: five canonical poses bind the exact 0..15 mask
      bijection and compiled-asset hashes, build isolated direction-exact stone
      supports, preserve mask 0 as zero-draw, fence the committed camera ahead
      of publication, and require two adjacent GPU-completed markers over the
      exact requested subchunks with stable generation, manifest, nonzero model
      reference total, and zero missing/stale/wrong-stream/zero-ref/draw-mismatch
      counters. Live evidence proved the total is subchunk-wide rather than one
      reference per central fixture (94 refs in run `20260714T023010Z-7488`), so
      the invalid interim 15/43 equality was removed in `eec96e2`; review then
      withdrew that recommendation and passed the corrected contract. The run
      produced adjacent exact witnesses at sequences 461/462, four keys, 94
      stable refs, and all contamination counters zero. Its only failure was the
      shared model-performance gate: 138.9454 ms mutation-to-visible against
      100 ms, p50 40.3 ms, p99 47.8 ms, with 8,595 resident subchunks. Combined
      RSS peaked at 532,107,264 bytes and mean CPU was 2.52%, within resource
      budgets. Native captures and the structural exact-count model-draw
      optimization remain required before this item closes; native capture is
      separately blocked by the confirmed RX 570 Bevy/wgpu presentation failure
      above. Exact visible-quad drawing is complete but was performance-neutral;
      GPU-stage measurement and the shared 100 ms gate remain open.
      **Glow-lichen/sculk-vein implementation complete (2026-07-13):** the
      remaining vine-like pink blocks were not `minecraft:vine`; they were all
      64 `minecraft:glow_lichen` and 64 `minecraft:sculk_vein` states still
      classified as unknown. Distinct registry families now preserve their
      different six-bit face orders, render mask zero with the vanilla all-six
      fallback, and compile exact 1/256-inset two-sided cutout planes with no
      occlusion coverage. Sculk vein additionally binds its pinned four-frame,
      20-tick flipbook. Exhaustive 128-state selector/geometry/UV/material
      tests, the real pinned-pack compiler, registrygen, runtime rendering,
      strict visual-coverage ratchet, Clippy, formatting, and independent
      review are green; the combined 16,913-state report has zero
      glow-lichen/sculk-vein diagnostics and 7,770 diagnostics remaining
      overall (`a70d3c6`). Native screenshot closure remains part of the shared
      RX 570 presentation gate above.
    - [x] Exhaustive vanilla visual-coverage ratchet: inventory every one of
      the 16,913 protocol-1001 canonical states through the production registry
      and runtime decoders, bind the exact registry/asset hashes, and reject any
      newly diagnostic, newly provisional-fallback, or unjustifiably invisible
      state. Diagnostic/fallback shrinkage is allowed while residual families are
      implemented; the final gate requires zero diagnostic and zero provisional
      fallback non-air states. The accepted design is recorded in
      `docs/superpowers/specs/2026-07-13-exhaustive-vanilla-coverage-design.md`.
      **Complete (2026-07-13):** `visualcoverage` uses the production decoders,
      enforces the exact 1,356-name/16,913-state/one-air protocol corpus and
      exact hash-to-sequential bijection, bounds all inputs, rejects diagnostic
      regression/invisible laundering, and writes deterministic hash-bound
      reports (`b131247`; 11 tests, strict Clippy, real-pack run, and independent
      review green). The reviewed baseline was refreshed cumulatively for the
      already-landed door, trapdoor, wall, pressure-plate, fence-gate, pane,
      fence, carpet, button, huge-mushroom, glow-lichen, sculk-vein, exact
      ordinary stained-glass, exact copper-grate, static-sign,
      chiseled-bookshelf, resin-clump, selector-alias opaque-cube, and exact
      cactus tranches.
      After lava, vine, and those connected/static/multiface/glass/grate
      families plus the exact chiseled-bookshelf, resin-clump, selector-alias
      opaque-cube, cactus, cake, farmland, and exact bee-housing tranches, the
      reviewed baseline held 2,398 diagnostics including one air state.
      **Provisional zero-pink cutover (2026-08-02; incomplete):** the compiler
      now maps the exact 2,397 non-air baseline identities to bounded neutral
      geometry and pinned-pack textures where safely resolvable, otherwise
      canonical stone. The identity table is bound to each network hash plus a
      canonical-state fingerprint, so unknown/custom server blocks remain
      diagnostic. `VisualSupport` preserves this distinction through MCBEAS06
      and runtime decode; visualcoverage v2 reports zero diagnostics and 2,397
      provisional fallbacks across 487 names instead of laundering them as exact.
      The source inventory and hashes are pinned in
      `assets/vanilla-fallback-source-v1001.json`. This removes pink vanilla
      blocks but closes no vanilla parity acceptance gate.
  - [ ] Complete the exhaustive residual-family report, continuing from the
    completed lava/flowing-lava depth-writing non-water-liquid pipeline, so
    every non-air one of the 16,913 canonical states has an exact or otherwise
    authoritative accepted visual; provisional fallbacks do not satisfy this
    requirement. Close deterministic galleries and live acceptance with
    globally zero diagnostic and zero provisional-fallback counters,
    vanilla-reference screenshots, upload/memory/CPU
    metrics, and teleport-remesh evidence.
    - [x] Torch, ladder, rail, tulip, golden-dandelion, and coral-plant exact
      routes (97 states) supersede their envelope inventory entries; leaves
      render Fancy. Wall-torch pivot, ladder/rail offsets, and rail curve
      sprite orientation need native measurement; the coverage baseline must
      be regenerated. Residual names: `docs/phase-2-family-inventory.md`.
      Torch lighting follows the native torch tessellation: one light value
      from the torch's own cell for every plane, no ambient occlusion, and no
      face dimming.
    - [ ] Coral fans compile spreading floor sprites and four wall mounting
      directions with replacement-pack artwork. Native-server placement and
      rendered parity acceptance remain incomplete.
    - [ ] Tripwire hooks compile separate mounts, pitched shafts, eyes and attached
      string in every facing and signal state. Native-server placement and
      rendered parity acceptance remain incomplete.
    - [x] Round two: night light floors lowered to the night sky transfer,
      biome tints blend on a 4-block lattice (four taps per axis; vertical axis
      unblended, needs measurement), chains route through crossed link planes.
      Remaining families and notes: `docs/phase-2-family-inventory.md`.
    - [x] Lava implementation: all 32 `minecraft:lava` and
      `minecraft:flowing_lava` depth states compile through the animated liquid
      mesher without water tint or alpha blending, use an immutable packed route
      bit, retain the O(n) transparent-water/depth-lava partition, and draw in a
      separate opaque depth-writing direct/MDI pipeline. Mixed interfaces and all
      six cross-subchunk boundaries are covered. Full assets/render suites,
      strict Clippy/formatting, the real 16,913-state ratchet, and independent
      review are green; deterministic native gallery/GPU/performance evidence
      remains part of the residual-family live gate.
    - [x] Exact bee nest/beehive cubes: all 48 canonical states across
      `minecraft:bee_nest` and `minecraft:beehive` now preserve the typed
      direction 0..3 by honey-level 0..5 product, full-cube collision and
      occlusion, and the compact packed cube stream. Protocol direction maps
      the front south/west/north/east; only honey level 5 selects the honeyed
      front. The compiler requires the pinned six-face block maps, singleton
      static terrain arrays, exact two-entry front arrays, literal paths, and
      no tint/extra/flipbook metadata. Both network-ID modes, all states, dense
      greedy closure, and all six cross-subchunk boundaries are covered. The
      full real-pack ratchet removes exactly IDs 10,395..10,418 and
      12,495..12,518 with zero additions, shrinking 2,448 -> 2,400 diagnostics.
      Bee occupants remain a separately reviewed block-entity concern rather
      than block-state geometry.
    - [x] Compile recessed models for all thirteen wood shelves, including
      Poplar Shelf. Typed direction, power, and shelf-type selectors choose
      the pose and one of five front regions; malformed selectors remain
      diagnostic. Original texture aliases and sheets with transparent texels
      work through ordinary pack routing with cutout materials.
      Active-registry tests cover every wood and all Poplar Shelf states,
      carrier round trips, both network-ID modes, and invalid selector types.
    - [ ] Complete shelf visual parity with a full native comparison gallery.
      The offline GPU fixture verifies compiled geometry and texture regions
      through the production model shader. Complete pack pixel fidelity and
      live server-pack comparisons remain incomplete.
    - [ ] Close static sign visual parity and its deterministic native gallery.
      - [x] Eliminate all 4,872 standing, wall, and hanging-sign diagnostics
        with typed, order-independent selectors; exact 16-way rotation, six-way
        facing, attachment, and hanging matrices; pinned terrain aliases; and
        bounded static model templates. All-six-facing tests caught and fixed
        reversed wall/wall-hanging support placement. The classic raw 24x12
        board plus 2/3 render transform establishes the 16x8 world silhouette,
        and sign text remains a block-entity deferral (`5987ed6`, `fba9e2e`).
      - [ ] Source-adjudicate exact board thickness, standing-post dimensions,
        and hanging board/support cuboids, then close UV/native-reference and
        GPU evidence. The pinned Bedrock sample pack exposes sign terrain aliases
        but no authoritative sign geometry, so the current shapes must not be
        described as final 1:1 geometry until this evidence is recorded.
    - [ ] Run all 67 exact-state GPU gallery pages (256 targets per logical page,
      with one final 17-state page), require exact palette readback plus two
      consecutive GPU-completed frames for every canonical target, and inspect
      fresh native `%TEMP%` screenshots. Family-specific support/neighbour
      fixtures do not count toward the 16,913 target inventory. The reviewed
      implementation order starts with a deterministic BREG/MCBEAS/hash-bound
      logical page inventory, then adds exact app-side palette witnesses,
      per-target GPU evidence, family-aware placement, and native captures; the
      logical inventory is independent of the RX 570 presentation blocker.
      - [x] Compile the exact logical target inventory: all 16,913 sequential
        IDs are assigned once and in order to 66 full 256-target pages plus a
        final 17-target page. The deterministic artifact binds BREG, MCBEAS, and
        coverage-baseline hashes, retains per-target diagnostic/drawable/
        invisible status, and is deliberately non-accepting until both the
        diagnostic count reaches zero and the strict semantic render-route gate
        passes. The CLI writes atomically and preserves an existing output on
        failure (`500c4af`, `59f692c`; full tests, pinned real-pack check,
        strict Clippy/formatting, and independent re-review green).
    - [ ] Generate a separate version-pinned block-entity inventory and reviewed
      renderer manifest. Prove chunk-NBT and live-update handling, required NBT
      variants, and GPU/no-draw evidence for every source ID; block entities are
      not folded into the canonical block-state count. The ingestion audit found
      21 explicit Dragonfly source IDs plus an id-less Note producer that must be
      explicitly adjudicated. Packet-56 and chunk/subchunk-tail NBT now reaches
      the bounded sparse world store; per-ID renderer evidence remains open. The
      bounded implementation order is
      NetworkLittleEndian NBT prefix decoding plus atomic sparse storage first,
      then a separate deterministic source/renderer-manifest generator and
      strict join, followed by per-ID GPU/no-draw witnesses.
      - [x] Implement bounded NetworkLittleEndian NBT prefix decoding and
        atomic sparse ingestion for inline and request-mode LevelChunk tails,
        successful SubChunk tails, and packet-56 live updates. Exact NBT bytes,
        optional source IDs, and absolute positions remain sparse and
        palette-native; strict byte/depth/collection/entity limits, scope and
        duplicate checks, vanilla dimension Y validation, per-chunk cumulative
        record/raw-byte caps across tail and live updates, FIFO worker decoding,
        malformed-update retention, all-air cleanup, and chunk eviction are
        covered without flat block expansion. The separate deterministic
        inventory/renderer manifest is now generated below; per-ID renderer
        and GPU/no-draw witnesses remain open.
      - [x] Generate and commit the independent protocol-1001 block-entity
        inventory plus reviewed renderer manifest from the exact Dragonfly
        `b85c56ffea6b306798a935f14cc941c76618be52` registration pin and the
        hash-bound BDS 1.26.32.2 runtime evidence. The isolated generator
        rejects workspace dependency drift, generic `unknownBlock` NBT
        passthrough, source/hash drift, missing/extra/duplicate source keys,
        ambiguous aliases, and missing chunk/live-update/variant declarations.
        Its deterministic artifact covers all 21 explicit Dragonfly NBT IDs
        plus the id-less `Note` producer, 63 backing block names, 446 canonical
        backing states, and 42 required NBT variants without changing the
        16,913 block-state count. The strict-final report truthfully remains
        red at 0 proven/22 deferred: renderer implementations, gallery builders,
        per-variant witnesses, and per-ID GPU or no-draw evidence remain open.
      - [x] Add the hash-bound runtime adjudication and lifecycle foundation for
        the reviewed block-entity catalog. Barrel, BlastFurnace, Furnace, and
        Smoker reuse their exact existing cube state; Jukebox and the exact
        id-less Note candidate route to logical no-additional-draw outcomes;
        every reviewed deferred ID requires its exact canonical backing; all
        mismatched/unknown sequential or hashed states fail closed with
        non-conflating route digests. Inline LevelChunk, request-mode
        LevelChunk, and packet-56 NBT-only replacements preserve block, light,
        mesh, connectivity, and render generations when backing/biomes are
        unchanged, while changed backing/biomes, eviction, dimension reset,
        and session reset retain destructive lifecycle behavior. Full affected
        Rust/Go suites, strict Clippy/formatting, and independent re-review are
        green through `558de41`. This is logical routing evidence only: the
        strict-final artifact remains 0 proven/22 deferred until the required
        per-variant GPU/no-draw witnesses below are generated and joined.
      - [ ] Implement the reviewed per-ID renderer routes and gallery builders,
        then close every required NBT-variant witness and GPU/no-draw witness so
        the block-entity strict-final gate reaches 22 proven with no deferrals.
        - [ ] Provisional, uncompiled and unmeasured (never closes a gate): the
          `.mcbeben` carrier (`make block-entity-assets`) and a dedicated
          block-entity pass draw chests (single/double, lid cue), ender and copper
          chests, beds, shulker boxes, skulls (dragon and piglin from entity
          geometry), banners, bell with frame and swing, lectern, enchant/lectern
          book, conduit, decorated pots, item and glow frames with their items,
          campfire items, tinted scrolling beacon beams, end portal, sign text and
          model-shaped break cracks. Entity-drawn block states compile to
          `Invisible` terrain (`entity_drawn.rs`). Filled maps, spawner mob,
          flower-pot plants, conduit wind cube, hopper/brewing-stand terrain and
          native lighting/measurement remain open. Campfire terrain logs, ember bed
          and flames match the 26.30 client model. The model is unchanged for
          1.26.50 but remains unverified there.
          The enchanting-table book follows the native block actor tick and model
          animation (Bedrock 1.26.10): it turns toward the nearest player within
          three blocks, opens and closes, leafs toward random pages, hovers and
          bobs. Flat covers and pages draw as two-faced sheets, and entity-lit
          back faces are culled. Its entity lighting from the table's glow is
          provisional, and the lectern stand placement needs native measurement.
          Standing and wall banner cloth share the crossbar's authoring pivot,
          with matching mounting scale and wave direction. Full banner parity
          remains incomplete: animation lookup quantization, pattern composition
          and native rendered/input acceptance remain open.
    - [ ] Merge both the Axolotl protocol-fix branch and Cinnabar feature branch
      into their respective `main` branches through reviewed PRs using normal
      history-preserving merge commits (never squash or rebase the feature
      history), only after the applicable deterministic tests, native/GPU
      acceptance, zero-diagnostic state gate, and block-entity manifest gate
      are green.
  - [ ] Render-invisible blocks. Barrier, structure void, light blocks 0–15, invisible bedrock
    and moving block compile to non-occluding `Invisible` terrain ahead of the fallback
    inventory (`literal::is_default_invisible`). Incomplete: vanilla also tessellates barriers
    and light blocks as camera-facing sprites (`materials/barrier.material`) and structure voids
    as cube faces into their own terrain layers, drawn only while a creative local player's
    selected item is that block; those layers are not built.
- [ ] **2.7 Client lighting and atmosphere.** `P2.7-ATMOSPHERE` Block/sky flood fill, baked vertex
  light and day/night, then sky, fog, and clouds; finish the Phase 2 parity and
  teleport-remesh acceptance gates.
  - [x] Normalize SetTime and rain/thunder level events into bounded,
    vendor-independent protocol events; retain StartGame's initial current
    tick, day-cycle lock time, case-insensitive boolean `doDaylightCycle`
    state (default enabled when absent), and clamped initial rain/lightning.
    Runtime `GameRulesChanged` packets normalize only a case-insensitive
    boolean `doDaylightCycle` entry and ignore wrong types. Two deferred
    pre-spawn SetTime
    packets retain FIFO order in Play, post-spawn normalization is identical,
    and non-finite initial weather values fail closed.
  - [x] Consume the normalized environment stream into app-owned clock and
    weather resources without interpreting visual curves. A replacement
    StartGame begins a new environment session, anchors its exact initial tick,
    preserves its cycle lock and bounded rain/lightning targets, advances only
    while `doDaylightCycle` is enabled, and uses the lock tick only when the
    rule is explicitly disabled. A runtime false transition freezes the exact
    current visual tick; true re-anchors and resumes it. Exact signed SetTime
    values immediately re-anchor either a running or frozen clock. Dimension
    changes
    preserve that world-session snapshot. FIFO-committed SetTime/day-cycle/weather
    updates do not dirty meshes, enqueue mesh changes, or change cave
    connectivity. Mesh-baked light response and vanilla atmosphere parity
    remain open.
  - [x] Implement the renderer-independent sparse light core: independent
    uniform-or-packed block/sky nibble volumes, copy-on-write snapshots,
    generation-checked storage/eviction, and a bounded darken-then-increase
    solver over explicit Unknown/KnownAir/Resident cells. Exact one-cell halo
    samples carry scheduler-owned trust and direct-sky provenance; unknown,
    dirty, or untrusted boundaries never seed light, Nether/End reject sky,
    and all propagation shares one enforced queue budget. WorldStream now owns
    the authoritative state metadata and bounded scheduling below; mesh baking,
    GPU/shader light consumption, and vanilla atmosphere parity remain open.
  - [x] Generate and ship bounded per-runtime-state light metadata without
    changing the reviewed `BREG1003`: `LREG1001` proves the exact protocol-state
    count, identity, property order, and committed-BREG SHA-256 before emitting
    one packed emission/filter byte per state. Dragonfly revision
    `dbbd8b787946e53b1def8d532050751dfcdc80e7` is authoritative for 16,911
    concrete states; exact, identifier-uniform pinned PMMP fallback supplies only
    `minecraft:redstone_lamp` and `minecraft:lit_redstone_lamp`, with deterministic
    provenance IDs/reporting and fail-closed disagreement/range checks. `MCBEAS05`
    atomically carries the byte beside each visual, exposes it through both
    sequential and network-hash resolution, rejects stale `MCBEAS04`, and names
    the `--light-registry` rebuild command. The solver and WorldStream scheduling
    integration are covered below; mesh baking, GPU/shader light consumption,
    vanilla sky/fog/cloud parity, and visual acceptance remain open.
  - [x] Derive a deterministic per-frame atmosphere snapshot from the app-owned
    clock and weather state: real elapsed time advances enabled sessions at 20
    ticks per second, explicitly disabled cycles freeze at their lock tick,
    signed times use
    Euclidean day and moon-phase wrapping, and rain/thunder remain bounded.
    Extract one stable 96-byte uniform, render the first procedural sky/sun/moon
    pass at reversed-Z far depth with per-view MSAA specialization, and apply
    camera-distance fog to chunk, model, and liquid paths without adding storage
    bindings or per-frame texture/bind-group churn. These checked curves and
    procedural disks establish the integration slice, not vanilla parity;
    asset-backed GPU hookup, precipitation visuals, underwater/lava medium fog,
    and live reference acceptance remain open.
  - [x] Render numbered PNG/TGA/JPEG pack skyboxes in the Overworld and End.
    Resolve pack priority before image extensions, retain stock sky beneath
    absent faces, and cache the active dimension's textures across frames.
    Preserve authored colours across time/weather, encoded-channel linear
    filtering, clamped face edges, alpha blending and finite cube depth without
    depth writes. Fancy Skies and active blindness/darkness govern visibility;
    camera height and water/lava do not change the cube's projection.
    **Bedrock Skybox Vertical Offset** is a persisted Video option, on by
    default; turning it off centers the Overworld cube without guessing pack
    compensation. GPU regressions cover face orientation, seams, both offset
    states, transparency, foreground occlusion and reloads. Existing raster
    size/allocation bounds remain; HDR and compressed KTX faces are unsupported.
    Broader calibrated atmosphere parity and enhanced rendering remain open.
  - [x] Pin and carry the exact vanilla sun, moon-phase atlas, and cloud texture
    in an independent bounded `MCBEATM1` runtime blob. The compiler requires the
    exact tracked Mojang manifest fields, canonical LF bytes (accepting only a
    uniform LF or Windows CRLF checkout), and per-source PNG hashes, records
    encoded and decoded hashes, rejects malformed/noncanonical layouts, and
    publishes only to ignored local paths. `make assets` and `make client`
    refresh the serialized blob/report pair through one portable producer;
    normalized, case-variant, symlink/junction, and hardlink output aliases fail
    before either write. Focused pinned tests, full assets/client-asset suites,
    strict Clippy, and independent re-review are green through `aed8d7f`; no
    Mojang payload is tracked.
  - [x] Load the required sibling `MCBEATM1` carrier once at startup and render
    its exact sun, 4x2 moon-phase atlas, and repeating cloud texture through the
    existing reversed-Z atmosphere phase. Three persistent GPU textures, one
    sampler, and one identity-cached bind group replace the procedural celestial
    disks without per-frame or per-subchunk resource churn; missing or malformed
    carriers fail hard with the exact rebuild command. Clouds use absolute world
    time, altitude 128, weather/fog fades, and a tested +X eastward speed of 0.03
    blocks per tick. Full app/render suites, strict Clippy, WGSL validation, and
    independent review are green through `4bf2c8c`; native multi-platform visual
    tuning and parity acceptance remain open.
    The original fixed 3x3 opaque finite-cloud visual behavior is superseded by
    `docs/superpowers/plans/2026-07-16-native-cloud-parity.md`: exact local-only
    1.26.33.1 occupancy, transparent depth-aware legacy composition, directional
    lighting, exact weather contributions, and calibrated native quality controls.
  - [x] Accept the exact local-only Bedrock 1.26.33.1 cloud PNG as an optional
    atmosphere compiler override without changing the pinned sun/moon inputs or
    `MCBEATM1` schema. The compiler fails closed on the exact 7,880-byte encoded
    SHA-256, 256x256 dimensions, decoded RGBA8 SHA-256, and 13,356 occupied
    texels; the runtime carrier and deterministic report retain only the canonical
    `textures/environment/clouds.png` logical path and independent hashes. The
    wrapper API and no-override fixture remain deterministic, while
    `assetc atmosphere --clouds-override` and optional `CINNABAR_CLOUDS_PNG`
    expose the local input portably. Both Make modes conservatively force the
    atmosphere producer, preventing a set-to-empty override transition from
    retaining a stale carrier. Startup evidence now identifies
    `cloud.wgsl` separately from `atmosphere.wgsl`. Synthetic rejection tests,
    synthetic accepted options tests through a private injected identity seam,
    a parsed-CLI-to-encoded-report test through the production command handler,
    environment-gated installed-input acceptance, the full assets suite, app asset
    suite, strict relevant Clippy, formatting, and diff checks are green;
    calibration and live parity acceptance remain open in the native-cloud plan.
  - [x] Replace opaque/depth-writing finite clouds with one transparent sorted
    render item using alpha blending, reversed-Z depth testing without depth
    writes, exact sequential rain/thunder colour contributions, real sun-vector
    directional lighting, bounded distance-fog alpha, and deterministic negative
    coordinate wrapping. Empty/sub-threshold alpha emits no geometry, GPU records
    and bind groups remain immutable, and collapsed/reversed/non-finite fog ranges
    fail to finite deterministic alpha. Focused meshing/render tests, WGSL/Naga
    validation, strict Clippy, formatting, independent review, and post-merge
    verification are green through `87e856f`. A live night run additionally
    proved that zero daylight could still make otherwise-transparent clouds
    pure black; clouds now share terrain's provisional `0.2` night sky-transfer
    floor through `e7c85ea`. **Forced-night native WGC evidence on `3bdb317`
    passed this colour gate:** cloud faces remained visibly medium gray against
    the purple night sky rather than becoming black. The same run was only
    16-18/11,760 chunks at about 8 FPS, so it is not a performance or full-view
    acceptance witness. Native geometry scale/density and
    above/below/within/grazing visual acceptance remain open.
    **Cloud geometry evidence contract complete (2026-07-16):** one bounded,
    identity-deduplicated `CLOUD_GEOMETRY_EVIDENCE calibrated=false` marker now
    records exact occupied texels, uploaded quad count/bytes, instance count,
    provisional 256-block period and Y=128..132 bounds, pinned High-quality
    native `grid_size=3`/`mesh_size=64`/`distance_scale=3` controls, and the
    atmosphere asset identity. It is produced only when a new atmosphere asset
    identity is prepared; no draw, shader, buffer topology, or per-frame work
    changed. Independent review and the complete render suite/strict checks are
    green through the merge following `c86d184`. The marker deliberately keeps
    `calibrated=false`: matching native views must derive the missing world-space
    mapping before the provisional layout changes.
  - [x] Resolve the camera-eye medium directly from palette-native liquid
    contributors, including secondary waterlogged layers, and use the exact
    two-triangle surface drawn by the shared quad index buffer for the air/water
    or air/lava transition, including noncoplanar corner heights. The sparse
    camera query reads packed indices and palette entries directly without
    constructing the mesher's allocating palette-fact cache; missing and
    non-finite samples fail open to air. Water and lava replace weather distance
    fog in the existing 96-byte atmosphere uniform, so chunk, model, liquid, and
    infinite-sky/celestial rendering share one medium response without a new
    binding or per-subchunk resource. The bounded water/lava colours and 32/3-
    block visibility ranges are the Phase 2.7 baseline; native reference
    calibration and precipitation visuals remain open.
  - [x] Compile and route exact pinned client biome/fog profiles by camera
    biome with Overworld/Nether/End fallback. `MCBATM2` carries the bounded,
    hashed environment-profile and fog-distance tables; biome-specific fog
    wins per medium and missing media layer from `minecraft:fog_default`.
    Fixed and render-relative air, weather, water, and lava endpoints plus
    exact sky colour now reach the shared atmosphere frame while clock,
    weather, and celestial state remain intact. Lighting and atmospherics
    identifiers are retained as explicit provisional routes rather than
    claimed native lighting calibration. Exact pinned-pack provenance,
    envelope, routing, render, and integration tests are green through
    `53fd591`.
  - [x] Wire the sparse solver and MCBEAS05 per-state emission/filter metadata
    into generation-qualified WorldStream light storage and bounded,
    nearest-first one-subchunk solves. Exact face block/light halos, dirty
    boundary trust, separately retained direct-sky provenance, decoded/update/
    eviction invalidation, convergent neighbour iteration, and current-light
    mesh scheduling are covered without flat block arrays. Mesh light baking,
    GPU/shader consumption, sky/fog/cloud rendering, and visual acceptance
    remain open.
    - Release scheduler workload gate on 2026-07-14: the exact radius-16 square
      (33×33 columns) across all 24 Overworld subchunks completed 26,136
      known-air light solves, with 26,136 uniform fast-path completions, zero
      stale completions, and all keys current in 1,006 ms. This measures the
      lighting scheduler only; live teleport/full-view remesh acceptance,
      mixed-block workloads and rendering acceptance remain open.
  - [x] Capture palette-native mesh lighting in a fixed 27-slot identity halo,
    gate mesh dispatch until every known slot is current, reject and losslessly
    requeue completions after exact light or direct-sky identity changes, and
    invalidate the center plus all 26 dependants on light completion, load, or
    eviction. Center/face/edge/corner routing and absent dark fallback read
    nibble channels directly without flat staging arrays. App tests passed
    233/233 (with one release-only test ignored), all app integration and world
    suites passed, and the exact release workload completed 26,136/26,136
    current subchunks with zero stale completions in 987 ms. The halo now
    implements the allocation-free render sampler and the worker calls the
    light-aware mesher. Cube, model, cross, and liquid CPU sidecars retain
    independent block/sky/AO channels; cube greedy merges split on exact packed
    lighting, and the cube sidecar survives bounded render-queue extraction
    with exact byte accounting. The combined app/render suites and strict
    Clippy are green. GPU arena/shader consumption, mixed-block visual
    acceptance, and live teleport full-view remesh acceptance remain open.
  - [x] Consume cube, model, and liquid light sidecars in the GPU world shaders
    without adding a buffer, bind group, or per-subchunk render resource. Commit
    `fe1a2ea` appends cube sidecars to the existing binding-13 arena, expands the
    per-draw origin ABI to carry exact cube/light bases, validates aligned and
    disjoint direct/MDI addressing, and converts discrete block/sky/AO samples
    at the vertex before smooth interpolation. Daylight affects only sky light;
    full solved skylight retains a named provisional `0.2` transfer floor at
    true night while block light remains independent. This floor is a
    conservative calibration to the existing horizon baseline, not a vanilla
    parity claim; native Bedrock reference tuning remains open. Alpha and fog
    ordering remain intact. Full render and app suites, strict
    combined Clippy, WGSL semantic/Metal-stage checks, formatting, and diff
    checks are green. Live mixed-block GPU parity and teleport/performance
    acceptance remain open.
  - [x] Preserve the authoritative all-air suffix omitted by limited-request
    `LevelChunk` columns and feed it into the sparse top-down skylight graph.
    `highest=0` columns require no outbound slot or packet; replacement,
    eviction, direct-sky propagation, and the final mesh-light sidecar are
    covered. This fixes the live zero-skylight world while leaving block light
    independent. Full client, render-atmosphere, asset, camera, strict Clippy,
    WGSL, and independent review gates are green through `7805402`. That commit's
    RGB black-key celestial workaround was later disproved by the pinned
    near-black border texels and is superseded by the additive tranche below.
  - [x] Keep solved light level zero visibly non-black through a named
    provisional `0.04` linear ambient floor applied after independent
    block/sky/daylight combination. The floor-to-one remap preserves every
    higher light step and exact full brightness instead of flattening low
    levels; AO remains independent. Native Bedrock capture tuning of the exact
    floor remains part of final visual acceptance.
  - [x] Composite the pinned opaque sun and all eight moon phases as emissive
    additions rather than RGB-keyed opaque replacements. A decoded `MCBEATM1`
    regression traverses every 32x32 tile border, covers the exact problematic
    sun `(1,1,0)` and moon `(0,0,1)` samples against bright and dark skies,
    preserves dark lunar detail and HDR energy, and Naga-validates the shared
    WGSL path. Startup now emits ordered full envelope/shader SHA-256 evidence
    without paths or payloads. Full workspace tests, strict Clippy/formatting,
    and independent re-review are green through `384e08a`; the live all-phase
    native/GDI comparison remains open in the visual blocker below.

Perf budget carried from Phase 0 gate; add: full remesh of view distance after teleport ≤ 2s.

**Edge anti-aliasing (2026-07-15):** [x] the client uses single-sample world
targets plus an FXAA-only post-process through `cd31f7a`. This replaces the
nominally portable 4x MSAA default after a real Radeon RX 570 DX12 adapter
presented black frames for every multisampled view (including a minimal Bevy
clear-color reproduction), while a macOS adapter had already rejected 8x
`Depth32Float`. Bevy's umbrella anti-alias plugin is disabled so unavailable
TAA/SMAA/CAS graph nodes are not installed; only `FxaaPlugin` runs. The full
280-test client suite, strict Clippy/formatting, release build, independent
review, and a fresh BDS/GDI above/within/below gallery are green. Higher
multisample modes remain conditional on a future capability- and live-tested
resolve path rather than a hardcoded camera sample count.

**Live visual acceptance (Computer Use):** run the Bevy app in representative vanilla
world scenes and compare visible results against the matching Mojang vanilla assets/reference
client at multiple distances and view angles. Verify exact texture/model selection, UV orientation
and wrapping, per-layer mip quality, opaque/cutout/blend behavior, flipbooks, biome tints,
block/sky lighting across day/night, fog, sky, and clouds. Exercise focus, keyboard input,
movement, and mouse-look/rotation during the pass. No placeholder/debug texture or visibly
non-vanilla rendering ships past this phase; record screenshots and any adjudicated parity gaps
in the phase report. If Computer Use window capture fails, take a native Windows screenshot,
store it only under the user's temporary directory, inspect that file, and never commit it.

**Open live defects (reported 2026-07-15; Phase 2 acceptance blockers):**

- [ ] Reproduce and eliminate the moving horizontal/vertical "TV static" or
  void-band artifact under camera motion. Separate presentation tearing from
  resident/frustum/submission loss with one identical-scene FIFO-versus-
  no-vsync capture and coherent resident/frustum/submitted/GPU-completed frame
  identities; do not change culling unless those identities prove a missing
  draw.
  - [x] Install bounded exact missing/extra key evidence through resident,
    cave, frustum, submitted, and GPU-completed stages for one coherent
    view/frame in both Direct and MDI paths. Acceptance defaults to FIFO;
    Immediate is an explicit A/B whose effective mode is proven from the
    primary surface capabilities with the same Bevy render instance/adapter,
    while absent/failed/fallback evidence rejects the comparison. Release
    profile, backend, adapter, and driver provenance plus PowerShell/Bash
    contracts passed independent review through `14db67f`.
  - [x] Remove the proven multisample resolve/presentation failure through
    `cd31f7a`: the affected Radeon DX12 adapter rendered black for every 4x
    MSAA view even in a minimal Bevy clear-colour reproduction, while the same
    release BDS scene renders through single-sample targets plus FXAA. The
    post-fix GDI gallery contains no black presentation or thin screen-space
    band, but this does not close the user-reported artifact without the
    binding identical-scene FIFO/Immediate motion comparison below.
  - [x] Make direct Windows launches and native visual inspection reliable
    through `1cad274`: default ignored-local assets fall back to the executable
    project root, Windows selects DX12 unless `WGPU_BACKEND` explicitly
    overrides it, and the local BDS scene was enumerated and captured through
    native Windows Graphics Capture at 1282x752. This closes the launch/capture
    tooling failure only; visual-parity and performance gates remain open.
  - [ ] Run the identical-scene release FIFO/Immediate capture, classify the
    artifact from coherent evidence, and eliminate the proven source.
- [ ] Make initial chunk publication and steady streaming meet the frame and
  teleport budgets without debug-only DX12 direct-draw collapse. Attribute
  decode, light-halo readiness, mesh queue wait, worker time, GPU-upload budget,
  render submission, and present latency independently; validate the final
  release path at radius 16 with no visible stalls and full-view remesh in at
  most two seconds.
  `P2-CHUNK-PUBLICATION`
  - [x] Eliminate unchanged-light publication churn through `39c44e8`: exact
    no-op and direct-sky-provenance-only completions preserve sampled light
    identity/generation and perform zero mesh invalidations, while genuine
    nibble changes retain the full 27-dependent invalidation contract. Exact
    saturating outcome counters and independent review are green; publication
    latency attribution, the full release benchmark, and live gates remain open.
  - [x] Attribute decode/light/mesh bounded-queue wait independently from
    worker duration and emit one coherent periodic publication snapshot that
    joins outcome counters, pending/in-flight gauges, upload bytes, draw mode,
    build profile, present proof, and adapter provenance. PowerShell and Bash
    reject missing, extra, duplicate, malformed, wrong-typed, or mismatched
    rows. Full client/script suites, strict Clippy/formatting, adversarial
    contract tests, and independent re-review are green through `0811c0a`;
    the full release radius-16 benchmark and live resource gates remain open.
  - [x] Install one shared adaptive item-and-byte publication budget across
    world handoff, main-world application, extraction, and GPU preparation.
    Genuine frame-pressure stalls reduce the budget multiplicatively, normal
    FIFO pacing is tolerated, and sustained healthy frames recover
    conservatively; nearest-first ordering and exact cohort identities remain
    unchanged. Zero-byte removals retain a separate 256-operation hard cap,
    and whole-arena growth copies are reserved inside the same GPU byte budget.
    Deterministic pressure, continuity, accounting, app, client-world, and
    render tests plus independent review are green through `9ea25e1`; the live
    radius-16 <=2-second acceptance gate remains open.
  - [x] Prove the deterministic full radius-16 publication path through
    `7098b65`: 26,136 current subchunks pass accepted lighting, meshing,
    main-world queue application, real render extraction, production GPU
    preparation, and exact acknowledgement in 1,369 ms on the integration
    rerun. The 64 populated witnesses produced 190,384 positive upload bytes;
    31,786 known-air removals completed without spending the production
    128-item non-empty upload budget, while all applications remained capped at
    256 per frame. Zero stale, pending, in-flight, duplicate, or
    unacknowledged work remained. Full render/client suites, strict
    Clippy/formatting, and three independent reviews are green. This closes only
    deterministic Task 3 Steps 1–3; the release BDS join/teleport,
    presentation, RSS, settled-CPU, and visible-stall gate remains open.
  - [x] Ship the bounded publication, same-connection fast-transfer recovery,
    and player-near request-priority implementation on canonical
    `phase2-textures` through `2fc7a33`. The combined history includes bounded
    publication at `dedbef1`, local reset and request-ordering work from
    `a7d8a90` through `c4ed5a9`, integration at `f0f27eb`, vanilla slash
    `CommandRequest` encoding at `be32657`, and final cross-platform CI repairs.
    Canonical CI run `29671070071` is green. This is an implementation-only
    milestone: the binding LBSG same-connection reset witness, ordered
    Lunar/Zeqa native evidence, and the master <=2-second live gate remain open.
  - [x] Remove the frame-coupled four-decode throttle through `2b16368`.
    The 2026-07-15 opt-in live trace proved that all 4,912 admitted jobs
    completed (3,988 SubChunk jobs at about 0.02 ms each), while 1,218 frames
    over 180 seconds limited throughput to 27.29 jobs/s solely because only
    four jobs could dispatch per frame. Decode dispatch now drains the already
    bounded 32-heavy-event admission window in one poll; retained payloads
    remain capped at 32 and the result channel at 128. The deterministic
    regression, all 284 client unit tests (two ignored), strict Clippy,
    formatting, and CI are green.
  - [ ] Mesh neighbourhood gate, provisional and labeled incomplete: a resident
    mesh waits while any of its 26 neighbours is owed (requested, or unsent in
    the announced Euclidean disk while its cohort made new progress within 1 s).
    Current vanilla behaviour instead requires eligible horizontal columns
    before rebuilding; the historical missing-column claim is superseded. The
    1 s quiet fallback remains provisional, with no native reference.
    `streaming_harness` checks slow delivery for transient geometry and dark seams.
  - [ ] Replace the provisional universal Euclidean publisher-disk rule with
    per-publisher-epoch membership from unique FIFO-committed request-mode
    `LevelChunk` announcements. The raw block radius remains a separate
    retention/classification witness: it does not define one protocol-wide
    enumerable cohort. The earlier BDS-specific run through `1fdd874` observed
    797 complete columns, while authoritative Lunar raw-128 evidence observes
    exactly 177; Dragonfly's attributable classifier gives 177 at raw 128 and
    749 at raw 256, and raw 120 => 177 remains labeled compatibility policy.
    A later unique announcement must expand the same epoch and invalidate the
    frozen count/hash/presentation proof; publisher identity, session, and
    dimension changes reset membership. Exact source eviction and GPU-manifest
    evidence remain mandatory. Earlier independent verification passed all 289
    client unit tests (two ignored), the 43/14/14 integration groups, strict
    Clippy, formatting, and diff checks. Live DX12/FIFO BDS run
    `20260716T001125Z-16608` reached `exact=true` with its 797 announced
    columns, 22,488 resident subchunks, 14,018 known-air
    subchunks, zero source/foreign evidence, and every network, request,
    decode, light, mesh, render-upload, and acknowledgement queue drained to
    zero. The run still timed out after this boundary because no binding pair
    of exact GPU-completed presented-frame acknowledgements was published;
    presentation-gate diagnosis and the <=2-second remesh gate remain open.
  - [x] Correct exact presented-frame acknowledgement for legitimately culled
    resident allocations through `9a195c1`. The gate still proves the complete
    target GPU allocation manifest, exact generations, and zero missing,
    unexpected, source, foreign, stale, or orphan evidence, but now requires
    every stable visible allocation to be drawn instead of incorrectly requiring
    every hidden allocation to be drawn in one frame. Two adjacent frames must
    retain the same visible manifest. The visible-undrawn, hidden-resident,
    visibility-churn, and skipped-frame regressions are green along with all 157
    render tests, 289 client unit tests (two ignored), strict Clippy, formatting,
    and diff checks. Live DX12/FIFO run `20260716T003124Z-7592` advanced through
    `CINNABAR_TELEPORT_SETTLED` with a complete 6,951-allocation manifest and a
    stable 1,676-allocation visible/drawn subset, with every contamination
    counter zero. The binding far teleport still took 48,128 ms and the forced
    full-view remesh did not finish inside the 60-second timed session; peak
    pending meshes reached 43,024, maximum mesh queue wait reached 30,833 ms,
    and the final snapshot still had 148 uploads queued. The <=2-second live
    remesh/performance gate therefore remains open and is the next publication
    bottleneck.
  - [x] Remove semantically empty work from the forced acceptance remesh. Trace
    attribution of `20260716T003124Z-7592` proved that the old gate scheduled all
    22,488 resident identities even though only 6,951 had published GPU
    allocations: 14,018 known-air plus 1,519 packed-empty identities created
    15,537 guaranteed no-mesh jobs, 69.1% of the forced cohort. The gate now
    remeshes the teleport's complete frozen allocation manifest, never its
    visible subset, and fails closed on empty, duplicate, stale-generation, or
    nonresident entries. Exact cohort hashes, forced generations, full
    allocation identity, zero contamination, and the adjacent GPU-presented
    frame proof remain unchanged. This reduces the hard application floor from
    88 to 55 frames; it does not yet prove the binding two-second gate. All 292
    active client unit tests plus the 43/14/14 integration groups, strict Clippy,
    formatting, and diff checks pass. A fresh live run must measure the new
    floor and determine whether baseline frame rate or upload scheduling is the
    next limiter.
    **Live result (2026-07-15):** release DX12/FIFO run
    `20260716T052758Z-9172` retained the exact 6,951-allocation cohort and zero
    contamination, then completed the forced remesh in 8,596 ms over 67 frames
    instead of timing out. The binding teleport remained 48,547 ms. At roughly
    eight presented frames per second, the current 128-nonempty/256-total
    frame-coupled budgets impose a 55-frame theoretical minimum, so adaptive
    upload/application scheduling is now the measured next limiter. The script
    failed only after both binding markers when its 60-second client exited
    during the later steady-resource sample; no final resource pass is claimed.
- [ ] Remove every dark rectangle/background pixel around the pinned sun and
  moon textures. The acceptance test must exercise decoded pinned pixels and
  mip/filter edges, not merely string-inspect WGSL, and must prove both bodies
  against bright and dark skies across all moon phases.
  - [x] Land the decoded-border additive composition and path-independent
    envelope/shader identity tranche through `384e08a`.
  - [ ] Run fresh release/GDI views against the matching native client for sun
    and all moon phases, including horizon and filter-edge cases, before
    closing the visible defect.
- [ ] Precipitation parity with the 26.30/1.26.50 `WeatherRenderer` and `Weather`
  material: ten wrapped 30-block particle layers per kind over a 2,500-quad mesh
  (925-particle pool), velocity-stretched sheet streaks, exact rain/snow params,
  UV cells, lattice offsets, intensity smoothing and density, and a 64x64 column
  occlusion grid replace the per-column Java-style sheet. Provisional, not
  closing the gate: wind uses our own seeded simplex (not the native permutation),
  the 0.01-scale per-layer turbulence and block-light tint are omitted, the
  density-halving view flag is assumed unset, and no native side-by-side capture
  has been taken.
- [ ] Replace the current infinitely thin cloud plane with a vanilla-parity
  cloud volume/layer that has visible thickness and side faces while retaining
  bounded GPU cost, world anchoring, weather/fog fades, and the existing shared
  atmosphere resource architecture. Verify from above, below, within, and at
  grazing angles against matching native reference captures.
  - [x] Record the installed native occupancy/material/config evidence and the
    reviewed greedy packed finite-mesh design/implementation plan in
    `docs/superpowers/specs/2026-07-15-finite-cloud-mesh-design.md` and
    `docs/superpowers/plans/2026-07-15-finite-cloud-mesh.md`.
  - [x] Implement and independently review the deterministic periodic CPU
    mesher: exact 256x256 alpha occupancy, toroidal seam culling, greedy exposed
    faces, an eight-byte packed ABI, checked worst-case ceilings, and canonical
    snapped 3x3 origins are green through `03a8c3a`.
  - [x] Render that finite mesh through one identity-cached custom GPU pipeline
    with immutable eight-byte quad records, vertex pulling, one nine-instance
    draw, physical reversed-Z depth, per-view MSAA/HDR specialization, bounded
    weather/fog shading, and explicit Metal-safe binding visibility. The old
    fullscreen sampled plane is removed. Full render/client suites, strict
    workspace Clippy/formatting, and independent review are green through
    `e2d0ea8`; native visual and performance acceptance remains open.
  - [ ] Resolve the 2026-07-15 live user rejection that the finite clouds look
    substantially non-vanilla. Reproduce the exact visible scale, thickness,
    silhouette, face shading, motion, fog, and seam behavior against the
    matching native Bedrock client, correct the proven differences, and obtain
    a fresh accepted above/below/within/grazing gallery before closing this
    blocker. Deterministic mesher/pipeline tests alone do not satisfy it. The
    first live/evidence audit proves three root mismatches that must be removed:
    the compiled `v1.26.30.32-preview` mask differs from the installed 1.26.33.1
    mask at 24,175/65,536 occupancy coordinates; the fixed 3x3 256-period draw
    ignores native `cloud_mesh_size: 64`, quality grid, and distance controls;
    and the opaque, depth-writing, fixed-face shader contradicts native
    transparent, directional-light, and exact weather-colour inputs. Exact
    native cloud bytes remain local build inputs and must never be committed.
    The opaque/depth-writing/material mismatch is resolved through `87e856f`;
    native mesh size, quality/distance controls, density, scale, thickness,
    silhouette, and live gallery acceptance remain open.
  - [ ] Apply the 26.30/1.26.50 `Clouds` material and cloud renderer values: one
    `clouds.png` texel per 16x16 blocks (4,096-block period), a 4-block slab at
    192.33, baked face shade (top 1, bottom 0.75, x sides 0.925), the
    vanilla day/weather/sunrise cloud colour with alpha 0.7, drift 0.02
    blocks/tick toward -X, and the 0.9D-1.9D distance fade with no fog. Landed
    provisionally: the pre-Caves-and-Cliffs 128 height, thunder mixing, the
    sunrise darkening term, above/below face flags and the quality/weather lerp
    of the fade distance are unverified, and no native gallery was taken.
  - [ ] Implement, independently review, and live-verify the finite cloud mesh.
