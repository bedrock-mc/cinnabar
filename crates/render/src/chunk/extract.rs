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
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::ChunkExtraction));
    let mut values = Vec::with_capacity(*previous_len);
    for (render_entity, instance) in &query {
        #[cfg(feature = "enhanced")]
        if !extract_enhanced_chunk(&mut commands, render_entity.id(), instance) {
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
