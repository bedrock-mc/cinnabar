//! Opt-in "Enhanced" world rendering: a custom, non-parity look that never
//! replaces or alters the vanilla path. Cameras without [`EnhancedRendering`]
//! keep byte-identical vanilla pipelines and never run these passes.
//!
//! Techniques are standard published ones implemented from scratch: stable
//! cascaded shadow maps with PCF, Bevy bloom, ray-marched
//! shadow-map light shafts, screen-space reflections, and a filmic shoulder.
//!
//! The passes compile only with the `enhanced` feature; the component, kill switch
//! and the shader imports vanilla variants resolve are always present.

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
#[cfg(all(test, feature = "enhanced"))]
mod graph_tests;
#[cfg(feature = "enhanced")]
mod indirect;
#[cfg(feature = "enhanced")]
mod local_lights;
#[cfg(feature = "enhanced")]
mod local_shadow_history;
#[cfg(feature = "enhanced")]
mod materials;
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
mod targets;
#[cfg(feature = "enhanced")]
mod temporal;
#[cfg(all(test, feature = "enhanced"))]
mod temporal_texture_tests;
#[cfg(all(test, feature = "enhanced"))]
mod validation;
#[cfg(all(test, feature = "enhanced"))]
mod water_tests;

#[cfg(feature = "enhanced")]
use render_model::ENHANCED_RENDERING_ENABLED;

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::extract_component::ExtractComponent,
    shader::Shader,
};
#[cfg(feature = "enhanced")]
use {
    bevy::{
        core_pipeline::core_3d::graph::{Core3d, Node3d},
        render::{
            Render, RenderApp, RenderSystems,
            extract_component::ExtractComponentPlugin,
            render_graph::{Node, RenderGraph, RenderLabel, ViewNodeRunner},
        },
    },
    depth::{EnhancedDepthLabel, EnhancedDepthNode, EnhancedDepthPipelines},
    gpu::{EnhancedGpu, prepare_enhanced_materials, prepare_enhanced_views},
    post::{
        EnhancedLightingLabel, EnhancedLightingNode, EnhancedPostLabel, EnhancedPostNode,
        EnhancedPostPipelines, EnhancedSkyLabel, EnhancedSkyNode,
    },
    shadows::{EnhancedShadowLabel, EnhancedShadowNode, EnhancedShadowPipelines},
    snapshot::{EnhancedSnapshotLabel, EnhancedSnapshotNode},
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

/// Per-camera opt-in for Enhanced rendering. The plugin enforces [`Msaa::Off`]
/// before extraction because its depth copies and sampling require single-sample textures.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, PartialEq)]
#[require(Msaa::Off, bevy::render::camera::TemporalJitter)]
pub struct EnhancedRendering {
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

impl Default for EnhancedRendering {
    fn default() -> Self {
        Self {
            shadows: true,
            shadow_resolution: 1024,
            shadow_cascades: 3,
            shadow_distance: 96.0,
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

#[derive(Debug, Clone, Copy, Default)]
pub struct EnhancedRenderPlugin;

/// Registers source imports that Bevy resolves even for vanilla shader variants.
/// This creates no Enhanced GPU pipelines or render passes.
pub(crate) fn load_shader_imports(app: &mut App) {
    if app
        .world()
        .resource::<Assets<Shader>>()
        .contains(&ENHANCED_COMMON_SHADER_HANDLE)
    {
        return;
    }
    load_internal_asset!(
        app,
        ENHANCED_COMMON_SHADER_HANDLE,
        "common.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        SHADOW_SHADER,
        "shadow.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        RADIANCE_SHADER,
        "radiance.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        WATER_SHADER,
        "water.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        ENVIRONMENT_SHADER,
        "environment.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        TEMPORAL_SHADER,
        "temporal.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        LOCAL_LIGHT_SHADER,
        "local_lights.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        ACTOR_MOTION_SHADER,
        "actor_motion.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        ATMOSPHERE_SHADER,
        "atmosphere.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        CLOUDS_SHADER,
        "clouds.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(app, AO_SHADER, "ao.wgsl", crate::shader_safety::from_wgsl);
    load_internal_asset!(
        app,
        INDIRECT_TRACE_SHADER,
        "indirect_trace.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        INDIRECT_SHADER,
        "indirect.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(app, PBR_SHADER, "pbr.wgsl", crate::material_shader::shader);
    load_internal_asset!(
        app,
        ENHANCED_VIEW_SHADER_HANDLE,
        "view.wgsl",
        crate::shader_safety::from_wgsl
    );
    load_internal_asset!(
        app,
        ENHANCED_CASTER_SHADER_HANDLE,
        "caster.wgsl",
        crate::shader_safety::from_wgsl
    );
}

#[cfg(not(feature = "enhanced"))]
impl Plugin for EnhancedRenderPlugin {
    fn build(&self, _app: &mut App) {}
}

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

#[cfg(feature = "enhanced")]
impl Plugin for EnhancedRenderPlugin {
    fn build(&self, app: &mut App) {
        if !ENHANCED_RENDERING_ENABLED {
            return;
        }
        load_shader_imports(app);
        load_internal_asset!(
            app,
            LOCAL_SHADOW_HISTORY_SHADER,
            "local_shadow_history.wgsl",
            crate::shader_safety::from_wgsl
        );
        load_internal_asset!(
            app,
            ENHANCED_POST_SHADER_HANDLE,
            "post.wgsl",
            crate::shader_safety::from_wgsl
        );
        load_internal_asset!(
            app,
            EXPOSURE_SHADER,
            "exposure.wgsl",
            crate::shader_safety::from_wgsl
        );
        load_internal_asset!(
            app,
            PROBE_SHADER,
            "probe.wgsl",
            crate::shader_safety::from_wgsl
        );
        load_internal_asset!(
            app,
            PROBE_FILTER_SHADER,
            "probe_filter.wgsl",
            crate::shader_safety::from_wgsl
        );
        app.add_plugins(ExtractComponentPlugin::<EnhancedRendering>::default());
        load_internal_asset!(
            app,
            cloud_noise::CLOUD_NOISE_SHADER,
            "cloud_noise.wgsl",
            cloud_noise::shader
        );
        load_internal_asset!(
            app,
            INDIRECT_COMPUTE_SHADER,
            "indirect_compute.wgsl",
            crate::shader_safety::from_wgsl
        );
        probes::install(app);
        app.add_systems(Last, enforce_single_sample_depth);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<EnhancedViews>()
            .init_resource::<local_lights::LocalLightSources>()
            .init_resource::<indirect::IndirectGeometry>()
            .add_systems(
                Render,
                (
                    local_lights::collect_sources,
                    indirect::collect_geometry,
                    prepare_cloud_noise,
                    prepare_enhanced_materials,
                    prepare_enhanced_views,
                )
                    .chain()
                    .in_set(RenderSystems::PrepareResources),
            );
        render_app.add_systems(
            Render,
            temporal::prepare_jitter.in_set(RenderSystems::ManageViews),
        );
        crate::chunk::enhanced::install_geometry_cache(render_app);
    }

    fn finish(&self, app: &mut App) {
        if !ENHANCED_RENDERING_ENABLED {
            return;
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<EnhancedGpu>()
            .init_resource::<cloud_noise::CloudNoiseVolume>()
            .init_resource::<indirect::IndirectPipelines>()
            .init_resource::<EnhancedPostPipelines>()
            .init_resource::<EnhancedShadowPipelines>()
            .init_resource::<EnhancedDepthPipelines>();
        render_app
            .init_resource::<exposure::ExposurePipeline>()
            .init_resource::<probes::ProbeGpu>()
            .init_resource::<probes::ProbePipelines>();
        render_app.init_resource::<local_shadow_history::LocalShadowPipeline>();
        install_graph(render_app.world_mut());
    }
}

#[cfg(feature = "enhanced")]
fn prepare_cloud_noise(
    views: Query<&EnhancedRendering>,
    noise: Res<cloud_noise::CloudNoiseVolume>,
    cache: Res<bevy::render::render_resource::PipelineCache>,
    device: Res<bevy::render::renderer::RenderDevice>,
    queue: Res<bevy::render::renderer::RenderQueue>,
) {
    if noise.ready() || views.is_empty() {
        return;
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    if noise.encode_once(&cache, &mut encoder) {
        queue.submit([encoder.finish()]);
    }
}

/// Keeps runtime MSAA changes from reaching the single-sample depth passes.
#[cfg(feature = "enhanced")]
fn enforce_single_sample_depth(mut cameras: Query<&mut Msaa, With<EnhancedRendering>>) {
    for mut msaa in &mut cameras {
        if *msaa != Msaa::Off {
            *msaa = Msaa::Off;
        }
    }
}

/// Orders world, Bloom and grade before the hand and UI on Enhanced views.
#[cfg(feature = "enhanced")]
fn install_graph(world: &mut World) {
    let snapshot = ViewNodeRunner::<EnhancedSnapshotNode>::new(EnhancedSnapshotNode, world);
    let shadow = ViewNodeRunner::<EnhancedShadowNode>::new(EnhancedShadowNode, world);
    let depth = ViewNodeRunner::<EnhancedDepthNode>::new(EnhancedDepthNode, world);
    let lighting = ViewNodeRunner::<EnhancedLightingNode>::new(EnhancedLightingNode, world);
    let post = ViewNodeRunner::<EnhancedPostNode>::new(EnhancedPostNode, world);
    let sky = ViewNodeRunner::<EnhancedSkyNode>::new(EnhancedSkyNode, world);
    let hand = crate::viewmodel_render::enhanced_post_node(world);
    let rig = crate::hand_rig_render::enhanced_post_node(world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(EnhancedSnapshotLabel, snapshot);
    graph.add_node(EnhancedSkyLabel, sky);
    graph.add_node_edges((
        Node3d::MainOpaquePass,
        EnhancedSkyLabel,
        EnhancedSnapshotLabel,
        Node3d::MainTransparentPass,
    ));
    graph.add_node(EnhancedShadowLabel, shadow);
    graph.add_node(EnhancedDepthLabel, depth);
    graph.add_node(EnhancedLightingLabel, lighting);
    graph.add_node_edges((
        Node3d::StartMainPass,
        EnhancedShadowLabel,
        EnhancedDepthLabel,
        EnhancedLightingLabel,
        Node3d::MainOpaquePass,
    ));
    graph.add_node(EnhancedPostLabel, post);
    // World -> Bloom -> grade -> hand and UI; Bloom stays in post-processing, where moving it
    // before EndMainPass would close a cycle through MotionBlur/Taa.
    graph.add_node_edges((
        Node3d::StartMainPassPostProcessing,
        EnhancedPostLabel,
        Node3d::Tonemapping,
    ));
    let _ = graph.try_add_node_edge(Node3d::Bloom, EnhancedPostLabel);
    let hand = add_post_node(
        graph,
        crate::viewmodel_render::HandLabel,
        EnhancedHandLabel,
        hand,
    );
    let rig = add_post_node(
        graph,
        crate::hand_rig_render::HandRigLabel,
        EnhancedHandRigLabel,
        rig,
    );
    let overlay = crate::ui_render::overlay::UiOverlayPostLabel.intern();
    if graph.get_node_state(overlay).is_ok() {
        graph.add_node_edges((EnhancedPostLabel, overlay, Node3d::Tonemapping));
        if hand {
            graph.add_node_edge(EnhancedHandLabel, overlay);
        }
        if rig {
            graph.add_node_edge(EnhancedHandRigLabel, overlay);
        }
    }
}

/// Adds the post-grade twin of an installed main-pass node; `false` when that pass is absent.
#[cfg(feature = "enhanced")]
fn add_post_node(
    graph: &mut RenderGraph,
    main: impl RenderLabel,
    post: impl RenderLabel + Clone,
    node: impl Node,
) -> bool {
    if graph.get_node_state(main).is_err() {
        return false;
    }
    graph.add_node(post.clone(), node);
    graph.add_node_edges((EnhancedPostLabel, post, Node3d::Tonemapping));
    true
}

#[cfg(feature = "enhanced")]
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct EnhancedHandLabel;

#[cfg(feature = "enhanced")]
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct EnhancedHandRigLabel;
