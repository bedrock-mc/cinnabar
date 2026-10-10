//! Opt-in enhanced passes preserve the ordinary path for unmarked cameras.
//! Passes require `enhanced`; the camera component and shared shader imports do not.

#[cfg(all(test, feature = "enhanced"))]
mod ao_tests;
#[cfg(feature = "enhanced")]
mod atmosphere_cache;
#[cfg(all(test, feature = "enhanced"))]
mod atmosphere_tests;
#[cfg(all(test, feature = "enhanced"))]
mod cache_validation;
#[cfg(all(feature = "enhanced", target_os = "windows"))]
mod compiler;
#[cfg(all(feature = "enhanced", target_os = "windows"))]
pub use compiler::configure_enhanced_shader_compiler;
#[cfg(feature = "enhanced")]
pub(crate) mod actor_motion;
#[cfg(all(test, feature = "enhanced"))]
mod cloud_fixture;
#[cfg(feature = "enhanced")]
mod cloud_noise;
#[cfg(all(test, feature = "enhanced"))]
mod cloud_noise_tests;
#[cfg(feature = "enhanced")]
mod depth;
#[cfg(all(test, feature = "enhanced"))]
mod depth_tests;
#[cfg(feature = "enhanced")]
mod exposure;
#[cfg(feature = "enhanced")]
mod frame;
#[cfg(feature = "enhanced")]
mod gpu;
#[cfg(feature = "enhanced")]
pub(crate) mod graph;
#[cfg(all(test, feature = "enhanced"))]
mod graph_tests;
#[cfg(feature = "enhanced")]
mod hand_layer;
#[cfg(feature = "enhanced")]
mod indirect;
#[cfg(feature = "enhanced")]
pub(crate) mod local_lights;
#[cfg(feature = "enhanced")]
mod local_shadow_history;
#[cfg(feature = "enhanced")]
mod materials;
#[cfg(feature = "enhanced")]
mod multiple_scattering;
pub(crate) mod plugin;
mod quality;
pub(crate) use quality::EnhancedQualityBudget;
pub use render_api::EnhancedQuality;
#[cfg(all(test, feature = "enhanced"))]
mod pbr_tests;
#[cfg(feature = "enhanced")]
mod post;
#[cfg(all(test, feature = "enhanced"))]
mod post_regressions;
#[cfg(feature = "enhanced")]
mod probes;
#[cfg(all(test, feature = "enhanced"))]
mod reflection_filter_tests;
#[cfg(all(test, feature = "enhanced"))]
mod reflection_tests;
#[cfg(all(test, feature = "enhanced"))]
mod shadow_tests;
#[cfg(feature = "enhanced")]
mod shadows;
#[cfg(feature = "enhanced")]
mod snapshot;
#[cfg(feature = "enhanced")]
mod sun_shadow_history;
#[cfg(feature = "enhanced")]
mod targets;
#[cfg(feature = "enhanced")]
mod temporal;
#[cfg(all(test, feature = "enhanced"))]
mod temporal_texture_tests;
#[cfg(all(test, feature = "enhanced"))]
mod validation;
#[cfg(all(test, feature = "enhanced"))]
mod water_tests;

use bevy::{
    asset::uuid_handle, prelude::*, render::extract_component::ExtractComponent, shader::Shader,
};

#[cfg(feature = "enhanced")]
pub(crate) use frame::CascadeBounds;
#[cfg(all(test, feature = "enhanced"))]
pub(crate) use gpu::enhanced_caster_layout;
#[cfg(feature = "enhanced")]
pub(crate) use gpu::{EnhancedViews, SetEnhancedViewBindGroup, enhanced_view_layout};
#[cfg(feature = "enhanced")]
pub(crate) use shadows::shadow_raster_bias;

const ENHANCED_COMMON_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("0f5b3a57-5d7e-4a3e-9d0e-2b1f6c8a4e11");
const ENHANCED_VIEW_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("6a2d9c41-8e3b-4f7a-b1c5-3d9e0f2a7b62");
const ENHANCED_CASTER_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("c4e81f23-7a9d-4b6e-8f10-5a3c2d1e9b73");
const SHADOW_SHADER: Handle<Shader> = uuid_handle!("a899424b-ce44-4087-af6b-b267fdcf6967");
const SUN_SHADOW_TEMPORAL_SHADER: Handle<Shader> =
    uuid_handle!("3c89a6de-826f-45d4-a9f5-737e768793b4");
const RADIANCE_SHADER: Handle<Shader> = uuid_handle!("e4a3a72c-cfd4-490a-bdae-1b780573965a");
const WATER_SHADER: Handle<Shader> = uuid_handle!("aec26144-3c2c-4b2e-a2e5-0518527c3030");
const ENVIRONMENT_SHADER: Handle<Shader> = uuid_handle!("8ffec817-4d44-4ec2-b78c-a187a067bef4");
const TEMPORAL_SHADER: Handle<Shader> = uuid_handle!("a5986b32-f3cf-47cc-b93f-a4330d0ae964");
const LOCAL_LIGHT_SHADER: Handle<Shader> = uuid_handle!("259e73e1-b9bd-4c75-aa2b-ece0e2e6a889");
#[cfg(feature = "enhanced")]
const LOCAL_SHADOW_HISTORY_SHADER: Handle<Shader> =
    uuid_handle!("a8143a34-f3a8-40b9-afb8-d916f875e8b1");
const INDIRECT_SHADER: Handle<Shader> = uuid_handle!("07f7a3b0-a370-4c03-a8f7-008fa5b4c011");
const INDIRECT_TRACE_SHADER: Handle<Shader> = uuid_handle!("07f7a3b0-a370-4c03-a8f7-008fa5b4c012");
#[cfg(feature = "enhanced")]
const INDIRECT_COMPUTE_SHADER: Handle<Shader> =
    uuid_handle!("07f7a3b0-a370-4c03-a8f7-008fa5b4c013");
const PBR_SHADER: Handle<Shader> = uuid_handle!("07f7a3b0-a370-4c03-a8f7-008fa5b4c014");
const ACTOR_MOTION_SHADER: Handle<Shader> = uuid_handle!("8ba8b5b1-4e35-45d5-95af-f90a86d38b2c");
#[cfg(feature = "enhanced")]
const PROBE_FILTER_SHADER: Handle<Shader> = uuid_handle!("a87683a8-486a-4511-a8ba-c94ca6fdb565");
const ATMOSPHERE_SHADER: Handle<Shader> = uuid_handle!("4a37ec08-5f88-40b9-8ad1-ef4f05d10001");
const CLOUDS_SHADER: Handle<Shader> = uuid_handle!("4a37ec08-5f88-40b9-8ad1-ef4f05d10002");
const AO_SHADER: Handle<Shader> = uuid_handle!("4a37ec08-5f88-40b9-8ad1-ef4f05d10003");
#[cfg(feature = "enhanced")]
const EXPOSURE_SHADER: Handle<Shader> = uuid_handle!("4a37ec08-5f88-40b9-8ad1-ef4f05d10004");
#[cfg(feature = "enhanced")]
const PROBE_SHADER: Handle<Shader> = uuid_handle!("4a37ec08-5f88-40b9-8ad1-ef4f05d10005");
#[cfg(feature = "enhanced")]
const ENHANCED_POST_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("9b7e2c15-3f4a-4d8b-a6e2-7c1d0f5b3a84");

/// Whether this device can bind every group in the Enhanced world pipeline.
#[derive(Resource, Clone, Copy, Debug)]
pub struct EnhancedRenderSupport(pub bool);

impl EnhancedRenderSupport {
    /// Checks the device limits before Enhanced pipelines or cameras are created.
    pub fn for_device_limits(limits: &wgpu::Limits) -> Self {
        #[cfg(feature = "enhanced")]
        {
            use bevy::render::render_resource::BindingType;
            let layout = gpu::enhanced_view_layout();
            let textures = layout
                .entries
                .iter()
                .filter(|entry| matches!(entry.ty, BindingType::Texture { .. }))
                .count() as u32;
            let samplers = layout
                .entries
                .iter()
                .filter(|entry| matches!(entry.ty, BindingType::Sampler(_)))
                .count() as u32;
            Self(
                crate::material_shader::chunk_atlas_views_fit(limits)
                    && limits.max_sampled_textures_per_shader_stage
                        >= crate::material_shader::CHUNK_SAMPLED_TEXTURE_BINDINGS + textures
                    && limits.max_samplers_per_shader_stage
                        >= crate::material_shader::CHUNK_SAMPLER_COUNT + samplers,
            )
        }
        #[cfg(not(feature = "enhanced"))]
        {
            let _ = limits;
            Self(false)
        }
    }
}

/// Per-camera opt-in for Enhanced rendering, with resolved depth for post effects.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, PartialEq)]
#[require(Msaa::Off, bevy::render::camera::TemporalJitter)]
pub struct EnhancedRendering {
    pub quality: EnhancedQuality,
    pub shadows: bool,
    pub shadow_resolution: u32,
    /// Sun shadow cascades, clamped to `2..=MAX_SHADOW_CASCADES`.
    pub shadow_cascades: u32,
    /// Blocks from the camera covered by the last cascade.
    pub shadow_distance: f32,
    pub bloom: bool,
    pub light_shafts: bool,
    pub waving: bool,
    pub water_reflections: bool,
    /// Enables the Cook-Torrance material response and linear PBR atlas layers.
    pub physically_based: bool,
    /// Enables depth-based ambient visibility and short sun contact shadows.
    pub ssao: bool,
    /// Enables the procedural 3D cloud layer composited over sky pixels.
    pub volumetric_clouds: bool,
    /// Enables reprojection, clipping, and disocclusion rejection of world history.
    pub temporal_aa: bool,
    /// Internal reflection views never sample their own captures or run post effects.
    pub reflection_capture: bool,
    /// Local diagnostic output; never changes the server or vanilla views.
    pub shadow_debug: EnhancedShadowDebug,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum EnhancedShadowDebug {
    #[default]
    Off = 0,
    Cascades = 1,
    Visibility = 2,
    DepthNear = 3,
    DepthMiddle = 4,
    DepthFar = 5,
}

pub const MAX_SHADOW_CASCADES: u32 = 3;
/// Smallest shadow target admitted by frame preparation.
#[cfg(feature = "enhanced")]
pub(crate) const MIN_SHADOW_RESOLUTION: u32 = 256;

impl Default for EnhancedRendering {
    fn default() -> Self {
        let quality = EnhancedQuality::default();
        let (shadow_resolution, shadow_distance) = quality.shadow_settings();
        Self {
            quality,
            shadows: true,
            shadow_resolution,
            shadow_cascades: MAX_SHADOW_CASCADES,
            shadow_distance,
            bloom: true,
            light_shafts: true,
            waving: true,
            water_reflections: true,
            physically_based: true,
            ssao: true,
            volumetric_clouds: true,
            temporal_aa: true,
            reflection_capture: false,
            shadow_debug: EnhancedShadowDebug::Off,
        }
    }
}

/// Maximum shadow reach admitted for hidden development captures.
#[cfg(feature = "enhanced-diagnostics")]
const DIAGNOSTIC_SHADOW_DISTANCE: f32 = 24.0;

#[cfg(feature = "enhanced-diagnostics")]
impl EnhancedRendering {
    /// Bounds the actual Enhanced passes for hidden development captures.
    #[must_use]
    pub fn bounded_diagnostic(self) -> Self {
        Self {
            shadow_resolution: MIN_SHADOW_RESOLUTION,
            shadow_distance: DIAGNOSTIC_SHADOW_DISTANCE,
            waving: false,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EnhancedRenderPlugin;

/// Binds nothing: the Enhanced view group exists only with the `enhanced` feature.
#[cfg(not(feature = "enhanced"))]
pub(crate) struct SetEnhancedViewBindGroup<const I: usize>;

#[cfg(not(feature = "enhanced"))]
impl<P: bevy::render::render_phase::PhaseItem, const I: usize>
    bevy::render::render_phase::RenderCommand<P> for SetEnhancedViewBindGroup<I>
{
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: (),
        _entity: Option<()>,
        _param: (),
        _pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
    ) -> bevy::render::render_phase::RenderCommandResult {
        bevy::render::render_phase::RenderCommandResult::Success
    }
}
