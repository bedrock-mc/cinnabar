use crate::chunk::*;

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn queue_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    mut opaque_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &RenderVisibleEntities,
        &Msaa,
    )>,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    allocations: Query<&GpuChunkAllocation>,
    arena: Res<ChunkGpuArena>,
    biome_tints: Res<ChunkBiomeTints>,
    mut model_witness_resources: ParamSet<(
        Res<PresentedFrameGate>,
        Res<ModelWitnessRequest>,
        Res<ModelWorkloadMetrics>,
    )>,
    mut probes: QueueFrameProbeParams,
    mut indirect_batch_sets: ParamSet<(
        ResMut<ChunkIndirectBatches>,
        ResMut<ChunkModelIndirectBatches>,
        ResMut<ChunkDepthLiquidIndirectBatches>,
    )>,
    mut next_tick: Local<Tick>,
    mut unsupported_reported: Local<bool>,
) {
    let frame_probe = &probes.frame_probe;
    model_witness_resources
        .p2()
        .begin_frame(summarize_model_workload(allocations.iter()));
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend).contains(Backends::DX12),
        cfg!(debug_assertions),
    );
    if probes.input.enabled() {
        let diagnostic_view = views
            .iter()
            .min_by_key(|(_, main_entity, _, _, _)| main_entity.id().to_bits());
        if let Some((view_entity, main_entity, view, visible_entities, _)) = diagnostic_view {
            let camera = extracted_camera_identity(main_entity, view);
            let generations = probes.camera_identity_tracker.observe(camera);
            let frustum_visible_opaque = visible_entities
                .get::<ChunkRenderInstance>()
                .iter()
                .filter_map(|(render_entity, _)| {
                    allocations
                        .get(*render_entity)
                        .ok()
                        .filter(|allocation| opaque_allocation_is_drawable(allocation))
                        .map(|allocation| allocation.key)
                });
            probes
                .visibility_probe
                .begin(VisibilityFrameProbe::begin_for_view(
                    probes.input.clone(),
                    view_entity,
                    camera,
                    generations,
                    diagnostic_draw_mode(draw_mode),
                    frustum_visible_opaque,
                    MAX_VISIBILITY_DIAGNOSTIC_KEYS,
                ));
        } else {
            probes.visibility_probe.clear();
        }
    } else {
        probes.visibility_probe.clear();
    }
    let draw_functions = draw_functions.read();
    let direct_draw = draw_functions.id::<DrawChunkCommands>();
    let indirect_draw = draw_functions.id::<DrawChunkIndirectCommands>();
    let model_direct_draw = draw_functions.id::<DrawModelCommands>();
    let model_indirect_draw = draw_functions.id::<DrawModelIndirectCommands>();
    let depth_liquid_direct_draw = draw_functions.id::<DrawDepthLiquidCommands>();
    let depth_liquid_indirect_draw = draw_functions.id::<DrawDepthLiquidIndirectCommands>();
    indirect_batch_sets.p0().0.clear();
    indirect_batch_sets.p1().0.clear();
    indirect_batch_sets.p2().0.clear();
    if draw_mode == ChunkDrawMode::Unsupported {
        frame_probe.clear();
        if !*unsupported_reported {
            bevy::log::error!(
                "packed chunk renderer requires DownlevelFlags::BASE_VERTEX; this adapter is unsupported"
            );
            *unsupported_reported = true;
        }
        return;
    }
    *unsupported_reported = false;
    if let Some(expectation) = model_witness_resources.p0().expectation() {
        let model_witness_request = (*model_witness_resources.p1()).clone();
        frame_probe.begin(FrameProbe::begin_with_model_witness(
            expectation,
            instances
                .iter()
                .map(|(entity, instance)| FrameInstanceIdentity {
                    entity,
                    key: instance.key,
                    generation: instance.generation,
                }),
            arena.allocations.iter().map(|(&entity, allocation)| {
                let model_ref_count = model_ref_count_for_witness(&allocation.gpu);
                (
                    FrameAllocationIdentity {
                        entity,
                        key: allocation.gpu.key,
                        generation: allocation.gpu.generation,
                    },
                    allocation.expected_streams(),
                    model_ref_count,
                )
            }),
            model_witness_request,
        ));
    } else {
        frame_probe.clear();
    }
    for (view_entity, view_main_entity, view, visible_entities, msaa) in &views {
        let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        let Ok(model_pipeline_id) = pipeline.model_variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        let Ok(depth_liquid_pipeline_id) = pipeline.depth_liquid_variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };

        model_witness_resources
            .p2()
            .record_visible(summarize_model_workload(
                visible_entities
                    .get::<ChunkRenderInstance>()
                    .iter()
                    .filter_map(|(render_entity, _)| {
                        let allocation = allocations.get(*render_entity).ok()?;
                        drawable_allocation_identity(
                            frame_probe,
                            *render_entity,
                            allocation,
                            biome_tints.table_identity(),
                        )?;
                        Some(allocation)
                    }),
            ));

        if draw_mode == ChunkDrawMode::MultiDrawIndirect {
            let visible = sorted_visible_entities(
                visible_entities
                    .get::<ChunkRenderInstance>()
                    .iter()
                    .copied(),
            )
            .into_iter()
            .filter(|(entity, _)| {
                let Ok(allocation) = allocations.get(*entity) else {
                    return false;
                };
                let Some(identity) = drawable_allocation_identity(
                    frame_probe,
                    *entity,
                    allocation,
                    biome_tints.table_identity(),
                ) else {
                    return false;
                };
                frame_probe.record_visible(*entity, identity)
            })
            .collect::<Vec<_>>();

            if visible.is_empty() {
                continue;
            }
            indirect_batch_sets.p0().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: visible
                        .iter()
                        .map(|(render_entity, _)| *render_entity)
                        .collect(),
                    drawn_allocations: Vec::new(),
                    indirect_offset: 0,
                    command_count: 0,
                },
            );
            indirect_batch_sets.p1().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: visible
                        .iter()
                        .map(|(render_entity, _)| *render_entity)
                        .collect(),
                    drawn_allocations: Vec::new(),
                    indirect_offset: 0,
                    command_count: 0,
                },
            );
            indirect_batch_sets.p2().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: visible
                        .iter()
                        .map(|(render_entity, _)| *render_entity)
                        .collect(),
                    drawn_allocations: Vec::new(),
                    indirect_offset: 0,
                    command_count: 0,
                },
            );

            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: indirect_draw,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: model_indirect_draw,
                    pipeline: model_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: depth_liquid_indirect_draw,
                    pipeline: depth_liquid_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            continue;
        }

        for &(render_entity, main_entity) in visible_entities.get::<ChunkRenderInstance>() {
            let Ok(allocation) = allocations.get(render_entity) else {
                continue;
            };
            let Some(identity) = drawable_allocation_identity(
                frame_probe,
                render_entity,
                allocation,
                biome_tints.table_identity(),
            ) else {
                continue;
            };
            if !frame_probe.record_visible(render_entity, identity) {
                continue;
            }
            let this_tick = next_tick.get() + 1;
            next_tick.set(this_tick);
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: direct_draw,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    vertex_slab: default(),
                    index_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (render_entity, main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
                *next_tick,
            );
            if model_direct_draw_command(allocation).is_some() {
                let this_tick = next_tick.get() + 1;
                next_tick.set(this_tick);
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: model_direct_draw,
                        pipeline: model_pipeline_id,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        vertex_slab: default(),
                        index_slab: None,
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::invalid().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                    *next_tick,
                );
            }
            if depth_liquid_direct_draw_command(allocation).is_some() {
                let this_tick = next_tick.get() + 1;
                next_tick.set(this_tick);
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: depth_liquid_direct_draw,
                        pipeline: depth_liquid_pipeline_id,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        vertex_slab: default(),
                        index_slab: None,
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::invalid().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                    *next_tick,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn queue_transparent_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    mut transparent_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &RenderVisibleEntities,
        &Msaa,
    )>,
    allocations: Query<&GpuChunkAllocation>,
    runtime: Res<TransparentSortRuntime>,
) {
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend).contains(Backends::DX12),
        cfg!(debug_assertions),
    );
    if draw_mode == ChunkDrawMode::Unsupported {
        return;
    }
    let draw_functions = draw_functions.read();
    let transparent_model_draw = draw_functions.id::<DrawTransparentModelCommands>();
    let direct_draw = draw_functions.id::<DrawTransparentLiquidCommands>();
    for (view_entity, main_entity, view, visible_entities, msaa) in &views {
        if runtime.view_entity != Some(view_entity) {
            continue;
        }
        let Some(phase) = transparent_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: view.hdr,
        };
        let rangefinder = view.rangefinder3d();
        if let Ok(model_pipeline_id) = pipeline
            .transparent_model_variants
            .specialize(&pipeline_cache, key)
        {
            for &(render_entity, main_entity) in visible_entities.get::<ChunkRenderInstance>() {
                let Ok(allocation) = allocations.get(render_entity) else {
                    continue;
                };
                if transparent_model_direct_draw_command(allocation).is_none() {
                    continue;
                }
                phase.add(Transparent3d {
                    entity: (render_entity, main_entity),
                    pipeline: model_pipeline_id,
                    draw_function: transparent_model_draw,
                    distance: transparent_model_phase_distance(&rangefinder, allocation.key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: true,
                });
            }
        }

        let Some(snapshot) = runtime.state.committed() else {
            continue;
        };
        if snapshot.refs().is_empty() || runtime.view_entity != Some(view_entity) {
            continue;
        }
        let Some(groups) = transparent_liquid_phase_groups(snapshot) else {
            bevy::log::error!(
                "committed transparent-liquid snapshot is not an exact contiguous sub-chunk partition"
            );
            continue;
        };
        let Ok(pipeline_id) = pipeline.liquid_variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        // Keep each sub-chunk's worker-sorted water refs contiguous while
        // giving water and blend models the same phase-distance contract.
        for group in groups {
            phase.add(Transparent3d {
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function: direct_draw,
                distance: transparent_liquid_phase_distance(&rangefinder, group.key),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                    range: group.ref_range,
                    batch_set_index: None,
                },
                indexed: true,
            });
        }
    }
}
