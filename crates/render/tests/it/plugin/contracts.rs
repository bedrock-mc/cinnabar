use super::*;

/// Validate compiler-visible storage and fragment inputs against the production device policy.
#[test]
fn model_shader_storage_and_fragment_inputs_fit_the_shared_pipeline_contract() {
    let source = standalone_world_shader(include_str!("../../../src/model.wesl"));
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    for (index, _) in module.entry_points.iter().enumerate() {
        let usage = info.get_entry_point(index);
        let storage = module
            .global_variables
            .iter()
            .filter(|(handle, variable)| {
                matches!(variable.space, naga::AddressSpace::Storage { .. })
                    && !usage[*handle].is_empty()
            })
            .count();
        assert!(storage <= render::required_vertex_storage_buffers() as usize);
    }
    let fragment = module
        .entry_points
        .iter()
        .find(|entry| entry.stage == naga::ShaderStage::Fragment && entry.name == "fragment")
        .unwrap();
    assert!(fragment.function.arguments.iter().any(
        |argument| argument.binding == Some(naga::Binding::BuiltIn(naga::BuiltIn::FrontFacing))
    ));
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
    visible: Vec<TransparentAllocationIdentity>,
    assets: u64,
    tint: u64,
) -> ViewSortKey {
    ViewSortKey::try_new(
        camera.map(|value| value as f32),
        visible,
        texture_assets(assets as usize, assets).identity(),
        meshing::ChunkBiomeTintIdentity::new(tint, tint),
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
    let first_key = sort_key([0, 0, 0], visible.clone(), 2, 3);
    let first = state.request(&first_key);
    assert_eq!(
        state.request(&first_key),
        first,
        "unchanged outstanding work is reused"
    );
    let rotated_key = sort_key([1, 0, 0], visible, 2, 3);
    let rotated = state.request(&rotated_key);
    assert!(
        first < rotated,
        "camera position near visible water is part of the key"
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
    let first_key = sort_key([0, 0, 0], visible.clone(), 1, 1);
    let first = state.request(&first_key);
    assert_eq!(state.complete(sort_result(first, first_key, 7)), Ok(false));
    let upload = state.next_upload_batch().unwrap();
    assert_eq!(upload.buffer_slot(), 0);
    assert_eq!(upload.ref_range(), 0..1);
    assert_eq!(upload.refs(), &[PackedTransparentDrawRef::new(7, 107)]);
    assert!(state.acknowledge_upload());
    let committed: TransparentOrderedSnapshot = state.committed().unwrap().clone();
    let second_key = sort_key([1, 0, 0], visible, 1, 1);
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
    let base = sort_key([0, 0, 0], vec![a.clone()], 10, 20);
    for unsafe_key in [
        sort_key([0, 0, 0], vec![], 10, 20),
        sort_key([0, 0, 0], vec![allocation(a.key(), 2, 8)], 10, 20),
        sort_key([0, 0, 0], vec![a.clone()], 11, 20),
        sort_key([0, 0, 0], vec![a.clone()], 10, 21),
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
    let initial = sort_key([0, 0, 0], visible, 10, 20);
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

    let unsafe_key = sort_key([0, 0, 0], vec![], 10, 20);
    state.request(&unsafe_key);
    assert_eq!(state.staged_ref_count(), 0);
    assert!(state.next_upload_batch().is_none());
}

#[test]
fn camera_motion_cannot_starve_a_partially_uploaded_water_sort() {
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let initial = sort_key([0, 0, 0], visible.clone(), 10, 20);
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

    let moved_once = sort_key([1, 0, 0], visible.clone(), 10, 20);
    assert_eq!(
        state.request(&moved_once),
        staged_generation,
        "camera-only motion must finish the bounded inactive-slot upload"
    );
    assert_eq!(state.next_upload_batch().unwrap().refs(), &refs[..1]);
    assert!(!state.acknowledge_upload());

    let moved_again = sort_key([2, 0, 0], visible, 10, 20);
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
    let forward = sort_key([1, 2, 3], vec![a.clone(), b.clone()], 9, 10);
    let reverse = sort_key([1, 2, 3], vec![b, a.clone(), a], 9, 10);
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
            vec![allocation(key, 1, 8), allocation(key, 2, 16)],
            texture_assets(1, 1).identity(),
            meshing::ChunkBiomeTintIdentity::new(1, 1),
        ),
        Err(TransparentSortError::ConflictingAllocation { key })
    );
}

#[test]
fn sort_key_tracks_quantized_position_only_while_water_is_near() {
    let near = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let far = vec![allocation(SubChunkKey::new(0, 4, 0, 0), 1, 8)];
    let key = |camera: [f32; 3], visible: &Vec<TransparentAllocationIdentity>| {
        ViewSortKey::try_new(
            camera,
            visible.clone(),
            texture_assets(1, 1).identity(),
            meshing::ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap()
    };
    assert_eq!(key([1.0, 2.0, 3.0], &near), key([1.001, 2.0, 3.0], &near));
    assert_ne!(key([1.0, 2.0, 3.0], &near), key([1.25, 2.0, 3.0], &near));
    assert_eq!(key([1.0, 2.0, 3.0], &far), key([1.25, 2.0, 3.0], &far));
    assert_ne!(key([1.0, 2.0, 3.0], &far), key([17.0, 2.0, 3.0], &far));
}

#[test]
#[allow(clippy::single_range_in_vec_init)] // One patched span is the expectation.
fn unchanged_transparent_order_reuses_committed_slot_without_upload() {
    let visible = vec![allocation(SubChunkKey::new(0, 0, 0, 0), 1, 8)];
    let first_key = sort_key([0, 0, 0], visible.clone(), 1, 1);
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

    let camera_only = sort_key([1, 0, 0], visible.clone(), 1, 1);
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

    let changed_key = sort_key([2, 0, 0], visible, 1, 1);
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
        Ok(true),
        "an unchanged address set is patched in place"
    );
    assert!(state.next_upload_batch().is_none());
    assert_eq!(state.take_patch(), [0..2]);
    assert_eq!(
        state.committed().unwrap().buffer_slot(),
        committed.buffer_slot()
    );
    assert_eq!(state.committed().unwrap().generation(), third);
}

#[test]
fn zero_transparent_upload_cap_still_makes_bounded_progress() {
    let key = sort_key([0, 0, 0], vec![], 1, 1);
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
    let key = sort_key([0, 0, 0], vec![], 1, 1);
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
}

/// Counts ordinary render entities separately from Bevy resource entities.
fn flowerbed_render_entity_contract(mesh: meshing::ChunkMesh) -> (u32, usize, Vec<String>, bool) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));
    let mut ordinary_entities = app.world_mut().query_filtered::<bevy::prelude::Entity, bevy::prelude::Without<bevy::ecs::resource::IsResource>>();
    let entity_count_before = ordinary_entities.iter(app.world()).count();
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
        u32::try_from(ordinary_entities.iter(app.world()).count() - entity_count_before).unwrap(),
        entities.len(),
        component_names,
        has_mesh3d,
    )
}
