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

#[cfg(feature = "enhanced")]
mod frame;
#[cfg(feature = "enhanced")]
mod gpu;
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
    bevy::{
        core_pipeline::core_3d::graph::{Core3d, Node3d},
        render::{
            Render, RenderApp, RenderSystems,
            extract_component::ExtractComponentPlugin,
            render_graph::{Node, RenderGraph, RenderLabel, ViewNodeRunner},
        },
    },
    gpu::{EnhancedGpu, prepare_enhanced_materials, prepare_enhanced_views},
    post::{EnhancedPostLabel, EnhancedPostNode, EnhancedPostPipelines},
    shadows::{EnhancedShadowLabel, EnhancedShadowNode, EnhancedShadowPipelines},
    snapshot::{EnhancedSnapshotLabel, EnhancedSnapshotNode},
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
    /// Enables the Cook-Torrance material response and linear PBR atlas layers.
    pub physically_based: bool,
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
            physically_based: true,
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

/// Orders world, Bloom and grade before the hand and UI on Enhanced views.
#[cfg(feature = "enhanced")]
fn install_graph(world: &mut World) {
    let snapshot = ViewNodeRunner::<EnhancedSnapshotNode>::new(EnhancedSnapshotNode, world);
    let shadow = ViewNodeRunner::<EnhancedShadowNode>::new(EnhancedShadowNode, world);
    let post = ViewNodeRunner::<EnhancedPostNode>::new(EnhancedPostNode, world);
    let hand = crate::viewmodel_render::enhanced_post_node(world);
    let rig = crate::hand_rig_render::enhanced_post_node(world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(EnhancedSnapshotLabel, snapshot);
    graph.add_node_edges((
        Node3d::MainOpaquePass,
        EnhancedSnapshotLabel,
        Node3d::MainTransparentPass,
    ));
    graph.add_node(EnhancedShadowLabel, shadow);
    graph.add_node_edges((EnhancedShadowLabel, Node3d::MainOpaquePass));
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
