use crate::chunk::*;

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
            upload_budget: ChunkUploadBudget {
                max_per_frame: max_uploads_per_frame,
            },
        }
    }
}

impl Plugin for ChunkRenderPlugin {
    fn build(&self, app: &mut App) {
        install_atmosphere(app);
        app.init_resource::<ChunkRenderQueue>()
            .init_resource::<ChunkUploadAcknowledgements>()
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

        install_chunk_extraction(app);

        load_internal_asset!(
            app,
            LIGHTING_SHADER_HANDLE,
            "../lighting.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            BIOME_TINT_SHADER_HANDLE,
            "../biome_tint.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(app, CHUNK_SHADER_HANDLE, "../chunk.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, MODEL_SHADER_HANDLE, "../model.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            LIQUID_SHADER_HANDLE,
            "../liquid.wgsl",
            Shader::from_wgsl
        );

        let acknowledgements = app
            .world()
            .resource::<ChunkUploadAcknowledgements>()
            .clone();
        let presented_frame_gate = app.world().resource::<PresentedFrameGate>().clone();
        let transparent_sort_metrics = app.world().resource::<TransparentSortMetrics>().clone();
        let model_workload_metrics = app.world().resource::<ModelWorkloadMetrics>().clone();
        let visibility_diagnostics = app.world().resource::<VisibilityDiagnostics>().clone();
        let transparent_witness_evidence =
            app.world().resource::<TransparentWitnessEvidence>().clone();

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .insert_resource(self.upload_budget)
            .insert_resource(acknowledgements)
            .insert_resource(presented_frame_gate)
            .insert_resource(transparent_sort_metrics)
            .insert_resource(model_workload_metrics)
            .insert_resource(visibility_diagnostics)
            .insert_resource(transparent_witness_evidence)
            .init_resource::<ChunkPipeline>()
            .init_resource::<ChunkGpuUploadStats>()
            .init_resource::<GpuUpdateFairness>()
            .init_resource::<ChunkGpuTextureAssets>()
            .init_resource::<ChunkGpuBiomeTints>()
            .init_resource::<ChunkTextureUploadStats>()
            .init_resource::<ChunkIndirectBatches>()
            .init_resource::<ChunkModelIndirectBatches>()
            .init_resource::<ChunkDepthLiquidIndirectBatches>()
            .init_resource::<ActiveFrameProbe>()
            .init_resource::<ActiveVisibilityFrameProbe>()
            .init_resource::<ExtractedCameraIdentityTracker>()
            .init_resource::<TransparentSortRuntime>()
            .init_resource::<TransparentModelSortRuntime>()
            .init_resource::<TransparentUploadBudget>()
            .init_resource::<TransparentPresentationFence>()
            .init_resource::<TransparentRetirementFence>();
        install_chunk_commands(render_app);
        render_app
            .add_systems(
                RenderStartup,
                (init_chunk_gpu_arena, init_chunk_gpu_animation_clock),
            )
            .add_systems(
                Render,
                (
                    publish_graphics_runtime_metadata
                        .after(RenderSystems::ExtractCommands)
                        .before(bevy::render::view::window::create_surfaces),
                    queue_chunks.in_set(RenderSystems::Queue),
                    queue_transparent_chunks.in_set(RenderSystems::Queue),
                    prepare_chunk_texture_assets.in_set(RenderSystems::PrepareResources),
                    prepare_chunk_animation_clock.in_set(RenderSystems::PrepareResources),
                    prepare_chunk_biome_tints.in_set(RenderSystems::PrepareResources),
                    prepare_gpu_chunks.in_set(RenderSystems::PrepareResources),
                    prepare_transparent_sorts
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_gpu_chunks),
                    prepare_transparent_model_sorts
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_transparent_sorts),
                    prepare_chunk_indirect_batches
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_gpu_chunks),
                    prepare_chunk_bind_group.in_set(RenderSystems::PrepareBindGroups),
                    submit_presented_frame_probe
                        .in_set(RenderSystems::Render)
                        .after(bevy::render::renderer::render_system),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        install_atmosphere(app);
    }
}
