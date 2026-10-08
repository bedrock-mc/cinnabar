//! Visibility and adapter diagnostics published by the render world.
use world::{ChunkKey, SubChunkKey};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityKeyDigest {
    pub count: u64,
    pub hash: u64,
}

impl VisibilityKeyDigest {
    #[must_use]
    pub fn from_keys(keys: impl IntoIterator<Item = SubChunkKey>) -> Self {
        keys.into_iter().fold(Self::default(), |mut digest, key| {
            digest.insert(key);
            digest
        })
    }

    fn insert(&mut self, key: SubChunkKey) {
        self.count = self.count.saturating_add(1);
        self.hash = self.hash.wrapping_add(hash_sub_chunk_key(key));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityKeyDelta {
    pub missing: VisibilityKeyDigest,
    pub extra: VisibilityKeyDigest,
}

fn hash_sub_chunk_key(key: SubChunkKey) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in key
        .dimension
        .to_le_bytes()
        .into_iter()
        .chain(key.x.to_le_bytes())
        .chain(key.y.to_le_bytes())
        .chain(key.z.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^ (hash >> 33)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractedCameraIdentity {
    pub stable_id: u64,
    pub pose_hash: u64,
    pub frustum_hash: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OpaqueDrawMode {
    Direct,
    MultiDrawIndirect,
    #[default]
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityDiagnosticSnapshot {
    pub frame_generation: u64,
    pub camera: ExtractedCameraIdentity,
    pub pose_generation: u64,
    pub view_generation: u64,
    pub witness_column: Option<ChunkKey>,
    pub resident_witness_subchunks: Option<u32>,
    pub frustum_witness_subchunks: Option<u32>,
    pub submitted_witness_subchunks: Option<u32>,
    pub gpu_completed_witness_subchunks: Option<u32>,
    pub resident_mesh: Option<VisibilityKeyDigest>,
    pub cave_visible: Option<VisibilityKeyDigest>,
    pub frustum_visible_opaque: Option<VisibilityKeyDigest>,
    pub submitted_opaque: Option<VisibilityKeyDigest>,
    pub gpu_completed_opaque: Option<VisibilityKeyDigest>,
    pub resident_to_cave: Option<VisibilityKeyDelta>,
    pub resident_to_frustum: Option<VisibilityKeyDelta>,
    pub cave_to_frustum: Option<VisibilityKeyDelta>,
    pub frustum_to_submitted: Option<VisibilityKeyDelta>,
    pub submitted_to_gpu_completed: Option<VisibilityKeyDelta>,
    pub draw_mode: OpaqueDrawMode,
    pub resident_overflowed: bool,
    pub cave_overflowed: bool,
    pub frustum_overflowed: bool,
    pub submitted_overflowed: bool,
}

impl VisibilityDiagnosticSnapshot {
    #[must_use]
    /// The snapshot once the GPU has completed everything it submitted.
    pub fn gpu_completed(mut self) -> Self {
        self.gpu_completed_opaque = self.submitted_opaque;
        self.gpu_completed_witness_subchunks = self.submitted_witness_subchunks;
        self.submitted_to_gpu_completed =
            self.submitted_opaque.map(|_| VisibilityKeyDelta::default());
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphicsAdapterMetadata {
    pub backend: String,
    pub adapter: String,
    pub driver: String,
    pub driver_info: String,
    pub requested_present_mode: String,
    pub effective_present_mode: String,
    pub present_mode_proven: bool,
}
