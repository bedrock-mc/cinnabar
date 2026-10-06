//! Opt-in enhanced passes preserve the ordinary path for unmarked cameras.
//! Passes require `enhanced`; the camera component and shared shader imports do not.

#[cfg(feature = "enhanced")]
mod frame;
#[cfg(feature = "enhanced")]
mod gpu;
#[cfg(feature = "enhanced")]
pub(crate) mod graph;
#[cfg(all(test, feature = "enhanced"))]
mod graph_tests;
#[cfg(feature = "enhanced")]
mod materials;
#[cfg(feature = "enhanced")]
mod post;
#[cfg(feature = "enhanced")]
mod shadows;
#[cfg(feature = "enhanced")]
mod snapshot;
#[cfg(all(test, feature = "enhanced"))]
mod validation;

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
    bevy::render::{Render, RenderApp, RenderSystems, extract_component::ExtractComponentPlugin},
    gpu::{EnhancedGpu, prepare_enhanced_materials, prepare_enhanced_views},
    graph::install_graph,
    post::EnhancedPostPipelines,
    shadows::EnhancedShadowPipelines,
};

#[cfg(feature = "enhanced")]
pub(crate) use frame::CascadeBounds;
#[cfg(feature = "enhanced")]
pub(crate) use gpu::{EnhancedViews, SetEnhancedViewBindGroup, enhanced_view_layout};

const ENHANCED_COMMON_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("0f5b3a57-5d7e-4a3e-9d0e-2b1f6c8a4e11");
const ENHANCED_VIEW_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("6a2d9c41-8e3b-4f7a-b1c5-3d9e0f2a7b62");
const ENHANCED_CASTER_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("c4e81f23-7a9d-4b6e-8f10-5a3c2d1e9b73");
#[cfg(feature = "enhanced")]
const ENHANCED_POST_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("9b7e2c15-3f4a-4d8b-a6e2-7c1d0f5b3a84");

/// Per-camera opt-in for Enhanced rendering. The plugin enforces [`Msaa::Off`]
/// before extraction because its depth copies and sampling require single-sample textures.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, PartialEq)]
#[require(Msaa::Off)]
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
}

pub const MAX_SHADOW_CASCADES: u32 = 3;

impl Default for EnhancedRendering {
    fn default() -> Self {
        Self {
            shadows: true,
            shadow_resolution: 1024,
            shadow_cascades: 2,
            shadow_distance: 96.0,
            bloom: true,
            light_shafts: true,
            waving: true,
            water_reflections: true,
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
            ENHANCED_POST_SHADER_HANDLE,
            "post.wgsl",
            crate::shader_safety::from_wgsl
        );
        app.add_plugins(ExtractComponentPlugin::<EnhancedRendering>::default());
        app.add_systems(Last, enforce_single_sample_depth);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<EnhancedViews>().add_systems(
            Render,
            (prepare_enhanced_materials, prepare_enhanced_views)
                .chain()
                .in_set(RenderSystems::PrepareResources),
        );
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
            .init_resource::<EnhancedPostPipelines>()
            .init_resource::<EnhancedShadowPipelines>();
        install_graph(render_app.world_mut());
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
