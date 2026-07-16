//! Pure CPU geometry construction for chunks, liquids, biomes, and clouds.

pub mod biome;
mod chunk;
mod classifier;
pub mod cloud;
pub mod color;
mod connectivity;
mod contributors;
pub mod lighting;
pub mod liquid;
mod types;

const SIDE: usize = 16;

pub use biome::{
    BIOME_NEIGHBOUR_SLOT_COUNT, ChunkBiomeTintIdentity, MAX_PACKED_BIOME_RECORD_WORDS,
    PackedBiomeRecord, biome_neighbour_index,
};
pub use chunk::build::{
    mesh_sub_chunk, mesh_sub_chunk_in_neighbourhood, mesh_sub_chunk_in_neighbourhood_with_lighting,
    mesh_sub_chunk_with_lighting,
};
pub use classifier::BlockClassifier;
pub use cloud::{
    CLOUD_MASK_SIZE, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CloudFace, CloudMeshError, MAX_CLOUD_BYTES,
    MAX_CLOUD_QUADS, PackedCloudQuad, cloud_instance_origins, mesh_cloud_texture,
};
pub use color::debug_color;
pub use contributors::{ContributorResolver, ResolvedContributors};
pub use lighting::{
    FullBrightLightSampler, MeshLightSample, MeshLightSampler, PHASE26_BLOCK_LIGHT,
    PHASE26_SKY_LIGHT, bake_quad_lighting, bake_quad_lighting_with_sampler, bake_template_lighting,
    bake_template_lighting_with_sampler, mesh_dependency_mask,
};
pub use liquid::{CameraMedium, LiquidLevel, sample_camera_medium};
pub use types::{
    ChunkMesh, ChunkMeshStreamError, ChunkMeshStreams, DiagnosticGeometryCount,
    DiagnosticGeometrySummary, Face, FaceConnectivity, MAX_DIAGNOSTIC_IDENTITIES_PER_MESH,
    Neighbourhood, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad,
    PackedQuadLighting,
};
