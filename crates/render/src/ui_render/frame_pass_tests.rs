//! Which passes a menu frame and a gameplay frame encode, and where FXAA sits.
use super::*;
use bevy::render::render_graph::EmptyNode;

fn batch(blend: u32) -> (usize, UiRenderBatch, ()) {
    (
        0,
        UiRenderBatch::new(0, render_model::UiScissor::new(0, 0, 1, 1), 0, 6, blend),
        (),
    )
}

/// A menu HUD is one retained layer that composites straight into the output.
#[test]
fn menu_frame_is_one_layer_composited_in_the_output_pass() {
    let menu = vec![batch(UI_BLEND_ALPHA); 3];
    let plan = plan_ui_passes(&menu, true);
    assert!(plan.retainable);
    assert_eq!(
        plan.segments,
        [UiSegment {
            layered: 0..3,
            inverted: None,
            present: true,
        }]
    );
    // Without the output-pass node the layer composites itself.
    assert!(!plan_ui_passes(&menu, false).segments[0].present);
}

/// The crosshair splits the HUD: layer, composite, invert, then the output-pass layer.
#[test]
fn gameplay_frame_composites_before_the_crosshair_and_presents_the_rest() {
    let hud = [
        batch(UI_BLEND_ALPHA),
        batch(UI_BLEND_INVERT),
        batch(UI_BLEND_ALPHA),
    ];
    let plan = plan_ui_passes(&hud, true);
    assert!(!plan.retainable);
    assert_eq!(
        plan.segments,
        [
            UiSegment {
                layered: 0..1,
                inverted: Some(1),
                present: false,
            },
            UiSegment {
                layered: 2..3,
                inverted: None,
                present: true,
            },
        ]
    );
    let trailing = plan_ui_passes(&hud[..2], true);
    assert!(trailing.retainable);
    assert!(trailing.segments.iter().all(|segment| !segment.present));
}

fn reaches(graph: &RenderGraph, from: impl RenderLabel, to: impl RenderLabel) -> bool {
    let target = to.intern();
    let mut stack = vec![from.intern()];
    let mut seen = std::collections::HashSet::new();
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
                    .map(|e| e.get_input_node()),
            );
        }
    }
    false
}

/// FXAA runs on the scene only: the HUD composites after it, inside the output pass.
#[test]
fn fxaa_never_reaches_ui_pixels() {
    let mut core = RenderGraph::default();
    let order = [
        Node3d::MainOpaquePass.intern(),
        Node3d::MainTransparentPass.intern(),
        Node3d::EndMainPass.intern(),
        Node3d::Tonemapping.intern(),
        Node3d::Fxaa.intern(),
        Node3d::EndMainPassPostProcessing.intern(),
        Node3d::Upscaling.intern(),
    ];
    for label in order {
        core.add_node(label, EmptyNode);
    }
    for pair in order.windows(2) {
        core.add_node_edge(pair[0], pair[1]);
    }
    let mut graphs = RenderGraph::default();
    graphs.add_sub_graph(Core3d, core);
    let mut world = World::new();
    world.insert_resource(graphs);
    install_overlay_graph(&mut world);
    install_overlay_graph(&mut world);
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    assert!(reaches(graph, UiWorldLabel, Node3d::Fxaa));
    assert!(reaches(graph, Node3d::Fxaa, UiOverlayLabel));
    assert!(!reaches(graph, UiOverlayLabel, Node3d::Fxaa));
    assert!(reaches(graph, UiOverlayLabel, Node3d::Upscaling));
    assert!(world.contains_resource::<super::super::composite::UiPresentInstalled>());
    assert!(
        graph
            .get_node_state(Node3d::Upscaling)
            .unwrap()
            .type_name
            .contains("UiPresentNode")
    );
}

/// An unchanged layer is reused; any change in what it was drawn from redraws it.
#[test]
fn retained_layer_is_reused_only_for_identical_content() {
    let world = super::super::ordered_command_tests::binding_world();
    let device = world.resource::<RenderDevice>();
    let texture = device.create_texture(&bevy::render::render_resource::TextureDescriptor {
        label: None,
        size: bevy::render::render_resource::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: bevy::render::render_resource::TextureDimension::D2,
        format: super::super::composite::UI_LAYER_FORMAT,
        usage: bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let layer = super::super::composite::UiLayerTexture::detached(texture, view);
    let content = super::super::composite::UiLayerContent {
        revision: 3,
        skip: None,
        viewport: None,
        model_depth: false,
    };
    assert_eq!(layer.holds(&content), None);
    layer.hold(Some((content.clone(), true)));
    assert_eq!(layer.holds(&content), Some(true));
    let next = super::super::composite::UiLayerContent {
        revision: 4,
        ..content.clone()
    };
    assert_eq!(layer.holds(&next), None);
    layer.hold(None);
    assert_eq!(layer.holds(&content), None);
}
