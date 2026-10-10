use crate::RuntimeStageProfiler;
use crate::chunk::gpu::graphics_metadata::{
    GraphicsMetadataPublication, GraphicsMetadataPublicationState,
    configure_graphics_metadata_publication, publish_graphics_runtime_metadata,
};
use crate::chunk::*;
use crate::chunk::{
    draw::{queue_chunks, queue_transparent_chunks},
    extract::install_chunk_extraction,
    pipeline::install_chunk_commands,
};

mod publication_schedule;
use publication_schedule::{ChunkPublicationStage, configure_chunk_publication};

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkEntities(pub(in crate::chunk) HashMap<SubChunkKey, Entity>);

/// Installs the capped main-world queue and the vertex-pulled Camera3d chunk
/// draw path. The renderer adds non-mesh items to Bevy's built-in opaque
/// phase, sharing its depth attachment without allocating a `Mesh` or
/// `StandardMaterial` per sub-chunk.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChunkRenderPlugin {
    pub(in crate::chunk) upload_budget: ChunkUploadBudget,
}

/// Main-world queue application boundary. Systems ordered after this set
/// observe spawned/updated/despawned chunk entities after deferred commands
/// and component observers have been applied.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkRenderApplySet;

impl ChunkRenderPlugin {
    #[must_use]
    pub const fn new(max_uploads_per_frame: usize) -> Self {
        Self {
            upload_budget: ChunkUploadBudget::new(
                max_uploads_per_frame,
                DEFAULT_RENDER_QUEUE_BYTES,
            ),
        }
    }

    #[must_use]
    pub const fn with_budget(upload_budget: ChunkUploadBudget) -> Self {
        Self { upload_budget }
    }
}

impl Plugin for ChunkRenderPlugin {
    fn build(&self, app: &mut App) {
        install_atmosphere(app);
        app.init_resource::<ChunkRenderQueue>()
            .init_resource::<crate::dropped_item_render::terrain_items::ImmediateTerrainMeshPublications>()
            .init_resource::<ChunkUploadAcknowledgements>()
            .init_resource::<ChunkGpuRemovalQueue>()
            .init_resource::<PresentedFrameGate>()
            .init_resource::<VisibilityDiagnosticsInput>()
            .init_resource::<VisibilityDiagnostics>()
            .init_resource::<ChunkEntities>()
            .init_resource::<ChunkTextureAssets>()
            .init_resource::<ChunkAnimationClock>()
            .init_resource::<ChunkBiomeTints>()
            .init_resource::<TransparentSortMetrics>()
            .init_resource::<ModelWorkloadMetrics>()
            .init_resource::<TransparentWitnessRequest>()
            .init_resource::<TransparentWitnessEvidence>()
            .init_resource::<ModelWitnessRequest>()
            .init_resource::<ModelWitnessEvidence>()
            .insert_resource(self.upload_budget)
            .add_systems(
                Update,
                (
                    apply_chunk_render_queue.in_set(ChunkRenderApplySet),
                    update_chunk_animation_clock,
                ),
            );

        if app.get_sub_app(RenderApp).is_none() {
            return;
        }

        app.init_resource::<ChunkTextureReload>();
        install_chunk_extraction(app);

        crate::lighting::install(app);
        load_internal_asset!(
            app,
            BIOME_TINT_SHADER_HANDLE,
            "../biome_tint.wgsl",
            |source, path| crate::shader_safety::from_wgsl(
                crate::material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
                    source
                )),
                path
            )
        );
        crate::enhanced::load_shader_imports(app);
        load_internal_asset!(
            app,
            CHUNK_BINDINGS_SHADER_HANDLE,
            "../chunk_bindings.wgsl",
            |source, path| crate::shader_safety::from_wgsl(
                crate::material_shader::source(source),
                path
            )
        );
        load_internal_asset!(app, CHUNK_SHADER_HANDLE, "../chunk.wgsl", |source, path| {
            crate::shader_safety::from_wgsl(crate::material_shader::source(source), path)
        });
        load_internal_asset!(app, MODEL_SHADER_HANDLE, "../model.wgsl", |source, path| {
            crate::shader_safety::from_wgsl(crate::material_shader::source(source), path)
        });
        load_internal_asset!(
            app,
            LIQUID_SHADER_HANDLE,
            "../liquid.wgsl",
            |source, path| crate::shader_safety::from_wgsl(
                crate::material_shader::source(source),
                path
            )
        );
        load_internal_asset!(
            app,
            TRANSPARENT_SHADER_HANDLE,
            "../transparent_terrain.wgsl",
            |source, path| crate::shader_safety::from_wgsl(
                crate::material_shader::source(source),
                path
            )
        );

        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();
        let presented_frame_gate = app.world().resource::<PresentedFrameGate>().clone();
        let transparent_sort_metrics = app.world().resource::<TransparentSortMetrics>().clone();
        let model_workload_metrics = app.world().resource::<ModelWorkloadMetrics>().clone();
        let visibility_diagnostics = app.world().resource::<VisibilityDiagnostics>().clone();
        let runtime_stage_profiler = app.world().get_resource::<RuntimeStageProfiler>().cloned();
        let transparent_witness_evidence =
            app.world().resource::<TransparentWitnessEvidence>().clone();

        crate::pipeline_warmup::register::<ChunkPipeline>(app);
        let render_app = app.sub_app_mut(RenderApp);
        crate::device_poll::install(render_app);
        render_app
            .insert_resource(self.upload_budget)
            .insert_resource(acknowledgements)
            .insert_resource(presented_frame_gate)
            .insert_resource(transparent_sort_metrics)
            .insert_resource(model_workload_metrics)
            .insert_resource(visibility_diagnostics)
            .insert_resource(transparent_witness_evidence)
            .init_resource::<ChunkPipeline>()
            .init_resource::<GraphicsMetadataPublicationState>()
            .init_resource::<crate::dropped_item_render::terrain_items::TerrainItemMeshGenerations>(
            )
            .init_resource::<ChunkGpuUploadStats>()
            .init_resource::<GpuUpdateFairness>()
            .init_resource::<ChunkGpuTextureAssets>()
            .init_resource::<ChunkGpuBiomeTints>()
            .init_resource::<ChunkTextureUploadStats>()
            .init_resource::<pipeline::solid::ChunkSolidIndirectBatches>()
            .init_resource::<ChunkIndirectBatches>()
            .init_resource::<ChunkModelIndirectBatches>()
            .init_resource::<ChunkDepthLiquidIndirectBatches>()
            .init_resource::<ActiveFrameProbe>()
            .init_resource::<ActiveVisibilityFrameProbe>()
            .init_resource::<VisibilityCompletionFence>()
            .init_resource::<ExtractedCameraIdentityTracker>()
            .init_resource::<TransparentSortRuntime>()
            .init_resource::<TransparentModelSortRuntime>()
            .init_resource::<TransparentUploadBudget>()
            .init_resource::<TransparentPresentationFence>()
            .init_resource::<TransparentRetirementFence>();
        if let Some(runtime_stage_profiler) = runtime_stage_profiler {
            render_app.insert_resource(runtime_stage_profiler);
            crate::runtime_profile_trace::install_surface_trace(render_app);
        }
        install_chunk_commands(render_app);
        transparent::gamma_pass::install(app);
        let render_app = app.sub_app_mut(RenderApp);
        render_app.edit_schedule(Render, configure_chunk_publication);
        render_app.edit_schedule(Render, configure_graphics_metadata_publication);
        crate::surface_capabilities::install(render_app);
        render_app
            .add_systems(
                RenderStartup,
                (init_chunk_gpu_arena, init_chunk_gpu_animation_clock),
            )
            .add_systems(
                Render,
                (
                    publish_graphics_runtime_metadata.in_set(GraphicsMetadataPublication),
                    queue_chunks
                        .run_if(crate::panorama::world_passes_enabled)
                        .in_set(RenderSystems::Queue),
                    queue_transparent_chunks
                        .run_if(crate::panorama::world_passes_enabled)
                        .in_set(RenderSystems::Queue),
                    prepare_chunk_texture_assets
                        .in_set(RenderSystems::PrepareAssets)
                        .before(RenderSystems::Queue),
                    prepare_chunk_animation_clock.in_set(ChunkPublicationStage::Attributes),
                    prepare_chunk_biome_tints.in_set(ChunkPublicationStage::Attributes),
                    prepare_gpu_chunks
                        .in_set(ChunkPublicationStage::Geometry)
                        .after(crate::dropped_item_render::terrain_items::TerrainItemSessionSet)
                        .after(prepare_chunk_texture_assets),
                    prepare_transparent_sorts.in_set(ChunkPublicationStage::LiquidSort),
                    prepare_transparent_model_sorts.in_set(ChunkPublicationStage::ModelSort),
                    prepare_chunk_indirect_batches
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_gpu_chunks),
                    prepare_chunk_bind_group.in_set(RenderSystems::PrepareBindGroups),
                    submit_presented_frame_probe.in_set(crate::device_poll::FrameSubmissions),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        install_atmosphere(app);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            transparent::gamma_pass::install_graph(render_app.world_mut());
        }
        gpu_cull::install(app);
    }
}
