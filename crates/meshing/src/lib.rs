//! Pure CPU geometry construction for chunks, liquids, biomes, and clouds.

pub mod biome;
pub mod biome_lattice;
mod chunk;
mod classifier;
mod output_memory;
pub use output_memory::{MeshOutputBounds, mesh_output_byte_len};
pub mod cloud;
pub mod cloud_viewport;
pub mod color;
mod connectivity;
mod contributors;
mod cube_layout;
pub mod lighting;
pub mod liquid;
mod publication;
mod types;

const SIDE: usize = world::SUB_CHUNK_SIDE;

pub use biome::{
    BIOME_NEIGHBOUR_SLOT_COUNT, BiomeBlendSample, ChunkBiomeTintIdentity,
    MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord, biome_neighbour_index, biome_volume_index,
};
pub use chunk::build::{
    mesh_sub_chunk, mesh_sub_chunk_in_neighbourhood, mesh_sub_chunk_in_neighbourhood_with_lighting,
    mesh_sub_chunk_with_lighting,
};
pub use classifier::BlockClassifier;
pub use cloud::{
    CLOUD_CELL_BLOCKS, CLOUD_MASK_SIZE, CLOUD_THICKNESS_BLOCKS, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y,
    CLOUD_WORLD_PERIOD, CloudFace, CloudMeshError, MAX_CLOUD_BYTES, MAX_CLOUD_QUADS,
    PackedCloudQuad, cloud_face_shade, cloud_instance_origins, mesh_cloud_texture,
};
pub use color::debug_color;
pub use contributors::{ContributorResolver, ResolvedContributors};
pub use cube_layout::{CubeQuadLayout, FaceMask, is_single_sided_opaque, sub_chunk_facing_faces};
pub use lighting::{
    FullBrightLightSampler, MeshLightSample, MeshLightSampler, PHASE26_BLOCK_LIGHT,
    PHASE26_SKY_LIGHT, bake_quad_lighting, bake_quad_lighting_with_sampler, bake_template_lighting,
    bake_template_lighting_with_sampler, mesh_dependency_mask,
};
pub use liquid::{CameraMedium, LiquidLevel, sample_camera_medium};
pub use publication::{CHUNK_PUBLICATION_ORIGIN_BYTES, chunk_publication_byte_len};
pub use types::{
    ChunkMesh, ChunkMeshStreamError, ChunkMeshStreams, DiagnosticGeometryCount,
    DiagnosticGeometrySummary, Face, FaceConnectivity, MAX_DIAGNOSTIC_IDENTITIES_PER_MESH,
    Neighbourhood, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad,
    PackedQuadLighting,
};
