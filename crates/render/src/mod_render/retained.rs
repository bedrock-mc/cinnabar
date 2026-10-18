//! GPU resources for personal-mod meshes; protocol debug shapes have separate ownership.
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, extract_resource::ExtractResourcePlugin,
        render_phase::AddRenderCommand,
    },
};
use render_scene::mod_scene::retained::DrawingFrame;
mod gpu;
mod pipeline;
#[cfg(test)]
mod tests;
const SHADER: Handle<Shader> = uuid_handle!("76e342a4-19e8-4592-813f-9b195ee8a15d");

/// Installs static geometry, per-view transforms and the shared unlit pipeline.
pub(super) fn install(app: &mut App) {
    crate::pipeline_warmup::register::<pipeline::Pipeline>(app);
    app.add_plugins(ExtractResourcePlugin::<DrawingFrame>::default());
    load_internal_asset!(
        app,
        SHADER,
        "retained/drawing.wesl",
        crate::shader_safety::from_wesl
    );
    crate::device_poll::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<gpu::Gpu>()
        .init_resource::<pipeline::Pipeline>()
        .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
    install_systems(app.sub_app_mut(RenderApp));
}

/// Registers drawing preparation and deferred resource release in the render schedule.
fn install_systems(app: &mut SubApp) {
    app.add_systems(
        Render,
        (
            gpu::queue
                .after(super::primitives::queue)
                .in_set(RenderSystems::Queue),
            pipeline::bindings.in_set(RenderSystems::PrepareBindGroups),
            gpu::retire.in_set(crate::device_poll::FrameSubmissions),
        ),
    );
}
