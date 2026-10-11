//! Opt-in enhanced passes preserve the ordinary path for unmarked cameras.
//! Passes require `enhanced`; the camera component and shared shader imports do not.

#[cfg(feature = "enhanced")]
use bevy::prelude::IntoScheduleConfigs;
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
use render_model::enhanced_rendering_enabled;
#[cfg(feature = "enhanced")]
use render_scene::EnhancedRendering;

#[cfg(all(test, feature = "enhanced"))]
use bevy::prelude::{Has, World};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::{App, Assets, Handle, Plugin},
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
        "common.wesl",
        crate::shader_safety::from_wesl
    );
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
        if !enhanced_rendering_enabled() {
            return;
        }
        load_shader_imports(app);
        load_internal_asset!(
            app,
            ENHANCED_POST_SHADER_HANDLE,
            "post.wesl",
            crate::shader_safety::from_wesl
        );
        app.add_plugins(ExtractComponentPlugin::<EnhancedRendering>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<EnhancedViews>().add_systems(
            Render,
            (prepare_enhanced_materials, prepare_enhanced_views)
                .chain()
                .in_set(RenderSystems::PrepareResources)
                .after(bevy::render::view::prepare_view_targets)
                .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
        );
    }

    fn finish(&self, app: &mut App) {
        if !enhanced_rendering_enabled() {
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
