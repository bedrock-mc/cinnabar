use bevy::{
    asset::{Handle, uuid_handle},
    shader::Shader,
};

pub(super) const CHUNK_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("b5664c91-763f-4e5c-9310-d12659f70cd4");
pub(super) const MODEL_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("2cd46297-17aa-4c18-bfb1-83373bf39475");
pub(super) const LIQUID_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("52e731aa-0a4d-4b07-9d66-80eb7688398f");
/// `cinnabar::chunk_bindings`, the group-0 declarations the liquid and model modules share.
pub(super) const CHUNK_BINDINGS_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("04e0465f-ca30-4ec7-8e35-eb99e7c04ec2");
/// Sorted transparent terrain: liquid and model draws through one pipeline.
pub(super) const TRANSPARENT_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("21dc97c9-bb04-4354-8927-9001e9f5ac95");
pub(super) const BIOME_TINT_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("ee40bfe6-1bd1-4aa6-bf15-e3185dfac253");
pub(super) const STATIC_QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
pub(super) const PACKED_QUAD_BYTES: u64 = 8;
pub(super) const PACKED_MODEL_REF_BYTES: u64 = 16;
pub(super) const PACKED_MODEL_DRAW_REF_BYTES: u64 = 8;
pub(super) const PACKED_QUAD_LIGHTING_BYTES: u64 = 8;
pub(super) const PACKED_LIQUID_QUAD_BYTES: u64 = 16;
pub(super) const GEOMETRY_STREAM_WORD_BYTES: u64 = 4;
pub(super) const CHUNK_ORIGIN_BYTES: u64 = 32;
pub(super) const BIOME_WORD_BYTES: u64 = 4;
pub(super) const FALLBACK_BIOME_WORDS: usize = meshing::biome::FALLBACK_BIOME_WORDS.len();
pub(super) const FALLBACK_BIOME_RECORD: [u32; FALLBACK_BIOME_WORDS] =
    meshing::biome::FALLBACK_BIOME_WORDS;
pub(super) const INDEXED_INDIRECT_BYTES: u64 = 20;
