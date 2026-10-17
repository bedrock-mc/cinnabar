//! Loads shared shader imports and installs opted-in Enhanced passes.

use super::*;
use bevy::{asset::load_internal_asset, prelude::*};
#[cfg(feature = "enhanced")]
use render_model::enhanced_rendering_enabled;
#[cfg(feature = "enhanced")]
use {
    bevy::render::{Render, RenderApp, RenderSystems, extract_component::ExtractComponentPlugin},
    depth::EnhancedDepthPipelines,
    gpu::{EnhancedGpu, prepare_enhanced_materials, prepare_enhanced_views},
    graph::install_graph,
    post::EnhancedPostPipelines,
    shadows::EnhancedShadowPipelines,
};

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
        "common.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        SHADOW_SHADER,
        "shadow.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        SUN_SHADOW_TEMPORAL_SHADER,
        "sun_shadow_temporal.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        RADIANCE_SHADER,
        "radiance.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        WATER_SHADER,
        "water.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        ENVIRONMENT_SHADER,
        "environment.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        TEMPORAL_SHADER,
        "temporal.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        LOCAL_LIGHT_SHADER,
        "local_lights.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        ACTOR_MOTION_SHADER,
        "actor_motion.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        ATMOSPHERE_SHADER,
        "atmosphere.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        CLOUDS_SHADER,
        "clouds.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(app, AO_SHADER, "ao.wesl", crate::shader_safety::from_wesl);
    load_internal_asset!(
        app,
        INDIRECT_TRACE_SHADER,
        "indirect_trace.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        INDIRECT_SHADER,
        "indirect.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(app, PBR_SHADER, "pbr.wesl", |source, path| {
        crate::shader_safety::from_wesl(crate::material_shader::source(source), path)
    });
    load_internal_asset!(
        app,
        ENHANCED_VIEW_SHADER_HANDLE,
        "view.wesl",
        crate::shader_safety::from_wesl
    );
    load_internal_asset!(
        app,
        ENHANCED_CASTER_SHADER_HANDLE,
        "caster.wesl",
        crate::shader_safety::from_wesl
    );
}

#[cfg(not(feature = "enhanced"))]
impl Plugin for EnhancedRenderPlugin {
    fn build(&self, _app: &mut App) {}
}

#[cfg(feature = "enhanced")]
impl Plugin for EnhancedRenderPlugin {
    fn build(&self, app: &mut App) {
        if !enhanced_rendering_enabled() {
            return;
        }
        load_shader_imports(app);
        load_internal_asset!(
            app,
            LOCAL_SHADOW_HISTORY_SHADER,
            "local_shadow_history.wesl",
            crate::shader_safety::from_wesl
        );
        load_internal_asset!(
            app,
            ENHANCED_POST_SHADER_HANDLE,
            "post.wesl",
            crate::shader_safety::from_wesl
        );
        load_internal_asset!(
            app,
            EXPOSURE_SHADER,
            "exposure.wesl",
            crate::shader_safety::from_wesl
        );
        load_internal_asset!(
            app,
            PROBE_SHADER,
            "probe.wesl",
            crate::shader_safety::from_wesl
        );
        load_internal_asset!(
            app,
            PROBE_FILTER_SHADER,
            "probe_filter.wesl",
            crate::shader_safety::from_wesl
        );
        app.add_plugins(ExtractComponentPlugin::<EnhancedRendering>::default());
        load_internal_asset!(
            app,
            cloud_noise::CLOUD_NOISE_SHADER,
            "cloud_noise.wesl",
            cloud_noise::shader
        );
        load_internal_asset!(
            app,
            multiple_scattering::MULTIPLE_SCATTER_SHADER,
            "multiple_scattering.wesl",
            multiple_scattering::shader
        );
        load_internal_asset!(
            app,
            INDIRECT_COMPUTE_SHADER,
            "indirect_compute.wesl",
            crate::shader_safety::from_wesl
        );
    }

    fn finish(&self, app: &mut App) {
        if !enhanced_rendering_enabled() {
            return;
        }
        let support = app
            .get_sub_app(RenderApp)
            .and_then(|app| {
                app.world()
                    .get_resource::<bevy::render::renderer::RenderDevice>()
            })
            .map_or(EnhancedRenderSupport(false), |device| {
                EnhancedRenderSupport::for_device_limits(&device.limits())
            });
        app.insert_resource(support);
        if !support.0 {
            warn!("Enhanced rendering exceeds this device's binding limits; using Vanilla");
            return;
        }
        probes::install(app);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<EnhancedViews>()
            .init_resource::<local_lights::LocalLightSources>()
            .init_resource::<indirect::geometry::IndirectGeometry>()
            .add_systems(
                Render,
                (
                    probes::prepare_quality,
                    local_lights::collect_sources,
                    indirect::geometry::collect_geometry,
                    prepare_cloud_noise,
                    multiple_scattering::prepare,
                    prepare_enhanced_materials,
                    prepare_enhanced_views,
                )
                    .chain()
                    .in_set(RenderSystems::PrepareResources)
                    .after(bevy::render::view::prepare_view_targets)
                    .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
            );
        render_app.add_systems(
            Render,
            temporal::prepare_jitter.in_set(RenderSystems::PrepareViews),
        );
        crate::chunk::enhanced::install_geometry_cache(render_app);
        render_app
            .init_resource::<EnhancedGpu>()
            .init_resource::<cloud_noise::CloudNoiseVolume>()
            .init_resource::<multiple_scattering::MultipleScattering>()
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

/// Generates cached cloud noise once an Enhanced view needs it.
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
