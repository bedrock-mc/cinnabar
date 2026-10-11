//! Packed chunk meshing and Bevy rendering for the Bedrock client.
mod alpha_coverage;
mod depth_smaa;
#[cfg(test)]
#[path = "../tests/it/support/gpu_snapshot.rs"]
mod gpu_snapshot;
pub use depth_smaa::DepthSmaaPlugin;
mod lighting;

#[cfg(test)]
mod render_test_support;
#[cfg(test)]
mod shader_test_support;

mod aim_assist;
pub use aim_assist::AimAssistHighlightPlugin;
mod actor;
mod actor_render;
#[cfg(test)]
mod alloc_count;

mod atmosphere_render;
mod block_entity;

mod chunk;

mod cloud_render;
mod device_poll;

mod enhanced;
mod entity_shadow_render;
pub use enhanced::EnhancedRenderPlugin;
pub use entity_shadow_render::EntityShadowRenderPlugin;
mod gpu_timing;
pub use gpu_timing::{DetailedGpuTiming, GpuTimingPlugin};

mod dropped_item_render;
mod hand_rig_render;
mod input_pacing;
pub use input_pacing::{FramePacing, FrameStart, InputPacingPlugin};

mod lightning_render;
mod media;
pub use media::MediaTexture;
mod media_screen;

mod material_shader;
#[cfg(test)]
#[path = "material_shader/shader_tests.rs"]
mod material_shader_shader_tests;
#[cfg(test)]
#[path = "material_shader/tests.rs"]
mod material_shader_tests;
mod mod_render;

pub use mod_render::{ModPassLabel, ModRenderPlugin};
pub mod motion_blur;
mod nametag_render;

mod pack_skybox;

mod panorama_render;
mod particle_render;

mod pipeline_warmup;
#[cfg(test)]
#[path = "pipeline_warmup/app_tests.rs"]
mod pipeline_warmup_app_tests;

mod present_mode;
mod primitive_shapes;
pub use primitive_shapes::PrimitiveShapesRenderPlugin;
mod render_bounds;

mod runtime_profile_phases;

mod scene_sampling;
mod scene_target;

mod screen_overlay_render;
mod shader_safety;
#[cfg(test)]
#[path = "../tests/it/support/shader_source.rs"]
mod shader_source;
mod surface_capabilities;
mod surface_lifecycle;
mod transparent_phase;
mod ui_render;
#[cfg(all(test, target_os = "macos"))]
mod upload_allocation_tests;
mod upload_staging;

mod viewmodel_render;

pub use hand_rig_render::HandRigRenderPlugin;
pub use particle_render::ParticleRenderPlugin;

pub use viewmodel_render::ViewmodelRenderPlugin;

mod weather_render;

use meshing::{
    PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad, PackedQuadLighting,
};

pub use actor::material::blend_state as actor_material_blend_state;
pub use actor_render::ActorRenderPlugin;

pub use atmosphere_render::AtmospherePlugin;
pub use block_entity::{BlockEntityRenderPlugin, DRAGON_DEATH_BLEND};

pub use chunk::required_vertex_storage_buffers;
pub use chunk::{
    ChunkRenderPlugin, ChunkTextureUploadStats, DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
    MAX_TRANSPARENT_DRAW_REFS, MAX_TRANSPARENT_VIEWS, PackedTransparentDrawRef,
    TRANSPARENT_REF_BUFFER_BYTES, TRANSPARENT_REF_SLOT_BYTES, TransparentAllocationIdentity,
    TransparentDrawArgs, TransparentOrderedSnapshot, TransparentSortError, TransparentSortJobGate,
    TransparentSortResult, TransparentSortState, TransparentUploadBatch, ViewSortGeneration,
    ViewSortKey, validate_transparent_sort_ref_count,
};
#[cfg(feature = "publication-test-support")]
pub use chunk::{
    PublicationRenderTerminalSnapshot, publication_noop_render_plugin,
    publication_render_terminal_snapshot, settle_publication_noop_frame,
};

pub use dropped_item_render::DroppedItemRenderPlugin;

pub use panorama_render::PanoramaRenderPlugin;
pub use present_mode::{
    PresentModePolicy, PresentModePolicyPlugin, PresentModePreference, PresentModeRemedy,
    frame_latency_for_vsync, requested_present_mode_kind, resolve_dx12_present_mode_remedy,
    window_present_mode,
};

pub use runtime_profile_phases::install_main_phase_spans;

pub use screen_overlay_render::ScreenOverlayRenderPlugin;
pub use ui_render::UiRenderPlugin;

mod opaque_phase;
pub(crate) use opaque_phase::install_opaque_phase_reset;
mod stars;

#[cfg(test)]
mod queue_review_support;

mod runtime_profile_trace;
