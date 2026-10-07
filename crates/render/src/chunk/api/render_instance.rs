use crate::chunk::*;

/// Extracted packed geometry for one visible, frustum-cullable sub-chunk.
#[derive(Component, Clone, ExtractComponent)]
#[extract_component_filter(Changed<ChunkRenderInstance>)]
#[require(VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<ChunkRenderInstance>)]
pub struct ChunkRenderInstance {
    pub(in crate::chunk) light_emitters: Arc<[meshing::BlockLightEmitter]>,
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) cube_quads: Arc<[PackedQuad]>,
    pub(in crate::chunk) cube_lighting: Arc<[PackedQuadLighting]>,
    pub(in crate::chunk) model_refs: Arc<[PackedModelRef]>,
    pub(in crate::chunk) model_lighting: Arc<[PackedQuadLighting]>,
    pub(in crate::chunk) model_draw_refs: Arc<[PackedModelDrawRef]>,
    pub(in crate::chunk) transparent_model_draw_refs: Arc<[PackedModelDrawRef]>,
    pub(in crate::chunk) liquid_quads: Arc<[PackedLiquidQuad]>,
    pub(in crate::chunk) liquid_lighting: Arc<[PackedQuadLighting]>,
    pub(in crate::chunk) has_depth_liquid: bool,
    pub(in crate::chunk) has_transparent_liquid: bool,
    pub(in crate::chunk) depth_liquid_start: Option<u32>,
    pub(in crate::chunk) biome: PackedBiomeRecord,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) priority: ChunkUploadPriority,
    pub(in crate::chunk) token: Option<ChunkUploadToken>,
    pub(in crate::chunk) publication_permit: Option<PublicationPermitSlot>,
    pub(in crate::chunk) origin: [i32; 3],
}

impl ChunkRenderInstance {
    /// Retains immutable resident geometry without copying its packed streams.
    pub(crate) fn indirect_geometry(
        &self,
    ) -> (
        Arc<[PackedQuad]>,
        Arc<[PackedModelRef]>,
        Arc<[PackedQuadLighting]>,
        Arc<[PackedQuadLighting]>,
    ) {
        (
            Arc::clone(&self.cube_quads),
            Arc::clone(&self.model_refs),
            Arc::clone(&self.cube_lighting),
            Arc::clone(&self.model_lighting),
        )
    }
    pub fn light_emitters(&self) -> &[meshing::BlockLightEmitter] {
        &self.light_emitters
    }
    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        self.key
    }

    #[must_use]
    pub fn quad_count(&self) -> usize {
        self.cube_quads.len()
    }

    #[must_use]
    pub fn quads(&self) -> &[PackedQuad] {
        &self.cube_quads
    }

    /// CPU-retained cube lighting sidecars consumed through the shared GPU
    /// geometry arena without changing this extraction contract.
    #[must_use]
    pub fn cube_lighting(&self) -> &[PackedQuadLighting] {
        &self.cube_lighting
    }

    #[must_use]
    pub fn model_refs(&self) -> &[PackedModelRef] {
        &self.model_refs
    }

    #[must_use]
    pub fn model_lighting(&self) -> &[PackedQuadLighting] {
        &self.model_lighting
    }

    #[must_use]
    pub fn model_draw_refs(&self) -> &[PackedModelDrawRef] {
        &self.model_draw_refs
    }

    #[must_use]
    pub fn transparent_model_draw_refs(&self) -> &[PackedModelDrawRef] {
        &self.transparent_model_draw_refs
    }

    #[must_use]
    pub fn liquid_quads(&self) -> &[PackedLiquidQuad] {
        &self.liquid_quads
    }

    #[must_use]
    pub fn liquid_lighting(&self) -> &[PackedQuadLighting] {
        &self.liquid_lighting
    }

    #[must_use]
    pub const fn biome_record(&self) -> &PackedBiomeRecord {
        &self.biome
    }

    #[must_use]
    pub const fn tint_revision(&self) -> u64 {
        self.tint_identity.revision()
    }

    #[must_use]
    pub const fn tint_identity(&self) -> ChunkBiomeTintIdentity {
        self.tint_identity
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}
