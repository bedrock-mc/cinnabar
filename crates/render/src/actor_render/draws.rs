//! Shares actor geometry and skin coverage while keeping shadow-only bodies out of camera draws.

use super::*;
use crate::actor::gpu::ActorDrawSpan;

pub(super) fn main_span(mut span: ActorDrawSpan, main_count: u32) -> Option<ActorDrawSpan> {
    if span.first >= main_count {
        return None;
    }
    span.count = span.count.min(main_count - span.first);
    Some(span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_spans_exclude_shadow_tail_without_changing_geometry_coverage() {
        let span = ActorDrawSpan {
            page: 2,
            first: 3,
            count: 5,
            vertex_count: 72,
        };
        let clipped = main_span(span, 5).unwrap();
        assert_eq!(clipped, ActorDrawSpan { count: 2, ..span });
        assert_eq!(main_span(span, 8), Some(span));
        assert!(main_span(ActorDrawSpan { first: 5, ..span }, 5).is_none());
        assert!(main_span(span, 0).is_none());
    }
}

fn prepared_spans(
    gpu: &ActorGpu,
    shadows: bool,
) -> impl Iterator<Item = (ActorDrawSpan, &BindGroup)> {
    gpu.spans.iter().copied().filter_map(move |span| {
        let span = if shadows {
            span
        } else {
            main_span(span, gpu.main_manifest.len() as u32)?
        };
        if span.page != 0 && !gpu.artwork_current {
            return None;
        }
        let bind_group = if span.page == 0 {
            gpu.bind_group.as_ref()
        } else {
            gpu.artwork
                .pages
                .get(usize::from(span.page) - 1)
                .and_then(|page| page.bind_group.as_ref())
        }?;
        Some((span, bind_group))
    })
}

fn draw_prepared_actors<'w>(
    gpu: &'w ActorGpu,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    shadows: bool,
    mut record: impl FnMut(ActorDrawSpan),
) -> u32 {
    let mut executed_instances = 0;
    let mut bound_page = None;
    for (span, bind_group) in prepared_spans(gpu, shadows) {
        if bound_page != Some(span.page) {
            pass.set_bind_group(0, bind_group, &[view_offset]);
            bound_page = Some(span.page);
        }
        pass.draw(0..span.vertex_count, span.first..span.first + span.count);
        record(span);
        executed_instances += span.count;
    }
    executed_instances
}

/// Reuses the prepared animated bone, geometry, and artwork bindings without recording presentation.
#[cfg(feature = "enhanced")]
pub(crate) fn draw_shadow_actors<'w>(
    world: &'w World,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    pipeline: &'w RenderPipeline,
) {
    let Some(gpu) = world.get_resource::<ActorGpu>() else {
        return;
    };
    if gpu.instance_count == 0 {
        return;
    }
    let Some(lightmap) = world.get_resource::<crate::lighting::LightmapGpu>() else {
        return;
    };
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    draw_prepared_actors(gpu, view_offset, pass, true, |_| {});
}

#[cfg(feature = "enhanced")]
pub(crate) fn draw_depth_actors<'w>(
    world: &'w World,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    pipeline: &'w RenderPipeline,
) {
    let Some(gpu) = world.get_resource::<ActorGpu>() else {
        return;
    };
    if gpu.instance_count == 0 {
        return;
    }
    let Some(lightmap) = world.get_resource::<crate::lighting::LightmapGpu>() else {
        return;
    };
    let Some(motion) = world.get_resource::<super::motion::ActorMotionGpu>() else {
        return;
    };
    let Some(motion_group) = motion.bind_group.as_ref() else {
        return;
    };
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    pass.set_bind_group(3, motion_group, &[]);
    draw_prepared_actors(gpu, view_offset, pass, false, |span| {
        super::motion::record_coverage(world, span.first, span.count);
    });
}

pub(super) type DrawActorCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    crate::enhanced::SetEnhancedViewBindGroup<2>,
    DrawActors,
);

pub(super) struct DrawActors;

impl<P: PhaseItem> RenderCommand<P> for DrawActors {
    type Param = (
        SRes<ActorGpu>,
        SRes<ActorDrawTracker>,
        SRes<ActorRuntimeWitness>,
    );
    type ViewQuery = (Entity, Read<ViewUniformOffset>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        params: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, tracker, witness) = params;
        let gpu = gpu.into_inner();
        let tracker = tracker.into_inner();
        let executed_instances = draw_prepared_actors(gpu, view.1.offset, pass, false, |span| {
            tracker.record_draw(view.0.to_bits(), span);
        });
        witness.into_inner().observe_draw(ActorDrawWitness {
            executed: executed_instances != 0,
            instances: executed_instances,
            maximum_vertices: gpu.maximum_vertex_count,
        });
        RenderCommandResult::Success
    }
}
