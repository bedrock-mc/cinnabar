//! Draws far-terrain tiles beyond near terrain, after opaque terrain so it only fills what
//! near terrain left uncovered.

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{Core3d, Core3dSystems},
    prelude::{App, Handle, IntoScheduleConfigs, Plugin, Resource, Shader, SystemSet, World},
    render::{
        Render, RenderApp, RenderSystems, extract_resource::ExtractResourcePlugin,
        renderer::RenderDevice,
    },
};
use render_scene::FarTerrainScene;

mod gpu;
mod node;
mod ranges;

pub(crate) use gpu::FarTerrainGpu;

const SHADER: Handle<Shader> = uuid_handle!("3c1f8e52-9a07-4d6b-b8e4-5f2a71c0d913");

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct FarTerrainLabel;

#[derive(Debug, Clone, Copy, Default)]
pub struct FarTerrainRenderPlugin;

impl Plugin for FarTerrainRenderPlugin {
    fn build(&self, app: &mut App) {
        crate::pipeline_warmup::register::<FarTerrainGpu>(app);
        load_internal_asset!(app, SHADER, "far_terrain.wesl", |source, path| {
            crate::shader_safety::from_wesl(crate::material_shader::source(source), path)
        });
        app.init_resource::<FarTerrainScene>()
            .add_plugins(ExtractResourcePlugin::<FarTerrainScene>::default());
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let device = render_app.world().resource::<RenderDevice>().clone();
        render_app
            .insert_resource(FarTerrainGpu::new(&device))
            .add_systems(
                Render,
                (
                    gpu::prepare_far_terrain.in_set(
                        crate::chunk::plugin::publication_schedule::ChunkPublicationStage::FarTiles,
                    ),
                    gpu::prepare_far_terrain_views
                        .in_set(RenderSystems::PrepareBindGroups)
                        .after(gpu::prepare_far_terrain),
                ),
            );
        install_graph(render_app.world_mut());
    }
}

/// Orders far terrain after opaque terrain and its late cull, before the enhanced-rendering
/// scene snapshot that reflections sample and before translucent geometry.
fn install_graph(world: &mut World) {
    if world.contains_resource::<FarTerrainPassInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            let pass = crate::gpu_timing::profiled(node::far_terrain, None, "FarTerrainLabel")
                .in_set(FarTerrainLabel)
                .after(crate::scene_target::ScenePass::Opaque)
                .after(crate::chunk::GpuCullLateLabel)
                .before(crate::scene_target::ScenePass::Transparent)
                .in_set(Core3dSystems::MainPass);
            #[cfg(feature = "enhanced")]
            let pass = pass.before(crate::enhanced::snapshot::EnhancedSnapshotLabel);
            schedule.add_systems(pass);
        })
        .is_ok();
    if installed {
        world.insert_resource(FarTerrainPassInstalled);
    }
}

#[derive(Resource)]
struct FarTerrainPassInstalled;

/// Clears per-view far draws when the shared terrain publication failed.
pub(crate) fn suppress_publication(world: &mut World) {
    for mut view in world.query::<&mut gpu::FarTerrainView>().iter_mut(world) {
        view.release();
    }
}
