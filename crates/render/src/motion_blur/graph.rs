use super::{CameraMotionBlur, prepare::BlurView};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
    render::{
        render_resource::*,
        renderer::RenderContext,
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct MotionBlurLabel;
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(super) struct SharpNametagsLabel;

/// Removes exposure passes when no view requests them; unchanged settings do no work.
pub(crate) fn sync_graph(
    world: &mut World,
    mut views: Local<Option<QueryState<&CameraMotionBlur>>>,
) {
    let enabled = views
        .get_or_insert_with(|| world.query())
        .iter(world)
        .next()
        .is_some();
    configure_graph(world, enabled);
}

/// Adds or removes exposure systems only when the requested state changes.
pub(super) fn configure_graph(world: &mut World, enabled: bool) {
    if world
        .get_resource::<BlurPassesInstalled>()
        .is_some_and(|state| state.0 == enabled)
    {
        return;
    }
    let configured = world
        .try_schedule_scope(Core3d, |world, schedule| {
            use bevy::ecs::schedule::ScheduleCleanupPolicy;
            let _ = schedule.remove_systems_in_set(
                MotionBlurLabel,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            let _ = schedule.remove_systems_in_set(
                SharpNametagsLabel,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            if enabled {
                schedule.add_systems(
                    (
                        crate::gpu_timing::profiled(motion_blur, None, "MotionBlurLabel")
                            .in_set(MotionBlurLabel)
                            .after(crate::depth_smaa::DepthSmaaLabel),
                        crate::gpu_timing::profiled(
                            crate::chunk::transparent::gamma_pass::gamma_transparent::<true>,
                            None,
                            "SharpNametagsLabel",
                        )
                        .in_set(SharpNametagsLabel)
                        .after(MotionBlurLabel),
                    )
                        .after(crate::scene_target::ScenePass::Transparent)
                        .before(crate::ui_render::UiWorldLabel)
                        .in_set(Core3dSystems::MainPass),
                );
            }
        })
        .is_ok();
    if configured {
        world.insert_resource(BlurPassesInstalled(enabled));
    }
}

#[derive(Resource)]
struct BlurPassesInstalled(bool);

type MotionBlurQuery = (
    &'static CameraMotionBlur,
    &'static BlurView,
    &'static ViewTarget,
    &'static crate::scene_target::SceneTarget,
    &'static ViewDepthStencilTexture,
);

/// Applies camera exposure to scene colour before sharp text and hands.
pub(super) fn motion_blur(
    world: &World,
    query: bevy::render::renderer::ViewQuery<MotionBlurQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (_, state, target, scene, depth) = query.into_inner();
    let context = &mut context;
    if !state.active {
        return Ok(());
    }
    let cache = world.resource::<PipelineCache>();
    let Some(pipeline) = cache.get_render_pipeline(state.pipeline) else {
        return Ok(());
    };
    let Some(binding) = state.binding(
        target.main_texture_view().id(),
        crate::scene_sampling::view_depth(depth).id(),
    ) else {
        return Ok(());
    };
    // Resolve only world colour, then write exposure back before sharp projected UI and hands.
    if scene.texture.sample_count() == 1 {
        context.command_encoder().copy_texture_to_texture(
            scene.texture.as_image_copy(),
            target.main_texture().as_image_copy(),
            scene.texture.size(),
        );
    } else {
        let attachments = [Some(
            scene.resolve_attachment(target.main_texture_view(), StoreOp::Store),
        )];
        context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("camera exposure scene resolve"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuPost,
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
    }
    let attachments = [Some(scene.color_attachment(target, false))];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("camera motion blur"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            crate::RuntimeStage::GpuPost,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let viewport = state.viewport();
    let Some(rect) = crate::render_bounds::scissor(
        render_model::UiScissor::new(viewport.x, viewport.y, viewport.z, viewport.w),
        crate::render_bounds::extent(scene.color_view(false)),
    ) else {
        return Ok(());
    };
    pass.set_viewport(
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
        0.0,
        1.0,
    );
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
    Ok(())
}
