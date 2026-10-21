//! Native scene layers render after pack colour and depth have been presented.

use bevy::{
    core_pipeline::{Core3dSystems, tonemapping::tonemapping},
    prelude::*,
    render::{
        renderer::{RenderContext, ViewQuery},
        view::ViewTarget,
    },
};

/// Places native geometry and private-depth hands after the pack and before labels and HUD.
pub(super) fn install(schedule: &mut bevy::ecs::schedule::Schedule) {
    schedule.add_systems(
        (
            begin,
            crate::scene_target::nodes::scene_opaque::<true>,
            crate::chunk::transparent::gamma_pass::gamma_transparent::<false, true>,
            crate::motion_blur::graph::motion_blur::<true>,
            crate::depth_smaa::node::depth_smaa::<true>,
            crate::ui_render::overlay::world::ui_world::<true>,
            crate::viewmodel_render::node::hand_view::<true>,
            crate::hand_rig_render::node::hand_rig::<true>,
            finish,
        )
            .chain()
            .after(super::plugin::ShaderpackPass)
            .before(tonemapping)
            .in_set(Core3dSystems::PostProcess),
    );
    schedule.add_systems(
        screen_overlay
            .after(Core3dSystems::PostProcess)
            .after(crate::nametag_render::overlay::NametagOverlay)
            .before(crate::ui_render::UiOverlayLabel)
            .before(bevy::core_pipeline::upscaling::upscaling),
    );
}

type NativeView = (
    &'static ViewTarget,
    &'static crate::scene_target::SceneTarget,
);

/// Seeds the native attachment with completed pack colour while retaining restored world depth.
fn begin(world: &World, query: ViewQuery<NativeView>, mut context: RenderContext) {
    if !super::native_pass_enabled(world, query.entity(), true) {
        return;
    }
    let (target, scene) = query.into_inner();
    context.command_encoder().copy_texture_to_texture(
        target.main_texture().as_image_copy(),
        scene.texture.as_image_copy(),
        scene.texture.size(),
    );
    scene.resume();
}

/// Publishes native composition before tonemapping and the independent label overlay.
fn finish(world: &World, query: ViewQuery<NativeView>, mut context: RenderContext) {
    if !super::native_pass_enabled(world, query.entity(), true) {
        return;
    }
    let (target, scene) = query.into_inner();
    scene.finish(&mut context, world, target);
}

/// Draws screen effects after native hands when no HUD pass owns that boundary.
fn screen_overlay(
    world: &World,
    query: ViewQuery<
        (
            &'static ViewTarget,
            &'static bevy::render::camera::ExtractedCamera,
            Option<&'static bevy::camera::MainPassResolutionOverride>,
        ),
        With<render_scene::shaderpack::ShaderPackRendering>,
    >,
    mut context: RenderContext,
) {
    let view = query.entity();
    if world.contains_resource::<crate::ui_render::UiGpu>() {
        return;
    }
    let (target, camera, resolution) = query.into_inner();
    crate::screen_overlay_render::draw_before_hud(
        view,
        target,
        camera,
        resolution,
        &mut context,
        world,
    );
}

#[cfg(test)]
#[path = "native_passes_tests.rs"]
mod tests;
