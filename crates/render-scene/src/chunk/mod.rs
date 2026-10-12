//! Chunk submission queues, immutable instances, and presentation acknowledgements.
use assets::{
    ANIMATION_FLAG_BLEND, Animation, NO_ANIMATION, ResolvedBiomeTints, RuntimeAssets, TextureArray,
    TextureMip,
};
use bevy::{
    camera::visibility::{self, VisibilityClass},
    prelude::{Changed, Component, Resource, Vec3},
    render::{
        extract_component::ExtractComponent, extract_resource::ExtractResource,
        render_resource::ShaderType,
    },
};
use meshing::{
    ChunkBiomeTintIdentity, ChunkMesh, CubeQuadLayout, Face, PackedBiomeRecord, PackedLiquidQuad,
    PackedModelDrawRef, PackedModelRef, PackedQuad, PackedQuadLighting, chunk_publication_byte_len,
};
use render_api::{PublicationPermit, PublicationPermitStage, PublicationServiceConfig};
use render_model::{
    ModelWorkloadCount, ModelWorkloadMetricsSnapshot, TransparentSortMetricsSnapshot,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};
use web_time::Instant;
use world::SubChunkKey;
pub mod api;
pub use api::{
    AcknowledgementSlot, AcknowledgementState, ChunkGpuRemovalQueue, ChunkUploadAcknowledgement,
    ChunkUploadAcknowledgements, ChunkUploadBudget, ChunkUploadPriority, ChunkUploadToken,
    CompletedFrameProbe, DEFAULT_ACKNOWLEDGEMENT_CAPACITY, DEFAULT_PRESENTED_FRAME_ACK_CAPACITY,
    DEFAULT_ZERO_BYTE_OPERATIONS_PER_FRAME, FrameCompletionEvidence, ModelWitnessFrameEvaluation,
    PendingGpuRemoval, PendingRemoval, PendingUpload, PresentedFrameAck, PresentedFrameGate,
    PresentedFrameGateState, PublicationPermitSlot, RenderViewCohort, TargetRenderExpectation,
    evaluate_model_witness_frame,
};
pub mod biome_tints;
pub use biome_tints::{
    BiomeTint, ChunkBiomeTintResourceIdentity, ChunkBiomeTints, MATERIAL_UV_REFLECT_U,
    MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270,
    MATERIAL_UV_ROTATION_MASK,
};
pub mod instance;
pub use instance::ChunkRenderInstance;
pub mod texture_reload;
pub use texture_reload::ChunkTextureReload;
pub mod textures;
pub use textures::{
    ANIMATION_TICK_MODULUS, ANIMATION_TICKS_PER_SECOND, AnimationFrameSample, ChunkAnimationClock,
    ChunkTextureAssetIdentity, ChunkTextureAssets, TextureArrayLimits, TextureLimitError,
    TextureMipUploadPlan, TexturePageBinding, TextureUploadPlanError, diagnostic_texture_page,
    greedy_texture_uv, plan_texture_mip_uploads, plan_texture_page_bindings,
    select_animation_frames, texture_asset_needs_rebuild,
};
pub mod queue;
pub use queue::{
    ChunkRenderQueue, ChunkRenderQueueLimits, DEFAULT_RENDER_QUEUE_BYTES,
    DEFAULT_RENDER_QUEUE_ITEMS, biome_record_byte_len, biome_record_is_fallback, chunk_origin,
    pending_upload_byte_len,
};
pub mod frame_evidence;
pub use frame_evidence::{
    ChunkStreamMask, MAX_MODEL_WITNESS_KEYS, MAX_TRANSPARENT_WITNESS_KEYS,
    build_presented_frame_ack,
};
pub mod metrics;
pub use metrics::{ModelWorkloadMetrics, TransparentSortMetrics};
pub mod model_witness;
pub use model_witness::{
    ModelWitnessEvent, ModelWitnessEvidence, ModelWitnessEvidenceState, ModelWitnessFrameAck,
    ModelWitnessManifestRecord, ModelWitnessRequest, ModelWitnessRequestError,
};
pub mod transparent_witness;
pub use transparent_witness::{
    TransparentWitnessEvent, TransparentWitnessEvidence, TransparentWitnessEvidenceState,
    TransparentWitnessIncompleteEvent, TransparentWitnessRequest, TransparentWitnessRequestError,
    TransparentWitnessStageEvent, TransparentWitnessStageRecord, TransparentWitnessToken,
};
pub mod schedule;
pub use schedule::ChunkRenderApplySet;
