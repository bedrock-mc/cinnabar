use crate::ui_render::DeviceObservation;
use crate::viewmodel::{
    HandVertex, ViewmodelCompletionGate, ViewmodelScene, ViewmodelToken, hand_projection,
    viewmodel_depth_bytes,
};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::graph::Core3d,
    ecs::system::{SystemChangeTick, SystemParam},
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_graph::{RenderGraph, RenderLabel, ViewNodeRunner},
        render_resource::*,
        renderer::{RenderAdapter, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::ExtractedView,
    },
};
use std::{mem::size_of, sync::Mutex};
mod gpu;
mod node;
use gpu::*;
#[cfg(test)]
mod tests;
const HAND_SHADER: Handle<Shader> = uuid_handle!("05c3d760-7ab6-4f19-b6b3-dea197927fa5");

#[derive(Debug, Clone, Copy, Default)]
pub struct ViewmodelRenderPlugin;
#[derive(Resource)]
struct Installed;
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct HandLabel;
impl Plugin for ViewmodelRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}
fn install(app: &mut App) {
    app.init_resource::<ViewmodelScene>()
        .init_resource::<ViewmodelCompletionGate>();
    crate::pipeline_warmup::register::<HandGpu>(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_hand_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    let gate = app.world().resource::<ViewmodelCompletionGate>().clone();
    app.add_plugins(ExtractResourcePlugin::<ViewmodelScene>::default());
    load_internal_asset!(
        app,
        HAND_SHADER,
        "viewmodel.wgsl",
        crate::shader_safety::from_wgsl
    );
    let render_app = app.sub_app_mut(RenderApp);
    crate::device_poll::install(render_app);
    render_app
        .insert_resource(Installed)
        .insert_resource(gate)
        .init_resource::<HandDrawn>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareResources),
                submit_completion.in_set(crate::device_poll::FrameSubmissions),
            ),
        );
    install_hand_graph(render_app.world_mut());
}

/// The hand pass Enhanced views run after Bloom and grading.
#[cfg(feature = "enhanced")]
pub(crate) fn enhanced_post_node(world: &mut World) -> impl bevy::render::render_graph::Node {
    ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, true>(node::HandViewNode),
        world,
    )
}

pub(crate) fn install_hand_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() {
        return;
    }
    let runner = ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, false>(node::HandViewNode),
        world,
    );
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    if graph
        .get_node_state(crate::ui_render::UiOverlayLabel)
        .is_err()
    {
        return;
    }
    if graph.get_node_state(HandLabel).is_err() {
        graph.add_node(HandLabel, runner);
    }
    // The hand retains world colour samples until the final resolve before the HUD.
    graph.add_node_edges((
        crate::ui_render::UiWorldLabel,
        HandLabel,
        bevy::core_pipeline::core_3d::graph::Node3d::EndMainPass,
    ));
    if graph
        .get_node_state(crate::hand_rig_render::HandRigLabel)
        .is_ok()
    {
        let _ = graph.try_add_node_edge(HandLabel, crate::hand_rig_render::HandRigLabel);
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for HandGpu {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let layout = &self.layout;
        let samples = view.msaa.samples();
        let id = memoized_hand_pipeline(&mut self.pipeline_variants, samples, view.hdr, || {
            cache.queue_render_pipeline(specialized_hand_pipeline(
                layout.clone(),
                samples,
                view.hdr,
            ))
        })
        .ok_or("unsupported hand pipeline sample count")?;
        ids.push(id);
        Ok(())
    }
}
