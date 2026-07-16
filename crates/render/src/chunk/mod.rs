use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque, hash_map::Entry},
    ops::Range,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    time::{Duration, Instant},
};

use assets::{
    ANIMATION_FLAG_BLEND, Animation, Material, ModelTemplate, NO_ANIMATION, ResolvedBiomeTints,
    RuntimeAssets, TextureArray, TextureMip, TextureRef,
};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    camera::{
        primitives::Aabb,
        visibility::{self, VisibilityClass},
    },
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
    },
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParam, SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::Mesh,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline,
            TrackedRenderPass, ViewBinnedRenderPhases, ViewRangefinder3d, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferDescriptor, BufferId, BufferInitDescriptor, BufferUsages,
            Canonical, ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, DownlevelFlags, DrawIndexedIndirectArgs, Extent3d, Face as CullFace,
            FilterMode, FragmentState, IndexFormat, Origin3d, PipelineCache, PollType,
            PrimitiveState, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState, WgpuFeatures,
        },
        renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue},
        settings::Backends,
        sync_world::MainEntity,
        view::{
            ExtractedView, RenderVisibleEntities, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms, window::ExtractedWindows,
        },
    },
};
use meshing::{ChunkBiomeTintIdentity, Face};
use world::SubChunkKey;

use crate::{
    AtmosphereFrame, ChunkMesh, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting,
    atmosphere_render::{AtmosphereGpu, install_atmosphere},
    visibility_diagnostics::{
        ActiveVisibilityFrameProbe, ExtractedCameraIdentity, ExtractedCameraIdentityTracker,
        GraphicsAdapterMetadata, MAX_VISIBILITY_DIAGNOSTIC_KEYS, OpaqueDrawMode,
        VisibilityDiagnostics, VisibilityDiagnosticsInput, VisibilityFrameProbe, hash_f32_words,
    },
};

const CHUNK_SHADER_HANDLE: Handle<Shader> = uuid_handle!("b5664c91-763f-4e5c-9310-d12659f70cd4");
const MODEL_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2cd46297-17aa-4c18-bfb1-83373bf39475");
const LIQUID_SHADER_HANDLE: Handle<Shader> = uuid_handle!("52e731aa-0a4d-4b07-9d66-80eb7688398f");
const LIGHTING_SHADER_HANDLE: Handle<Shader> = uuid_handle!("4562a3ce-92ab-46f2-823f-af9faf2cc5c8");
const BIOME_TINT_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("ee40bfe6-1bd1-4aa6-bf15-e3185dfac253");
const STATIC_QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
const PACKED_QUAD_BYTES: u64 = 8;
const PACKED_MODEL_REF_BYTES: u64 = 16;
const PACKED_MODEL_DRAW_REF_BYTES: u64 = 8;
const PACKED_QUAD_LIGHTING_BYTES: u64 = 8;
const PACKED_LIQUID_QUAD_BYTES: u64 = 16;
const GEOMETRY_STREAM_WORD_BYTES: u64 = 4;
const CHUNK_ORIGIN_BYTES: u64 = 32;
const BIOME_WORD_BYTES: u64 = 4;
const FALLBACK_BIOME_WORDS: usize = 13;
const FALLBACK_BIOME_RECORD: [u32; FALLBACK_BIOME_WORDS] =
    [0x4249_4f31, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 1 << 8, 0];
const INDEXED_INDIRECT_BYTES: u64 = 20;

mod api;
mod biome_tints;
mod draw;
mod extract;
mod gpu;
mod pipeline;
mod plugin;
mod presentation;
mod queue;
mod textures;
mod transparent;

#[allow(unused_imports)]
use api::{
    AcknowledgementSlot, AcknowledgementState, CompletedFrameProbe,
    DEFAULT_ACKNOWLEDGEMENT_CAPACITY, DEFAULT_PRESENTED_FRAME_ACK_CAPACITY,
    FrameCompletionEvidence, ModelWitnessFrameEvaluation, PendingRemoval, PendingUpload,
    PresentedFrameGateState, evaluate_model_witness_frame,
};
pub use api::{
    ChunkRenderInstance, ChunkUploadAcknowledgement, ChunkUploadAcknowledgements,
    ChunkUploadBudget, ChunkUploadPriority, ChunkUploadToken, PresentedFrameAck,
    PresentedFrameGate, RenderViewCohort, TargetRenderExpectation,
};
pub use biome_tints::{
    BiomeTint, ChunkBiomeTints, MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V,
    MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270,
};
#[allow(unused_imports)]
use biome_tints::{ChunkBiomeTintResourceIdentity, MATERIAL_UV_ROTATION_MASK};
use draw::{queue_chunks, queue_transparent_chunks};
use extract::install_chunk_extraction;
#[allow(unused_imports)]
use gpu::arena::{
    ArenaLimits, ChunkGpuArena, ChunkGpuUploadStats, ChunkRangePlan, GPU_UPDATE_OVERDUE_FRAMES,
    GpuUpdateCandidate, GpuUpdateFairness, MAX_GPU_UPDATE_FAIRNESS_ENTRIES,
    allocate_aligned_quad_range, allocate_aligned_range_for_update, allocate_for_chunk_update,
    allocate_origin, allocate_quad_range, allocate_range_for_update,
    arena_limits_from_device_limits, checked_geometry_range, chunk_tint_identity_is_active,
    create_indirect_buffer, create_storage_buffer, free_allocation, init_chunk_gpu_arena,
    insert_free_quad_range, plan_chunk_range_update, plan_gpu_chunk_updates,
    release_completed_transparent_retirements, release_origin, release_quad_range,
    take_free_quad_range,
};
pub use gpu::bind_groups::ChunkTextureUploadStats;
#[allow(unused_imports)]
use gpu::bind_groups::{
    AnimationGpu, BiomeTintGpu, ChunkBindGroupBuffers, ChunkGpuAnimationClock, ChunkGpuBiomeTints,
    ChunkGpuTextureAssets, MaterialGpu, PreparedChunkBiomeTints, PreparedChunkTextureAssets,
    bind_group_needs_rebuild, biome_tint_bind_group_needs_rebuild,
    biome_tint_gpu_buffer_needs_rebuild, chunk_sampler_descriptor, encode_model_template_words,
    init_chunk_gpu_animation_clock, pack_linear_rgb10, padded_mip_bytes,
    prepare_biome_tint_entries, prepare_chunk_animation_clock, prepare_chunk_bind_group,
    prepare_chunk_biome_tints, prepare_chunk_texture_assets, storage_table_fits,
    upload_texture_page,
};
#[allow(unused_imports)]
use gpu::layout::{
    ArenaGrowthError, ArenaGrowthPlan, GeometryStreamCounts, GeometryStreamLayout,
    SHARED_GEOMETRY_ALIGNMENT_WORDS, account_chunk_gpu_uploads, buffer_byte_len, checked_align_up,
    copy_gpu_buffer, ensure_biome_capacity, ensure_geometry_stream_capacities,
    ensure_origin_capacity, ensure_quad_capacity, ensure_stream_capacity, plan_arena_growth,
    transparent_geometry_update_requires_cow, write_stream_records,
};
#[allow(unused_imports)]
use gpu::types::{
    ArenaAllocation, ChunkDepthLiquidIndirectBatches, ChunkDrawMode, ChunkIndirectBatch,
    ChunkIndirectBatches, ChunkModelIndirectBatches, GpuChunkAllocation, GpuChunkOrigin,
    LEGACY_FIXED_MODEL_QUADS_PER_REF, MODEL_INDEX_COUNT, QueueFrameProbeParams,
    RetiredArenaAllocation, StreamAddresses, adapter_metadata_field, cube_lighting_record_address,
    cube_stream_addresses_valid, depth_liquid_direct_draw_command, depth_liquid_draw_command,
    depth_liquid_mdi_draw_command, diagnostic_draw_mode, direct_stream_addresses,
    extracted_camera_identity, gpu_chunk_origin, indexed_indirect_command, mdi_stream_addresses,
    metadata_base_vertex, model_direct_draw_command, model_draw_command, model_mdi_draw_command,
    model_ref_count_for_witness, opaque_allocation_is_drawable, publish_graphics_runtime_metadata,
    resolve_surface_present_mode, select_chunk_draw_mode, shared_stream_ranges_disjoint,
    summarize_model_workload, surface_present_mode_name, transparent_model_direct_draw_command,
    window_present_mode_name,
};
#[allow(unused_imports)]
use gpu::upload::{
    absolutize_liquid_lighting_indices, absolutize_model_lighting_bases,
    absolutize_partitioned_model_draw_refs, liquid_quad_centroid, packed_lighting_records,
    packed_stream_range_matches, prepare_gpu_chunks, transparent_allocation_matches,
    transparent_model_allocation_matches, validate_partitioned_model_streams,
};
#[allow(unused_imports)]
use pipeline::commands::{
    DrawChunkCommands, DrawChunkIndirectCommands, DrawDepthLiquid, DrawDepthLiquidCommands,
    DrawDepthLiquidIndirectCommands, DrawDepthLiquidsIndirect, DrawModelCommands,
    DrawModelIndirectCommands, DrawPackedChunk, DrawPackedChunksIndirect, DrawPackedModel,
    DrawPackedModelsIndirect, DrawPackedTransparentModel, DrawTransparentLiquid,
    DrawTransparentLiquidCommands, DrawTransparentLiquidIndirect,
    DrawTransparentLiquidIndirectCommands, DrawTransparentModelCommands, OpaqueChunkViewQuery,
    drawable_allocation_identity, indirect_batch_draw_args, prepare_chunk_indirect_batches,
    prepare_depth_liquid_indirect_batch_draws, prepare_indirect_batch_draws,
    prepare_model_indirect_batch_draws, record_visibility_direct_submission,
    record_visibility_mdi_submissions, sorted_visible_entities,
};
use pipeline::install_chunk_commands;
#[allow(unused_imports)]
use pipeline::layouts::{ChunkPipeline, ChunkPipelineKey, ChunkPipelineSpecializer};
use plugin::ChunkEntities;
pub use plugin::{ChunkRenderApplySet, ChunkRenderPlugin};
#[allow(unused_imports)]
use presentation::frame_probe::{
    ActiveFrameProbe, ActiveFrameProbeState, ChunkStreamMask, FrameAllocationIdentity,
    FrameInstanceIdentity, FrameProbe, build_presented_frame_ack, submit_presented_frame_probe,
};
pub use presentation::metrics::{
    ModelWorkloadCount, ModelWorkloadMetrics, ModelWorkloadMetricsSnapshot, TransparentSortMetrics,
    TransparentSortMetricsSnapshot,
};
#[allow(unused_imports)]
use presentation::model_witness::ModelWitnessEvidenceState;
pub use presentation::model_witness::{
    ModelWitnessEvent, ModelWitnessEvidence, ModelWitnessFrameAck, ModelWitnessManifestRecord,
    ModelWitnessRequest, ModelWitnessRequestError,
};
pub use presentation::transparent_witness::{
    TransparentWitnessEvent, TransparentWitnessEvidence, TransparentWitnessIncompleteEvent,
    TransparentWitnessRequest, TransparentWitnessRequestError, TransparentWitnessStageEvent,
    TransparentWitnessStageRecord,
};
#[allow(unused_imports)]
use presentation::transparent_witness::{TransparentWitnessEvidenceState, TransparentWitnessToken};
pub use queue::{ChunkRenderQueue, ChunkRenderQueueLimits};
#[allow(unused_imports)]
use queue::{
    DEFAULT_RENDER_QUEUE_BYTES, DEFAULT_RENDER_QUEUE_ITEMS, apply_chunk_render_queue,
    biome_record_byte_len, biome_record_is_fallback, chunk_origin, mesh_byte_len,
    pending_upload_byte_len, update_chunk_animation_clock,
};
#[allow(unused_imports)]
use textures::{ANIMATION_TICK_MODULUS, ANIMATION_TICKS_PER_SECOND};
pub use textures::{
    AnimationFrameSample, ChunkAnimationClock, ChunkTextureAssetIdentity, ChunkTextureAssets,
    TextureArrayLimits, TextureLimitError, TextureMipUploadPlan, TexturePageBinding,
    TextureUploadPlanError, diagnostic_texture_page, greedy_texture_uv, plan_texture_mip_uploads,
    plan_texture_page_bindings, select_animation_frames, texture_asset_needs_rebuild,
};
#[allow(unused_imports)]
use transparent::liquid::{
    transparent_frame_draw_for_range, transparent_frame_draws, transparent_liquid_phase_distance,
};
#[allow(unused_imports)]
use transparent::model::{
    TransparentModelAddressIdentity, TransparentModelAllocationIdentity,
    TransparentModelCandidateCache, TransparentModelSortBatch, TransparentModelSortCandidate,
    TransparentModelSortKey, TransparentModelSortRuntime, TransparentModelSortWork,
    TransparentModelStagedSort, TransparentModelWorkerResult, TransparentUploadBudget,
    canonical_transparent_rotation_bits, clear_active_transparent_metrics,
    fail_closed_transparent_sort_key_error, prepare_transparent_model_sorts,
    sort_transparent_model_candidates, spawn_transparent_model_sort, spawn_transparent_sort,
    take_transparent_model_upload_batches, transparent_model_draw_candidate,
    transparent_model_phase_distance, transparent_model_subchunk_center,
    transparent_request_to_commit_latency,
};
#[allow(unused_imports)]
use transparent::retirement::{
    TransparentPresentationFence, TransparentRetirementBudget, TransparentRetirementFence,
    TransparentRetirementFenceState, record_encoded_transparent_generation,
    record_gpu_completed_transparent_generation, transparent_retirement_can_arm,
    transparent_snapshot_references_allocation, transparent_view_missing_witness_keys,
};

#[cfg(test)]
#[allow(unused_imports)]
use gpu::types::build_indexed_indirect_commands;
#[cfg(test)]
#[allow(unused_imports)]
use gpu::upload::{
    PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR, PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR,
    absolutize_model_draw_refs, packed_light_factor, validate_local_model_streams,
};
#[cfg(test)]
#[allow(unused_imports)]
use transparent::model::sorted_transparent_model_draw_words;
#[cfg(test)]
#[allow(unused_imports)]
use transparent::retirement::transparent_view_key_satisfies_witness;
pub use transparent::sort::{
    DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME, MAX_MODEL_WITNESS_KEYS, MAX_TRANSPARENT_DRAW_REFS,
    MAX_TRANSPARENT_VIEWS, MAX_TRANSPARENT_WITNESS_KEYS, PackedTransparentDrawRef,
    TRANSPARENT_REF_BUFFER_BYTES, TRANSPARENT_REF_SLOT_BYTES, TransparentAllocationIdentity,
    TransparentDrawArgs, TransparentOrderedSnapshot, TransparentSortCandidate,
    TransparentSortError, TransparentSortJobGate, TransparentSortResult, TransparentSortState,
    TransparentUploadBatch, ViewSortGeneration, ViewSortKey, direct_transparent_draw_args_for_test,
    mdi_transparent_draw_args_for_test, sort_transparent_candidates_for_test,
    validate_transparent_sort_ref_count,
};
#[allow(unused_imports)]
use transparent::sort::{
    MAX_TRANSPARENT_RETIRED_ALLOCATIONS, MAX_TRANSPARENT_RETIRED_BYTES, TransparentAddressIdentity,
    TransparentCandidateCache, TransparentLiquidPhaseGroup, TransparentSortRequest,
    TransparentSortRuntime, TransparentSortWork, TransparentStagedSnapshot,
    TransparentWorkerResult, build_transparent_candidates, prepare_transparent_sorts,
    sort_transparent_candidates, transparent_draw_args, transparent_draw_range_args,
    transparent_indirect_args, transparent_liquid_phase_groups,
    transparent_snapshot_addresses_are_resident,
};

#[cfg(test)]
mod tests;
