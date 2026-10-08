use super::*;

#[test]
fn biome_tint_table_is_revisioned_and_keeps_a_fallback_entry() {
    let fallback = ChunkBiomeTints::default();
    assert_eq!(fallback.entries().len(), 1);
    assert_eq!(fallback.revision(), 0);
    assert_eq!(prepare_biome_tint_entries(fallback.entries()).len(), 1);

    let empty = ChunkBiomeTints::with_revision(Arc::from([]), 7);
    assert_eq!(empty.entries().len(), 1);
    assert_eq!(empty.revision(), 7);

    let shared_entries = Arc::from([BiomeTint::default()]);
    let first = ChunkBiomeTints::with_revision(Arc::clone(&shared_entries), 7);
    let replacement = ChunkBiomeTints::with_revision(shared_entries, 8);
    assert_ne!(first.resource_identity(), replacement.resource_identity());

    assert_eq!(pack_linear_rgb10([0.0, 0.0, 0.0]), 0);
    assert_eq!(pack_linear_rgb10([1.0, 1.0, 1.0]), 0x3fff_ffff);
}

#[test]
fn biome_gpu_entries_pack_all_six_tint_classes_and_flags() {
    let entry = BiomeTint {
        grass: [0.1, 0.2, 0.3],
        foliage: [0.2, 0.3, 0.4],
        birch: [0.3, 0.4, 0.5],
        evergreen: [0.4, 0.5, 0.6],
        dry_foliage: [0.5, 0.6, 0.7],
        seasonal_foliage: std::array::from_fn(|index| {
            [index as f32 / assets::SEASONAL_FOLIAGE_COUNT as f32; 3]
        }),
        water: [0.6, 0.7, 0.8],
        water_opacity: 165.0 / 255.0,
        flags: 0x5a,
    };
    let gpu = prepare_biome_tint_entries(&[entry])[0];

    assert_eq!(gpu.grass, pack_linear_rgb10(entry.grass));
    assert_eq!(gpu.foliage, pack_linear_rgb10(entry.foliage));
    assert_eq!(gpu.birch, pack_linear_rgb10(entry.birch));
    assert_eq!(gpu.evergreen, pack_linear_rgb10(entry.evergreen));
    assert_eq!(gpu.dry_foliage, pack_linear_rgb10(entry.dry_foliage));
    assert_eq!(
        gpu.water,
        u32::from_le_bytes(
            Color::linear_rgb(entry.water[0], entry.water[1], entry.water[2])
                .to_srgba()
                .to_u8_array()
        )
    );
    assert_eq!(
        gpu.seasonal_foliage,
        entry.seasonal_foliage.map(|[r, g, b]| [r, g, b, 1.0])
    );
    assert_eq!(gpu.flags, entry.flags);
    assert_eq!(gpu.water_opacity, entry.water_opacity);
}

#[test]
fn tint_table_identity_rebuilds_the_gpu_buffer_and_shared_bind_group() {
    let entries = Arc::from([BiomeTint::default()]);
    let first =
        ChunkBiomeTints::with_identity(Arc::clone(&entries), ChunkBiomeTintIdentity::new(4, 7));
    let replacement = ChunkBiomeTints::with_identity(entries, ChunkBiomeTintIdentity::new(5, 7));
    let first_identity = first.resource_identity();
    let replacement_identity = replacement.resource_identity();

    assert!(!biome_tint_gpu_buffer_needs_rebuild(
        Some(first_identity),
        first_identity,
    ));
    assert!(biome_tint_gpu_buffer_needs_rebuild(
        Some(first_identity),
        replacement_identity,
    ));
    assert!(biome_tint_bind_group_needs_rebuild(
        Some(first_identity),
        replacement_identity,
    ));
}

#[test]
fn seasonal_palette_content_rebuilds_gpu_buffer_without_revising_dense_table() {
    let table = ChunkBiomeTintIdentity::new(4, 7);
    let first = ChunkBiomeTints::with_identity(Arc::from([BiomeTint::default()]), table);
    let mut entry = BiomeTint::default();
    entry.seasonal_foliage[assets::SEASONAL_FOLIAGE_EXPOSED_OFFSET] = [1.0; 3];
    let next = ChunkBiomeTints::with_identity(Arc::from([entry]), table);
    assert_eq!(first.table_identity(), next.table_identity());
    assert_ne!(
        prepare_biome_tint_entries(first.entries())[0].seasonal_foliage,
        prepare_biome_tint_entries(next.entries())[0].seasonal_foliage
    );
    assert!(biome_tint_gpu_buffer_needs_rebuild(
        Some(first.resource_identity()),
        next.resource_identity()
    ));
    assert!(biome_tint_bind_group_needs_rebuild(
        Some(first.resource_identity()),
        next.resource_identity()
    ));
}

#[test]
fn world_seasonal_gpu_tints_preserve_channels_above_one() {
    let mut entry = BiomeTint::default();
    let index = assets::SEASONAL_FOLIAGE_EXPOSED_OFFSET;
    entry.seasonal_foliage[index] = [1.25, 2.0, 3.5];
    let gpu = prepare_biome_tint_entries(&[entry])[0];
    assert_eq!(gpu.seasonal_foliage[index], [1.25, 2.0, 3.5, 1.0]);
}

#[test]
fn matching_identity_uploads_acks_and_queues_direct_and_mdi_draws() {
    fn solid_sub_chunk() -> world::SubChunk {
        world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 })
    }

    let active = ChunkBiomeTintIdentity::new(4, 7);
    let mismatched = ChunkBiomeTintIdentity::new(5, 7);
    let matching_key = SubChunkKey::new(0, 0, 0, 0);
    let mismatched_key = SubChunkKey::new(0, 1, 0, 0);
    let now = Instant::now();
    let matching_token = ChunkUploadToken {
        generation: 1,
        dirty_since: now,
    };
    let mismatched_token = ChunkUploadToken {
        generation: 2,
        dirty_since: now,
    };
    let solid = solid_sub_chunk();
    let mesh = || {
        meshing::mesh_sub_chunk(
            &meshing::BlockClassifier::new(0),
            opaque_runtime_assets(),
            assets::NetworkIdMode::Sequential,
            &meshing::Neighbourhood::empty(),
            &solid,
        )
    };
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(acknowledgements.clone())
        .insert_resource(ChunkBiomeTints::with_identity(
            Arc::from([BiomeTint::default()]),
            active,
        ))
        .add_plugins(ChunkRenderPlugin::new(2));
    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update_tracked_with_biome_identity(
                matching_key,
                mesh(),
                PackedBiomeRecord::fallback(),
                active,
                ChunkUploadPriority::new(0.0),
                matching_token,
            )
            .unwrap();
        queue
            .try_update_tracked_with_biome_identity(
                mismatched_key,
                mesh(),
                PackedBiomeRecord::fallback(),
                mismatched,
                ChunkUploadPriority::new(1.0),
                mismatched_token,
            )
            .unwrap();
    }
    app.update();
    let instances = app
        .world_mut()
        .query::<(Entity, &ChunkRenderInstance)>()
        .iter(app.world())
        .map(|(entity, instance)| (entity, instance.clone()))
        .collect::<HashMap<_, _>>();
    let candidates = instances
        .iter()
        .map(|(&entity, instance)| GpuUpdateCandidate {
            entity,
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            priority: instance.priority,
        })
        .collect::<Vec<_>>();
    let selected = plan_gpu_chunk_updates(
        candidates,
        &HashMap::new(),
        Vec3::ZERO,
        active,
        &GpuUpdateFairness::default(),
    );
    assert_eq!(selected.len(), 1);
    let selected_entity = selected[0];
    let selected_instance = &instances[&selected_entity];
    assert_eq!(selected_instance.key, matching_key);
    assert!(acknowledgements.try_reserve(matching_key, matching_token));
    assert!(acknowledgements.complete_with_bytes(matching_key, matching_token, now, 64,));
    let acked = acknowledgements.drain();
    assert_eq!(acked.len(), 1);
    assert_eq!(acked[0].key, matching_key);

    let allocations = instances
        .iter()
        .enumerate()
        .map(|(index, (&entity, instance))| {
            (
                entity,
                GpuChunkAllocation {
                    cube_layout: CubeQuadLayout::default(),
                    key: instance.key,
                    generation: instance.generation,
                    tint_identity: instance.tint_identity,
                    quad_range: (index as u32 * 6)..(index as u32 * 6 + 6),
                    cube_lighting_range: Some((200 + index as u32 * 12)..(212 + index as u32 * 12)),
                    model_range: None,
                    model_lighting_range: None,
                    model_draw_range: None,
                    transparent_model_draw_range: None,
                    liquid_range: None,
                    liquid_lighting_range: None,
                    has_depth_liquid: false,
                    has_transparent_liquid: false,
                    depth_liquid_range: None,
                    metadata_index: index as u32,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let frame_probe = ActiveFrameProbe::default();
    let frame_probe = frame_probe.scope();
    let direct = allocations
        .iter()
        .filter_map(|(&entity, allocation)| {
            drawable_allocation_identity(&frame_probe, entity, allocation, active)
        })
        .collect::<Vec<_>>();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].key, matching_key);

    let (commands, drawn) = prepare_indirect_batch_draws(
        allocations
            .iter()
            .map(|(&entity, allocation)| (entity, allocation)),
        &frame_probe,
        active,
    );
    assert_eq!(commands.len(), 1);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1.key, matching_key);
    assert!(acknowledgements.drain().is_empty());
}
