use crate::chunk::pipeline::batches::{
    DrawableTerrain, RetainedTerrainBatches, retain_indirect_views,
};
use crate::chunk::transparent::liquid::transparent_liquid_phase_distance;
use crate::chunk::*;
use render_scene::{
    ChunkBiomeTints, ChunkRenderInstance, ChunkTextureAssets, MAX_VISIBILITY_DIAGNOSTIC_KEYS,
    ModelWitnessRequest, ModelWorkloadMetrics, PresentedFrameGate, RuntimeStageProfiler,
};

/// World views shared by opaque and transparent chunk queues.
type ChunkViewQuery = (
    Entity,
    Read<MainEntity>,
    Read<ExtractedView>,
    Read<RenderVisibleEntities>,
    Read<Msaa>,
    Option<Read<render_scene::EnhancedRendering>>,
    Read<bevy::render::camera::ExtractedCamera>,
);

/// Each indirect draw path's batches, retained while its view remains active.
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
    (arena, mut retained, mut units): (
        Res<ChunkGpuArena>,
        ResMut<RetainedTerrainBatches>,
        ResMut<pipeline::units::TerrainUnitBatches>,
    ),
    biome_tints: Res<ChunkBiomeTints>,
    mut model_witness_resources: ParamSet<(
        Res<PresentedFrameGate>,
        Res<ModelWitnessRequest>,
        Res<ModelWorkloadMetrics>,
    )>,
    mut probes: QueueFrameProbeParams,
    mut indirect_batch_sets: ParamSet<IndirectBatches>,
    mut gpu_culling: gpu_cull::GpuCullQueue,
    pack: render_scene::shaderpack::ShaderPackDrawnViews,
    mut unsupported_reported: Local<bool>,
) {
    let _timer = probes
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::OpaqueQueue));
    let frame_probe = &probes.frame_probe;
    let changes = retained.begin_frame();
    units.begin_frame();
    let diagnostic_timer = probes
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::OpaqueDiagnostics));
    let resident_models =
        retained.resident_models(&changes, || summarize_model_workload(allocations.iter()));
    model_witness_resources.p2().begin_frame(resident_models);
    drop(diagnostic_timer);
    let draw_mode = select_chunk_draw_mode(
        render_adapter.get_downlevel_capabilities().flags,
        render_device.features(),
        Backends::from(render_adapter.get_info().backend),
        apple_gpu(&render_adapter.get_info()),
    );
    let diagnostic_timer = probes
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::OpaqueDiagnostics));
    if probes.input.enabled() {
        let diagnostic_view = views
            .iter()
            .min_by_key(|(_, main_entity, _, _, _, _, _)| main_entity.id().to_bits());
        if let Some((view_entity, main_entity, view, visible_entities, _, _, _)) = diagnostic_view {
            let camera = extracted_camera_identity(main_entity, view);
            let generations = probes.camera_identity_tracker.observe(camera);
            let frustum_visible_opaque = visible_entities
                .get::<ChunkRenderInstance>()
                .into_iter()
                .flat_map(|class| class.iter_visible())
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
    // Keep active views' indirect batches on either draw path.
    let mut indirect_views = Vec::new();
    if draw_mode == ChunkDrawMode::Unsupported {
        retain_queued_indirect_views(&mut indirect_batch_sets, &indirect_views, &mut retained);
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
    // A frame probe filters the lists by its own manifest, so they are neither reused nor kept.
    let reusable = !frame_probe.is_active();
    let direct_streams = draw_mode == ChunkDrawMode::Direct;
    // A shader pack that draws terrain shows nothing where it has not expanded a section yet,
    // so its cameras get no Vanilla terrain to show through its overlay.
    let pack_terrain = |view: Entity| pack.replaces(view, |drawn| drawn.terrain);
    let gpu_cull_view = gpu_culling.select(
        draw_mode,
        probing,
        views
            .iter()
            .filter(|(entity, ..)| !pack_terrain(*entity))
            .map(|(entity, main, view, _, _, enhanced, _)| {
                (entity, main, view, enhanced.is_some())
            }),
    );
    let direct_view = gpu_culling.select_direct(
        draw_mode,
        probing,
        views
            .iter()
            .filter(|(entity, ..)| !pack_terrain(*entity))
            .map(|(entity, main, view, _, _, enhanced, _)| {
                (entity, main, view, enhanced.is_some())
            }),
    );
    let frame_probe = &frame_probe.scope();
    for (view_entity, view_main_entity, view, visible_entities, msaa, enhanced, extracted_camera) in
        &views
    {
        if pack_terrain(view_entity) {
            continue;
        }
        let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: extracted_camera.hdr,
            enhanced: enhanced.is_some(),
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        let Ok(solid_pipeline_id) = pipeline.solid_variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        let Ok(model_pipeline_id) = pipeline.model_variants.specialize(&pipeline_cache, key) else {
            continue;
        };
        let Ok(depth_liquid_pipeline_id) = pipeline
            .depth_liquid_variants
            .specialize(&pipeline_cache, key)
        else {
            continue;
        };

        let lists = retained.refresh_view(
            view_entity,
            visible_entities.get::<ChunkRenderInstance>(),
            &changes,
            reusable,
        );
        let diagnostic_timer = probes
            .profiler
            .as_deref()
            .map(|profiler| profiler.time(RuntimeStage::OpaqueDiagnostics));
        let visible_models = lists.visible_models(|source| {
            summarize_model_workload(source.iter().filter_map(|(render_entity, _)| {
                let allocation = allocations.get(*render_entity).ok()?;
                drawable_allocation_identity(
                    frame_probe,
                    *render_entity,
                    allocation,
                    biome_tints.table_identity(),
                )?;
                Some(allocation)
            }))
        });
        model_witness_resources.p2().record_visible(visible_models);
        drop(diagnostic_timer);
        let admit = |entity: Entity, main: MainEntity| {
            let allocation = allocations.get(entity).ok()?;
            let identity = drawable_allocation_identity(
                frame_probe,
                entity,
                allocation,
                biome_tints.table_identity(),
            )?;
            frame_probe
                .record_visible(entity, identity)
                .then(|| DrawableTerrain::new(entity, main, allocation, direct_streams))
        };

        if gpu_cull_view == Some(view_entity) {
            // Retain CPU commands and their buffer offsets without queueing their draws.
            indirect_views.push(view_entity);
            let pipelines = [
                solid_pipeline_id,
                pipeline_id,
                model_pipeline_id,
                depth_liquid_pipeline_id,
            ];
            let early = gpu_cull::draw_function_ids(&draw_functions, false);
            for (draw_function, pipeline) in early.into_iter().zip(pipelines) {
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function,
                        pipeline,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        slabs: default(),
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (view_entity, *view_main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
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
            if lists.drawable(admit).is_empty() {
                continue;
            }
            let requeue = !indirect_batch_sets.p0().0.contains_key(&view_entity);
            let update = lists.indirect_lists(
                view.world_from_view.to_matrix(),
                &view.rangefinder3d(),
                requeue,
            );
            retained.stats.cube_sorts += u64::from(update.sorted);
            if let Some(visible) = update.visible {
                indirect_batch_sets
                    .p1()
                    .0
                    .insert(view_entity, ChunkIndirectBatch::new(visible.clone()));
                indirect_batch_sets
                    .p2()
                    .0
                    .insert(view_entity, ChunkIndirectBatch::new(visible));
            }
            if let Some(cubes) = update.cubes {
                indirect_batch_sets
                    .p0()
                    .0
                    .insert(view_entity, ChunkIndirectBatch::new(cubes.clone()));
                indirect_batch_sets.p3().0.insert(
                    view_entity,
                    pipeline::solid::ChunkSolidIndirectBatch::new(cubes),
                );
            }
            if let Some(solid) = indirect_batch_sets.p3().0.get_mut(&view_entity) {
                solid.camera = pipeline::solid::solid_cull_camera(view, enhanced.is_some());
            }
            indirect_views.push(view_entity);

            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: solid_indirect_draw,
                    pipeline: solid_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::default().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );

            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: indirect_draw,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::default().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );

            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: model_indirect_draw,
                    pipeline: model_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::default().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );

            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: depth_liquid_indirect_draw,
                    pipeline: depth_liquid_pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::default().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );
            continue;
        }

        let use_units =
            !probing && enhanced.is_none() && !units.disabled && pipeline.unit_supported;
        if use_units {
            lists.drawable(admit);
            let sources = lists.units(&allocations);
            let draws = [
                draw_functions.id::<pipeline::units::commands::DrawTerrainUnits<0>>(),
                draw_functions.id::<pipeline::units::commands::DrawTerrainUnits<1>>(),
                draw_functions.id::<pipeline::units::commands::DrawTerrainUnits<2>>(),
                draw_functions.id::<pipeline::units::commands::DrawTerrainUnits<3>>(),
            ];
            let [Ok(solid), Ok(cutout), Ok(model), Ok(liquid)] = pipeline
                .unit_variants
                .each_mut()
                .map(|variants| variants.specialize(&pipeline_cache, key))
            else {
                continue;
            };
            let pipelines = [solid, cutout, model, liquid];
            let terrain_pass = if direct_view == Some(view_entity) {
                gpu_culling.begin_direct(view_entity, view, *msaa, (pipelines[0], draws[0]))
            } else {
                None
            };
            if terrain_pass.is_some()
                && let Some(frame) = gpu_culling.direct_frame()
            {
                for terrain in lists.drawable(admit) {
                    frame.push(terrain.entity, terrain.solid);
                }
            }
            for &stream in &sources.order {
                if stream == 0 && terrain_pass == Some(true) {
                    continue;
                }
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: draws[stream],
                        pipeline: pipelines[stream],
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        slabs: default(),
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (view_entity, *view_main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                );
                units.stats.phase_items += 1;
            }
            units.queue(
                view_entity,
                sources,
                pipeline::solid::solid_cull_camera(view, false),
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
        for terrain in lists.drawable(admit) {
            let (render_entity, main_entity) = (terrain.entity, terrain.main);
            if terrain_pass.is_some()
                && let Some(frame) = gpu_culling.direct_frame()
            {
                frame.push(render_entity, terrain.solid);
            }
            // Solid before cutout, so the cutout bin follows it in insertion order.
            let cube_draws = [
                (
                    terrain.solid && terrain_pass != Some(true),
                    solid_direct_draw,
                    solid_pipeline_id,
                ),
                (terrain.cutout, direct_draw, pipeline_id),
            ];
            for (_, draw_function, pipeline) in cube_draws.into_iter().filter(|draw| draw.0) {
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function,
                        pipeline,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        slabs: default(),
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                );
            }
            if terrain.model {
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: model_direct_draw,
                        pipeline: model_pipeline_id,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        slabs: default(),
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                );
            }
            if terrain.depth_liquid {
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: depth_liquid_direct_draw,
                        pipeline: depth_liquid_pipeline_id,
                        material_bind_group_index: None,
                        lightmap_slab: None,
                        slabs: default(),
                    },
                    Opaque3dBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (render_entity, main_entity),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                );
            }
        }
    }
    retain_queued_indirect_views(&mut indirect_batch_sets, &indirect_views, &mut retained);
}

/// Drops the indirect batches of views not in `views`, so the next prepare lays the rest out again.
fn retain_queued_indirect_views(
    sets: &mut ParamSet<'_, '_, IndirectBatches>,
    views: &[Entity],
    retained: &mut RetainedTerrainBatches,
) {
    let mut dropped = retain_indirect_views(&mut sets.p0().0, views);
    dropped |= retain_indirect_views(&mut sets.p1().0, views);
    dropped |= retain_indirect_views(&mut sets.p2().0, views);
    dropped |= retain_indirect_views(&mut sets.p3().0, views);
    retained.layout_changed |= dropped;
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
    (arena, mut unsorted, mut water_order, mut scratch, pack): (
        Res<ChunkGpuArena>,
        Local<UnsortedWaterDiagnostics>,
        Local<VisibleWaterOrder>,
        Local<TransparentQueueScratch>,
        render_scene::shaderpack::ShaderPackDrawnViews,
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
        apple_gpu(&render_adapter.get_info()),
    );
    if draw_mode == ChunkDrawMode::Unsupported {
        return;
    }
    let draw_functions = draw_functions.read();
    let transparent_model_draw = draw_functions.id::<DrawTransparentModelCommands>();
    let direct_draw = draw_functions.id::<DrawTransparentLiquidCommands>();
    let record_draw = draw_functions.id::<DrawTransparentLiquidDirectCommands>();
    let mixed_draw = draw_functions.id::<DrawMixedTerrainCommands>();
    for (view_entity, main_entity, view, visible_entities, msaa, enhanced, extracted_camera) in
        &views
    {
        if runtime.view_entity != Some(view_entity)
            || pack.replaces(view_entity, |drawn| drawn.terrain)
        {
            continue;
        }
        let Some(phase) = transparent_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: extracted_camera.hdr,
            enhanced: enhanced.is_some(),
        };
        let rangefinder = view.rangefinder3d();
        let Ok(pipeline_id) = pipeline
            .transparent_variants
            .specialize(&pipeline_cache, key)
        else {
            continue;
        };
        let TransparentQueueScratch { models, water } = &mut *scratch;
        models.clear();
        water.clear();
        for (entity, main) in visible_entities
            .get::<ChunkRenderInstance>()
            .into_iter()
            .flat_map(|class| class.iter_visible())
            .map(|(entity, main)| (*entity, *main))
        {
            let Ok(allocation) = allocations.get(entity) else {
                continue;
            };
            if transparent_model_direct_draw_command(allocation).is_some() {
                models.push(QueuedTransparentModel {
                    key: allocation.key,
                    entity,
                    main,
                    merged: false,
                });
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
        // Key order with the last visible entity of each key, as a map insert would keep.
        models.reverse();
        models.sort_by_key(|model| model.key);
        models.dedup_by_key(|model| model.key);
        // Equal phase distances keep insertion order, so water enters in key order.
        let water = water_order.update(water.drain(..));
        // Displaced water can overlap itself even when flat, so such views sort all of it.
        let direct_order_independent = !view_displaces_water(enhanced.is_some());
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
                crate::transparent_phase::add(
                    phase,
                    Transparent3d {
                        sorting_info:
                            bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                        entity: (entity, main),
                        pipeline: pipeline_id,
                        draw_function: record_draw,
                        distance: transparent_liquid_phase_distance(&rangefinder, water_key),
                        batch_range: 0..1,
                        extra_index: PhaseItemExtraIndex::None,
                        indexed: true,
                    },
                );
                continue;
            };
            // Native deferred water uses layer 2, not ordinary blend layer 3.
            if enhanced.is_none()
                && let Ok(model_index) = models.binary_search_by_key(&group.key, |model| model.key)
                && let Some(&QueuedTransparentModel { entity, main, .. }) = models.get(model_index)
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
                    pipeline_id,
                )
            {
                models[model_index].merged = true;
                crate::transparent_phase::add(
                    phase,
                    Transparent3d {
                        sorting_info:
                            bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                        entity: (entity, main),
                        pipeline: pipeline_id,
                        draw_function: mixed_draw,
                        distance: transparent_model_phase_distance(&rangefinder, group.key),
                        batch_range: 0..1,
                        extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                            range: index..index + 1,
                            batch_set_index: None,
                        },
                        indexed: true,
                    },
                );
                continue;
            }
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (view_entity, *main_entity),
                    pipeline: pipeline_id,
                    draw_function: direct_draw,
                    distance: transparent_liquid_phase_distance(&rangefinder, group.key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                        range: group.ref_range.clone(),
                        batch_set_index: None,
                    },
                    indexed: true,
                },
            );
        }
        for &QueuedTransparentModel {
            key: model_key,
            entity,
            main,
            ..
        } in models.iter().filter(|model| !model.merged)
        {
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (entity, main),
                    pipeline: pipeline_id,
                    draw_function: transparent_model_draw,
                    distance: transparent_model_phase_distance(&rangefinder, model_key),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: true,
                },
            );
        }
    }
    mixed.finish_frame();
    unsorted.finish_frame();
}

/// A visible sub-chunk with transparent models, queued unless its group merged it with water.
#[derive(Clone, Copy)]
pub(in crate::chunk) struct QueuedTransparentModel {
    key: SubChunkKey,
    entity: Entity,
    main: MainEntity,
    /// Drawn inside its water group's mixed order instead of on its own.
    merged: bool,
}

/// Per-view lists the transparent queue refills each frame without reallocating.
#[derive(Default)]
pub(in crate::chunk) struct TransparentQueueScratch {
    /// Sorted by key with one entry per key once a view's visible set is gathered.
    models: Vec<QueuedTransparentModel>,
    water: Vec<VisibleWater>,
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
