use crate::chunk::*;
use crate::{ChunkUploadPriority, ChunkUploadToken};

/// Extracted packed geometry for one visible, frustum-cullable sub-chunk.
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(bevy::render::RenderApp)]
#[extract_component_filter(Changed<ChunkRenderInstance>)]
#[require(VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<ChunkRenderInstance>)]
pub struct ChunkRenderInstance {
    pub key: SubChunkKey,
    pub cube_quads: Arc<[PackedQuad]>,
    pub cube_lighting: Arc<[PackedQuadLighting]>,
    pub cube_layout: CubeQuadLayout,
    pub model_refs: Arc<[PackedModelRef]>,
    pub model_lighting: Arc<[PackedQuadLighting]>,
    pub model_draw_refs: Arc<[PackedModelDrawRef]>,
    pub transparent_model_draw_refs: Arc<[PackedModelDrawRef]>,
    pub liquid_quads: Arc<[PackedLiquidQuad]>,
    pub liquid_lighting: Arc<[PackedQuadLighting]>,
    pub has_depth_liquid: bool,
    pub has_transparent_liquid: bool,
    pub depth_liquid_start: Option<u32>,
    pub biome: PackedBiomeRecord,
    pub tint_identity: ChunkBiomeTintIdentity,
    pub generation: u64,
    pub priority: ChunkUploadPriority,
    pub token: Option<ChunkUploadToken>,
    pub publication_permit: Option<PublicationPermitSlot>,
    pub origin: [i32; 3],
}

impl ChunkRenderInstance {
    /// Replaces geometry while preserving the resident entity and its world identity.
    pub fn with_resource_mesh(
        mut self,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> Self {
        self.biome = biome;
        self.tint_identity = tint_identity;
        self.cube_layout = mesh.cube_layout();
        let (cubes, cube_light, models, model_light, draws, transparent, liquids, liquid_light) =
            mesh.into_streams();
        self.cube_quads = cubes.into();
        self.cube_lighting = cube_light.into();
        self.model_refs = models.into();
        self.model_lighting = model_light.into();
        self.model_draw_refs = draws.into();
        self.transparent_model_draw_refs = transparent.into();
        self.depth_liquid_start = liquids
            .iter()
            .position(|quad| quad.is_depth_writing())
            .map(|i| i as u32);
        self.has_depth_liquid = self.depth_liquid_start.is_some();
        self.has_transparent_liquid = liquids.first().is_some_and(|quad| !quad.is_depth_writing());
        self.liquid_quads = liquids.into();
        self.liquid_lighting = liquid_light.into();
        self.token = None;
        self.publication_permit = None;
        self
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
