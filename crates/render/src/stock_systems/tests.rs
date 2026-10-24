//! Removing unfed stock systems leaves every camera pass in the graph.
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
    prelude::*,
    render::{RenderApp, RenderStartup, render_resource::WgpuFeatures},
    window::WindowPlugin,
};
use std::any::TypeId;

/// The camera graph's systems after render startup, sorted, for the client's render plugins on
/// a NOOP device, optionally with the stock-system removal.
fn camera_passes(remove_stock_systems: bool) -> Vec<TypeId> {
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
            crate::ClientPostProcessPlugin,
        ));
    if remove_stock_systems {
        app.add_plugins(StockSystemsPlugin);
    }
    app.add_plugins((
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
    if remove_stock_systems {
        // The removal panics if Bevy no longer schedules a listed main-world system.
        super::remove_unused_main_systems(app.world_mut());
    }
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(RenderStartup);
    let mut passes: Vec<_> = world.schedule_scope(Core3d, |world, schedule| {
        schedule.initialize(world).unwrap();
        schedule
            .systems()
            .unwrap()
            .map(|(_, system)| system.system_type())
            .collect()
    });
    passes.sort_unstable();
    passes
}

#[test]
fn removing_stock_systems_keeps_every_camera_pass() {
    assert_eq!(camera_passes(true), camera_passes(false));
}
