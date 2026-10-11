use super::*;

/// Camera identity and settings used to select actor render phases.
type ActorQueueView = (
    Entity,
    &'static MainEntity,
    &'static ExtractedView,
    &'static bevy::render::camera::ExtractedCamera,
    &'static Msaa,
    Option<&'static crate::EnhancedRendering>,
);

#[derive(SystemParam)]
pub(super) struct QueueActorParams<'w, 's> {
    pipeline: Res<'w, ActorPipeline>,
    gpu: Res<'w, ActorGpu>,
    phases: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<'w, DrawFunctions<Opaque3d>>,
    transparent_phases: ResMut<'w, ViewSortedRenderPhases<Transparent3d>>,
    transparent_functions: Res<'w, DrawFunctions<Transparent3d>>,
    views: Query<'w, 's, ActorQueueView>,
    draw_tracker: Res<'w, ActorDrawTracker>,
    witness: Res<'w, ActorRuntimeWitness>,
}

pub(super) fn queue_actors(
    mut params: QueueActorParams<'_, '_>,

    mut next_draw_generation: Local<u64>,
) {
    params.draw_tracker.clear();
    params
        .gpu
        .executed_instances
        .store(0, std::sync::atomic::Ordering::Relaxed);
    let view_count = params.views.iter().count();
    if params.gpu.instance_count == 0 {
        params.witness.observe_queue(ActorQueueWitness {
            prepared_instances: params.gpu.instance_count,
            bind_group: params.gpu.bind_group.is_some(),
            view_count,
            queued: false,
        });
        return;
    }
    let draw_function = params.draw_functions.read().id::<DrawActorCommands>();
    let transparent_draw = params
        .transparent_functions
        .read()
        .id::<DrawTransparentActorCommands>();
    let mut queued = false;
    let mut intended_view = None;
    for (view_entity, main_entity, view, extracted_camera, msaa, enhanced) in &params.views {
        let Some(phase) = params.phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(pipeline_id) = params.pipeline.draw_variant(
            *msaa,
            extracted_camera.hdr,
            enhanced.is_some(),
            assets::EntityRenderMaterial::Default as u32,
        ) else {
            continue;
        };

        let mut view_queued = false;
        if params.gpu.spans.iter().any(|span| !sorted(span.material)) {
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Shader>::invalid().untyped(),
                },
                (view_entity, *main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );
            view_queued = true;
        }
        if let Some(transparent) = params
            .transparent_phases
            .get_mut(&view.retained_view_entity)
        {
            let rangefinder = view.rangefinder3d();
            for (index, range) in params.gpu.sorted.ranges.iter().enumerate() {
                let span = &params.gpu.spans[params.gpu.sorted.indices[range.start]];
                let Some(instance) = params.gpu.instances.get(span.first as usize) else {
                    continue;
                };
                let Some(pipeline) = params.pipeline.draw_variant(
                    *msaa,
                    extracted_camera.hdr,
                    enhanced.is_some(),
                    span.material,
                ) else {
                    continue;
                };
                let position = Vec3::new(
                    instance.world_from_actor[0][3],
                    instance.world_from_actor[1][3],
                    instance.world_from_actor[2][3],
                );
                crate::transparent_phase::add(
                    transparent,
                    Transparent3d {
                        sorting_info:
                            bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                        entity: (view_entity, *main_entity),
                        pipeline,
                        draw_function: transparent_draw,
                        distance: rangefinder.distance(&position),
                        batch_range: 0..1,
                        extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                            range: index as u32..index as u32 + 1,
                            batch_set_index: None,
                        },
                        indexed: false,
                    },
                );
                view_queued = true;
            }
        }
        if !view_queued {
            continue;
        }
        queued = true;
        intended_view = Some(intended_view.map_or(view_entity.to_bits(), |current: u64| {
            current.min(view_entity.to_bits())
        }));
    }
    if queued {
        let Some(draw_generation) = next_draw_generation.checked_add(1) else {
            return;
        };
        *next_draw_generation = draw_generation;
        let _ = params.draw_tracker.begin(
            ActorDrawFrame {
                artwork_identity: params.gpu.artwork_identity,
                skin_revision: params.gpu.skin_revision,
                geometry_revision: params.gpu.geometry_revision,
                frame_generation: params.gpu.frame_generation,
                draw_generation,
                manifest: std::sync::Arc::clone(&params.gpu.manifest),
            },
            intended_view.expect("queued view exists"),
            &params.gpu.spans,
        );
    }
    params.witness.observe_queue(ActorQueueWitness {
        prepared_instances: params.gpu.instance_count,
        bind_group: params.gpu.bind_group.is_some(),
        view_count,
        queued,
    });
}

pub(super) type DrawActorCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuActors as usize },
    (
        SetItemPipeline,
        crate::lighting::SetWorldLightmap,
        DrawActors<false>,
    ),
>;

pub(crate) type DrawTransparentActorCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuActors as usize },
    (
        SetItemPipeline,
        crate::lighting::SetWorldLightmap,
        DrawActors<true>,
    ),
>;

/// Draws blending, always-depth spans and their companion color layers after terrain.
pub(super) fn sorted(material: u32) -> bool {
    crate::actor::material::state(material).is_some_and(|state| state.blend || state.depth_always)
        || material & crate::actor::material::LATE_DISSOLVE_COLOR != 0
}

pub(crate) struct DrawActors<const SORTED: bool>;

impl<P: PhaseItem, const SORTED: bool> RenderCommand<P> for DrawActors<SORTED> {
    type Param = (
        SRes<ActorGpu>,
        SRes<ActorDrawTracker>,
        SRes<ActorRuntimeWitness>,
        SRes<ActorPipeline>,
        SRes<PipelineCache>,
    );
    type ViewQuery = (
        Entity,
        Read<ViewUniformOffset>,
        Read<Msaa>,
        Read<ExtractedView>,
        Option<Read<crate::EnhancedRendering>>,
        Read<bevy::render::camera::ExtractedCamera>,
    );
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        params: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, tracker, witness, pipeline, cache) = params;
        let gpu = gpu.into_inner();
        let tracker = tracker.into_inner();
        let pipeline = pipeline.into_inner();
        let cache = cache.into_inner();
        let mut executed_instances = 0;
        let mut bound_page = None;
        let span_indices = if SORTED {
            let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = item.extra_index()
            else {
                return RenderCommandResult::Skip;
            };
            if range.is_empty() {
                return RenderCommandResult::Skip;
            }
            let (Some(first), Some(last)) = (
                gpu.sorted.ranges.get(range.start as usize),
                gpu.sorted.ranges.get(range.end as usize - 1),
            ) else {
                return RenderCommandResult::Skip;
            };
            let Some(indices) = gpu.sorted.indices.get(first.start..last.end) else {
                return RenderCommandResult::Skip;
            };
            Some(indices)
        } else {
            None
        };
        for ordinal in 0..span_indices.map_or(gpu.spans.len(), |indices| indices.len()) {
            let index = span_indices.map_or(ordinal, |indices| indices[ordinal]);
            let span = &gpu.spans[index];
            if sorted(span.material) != SORTED {
                continue;
            }
            if span.page != 0 && !gpu.artwork_current {
                continue;
            }
            let Some(id) =
                pipeline.draw_variant(*view.2, view.5.hdr, view.4.is_some(), span.material)
            else {
                continue;
            };
            let Some(variant) = cache.get_render_pipeline(id) else {
                continue;
            };
            pass.set_render_pipeline(variant);
            if bound_page != Some(span.page) {
                let bind_group = if span.page == 0 {
                    gpu.bind_group.as_ref()
                } else {
                    gpu.artwork
                        .pages
                        .get(usize::from(span.page) - 1)
                        .and_then(|page| page.bind_group.as_ref())
                };
                let Some(bind_group) = bind_group else {
                    continue;
                };
                pass.set_bind_group(0, bind_group, &[view.1.offset]);
                bound_page = Some(span.page);
            }
            pass.draw(0..span.vertex_count, span.first..span.first + span.count);
            tracker.record_draw(view.0.to_bits(), *span);
            executed_instances += span.count;
        }
        witness.into_inner().observe_draw(ActorDrawWitness {
            executed: executed_instances != 0,
            instances: gpu
                .executed_instances
                .fetch_add(executed_instances, std::sync::atomic::Ordering::Relaxed)
                + executed_instances,
            maximum_vertices: gpu.maximum_vertex_count,
        });
        RenderCommandResult::Success
    }
}
