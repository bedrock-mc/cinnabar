use crate::chunk::transparent::liquid::transparent_liquid_phase_distance;
use crate::chunk::*;

/// World views shared by opaque and transparent chunk queues.
type ChunkViewQuery = (
    Entity,
    Read<MainEntity>,
    Read<ExtractedView>,
    Read<RenderVisibleEntities>,
    Read<Msaa>,
    Option<Read<crate::EnhancedRendering>>,
);

/// Each indirect draw path's batches, cleared and refilled every queue.
type IndirectBatches = (
    ResMut<'static, ChunkIndirectBatches>,
    ResMut<'static, ChunkModelIndirectBatches>,
    ResMut<'static, ChunkDepthLiquidIndirectBatches>,
    ResMut<'static, pipeline::solid::ChunkSolidIndirectBatches>,
);

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn queue_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    mut opaque_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    (render_adapter, render_device): (Res<RenderAdapter>, Res<RenderDevice>),
    views: Query<ChunkViewQuery>,
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
    mut indirect_batch_sets: ParamSet<IndirectBatches>,
    mut gpu_culling: gpu_cull::GpuCullQueue,
    mut next_tick: Local<Tick>,
    mut unsupported_reported: Local<bool>,
) {
    let _timer = probes
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::OpaqueQueue));
    let frame_probe = &probes.frame_probe;
    let diagnostic_timer = probes
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::OpaqueDiagnostics));
    model_witness_resources
        .p2()
        .begin_frame(summarize_model_workload(allocations.iter()));
    drop(diagnostic_timer);
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend),
    );
    let diagnostic_timer = probes
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::OpaqueDiagnostics));
    if probes.input.enabled() {
        let diagnostic_view = views
            .iter()
            .min_by_key(|(_, main_entity, _, _, _, _)| main_entity.id().to_bits());
        if let Some((view_entity, main_entity, view, visible_entities, _, _)) = diagnostic_view {
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
    drop(diagnostic_timer);
    let draw_functions = draw_functions.read();
    let solid_direct_draw = draw_functions.id::<pipeline::solid::DrawSolidChunkCommands>();
    let solid_indirect_draw =
        draw_functions.id::<pipeline::solid::DrawSolidChunkIndirectCommands>();
    let direct_draw = draw_functions.id::<DrawChunkCommands>();
    let indirect_draw = draw_functions.id::<DrawChunkIndirectCommands>();
    let model_direct_draw = draw_functions.id::<DrawModelCommands>();
    let model_indirect_draw = draw_functions.id::<DrawModelIndirectCommands>();
    let depth_liquid_direct_draw = draw_functions.id::<DrawDepthLiquidCommands>();
    let depth_liquid_indirect_draw = draw_functions.id::<DrawDepthLiquidIndirectCommands>();
    indirect_batch_sets.p0().0.clear();
    indirect_batch_sets.p1().0.clear();
    indirect_batch_sets.p2().0.clear();
    indirect_batch_sets.p3().0.clear();
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
    let probing = frame_probe.is_active() || probes.input.enabled();
    let gpu_cull_view = gpu_culling.select(
        draw_mode,
        probing,
        views
            .iter()
            .map(|(entity, main, view, _, _, enhanced)| (entity, main, view, enhanced.is_some())),
    );
    let direct_view = gpu_culling.select_direct(
        draw_mode,
        probing,
        views
            .iter()
            .map(|(entity, main, view, _, _, enhanced)| (entity, main, view, enhanced.is_some())),
    );
    let frame_probe = &frame_probe.scope();
    for (view_entity, view_main_entity, view, visible_entities, msaa, enhanced) in &views {
        let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: view.hdr,
            enhanced: enhanced.is_some(),
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        let Ok(solid_pipeline_id) = pipeline.solid_variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        let Ok(model_pipeline_id) = pipeline.model_variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                enhanced: enhanced.is_some(),
            },
        ) else {
            continue;
        };
        let Ok(depth_liquid_pipeline_id) = pipeline.depth_liquid_variants.specialize(
            &pipeline_cache,
            ChunkPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                enhanced: enhanced.is_some(),
            },
        ) else {
            continue;
        };

        let diagnostic_timer = probes
            .profiler
            .as_deref()
            .map(|profiler| profiler.time(RuntimeStage::OpaqueDiagnostics));
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
        drop(diagnostic_timer);

        if gpu_cull_view == Some(view_entity) {
            let pipelines = [
                solid_pipeline_id,
                pipeline_id,
                model_pipeline_id,
                depth_liquid_pipeline_id,
            ];
            let early = gpu_cull::draw_function_ids(&draw_functions, false);
            for (draw_function, pipeline) in early.into_iter().zip(pipelines) {
                let this_tick = next_tick.get() + 1;
                next_tick.set(this_tick);
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function,
                        pipeline,
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
            }
            gpu_culling.set_view(gpu_cull::GpuCullView {
                entity: view_entity,
                main: *view_main_entity,
                pipelines,
                late_draws: gpu_cull::draw_function_ids(&draw_functions, true),
            });
            continue;
        }

        if draw_mode == ChunkDrawMode::MultiDrawIndirect {
            let _batch_timer = probes
                .profiler
                .as_deref()
                .map(|profiler| profiler.time(RuntimeStage::OpaqueBatchPlanning));
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
            let cube_entities = front_to_back_cube_entities(
                visible.iter().filter_map(|(entity, _)| {
                    allocations
                        .get(*entity)
                        .ok()
                        .map(|item| (*entity, item.key))
                }),
                &view.rangefinder3d(),
            );
            indirect_batch_sets.p3().0.insert(
                view_entity,
                pipeline::solid::ChunkSolidIndirectBatch {
                    camera: pipeline::solid::solid_cull_camera(view, enhanced.is_some()),
                    cubes: ChunkIndirectBatch {
                        visible_entities: cube_entities.clone(),
                        drawn_allocations: Vec::new(),
                        indirect_offset: 0,
                        command_count: 0,
                    },
                },
            );
            indirect_batch_sets.p0().0.insert(
                view_entity,
                ChunkIndirectBatch {
                    visible_entities: cube_entities,
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
                    draw_function: solid_indirect_draw,
                    pipeline: solid_pipeline_id,
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

        // `Some(true)` routes solid faces to the terrain pass instead of this phase.
        let terrain_pass = if direct_view == Some(view_entity) {
            gpu_culling.begin_direct(
                view_entity,
                view,
                *msaa,
                (solid_pipeline_id, solid_direct_draw),
            )
        } else {
            None
        };
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
            let solid = cube_stream_drawable(allocation);
            if terrain_pass.is_some()
                && let Some(frame) = gpu_culling.direct_frame()
            {
                frame.push(render_entity, solid);
            }
            // Solid before cutout, so the cutout bin follows it in insertion order.
            let cube_draws = [
                (
                    solid && terrain_pass != Some(true),
                    solid_direct_draw,
                    solid_pipeline_id,
                ),
                (
                    cutout_indirect_command(allocation).is_some(),
                    direct_draw,
                    pipeline_id,
                ),
            ];
            for (_, draw_function, pipeline) in cube_draws.into_iter().filter(|draw| draw.0) {
                let this_tick = next_tick.get() + 1;
                next_tick.set(this_tick);
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function,
                        pipeline,
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
    views: Query<ChunkViewQuery>,
    allocations: Query<&GpuChunkAllocation>,
    runtime: Res<TransparentSortRuntime>,
    instances: Query<&ChunkRenderInstance>,
    model_runtime: Res<TransparentModelSortRuntime>,
    texture_assets: Res<ChunkTextureAssets>,
    biome_tints: Res<ChunkBiomeTints>,
    mut mixed: ResMut<crate::chunk::transparent::mixed::MixedTerrainRuntime>,
    (arena, mut unsorted, mut water_order): (
        Res<ChunkGpuArena>,
        Local<UnsortedWaterDiagnostics>,
        Local<VisibleWaterOrder>,
    ),
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    use crate::chunk::transparent::mixed::DrawMixedTerrainCommands;
    mixed.begin_frame();
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::TransparentQueue));
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend),
    );
    if draw_mode == ChunkDrawMode::Unsupported {
        return;
    }
    let draw_functions = draw_functions.read();
    let transparent_model_draw = draw_functions.id::<DrawTransparentModelCommands>();
    let direct_draw = draw_functions.id::<DrawTransparentLiquidCommands>();
    let record_draw = draw_functions.id::<DrawTransparentLiquidDirectCommands>();
    let mixed_draw = draw_functions.id::<DrawMixedTerrainCommands>();
    for (view_entity, main_entity, view, visible_entities, msaa, enhanced) in &views {
        if runtime.view_entity != Some(view_entity) {
            continue;
        }
        let Some(phase) = transparent_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: view.hdr,
            enhanced: enhanced.is_some(),
        };
        let rangefinder = view.rangefinder3d();
        let model_pipeline_id = pipeline
            .transparent_model_variants
            .specialize(&pipeline_cache, key)
            .ok();
        let mut models = BTreeMap::new();
        let mut water = Vec::new();
        for &(entity, main) in visible_entities.get::<ChunkRenderInstance>() {
            let Ok(allocation) = allocations.get(entity) else {
                continue;
            };
            if transparent_model_direct_draw_command(allocation).is_some() {
                models.insert(allocation.key, (entity, main));
            }
            // Like opaque terrain, water waits for its re-upload under a new tint table.
            if transparent_liquid_direct_draw_command(allocation).is_some()
                && chunk_tint_identity_is_active(
                    allocation.tint_identity,
                    biome_tints.table_identity(),
                )
            {
                water.push(VisibleWater {
                    key: allocation.key,
                    entity,
                    main,
                    order_independent: allocation.order_independent_liquid,
                });
            }
        }
        // Equal phase distances keep insertion order, so water enters in key order.
        let water = water_order.update(water);
        // Displaced water can overlap itself even when flat, so such views sort all of it.
        let direct_order_independent = !view_displaces_water(enhanced.is_some());
        let mut merged = HashSet::new();
        let water_pipeline_id = pipeline
            .liquid_variants
            .specialize(&pipeline_cache, key)
            .ok();
        let snapshot = runtime.state.committed();
        let groups = snapshot.and_then(TransparentOrderedSnapshot::phase_groups);
        if snapshot.is_some() && groups.is_none() {
            bevy::log::error!(
                "committed transparent-liquid snapshot is not an exact contiguous sub-chunk partition"
            );
        }
        let camera = view.world_from_view.translation();
        for VisibleWater {
            key: water_key,
            entity,
            main,
            order_independent,
        } in each_visible_key(water, &arena.transparent_liquids)
        {
            let Some(water_pipeline_id) = water_pipeline_id else {
                break;
            };
            let direct = direct_order_independent && order_independent;
            // Water that needs an order draws the committed snapshot's group for it; the
            // frustum only chooses which of the groups appear.
            let sorted = groups
                .as_deref()
                .zip(snapshot)
                .filter(|_| !direct)
                .and_then(|(groups, committed)| {
                    let index = groups
                        .binary_search_by(|group| group.key.cmp(&water_key))
                        .ok()?;
                    Some((committed, &groups[index]))
                });
            let Some((committed, group)) = sorted else {
                unsorted.count += usize::from(!direct);
                phase.add(Transparent3d {
                    entity: (entity, main),
                    pipeline: water_pipeline_id,
                    draw_function: record_draw,
                    distance: transparent_liquid_phase_distance(&rangefinder, water_key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: true,
                });
                continue;
            };
            // Native deferred water uses layer 2, not ordinary blend layer 3.
            if enhanced.is_none()
                && let Some(model_pipeline_id) = model_pipeline_id
                && let Some(&(entity, main)) = models.get(&group.key)
                && let (Ok(instance), Ok(allocation)) =
                    (instances.get(entity), allocations.get(entity))
                && let Some(index) = mixed.plan(
                    view_entity,
                    camera,
                    entity,
                    instance,
                    allocation,
                    &model_runtime,
                    &texture_assets,
                    committed,
                    group,
                    water_pipeline_id,
                    model_pipeline_id,
                )
            {
                merged.insert(entity);
                phase.add(Transparent3d {
                    entity: (entity, main),
                    pipeline: model_pipeline_id,
                    draw_function: mixed_draw,
                    distance: transparent_model_phase_distance(&rangefinder, group.key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                        range: index..index + 1,
                        batch_set_index: None,
                    },
                    indexed: true,
                });
                continue;
            }
            phase.add(Transparent3d {
                entity: (view_entity, *main_entity),
                pipeline: water_pipeline_id,
                draw_function: direct_draw,
                distance: transparent_liquid_phase_distance(&rangefinder, group.key),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                    range: group.ref_range.clone(),
                    batch_set_index: None,
                },
                indexed: true,
            });
        }
        if let Some(model_pipeline_id) = model_pipeline_id {
            for (&model_key, &(entity, main)) in &models {
                if merged.contains(&entity) {
                    continue;
                }
                phase.add(Transparent3d {
                    entity: (entity, main),
                    pipeline: model_pipeline_id,
                    draw_function: transparent_model_draw,
                    distance: transparent_model_phase_distance(&rangefinder, model_key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: true,
                });
            }
        }
    }
    mixed.finish_frame();
    unsorted.finish_frame();
}

const UNSORTED_WATER_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// Counts visible water that needed a sort but drew in mesh order, because its sort had
/// not committed yet or the ref ceiling left it out.
#[derive(Default)]
pub(in crate::chunk) struct UnsortedWaterDiagnostics {
    count: usize,
    last_log: Option<Instant>,
}

impl UnsortedWaterDiagnostics {
    /// Logs a nonzero count at most once per interval, then starts the next frame's count.
    fn finish_frame(&mut self) {
        let count = std::mem::take(&mut self.count);
        let now = Instant::now();
        if count == 0
            || self
                .last_log
                .is_some_and(|last| now.duration_since(last) < UNSORTED_WATER_LOG_INTERVAL)
        {
            return;
        }
        self.last_log = Some(now);
        bevy::log::info!(
            sub_chunks = count,
            "transparent water drew unsorted while its sort was pending or over the ceiling"
        );
    }
}
