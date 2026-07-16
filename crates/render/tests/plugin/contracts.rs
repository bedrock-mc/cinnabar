use super::*;

#[test]
fn chunk_sampler_source_contract_is_crisp_for_magnification_and_filtered_for_mips() {
    let source = CHUNK_RENDERER_SOURCE;
    assert!(source.contains("mag_filter: FilterMode::Nearest"));
    assert!(source.contains("min_filter: FilterMode::Linear"));
    assert!(source.contains("mipmap_filter: FilterMode::Linear"));
    assert!(source.contains("anisotropy_clamp: 1"));
}

#[test]
fn graphics_runtime_metadata_waits_for_extracted_diagnostics_before_surface_probe() {
    let source = CHUNK_RENDERER_SOURCE.replace("\r\n", "\n");
    assert!(
        source.contains(
            "publish_graphics_runtime_metadata\n                        .after(RenderSystems::ExtractCommands)\n                        .before(bevy::render::view::window::create_surfaces)"
        ),
        "the metadata probe consumes an ExtractResource and must run after deferred extraction commands but before Bevy creates the surface"
    );
}

#[test]
fn shared_biome_bindings_are_visible_to_vertex_and_fragment_pipelines() {
    let source = CHUNK_RENDERER_SOURCE;
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
        ("chunk", include_str!("../../src/chunk.wgsl")),
        ("model", include_str!("../../src/model.wgsl")),
        ("liquid", include_str!("../../src/liquid.wgsl")),
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

    let source = CHUNK_RENDERER_SOURCE;
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
        texture_identity(assets as usize, assets),
        meshing::ChunkBiomeTintIdentity::new(tint, tint),
    )
    .unwrap()
}

fn exact_sort_key(camera: [f32; 3], orientation: [f32; 4]) -> ViewSortKey {
    ViewSortKey::try_new(
        camera,
        orientation,
        vec![],
        texture_identity(1, 1),
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
            texture_identity(1, 1),
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
fn transparent_pipeline_uses_alpha_without_depth_write() {
    let plugin = CHUNK_RENDERER_SOURCE;
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
    let plugin = CHUNK_RENDERER_SOURCE;
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
fn transparent_draw_evidence_scan_is_only_paid_for_an_active_frame_probe() {
    let plugin = CHUNK_RENDERER_SOURCE;
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
    let plugin = CHUNK_RENDERER_SOURCE;
    assert!(plugin.contains("last_indirect_identity"));
    assert!(plugin.contains("runtime.last_indirect_identity != Some(identity)"));
}

#[test]
fn task7_streams_share_one_physical_buffer_with_binding_headroom() {
    const CURRENT_VERTEX_STORAGE_BINDINGS: usize = 6;
    const MODEL_TEMPLATE_BINDINGS: usize = 1;
    let plugin = CHUNK_RENDERER_SOURCE;
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
    let plugin = CHUNK_RENDERER_SOURCE;
    let shader = include_str!("../../src/model.wgsl");
    assert!(plugin.contains("load_internal_asset!(app, MODEL_SHADER_HANDLE, \"../model.wgsl\""));
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
    let plugin = CHUNK_RENDERER_SOURCE;
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
    let shader = standalone_world_shader(include_str!("../../src/model.wgsl"));
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
    let source = CHUNK_RENDERER_SOURCE;
    assert!(source.contains("fn model_direct_draw_command("));
    assert!(source.contains("fn model_mdi_draw_command("));
    assert!(source.contains("model_draw_command(allocation, direct_stream_addresses(allocation))"));
    assert!(source.contains("model_draw_command(allocation, mdi_stream_addresses(allocation))"));
}

#[test]
fn transparent_model_pipeline_blends_without_depth_write_or_alpha_cutoff() {
    let plugin = CHUNK_RENDERER_SOURCE;
    let shader = include_str!("../../src/model.wgsl");

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
    let plugin = CHUNK_RENDERER_SOURCE;

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

    let plugin = CHUNK_RENDERER_SOURCE;
    let shader = include_str!("../../src/model.wgsl");
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

    let plugin = CHUNK_RENDERER_SOURCE.replace("\r\n", "\n");
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
        .add_plugins(ChunkRenderPlugin::new(1));
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
