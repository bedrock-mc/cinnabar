use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::render_graph::{InternedRenderLabel, Node, RenderGraph, RenderLabel, ViewNodeRunner},
};

use super::{
    depth::{EnhancedDepthLabel, EnhancedDepthNode},
    hand_layer::{EnhancedHandCompositeLabel, EnhancedHandCompositeNode},
    post::{
        EnhancedLightingLabel, EnhancedLightingNode, EnhancedPostLabel, EnhancedPostNode,
        EnhancedSkyLabel, EnhancedSkyNode,
    },
    shadows::{EnhancedShadowLabel, EnhancedShadowNode},
    snapshot::{EnhancedSnapshotLabel, EnhancedSnapshotNode},
};

/// Orders world, Bloom and grade before the hand and UI on Enhanced views.
pub(super) fn install_graph(world: &mut World) {
    let snapshot = ViewNodeRunner::<EnhancedSnapshotNode>::new(EnhancedSnapshotNode, world);
    let shadow = ViewNodeRunner::<EnhancedShadowNode>::new(EnhancedShadowNode, world);
    let depth = ViewNodeRunner::<EnhancedDepthNode>::new(EnhancedDepthNode, world);
    let lighting = ViewNodeRunner::<EnhancedLightingNode>::new(EnhancedLightingNode, world);
    let sky = ViewNodeRunner::<EnhancedSkyNode>::new(EnhancedSkyNode, world);
    let post = ViewNodeRunner::<EnhancedPostNode>::new(EnhancedPostNode, world);
    let composite = ViewNodeRunner::new(EnhancedHandCompositeNode, world);
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
    for predecessor in [
        crate::chunk::GpuCullLateLabel.intern(),
        crate::entity_shadow_render::EntityShadowLabel.intern(),
    ] {
        if graph.get_node_state(predecessor).is_ok() {
            graph.add_node_edge(predecessor, EnhancedSnapshotLabel);
        }
    }
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
    if hand && rig {
        graph.add_node_edge(EnhancedHandLabel, EnhancedHandRigLabel);
    }
    graph.add_node(EnhancedHandCompositeLabel, composite);
    graph.add_node_edges((
        EnhancedPostLabel,
        EnhancedHandCompositeLabel,
        Node3d::Tonemapping,
    ));
    if hand {
        graph.add_node_edge(EnhancedHandLabel, EnhancedHandCompositeLabel);
    }
    if rig {
        graph.add_node_edge(EnhancedHandRigLabel, EnhancedHandCompositeLabel);
    }
    let overlay = crate::ui_render::overlay::UiOverlayPostLabel.intern();
    if graph.get_node_state(overlay).is_ok() {
        // The overlay graph places the HUD after post-processing; it still follows the grade.
        graph.add_node_edge(EnhancedHandCompositeLabel, overlay);
        if hand {
            graph.add_node_edge(EnhancedHandLabel, overlay);
        }
        if rig {
            graph.add_node_edge(EnhancedHandRigLabel, overlay);
        }
    }
}

/// Enhanced nodes timed by GPU timestamps.
pub(crate) fn timed_nodes() -> [(InternedRenderLabel, crate::RuntimeStage); 6] {
    use crate::RuntimeStage;
    [
        (EnhancedShadowLabel.intern(), RuntimeStage::GpuShadows),
        (EnhancedSnapshotLabel.intern(), RuntimeStage::GpuBlit),
        (EnhancedPostLabel.intern(), RuntimeStage::GpuPost),
        (EnhancedHandLabel.intern(), RuntimeStage::GpuHand),
        (EnhancedHandRigLabel.intern(), RuntimeStage::GpuHand),
        (EnhancedHandCompositeLabel.intern(), RuntimeStage::GpuHand),
    ]
}

/// Adds the post-grade twin of an installed main-pass node; `false` when that pass is absent.
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

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(super) struct EnhancedHandLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(super) struct EnhancedHandRigLabel;
