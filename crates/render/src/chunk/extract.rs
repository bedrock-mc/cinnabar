use bevy::render::{
    Extract, ExtractSchedule, sync_component::SyncComponentPlugin, sync_world::RenderEntity,
};

use super::*;
use crate::{RuntimeStage, RuntimeStageProfiler};

pub(in crate::chunk) fn install_chunk_extraction(app: &mut App) {
    app.add_plugins(SyncComponentPlugin::<ChunkRenderInstance>::default())
        .add_plugins((
            ExtractResourcePlugin::<ChunkTextureAssets>::default(),
            ExtractResourcePlugin::<ChunkAnimationClock>::default(),
            ExtractResourcePlugin::<ChunkBiomeTints>::default(),
            ExtractResourcePlugin::<ChunkUploadBudget>::default(),
            ExtractResourcePlugin::<ChunkGpuRemovalQueue>::default(),
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
    values.extend(
        query
            .iter()
            .map(|(render_entity, instance)| (render_entity, instance.clone())),
    );
    *previous_len = values.len();
    commands.try_insert_batch(values);
}
