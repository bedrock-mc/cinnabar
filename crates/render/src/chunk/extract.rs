#[cfg(feature = "enhanced")]
use super::resident_coverage::ChunkResidentCoverage;
use bevy::render::{
    Extract, ExtractSchedule, sync_component::SyncComponentPlugin, sync_world::RenderEntity,
};
use render_scene::ChunkRenderInstance;

use super::*;
use render_scene::{RuntimeStage, RuntimeStageProfiler};

pub(in crate::chunk) fn install_chunk_extraction(app: &mut App) {
    #[cfg(feature = "enhanced")]
    app.add_plugins(ExtractResourcePlugin::<ChunkResidentCoverage>::default());
    app.add_plugins(SyncComponentPlugin::<ChunkRenderInstance>::default())
        .add_plugins((
            ExtractResourcePlugin::<ChunkTextureAssets>::default(),
            ExtractResourcePlugin::<ChunkTextureReload>::default(),
            ExtractResourcePlugin::<ChunkAnimationClock>::default(),
            ExtractResourcePlugin::<ChunkBiomeTints>::default(),
            ExtractResourcePlugin::<ChunkUploadBudget>::default(),
            ExtractResourcePlugin::<render_scene::chunk::capacity::ChunkArenaCapacityHint>::default(
            ),
            ExtractResourcePlugin::<ChunkGpuRemovalQueue>::default(),
            ExtractResourcePlugin::<
                crate::dropped_item_render::terrain_items::ImmediateTerrainMeshPublications,
            >::default(),
            ExtractResourcePlugin::<TransparentWitnessRequest>::default(),
            ExtractResourcePlugin::<ModelWitnessRequest>::default(),
            ExtractResourcePlugin::<VisibilityDiagnosticsInput>::default(),
        ));
    app.sub_app_mut(RenderApp)
        .add_systems(ExtractSchedule, extract_chunk_render_instances);
}

fn extract_chunk_render_instances(
    mut commands: Commands,
    mut previous_len: Local<usize>,
    query: Extract<Query<(RenderEntity, &ChunkRenderInstance), Changed<ChunkRenderInstance>>>,
    profiler: Extract<Option<Res<RuntimeStageProfiler>>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::ChunkExtraction));
    let mut values = Vec::with_capacity(*previous_len);
    for (render_entity, instance) in &query {
        #[cfg(feature = "enhanced")]
        if !extract_enhanced_chunk(&mut commands, render_entity, instance) {
            continue;
        }
        values.push((render_entity, instance.clone()));
    }
    *previous_len = values.len();
    commands.try_insert_batch(values);
}

/// Publishes lights separately and retires GPU geometry for CPU-only instances.
#[cfg(feature = "enhanced")]
pub(in crate::chunk) fn extract_enhanced_chunk(
    commands: &mut Commands,
    entity: Entity,
    instance: &ChunkRenderInstance,
) -> bool {
    use render_scene::chunk::instance::ChunkLightEmitters;
    if instance.light_emitters().is_empty() {
        commands.entity(entity).remove::<ChunkLightEmitters>();
    } else {
        commands.entity(entity).insert(ChunkLightEmitters {
            key: instance.key,
            emitters: Arc::clone(&instance.light_emitters),
        });
    }
    if instance.geometry_is_empty() {
        commands.entity(entity).remove::<ChunkRenderInstance>();
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use render_scene::RuntimeStageProfiler;
    /// Actual extraction and submission spans use the input frame currently being extracted.
    #[test]
    fn extraction_samples_use_the_current_input_identity() {
        use bevy::ecs::schedule::ScheduleLabel;
        use bevy::prelude::*;
        use bevy::render::{Render, RenderApp, RenderSystems, extract_plugin::ExtractPlugin};
        /// Starts a distinct input frame before extracting its render data.
        fn input_frame(profiler: Res<RuntimeStageProfiler>) {
            profiler.begin_frame(true, false);
            profiler.trace_frame(true, false, 0.0);
        }
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("identity.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        let mut app = App::new();
        app.insert_resource(profiler.clone())
            .add_systems(First, input_frame)
            .add_plugins(ExtractPlugin::<RenderApp>::new(
                |_, _| {},
                Render::base_schedule,
                Render.intern(),
                RenderSystems::ExtractCommands.intern(),
                RenderSystems::PostCleanup.intern(),
            ));
        let render = app.sub_app_mut(RenderApp);
        render.insert_resource(profiler.clone());
        render.update_schedule = Some(Render.intern());
        crate::runtime_profile_trace::install_surface_trace(render);
        render.add_systems(
            bevy::render::ExtractSchedule,
            super::extract_chunk_render_instances,
        );
        app.update();
        app.update();
        profiler.flush_trace();
        profiler.wait_for_trace_export();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for name in ["chunk_extraction", "render_submission", "render_frame"] {
            let ids: Vec<_> = saved["traceEvents"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["name"] == name)
                .map(|event| event["args"]["frame_id"].as_u64().unwrap())
                .collect();
            assert_eq!(ids, [1, 2], "{name}");
        }
    }
}
