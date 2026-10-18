//! Ordinary world transparency blends encoded colour while retaining sorted draw order.

#[cfg(test)]
mod encoding_tests;
#[cfg(test)]
mod tests;

use crate::chunk::*;
use crate::scene_target::SceneTarget;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::Core3d,
    render::{
        render_phase::DrawFunctionId,
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::ViewDepthStencilTexture,
    },
};

/// Encoded blending applies to ordinary LDR views at every sample count.
pub(crate) fn admitted(hdr: bool, _msaa: Msaa, enhanced: bool) -> bool {
    !(hdr || render_model::enhanced_rendering_enabled() && enhanced)
}

/// Shares the main colour attachment with opaque geometry and later hand passes.
pub(in crate::chunk) fn install(app: &mut App) {
    crate::scene_target::install(app);
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
}

/// Preserves graph dependencies while selecting the blend colour space per sorted range.
pub(in crate::chunk) fn install_graph(world: &mut World) {
    crate::scene_target::install_graph(world);
    if world.contains_resource::<TransparentInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |world, schedule| {
            schedule
                .remove_systems_in_set(
                    bevy::core_pipeline::core_3d::main_transparent_pass_3d,
                    world,
                    bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("replace the stock transparent pass");
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    gamma_transparent::<false>,
                    Some(render_scene::RuntimeStage::GpuTransparent),
                    "MainTransparentPass",
                )
                .in_set(crate::scene_target::ScenePass::Transparent),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(TransparentInstalled);
    }
}

#[derive(Resource)]
struct TransparentInstalled;

type GammaView = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    &'static ViewDepthStencilTexture,
    Option<&'static MainPassResolutionOverride>,
    &'static Msaa,
    Option<&'static render_scene::EnhancedRendering>,
    &'static SceneTarget,
    Option<&'static bevy::anti_alias::smaa::Smaa>,
);

type GammaTransparentQuery = GammaView;

/// Keeps sorted transparency in its native blend colour space.
pub(crate) fn gamma_transparent<const NAMETAGS_ONLY: bool>(
    world: &World,
    query: bevy::render::renderer::ViewQuery<GammaTransparentQuery>,
    mut render_context: RenderContext,
) -> bevy::ecs::error::Result {
    let view_entity = query.entity();
    let (camera, view, target, depth, resolution, msaa, enhanced, scene, smaa) = query.into_inner();
    let render_context = &mut render_context;
    let blur = crate::motion_blur::applies(world, view_entity);
    if NAMETAGS_ONLY && !blur {
        return Ok(());
    }
    let Some(phases) = world.get_resource::<ViewSortedRenderPhases<Transparent3d>>() else {
        return Ok(());
    };
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return Ok(());
    };
    if phase.items.is_empty() {
        return Ok(());
    }
    let gamma = admitted(camera.hdr, *msaa, enhanced.is_some());
    let draws = gamma.then(|| native_draws(world));
    let filtered = blur || smaa.is_some();
    let nametag = filtered
        .then(|| crate::nametag_render::draw_function(world))
        .flatten();
    let nametags_only = NAMETAGS_ONLY;
    // Each drawn range of equal colour space, in sorted order.
    let ranges = move || {
        contiguous_ranges(phase.items.values(), move |item| {
            (
                draws
                    .as_ref()
                    .is_some_and(|draws| draws.contains(&Some(item.draw_function()))),
                crate::nametag_render::deferred_by_world_filter(
                    filtered,
                    nametag,
                    item.draw_function(),
                ),
            )
        })
        .filter(move |(_, (_, deferred))| *deferred == nametags_only)
        .map(|(range, (gamma, _))| (range, gamma))
    };
    #[cfg(feature = "tracy")]
    if !nametags_only && let Some(client) = tracy_client::Client::running() {
        use tracy_client::plot_name;
        let mut counts = [0_usize; 3];
        let mut previous = None;
        for (range, gamma) in ranges() {
            counts[usize::from(gamma)] += range.len();
            counts[2] += usize::from(previous != Some(gamma));
            previous = Some(gamma);
        }
        let switches = phase
            .items
            .values()
            .zip(phase.items.values().skip(1))
            .filter(|(left, right)| left.pipeline != right.pipeline)
            .count();
        client.plot(plot_name!("transparent linear items"), counts[0] as f64);
        client.plot(plot_name!("transparent gamma items"), counts[1] as f64);
        client.plot(plot_name!("transparent passes"), counts[2] as f64);
        client.plot(plot_name!("transparent pipeline switches"), switches as f64);
    }
    // `None` keeps the full target; `Some(None)` is a camera viewport outside the attachment.
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution).map(
        |viewport| {
            crate::render_bounds::viewport(
                &viewport,
                crate::render_bounds::extent(scene.color_view(false)),
            )
        },
    );
    // Phase encoding holds the shared draw-functions lock, so worker ranges cannot draw
    // concurrently. Keep their ordered colour-space passes in one encoder without a worker wait.
    let mut ranges = ranges().peekable();
    while let Some(gamma) = ranges.peek().map(|(_, gamma)| *gamma) {
        let attachments = [Some(scene.color_attachment(target, gamma))];
        let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("sorted ordinary transparent colour-space range"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                render_scene::RuntimeStage::GpuTransparent,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let draw = match &viewport {
            Some(Some(viewport)) => {
                pass.set_camera_viewport(viewport);
                true
            }
            Some(None) => false,
            None => true,
        };
        // Even an empty viewport begins one pass so the target's first use still clears it.
        if !draw {
            break;
        }
        // Deferred names leave gaps in the phase, but do not change this pass's attachments.
        while let Some((range, _)) = ranges.next_if(|(_, next_gamma)| *next_gamma == gamma) {
            if let Err(error) = phase.render_range(&mut pass, world, view_entity, range) {
                bevy::log::error!("Error rendering sorted transparency: {error:?}");
            }
        }
    }
    Ok(())
}

/// Native transparent families output encoded colour and blend through the compatible UNORM view.
fn native_draws(world: &World) -> [Option<DrawFunctionId>; 9] {
    use crate::chunk::transparent::mixed::DrawMixedTerrainCommands;
    let nametags = crate::nametag_render::draw_function(world);
    let primitives = crate::primitive_shapes::draw_function(world);
    let clouds = world
        .get_resource::<render_scene::AtmosphereFrame>()
        .filter(|frame| frame.java_world_visuals())
        .and_then(|_| crate::cloud_render::draw_function(world));
    let draws = world.resource::<DrawFunctions<Transparent3d>>().read();
    [
        Some(draws.id::<DrawTransparentLiquidCommands>()),
        Some(draws.id::<DrawTransparentLiquidDirectCommands>()),
        Some(draws.id::<DrawTransparentLiquidIndirectCommands>()),
        Some(draws.id::<DrawTransparentModelCommands>()),
        Some(draws.id::<DrawMixedTerrainCommands>()),
        draws.get_id::<crate::actor_render::phase::DrawTransparentActorCommands>(),
        nametags,
        primitives,
        clouds,
    ]
}

/// Keeps sorted items contiguous without auxiliary per-item storage or crossing colour spaces.
pub(crate) fn contiguous_ranges<'a, T: 'a, M: Copy + PartialEq + 'a>(
    mut items: impl Iterator<Item = &'a T> + 'a,
    mut classify: impl FnMut(&T) -> M + 'a,
) -> impl Iterator<Item = (Range<usize>, M)> + 'a {
    let mut pending = items.next();
    let mut start = 0;
    std::iter::from_fn(move || {
        let mode = classify(pending.take()?);
        let mut end = start + 1;
        for item in items.by_ref() {
            if classify(item) != mode {
                pending = Some(item);
                break;
            }
            end += 1;
        }
        let range = start..end;
        start = end;
        Some((range, mode))
    })
}
