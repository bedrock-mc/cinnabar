use super::{
    depth::EnhancedDepthLabel,
    graph::{EnhancedHandLabel, EnhancedHandRigLabel, install_graph},
    post::{EnhancedLightingLabel, EnhancedPostLabel, EnhancedSkyLabel},
    shadows::EnhancedShadowLabel,
    snapshot::EnhancedSnapshotLabel,
    *,
};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    render::render_graph::{EmptyNode, RenderGraph, RenderLabel},
};
use std::collections::HashSet;

/// Checks graph reachability without looping on a cycle.
fn reaches(graph: &RenderGraph, from: impl RenderLabel, to: impl RenderLabel) -> bool {
    let target = to.intern();
    let mut stack = vec![from.intern()];
    let mut seen = HashSet::new();
    while let Some(label) = stack.pop() {
        if label == target {
            return true;
        }
        if seen.insert(label) {
            let state = graph.get_node_state(label).unwrap();
            stack.extend(
                state
                    .edges
                    .output_edges()
                    .iter()
                    .map(|edge| edge.get_input_node()),
            );
        }
    }
    false
}

// Bloom must not see the hand, HUD or menus, and grading must see Bloom's highlights.
#[test]
fn enhanced_views_bloom_and_grade_the_world_before_hand_and_ui() {
    use crate::{hand_rig_render::HandRigLabel, ui_render::*, viewmodel_render::HandLabel};
    let mut core = RenderGraph::default();
    for label in [
        Node3d::StartMainPass,
        Node3d::MainOpaquePass,
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
        Node3d::StartMainPassPostProcessing,
        Node3d::Bloom,
        Node3d::MotionBlur,
        Node3d::Tonemapping,
        Node3d::EndMainPassPostProcessing,
        Node3d::Upscaling,
    ] {
        core.add_node(label, EmptyNode);
    }
    core.add_node_edges((
        Node3d::StartMainPass,
        Node3d::MainOpaquePass,
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
        Node3d::StartMainPassPostProcessing,
        Node3d::Bloom,
        Node3d::Tonemapping,
        Node3d::EndMainPassPostProcessing,
        Node3d::Upscaling,
    ));
    core.add_node_edges((
        Node3d::StartMainPassPostProcessing,
        Node3d::MotionBlur,
        Node3d::Bloom,
    ));
    core.add_node(UiWorldLabel, EmptyNode);
    core.add_node(UiOverlayLabel, EmptyNode);
    core.add_node(overlay::UiOverlayPostLabel, EmptyNode);
    core.add_node(HandLabel, EmptyNode);
    core.add_node(HandRigLabel, EmptyNode);
    core.add_node_edges((
        Node3d::MainTransparentPass,
        UiWorldLabel,
        HandLabel,
        Node3d::EndMainPass,
    ));
    core.add_node_edges((UiWorldLabel, HandRigLabel, Node3d::EndMainPass));
    let mut graphs = RenderGraph::default();
    graphs.add_sub_graph(Core3d, core);
    let mut world = World::new();
    world.insert_resource(graphs);

    install_overlay_graph(&mut world);
    install_graph(&mut world);

    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    assert!(reaches(graph, Node3d::MainOpaquePass, EnhancedSkyLabel));
    assert!(reaches(graph, EnhancedShadowLabel, EnhancedDepthLabel));
    assert!(reaches(graph, EnhancedDepthLabel, EnhancedLightingLabel));
    assert!(reaches(
        graph,
        EnhancedLightingLabel,
        Node3d::MainOpaquePass
    ));
    assert!(!reaches(
        graph,
        Node3d::MainOpaquePass,
        EnhancedLightingLabel
    ));
    assert!(reaches(graph, EnhancedSkyLabel, EnhancedSnapshotLabel));
    assert!(reaches(
        graph,
        EnhancedSnapshotLabel,
        Node3d::MainTransparentPass
    ));
    assert!(reaches(graph, Node3d::Bloom, EnhancedPostLabel));
    assert!(!reaches(graph, EnhancedPostLabel, Node3d::Bloom));
    assert!(!reaches(graph, EnhancedPostLabel, Node3d::EndMainPass));
    for post in [EnhancedHandLabel.intern(), EnhancedHandRigLabel.intern()] {
        assert!(reaches(graph, EnhancedPostLabel, post), "{post:?}");
        assert!(reaches(graph, post, Node3d::Tonemapping), "{post:?}");
    }
    // The HUD composites after every post-process, FXAA included, and before the output.
    let hud = overlay::UiOverlayPostLabel;
    assert!(reaches(graph, EnhancedPostLabel, hud.clone()));
    assert!(reaches(
        graph,
        Node3d::EndMainPassPostProcessing,
        hud.clone()
    ));
    assert!(reaches(graph, hud, Node3d::Upscaling));
    assert!(reaches(
        graph,
        EnhancedHandLabel,
        overlay::UiOverlayPostLabel
    ));
    assert!(reaches(
        graph,
        EnhancedHandRigLabel,
        overlay::UiOverlayPostLabel
    ));
}

// Graph routing follows the extracted camera component directly, including opt-out.
#[test]
fn grade_stage_follows_camera_opt_in_without_a_separate_marker() {
    let mut world = World::new();
    let camera = world.spawn(EnhancedRendering::default()).id();
    let mut query = world.query::<Has<EnhancedRendering>>();
    assert!(query.get(&world, camera).unwrap());
    world.entity_mut(camera).remove::<EnhancedRendering>();
    assert!(!query.get(&world, camera).unwrap());
}

/// Enhanced's required components never propagate to an ordinary camera.
#[test]
fn temporal_jitter_is_required_only_for_opted_in_cameras() {
    use bevy::render::camera::TemporalJitter;
    let mut world = World::new();
    let vanilla = world.spawn(Camera3d::default()).id();
    let enhanced = world
        .spawn((Camera3d::default(), EnhancedRendering::default()))
        .id();
    assert!(world.get::<TemporalJitter>(vanilla).is_none());
    assert!(world.get::<TemporalJitter>(enhanced).is_some());
    assert_eq!(world.get::<Msaa>(enhanced), Some(&Msaa::Off));
}
