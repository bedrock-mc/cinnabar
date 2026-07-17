# Phase 5 UI, Interaction, Combat, and Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Phase 5.1 through 5.8: bounded Bedrock UI and fonts, HUD and
chat, scoreboards and boss bars, server-authoritative inventory and
interaction, strictly vanilla entity combat, server forms, native UI parity,
and persisted in-game controls/video settings.

**Architecture:** `crates/ui` owns renderer-independent retained UI, network
state projections, settings, and deterministic layout. `crates/protocol` owns
bounded protocol normalization and outbound packet builders; `crates/render`
consumes a compact shared UI draw list; the app owns input authority, network
ordering, persistence paths, and live system wiring. Inventory is the only
authority for the selected stack, and every item presentation layer consumes
the Phase 4 canonical item identity.

**Tech Stack:** Rust 1.93.1 edition 2024, Bevy 0.18.1/WGPU 27, protocol 1001
Valentine packet definitions, `serde`/`serde_json` with explicit limits,
provenance-checked Bedrock bitmap fonts, PowerShell acceptance harnesses,
local BDS, and matching native Bedrock references.

## Global Constraints

- Work only in the fresh `completion-phase5` worktree based on the current
  `completion-integration` checkpoint; never modify an archival worktree.
- The integration lane exclusively owns root `Cargo.toml`, `Cargo.lock`,
  `plan.md`, architecture allowlists, every shared crate module root,
  protocol world dispatch, asset-compiler command registration, and final app
  module/schedule assembly. Phase 5 commits leaf modules and tests only, then
  sends an explicit export/dispatch/assembly request to integration.
- Before Task 1, integration creates and registers the minimal `crates/ui`
  workspace shell (`Cargo.toml`, `src/lib.rs`, root membership, lockfile, and
  policy edge). Phase 5 modifies that shell; no task runs `cargo -p ui
  --locked` against an unregistered crate.
- Combat is strictly vanilla: no extended or randomized reach, enlarged
  targets, automatic target selection, auto-clicking, or Lunar-specific path.
- UI focus consumes input and emits semantic releases before physics or
  networking samples the next tick.
- Protocol, UI, settings, and form strings/collections have explicit byte,
  depth, and item ceilings; malformed required state fails closed.
- The selected stack, local viewmodel, remote equipment, inventory UI,
  dropped items, and outbound transactions use the same Phase 4
  `ItemStackIdentity`/`ItemVisualRoute` contract.
- Valentine item payloads normalize once into Phase 4
  `protocol::NetworkItemStack`; protocol never imports `assets`. A single
  `WorldEvent::Equipment(EquipmentEvent)` is fanned out by the app only after
  local runtime identity is known: local selection enters the Phase 5
  inventory authority, while remote equipment enters the Phase 4 actor store.
- Phase 5 owns the single local item-action timeline and publishes immutable
  snapshots. Phase 4 presentation consumes those snapshots and must not infer
  or advance a second attack, break, place, or use timeline.
- Damage, knockback, death, durability, cooldown, inventory mutation, and
  container contents remain server-authoritative.
- Use one shared UI vertex/index arena, font/item texture pages, and bind-group
  family; never create per-glyph, per-control, per-item, or per-action GPU
  resources.
- Per-user settings live outside the repository, use atomic replacement, and
  never contain credentials or tokens. Acceptance overrides remain separate.
- Every task finishes with focused tests, warnings-denied Clippy, formatting,
  an independently reviewable commit, and no tracked generated Mojang payload.

## File and Module Map

- `crates/protocol/src/ui.rs`: bounded inbound text/HUD/scoreboard/boss/form
  normalization.
- `crates/protocol/src/inventory.rs`: bounded item/container/hotbar/stack
  events reusing Phase 4 `NetworkItemStack`; local/remote routing happens in
  the app after local runtime identity is known.
- `crates/protocol/src/interaction.rs`: vanilla ray decision inputs and exact
  outbound attack/use/stack request packet builders.
- `crates/ui/src/action.rs`: device-neutral UI actions consumed from Phase 3
  semantic input.
- `crates/ui/src/model.rs`: retained node tree, focus, navigation, scaling,
  safe area, clipping, and draw-list output.
- `crates/ui/src/text.rs`: formatting-code parser, glyph shaping, wrapping,
  and cached text layout.
- `crates/ui/src/hud.rs`, `chat.rs`, `scoreboard.rs`: receive-side stores and
  view builders.
- `crates/ui/src/inventory.rs`: authoritative inventory/container state,
  selected stack, pending request journal, and rollback.
- `crates/ui/src/forms.rs`: bounded modal/menu/custom form model and responses.
- `crates/ui/src/settings.rs`: typed settings, validation, migration, atomic
  persistence payload, and defaults.
- `crates/ui/src/menu.rs`: in-game menu and settings screens.
- `crates/render/src/ui.rs`, `ui_render.rs`, `ui.wgsl`: shared UI extraction,
  preparation, pipeline, texture pages, clipping, and draw commands.
- `app/src/ui_runtime.rs`: UI stores, network event application, focus
  authority, draw-list publication, and app-facing UI actions.
- `app/src/interaction.rs`: local block/entity ray selection and provisional
  action timeline.
- `app/src/settings_runtime.rs`: platform path, load/save, runtime application,
  and acceptance override separation.
- Integration handoffs: shared `lib.rs` exports, `protocol/src/world.rs`
  dispatch, `asset-compiler/src/bin/assetc.rs` registration, root manifests,
  architecture policy, and `app` module/plugin/schedule wiring.

## Ordered integration checkpoints

Published checkpoint branches are immutable. Integration advances in this
order; a later producer/consumer lane starts from the newly advanced
`completion-integration` rather than rewriting an earlier checkpoint:

```powershell
git switch completion-integration
git merge --no-ff completion-phase4-item-interface
git merge --no-ff completion-phase4-actor-interface
git merge --no-ff completion-phase5-ui-interface
git merge --no-ff completion-phase5-authority
git merge --no-ff completion-phase4-presentation
git merge --no-ff completion-phase5-consumer
```

The semantic dependency is therefore Phase 5 authority producer -> Phase 4
presentation viewmodel -> Phase 5 acceptance witness consumer. Integration
must not merge `completion-phase4-presentation` before
`completion-phase5-authority`, or run Task 17 before
`completion-phase4-presentation` is present in the Phase 5 consumer worktree.

---

### Task 1: Scaffold the renderer-independent UI crate and frozen interfaces

**Files:**
- Modify the integration-created shell: `crates/ui/Cargo.toml`
- Request integration export edit: `crates/ui/src/lib.rs`
- Create: `crates/ui/src/action.rs`
- Create: `crates/ui/src/geometry.rs`
- Create: `crates/ui/src/settings.rs`
- Create: `crates/ui/tests/action.rs`
- Request integration edits: root `Cargo.toml`, `Cargo.lock`, architecture
  policy, and the initial registered UI shell before this task starts

**Interfaces:**
- Consumes: Phase 3 `ActionSnapshot` through app-owned translation only.
- Produces: `UiAction`, `PointerPhase`, `UiScale`, `DpiScale`, `SafeArea`, `UiPoint`, `UiRect`,
  `UiLimits`, and the stable `UserSettings`/settings section types. Task 5
  publishes the checkpoint only after the draw-list/render carrier is frozen.

- [ ] **Step 1: Write failing interface and bounds tests**

```rust
use ui::{DpiScale, PointerPhase, SafeArea, UiAction, UiLimits, UiPoint, UiRect, UiScale};

#[test]
fn scale_and_geometry_reject_non_finite_or_inverted_values() {
    assert!(UiScale::new(f32::NAN).is_err());
    assert!(UiScale::new(0.49).is_err());
    assert_eq!(UiScale::new(2.0).unwrap().get(), 2.0);
    assert_eq!(DpiScale::new(1.5).unwrap().physical_to_logical(150.0), 100.0);
    assert!(UiRect::new(UiPoint::new(5.0, 0.0).unwrap(), UiPoint::new(4.0, 1.0).unwrap()).is_err());
}

#[test]
fn actions_are_device_neutral_and_limits_are_fixed() {
    assert_eq!(UiAction::Accept, UiAction::Accept);
    let point = UiPoint::new(10.0, 20.0).unwrap();
    assert_eq!(UiAction::PointerPrimary { position: point, phase: PointerPhase::Pressed },
               UiAction::PointerPrimary { position: point, phase: PointerPhase::Pressed });
    assert_eq!(UiLimits::MAX_NODES, 16_384);
    assert_eq!(SafeArea::ZERO.left(), 0.0);
}

#[test]
fn settings_interface_has_versioned_typed_sections() {
    let settings = UserSettings::default();
    assert_eq!(settings.schema_version, CURRENT_SETTINGS_SCHEMA);
    assert!(settings.video.horizontal_fov_degrees.is_finite());
    assert!(settings.controls.bindings().len() <= 128);
}
```

- [ ] **Step 2: Run the test and verify it fails**

```powershell
cargo test -p ui --locked --test action -- --nocapture
```

Expected: FAIL because the crate and types are absent.

- [ ] **Step 3: Implement the minimal frozen surface**

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiAction {
    Navigate([i8; 2]), Accept, Cancel, TabNext, TabPrevious,
    PointerMove { position: UiPoint },
    PointerPrimary { position: UiPoint, phase: PointerPhase },
    PointerSecondary { position: UiPoint, phase: PointerPhase },
    Scroll { delta: UiPoint },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase { Pressed, Held, Released }

pub struct UiLimits;
impl UiLimits {
    pub const MAX_NODES: usize = 16_384;
    pub const MAX_TEXT_BYTES: usize = 16_384;
    pub const MAX_FOCUSABLE: usize = 4_096;
    pub const MAX_CLIP_DEPTH: usize = 32;
}
```

Implement `UiScale` as a finite `0.5..=4.0` user-scale newtype and `DpiScale`
as a finite `0.5..=8.0` platform-scale newtype. Convert physical pointer/touch
positions and scroll deltas to logical coordinates exactly once using
`DpiScale` before constructing `UiAction`; reject non-finite coordinates or
zero/negative DPI. Add tests at DPI 1.0, 1.25, 1.5, 2.0, and 3.0 proving the
same logical hit target and clip bounds. Keep finite `UiPoint`,
non-inverted `UiRect`, and finite nonnegative `SafeArea` with `ZERO`.
Add stable settings structs now so Phase 3 bindings and Phase 5 screens target
one interface; Task 15 adds validation, migration, and persistence:

```rust
pub const CURRENT_SETTINGS_SCHEMA: u32 = 2;
pub struct UserSettings {
    pub schema_version: u32,
    pub controls: semantic_input::ControlSettings,
    pub video: VideoSettings,
    pub gameplay: GameplaySettings,
}

pub struct GameplaySettings {
    pub default_perspective: semantic_input::PerspectiveMode,
}
```

- [ ] **Step 4: Run focused checks and commit the foundation**

```powershell
cargo test -p ui --locked --test action -- --nocapture
cargo clippy -p ui --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
git add crates/ui/src/action.rs crates/ui/src/geometry.rs crates/ui/src/settings.rs crates/ui/tests/action.rs
git commit -m "feat(ui): add bounded UI foundation interfaces"
```

Expected: PASS; this remains an unpublished constituent commit until Task 5.

### Task 2: Compile and decode provenance-bound Bedrock bitmap fonts

**Files:**
- Create: `crates/asset-compiler/src/font.rs`
- Request integration exports/registration: `crates/asset-compiler/src/lib.rs`,
  `crates/asset-compiler/src/bin/assetc.rs`
- Create: `crates/assets/src/font.rs`
- Request integration export: `crates/assets/src/lib.rs`
- Create: `crates/asset-compiler/tests/font.rs`
- Create: `crates/assets/tests/font.rs`

**Interfaces:**
- Consumes: pinned reviewed resource-pack `font/` descriptors and textures.
- Produces: `CompiledFontCatalog`, `FontCatalogIdentity`, `GlyphMetrics`,
  `FontTexturePage`, and deterministic `MCBEFONT1` bytes.

- [ ] **Step 1: Write compiler rejection and determinism tests**

```rust
#[test]
fn font_carrier_is_deterministic_and_bounded() {
    let pack = fixture_pack_with_ascii_and_unicode_pages();
    let a = compile_fonts(pack.path()).unwrap();
    let b = compile_fonts(pack.path()).unwrap();
    assert_eq!(a.bytes, b.bytes);
    assert_eq!(&a.bytes[..9], b"MCBEFONT1");
    assert!(a.report.glyphs <= 65_536);
    assert!(a.report.pages <= 256);
}

#[test]
fn malformed_metrics_and_oversized_pages_fail_closed() {
    assert!(matches!(compile_fonts(malformed_metric_pack().path()), Err(FontCompileError::NonFiniteMetric { .. })));
    assert!(matches!(compile_fonts(oversized_page_pack().path()), Err(FontCompileError::PageTooLarge { .. })));
}
```

- [ ] **Step 2: Run tests and verify they fail**

```powershell
cargo test -p asset-compiler --locked --test font -- --nocapture
cargo test -p assets --locked --test font -- --nocapture
```

Expected: FAIL because font compilation/runtime decoding is absent.

- [ ] **Step 3: Implement the carrier types and limits**

```rust
pub const MAX_FONT_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_FONT_PAGES: usize = 256;
pub const MAX_FONT_GLYPHS: usize = 65_536;
pub const MAX_FONT_PAGE_SIDE: u32 = 4_096;

pub struct GlyphMetrics {
    pub codepoint: char,
    pub page: u16,
    pub uv: [u16; 4],
    pub bearing: [i16; 2],
    pub advance_64: i16,
}

pub struct FontCatalogIdentity {
    pub schema: u32,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
}
```

Sort glyphs by codepoint and source identity, reject duplicate semantic glyphs,
checked-convert every integer, premultiply no texture data, and require exact
source-manifest provenance at startup. Runtime decoding validates all offsets
before allocating and never retains source JSON.

- [ ] **Step 4: Run focused checks and commit**

```powershell
cargo test -p asset-compiler --locked --test font -- --nocapture
cargo test -p assets --locked --test font -- --nocapture
cargo clippy -p asset-compiler -p assets --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
git add crates/asset-compiler crates/assets
git commit -m "feat(assets): compile bounded Bedrock bitmap fonts"
```

### Task 3: Implement formatting-aware cached text layout

**Files:**
- Create: `crates/ui/src/text.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/text.rs`

**Interfaces:**
- Consumes: `assets::CompiledFontCatalog`, finite `UiScale`, bounded UTF-8.
- Produces: `TextStyle`, `TextSpan`, `TextLayoutKey`, `TextLayout`,
  `GlyphQuad`, and `TextLayoutCache`.

- [ ] **Step 1: Write parsing, wrapping, cache, and bounds tests**

```rust
#[test]
fn formatting_codes_change_style_without_emitting_glyphs() {
    let spans = parse_bedrock_text("A§cB§rC", 64).unwrap();
    assert_eq!(spans.plain_text(), "ABC");
    assert_eq!(spans[1].style.color, BedrockColor::Red);
    assert_eq!(spans[2].style, TextStyle::default());
}

#[test]
fn layout_cache_is_identity_and_scale_qualified() {
    let mut cache = TextLayoutCache::new(8, 64 * 1024);
    let first = cache.layout(request("hello", 1.0, [1; 32])).unwrap();
    let same = cache.layout(request("hello", 1.0, [1; 32])).unwrap();
    let changed = cache.layout(request("hello", 2.0, [1; 32])).unwrap();
    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed.id());
    assert!(cache.retained_bytes() <= 64 * 1024);
}
```

- [ ] **Step 2: Run the test and verify it fails**

```powershell
cargo test -p ui --locked --test text -- --nocapture
```

Expected: FAIL because text layout is absent.

- [ ] **Step 3: Implement bounded parsing and layout**

```rust
pub const MAX_TEXT_SPANS: usize = 4_096;
pub const MAX_GLYPHS_PER_LAYOUT: usize = 16_384;
pub const MAX_WRAP_LINES: usize = 1_024;

pub struct TextLayoutKey {
    pub content_sha256: [u8; 32],
    pub style: TextStyle,
    pub width_64: u32,
    pub scale_1024: u16,
    pub font_identity: [u8; 32],
}
```

Parse Bedrock colour/style/reset codes in one pass, preserve invalid trailing
section signs as visible text, normalize CRLF to LF, perform checked fixed-point
layout, use the font replacement glyph for missing codepoints, and evict cache
entries in deterministic LRU order within both entry and byte caps.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test text -- --nocapture
cargo clippy -p ui --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui
git commit -m "feat(ui): lay out Bedrock formatted text"
```

### Task 4: Build retained layout, focus, navigation, and draw-list output

**Files:**
- Create: `crates/ui/src/model.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/model.rs`

**Interfaces:**
- Consumes: UI geometry, `UiAction`, and cached text layout.
- Produces: `UiTree`, `UiNodeId`, `UiNode`, `FocusState`, `UiVertex`,
  `UiDrawBatch`, and `UiDrawList`.

- [ ] **Step 1: Write deterministic layout and focus tests**

```rust
#[test]
fn safe_area_scale_and_focus_order_are_deterministic() {
    let mut tree = fixture_menu();
    let frame = tree.layout(viewport(1920, 1080), UiScale::new(2.0).unwrap(), safe_area(20.0)).unwrap();
    assert_eq!(frame.focus_order(), &[node("resume"), node("settings"), node("disconnect")]);
    assert!(frame.bounds(node("resume")).unwrap().left() >= 20.0);
}

#[test]
fn focus_change_releases_pointer_capture_and_respects_clip_depth() {
    let mut tree = deeply_clipped_tree(UiLimits::MAX_CLIP_DEPTH + 1);
    assert!(matches!(tree.build_draw_list(), Err(UiError::ClipDepthExceeded { .. })));
}
```

- [ ] **Step 2: Run the test and verify it fails**

```powershell
cargo test -p ui --locked --test model -- --nocapture
```

Expected: FAIL because the retained model is absent.

- [ ] **Step 3: Implement bounded retained layout**

```rust
pub struct UiDrawList {
    pub revision: u64,
    pub vertices: Vec<UiVertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<UiDrawBatch>,
}

pub struct UiDrawBatch {
    pub texture_page: u16,
    pub clip: UiRect,
    pub index_range: core::ops::Range<u32>,
}
```

Cap vertices at 262,144, indices at 393,216, and batches at 8,192; reserve
within checked byte ceilings before traversal. Traverse nodes in stable ID
order, compute focus order from explicit navigation then geometry, reject
cycles and duplicate IDs, intersect clips, and emit no empty batch.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test model -- --nocapture
cargo test -p ui --locked
cargo clippy -p ui --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui
git commit -m "feat(ui): build retained UI draw lists"
```

### Task 5: Render UI through one shared bounded GPU pipeline

**Files:**
- Create: `crates/render/src/ui.rs`
- Create: `crates/render/src/ui_render.rs`
- Create: `crates/render/src/ui.wgsl`
- Request integration export: `crates/render/src/lib.rs`
- Create: `crates/render/tests/ui_render.rs`
- Create: `app/src/ui_runtime/render_adapter.rs`
- Request integration assembly: `app/src/ui_runtime.rs` and app schedule

**Interfaces:**
- Consumes: render-owned `UiRenderInput` POD copied by the app from
  `ui::UiDrawList`, plus font/item texture pages and viewport/safe-area
  identity. `render` does not depend on `ui` or `client-world`.
- Produces: `UiRenderPlugin`, `UiRenderScene`, `UiRenderStats`, and one
  transparent overlay draw phase after the 3D scene.

- [ ] **Step 1: Write pipeline and allocation-contract tests**

```rust
#[test]
fn repeated_draw_lists_reuse_gpu_resources() {
    let mut harness = UiRenderHarness::new();
    harness.publish(fixture_draw_list(1));
    let first = harness.prepare().unwrap();
    harness.publish(fixture_draw_list(2));
    let second = harness.prepare().unwrap();
    assert_eq!(first.pipeline_id, second.pipeline_id);
    assert_eq!(first.bind_group_family_id, second.bind_group_family_id);
    assert_eq!(second.per_node_gpu_allocations, 0);
}

#[test]
fn shader_parses_and_batches_preserve_clip_order() {
    naga::front::wgsl::parse_str(include_str!("../src/ui.wgsl")).unwrap();
    assert_eq!(prepared_fixture().draw_order(), &[0, 1, 2]);
}
```

- [ ] **Step 2: Run tests and verify they fail**

```powershell
cargo test -p render --locked --test ui_render -- --nocapture
```

Expected: FAIL because the plugin and shader are absent.

- [ ] **Step 3: Implement shared extraction and preparation**

```rust
#[derive(Resource, Default)]
pub struct UiRenderScene {
    pub revision: u64,
    pub input: Option<std::sync::Arc<UiRenderInput>>,
}

pub struct UiRenderInput {
    pub vertices: Arc<[UiRenderVertex]>,
    pub indices: Arc<[u32]>,
    pub batches: Arc<[UiRenderBatch]>,
}

#[derive(Resource, Default)]
pub struct UiRenderStats {
    pub uploaded_vertices: u32,
    pub uploaded_indices: u32,
    pub draw_calls: u32,
    pub retained_gpu_bytes: u64,
    pub rejected_revision: Option<u64>,
}
```

Use one grow-with-ceiling vertex arena, one index arena, one texture-array
family, dynamic scissor rectangles, premultiplied alpha, and stable batch
order. Reject an oversized draw list without replacing the last valid scene;
attribute the rejected revision and reason. The WGSL converts logical UI pixels
to NDC using a viewport uniform and samples the batch texture page without
gamma-applying alpha.
The app adapter validates the UI limits and copies logical draw-list fields
into render-owned POD; no UI store or client-world object crosses the render
boundary.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p render --locked --test ui_render -- --nocapture
cargo test -p render --locked
cargo clippy -p render --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/render/src/ui.rs crates/render/src/ui_render.rs crates/render/src/ui.wgsl crates/render/tests/ui_render.rs app/src/ui_runtime/render_adapter.rs
git commit -m "feat(render): draw retained UI through shared arenas"
git branch completion-phase5-ui-interface HEAD
```

Expected: PASS and an immutable interface branch containing the action,
settings, `UiDrawList`, and render-owned `UiRenderInput` carriers required by
integration checkpoint 1.

### Task 6: Normalize bounded text, HUD, scoreboard, boss, and form packets

**Files:**
- Create: `crates/protocol/src/ui.rs`
- Request integration exports/dispatch: `crates/protocol/src/lib.rs`,
  `crates/protocol/src/world.rs`
- Create: `crates/protocol/tests/ui_packets.rs`
- Extend fixture generator: `tools/fixturegen`

**Interfaces:**
- Consumes: Valentine `Text`, title/actionbar, toast/player-status, attribute,
  objective/display/score, boss-event, modal-form, command-suggestion, and
  block-break-progress packets/events.
- Produces: `UiEvent`, `BlockCrackEvent`, and bounded subevents with
  dimension/session-independent packet content; app sequencing attaches
  session/FIFO identities.

- [ ] **Step 1: Generate fixtures and write normalization tests**

```rust
#[test]
fn ui_packets_normalize_without_vendor_types() {
    assert!(matches!(decode_ui_fixture("text.bin"), UiEvent::Text(_)));
    assert!(matches!(decode_ui_fixture("set_title.bin"), UiEvent::Title(_)));
    assert!(matches!(decode_ui_fixture("boss_event.bin"), UiEvent::Boss(_)));
    assert!(matches!(decode_ui_fixture("modal_form_request.bin"), UiEvent::Form(_)));
}

#[test]
fn oversized_text_scores_and_form_json_fail_closed() {
    assert!(matches!(normalize_text(text_bytes(16_385)), Err(UiPacketError::TextTooLong { .. })));
    assert!(matches!(normalize_scores(8_193), Err(UiPacketError::TooManyScores { .. })));
    assert!(matches!(normalize_form_json(json_bytes(1_048_577)), Err(UiPacketError::FormTooLarge { .. })));
}
```

- [ ] **Step 2: Run tests and verify they fail**

```powershell
cargo test -p protocol --locked --test ui_packets -- --nocapture
```

Expected: FAIL because `UiEvent` is absent.

- [ ] **Step 3: Implement the vendor-neutral event surface**

```rust
pub const MAX_UI_TEXT_BYTES: usize = 16_384;
pub const MAX_CHAT_PARAMETERS: usize = 128;
pub const MAX_SCORE_ENTRIES_PER_PACKET: usize = 8_192;
pub const MAX_BOSS_EVENTS: usize = 64;
pub const MAX_FORM_JSON_BYTES: usize = 1_048_576;

pub enum UiEvent {
    Text(TextEvent), Title(TitleEvent), Hud(HudEvent),
    Objective(ObjectiveEvent), Score(ScoreEvent), Boss(BossEvent),
    Form(FormRequestEvent), ChatAutocomplete(ChatAutocompleteEvent),
}

pub enum BossColor { Pink, Blue, Red, Green, Yellow, Purple, White }
pub enum BossOverlay { Progress, Notched6, Notched10, Notched12, Notched20 }
pub struct BossStyle {
    pub color: BossColor,
    pub overlay: BossOverlay,
    pub darken_sky: bool,
    pub create_world_fog: bool,
}

pub enum BlockCrackAction { Start, Progress { stage: u8 }, Stop }
pub struct BlockCrackEvent {
    pub position: [i32; 3],
    pub actor_runtime_id: u64,
    pub action: BlockCrackAction,
}
```

Copy strings into bounded `Arc<str>`, reject invalid UTF-8 at decode, preserve
Bedrock formatting codes, validate every float as finite, and retain protocol
IDs/actions rather than inferred UI state. Add `WorldEvent::Ui(UiEvent)` and
`WorldEvent::BlockCrack(BlockCrackEvent)` plus explicit packet/event match
arms; validate crack stages as `0..=9`. Normalize every boss add/update field,
including color, overlay, sky-darkening, and fog flags; unknown required enum
values fail closed. Unrelated packets still return `Ok(None)`.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p protocol --locked --test ui_packets -- --nocapture
cargo test -p protocol --locked
cargo clippy -p protocol --all-targets --locked -- -D warnings
Push-Location tools/fixturegen
try { $env:GOWORK='off'; go test ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
cargo fmt --all -- --check
git add crates/protocol/src/ui.rs crates/protocol/tests/ui_packets.rs tools/fixturegen
git commit -m "feat(protocol): normalize bounded Bedrock UI events"
```

### Task 7: Retain receive-only HUD, chat, title, toast, and player status

**Files:**
- Create: `crates/ui/src/hud.rs`
- Create: `crates/ui/src/chat.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/hud.rs`
- Create: `crates/ui/tests/chat.rs`
- Create: `app/src/ui_runtime.rs`
- Create: `app/src/ui_runtime/tests.rs`
- Request integration module/schedule assembly for `app/src/ui_runtime.rs`

**Interfaces:**
- Consumes: sequenced `protocol::UiEvent`, font/text layout, UI tree.
- Produces: `HudStore`, `ChatStore`, `UiRuntime`, and deterministic view nodes.

- [ ] **Step 1: Write lifecycle and focus tests**

```rust
#[test]
fn session_replacement_clears_receive_side_ui_atomically() {
    let mut runtime = populated_runtime(session(1));
    runtime.begin_session(session(2));
    assert!(runtime.chat().messages().is_empty());
    assert!(runtime.hud().title().is_none());
    assert!(runtime.hud().toasts().is_empty());
}

#[test]
fn chat_focus_requests_context_and_router_releases_gameplay_actions() {
    let (mut router, transition) = open_chat_with_held_move_and_attack();
    assert!(transition.ui_consumed_text());
    assert_eq!(transition.requested_input_context(), InputContext::UiFocused);
    router.set_context(transition.requested_input_context());
    let snapshot = router.finalize().unwrap();
    assert!(snapshot.phases[Action::MoveForward as usize].released);
    assert!(snapshot.phases[Action::Attack as usize].released);
}
```

- [ ] **Step 2: Run tests and verify they fail**

```powershell
cargo test -p ui --locked --test hud --test chat -- --nocapture
cargo test -p bedrock-client --locked ui_runtime -- --nocapture
```

Expected: FAIL because the stores/runtime are absent.

- [ ] **Step 3: Implement bounded stores**

```rust
pub const MAX_CHAT_MESSAGES: usize = 512;
pub const MAX_CHAT_RETAINED_BYTES: usize = 1_048_576;
pub const MAX_TOASTS: usize = 32;

pub struct HudStore {
    pub health: BoundedStat,
    pub hunger: BoundedStat,
    pub armor: BoundedStat,
    pub air: BoundedStat,
    pub title: Option<TimedText>,
    pub actionbar: Option<TimedText>,
}
```

Qualify every apply operation by session/FIFO identity, coalesce replaceable
HUD state, retain ordered chat/toasts within count and byte caps, use server
ticks where present and monotonic local time otherwise, and clear all state in
one session replacement operation. Build nodes from snapshots; never let draw
code mutate the stores.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test hud --test chat -- --nocapture
cargo test -p bedrock-client --locked ui_runtime -- --nocapture
cargo clippy -p ui -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui app/src/ui_runtime.rs app/src/ui_runtime
git commit -m "feat(ui): retain server HUD and chat state"
```

### Task 8: Add bounded interactive chat and ordered sends

**Files:**
- Modify: `crates/ui/src/chat.rs`
- Modify: `crates/protocol/src/ui.rs`
- Create: `crates/protocol/tests/chat_send.rs`
- Create: `crates/ui/tests/chat_edit.rs`
- Modify: `app/src/ui_runtime.rs`

**Interfaces:**
- Consumes: UI text actions, clipboard adapter, session-aware network handle.
- Produces: `ChatEditor`, `ChatSendRequest`, exact outbound `Text` packet.

- [ ] **Step 1: Write UTF-8 editing and rate-limit tests**

```rust
#[test]
fn editor_never_splits_utf8_and_caps_bytes() {
    let mut editor = ChatEditor::new(16);
    editor.insert("a🙂bc").unwrap();
    editor.move_left();
    editor.backspace();
    assert!(core::str::from_utf8(editor.bytes()).is_ok());
    assert!(editor.len_bytes() <= 16);
}

#[test]
fn sends_preserve_fifo_and_reject_spam_without_reordering() {
    let mut queue = ChatSendQueue::new(4, rate(2, 1));
    assert_eq!(queue.push(session(7), "one").unwrap().sequence, 0);
    assert_eq!(queue.push(session(7), "two").unwrap().sequence, 1);
    assert!(matches!(queue.push(session(7), "three"), Err(ChatSendError::RateLimited)));
}

#[test]
fn autocomplete_ignores_stale_responses_and_bounds_suggestions() {
    let mut state = ChatAutocompleteState::new();
    let first = state.request("/gi", 3).unwrap();
    let second = state.request("/give", 5).unwrap();
    assert!(!state.apply(response(first.request_id, &["/give"])).unwrap());
    assert!(state.apply(response(second.request_id, &["/give", "/gamerule"])).unwrap());
    assert_eq!(state.suggestions(), ["/give", "/gamerule"]);
}
```

- [ ] **Step 2: Run tests and verify they fail**

```powershell
cargo test -p ui --locked --test chat_edit -- --nocapture
cargo test -p protocol --locked --test chat_send -- --nocapture
```

Expected: FAIL because editing/send builders are absent.

- [ ] **Step 3: Implement editing and exact outbound encoding**

```rust
pub const MAX_CHAT_INPUT_BYTES: usize = 512;
pub const MAX_CHAT_HISTORY: usize = 128;
pub const MAX_CHAT_AUTOCOMPLETE: usize = 256;
pub const MAX_CHAT_AUTOCOMPLETE_BYTES: usize = 65_536;

pub struct ChatSendRequest {
    pub session: u64,
    pub sequence: u64,
    pub message: std::sync::Arc<str>,
}

pub struct ChatAutocompleteRequest {
    pub session: u64,
    pub input_revision: u64,
    pub request_id: u64,
    pub cursor_byte: u16,
    pub input: std::sync::Arc<str>,
}
```

Index cursor/selection only at UTF-8 character boundaries, cap clipboard paste
before allocation, keep bounded history with duplicate coalescing, and build
the protocol-1001 text packet from the vendor-neutral request. On each changed
input revision, issue at most one exact autocomplete request; retain at most
256 suggestions/65,536 UTF-8 bytes, apply only the latest matching request ID,
support keyboard/controller/touch selection through `UiAction`, and clear on
send, close, or session replacement. Session changes drop unsent old-session
messages with an attributable count; backpressure does not reorder accepted
messages.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test chat_edit -- --nocapture
cargo test -p protocol --locked --test chat_send -- --nocapture
cargo clippy -p ui -p protocol --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui crates/protocol app/src/ui_runtime.rs
git commit -m "feat(ui): send bounded interactive chat"
```

### Task 9: Retain and render scoreboard objectives and boss bars

**Files:**
- Create: `crates/ui/src/scoreboard.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/scoreboard.rs`
- Modify: `app/src/ui_runtime.rs`

**Interfaces:**
- Consumes: sequenced objective/score/boss `UiEvent`s.
- Produces: `ScoreboardStore`, `BossBarStore`, stable sidebar/list/below-name
  and boss-stack views.

- [ ] **Step 1: Write replacement, ordering, and removal tests**

```rust
#[test]
fn scores_order_by_value_then_stable_identity() {
    let store = fixture_scores(&[("b", 10), ("a", 10), ("c", 9)]);
    assert_eq!(store.sidebar_rows(), ["a", "b", "c"]);
}

#[test]
fn boss_updates_and_removals_do_not_leak_stale_rows() {
    let mut store = BossBarStore::default();
    store.apply(add_boss(4, "Dragon", 1.0)).unwrap();
    store.apply(update_boss_health(4, 0.5)).unwrap();
    store.apply(remove_boss(4)).unwrap();
    assert!(store.rows().is_empty());
}

#[test]
fn boss_style_color_overlay_and_flags_update_atomically() {
    let mut store = BossBarStore::default();
    store.apply(add_styled_boss(7, BossColor::Purple, BossOverlay::Notched10, true, false)).unwrap();
    let row = store.rows().first().unwrap();
    assert_eq!(row.style.color, BossColor::Purple);
    assert_eq!(row.style.overlay, BossOverlay::Notched10);
    assert!(row.style.darken_sky);
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p ui --locked --test scoreboard -- --nocapture
```

Expected: FAIL because stores are absent.

- [ ] **Step 3: Implement bounded independent lifecycles**

```rust
pub const MAX_OBJECTIVES: usize = 128;
pub const MAX_SCORES: usize = 8_192;
pub const MAX_BOSS_BARS: usize = 64;

pub struct ScoreIdentity { pub objective: Arc<str>, pub entry_id: i64 }
pub struct BossIdentity(pub i64);
```

Keep objective, display-slot, score, and boss lifecycles separate; reject
updates to absent required identities with diagnostics; sort sidebar scores by
server sort order/value and stable entry identity; cap visible boss rows and
retain no removed text. Titles/actionbar and boss bars coexist in distinct
layout regions. Render the normalized boss color and notch overlay exactly;
sky-darkening/fog flags are forwarded as explicit environment requests and are
cleared on removal/session replacement.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test scoreboard -- --nocapture
cargo test -p ui --locked
cargo clippy -p ui --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui app/src/ui_runtime.rs
git commit -m "feat(ui): render scoreboards and boss bars"
```

### Task 10: Normalize inventory, equipment selection, and stack responses

**Files:**
- Create: `crates/protocol/src/inventory.rs`
- Request integration exports/dispatch: `crates/protocol/src/lib.rs`,
  `crates/protocol/src/world.rs`
- Create: `crates/protocol/tests/inventory_packets.rs`
- Create: `app/src/ui_runtime/inventory_router.rs`
- Extend fixture generator: `tools/fixturegen`

**Interfaces:**
- Consumes: StartGame inventory-authority flag, InventoryContent,
  InventorySlot, PlayerHotbar, ItemStackResponse, container open/close/data
  packets, and the already-normalized Phase 4 `WorldEvent::Equipment` stream.
- Produces: `InventoryAuthority`, `InventoryEvent`, `ContainerIdentity`,
  `SlotIdentity`, and events containing Phase 4 `NetworkItemStack`. Protocol
  does not construct `assets::ItemStackIdentity` and does not import `assets`.

- [ ] **Step 1: Write packet fixture and bounds tests**

```rust
#[test]
fn content_slot_hotbar_and_response_packets_normalize() {
    assert!(matches!(fixture("inventory_content.bin"), InventoryEvent::Content(_)));
    assert!(matches!(fixture("inventory_slot.bin"), InventoryEvent::Slot(_)));
    assert!(matches!(fixture("player_hotbar.bin"), InventoryEvent::SelectedSlot(_)));
    assert!(matches!(fixture("item_stack_response.bin"), InventoryEvent::Response(_)));
}

#[test]
fn invalid_slots_nbt_and_collection_sizes_are_rejected() {
    assert!(matches!(normalize_slot(-1), Err(InventoryPacketError::InvalidSlot(-1))));
    assert!(matches!(normalize_content(4_097), Err(InventoryPacketError::TooManySlots { .. })));
    assert!(matches!(normalize_item_with_nbt(1_048_577), Err(InventoryPacketError::ItemNbtTooLarge { .. })));
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p protocol --locked --test inventory_packets -- --nocapture
```

Expected: FAIL because normalization is absent.

- [ ] **Step 3: Implement bounded identity-preserving normalization**

```rust
pub const MAX_CONTAINER_SLOTS: usize = 4_096;
pub const MAX_ITEM_NBT_BYTES: usize = 1_048_576;
pub const MAX_STACK_RESPONSES: usize = 512;
pub const MAX_RESPONSE_CONTAINERS: usize = 128;

pub enum InventoryEvent {
    Authority(InventoryAuthority), Content(InventoryContentEvent),
    Slot(InventorySlotEvent), SelectedSlot(SelectedSlotEvent),
    Response(ItemStackResponseEvent), Open(ContainerOpenEvent),
    Close(ContainerCloseEvent), Data(ContainerDataEvent),
}

pub struct VerifiedNetworkItemStack {
    inner: NetworkItemStack,
}

impl VerifiedNetworkItemStack {
    pub fn try_new(
        stack: NetworkItemStack,
        expected_digest: [u8; 32],
    ) -> Result<Self, InventoryPacketError>;
}
```

Reuse the Phase 4 `NetworkItemStack` in every content, slot, response, and
equipment payload. Its constructor bounds `extra_data` at 64 KiB, computes
`nbt_digest`, and rejects a supplied digest that does not match the
retained bytes. Checked-convert slots/counts/network IDs and preserve packet
order. Integration attaches `WorldEvent::Inventory` and the already-canonical
`WorldEvent::Equipment`; there is no second `MobEquipment` match arm or
Phase-5-specific equipment event.

`VerifiedNetworkItemStack` exposes read-only field accessors and an internal
consuming vendor-conversion method, but never its inner stack. `try_new`
rejects more than 64 KiB, recomputes SHA-256 over the retained extra/NBT bytes,
and requires equality with both `stack.nbt_digest` and `expected_digest`.
Outbound packet request APIs accept only this verified wrapper.

`inventory_router` waits until StartGame/local-player bootstrap publishes the
local runtime identity. It then fans out each `WorldEvent::Equipment` exactly
once: the matching local actor updates selected-equipment authority, all other
actors update the Phase 4 actor store. Buffered pre-identity equipment is
bounded and FIFO, and session replacement clears it. Tests prove one packet
cannot reach both consumers.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p protocol --locked --test inventory_packets -- --nocapture
cargo test -p protocol --locked
cargo clippy -p protocol --all-targets --locked -- -D warnings
Push-Location tools/fixturegen
try { $env:GOWORK='off'; go test ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
cargo fmt --all -- --check
git add crates/protocol/src/inventory.rs crates/protocol/tests/inventory_packets.rs app/src/ui_runtime/inventory_router.rs tools/fixturegen
git commit -m "feat(protocol): normalize inventory authority events"
```

### Task 11: Implement authoritative inventory state and exact rollback

**Files:**
- Create: `crates/ui/src/inventory.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/inventory.rs`
- Modify: `app/src/ui_runtime.rs`

**Interfaces:**
- Consumes: sequenced `InventoryEvent<NetworkItemStack>` and the canonical
  Phase 4 item identity/visual resolver.
- Produces: `InventoryStore`, `SelectedItemSnapshot`, `PendingStackRequest`,
  `AuthoritativeItemWireStack`, and `InventoryRevision` consumed by the Phase
  4 presentation checkpoint.

- [ ] **Step 1: Write lifecycle, reconciliation, and rollback tests**

```rust
#[test]
fn content_then_delta_and_hotbar_share_one_selected_identity() {
    let mut store = InventoryStore::new(session(1));
    store.apply(content_with_slot(0, stack(10, 1))).unwrap();
    store.apply(select_hotbar(0)).unwrap();
    store.apply(slot_update(0, stack(11, 2))).unwrap();
    assert_eq!(store.selected().stack.unwrap(), stack(11, 2).identity);
}

#[test]
fn rejected_request_restores_exact_authoritative_snapshot() {
    let mut store = inventory_with_two_slots();
    let before = store.snapshot();
    let request = store.predict_swap(0, 1).unwrap();
    store.apply(reject_response(request.id)).unwrap();
    assert_eq!(store.snapshot(), before);
    assert_eq!(store.pending_count(), 0);
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p ui --locked --test inventory -- --nocapture
```

Expected: FAIL because the store is absent.

- [ ] **Step 3: Implement transaction-journal authority**

```rust
pub const MAX_OPEN_CONTAINERS: usize = 16;
pub const MAX_PENDING_STACK_REQUESTS: usize = 64;
pub const MAX_INVENTORY_RETAINED_BYTES: usize = 8 * 1024 * 1024;

pub struct SelectedItemSnapshot {
    pub session_id: u64,
    pub inventory_revision: u64,
    pub selected_slot: u8,
    pub stack: Option<ItemStackIdentity>,
    pub visual: ItemVisualRoute,
}

pub struct AuthoritativeItemWireStack {
    identity: ItemStackIdentity,
    wire: protocol::NetworkItemStack,
}
```

Full content replaces a container atomically; slot deltas require a live
container; selected slot is `0..=8`; pending requests retain the exact prestate
and expected poststate within a FIFO journal. Accepted responses reconcile
server slots; rejected/invalidated responses restore the last authoritative
snapshot, not a predicted predecessor. Session/dimension/respawn replacement
clears containers, requests, and selected presentation atomically.

The app performs the only conversion outside protocol:
`NetworkItemStack -> AuthoritativeItemWireStack`. Its checked constructor
rechecks the
64 KiB bound and SHA-256 over `wire.extra_data`, resolves the canonical
`ItemStackIdentity`, and binds that identity to the exact network ID, count,
metadata, stack network ID (`-1` means absent), block runtime ID, and bytes. Slots retain
this authoritative wire stack; `SelectedItemSnapshot` exposes the immutable
identity/visual projection (including the frozen Phase 4 `ItemIconRef`), while
public accessors expose identity/visual metadata but never mutable or raw wire
bytes. `InventoryStore::verified_selected_wire` returns a protocol
`VerifiedNetworkItemStack` only after matching session, inventory revision,
selected slot, and identity and passing `identity.nbt_digest` to the verified
constructor.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test inventory -- --nocapture
cargo test -p ui --locked
cargo clippy -p ui --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui app/src/ui_runtime.rs
git commit -m "feat(ui): reconcile authoritative inventory state"
```

### Task 12: Add deterministic vanilla entity targeting and attack packets

**Files:**
- Create: `crates/protocol/src/interaction.rs`
- Request integration export: `crates/protocol/src/lib.rs`
- Create: `crates/protocol/tests/interaction.rs`
- Create: `app/src/interaction.rs`
- Create: `app/src/interaction/tests.rs`

**Interfaces:**
- Consumes: Phase 3 `InteractionOriginSnapshot` and `PerspectiveMode`, actor
  snapshots/bounds, authoritative selected stack, one semantic
  `Action::Attack` press.
- Produces: `AttackDecision`, `InteractionRequest`, exact protocol-1001 attack
  packet, and `LocalActionReconciliation` for Phase 4 presentation.

- [ ] **Step 1: Write nearest-intercept and occlusion tests**

```rust
#[test]
fn nearest_box_wins_and_a_nearer_solid_block_occludes() {
    let snapshot = combat_snapshot_with_targets([(7, 2.5), (8, 3.0)]);
    assert_eq!(resolve_attack(snapshot.clone()).unwrap().target_runtime_id, 7);
    assert_eq!(resolve_attack(snapshot.with_solid_block_at(2.0)), None);
}

#[test]
fn third_person_camera_offset_never_changes_attack_origin() {
    let first = resolve_attack(snapshot(PerspectiveMode::FirstPerson)).unwrap();
    let rear = resolve_attack(snapshot(PerspectiveMode::ThirdPersonBack)).unwrap();
    assert_eq!(first.ray_origin, rear.ray_origin);
    assert_eq!(first.target_runtime_id, rear.target_runtime_id);
}

#[test]
fn miss_emits_no_entity_transaction() {
    assert!(attack_packet(resolve_attack(empty_snapshot())).is_none());
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p bedrock-client --locked interaction -- --nocapture
cargo test -p protocol --locked --test interaction -- --nocapture
```

Expected: FAIL because target resolution/builders are absent.

- [ ] **Step 3: Implement immutable vanilla combat decisions**

```rust
pub struct CombatSnapshot {
    pub origin: InteractionOriginSnapshot,
    pub game_mode: GameMode,
    pub selected: SelectedItemSnapshot,
    pub actors: Arc<[CombatActor]>,
}

pub struct AttackDecision {
    pub target_runtime_id: u64,
    pub intercept: [f32; 3],
    pub ray_origin: [f32; 3],
    pub distance: f32,
    pub snapshot_identity: AttackSnapshotIdentity,
}
```

Reject non-finite/zero directions, stale actor lifetimes, removed/dead or
server-declared non-attackable targets, unloaded collision space, and rays
beyond native-evidenced game-mode reach. Use slab intersection against reviewed
pose-dependent boxes; choose smallest nonnegative distance then runtime ID.
Raycast solid collision against `origin.world_identity` to the chosen
intercept and reject when its hit is
closer. Never inflate boxes or fluctuate reach.

- [ ] **Step 4: Encode the exact attack transaction**

```rust
use valentine::bedrock::version::v1_26_30::{
    InventoryTransactionPacket, ItemV4, ItemV4NetIdVariant,
    ItemV4NetIdVariantType, Transaction, TransactionLegacy,
    TransactionTransactionData, TransactionTransactionDataItemUseOnEntity,
    TransactionTransactionDataItemUseOnEntityActionType, TransactionTransactionType,
    Vec3F,
};

pub struct AttackPacketRequest {
    pub target_runtime_id: u64,
    pub hotbar_slot: u8,
    pub held_item: VerifiedNetworkItemStack,
    pub player_position: [f32; 3],
    pub click_position: [f32; 3],
}

pub fn attack_entity(request: AttackPacketRequest) -> Result<Packet, InteractionPacketError> {
    let entity_runtime_id = i64::try_from(request.target_runtime_id)
        .map_err(|_| InteractionPacketError::TargetOutOfRange(request.target_runtime_id))?;
    if request.hotbar_slot > 8 {
        return Err(InteractionPacketError::InvalidHotbarSlot(request.hotbar_slot));
    }
    if request.player_position.into_iter().chain(request.click_position).any(|v| !v.is_finite()) {
        return Err(InteractionPacketError::NonFinitePosition);
    }
    let vec3 = |value: [f32; 3]| Vec3F { x: value[0], y: value[1], z: value[2] };
    Ok(InventoryTransactionPacket {
        transaction: Transaction {
            legacy: TransactionLegacy::default(),
            transaction_type: Some(TransactionTransactionType::ItemUseOnEntity),
            actions: Some(Vec::new()),
            transaction_data: Some(TransactionTransactionData::ItemUseOnEntity(Box::new(
                TransactionTransactionDataItemUseOnEntity {
                    entity_runtime_id,
                    action_type: TransactionTransactionDataItemUseOnEntityActionType::Attack,
                    hotbar_slot: i32::from(request.hotbar_slot),
                    held_item: item_v4_from_network_stack(request.held_item)?,
                    player_pos: vec3(request.player_position),
                    click_pos: vec3(request.click_position),
                },
            ))),
        },
    }.into())
}
```

Immediately before the builder call, the app asks
`InventoryStore::verified_selected_wire` for the
`VerifiedNetworkItemStack` matching the decision's inventory revision,
selected slot, and `ItemStackIdentity`; neither arbitrary bytes nor a raw
`NetworkItemStack` satisfy the packet API. The protocol builder validates
`hotbar_slot <= 8` and that the target fits the generated signed runtime-ID
field. The verified wrapper has already rechecked the 64 KiB bound and SHA-256
binding. The builder then performs the one vendor conversion,
`VerifiedNetworkItemStack -> ItemV4`, with checked
numeric conversions and finite positions. A miss returns no packet and only
advances the native missed-swing visual timeline. One `Action::Attack` press
produces at most one request sequence.

- [ ] **Step 5: Verify vanilla-only policy and commit**

```powershell
cargo test -p bedrock-client --locked interaction -- --nocapture
cargo test -p protocol --locked --test interaction -- --nocapture
cargo clippy -p bedrock-client -p protocol --all-targets --locked -- -D warnings
cargo fmt --all -- --check
rg -n "ExtendedReach|ReachFluctuation|AutoClicker|KillAura|LunarCombat" app/src crates/protocol/src crates/ui/src
git diff --check
git add app/src/interaction.rs app/src/interaction crates/protocol
git commit -m "feat: send strictly vanilla entity attacks"
```

Expected: tests pass and `rg` returns no production match.

### Task 13: Implement block interaction, item use, hotbar, and containers

**Files:**
- Modify: `crates/protocol/src/interaction.rs`
- Modify: `crates/ui/src/inventory.rs`
- Create: `crates/ui/src/inventory_view.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/inventory_view.rs`
- Modify: `app/src/interaction.rs`
- Extend tests: `crates/protocol/tests/interaction.rs`,
  `app/src/interaction/tests.rs`
- Create: `crates/render/src/block_crack.rs`
- Create: `crates/render/src/block_crack.wgsl`
- Create: `crates/render/tests/block_crack.rs`
- Request integration export: `crates/render/src/lib.rs`

**Interfaces:**
- Consumes: semantic Attack/Use/Hotbar actions, collision snapshot, inventory
  authority, canonical item visuals, and normalized
  `WorldEvent::BlockCrack` events.
- Produces: break/place/use action requests, the single local
  `LocalActionTimelineSnapshot`, and survival/creative/chest/furnace/crafting
  UI nodes, plus the server-authoritative block-crack overlay.

- [ ] **Step 1: Write interaction and rollback matrix tests**

```rust
#[test]
fn holding_attack_advances_one_block_break_and_releasing_aborts() {
    let mut actions = fixture_actions();
    actions.hold_attack(block_hit([1, 64, 1], 2));
    assert_eq!(actions.tick().unwrap().kind, ActionKind::ContinueBreak);
    assert_eq!(actions.release_attack().unwrap().kind, ActionKind::AbortBreak);
}

#[test]
fn rejected_place_or_use_restores_inventory_and_visual_timeline() {
    let mut fixture = predicted_place_fixture();
    let before = fixture.authoritative_snapshot();
    fixture.reject_server_response();
    assert_eq!(fixture.inventory_snapshot(), before);
    assert!(fixture.provisional_action().is_none());
}

#[test]
fn authoritative_crack_progress_replaces_provisional_progress_and_stop_clears() {
    let mut cracks = block_crack_fixture();
    cracks.begin_provisional([1, 64, 1], 2);
    cracks.apply(server_crack_progress([1, 64, 1], 7));
    assert_eq!(cracks.visible_stage([1, 64, 1]), Some(7));
    cracks.apply(server_crack_stop([1, 64, 1]));
    assert_eq!(cracks.visible_stage([1, 64, 1]), None);
}

#[test]
fn crack_overlay_is_bounded_and_allocates_no_per_crack_gpu_resources() {
    let before = fixture_gpu_resource_counts();
    let report = render_many_authoritative_cracks(MAX_BLOCK_CRACKS + 1);
    let after = fixture_gpu_resource_counts();
    assert_eq!(report.retained_cracks, MAX_BLOCK_CRACKS);
    assert_eq!(after.pipelines, before.pipelines);
    assert_eq!(after.bind_group_families, before.bind_group_families);
    assert_eq!(after.per_crack_allocations, 0);
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p bedrock-client --locked block_interaction -- --nocapture
cargo test -p ui --locked --test inventory_view -- --nocapture
cargo test -p protocol --locked --test interaction -- --nocapture
cargo test -p render --locked --test block_crack -- --nocapture
```

Expected: FAIL because the action state/container views are absent.

- [ ] **Step 3: Implement bounded action state and packet builders**

```rust
pub enum ActionKind {
    StartBreak, ContinueBreak, AbortBreak, Place, UseAir, UseBlock, UseEntity,
}

pub struct ProvisionalAction {
    pub sequence: u64,
    pub session: u64,
    pub origin: InteractionOriginSnapshot,
    pub selected_revision: u64,
    pub kind: ActionKind,
    pub started_tick: u64,
}

pub enum LocalItemAction { Attack, Break, Place, Use }
pub enum ActionCancelReason {
    InputReleased, UiFocus, TargetChanged, SelectedItemChanged,
    WorldReplaced, SessionReplaced, ServerRejected,
}

pub enum LocalActionReconciliation {
    Predicted { action_id: u64, kind: LocalItemAction, source_tick: u64 },
    Confirmed { action_id: u64, authoritative_tick: u64 },
    Rejected { action_id: u64, authoritative_tick: u64 },
    Cancelled { action_id: u64, reason: ActionCancelReason },
}

pub struct LocalActionTimelineSnapshot {
    pub session_id: u64,
    pub action_id: u64,
    pub revision: u64,
    pub phase: assets::ItemActionPhase,
    pub reconciliation: LocalActionReconciliation,
}
```

Block rays use authoritative collision shapes and nearest-face ordering. Send
the exact protocol action/transaction form required by protocol 1001. One
`LocalActionTimeline` owns attack, break, place, and use state; it maps every
accepted semantic edge to the canonical Phase 4 `ItemActionPhase` and is the
only producer of `LocalActionTimelineSnapshot`. It may retain mutually
exclusive break/use substates internally, but no second visual or networking
timeline may advance. Dedupe `(session_id, action_id, revision)`; a higher
revision for the same action replaces the phase without replaying start, and a
new action ID restarts from its mapped initial phase. Abort on target, selected
item, world, focus, session, or dimension change and publish the corresponding
`LocalActionReconciliation`. Container views read immutable inventory
snapshots and emit stack requests through the journal; they never mutate slots
directly.

The block-crack renderer consumes only normalized
`WorldEvent::BlockCrack(BlockCrackEvent)`. A server Start or Progress event
replaces any local provisional stage for the same session, dimension, and
block position; Stop removes it immediately. Stages are accepted only in
`0..=9` and are never guessed from elapsed time. Session/world/dimension
replacement or a stale event clears or rejects the entry. Local target
replacement clears only a matching provisional local crack; it never clears
an authoritative server crack, including one produced by another actor.
Authoritative entries persist until server Stop or session/world/dimension
invalidation. One shared atlas, pipeline, bind-group family, and instance buffer
serve at most `MAX_BLOCK_CRACKS = 1024` visible entries; there is no
per-crack GPU allocation. Invalid positions or camera inputs fail closed and
never reach the renderer.

Freeze one mapping in tests: accepted attack/break/place starts at `Windup`;
the authoritative hit/break/place tick advances to `Active`; completion moves
through `Recover` to `Idle`; a held duration-use maps only to `UseHeld`; and
release, focus loss, target/selection/world replacement, or rejection maps to
`Cancelled` before the producer-authored return to `Idle`. No consumer remaps
these phases.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p bedrock-client --locked block_interaction -- --nocapture
cargo test -p ui --locked --test inventory_view -- --nocapture
cargo test -p protocol --locked --test interaction -- --nocapture
cargo test -p render --locked --test block_crack -- --nocapture
cargo clippy -p bedrock-client -p ui -p protocol --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add app/src/interaction.rs app/src/interaction crates/protocol crates/ui crates/render/src/block_crack.rs crates/render/src/block_crack.wgsl crates/render/tests/block_crack.rs
git commit -m "feat: add authoritative block and inventory interaction"
git branch completion-phase5-authority HEAD
```

Expected: PASS and an immutable authority-producer branch containing Tasks
10-13. Integration merges this branch before asking Phase 4 presentation to
consume the selected-item and local-action snapshots.

### Task 14: Parse, navigate, and answer bounded server forms

**Files:**
- Create: `crates/ui/src/forms.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/forms.rs`
- Modify: `crates/protocol/src/ui.rs`
- Create: `crates/protocol/tests/form_response.rs`
- Modify: `app/src/ui_runtime.rs`

**Interfaces:**
- Consumes: `FormRequestEvent`, UI actions, bounded editor controls.
- Produces: `FormStore`, modal/menu/custom UI trees, and exact
  session-qualified response/cancel packets.

- [ ] **Step 1: Write schema, bounds, navigation, and response tests**

```rust
#[test]
fn all_three_form_kinds_parse_and_round_trip_responses() {
    assert_eq!(parse(modal_fixture()).unwrap().kind(), FormKind::Modal);
    assert_eq!(parse(menu_fixture()).unwrap().kind(), FormKind::Menu);
    let custom = parse(custom_fixture()).unwrap();
    assert_eq!(custom.response_json(selected_values()).unwrap(), r#"[true,2,"ok"]"#);
}

#[test]
fn deep_or_oversized_json_produces_safe_cancellation() {
    assert_eq!(parse(deep_json(65)).unwrap_err().safe_response(), FormResponse::Cancelled);
    assert_eq!(parse(options_json(4_097)).unwrap_err().safe_response(), FormResponse::Cancelled);
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p ui --locked --test forms -- --nocapture
cargo test -p protocol --locked --test form_response -- --nocapture
```

Expected: FAIL because form parsing/response encoding is absent.

- [ ] **Step 3: Implement strict bounded form models**

```rust
pub const MAX_FORM_DEPTH: usize = 64;
pub const MAX_FORM_CONTROLS: usize = 1_024;
pub const MAX_FORM_OPTIONS: usize = 4_096;
pub const MAX_FORM_RESPONSE_BYTES: usize = 1_048_576;

pub enum FormModel { Modal(ModalForm), Menu(MenuForm), Custom(CustomForm) }
```

Use a byte-capped JSON deserializer, reject duplicate required keys and wrong
types, ignore only unknown optional fields, validate slider/dropdown indices,
retain exactly one active form per ID/session, and respond once. Session change,
server replacement, Escape, or malformed required structure yields a safe
cancel response with the original form ID only.

- [ ] **Step 4: Verify Lunar ClickUI-compatible navigation and commit**

```powershell
cargo test -p ui --locked --test forms -- --nocapture
cargo test -p protocol --locked --test form_response -- --nocapture
cargo clippy -p ui -p protocol --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui crates/protocol app/src/ui_runtime.rs
git commit -m "feat(ui): handle bounded Bedrock server forms"
```

### Task 15: Validate, migrate, and atomically persist typed user settings

**Files:**
- Modify: `crates/ui/src/settings.rs`
- Create: `crates/ui/tests/settings.rs`
- Create: `app/src/settings_runtime.rs`
- Create: `app/src/settings_runtime/tests.rs`
- Request integration edit: app module registration and acceptance override
  wiring

**Interfaces:**
- Consumes: Phase 3 semantic binding catalog/camera modes and implemented
  render/environment controls, including Phase 3 `PerspectiveMode` and Phase
  2 `assets::{CloudQuality, PrecipitationQuality,
  EnvironmentQualitySettings}`.
- Produces: validated `UserSettings`, `SettingsPatch`, `SettingsStore`, and
  `RuntimeSettingsUpdate`; settings never directly mutate device/network state.

- [ ] **Step 1: Write defaults, range, migration, and corruption tests**

```rust
#[test]
fn settings_round_trip_and_migrate_v1_to_current() {
    let current = UserSettings::default();
    assert_eq!(decode_settings(&encode_settings(&current).unwrap()).unwrap(), current);
    let migrated = decode_settings(br#"{"schema_version":1,"fov":90.0}"#).unwrap();
    assert_eq!(migrated.schema_version, CURRENT_SETTINGS_SCHEMA);
    assert_eq!(migrated.video.horizontal_fov_degrees, 90.0);
}

#[test]
fn malformed_or_out_of_range_settings_use_attributed_defaults() {
    let loaded = load_settings_bytes(br#"{"schema_version":2,"video":{"horizontal_fov_degrees":9999}}"#);
    assert_eq!(loaded.settings, UserSettings::default());
    assert_eq!(loaded.diagnostic, Some(SettingsDiagnostic::ValidationFailed));
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p ui --locked --test settings -- --nocapture
cargo test -p bedrock-client --locked settings_runtime -- --nocapture
```

Expected: FAIL because validation/persistence is absent.

- [ ] **Step 3: Implement the exact typed schema**

```rust
pub const CURRENT_SETTINGS_SCHEMA: u32 = 2;
pub const MAX_BINDINGS: usize = 128;
pub const MAX_SETTINGS_BYTES: usize = 262_144;

pub struct UserSettings {
    pub schema_version: u32,
    pub controls: semantic_input::ControlSettings,
    pub video: VideoSettings,
    pub gameplay: GameplaySettings,
}

pub struct GameplaySettings {
    pub default_perspective: semantic_input::PerspectiveMode,
}

pub struct VideoSettings {
    pub horizontal_fov_degrees: f32,
    pub fullscreen: bool,
    pub frame_cap: Option<u16>,
    pub vsync: bool,
    pub ui_scale: f32,
    pub render_distance_chunks: u8,
    pub brightness: f32,
    pub cloud_quality: assets::CloudQuality,
    pub precipitation_quality: assets::PrecipitationQuality,
}
```

Validate FOV `30..=120`, UI scale `0.5..=4`, render distance `2..=96`,
brightness `0..=1`, sensitivity `0.01..=10`, deadzones `0..=0.95`, finite
floats, unique action/device/chord bindings, and exactly the three imported
`semantic_input::PerspectiveMode` variants. Deserialize
from at most 256 KiB with `deny_unknown_fields` for required typed sections;
migrate schema 1 explicitly and reject future schemas to attributed defaults.
Persist the Phase 2 enum variants directly; do not introduce numeric cloud or
weather quality mirrors. The app submits the pair only through Phase 2
`EnvironmentQualitySettings`, leaving validation and GPU semantics in render.

- [ ] **Step 4: Implement platform path and atomic replacement**

```rust
pub fn settings_path(config_root: &std::path::Path) -> std::path::PathBuf {
    config_root.join("cinnabar").join("settings-v2.json")
}

pub fn persist_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    write_new_synced(&temporary, bytes)?;
    replace_file(&temporary, path)
}
```

`write_new_synced` creates parent directories, opens the explicit temporary
path with create/truncate/write, writes all bytes, calls `sync_all`, and closes
before `replace_file`. On Windows, `replace_file` uses a same-directory backup
and restores it if replacement fails; on other platforms it uses same-filesystem
rename. Successful load never rewrites the file. Credentials and acceptance
overrides have no fields in `UserSettings`.

- [ ] **Step 5: Verify interruption and commit**

```powershell
cargo test -p ui --locked --test settings -- --nocapture
cargo test -p bedrock-client --locked settings_runtime -- --nocapture
cargo clippy -p ui -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
git add crates/ui app/src/settings_runtime.rs app/src/settings_runtime
git commit -m "feat(settings): persist validated user preferences"
```

### Task 16: Build the in-game menu and apply controls/video settings live

**Files:**
- Create: `crates/ui/src/menu.rs`
- Request integration export: `crates/ui/src/lib.rs`
- Create: `crates/ui/tests/menu.rs`
- Modify: `app/src/ui_runtime.rs`
- Modify: `app/src/settings_runtime.rs`
- Create: `app/src/settings_runtime/apply.rs`

**Interfaces:**
- Consumes: distinct semantic `Action::Menu`/`Action::Back` plus UI actions,
  typed settings, runtime adapters for
  camera, window, renderer, environment, and input bindings.
- Produces: resume/settings/disconnect/quit actions and validated live settings
  patches with apply/cancel/default flows.

- [ ] **Step 1: Write menu authority and live-apply tests**

```rust
#[test]
fn opening_menu_releases_gameplay_and_one_escape_resumes() {
    let (mut router, mut menu) = fixture_closed_menu_with_held_actions();
    let opened = menu.dispatch(UiAction::Cancel).unwrap();
    assert_eq!(opened.requested_input_context, InputContext::UiFocused);
    router.set_context(opened.requested_input_context);
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::MoveForward as usize].released);
    assert!(released.phases[Action::Attack as usize].released);
    assert!(menu.is_open());
    menu.dispatch(UiAction::Cancel).unwrap();
    assert!(!menu.is_open());
}

#[test]
fn apply_changes_runtime_and_cancel_restores_draft_only() {
    let mut fixture = settings_menu_fixture();
    fixture.draft_fov(95.0).unwrap();
    fixture.cancel();
    assert_eq!(fixture.runtime_fov(), fixture.original_fov());
    fixture.draft_fov(95.0).unwrap();
    fixture.apply().unwrap();
    assert_eq!(fixture.runtime_fov(), 95.0);
}

#[test]
fn default_perspective_uses_the_shared_enum_and_applies_live() {
    let mut fixture = settings_menu_fixture();
    fixture
        .draft_default_perspective(semantic_input::PerspectiveMode::ThirdPersonBack)
        .unwrap();
    fixture.apply().unwrap();
    assert_eq!(
        fixture.runtime_perspective(),
        semantic_input::PerspectiveMode::ThirdPersonBack,
    );
}
```

- [ ] **Step 2: Run and verify failure**

```powershell
cargo test -p ui --locked --test menu -- --nocapture
cargo test -p bedrock-client --locked settings_runtime -- --nocapture
```

Expected: FAIL because the menu/apply adapters are absent.

- [ ] **Step 3: Implement complete backed settings screens**

```rust
pub enum GameMenuAction { Resume, OpenSettings, Disconnect, Quit }
pub enum SettingsPage { Controls, Video, Gameplay }

pub enum RuntimeSettingsUpdate {
    Bindings(semantic_input::ControlSettings), HorizontalFov(f32), Fullscreen(bool),
    FrameCap(Option<u16>), Vsync(bool), UiScale(f32), RenderDistance(u8),
    Brightness(f32),
    EnvironmentQuality(assets::EnvironmentQualitySettings),
    DefaultPerspective(PerspectiveMode),
}
```

Build controls for every Phase 3 semantic action and implemented setting only.
Rebinding calls only `SemanticInputRouter::replace_bindings`; the router queues
`ReleaseReason::BindingChanged`, and its `SemanticFinalize` barrier emits the
release before the new binding can become active. UI/settings code never
manufactures a release list. Conflicts use the frozen Phase 3 deterministic
rule shown before Apply. Apply validates the
whole draft, sends updates in stable enum order, rolls back already-applied
updates if any adapter rejects, then persists. Cancel drops the draft. Defaults
create a validated default draft but require Apply.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p ui --locked --test menu -- --nocapture
cargo test -p bedrock-client --locked settings_runtime -- --nocapture
cargo clippy -p ui -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git add crates/ui app/src/ui_runtime.rs app/src/settings_runtime.rs app/src/settings_runtime
git commit -m "feat(settings): add live in-game controls and video menu"
```

### Task 17: Consume Phase 4 presentation and prove full UI performance

**Files:**
- Modify: `app/src/ui_runtime.rs`
- Modify: `app/src/interaction.rs`
- Create: `app/src/ui_runtime/acceptance.rs`
- Create: `scripts/tests/ui-acceptance.Tests.ps1`
- Request integration edits: app schedule/plugin assembly and root manifests

**Interfaces:**
- Consumes: the merged `completion-phase4-presentation` local viewmodel,
  Phase 5-owned `LocalActionTimelineSnapshot`, selected stack,
  equipment/dropped item publication, UI renderer stats, and Phase 2
  `PublicationServiceConfig` diagnostics.
- Produces: cross-system item identity witness and Phase 5.7 performance
  manifest.

- [ ] **Step 1: Write identity and no-churn acceptance tests**

Dependency gate: integration must first merge
`completion-phase5-authority`, then `completion-phase4-presentation`. On the
unpublished Phase 5 continuation only, run:

```powershell
git fetch --all --prune
git rebase completion-integration
git merge-base --is-ancestor completion-phase4-presentation HEAD
```

The last command must exit 0 before any Task 17 edit.

```rust
#[test]
fn selected_item_identity_matches_every_local_consumer() {
    let witness = integrated_item_witness();
    assert_eq!(witness.inventory_identity, witness.hotbar_identity);
    assert_eq!(witness.inventory_identity, witness.viewmodel_identity);
    assert_eq!(witness.inventory_identity, witness.outbound_identity);
}

#[test]
fn active_ui_and_actions_allocate_no_per_action_gpu_resources() {
    let before = fixture_gpu_resource_counts();
    exercise_chat_inventory_forms_combat_and_settings();
    let after = fixture_gpu_resource_counts();
    assert_eq!(after.pipelines, before.pipelines);
    assert_eq!(after.bind_group_families, before.bind_group_families);
    assert_eq!(after.per_action_allocations, 0);
}

#[test]
fn active_server_cracks_share_resources_and_follow_authoritative_stage() {
    let witness = integrated_block_crack_witness();
    assert_eq!(witness.server_stage, witness.rendered_stage);
    assert_eq!(witness.per_crack_gpu_allocations, 0);
    assert!(witness.retained_cracks <= 1024);
}

#[test]
fn settings_overlay_does_not_starve_phase2_publication_or_remesh() {
    let report = run_forced_full_view_remesh_with_overlay(
        PublicationServiceConfig::PHASE2_GATE,
    );
    assert_eq!(report.starved_publication_frames, 0);
    assert!(report.required_remesh_completed_within(Duration::from_secs(2)));
    assert!(report.join_max_latency_ms <= 2_000.0);
    assert!(report.teleport_max_latency_ms <= 2_000.0);
    assert!(report.frame_time_p95_ms <= 16.6666666667);
    assert!(report.frame_time_p99_ms <= 16.6666666667);
    assert!(report.frame_time_max_ms <= 50.0);
}
```

- [ ] **Step 2: Run and verify failure before Phase 4 presentation integration**

```powershell
cargo test -p bedrock-client --locked integrated_item_witness -- --nocapture
$result = Invoke-Pester -Script scripts/tests/ui-acceptance.Tests.ps1 -PassThru
if ($result.FailedCount -ne 0) { exit 1 }
```

Expected: FAIL until the Phase 4 presentation consumer interfaces and acceptance metrics
are integrated.

- [ ] **Step 3: Wire immutable snapshots and acceptance metrics**

```rust
pub struct UiAcceptanceSnapshot {
    pub session: u64,
    pub ui_revision: u64,
    pub inventory_revision: u64,
    pub selected_identity: ItemStackIdentity,
    pub viewmodel_identity: ItemStackIdentity,
    pub outbound_identity: Option<ItemStackIdentity>,
    pub retained_ui_bytes: u64,
    pub ui_vertices: u32,
    pub ui_draw_calls: u32,
    pub per_action_gpu_allocations: u64,
    pub publication_starved_frames: u64,
    pub join_max_latency_ms: f64,
    pub teleport_max_latency_ms: f64,
    pub required_remesh_max_latency_ms: f64,
    pub frame_time_p95_ms: f64,
    pub frame_time_p99_ms: f64,
    pub frame_time_max_ms: f64,
}
```

Reject snapshots whose session/revisions do not match. The release acceptance
manifest records hotbar, viewmodel, multiple equipped actors, dropped item,
combat/use animation, chat, scoreboard, boss bars, inventory, forms, and
settings overlay together with server-authoritative block-crack start,
progress, replacement, and stop in one active run. Run a 30-second warmup followed by a
120-second measured interval; the explicit steady-state gates are frame-time
p95 and p99 `<= 16.6666666667 ms`, max `<= 50.0 ms`, zero publication
starvation, and join, teleport, and required-remesh latency each `<= 2.0 s`
under the Phase 2 gate.

- [ ] **Step 4: Run the Phase 5 deterministic gate and commit**

```powershell
cargo test -p protocol --locked
cargo test -p ui --locked
cargo test -p render --locked
cargo test -p bedrock-client --locked
$result = Invoke-Pester -Script scripts/tests/ui-acceptance.Tests.ps1 -PassThru
if ($result.FailedCount -ne 0) { exit 1 }
cargo clippy -p protocol -p ui -p render -p bedrock-client --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
git add app scripts/tests/ui-acceptance.Tests.ps1
git commit -m "feat(ui): integrate item actions and UI acceptance"
```

Expected: PASS; Task 18 adds the evidence commit before publishing the
immutable final consumer branch.

### Task 18: Run live/native Phase 5 acceptance and final review

**Files:**
- Create: `docs/evidence/phase-5-acceptance.md`
- Update through integration lane: `docs/evidence/phases-2-5-completion-ledger.md`

**Interfaces:**
- Consumes: clean release candidate, local BDS, Lunar 19134, Zeqa 19132,
  matching native client.
- Produces: Phase 5 authoritative evidence or explicit open failures.

- [ ] **Step 1: Run the controlled two-client BDS matrix**

Cycle all nine hotbar slots; replace selected stack; compare local viewmodel
and remote held item; attack one valid actor and miss once; place a block; use
empty hand, melee item, block item, consumable, and duration item; reject one
stack request; start a block break and verify the exact server crack stage,
then verify progress replacement, stop, target change, respawn, and dimension
change clear the overlay; open survival, creative, chest, furnace, crafting, modal, menu,
and custom form surfaces; respawn and change dimension.

Expected: exact authoritative convergence, no duplicate item, stale visual,
invented hit, stuck action, phantom UI input, or unbounded diagnostic.

- [ ] **Step 2: Run authenticated third-party smoke tests**

On Lunar `pvp.lunarbedrock.com:19134` and Zeqa `zeqa.net:19132`, verify chat,
HUD, forms where served, hotbar selection, one permitted ordinary vanilla
attack/use, perspective-independent target origin, settings UI, disconnect,
and reconnect. Record zero extended-reach or automated attacks. Do not retain
credentials or private chat contents.

- [ ] **Step 3: Capture matching native references**

Exercise keyboard/mouse, controller, and touch independently at every
supported UI scale/aspect ratio. For each device, verify focus acquisition,
focus loss, pointer/cursor capture, chat text entry where applicable,
controller disconnect/touch cancellation, UI-consumed actions, semantic
release ordering, and restoration of gameplay authority. Then compare font metrics, formatting,
HUD positions, chat, title/actionbar, scoreboard, boss bars, hotbar, inventory,
forms, first-person hand/item, swing/use extrema and timing, third-person held
item, authoritative block-crack stages and clearing, combat target/reach, FOV,
menu, controls, and focus transitions.

- [ ] **Step 4: Prove performance and resource bounds**

After a 30-second warmup and over a 120-second measurement, the acceptance
reducer must report frame-time p95 and p99 `<= 16.6666666667 ms`, and max
`<= 50.0 ms`; join, teleport, and required-remesh latency each `<= 2.0 s`;
combined client/core RSS at most 650 MB; combined CPU at most
15 percent; bounded UI/font/item stores; zero per-action GPU allocations;
zero per-crack GPU allocations; at most 1024 retained crack entries;
and zero Phase 2 publication starvation while all Phase 5 surfaces and
multiple equipped actors are active.

- [ ] **Step 5: Run final code gates and request review**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location tools/fixturegen
try { $env:GOWORK='off'; go test ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
git diff --check
git status --short
```

Expected: all commands pass and the worktree is clean. Use
`superpowers:requesting-code-review` for the complete Phase 5 behavior range;
address every Important or Critical finding before integration.

- [ ] **Step 6: Commit the reviewed evidence**

Record command output summaries, BDS/native build identities, device/focus
matrix, percentile sample window, publication/remesh results, screenshots or
artifact paths, and every open failure in
`docs/evidence/phase-5-acceptance.md`. The integration lane alone updates the
cross-phase completion ledger. Then run:

```powershell
git add docs/evidence/phase-5-acceptance.md
git commit -m "docs(evidence): record Phase 5 acceptance"
git branch completion-phase5-consumer HEAD
git status --short
```

Expected: the evidence commit exists and `git status --short` is empty before
the final branch is handed to integration. If evidence has an open failure,
do not mark Phase 5 complete.

## Self-Review

- 5.1 maps to Tasks 1-5; 5.2 to Tasks 6-7; 5.3 to Task 8; 5.4 to Task 9;
  5.5 to Tasks 10-13; 5.6 to Task 14; 5.7 to Tasks 17-18; 5.8 to Tasks 15-16.
- Vanilla combat uses an immutable eye/world/actor/item snapshot, nearest-box
  ordering, solid-block occlusion, native game-mode reach, one packet per
  physical press, and no Lunar module behaviour.
- The inventory store is the sole selected-stack authority and exact rollback
  source; Phase 4 presentation consumes it without owning inventory state.
- UI focus, window focus loss, controller disconnect, rebinding, session
  replacement, and dimension changes emit releases or clear provisional state
  before another authority consumes input.
- All external text, JSON, NBT, arrays, caches, stores, journals, vertices,
  indices, batches, and GPU resources have explicit limits and attribution.
- Audio, account, server browser, Realms, friends, and resource-pack management
  remain outside this bounded Phase 5 plan.
