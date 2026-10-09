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
#[cfg(any(test, feature = "publication-test-support"))]
use bevy::render::renderer::RenderInstance;
use bevy::{
    asset::{AssetId, load_internal_asset},
    camera::visibility::{self, VisibilityClass},
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
        extract_component::ExtractComponent,
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
            DepthStencilState, DownlevelFlags, DrawIndexedIndirectArgs, Extent3d, FragmentState,
            IndexFormat, Origin3d, PipelineCache, PrimitiveState, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, Specializer, SpecializerKey, TexelCopyBufferLayout, TexelCopyTextureInfo,
            Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
            TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension, Variants,
            VertexState, WgpuFeatures,
        },
        renderer::{RenderAdapter, RenderDevice, RenderQueue},
        settings::Backends,
        sync_world::MainEntity,
        view::{
            ExtractedView, RenderVisibleEntities, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms, window::ExtractedWindows,
        },
    },
};
use meshing::{ChunkBiomeTintIdentity, CubeQuadLayout, Face, chunk_publication_byte_len};
use render_api::{PublicationPermit, PublicationPermitStage, PublicationServiceConfig};
use world::SubChunkKey;

use crate::{
    AtmosphereFrame, ChunkMesh, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting, RuntimeStage, RuntimeStageProfiler,
    atmosphere_render::{AtmosphereGpu, install_atmosphere},
    visibility_diagnostics::{
        ActiveVisibilityFrameProbe, ExtractedCameraIdentityTracker, MAX_VISIBILITY_DIAGNOSTIC_KEYS,
        VisibilityCompletionFence, VisibilityDiagnostics, VisibilityDiagnosticsInput,
        VisibilityFrameProbe, hash_f32_words,
    },
};
use render_model::{
    ExtractedCameraIdentity, GraphicsAdapterMetadata, ModelWorkloadCount,
    ModelWorkloadMetricsSnapshot, OpaqueDrawMode, TransparentSortMetricsSnapshot,
};

mod api;
mod biome_tints;
mod bounds;
mod constants;
mod draw;
#[cfg(feature = "enhanced")]
pub(crate) mod enhanced;
mod extract;
mod gpu;
mod gpu_cull;
#[cfg(test)]
pub(crate) use gpu_cull::app_tests::noop_render_plugin;
pub(crate) use gpu_cull::{GpuCullLateLabel, TerrainPassLabel, admit_depth_sampling};
mod instance;
pub(crate) mod pipeline;
pub use pipeline::layouts::required_vertex_storage_buffers;
mod plugin;
mod presentation;
#[cfg(feature = "publication-test-support")]
mod publication_test_support;
mod queue;
mod resource_geometry;
pub use instance::ChunkRenderInstance;
mod texture_reload;
mod textures;
pub use texture_reload::ChunkTextureReload;
pub(crate) mod transparent;

use constants::{
    BIOME_TINT_SHADER_HANDLE, BIOME_WORD_BYTES, CHUNK_ORIGIN_BYTES, CHUNK_SHADER_HANDLE,
    FALLBACK_BIOME_RECORD, FALLBACK_BIOME_WORDS, GEOMETRY_STREAM_WORD_BYTES,
    INDEXED_INDIRECT_BYTES, LIQUID_SHADER_HANDLE, MODEL_SHADER_HANDLE, PACKED_LIQUID_QUAD_BYTES,
    PACKED_MODEL_DRAW_REF_BYTES, PACKED_MODEL_REF_BYTES, PACKED_QUAD_BYTES,
    PACKED_QUAD_LIGHTING_BYTES, STATIC_QUAD_INDICES,
};

#[allow(unused_imports)]
use api::{
    AcknowledgementSlot, AcknowledgementState, ChunkGpuRemovalQueue, CompletedFrameProbe,
    DEFAULT_ACKNOWLEDGEMENT_CAPACITY, DEFAULT_PRESENTED_FRAME_ACK_CAPACITY,
    DEFAULT_ZERO_BYTE_OPERATIONS_PER_FRAME, FrameCompletionEvidence, ModelWitnessFrameEvaluation,
    PendingGpuRemoval, PendingRemoval, PendingUpload, PresentedFrameGateState,
    PublicationPermitSlot, evaluate_model_witness_frame,
};
pub use api::{
    ChunkUploadAcknowledgement, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, PresentedFrameAck, PresentedFrameGate, RenderViewCohort,
    TargetRenderExpectation,
};
pub use biome_tints::{
    BiomeTint, ChunkBiomeTints, MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V,
    MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270,
};
#[allow(unused_imports)]
use biome_tints::{ChunkBiomeTintResourceIdentity, MATERIAL_UV_ROTATION_MASK};
use draw::{queue_chunks, queue_transparent_chunks};
use extract::install_chunk_extraction;
#[cfg(test)]
use gpu::arena::plan_chunk_range_update;
#[allow(unused_imports)]
use gpu::arena::{
    ArenaLimits, ChunkGpuArena, ChunkGpuUploadStats, FreshChunkRanges, GPU_UPDATE_OVERDUE_FRAMES,
    GpuUpdateCandidate, GpuUpdateFairness, MAX_GPU_UPDATE_FAIRNESS_ENTRIES,
    allocate_aligned_quad_range, allocate_aligned_range_for_update, allocate_origin,
    allocate_quad_range, allocate_range_for_update, arena_limits_from_device_limits,
    checked_geometry_range, chunk_tint_identity_is_active, commit_fresh_chunk_ranges,
    create_indirect_buffer, create_storage_buffer, init_chunk_gpu_arena, insert_free_quad_range,
    plan_fresh_chunk_ranges, plan_gpu_chunk_updates, plan_origin_allocation,
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
use gpu::graphics_metadata::{adapter_metadata_field, publish_graphics_runtime_metadata};
#[cfg(test)]
use gpu::layout::transparent_geometry_update_requires_cow;
#[allow(unused_imports)]
use gpu::layout::{
    ARENA_MIGRATION_FRAME_BYTES, ArenaGrowthError, ArenaGrowthPlan, ArenaMigration,
    ArenaRequiredLengths, ArenaStream, GeometryStreamCounts, GeometryStreamLayout,
    GpuUploadReservation, SHARED_GEOMETRY_ALIGNMENT_WORDS, account_chunk_gpu_uploads,
    advance_arena_migration, arena_capacities, begin_arena_migration, buffer_byte_len,
    checked_align_up, first_arena_growth, plan_arena_growth, write_geometry_stream_words,
};
#[allow(unused_imports)]
use gpu::types::{
    ArenaAllocation, ChunkDepthLiquidIndirectBatches, ChunkDrawMode, ChunkIndirectBatch,
    ChunkIndirectBatches, ChunkModelIndirectBatches, GpuChunkAllocation, GpuChunkOrigin,
    LEGACY_FIXED_MODEL_QUADS_PER_REF, MODEL_INDEX_COUNT, QueueFrameProbeParams,
    RetiredArenaAllocation, StreamAddresses, absolutize_liquid_lighting_indices, cube_draw_base,
    cube_lighting_record_address, cube_stream_addresses_valid, cube_stream_drawable,
    cutout_indirect_command, depth_liquid_direct_draw_command, depth_liquid_draw_command,
    depth_liquid_mdi_draw_command, diagnostic_draw_mode, direct_stream_addresses,
    extracted_camera_identity, gpu_chunk_origin, mdi_stream_addresses, metadata_base_vertex,
    model_direct_draw_command, model_draw_command, model_mdi_draw_command,
    model_ref_count_for_witness, opaque_allocation_is_drawable, select_chunk_draw_mode,
    shared_stream_ranges_disjoint, solid_indirect_commands, summarize_model_workload,
    transparent_liquid_direct_draw_command, transparent_model_direct_draw_command,
};
#[allow(unused_imports)]
use gpu::upload::{
    absolutize_model_lighting_bases, absolutize_partitioned_model_draw_refs,
    chunk_instance_upload_byte_len, liquid_quad_centroid, packed_lighting_records,
    packed_stream_range_matches, prepare_gpu_chunks, transparent_allocation_matches,
    transparent_model_allocation_matches, validate_partitioned_model_streams,
};
#[allow(unused_imports)]
use pipeline::commands::{
    DrawChunkCommands, DrawChunkIndirectCommands, DrawDepthLiquid, DrawDepthLiquidCommands,
    DrawDepthLiquidIndirectCommands, DrawDepthLiquidsIndirect, DrawModelCommands,
    DrawModelIndirectCommands, DrawPackedChunk, DrawPackedChunksIndirect, DrawPackedModel,
    DrawPackedModelsIndirect, DrawPackedTransparentModel, DrawTransparentLiquid,
    DrawTransparentLiquidCommands, DrawTransparentLiquidDirect,
    DrawTransparentLiquidDirectCommands, DrawTransparentLiquidIndirect,
    DrawTransparentLiquidIndirectCommands, DrawTransparentModelCommands, OpaqueChunkViewQuery,
    drawable_allocation_identity, front_to_back_cube_entities, indirect_batch_draw_args,
    prepare_chunk_indirect_batches, prepare_depth_liquid_indirect_batch_draws,
    prepare_indirect_batch_draws, prepare_model_indirect_batch_draws,
    record_visibility_direct_submission, record_visibility_mdi_submissions,
    sorted_visible_entities, upload_indirect_commands_if_changed,
};
use pipeline::install_chunk_commands;
#[allow(unused_imports)]
use pipeline::layouts::{ChunkPipeline, ChunkPipelineKey, ChunkPipelineSpecializer};
use plugin::ChunkEntities;
pub use plugin::{ChunkRenderApplySet, ChunkRenderPlugin};
#[allow(unused_imports)]
use presentation::frame_probe::{
    ActiveFrameProbe, ActiveFrameProbeState, ChunkStreamMask, FrameAllocationIdentity,
    FrameInstanceIdentity, FrameProbe, FrameProbeScope, build_presented_frame_ack,
    submit_presented_frame_probe,
};
pub use presentation::metrics::{ModelWorkloadMetrics, TransparentSortMetrics};
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
#[cfg(feature = "publication-test-support")]
pub use publication_test_support::{
    PublicationRenderTerminalSnapshot, publication_noop_render_plugin,
    publication_render_terminal_snapshot, settle_publication_noop_frame,
};
pub use queue::{ChunkRenderQueue, ChunkRenderQueueLimits};
#[allow(unused_imports)]
use queue::{
    DEFAULT_RENDER_QUEUE_BYTES, DEFAULT_RENDER_QUEUE_ITEMS, apply_chunk_render_queue,
    biome_record_byte_len, biome_record_is_fallback, chunk_origin, pending_upload_byte_len,
    update_chunk_animation_clock,
};
pub use textures::{
    AnimationFrameSample, ChunkAnimationClock, ChunkTextureAssetIdentity, ChunkTextureAssets,
    TextureArrayLimits, TextureLimitError, TextureMipUploadPlan, TexturePageBinding,
    TextureUploadPlanError, diagnostic_texture_page, greedy_texture_uv, plan_texture_mip_uploads,
    plan_texture_page_bindings, select_animation_frames, texture_asset_needs_rebuild,
};
#[allow(unused_imports)]
use transparent::face_metric::{FaceOrderCamera, FaceOrderClass, TransparentFaceMetric};
#[allow(unused_imports)]
use transparent::model::{
    TransparentModelAddressIdentity, TransparentModelAllocationIdentity,
    TransparentModelCandidateCache, TransparentModelSortBatch, TransparentModelSortCandidate,
    TransparentModelSortKey, TransparentModelSortRuntime, TransparentModelSortWork,
    TransparentModelStagedSort, TransparentModelWorkerResult, TransparentUploadBudget,
    clear_active_transparent_metrics, fail_closed_transparent_sort_key_error,
    prepare_transparent_model_sorts, sort_transparent_model_candidates,
    spawn_transparent_model_sort, take_transparent_model_upload_batches,
    transparent_model_draw_candidate, transparent_model_phase_distance,
    transparent_model_subchunk_center, transparent_request_to_commit_latency,
};
use transparent::planar::liquid_quads_are_order_independent;
use transparent::residents::{TransparentLiquidResidents, select_sorted_residents};
#[allow(unused_imports)]
use transparent::retirement::{
    TransparentPresentationFence, TransparentRetirementBudget, TransparentRetirementFence,
    TransparentRetirementFenceState, arm_transparent_retirements,
    record_encoded_transparent_generation, record_gpu_completed_transparent_generation,
    transparent_allocation_is_exact, transparent_resident_allocation_contains,
    transparent_retirement_can_arm, transparent_snapshot_references_allocation,
    transparent_snapshot_references_resident_allocation, transparent_view_missing_witness_keys,
};
pub use transparent::sort::{
    DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME, MAX_MODEL_WITNESS_KEYS, MAX_TRANSPARENT_DRAW_REFS,
    MAX_TRANSPARENT_VIEWS, MAX_TRANSPARENT_WITNESS_KEYS, PackedTransparentDrawRef,
    TRANSPARENT_REF_BUFFER_BYTES, TRANSPARENT_REF_SLOT_BYTES, TransparentAllocationIdentity,
    TransparentDrawArgs, TransparentOrderedSnapshot, TransparentSortError, TransparentSortJobGate,
    TransparentSortResult, TransparentSortState, TransparentUploadBatch, ViewSortGeneration,
    ViewSortKey, validate_transparent_sort_ref_count,
};
#[allow(unused_imports)]
use transparent::sort::{
    INITIAL_TRANSPARENT_SLOT_REFS, MAX_TRANSPARENT_RETIRED_ALLOCATIONS,
    MAX_TRANSPARENT_RETIRED_BYTES, TransparentGroupInput, TransparentGroups, TransparentLayoutBase,
    TransparentLiquidPhaseGroup, TransparentRefPatch, TransparentSnapshotLayout,
    TransparentSortOutput, TransparentSortRuntime, TransparentSortWork, TransparentStagedSnapshot,
    TransparentWorkerResult, build_transparent_group, changed_ref_spans, distinct_tint_count,
    ensure_transparent_ref_capacity, plan_transparent_slot, prepare_transparent_sorts, sort_group,
    spawn_transparent_sort, transparent_draw_args, transparent_draw_range_args,
    transparent_indirect_args, transparent_liquid_phase_groups, transparent_ref_buffer,
    transparent_ref_offset, transparent_snapshot_addresses_are_resident, view_displaces_water,
};
use transparent::visible_order::{VisibleWater, VisibleWaterOrder, each_visible_key};

#[cfg(test)]
mod tests;
