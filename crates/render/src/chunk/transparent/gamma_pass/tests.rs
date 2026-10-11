use super::*;

#[test]
fn gamma_target_admission_is_narrow() {
    assert!(admitted(false, Msaa::Off, false));
    assert!(!admitted(true, Msaa::Off, false));
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        assert!(admitted(false, msaa, false));
    }
    assert_eq!(
        admitted(false, Msaa::Off, true),
        !render_model::ENHANCED_RENDERING_ENABLED
    );
}

#[test]
fn encoded_and_standard_ranges_keep_sorted_order() {
    let items = [false, true, true, false, true, false, false];
    let ranges = contiguous_ranges(&items, |item| *item).collect::<Vec<_>>();
    assert_eq!(
        ranges,
        vec![
            (0..1, false),
            (1..3, true),
            (3..4, false),
            (4..5, true),
            (5..7, false)
        ]
    );
    let replay = ranges
        .iter()
        .flat_map(|(range, _)| range.clone())
        .collect::<Vec<_>>();
    assert_eq!(replay, (0..items.len()).collect::<Vec<_>>());
}

#[test]
fn homogeneous_and_empty_ranges_do_not_allocate_extra_passes() {
    assert_eq!(
        contiguous_ranges(&[true, true], |item| *item).collect::<Vec<_>>(),
        vec![(0..2, true)]
    );
    assert_eq!(
        contiguous_ranges::<bool, bool>(&[], |item| *item).count(),
        0
    );
}

#[test]
fn scratch_and_scene_formats_are_raw_copy_compatible() {
    let scene = TextureFormat::bevy_default();
    let scratch = scene.remove_srgb_suffix();
    assert_ne!(scene, scratch);
    assert_eq!(scene.remove_srgb_suffix(), scratch.remove_srgb_suffix());
}

#[test]
fn nametag_draws_enter_the_encoded_phase_without_reordering() {
    use crate::chunk::transparent::mixed::DrawMixedTerrainCommands;
    use bevy::{app::SubApp, render::render_phase::AddRenderCommand};

    let mut app = App::new();
    app.init_resource::<Assets<Shader>>();
    let mut render = SubApp::new();
    render
        .init_resource::<DrawFunctions<Transparent3d>>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidCommands>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidDirectCommands>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidIndirectCommands>()
        .add_render_command::<Transparent3d, DrawTransparentModelCommands>()
        .add_render_command::<Transparent3d, DrawMixedTerrainCommands>();
    app.insert_sub_app(RenderApp, render);
    crate::nametag_render::install_nametag_render(&mut app);
    let world = app.sub_app(RenderApp).world();
    let tag = crate::nametag_render::draw_function(world).unwrap();
    let families = native_draws(world);
    assert!(families.contains(&Some(tag)));
    let items = [tag, tag];
    assert_eq!(
        contiguous_ranges(&items, |id| families.contains(&Some(*id))).collect::<Vec<_>>(),
        vec![(0..2, true)]
    );
    let liquid = world
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawTransparentLiquidCommands>();
    let items = [liquid, tag, tag, liquid, tag];
    for enabled in [false, true] {
        for gamma in [false, true] {
            let ranges = contiguous_ranges(&items, |draw| {
                (
                    gamma,
                    crate::nametag_render::deferred_by_world_filter(enabled, Some(tag), *draw),
                )
            })
            .collect::<Vec<_>>();
            let ordinary = ranges
                .iter()
                .filter(|(_, (_, deferred))| !deferred)
                .flat_map(|(range, _)| range.clone())
                .collect::<Vec<_>>();
            let delayed = ranges
                .iter()
                .filter(|(_, (_, deferred))| *deferred)
                .flat_map(|(range, _)| range.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                ordinary,
                if enabled {
                    vec![0, 3]
                } else {
                    vec![0, 1, 2, 3, 4]
                }
            );
            assert_eq!(delayed, if enabled { vec![1, 2, 4] } else { vec![] });
            assert!(ranges.iter().all(|(_, (mode, _))| *mode == gamma));
        }
    }
    assert!(!crate::nametag_render::deferred_by_world_filter(
        true, None, tag
    ));
}

#[test]
fn graph_replacement_preserves_existing_dependencies() {
    use bevy::render::render_graph::EmptyNode;
    let mut world = World::new();
    let mut core = RenderGraph::default();
    core.add_node(Node3d::MainOpaquePass, EmptyNode);
    core.add_node(Node3d::MainTransparentPass, EmptyNode);
    core.add_node(Node3d::EndMainPass, EmptyNode);
    core.add_node_edges((
        Node3d::MainOpaquePass,
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
    ));
    let mut graphs = RenderGraph::default();
    graphs.add_sub_graph(Core3d, core);
    world.insert_resource(graphs);
    install_graph(&mut world);
    let graphs = world.resource::<RenderGraph>();
    let core = graphs.get_sub_graph(Core3d).unwrap();
    let node = core.get_node_state(Node3d::MainTransparentPass).unwrap();
    assert_eq!(node.edges.input_edges().len(), 1);
    assert_eq!(node.edges.output_edges().len(), 1);
    assert!(node.node::<ViewNodeRunner<GammaTransparentPass>>().is_ok());
}
