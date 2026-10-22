//! Replacing Bevy's post-processing group removes only the camera passes no client camera uses.
//!
//! Bevy leaves the order of passes without an edge between them undefined, and any added or
//! removed graph node may change it, so this compares the set of passes. Passes that share an
//! attachment carry explicit edges instead.

use super::StockSystemsPlugin;
use bevy::{
    asset::AssetPlugin,
    camera::CameraPlugin,
    core_pipeline::{Core3d, CorePipelinePlugin},
    image::ImagePlugin,
    mesh::MeshPlugin,
    post_process::PostProcessPlugin,
    prelude::*,
    render::{RenderApp, RenderStartup, render_resource::WgpuFeatures},
    window::WindowPlugin,
};
use std::any::TypeId;

/// The camera graph's systems after render startup, for the client's render plugins on a NOOP
/// device with `post_processing` in place of Bevy's group.
fn camera_passes(post_processing: impl Plugin) -> Vec<TypeId> {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((AssetPlugin::default(), TransformPlugin))
        .add_plugins(crate::chunk::noop_render_plugin(WgpuFeatures::empty()))
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ))
        .add_plugins(post_processing)
        .add_plugins((
            crate::ChunkRenderPlugin::default(),
            crate::AtmospherePlugin,
            crate::FarTerrainRenderPlugin,
            crate::EntityShadowRenderPlugin,
            crate::UiRenderPlugin,
            crate::DepthSmaaPlugin,
            crate::motion_blur::CameraMotionBlurPlugin,
        ));
    app.finish();
    app.cleanup();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(RenderStartup);
    world.schedule_scope(Core3d, |world, schedule| {
        schedule.initialize(world).unwrap();
        schedule
            .systems()
            .unwrap()
            .map(|(_, system)| system.system_type())
            .collect()
    })
}

#[test]
fn replacing_post_processing_removes_only_its_unused_camera_passes() {
    let mut removed = camera_passes(PostProcessPlugin);
    for pass in camera_passes(StockSystemsPlugin) {
        let index = removed
            .iter()
            .position(|&full| full == pass)
            .expect("the replacement adds no camera pass");
        removed.swap_remove(index);
    }
    assert_eq!(
        removed.len(),
        3,
        "only depth of field, the effect stack and Bevy's motion blur leave the camera graph"
    );
}
