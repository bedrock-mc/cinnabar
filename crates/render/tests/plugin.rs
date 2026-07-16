use std::{
    fs,
    mem::size_of,
    path::Path,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use asset_compiler::compile_pack as compile_pack_with_lights;
use assets::{
    ANIMATION_FLAG_BLEND, Animation, AssetError, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_CUTOUT,
    MODEL_QUAD_FLAG_TWO_SIDED, Material, ModelStateField, NO_ANIMATION, NO_MODEL_TEMPLATE,
    NetworkIdMode, RegistryRecord, RuntimeAssets, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, encode_blob, read_registry,
};

fn compile_pack(root: &Path, records: &[RegistryRecord]) -> Result<CompiledAssets, AssetError> {
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap_or(0)
    ];
    compile_pack_with_lights(root, records, &lights)
}
use bevy::{
    camera::primitives::Aabb,
    prelude::{App, Mat4, MinimalPlugins, Quat, Vec3, Visibility},
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use meshing::{
    BlockClassifier, Face, Neighbourhood, PackedBiomeRecord, PackedQuad, mesh_sub_chunk,
};
use render::{
    AnimationFrameSample, ChunkAnimationClock, ChunkRenderInstance, ChunkRenderQueue,
    ChunkRenderQueueLimits, ChunkTextureAssetIdentity, ChunkUploadPriority, DebugWorldPlugin,
    MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180,
    MATERIAL_UV_ROTATE_270, MAX_TRANSPARENT_DRAW_REFS, MAX_TRANSPARENT_VIEWS,
    PackedTransparentDrawRef, PresentedFrameAck, RenderViewCohort, TRANSPARENT_REF_BUFFER_BYTES,
    TRANSPARENT_REF_SLOT_BYTES, TextureArrayLimits, TextureLimitError, TexturePageBinding,
    TransparentAllocationIdentity, TransparentOrderedSnapshot, TransparentSortCandidate,
    TransparentSortError, TransparentSortJobGate, TransparentSortMetrics,
    TransparentSortMetricsSnapshot, TransparentSortResult, TransparentSortState,
    ViewSortGeneration, ViewSortKey, diagnostic_texture_page,
    direct_transparent_draw_args_for_test, greedy_texture_uv, mdi_transparent_draw_args_for_test,
    plan_texture_mip_uploads, plan_texture_page_bindings, select_animation_frames,
    sort_transparent_candidates_for_test, texture_asset_needs_rebuild,
};
use world::{DecodedBiomeColumn, SubChunk, SubChunkKey};

const AIR: u32 = 12_530;

fn standalone_world_shader(source: &str) -> String {
    let lighting = include_str!("../src/lighting.wgsl").replacen(
        "#define_import_path cinnabar::lighting",
        "",
        1,
    );
    let biome_tint = include_str!("../src/biome_tint.wgsl").replacen(
        "#define_import_path cinnabar::biome_tint",
        "",
        1,
    );
    source
        .replacen(
            "#import bevy_render::view::View",
            "struct View { clip_from_world: mat4x4<f32>, world_position: vec3<f32>, }",
            1,
        )
        .replacen(
            "#import cinnabar::lighting::{light_ao_factor, light_brightness, lit_colour}",
            &lighting,
            1,
        )
        .replacen(
            "#import cinnabar::biome_tint::blended_biome_tint",
            &biome_tint,
            1,
        )
}

fn entry_points_use_binding(module: &naga::Module, stage: naga::ShaderStage, binding: u32) -> bool {
    let Some((handle, _)) = module.global_variables.iter().find(|(_, variable)| {
        variable
            .binding
            .as_ref()
            .is_some_and(|resource| resource.group == 0 && resource.binding == binding)
    }) else {
        return false;
    };
    let mut entries = module
        .entry_points
        .iter()
        .filter(|entry| entry.stage == stage)
        .peekable();
    entries.peek().is_some()
        && entries.all(|entry| function_uses_global(module, &entry.function, handle, 0))
}

fn function_uses_global(
    module: &naga::Module,
    function: &naga::Function,
    global: naga::Handle<naga::GlobalVariable>,
    depth: usize,
) -> bool {
    if function.expressions.iter().any(|(_, expression)| {
        matches!(expression, naga::Expression::GlobalVariable(handle) if *handle == global)
    }) {
        return true;
    }
    if depth >= 16 {
        return false;
    }
    let mut calls = Vec::new();
    collect_function_calls(&function.body, &mut calls);
    calls
        .into_iter()
        .any(|handle| function_uses_global(module, &module.functions[handle], global, depth + 1))
}

fn collect_function_calls(block: &naga::Block, calls: &mut Vec<naga::Handle<naga::Function>>) {
    for statement in block {
        match statement {
            naga::Statement::Call { function, .. } => calls.push(*function),
            naga::Statement::Block(block) => collect_function_calls(block, calls),
            naga::Statement::If { accept, reject, .. } => {
                collect_function_calls(accept, calls);
                collect_function_calls(reject, calls);
            }
            naga::Statement::Switch { cases, .. } => {
                for case in cases {
                    collect_function_calls(&case.body, calls);
                }
            }
            naga::Statement::Loop {
                body, continuing, ..
            } => {
                collect_function_calls(body, calls);
                collect_function_calls(continuing, calls);
            }
            _ => {}
        }
    }
}

fn entry_points_call_function(
    module: &naga::Module,
    stage: naga::ShaderStage,
    function_name: &str,
) -> bool {
    let Some((target, _)) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some(function_name))
    else {
        return false;
    };
    let mut entries = module
        .entry_points
        .iter()
        .filter(|entry| entry.stage == stage)
        .peekable();
    entries.peek().is_some()
        && entries.all(|entry| function_calls(module, &entry.function, target, 0))
}

fn function_calls(
    module: &naga::Module,
    function: &naga::Function,
    target: naga::Handle<naga::Function>,
    depth: usize,
) -> bool {
    if depth >= 16 {
        return false;
    }
    let mut calls = Vec::new();
    collect_function_calls(&function.body, &mut calls);
    calls.contains(&target)
        || calls
            .into_iter()
            .any(|handle| function_calls(module, &module.functions[handle], target, depth + 1))
}

#[test]
fn chunk_sampler_source_contract_is_crisp_for_magnification_and_filtered_for_mips() {
    let source = include_str!("../src/plugin.rs");
    assert!(source.contains("mag_filter: FilterMode::Nearest"));
    assert!(source.contains("min_filter: FilterMode::Linear"));
    assert!(source.contains("mipmap_filter: FilterMode::Linear"));
    assert!(source.contains("anisotropy_clamp: 1"));
}

#[test]
fn graphics_runtime_metadata_waits_for_extracted_diagnostics_before_surface_probe() {
    let source = include_str!("../src/plugin.rs").replace("\r\n", "\n");
    assert!(
        source.contains(
            "publish_graphics_runtime_metadata\n                        .after(RenderSystems::ExtractCommands)\n                        .before(bevy::render::view::window::create_surfaces)"
        ),
        "the metadata probe consumes an ExtractResource and must run after deferred extraction commands but before Bevy creates the surface"
    );
}

#[test]
fn shared_biome_bindings_are_visible_to_vertex_and_fragment_pipelines() {
    let source = include_str!("../src/plugin.rs");
    for binding in [7, 8] {
        let marker = format!("binding: {binding},");
        let start = source
            .find(&marker)
            .unwrap_or_else(|| panic!("missing shared biome binding {binding}"));
        let entry = &source[start
            ..source[start..]
                .find("count: None,")
                .map(|offset| start + offset)
                .expect("bind group entry must retain a count")];
        assert!(
            entry.contains("visibility: ShaderStages::VERTEX_FRAGMENT"),
            "shared biome binding {binding} must support opaque fragment and liquid vertex reads",
        );
    }
}

#[test]
fn fragment_view_reads_are_covered_by_the_shared_chunk_layout() {
    for (name, source) in [
        ("chunk", include_str!("../src/chunk.wgsl")),
        ("model", include_str!("../src/model.wgsl")),
        ("liquid", include_str!("../src/liquid.wgsl")),
    ] {
        let shader = standalone_world_shader(source);
        let module = naga::front::wgsl::parse_str(&shader)
            .unwrap_or_else(|error| panic!("parse {name} WGSL: {error}"));
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("validate {name} WGSL: {error}"));
        let (view_handle, _) = module
            .global_variables
            .iter()
            .find(|(_, global)| {
                global
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.group == 0 && binding.binding == 0)
            })
            .unwrap_or_else(|| panic!("{name} shader has no group 0 binding 0 view uniform"));
        let fragment_reads_view = module
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.stage == naga::ShaderStage::Fragment)
            .any(|(index, _)| !info.get_entry_point(index)[view_handle].is_empty());
        assert!(
            fragment_reads_view,
            "{name} fragment entry points must exercise the shared view binding contract"
        );
    }

    let source = include_str!("../src/plugin.rs");
    let layout_start = source
        .find("chunk vertex-pulling bind group layout")
        .expect("shared chunk layout");
    let binding_start = source[layout_start..]
        .find("binding: 0,")
        .map(|offset| layout_start + offset)
        .expect("shared view binding");
    let entry = &source[binding_start
        ..source[binding_start..]
            .find("count: None,")
            .map(|offset| binding_start + offset)
            .expect("complete shared view layout entry")];
    assert!(
        entry.contains("visibility: ShaderStages::VERTEX_FRAGMENT"),
        "group 0 binding 0 is read by every world fragment family and must be fragment-visible"
    );
}

#[test]
fn sort_ref_ceiling_is_enforced() {
    assert_eq!(size_of::<PackedTransparentDrawRef>(), 8);
    assert_eq!(MAX_TRANSPARENT_DRAW_REFS, 2_097_152);
    assert_eq!(
        render::validate_transparent_sort_ref_count(MAX_TRANSPARENT_DRAW_REFS),
        Ok(())
    );
    assert_eq!(
        render::validate_transparent_sort_ref_count(MAX_TRANSPARENT_DRAW_REFS + 1),
        Err(TransparentSortError::ReferenceCeiling {
            requested: 2_097_153,
            ceiling: 2_097_152,
        })
    );
    let packed = PackedTransparentDrawRef::new(17, 29);
    assert_eq!(packed.liquid_record_index(), 17);
    assert_eq!(packed.metadata_index(), 29);
}

fn allocation(key: SubChunkKey, generation: u64, base: u32) -> TransparentAllocationIdentity {
    TransparentAllocationIdentity::new(
        key,
        generation,
        base..base + 8,
        base + 32..base + 40,
        base / 8,
    )
}

fn sort_key(
    camera: [i32; 3],
    orientation: [i32; 4],
    visible: Vec<TransparentAllocationIdentity>,
    assets: u64,
    tint: u64,
) -> ViewSortKey {
    ViewSortKey::try_new(
        camera.map(|value| value as f32),
        orientation.map(|value| value as f32),
        visible,
        ChunkTextureAssetIdentity::for_test(assets as usize, assets),
        meshing::ChunkBiomeTintIdentity::new(tint, tint),
    )
    .unwrap()
}

fn exact_sort_key(camera: [f32; 3], orientation: [f32; 4]) -> ViewSortKey {
    ViewSortKey::try_new(
        camera,
        orientation,
        vec![],
        ChunkTextureAssetIdentity::for_test(1, 1),
        meshing::ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap()
}

fn sort_result(
    generation: ViewSortGeneration,
    key: ViewSortKey,
    record: u32,
) -> TransparentSortResult {
    TransparentSortResult::new(
        generation,
        key,
        vec![PackedTransparentDrawRef::new(record, record + 100)],
    )
    .unwrap()
}

#[test]
fn older_view_sort_generation_is_rejected() {
    let mut state = TransparentSortState::with_upload_cap(8);
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 3, 8)];
    let first_key = sort_key([0, 0, 0], [0, 0, 0, 1], visible.clone(), 2, 3);
    let first = state.request(&first_key);
    assert_eq!(
        state.request(&first_key),
        first,
        "unchanged outstanding work is reused"
    );
    let rotated_key = sort_key([0, 0, 0], [0, 1, 0, 1], visible, 2, 3);
    let rotated = state.request(&rotated_key);
    assert!(
        first < rotated,
        "camera orientation is part of the exact key"
    );
    assert_eq!(state.complete(sort_result(first, first_key, 1)), Ok(false));
    assert!(state.committed().is_none());
    assert_eq!(
        state.complete(sort_result(rotated, rotated_key, 4)),
        Ok(false)
    );
    assert!(state.next_upload_batch().is_some());
    assert!(state.acknowledge_upload());
    assert_eq!(state.committed().unwrap().generation(), rotated);
}

#[test]
fn last_complete_sort_remains_bound() {
    let mut state = TransparentSortState::with_upload_cap(1);
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let first_key = sort_key([0, 0, 0], [0, 0, 0, 1], visible.clone(), 1, 1);
    let first = state.request(&first_key);
    assert_eq!(state.complete(sort_result(first, first_key, 7)), Ok(false));
    let upload = state.next_upload_batch().unwrap();
    assert_eq!(upload.buffer_slot(), 0);
    assert_eq!(upload.ref_range(), 0..1);
    assert_eq!(upload.refs(), &[PackedTransparentDrawRef::new(7, 107)]);
    assert!(state.acknowledge_upload());
    let committed: TransparentOrderedSnapshot = state.committed().unwrap().clone();
    let second_key = sort_key([1, 0, 0], [0, 0, 0, 1], visible, 1, 1);
    let second = state.request(&second_key);
    assert_eq!(state.committed(), Some(&committed));
    let oversized = TransparentSortResult::new(
        second,
        second_key,
        vec![
            PackedTransparentDrawRef::new(8, 1),
            PackedTransparentDrawRef::new(9, 1),
        ],
    )
    .unwrap();
    assert_eq!(state.complete(oversized), Ok(false));
    assert_eq!(state.committed(), Some(&committed));
    let upload = state.next_upload_batch().unwrap();
    assert_eq!(upload.buffer_slot(), 1);
    assert_eq!(upload.ref_range(), 0..1);
    assert_eq!(upload.refs(), &[PackedTransparentDrawRef::new(8, 1)]);
    assert!(!state.acknowledge_upload());
    assert_eq!(state.committed(), Some(&committed));
    let upload = state.next_upload_batch().unwrap();
    assert_eq!(upload.ref_range(), 1..2);
    assert_eq!(upload.refs(), &[PackedTransparentDrawRef::new(9, 1)]);
    assert!(state.acknowledge_upload());
    let replacement = state.committed().unwrap();
    assert_eq!(replacement.generation(), second);
    assert_ne!(replacement.buffer_slot(), committed.buffer_slot());
}

#[test]
fn unsafe_sort_identity_changes_clear_bound_snapshot() {
    let a = allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8);
    let base = sort_key([0, 0, 0], [0, 0, 0, 1], vec![a.clone()], 10, 20);
    for unsafe_key in [
        sort_key([0, 0, 0], [0, 0, 0, 1], vec![], 10, 20),
        sort_key(
            [0, 0, 0],
            [0, 0, 0, 1],
            vec![allocation(a.key(), 2, 8)],
            10,
            20,
        ),
        sort_key([0, 0, 0], [0, 0, 0, 1], vec![a.clone()], 11, 20),
        sort_key([0, 0, 0], [0, 0, 0, 1], vec![a.clone()], 10, 21),
    ] {
        let mut state = TransparentSortState::with_upload_cap(8);
        let generation = state.request(&base);
        assert_eq!(
            state.complete(sort_result(generation, base.clone(), 1)),
            Ok(false)
        );
        assert!(state.next_upload_batch().is_some());
        assert!(state.acknowledge_upload());
        state.request(&unsafe_key);
        assert!(state.committed().is_none());
        assert_eq!(state.staged_ref_count(), 0);
    }
}

#[test]
fn unsafe_sort_identity_change_discards_partially_staged_refs() {
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let initial = sort_key([0, 0, 0], [0, 0, 0, 1], visible, 10, 20);
    let mut state = TransparentSortState::with_upload_cap(1);
    let generation = state.request(&initial);
    let result = TransparentSortResult::new(
        generation,
        initial,
        vec![
            PackedTransparentDrawRef::new(8, 1),
            PackedTransparentDrawRef::new(9, 1),
        ],
    )
    .unwrap();
    assert_eq!(state.complete(result), Ok(false));
    assert!(state.next_upload_batch().is_some());
    assert!(!state.acknowledge_upload());
    assert_eq!(state.staged_ref_count(), 2);

    let unsafe_key = sort_key([0, 0, 0], [0, 0, 0, 1], vec![], 10, 20);
    state.request(&unsafe_key);
    assert_eq!(state.staged_ref_count(), 0);
    assert!(state.next_upload_batch().is_none());
}

#[test]
fn camera_motion_cannot_starve_a_partially_uploaded_water_sort() {
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let initial = sort_key([0, 0, 0], [0, 0, 0, 1], visible.clone(), 10, 20);
    let mut state = TransparentSortState::with_upload_cap(1);
    let staged_generation = state.request(&initial);
    let refs = vec![
        PackedTransparentDrawRef::new(8, 1),
        PackedTransparentDrawRef::new(9, 1),
    ];
    assert_eq!(
        state.complete(
            TransparentSortResult::new(staged_generation, initial, refs.clone()).unwrap()
        ),
        Ok(false)
    );

    let moved_once = sort_key([1, 0, 0], [0, 0, 0, 1], visible.clone(), 10, 20);
    assert_eq!(
        state.request(&moved_once),
        staged_generation,
        "camera-only motion must finish the bounded inactive-slot upload"
    );
    assert_eq!(state.next_upload_batch().unwrap().refs(), &refs[..1]);
    assert!(!state.acknowledge_upload());

    let moved_again = sort_key([2, 0, 0], [0, 0, 0, 1], visible, 10, 20);
    assert_eq!(state.request(&moved_again), staged_generation);
    assert_eq!(state.next_upload_batch().unwrap().refs(), &refs[1..]);
    assert!(state.acknowledge_upload());
    assert_eq!(state.committed().unwrap().refs(), refs);

    assert!(
        state.request(&moved_again) > staged_generation,
        "the latest camera pose is scheduled immediately after the atomic commit"
    );
}

#[test]
fn visible_sort_manifest_is_canonical_and_reuses_the_outstanding_generation() {
    let a = allocation(SubChunkKey::new(0, -1, 2, 3), 4, 40);
    let b = allocation(SubChunkKey::new(0, 5, 6, 7), 8, 80);
    let forward = sort_key([1, 2, 3], [0, 0, 0, 1], vec![a.clone(), b.clone()], 9, 10);
    let reverse = sort_key([1, 2, 3], [0, 0, 0, 1], vec![b, a.clone(), a], 9, 10);
    assert_eq!(forward, reverse);
    let mut state = TransparentSortState::with_upload_cap(8);
    assert_eq!(state.request(&forward), state.request(&reverse));
}

#[test]
fn conflicting_duplicate_visible_allocation_is_rejected() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    assert_eq!(
        ViewSortKey::try_new(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![allocation(key, 1, 8), allocation(key, 2, 16)],
            ChunkTextureAssetIdentity::for_test(1, 1),
            meshing::ChunkBiomeTintIdentity::new(1, 1),
        ),
        Err(TransparentSortError::ConflictingAllocation { key })
    );
}

#[test]
fn exact_camera_key_distinguishes_sub_quantum_motion_and_canonicalizes_quaternion_sign() {
    let base = exact_sort_key([1.0, 2.0, 3.0], [0.1, 0.2, 0.3, 0.9]);
    let moved = exact_sort_key(
        [f32::from_bits(1.0_f32.to_bits() + 1), 2.0, 3.0],
        [0.1, 0.2, 0.3, 0.9],
    );
    let negated = exact_sort_key([1.0, 2.0, 3.0], [-0.1, -0.2, -0.3, -0.9]);
    assert_ne!(base, moved);
    assert_eq!(base, negated);
}

#[test]
fn unchanged_transparent_order_reuses_committed_slot_without_upload() {
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let first_key = sort_key([0, 0, 0], [0, 0, 0, 1], visible.clone(), 1, 1);
    let mut state = TransparentSortState::with_upload_cap(8);
    let first = state.request(&first_key);
    let refs = vec![
        PackedTransparentDrawRef::new(8, 1),
        PackedTransparentDrawRef::new(9, 1),
    ];
    assert_eq!(
        state.complete(TransparentSortResult::new(first, first_key, refs.clone()).unwrap()),
        Ok(false)
    );
    assert!(state.next_upload_batch().is_some());
    assert!(state.acknowledge_upload());
    let committed = state.committed().unwrap().clone();

    let camera_only = sort_key([1, 0, 0], [0, 0, 0, 1], visible.clone(), 1, 1);
    let second = state.request(&camera_only);
    assert_eq!(
        state.complete(TransparentSortResult::new(second, camera_only, refs).unwrap()),
        Ok(true)
    );
    assert!(state.next_upload_batch().is_none());
    assert_eq!(
        state.committed().unwrap().buffer_slot(),
        committed.buffer_slot()
    );
    assert_eq!(state.committed().unwrap().generation(), second);

    let changed_key = sort_key([2, 0, 0], [0, 0, 0, 1], visible, 1, 1);
    let third = state.request(&changed_key);
    assert_eq!(
        state.complete(
            TransparentSortResult::new(
                third,
                changed_key,
                vec![
                    PackedTransparentDrawRef::new(9, 1),
                    PackedTransparentDrawRef::new(8, 1)
                ],
            )
            .unwrap(),
        ),
        Ok(false)
    );
    assert!(state.next_upload_batch().is_some());
}

#[test]
fn zero_transparent_upload_cap_still_makes_bounded_progress() {
    let key = sort_key([0, 0, 0], [0, 0, 0, 1], vec![], 1, 1);
    let mut state = TransparentSortState::with_upload_cap(0);
    let generation = state.request(&key);
    let result =
        TransparentSortResult::new(generation, key, vec![PackedTransparentDrawRef::new(1, 2)])
            .unwrap();
    assert_eq!(state.complete(result), Ok(false));
    assert_eq!(state.next_upload_batch().unwrap().refs().len(), 1);
    assert!(state.acknowledge_upload());
}

#[test]
fn transparent_view_reset_preserves_monotonic_sort_generations() {
    let key = sort_key([0, 0, 0], [0, 0, 0, 1], vec![], 1, 1);
    let mut state = TransparentSortState::with_upload_cap(8);
    let before_reset = state.request(&key);
    state.reset_preserving_generation();
    let after_reset = state.request(&key);
    assert!(after_reset > before_reset);
    assert!(state.committed().is_none());
    assert_eq!(state.staged_ref_count(), 0);
}

#[test]
fn transparent_sort_metrics_cross_world_clones_share_the_latest_snapshot() {
    let metrics = TransparentSortMetrics::default();
    let render_world = metrics.clone();
    let snapshot = TransparentSortMetricsSnapshot {
        request_generation: 7,
        result_generation: 6,
        committed_generation: 5,
        encoded_generation: 5,
        presented_generation: 5,
        ref_count: 4,
        cpu_duration: Duration::from_micros(30),
        request_to_commit_latency: Duration::from_micros(50),
        staged_bytes: 24,
        upload_bytes: 16,
        stale_reject_count: 3,
        ceiling_reject_count: 2,
        active_slot_age_frames: 9,
        transparent_water_distinct_tint_count: 2,
    };
    render_world.publish_for_test(snapshot);
    assert_eq!(metrics.snapshot(), snapshot);
}

#[test]
fn transparent_view_and_double_slot_memory_are_strictly_bounded() {
    assert_eq!(MAX_TRANSPARENT_VIEWS, 1);
    assert_eq!(TRANSPARENT_REF_SLOT_BYTES, 16 * 1024 * 1024);
    assert_eq!(TRANSPARENT_REF_BUFFER_BYTES, 32 * 1024 * 1024);
    assert_eq!(
        TRANSPARENT_REF_BUFFER_BYTES,
        size_of::<PackedTransparentDrawRef>() * MAX_TRANSPARENT_DRAW_REFS * 2
    );
}

#[test]
fn transparent_sort_job_gate_keeps_one_in_flight_and_only_the_newest_replacement() {
    let mut gate = TransparentSortJobGate::default();
    let first = ViewSortGeneration::for_test(1);
    let second = ViewSortGeneration::for_test(2);
    let newest = ViewSortGeneration::for_test(3);
    assert_eq!(gate.submit(first, "first"), Some((first, "first")));
    assert_eq!(gate.submit(second, "second"), None);
    assert_eq!(gate.submit(newest, "newest"), None);
    assert_eq!(gate.in_flight_generation(), Some(first));
    assert_eq!(gate.pending_generation(), Some(newest));
    assert_eq!(gate.complete(first), Some((newest, "newest")));
    assert_eq!(gate.in_flight_generation(), Some(newest));
    assert_eq!(gate.pending_generation(), None);
    assert_eq!(gate.complete(newest), None);
    assert_eq!(gate.in_flight_generation(), None);
}

#[test]
fn transparent_pipeline_uses_alpha_without_depth_write() {
    let plugin = include_str!("../src/plugin.rs");
    let packed = PackedTransparentDrawRef::new(17, 29);
    assert_eq!(bytemuck::bytes_of(&packed).len(), 8);
    assert!(plugin.contains("ViewSortedRenderPhases<Transparent3d>"));
    assert!(plugin.contains(".blend = Some(BlendState::ALPHA_BLENDING)"));
    assert!(plugin.contains(".depth_write_enabled = false"));
    assert!(plugin.contains("depth_compare: CompareFunction::GreaterEqual"));
    assert!(plugin.contains("liquid_descriptor.primitive.cull_mode = None"));
    assert!(plugin.contains("binding: 14"));
}

#[test]
fn non_water_liquid_pipeline_is_opaque_and_depth_writing() {
    let plugin = include_str!("../src/plugin.rs");
    assert!(plugin.contains("packed depth-writing liquid pipeline"));
    assert!(plugin.contains("depth_liquid_variants"));
    assert!(plugin.contains("vertex_depth"));
    assert!(plugin.contains("fragment_depth"));
    assert!(plugin.contains("DrawDepthLiquidCommands"));
    assert!(plugin.contains("DrawDepthLiquidIndirectCommands"));
    let depth_pipeline = plugin
        .split("let mut depth_liquid_descriptor = descriptor.clone();")
        .nth(1)
        .and_then(|source| source.split("Self {").next())
        .expect("depth-writing liquid descriptor");
    assert!(depth_pipeline.contains("vertex_depth"));
    assert!(depth_pipeline.contains("fragment_depth"));
    assert!(depth_pipeline.contains("depth_liquid_descriptor.primitive.cull_mode = None"));
    assert!(!depth_pipeline.contains("BlendState::ALPHA_BLENDING"));
    assert!(!depth_pipeline.contains("depth_write_enabled = false"));
}

#[test]
fn direct_and_mdi_share_transparent_order() {
    let direct = direct_transparent_draw_args_for_test(1, 37).unwrap();
    let mdi = mdi_transparent_draw_args_for_test(1, 37).unwrap();
    assert_eq!(direct, mdi);
    assert_eq!(direct.index_count, 6);
    assert_eq!(direct.instance_count, 37);
    assert_eq!(direct.first_index, 0);
    assert_eq!(direct.base_vertex, 0);
    assert_eq!(direct.first_instance, MAX_TRANSPARENT_DRAW_REFS as u32);
}

#[test]
fn transparent_draw_evidence_scan_is_only_paid_for_an_active_frame_probe() {
    let plugin = include_str!("../src/plugin.rs");
    assert!(plugin.contains("fn is_active(&self) -> bool"));
    assert_eq!(
        plugin.matches("if frame_probe.is_active()").count(),
        2,
        "direct and MDI must keep normal liquid drawing O(1)"
    );
    assert!(plugin.contains("transparent_frame_draw_for_range(snapshot, arena, ref_range)"));
}

#[test]
fn transparent_indirect_command_upload_is_generation_cached() {
    let plugin = include_str!("../src/plugin.rs");
    assert!(plugin.contains("last_indirect_identity"));
    assert!(plugin.contains("runtime.last_indirect_identity != Some(identity)"));
}

fn sort_candidate(
    key: SubChunkKey,
    local_quad_index: u32,
    record: u32,
    subchunk_center: [f32; 3],
    quad_centroid: [f32; 3],
) -> TransparentSortCandidate {
    TransparentSortCandidate::new(
        key,
        local_quad_index,
        record,
        record + 100,
        subchunk_center,
        quad_centroid,
    )
}

#[test]
fn transparent_sort_is_grouped_back_to_front_stable_and_rotation_sensitive() {
    let near_key = SubChunkKey::new(0, 0, 0, -1);
    let far_key = SubChunkKey::new(0, 0, 0, -2);
    let candidates = vec![
        sort_candidate(near_key, 1, 11, [0.0, 0.0, -2.0], [0.0, 0.0, -2.5]),
        sort_candidate(far_key, 1, 21, [0.0, 0.0, -10.0], [0.0, 0.0, -10.0]),
        sort_candidate(far_key, 0, 20, [0.0, 0.0, -10.0], [0.0, 0.0, -12.0]),
        sort_candidate(far_key, 2, 22, [0.0, 0.0, -10.0], [0.0, 0.0, -10.0]),
    ];
    let identity = sort_transparent_candidates_for_test(Mat4::IDENTITY, candidates.clone());
    assert_eq!(
        identity
            .iter()
            .map(|draw_ref| draw_ref.liquid_record_index())
            .collect::<Vec<_>>(),
        vec![20, 21, 22, 11],
        "subchunks and their internal faces are back-to-front; ties use local index"
    );

    let rotated = sort_transparent_candidates_for_test(
        Mat4::from_quat(Quat::from_rotation_y(std::f32::consts::PI)),
        candidates,
    );
    assert_eq!(rotated[0].liquid_record_index(), 11);
}

#[test]
fn task7_streams_share_one_physical_buffer_with_binding_headroom() {
    const CURRENT_VERTEX_STORAGE_BINDINGS: usize = 6;
    const MODEL_TEMPLATE_BINDINGS: usize = 1;
    let plugin = include_str!("../src/plugin.rs");
    let legacy_task7_buffers = [
        "model_buffer: Buffer",
        "model_lighting_buffer: Buffer",
        "liquid_buffer: Buffer",
        "liquid_lighting_buffer: Buffer",
    ]
    .into_iter()
    .filter(|field| plugin.contains(field))
    .count();
    let task7_physical_buffers = usize::from(plugin.contains("geometry_stream_buffer: Buffer"));

    assert_eq!(legacy_task7_buffers, 0);
    assert_eq!(
        task7_physical_buffers, 1,
        "the four logical Task 7 streams must share one physical storage buffer"
    );
    assert!(
        CURRENT_VERTEX_STORAGE_BINDINGS + task7_physical_buffers + MODEL_TEMPLATE_BINDINGS <= 8,
        "the projected vertex-stage storage bindings must fit the common/minimum limit"
    );
}

#[test]
fn crossed_model_pipeline_is_two_sided_and_uses_shared_bounded_bindings() {
    let plugin = include_str!("../src/plugin.rs");
    let shader = include_str!("../src/model.wgsl");
    assert!(plugin.contains("load_internal_asset!(app, MODEL_SHADER_HANDLE, \"model.wgsl\""));
    assert!(plugin.contains("\"packed model pipeline\""));
    assert!(plugin.contains("model_descriptor.primitive.cull_mode = None"));
    assert!(plugin.contains("resource: arena.geometry_stream_buffer.as_entire_binding()"));
    assert!(plugin.contains("resource: texture_assets.model_template_buffer.as_entire_binding()"));
    assert!(plugin.contains("MODEL_TEMPLATE_BINDING_BUDGET: u32 = 8"));
    assert!(plugin.contains("MODEL_VERTEX_STORAGE_BINDINGS: u32 = 8"));
    assert!(plugin.contains("MODEL_VERTEX_STORAGE_BINDINGS <= MODEL_TEMPLATE_BINDING_BUDGET"));
    assert!(shader.contains("@binding(12) var<storage, read> model_templates: array<u32>"));
    assert!(shader.contains("@binding(13) var<storage, read> geometry_streams: array<u32>"));
    assert!(shader.contains("visible_quad_mask"));
    assert!(shader.contains("lighting_base_index"));
    assert!(shader.contains("let draw_ref_word = instance_index * 2u"));
    assert!(shader.contains("let model_ref_index = geometry_streams[draw_ref_word]"));
    assert!(shader.contains("let quad_index = geometry_streams[draw_ref_word + 1u]"));
    assert!(shader.contains("let geometry_word_count = arrayLength(&geometry_streams)"));
    assert!(shader.contains("if (draw_ref_word + 1u >= geometry_word_count)"));
    assert!(shader.contains("if (quad_index >= 32u || model_ref_index > 0x3fffffffu)"));
    assert!(shader.contains("if (ref_word + 3u >= geometry_word_count)"));
    assert!(shader.contains("block_light"));
    assert!(shader.contains("sky_light"));
    assert!(!shader.contains("safe_quad_index"));
    let masked_guard = shader
        .find("if (is_visible == 0u) {")
        .expect("masked/padded model quads must exit in the vertex stage");
    let masked_return = shader[masked_guard..]
        .find("return invisible_vertex();")
        .expect("masked/padded model quads must return an invisible vertex")
        + masked_guard;
    assert!(
        shader[masked_guard..]
            .starts_with("if (is_visible == 0u) {\r\n        return invisible_vertex();")
            || shader[masked_guard..]
                .starts_with("if (is_visible == 0u) {\n        return invisible_vertex();"),
        "masked/padded model guard must immediately return an invisible vertex"
    );
    assert!(masked_guard < shader.find("var template_position").unwrap());
    assert!(masked_guard < shader.find("let light_word").unwrap());
    assert!(masked_return < shader.find("var template_position").unwrap());
    let zero_guard = shader
        .find("if (quad_count == 0u || quad_index >= quad_count)")
        .expect("zero-quad templates require an early invisible return");
    assert!(zero_guard < shader.find("template_quad_base").unwrap());
    assert!(zero_guard < shader.find("let light_word").unwrap());
    assert!(shader.contains("sampled.a < 0.5"));
    assert!(shader.contains("let quad_flags = model_templates[template_quad_base + 11u]"));
    assert!(shader.contains("@builtin(front_facing) front_facing: bool"));
    assert!(shader.contains("if (!front_facing && in.two_sided == 0u) { discard; }"));
    assert!(!shader.contains("face_light"));
}

#[test]
fn opaque_model_pipeline_selects_its_fragment_entry_point_explicitly() {
    let plugin = include_str!("../src/plugin.rs");
    let model_pipeline = plugin
        .split_once("model_descriptor.label = Some(\"packed model pipeline\".into());")
        .expect("packed model pipeline descriptor")
        .1
        .split_once("let mut transparent_model_descriptor = model_descriptor.clone();")
        .expect("transparent model pipeline follows opaque model pipeline")
        .0;

    assert!(
        model_pipeline.contains(".entry_point = Some(\"fragment\".into());"),
        "the opaque model pipeline must select `fragment` now that model.wgsl has multiple fragment entry points"
    );
}

#[test]
fn crossed_model_shader_parses_validates_and_has_one_shared_binding_shape() {
    let shader = standalone_world_shader(include_str!("../src/model.wgsl"));
    let module = naga::front::wgsl::parse_str(&shader).expect("parse packed model WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate packed model WGSL");
    assert_eq!(shader.matches("@group(0) @binding(").count(), 15);
    for binding in 0..=13 {
        assert!(shader.contains(&format!("@group(0) @binding({binding})")));
    }
    assert!(shader.contains("@group(0) @binding(15)"));
}

#[test]
fn crossed_model_direct_and_mdi_commands_have_identical_output_addressing() {
    let source = include_str!("../src/plugin.rs");
    assert!(source.contains("fn model_direct_draw_command("));
    assert!(source.contains("fn model_mdi_draw_command("));
    assert!(source.contains("model_draw_command(allocation, direct_stream_addresses(allocation))"));
    assert!(source.contains("model_draw_command(allocation, mdi_stream_addresses(allocation))"));
}

#[test]
fn transparent_model_pipeline_blends_without_depth_write_or_alpha_cutoff() {
    let plugin = include_str!("../src/plugin.rs");
    let shader = include_str!("../src/model.wgsl");

    assert!(plugin.contains("packed transparent model pipeline"));
    assert!(plugin.contains("transparent_model_descriptor"));
    assert!(plugin.contains("entry_point = Some(\"fragment_blend\".into())"));
    assert!(plugin.contains("blend = Some(BlendState::ALPHA_BLENDING)"));
    assert!(plugin.contains("depth_write_enabled = false"));

    let blend_start = shader
        .find("fn fragment_blend(")
        .expect("transparent model fragment entry point");
    let blend_body = &shader[blend_start..];
    assert!(blend_body.contains("let lit = lit_colour("));
    assert!(
        blend_body.contains("return vec4(apply_distance_fog(lit, in.world_position), colour.a);")
    );
    assert!(
        shader.contains("return vec4(sampled.rgb, sampled.a);")
            && shader
                .contains("return vec4(sampled.rgb * blended_biome_tint(tint_kind, flags, record, position), sampled.a);"),
        "biome tinting must preserve sampled alpha for the blend entry point"
    );
    assert!(
        !blend_body.contains("sampled.a < 0.5"),
        "blend models must preserve fractional sampled alpha"
    );
}

#[test]
fn transparent_models_queue_as_distance_sorted_subchunk_phase_items() {
    let plugin = include_str!("../src/plugin.rs");

    assert!(plugin.contains("add_render_command::<Transparent3d, DrawTransparentModelCommands>()"));
    assert!(plugin.contains(".transparent_model_variants"));
    assert!(plugin.contains(".specialize(&pipeline_cache, key)"));
    assert!(plugin.contains("transparent_model_phase_distance(&rangefinder, allocation.key)"));
    assert!(plugin.contains("prepare_transparent_model_sorts"));
    assert!(plugin.contains("spawn_transparent_model_sort"));
    assert!(plugin.contains("DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME"));
    assert!(plugin.contains("if runtime.view_entity != Some(view_entity)"));
    assert!(plugin.contains("transparent_liquid_phase_distance(&rangefinder, group.key)"));
    assert!(plugin.contains("range: group.ref_range"));
    assert!(!plugin.contains("distance: 0.0,"));
    assert!(plugin.contains("draw_function: transparent_model_draw"));
    assert!(plugin.contains("entity: (render_entity, main_entity)"));
}

#[test]
fn flowerbed_uses_packed_model_lighting_and_conservative_connectivity() {
    let assets = flowerbed_runtime_assets();
    let sub_chunk = flowerbed_sub_chunk(&[([4, 8, 8], 0), ([8, 8, 8], 1), ([12, 8, 8], 2)]);
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub_chunk,
    );

    assert!(mesh.cube_quads().is_empty());
    assert_eq!(mesh.model_refs().len(), 3);
    let growth_zero = assets
        .resolve(NetworkIdMode::Sequential, 0)
        .model_template()
        .unwrap();
    let growth_three = assets
        .resolve(NetworkIdMode::Sequential, 1)
        .model_template()
        .unwrap();
    let growth_seven = assets
        .resolve(NetworkIdMode::Sequential, 2)
        .model_template()
        .unwrap();
    assert_ne!(growth_zero, growth_three);
    assert_eq!(
        growth_three, growth_seven,
        "growth 7 aliases the measured full layout"
    );
    let zero_quads = assets.model_templates()[growth_zero as usize].quad_count;
    let full_quads = assets.model_templates()[growth_three as usize].quad_count;
    assert!(zero_quads < full_quads);
    assert_eq!(
        mesh.model_refs()
            .iter()
            .map(|packed| packed.words())
            .collect::<Vec<_>>(),
        vec![
            [
                4 | (8 << 4) | (8 << 8),
                growth_zero,
                0,
                (1 << zero_quads) - 1
            ],
            [
                8 | (8 << 4) | (8 << 8),
                growth_three,
                zero_quads,
                (1 << full_quads) - 1
            ],
            [
                12 | (8 << 4) | (8 << 8),
                growth_seven,
                zero_quads + full_quads,
                (1 << full_quads) - 1
            ],
        ]
    );
    assert_eq!(
        mesh.model_lighting().len(),
        (zero_quads + full_quads * 2) as usize
    );
    assert!(
        mesh.connectivity().is_all_connected(),
        "a flowerbed is non-occluding and must conservatively preserve every cave-visibility path"
    );
}

#[test]
fn flowerbed_is_two_sided_alpha_cutout_on_the_shared_model_pipeline() {
    let assets = flowerbed_runtime_assets();
    for runtime_id in 0..3 {
        let template_id = assets
            .resolve(NetworkIdMode::Sequential, runtime_id)
            .model_template()
            .expect("compiled FlowerBed visual");
        let template = assets.model_templates()[template_id as usize];
        let quads = &assets.model_quads()
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert!(!quads.is_empty());
        assert!(
            quads
                .iter()
                .all(|quad| quad.flags & MODEL_QUAD_FLAG_TWO_SIDED != 0)
        );
        assert!(quads.iter().all(|quad| {
            assets.material(quad.material).flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
        }));
    }

    let plugin = include_str!("../src/plugin.rs");
    let shader = include_str!("../src/model.wgsl");
    assert!(plugin.contains("model_descriptor.primitive.cull_mode = None"));
    assert!(shader.contains("sampled.a < 0.5"));
}

#[test]
fn flowerbed_adds_no_renderer_object_or_binding_per_block_or_subchunk() {
    let assets = flowerbed_runtime_assets();
    let placements = (0..8)
        .flat_map(|x| (0..8).map(move |z| ([x, 8, z], ((x + z) % 3) as usize)))
        .collect::<Vec<_>>();
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &flowerbed_sub_chunk(&placements),
    );
    let expected_lighting = placements
        .iter()
        .map(|(_, runtime_id)| {
            let template = assets
                .resolve(NetworkIdMode::Sequential, *runtime_id as u32)
                .model_template()
                .unwrap();
            assets.model_templates()[template as usize].quad_count as usize
        })
        .sum::<usize>();
    assert_eq!(mesh.model_refs().len(), placements.len());
    assert_eq!(mesh.model_lighting().len(), expected_lighting);
    assert_eq!(
        size_of_val(mesh.model_refs()) + size_of_val(mesh.model_lighting()),
        mesh.model_refs().len() * 16 + expected_lighting * 8
    );

    let one_mesh = mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &flowerbed_sub_chunk(&[([0, 8, 0], 0)]),
    );
    let (one_entity_delta, one_instances, one_components, one_has_mesh3d) =
        flowerbed_render_entity_contract(one_mesh);
    let (many_entity_delta, many_instances, many_components, many_has_mesh3d) =
        flowerbed_render_entity_contract(mesh);
    assert_eq!(one_instances, 1);
    assert_eq!(many_instances, 1);
    assert_eq!(
        many_entity_delta, one_entity_delta,
        "64 FlowerBeds must not add per-block ECS entities"
    );
    assert_eq!(many_components, one_components);
    assert!(!one_has_mesh3d);
    assert!(!many_has_mesh3d);
    for forbidden in [
        "Mesh3d",
        "MeshMaterial3d",
        "Material",
        "BindGroup",
        "Pipeline",
    ] {
        assert!(
            many_components
                .iter()
                .all(|component| !component.contains(forbidden)),
            "produced chunk entity must not own a {forbidden} component: {many_components:?}"
        );
    }

    let plugin = include_str!("../src/plugin.rs").replace("\r\n", "\n");
    for structure in [
        "ChunkRenderInstance",
        "GpuChunkAllocation",
        "ArenaAllocation",
        "RetiredArenaAllocation",
    ] {
        let body = rust_struct_body(&plugin, structure);
        for forbidden in [
            "Mesh",
            "Material",
            "BindGroup",
            "RenderPipeline",
            "Variants<",
        ] {
            assert!(
                !body.contains(forbidden),
                "{structure} must not own a per-subchunk {forbidden}"
            );
        }
    }
    let arena = rust_struct_body(&plugin, "ChunkGpuArena");
    assert!(arena.contains("bind_group: Option<BindGroup>"));
    assert!(arena.contains("allocations: HashMap<Entity, ArenaAllocation>"));
    let pipeline = rust_struct_body(&plugin, "ChunkPipeline");
    assert!(pipeline.contains("Variants<RenderPipeline"));
    assert!(pipeline.contains("bind_group_layout: BindGroupLayoutDescriptor"));
}

fn rust_struct_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("struct {name} ");
    let declaration = source
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {name} declaration"));
    let open = declaration
        + source[declaration..]
            .find('{')
            .unwrap_or_else(|| panic!("missing {name} body"));
    let mut depth = 0_u32;
    for (offset, byte) in source.as_bytes()[open..].iter().copied().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {name} body")
}

fn flowerbed_render_entity_contract(mesh: meshing::ChunkMesh) -> (u32, usize, Vec<String>, bool) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));
    let entity_count_before = app.world().entities().len();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(
            SubChunkKey::new(0, 0, 0, 0),
            mesh,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    app.update();
    let entities = app
        .world_mut()
        .query::<(bevy::prelude::Entity, &ChunkRenderInstance)>()
        .iter(app.world())
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    assert_eq!(entities.len(), 1, "one chunk entity per queued subchunk");
    let component_ids = app
        .world()
        .entity(entities[0])
        .archetype()
        .components()
        .to_vec();
    let has_mesh3d = app
        .world()
        .entity(entities[0])
        .contains::<bevy::mesh::Mesh3d>();
    let component_names = component_ids
        .into_iter()
        .map(|component| {
            app.world()
                .components()
                .get_info(component)
                .expect("chunk archetype component metadata")
                .name()
                .to_string()
        })
        .collect::<Vec<_>>();
    (
        app.world().entities().len() - entity_count_before,
        entities.len(),
        component_names,
        has_mesh3d,
    )
}

#[test]
fn queue_counts_every_stream_and_sidecar() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let cube = solid_mesh(1);
    let mesh = meshing::ChunkMesh::from_streams(
        cube.quads().to_vec(),
        vec![meshing::PackedModelRef::new(1, 2, 0, 1)],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        vec![meshing::PackedModelDrawRef::new(0, 0)],
        vec![meshing::PackedLiquidQuad::try_from_words([4, 5, 6, 7]).unwrap()],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        cube.connectivity(),
    )
    .with_transparent_model_draw_refs(vec![meshing::PackedModelDrawRef::new(0, 1)]);
    let expected = 6 * (size_of::<PackedQuad>() + size_of::<meshing::PackedQuadLighting>())
        + size_of::<meshing::PackedModelRef>()
        + size_of::<meshing::PackedQuadLighting>()
        + 2 * size_of::<meshing::PackedModelDrawRef>()
        + size_of::<meshing::PackedLiquidQuad>()
        + size_of::<meshing::PackedQuadLighting>();
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 1,
        max_bytes: expected as u64,
    });

    queue
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .expect("every geometry stream fits at its exact combined byte count");
    assert_eq!(queue.pending_bytes(), expected as u64);
}

#[test]
fn cube_lighting_counts_toward_exact_queue_caps_and_survives_extraction() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let mesh = solid_mesh(1);
    let expected_lighting = mesh.cube_lighting().to_vec();
    let exact_bytes =
        6 * (size_of::<PackedQuad>() + size_of::<meshing::PackedQuadLighting>()) as u64;
    let mut too_small = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 1,
        max_bytes: exact_bytes - 1,
    });
    let mesh = too_small
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .expect_err("cube lighting must participate in the CPU queue cap");
    assert_eq!(too_small.pending_bytes(), 0);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: 1,
            max_bytes: exact_bytes,
        }))
        .add_plugins(DebugWorldPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .expect("exact cube geometry plus CPU-light sidecar cap fits");
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().pending_bytes(),
        exact_bytes
    );
    app.update();

    let instance = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.cube_lighting(), expected_lighting);
    assert_eq!(instance.cube_lighting().len(), instance.quads().len());
}

#[test]
fn opaque_and_model_streams_share_one_subchunk_visibility_component() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let cube = solid_mesh(1);
    let mesh = meshing::ChunkMesh::from_streams(
        cube.quads().to_vec(),
        vec![meshing::PackedModelRef::new(1, 0, 0, 1)],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        vec![meshing::PackedModelDrawRef::new(0, 0)],
        vec![],
        vec![],
        cube.connectivity(),
    );
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .unwrap();

    app.update();

    {
        let mut query = app
            .world_mut()
            .query::<(&ChunkRenderInstance, &mut Visibility)>();
        let (instance, mut visibility) = query.single_mut(app.world_mut()).unwrap();
        assert_eq!(instance.key(), key);
        assert!(!instance.quads().is_empty());
        assert!(!instance.model_refs().is_empty());
        *visibility = Visibility::Hidden;
    }

    let (instance, visibility) = app
        .world_mut()
        .query::<(&ChunkRenderInstance, &Visibility)>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.key(), key);
    assert_eq!(*visibility, Visibility::Hidden);
    assert!(!instance.quads().is_empty());
    assert!(!instance.model_refs().is_empty());
}

#[test]
fn allocated_but_undrawn_target_is_not_exact_presented_evidence() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let acknowledgement = PresentedFrameAck {
        cohort: RenderViewCohort::new(0, [65, 65], 16),
        frame_sequence: 1,
        allocation_manifest: Arc::from([(key, 7)]),
        visible_allocation_manifest: Arc::from([(key, 7)]),
        drawn_manifest: Arc::from([]),
        view_generation: 1,
        render_ready_at: now,
        present_returned_at: now + Duration::from_millis(1),
        gpu_completed_at: now + Duration::from_millis(2),
        missing_target_instances: 0,
        unexpected_target_instances: 0,
        source_instances: 0,
        foreign_instances: 0,
        stale_generation_instances: 0,
        orphan_allocations: 0,
        transparent_sort_generation: 0,
        model_witness: None,
    };

    assert!(!acknowledgement.is_exact());
}

fn runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let mut visuals = vec![
            BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
                kind: VisualKind::Diagnostic,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
            14
        ];
        for material_id in 1..14_u32 {
            visuals[material_id as usize] = BlockVisual {
                faces: [material_id; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
        }
        let compiled = CompiledAssets {
            visuals: visuals.into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![
                Material {
                    texture: TextureRef::DIAGNOSTIC,
                    flags: 0,
                    animation: NO_ANIMATION
                };
                14
            ]
            .into_boxed_slice(),
            light_properties: vec![assets::LightProperties::default(); 14].into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(TextureArray {
                layers: 1,
                mips: [16_u32, 8, 4, 2, 1]
                    .into_iter()
                    .map(|size| TextureMip {
                        size,
                        rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })]
            .into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
        };
        let blob = encode_blob(&compiled).expect("encode synthetic plugin assets");
        RuntimeAssets::decode(&blob).expect("decode synthetic plugin assets")
    })
}

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn write_flowerbed_pack(root: &Path) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create FlowerBed fixture tree");
    fs::write(
        root.join("blocks.json"),
        r#"{"wildflowers":{"textures":"wildflowers"}}"#,
    )
    .expect("write FlowerBed blocks routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"wildflowers":{"textures":["textures/blocks/wildflowers","textures/blocks/wildflowers_stem"]}}}"#,
    )
    .expect("write FlowerBed terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write empty flipbook inventory");

    for (index, name) in ["wildflowers", "wildflowers_stem"].into_iter().enumerate() {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for (pixel_index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[
                20 + index as u8,
                80 + (pixel_index % 16) as u8,
                140,
                if pixel_index % 3 == 0 { 0 } else { 255 },
            ]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode FlowerBed fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write FlowerBed fixture PNG");
    }
}

fn flowerbed_runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let directory = tempfile::tempdir().expect("create FlowerBed render fixture");
        write_flowerbed_pack(directory.path());
        let generated = read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .expect("decode committed FlowerBed registry");
        let records = [0_u32, 3, 7]
            .into_iter()
            .enumerate()
            .map(|(sequential_id, growth)| {
                let mut record = generated
                    .iter()
                    .find(|record| {
                        record.name.as_ref() == "minecraft:wildflowers"
                            && record.model_state.get(ModelStateField::Growth) == Some(growth)
                            && record.model_state.get(ModelStateField::Orientation) == Some(2)
                    })
                    .unwrap_or_else(|| panic!("missing generated growth={growth} FlowerBed record"))
                    .clone();
                record.sequential_id = sequential_id as u32;
                record.network_hash = 50_000 + sequential_id as u32;
                record
            })
            .collect::<Vec<_>>();
        let compiled = compile_pack(directory.path(), &records)
            .expect("compile real FlowerBed render fixture through assets");
        let blob = encode_blob(&compiled).expect("encode compiled FlowerBed render fixture");
        RuntimeAssets::decode(&blob).expect("decode compiled FlowerBed render fixture")
    })
}

fn flowerbed_sub_chunk(placements: &[([u8; 3], usize)]) -> SubChunk {
    let bits_per_index = 2_usize;
    let values_per_word = 32 / bits_per_index;
    let mut words = vec![0_u32; 4096_usize.div_ceil(values_per_word)];
    for &([x, y, z], runtime_id) in placements {
        assert!(x < 16 && y < 16 && z < 16);
        assert!(runtime_id < 3);
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        let shift = (linear % values_per_word) * bits_per_index;
        words[linear / values_per_word] |= (runtime_id as u32 + 1) << shift;
    }

    let mut encoded = vec![9, 1, 0, 5];
    for word in words {
        encoded.extend_from_slice(&word.to_le_bytes());
    }
    encoded.extend(zig_zag_i32(4));
    encoded.extend(zig_zag_i32(AIR as i32));
    encoded.extend(zig_zag_i32(0));
    encoded.extend(zig_zag_i32(1));
    encoded.extend(zig_zag_i32(2));
    SubChunk::decode(&encoded).expect("decode packed FlowerBed subchunk")
}

#[test]
fn render_queue_enforces_item_and_byte_limits_without_losing_replacements() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey::new(0, 1, 0, 0);
    let third = SubChunkKey::new(0, 2, 0, 0);
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 2,
        max_bytes: 96,
    });

    queue
        .try_insert(first, solid_mesh(1), ChunkUploadPriority::new(0.0))
        .unwrap();
    queue
        .try_insert(
            second,
            meshing::ChunkMesh::default(),
            ChunkUploadPriority::new(1.0),
        )
        .unwrap();
    assert_eq!(queue.retained_len(), 2);
    assert_eq!(queue.pending_bytes(), 96);

    let rejected = queue
        .try_insert(third, solid_mesh(3), ChunkUploadPriority::new(2.0))
        .unwrap_err();
    assert_eq!(rejected.quad_count(), 6);
    assert_eq!(queue.retained_len(), 2);
    assert_eq!(queue.pending_bytes(), 96);

    queue
        .try_update(
            first,
            meshing::ChunkMesh::default(),
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    queue
        .try_update(second, solid_mesh(2), ChunkUploadPriority::new(1.0))
        .unwrap();
    let superseding = queue
        .try_update(first, solid_mesh(4), ChunkUploadPriority::new(0.0))
        .unwrap_err();
    assert_eq!(superseding.quad_count(), 6);
    assert_eq!(queue.pending_bytes(), 96);

    queue.try_remove(first).unwrap();
    assert_eq!(queue.retained_len(), 2, "removal remains losslessly queued");
    assert!(queue.try_remove(third).is_err());
}

#[test]
fn rejected_mesh_is_eventually_delivered_after_the_capped_queue_drains() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey::new(0, 1, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: 1,
            max_bytes: 96,
        }))
        .add_plugins(DebugWorldPlugin::new(1));

    let rejected = {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_insert(first, solid_mesh(1), ChunkUploadPriority::new(0.0))
            .unwrap();
        queue
            .try_insert(second, solid_mesh(2), ChunkUploadPriority::new(1.0))
            .unwrap_err()
    };

    app.update();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(second, rejected, ChunkUploadPriority::new(1.0))
        .unwrap();
    app.update();

    let mut keys = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(ChunkRenderInstance::key)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(keys, [first, second]);
}

fn solid_mesh(runtime_id: u32) -> meshing::ChunkMesh {
    let mut encoded = vec![9, 1, 0, 1];
    encoded.extend(zig_zag_i32(runtime_id as i32));
    let sub_chunk = SubChunk::decode(&encoded).expect("uniform sub-chunk");
    mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub_chunk,
    )
}

#[test]
fn upload_budget_is_nearest_first_and_queue_supports_update_remove() {
    let near = SubChunkKey::new(0, 1, 2, 3);
    let far = SubChunkKey::new(0, 20, 2, 20);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_insert(far, solid_mesh(7), ChunkUploadPriority::new(100.0))
            .unwrap();
        queue
            .try_insert(near, solid_mesh(11), ChunkUploadPriority::new(1.0))
            .unwrap();
    }
    app.update();

    let rendered = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(|instance| {
            (
                instance.key(),
                instance.quad_count(),
                instance.quads()[0].material_id(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec![(near, 6, 11)]);
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<(&Aabb, &Visibility)>()
            .iter(app.world())
            .count(),
        1,
    );

    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ChunkRenderInstance>()
            .iter(app.world())
            .count(),
        2
    );
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        0
    );

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update(far, solid_mesh(13), ChunkUploadPriority::new(0.0))
            .unwrap();
        queue.try_remove(near).unwrap();
    }
    app.update();
    app.update();

    let rendered = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(|instance| {
            (
                instance.key(),
                instance.quad_count(),
                instance.quads()[0].material_id(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec![(far, 6, 13)]);
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        0
    );

    assert!(
        ChunkUploadPriority::from_camera(near, Vec3::splat(16.0))
            < ChunkUploadPriority::from_camera(far, Vec3::splat(16.0))
    );
}

#[test]
fn packed_chunk_shader_parses_and_validates() {
    let shader = standalone_world_shader(include_str!("../src/chunk.wgsl"));
    let module = naga::front::wgsl::parse_str(&shader).expect("parse packed chunk WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate packed chunk WGSL");

    assert_eq!(shader.matches("@group(0) @binding(").count(), 14);
    for binding in 0..=11 {
        assert!(
            shader.contains(&format!("@group(0) @binding({binding})")),
            "packed chunk shader is missing global texture binding {binding}"
        );
    }
    assert!(shader.contains("@group(0) @binding(15)"));
    assert_eq!(shader.matches("textureSample(").count(), 0);
    assert_eq!(shader.matches("textureSampleGrad(").count(), 2);
    assert!(shader.contains("fn sample_texture_ref("));
    assert!(shader.contains("texture_ref >> 31u"));
    assert!(shader.contains("texture_ref & 0x7ffu"));
    assert!(shader.contains("let uv_dx = dpdx(in.uv);"));
    assert!(shader.contains("let uv_dy = dpdy(in.uv);"));
    assert!(shader.contains("if (in.frame_blend > 0.0)"));
    assert!(shader.contains("mix(current_sample, next_sample, in.frame_blend)"));
    assert!(shader.contains("@interpolate(flat) current_texture: u32"));
    assert!(shader.contains("@interpolate(flat) next_texture: u32"));
    assert!(shader.contains("@interpolate(flat) frame_blend: f32"));
    assert!(shader.contains("@location(3) @interpolate(flat) material_flags: u32"));
    assert!(shader.contains("@interpolate(flat) material_flags: u32"));
    assert_eq!(
        shader
            .matches("out.material_flags = material.flags;")
            .count(),
        1
    );
    assert!(shader.contains("material_flags & (1u << 8u)"));
    assert!(shader.contains("sampled.a < 0.5"));
    assert_eq!(shader.matches("discard").count(), 1);
    assert!(shader.contains("material_flags & 0x30u"));
    assert!(shader.contains("material_flags & (1u << 6u)"));
    assert!(shader.contains("mix(sampled.rgb, tinted, sampled.a)"));
    assert!(shader.contains("in.biome_record,"));
    assert!(shader.contains("if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) {"));
    assert!(shader.contains("greedy_uv"));
    assert!(shader.contains("var<storage, read> biome_records: array<u32>"));
    assert!(shader.contains("var<storage, read> biome_tints: array<BiomeTintGpu>"));
    assert!(shader.contains("out.biome_record = u32(chunk_origin.value.w);"));
    assert!(shader.contains("local_position - normal * 0.001"));
    assert!(shader.contains("(coordinate.x << 8u) | (coordinate.z << 4u) | coordinate.y"));
    assert!(shader.contains("fn packed_biome_tint_index"));
    assert!(shader.contains("fn unpack_linear_rgb10"));
    assert!(shader.contains("if (tint_kind == 0x10u)"));
    assert!(shader.contains("if (tint_kind == 0x30u)"));
    assert!(shader.contains("switch material_flags & 0x600u"));
    assert!(shader.contains("case 0x200u"));
    assert!(shader.contains("case 0x400u"));
    assert!(shader.contains("case 0x600u"));
    assert!(shader.contains("clock.tick / animation.ticks_per_frame"));
    assert!(shader.contains("clock.tick % animation.ticks_per_frame"));
    assert!(shader.contains("(current_index + 1u) % animation.frame_count"));
    assert!(shader.contains("animation.flags & 1u"));
    assert!(shader.contains("material.animation == 0xffffffffu"));
    assert!(shader.contains("var block_textures_page_0: texture_2d_array<f32>"));
    assert!(shader.contains("var block_textures_page_1: texture_2d_array<f32>"));
    assert!(shader.contains("var<storage, read> animations: array<AnimationGpu>"));
    assert!(shader.contains("var<storage, read> animation_frames: array<u32>"));
    assert!(shader.contains("var<uniform> clock: AnimationClockGpu"));
    assert!(!shader.contains("debug_color"));
}

#[test]
fn world_shaders_share_light_curve_channels_and_fragment_only_daylight() {
    let plugin = include_str!("../src/plugin.rs").replace("\r\n", "\n");
    let lighting = include_str!("../src/lighting.wgsl");
    assert_eq!(
        lighting
            .matches("const LIGHT_CURVE: array<f32, 16>")
            .count(),
        1
    );
    assert_eq!(lighting.matches("fn lit_colour(").count(), 1);
    assert_eq!(
        lighting
            .matches("const PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR: f32 = 0.2;")
            .count(),
        1,
        "the conservative floor remains explicitly provisional until native visual tuning"
    );
    assert_eq!(
        lighting
            .matches("const PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR: f32 = 0.04;")
            .count(),
        1,
        "light level zero must retain an explicit, independently tunable ambient floor"
    );
    assert!(lighting.contains("let block_contribution = vec3("));
    assert!(lighting.contains(
        "let effective_daylight = max(clamp(daylight, 0.0, 1.0), PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR);"
    ));
    assert!(lighting.contains("let sky_contribution = vec3("));
    assert!(lighting.contains("let channel_light = max(block_contribution, sky_contribution);"));
    assert!(lighting.contains("vec3(PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR)"));
    assert!(lighting.contains("vec3(1.0)"));
    assert!(lighting.contains("channel_light"));
    assert!(lighting.contains("return colour * combined * clamp(ao_factor, 0.0, 1.0)"));
    assert!(lighting.contains("fn light_brightness(level: u32)"));
    assert!(lighting.contains("fn light_ao_factor(level: u32)"));
    for shader in [
        include_str!("../src/chunk.wgsl"),
        include_str!("../src/model.wgsl"),
        include_str!("../src/liquid.wgsl"),
    ] {
        assert!(shader.contains(
            "#import cinnabar::lighting::{light_ao_factor, light_brightness, lit_colour}"
        ));
        assert!(!shader.contains("const LIGHT_CURVE: array<f32, 16>"));
        assert!(!shader.contains("fn lit_colour("));
        assert!(shader.contains("block_light"));
        assert!(shader.contains("sky_light"));
        assert!(shader.contains("ambient_occlusion"));
        let vertex = shader.split("@fragment").next().unwrap();
        assert!(!vertex.contains("atmosphere.sun_direction_daylight.w"));
        let fragment = shader.split("@fragment").nth(1).unwrap();
        assert!(fragment.contains("atmosphere.sun_direction_daylight.w"));

        let standalone = standalone_world_shader(shader);
        let module = naga::front::wgsl::parse_str(&standalone).expect("parse world shader");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validate world shader");
        assert!(entry_points_use_binding(
            &module,
            naga::ShaderStage::Vertex,
            13
        ));
        assert!(!entry_points_use_binding(
            &module,
            naga::ShaderStage::Vertex,
            15
        ));
        assert!(entry_points_use_binding(
            &module,
            naga::ShaderStage::Fragment,
            15
        ));
        assert!(entry_points_call_function(
            &module,
            naga::ShaderStage::Vertex,
            "light_brightness"
        ));
        assert!(!entry_points_call_function(
            &module,
            naga::ShaderStage::Fragment,
            "light_brightness"
        ));
    }
    assert!(plugin.contains("binding: 13,\n                    visibility: ShaderStages::VERTEX"));
    assert!(
        plugin.contains("binding: 15,\n                    visibility: ShaderStages::FRAGMENT")
    );
    assert!(!plugin.contains("binding: 15,\n                    visibility: ShaderStages::VERTEX"));
}

#[test]
fn chunk_shader_reads_cube_light_from_expanded_origin_without_changing_bindings() {
    let shader = include_str!("../src/chunk.wgsl");
    assert!(shader.contains("struct ChunkOrigin"));
    assert!(shader.contains("cube_bases: vec4<u32>"));
    assert!(shader.contains("@binding(13) var<storage, read> geometry_streams: array<u32>"));
    assert!(shader.contains("let local_quad_index = instance_index - chunk_origin.cube_bases.x"));
    assert!(shader.contains("chunk_origin.cube_bases.y + local_quad_index"));
    assert_eq!(
        standalone_world_shader(shader)
            .matches("@group(0) @binding(")
            .count(),
        14
    );
    assert_eq!(std::mem::size_of::<PackedQuad>(), 8);
    assert_eq!(std::mem::size_of::<meshing::PackedQuadLighting>(), 8);
}

fn uniform_biome_record(tint_index: u32) -> PackedBiomeRecord {
    let mut encoded = vec![1];
    encoded.extend(zig_zag_i32(42));
    let storage = DecodedBiomeColumn::decode(0, 1, &encoded)
        .expect("uniform biome column")
        .storage(0)
        .expect("uniform biome storage");
    PackedBiomeRecord::from_storage(&storage, |_| tint_index)
}

#[test]
fn render_queue_counts_and_extracts_non_fallback_biome_records() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mesh = solid_mesh(1);
    let biome = uniform_biome_record(7);
    let expected_bytes = 6
        * (size_of::<PackedQuad>() + size_of::<meshing::PackedQuadLighting>()) as u64
        + biome.byte_len();
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 1,
        max_bytes: expected_bytes - 1,
    });

    let (rejected_mesh, rejected_biome) = queue
        .try_insert_with_biome(key, mesh, biome.clone(), ChunkUploadPriority::new(0.0))
        .expect_err("biome bytes must participate in queue limits");
    assert_eq!(rejected_mesh.quad_count(), 6);
    assert_eq!(rejected_biome, biome);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: 1,
            max_bytes: expected_bytes,
        }))
        .add_plugins(DebugWorldPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert_with_biome(
            key,
            rejected_mesh,
            rejected_biome,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().pending_bytes(),
        expected_bytes
    );

    app.update();
    let instance = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.biome_record(), &biome);
}

#[test]
fn render_queue_carries_biome_tint_revision_to_the_instance() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert_with_biome_revision(
            key,
            solid_mesh(1),
            uniform_biome_record(7),
            9,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();

    app.update();
    let instance = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.tint_revision(), 9);
}

#[test]
fn packed_chunk_pipeline_family_shares_one_opaque_depth_writing_phase() {
    let plugin = include_str!("../src/plugin.rs");

    assert_eq!(
        plugin
            .matches("let descriptor = RenderPipelineDescriptor {")
            .count(),
        1
    );
    assert_eq!(plugin.matches(".add_render_command::<Opaque3d").count(), 6);
    assert_eq!(plugin.matches("BindGroupLayoutDescriptor::new(").count(), 1);
    assert_eq!(
        plugin.matches("render_device.create_bind_group(").count(),
        1
    );
    assert_eq!(plugin.matches("render_device.create_texture(").count(), 1);
    assert_eq!(plugin.matches("render_device.create_sampler(").count(), 1);
    assert!(plugin.contains("layout: vec![bind_group_layout.clone()]"));
    assert!(plugin.contains("blend: None"));
    assert!(plugin.contains("depth_write_enabled: true"));
    assert_eq!(plugin.matches("binding: ").count(), 32);
    for binding in 0..=15 {
        assert_eq!(
            plugin.matches(&format!("binding: {binding},")).count(),
            2,
            "packed chunk pipeline changed binding {binding}"
        );
    }
    assert_eq!(
        plugin
            .matches("resource: texture_assets.material_buffer.as_entire_binding()")
            .count(),
        1
    );
    assert_eq!(
        plugin
            .matches("BindingResource::TextureView(&texture_assets.views[")
            .count(),
        2
    );
    assert_eq!(
        plugin
            .matches("BindingResource::Sampler(&texture_assets.sampler)")
            .count(),
        1
    );
    assert!(!plugin.contains("AlphaMask3d"));
    assert_eq!(
        plugin
            .matches(".add_render_command::<Transparent3d")
            .count(),
        3
    );
    assert_eq!(size_of::<Material>(), 12);
    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(
        plugin
            .matches("pass.set_bind_group(0, bind_group, &[view_offset.offset]);")
            .count(),
        9,
        "cube/opaque-model/transparent-model/transparent-liquid/depth-liquid direct and MDI share the global bind group"
    );
}

#[test]
fn greedy_uvs_match_every_face_and_repeat_once_per_block() {
    // Resource-pack PNGs and WGPU both treat v=0 as the top row. Vertical
    // faces must therefore assign v=0 to their upper-Y geometry corners.
    let vertical_standard = [[0.0, 1.0], [16.0, 1.0], [16.0, 0.0], [0.0, 0.0]];
    let vertical_transposed = [[0.0, 1.0], [0.0, 0.0], [16.0, 0.0], [16.0, 1.0]];
    let horizontal_standard = [[0.0, 0.0], [16.0, 0.0], [16.0, 1.0], [0.0, 1.0]];
    let horizontal_transposed = [[0.0, 0.0], [0.0, 1.0], [16.0, 1.0], [16.0, 0.0]];

    for face in [Face::NegativeX, Face::PositiveZ] {
        assert_eq!(
            std::array::from_fn(|corner| greedy_texture_uv(face, corner as u32, 16, 1, 0)),
            vertical_standard,
            "unexpected UV corners for {face:?}"
        );
    }
    for face in [Face::PositiveX, Face::NegativeZ] {
        assert_eq!(
            std::array::from_fn(|corner| greedy_texture_uv(face, corner as u32, 16, 1, 0)),
            vertical_transposed,
            "unexpected UV corners for {face:?}"
        );
    }
    assert_eq!(
        std::array::from_fn(|corner| {
            greedy_texture_uv(Face::NegativeY, corner as u32, 16, 1, 0)
        }),
        horizontal_standard
    );
    assert_eq!(
        std::array::from_fn(|corner| {
            greedy_texture_uv(Face::PositiveY, corner as u32, 16, 1, 0)
        }),
        horizontal_transposed
    );

    assert_eq!(
        std::array::from_fn(|corner| greedy_texture_uv(Face::PositiveZ, corner as u32, 1, 1, 0)),
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
    );
    assert_eq!(
        std::array::from_fn(|corner| greedy_texture_uv(Face::PositiveZ, corner as u32, 16, 16, 0)),
        [[0.0, 16.0], [16.0, 16.0], [16.0, 0.0], [0.0, 0.0]]
    );
}

#[test]
fn material_uv_flags_rotate_and_reflect_greedy_coordinates() {
    let face = Face::PositiveZ;
    let base = greedy_texture_uv(face, 1, 4, 2, 0);
    assert_eq!(base, [4.0, 2.0]);
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_ROTATE_90),
        [2.0, 0.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_ROTATE_180),
        [0.0, 0.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_ROTATE_270),
        [0.0, 4.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_REFLECT_U),
        [0.0, 2.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_REFLECT_V),
        [4.0, 0.0]
    );
}

#[test]
fn adapter_limits_reject_oversized_texture_arrays() {
    assert_eq!(
        TextureArrayLimits {
            max_layers: 4,
            max_dimension_2d: 16,
        }
        .validate(5, 16),
        Err(TextureLimitError::Layers {
            requested: 5,
            supported: 4,
        })
    );
    assert_eq!(
        TextureArrayLimits {
            max_layers: 4,
            max_dimension_2d: 8,
        }
        .validate(4, 16),
        Err(TextureLimitError::Dimension {
            requested: 16,
            supported: 8,
        })
    );
}

#[test]
fn mip_upload_plan_preserves_exact_layer_offsets_and_row_padding() {
    let plans = plan_texture_mip_uploads(runtime_assets().texture_array(), 256)
        .expect("plan synthetic texture upload");
    assert_eq!(plans.len(), 5);
    assert_eq!(
        plans
            .iter()
            .map(|plan| (
                plan.mip_level,
                plan.size,
                plan.bytes_per_row,
                plan.rows_per_image,
                plan.layer_source_offsets.as_ref(),
                plan.layer_staging_offsets.as_ref(),
                plan.staging_bytes,
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 16, 256, 16, &[0][..], &[0][..], 4096),
            (1, 8, 256, 8, &[0][..], &[0][..], 2048),
            (2, 4, 256, 4, &[0][..], &[0][..], 1024),
            (3, 2, 256, 2, &[0][..], &[0][..], 512),
            (4, 1, 256, 1, &[0][..], &[0][..], 256),
        ]
    );
    assert_eq!(runtime_assets().materials().len(), 14);

    let two_layers = TextureArray {
        layers: 2,
        mips: vec![TextureMip {
            size: 2,
            rgba8: vec![0; 2 * 2 * 4 * 2].into_boxed_slice(),
        }]
        .into_boxed_slice(),
    };
    let plan = plan_texture_mip_uploads(&two_layers, 256)
        .expect("plan two-layer texture upload")
        .remove(0);
    assert_eq!(plan.layer_source_offsets.as_ref(), [0, 16]);
    assert_eq!(plan.layer_staging_offsets.as_ref(), [0, 512]);
    assert_eq!(plan.staging_bytes, 1024);
}

#[test]
fn global_texture_bind_group_rebuilds_only_for_new_asset_identity_or_revision() {
    let current = ChunkTextureAssetIdentity::for_test(0x1000, 7);
    assert!(!texture_asset_needs_rebuild(Some(current), current));
    assert!(texture_asset_needs_rebuild(
        Some(current),
        ChunkTextureAssetIdentity::for_test(0x2000, 7)
    ));
    assert!(texture_asset_needs_rebuild(
        Some(current),
        ChunkTextureAssetIdentity::for_test(0x1000, 8)
    ));
    assert!(texture_asset_needs_rebuild(None, current));
}

fn texture_ref(page: u32, layer: u32) -> TextureRef {
    TextureRef::new(page, layer).expect("valid synthetic texture reference")
}

fn animation(flags: u32) -> Animation {
    Animation {
        frame_start: 0,
        frame_count: 3,
        ticks_per_frame: 2,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags,
    }
}

#[test]
fn animation_selects_current_next_cross_page_wrap_and_non_blended_frames() {
    let frames = [texture_ref(0, 7), texture_ref(0, 8), texture_ref(1, 3)];
    let material = Material {
        texture: frames[0],
        flags: 0,
        animation: 0,
    };
    let sample = |tick, partial_tick, flags| {
        select_animation_frames(
            material,
            &[animation(flags)],
            &frames,
            ChunkAnimationClock::from_parts(tick, partial_tick),
        )
    };

    assert_eq!(
        sample(0, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.0)
    );
    assert_eq!(
        sample(1, 0.5, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.75)
    );
    assert_eq!(
        sample(2, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[1], frames[2], 0.0)
    );
    assert_eq!(
        sample(5, 0.5, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[2], frames[0], 0.75)
    );
    assert_eq!(
        sample(6, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.0)
    );
    assert_eq!(
        sample(3, 0.9, 0),
        AnimationFrameSample::new(frames[1], frames[1], 0.0)
    );

    let static_material = Material {
        texture: texture_ref(1, 4),
        flags: 0,
        animation: NO_ANIMATION,
    };
    assert_eq!(
        select_animation_frames(
            static_material,
            &[],
            &[],
            ChunkAnimationClock::from_parts(123, 0.75),
        ),
        AnimationFrameSample::new(static_material.texture, static_material.texture, 0.0)
    );

    let one_frame = Animation {
        frame_count: 1,
        ..animation(ANIMATION_FLAG_BLEND)
    };
    assert_eq!(
        select_animation_frames(
            material,
            &[one_frame],
            &frames[..1],
            ChunkAnimationClock::from_parts(1, 0.5),
        ),
        AnimationFrameSample::new(frames[0], frames[0], 0.0)
    );
}

#[test]
fn one_and_two_page_assets_have_one_stable_shared_binding_shape() {
    assert_eq!(
        plan_texture_page_bindings(1),
        Some([
            TexturePageBinding::Asset(0),
            TexturePageBinding::DiagnosticFallback,
        ])
    );
    assert_eq!(
        plan_texture_page_bindings(2),
        Some([TexturePageBinding::Asset(0), TexturePageBinding::Asset(1)])
    );
    assert_eq!(plan_texture_page_bindings(0), None);
    assert_eq!(plan_texture_page_bindings(3), None);
}

#[test]
fn one_page_fallback_is_a_real_one_layer_copy_of_diagnostic_mips() {
    let source = TextureArray {
        layers: 2,
        mips: [4_u32, 2, 1]
            .into_iter()
            .map(|size| {
                let layer_bytes = size as usize * size as usize * 4;
                let mut rgba8 = vec![0x11; layer_bytes];
                rgba8.extend(vec![0x22; layer_bytes]);
                TextureMip {
                    size,
                    rgba8: rgba8.into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };

    let fallback = diagnostic_texture_page(&source).expect("extract diagnostic page");
    assert_eq!(fallback.layers, 1);
    assert_eq!(fallback.mips.len(), source.mips.len());
    for (source_mip, fallback_mip) in source.mips.iter().zip(&fallback.mips) {
        let layer_bytes = source_mip.size as usize * source_mip.size as usize * 4;
        assert_eq!(
            fallback_mip.rgba8.as_ref(),
            &source_mip.rgba8[..layer_bytes]
        );
    }
}

#[test]
fn animation_clock_updates_do_not_rebuild_or_reupload_texture_assets() {
    let identity = ChunkTextureAssetIdentity::for_test(0x1000, 7);
    let mut current = None;
    let mut immutable_uploads = 0;
    let mut clock_bytes = 0;
    for frame in 0..120_u32 {
        if texture_asset_needs_rebuild(current, identity) {
            immutable_uploads += 1;
            current = Some(identity);
        }
        let clock = ChunkAnimationClock::from_elapsed_seconds(f64::from(frame) / 60.0);
        assert!(clock.partial_tick() >= 0.0 && clock.partial_tick() < 1.0);
        clock_bytes += size_of::<ChunkAnimationClock>();
    }
    assert_eq!(immutable_uploads, 1);
    assert_eq!(clock_bytes, 120 * 16);

    let plugin = include_str!("../src/plugin.rs");
    assert_eq!(plugin.matches("render_queue.write_texture(").count(), 1);
    assert_eq!(
        plugin.matches("render_queue.write_buffer(").count(),
        9,
        "shared writers cover immutable geometry plus bounded liquid and model transparent sorts"
    );
    assert!(plugin.contains("render_queue.write_buffer(&gpu_clock.buffer"));
}

#[test]
fn asset_revision_replacement_is_atomic_and_retains_the_previous_prepared_set_on_failure() {
    let plugin = include_str!("../src/plugin.rs");
    let start = plugin
        .find("fn prepare_chunk_texture_assets(")
        .expect("texture preparation system");
    let end = plugin[start..]
        .find("\nfn storage_table_fits(")
        .map(|offset| start + offset)
        .expect("end of texture preparation system");
    let prepare = &plugin[start..end];

    assert!(
        !prepare.contains("gpu_assets.prepared = None"),
        "a rejected new revision must retain the previous complete GPU asset set"
    );
    let second_page = prepare
        .find("let (texture_1, view_1, padded_1) = upload_texture_page(")
        .expect("second page is prepared before publication");
    let publish = prepare
        .find("gpu_assets.prepared = Some(PreparedChunkTextureAssets {")
        .expect("complete revision publication");
    assert!(second_page < publish);
    assert!(prepare.contains("material.texture.raw()"));
    assert!(prepare.contains(".animations()"));
    assert!(prepare.contains(".animation_frames()"));
    assert!(prepare.contains("_textures: [texture_0, texture_1]"));
    assert!(prepare.contains("views: [view_0, view_1]"));
}
