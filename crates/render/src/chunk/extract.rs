use crate::chunk::*;

pub(in crate::chunk) fn install_chunk_extraction(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<ChunkRenderInstance>::default(),
        ExtractResourcePlugin::<ChunkTextureAssets>::default(),
        ExtractResourcePlugin::<ChunkAnimationClock>::default(),
        ExtractResourcePlugin::<ChunkBiomeTints>::default(),
        ExtractResourcePlugin::<ChunkUploadBudget>::default(),
        ExtractResourcePlugin::<ChunkGpuRemovalQueue>::default(),
        ExtractResourcePlugin::<TransparentWitnessRequest>::default(),
        ExtractResourcePlugin::<ModelWitnessRequest>::default(),
        ExtractResourcePlugin::<VisibilityDiagnosticsInput>::default(),
    ));
}
