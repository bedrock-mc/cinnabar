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
pub(super) const LIGHTING_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("4562a3ce-92ab-46f2-823f-af9faf2cc5c8");
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
pub(super) const FALLBACK_BIOME_WORDS: usize = 13;
pub(super) const FALLBACK_BIOME_RECORD: [u32; FALLBACK_BIOME_WORDS] =
    [0x4249_4f31, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 1 << 8, 0];
pub(super) const INDEXED_INDIRECT_BYTES: u64 = 20;
