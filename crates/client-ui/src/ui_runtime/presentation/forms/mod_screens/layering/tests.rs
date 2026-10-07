use super::*;
use ui::{UiNode, UiNodeId, UiRect, UiVisual};

fn rect(x: f32, y: f32, w: f32, h: f32) -> UiRect {
    UiRect::new(
        ui::UiPoint::new(x, y).unwrap(),
        ui::UiPoint::new(x + w, y + h).unwrap(),
    )
    .unwrap()
}

fn group(id: u32, x: f32, y: f32, w: f32, h: f32) -> UiNode {
    UiNode::new(UiNodeId::new(id), None, rect(x, y, w, h)).with_clip_children(true)
}

fn leaf(id: u32, parent: u32, x: f32, y: f32, w: f32, h: f32) -> UiNode {
    UiNode::new(
        UiNodeId::new(id),
        Some(UiNodeId::new(parent)),
        rect(x, y, w, h),
    )
    .with_visual(UiVisual::Solid {
        texture_page: 0,
        color: [1, 2, 3, 255],
    })
}

fn ids(nodes: &[UiNode]) -> Vec<u32> {
    nodes.iter().map(|node| node.id().get()).collect()
}

#[test]
fn the_gui_rect_is_the_panel_and_every_control_but_full_screen_ones() {
    let root = [480.0, 270.0];
    let panel = Some([152.0, 52.0, 176.0, 166.0]);
    // A recipe book beside the panel and a full-screen dismiss region.
    let controls = [[4.0, 52.0, 147.0, 166.0], [0.0, 0.0, 480.0, 270.0]];
    let gui = gui_rect(panel, controls.iter().copied(), root).unwrap();
    assert_eq!(
        [gui.x, gui.y, gui.width, gui.height],
        [4.0, 52.0, 324.0, 166.0]
    );
    assert!(gui_rect(None, std::iter::empty(), root).is_none());
}

#[test]
fn overlay_nodes_touching_a_forbidden_area_are_dropped() {
    let mut nodes = vec![
        group(1, 0.0, 0.0, 400.0, 300.0),
        // Vanilla's nodes before the overlay are never touched.
        leaf(2, 1, 100.0, 100.0, 10.0, 10.0),
        group(3, 100.0, 0.0, 300.0, 300.0),
        // Relative to its group: at 110..120, inside the forbidden 100..200.
        leaf(4, 3, 10.0, 10.0, 10.0, 10.0),
        // At 300..320, clear of it.
        leaf(5, 3, 200.0, 10.0, 20.0, 10.0),
    ];
    clip_overlay(&mut nodes, 2, &[[100.0, 0.0, 200.0, 300.0]], &[], &mut 10);
    assert_eq!(ids(&nodes), [1, 2, 3, 5]);
}

#[test]
fn overlay_tooltips_survive_the_clip_and_draw_above_the_overlay() {
    let mut nodes = vec![
        group(1, 0.0, 0.0, 400.0, 300.0),
        // The hovered entry's tooltip, flipped left over the forbidden area.
        group(2, 150.0, 0.0, 120.0, 20.0),
        leaf(3, 2, 0.0, 0.0, 120.0, 20.0),
        // A later entry, clear of the forbidden area.
        leaf(4, 1, 250.0, 0.0, 16.0, 16.0),
    ];
    let mut next = 10;
    clip_overlay(
        &mut nodes,
        1,
        &[[100.0, 0.0, 200.0, 300.0]],
        std::slice::from_ref(&(1..3)),
        &mut next,
    );
    assert_eq!(ids(&nodes), [1, 4, 10, 11]);
    assert_eq!(nodes[2].bounds(), rect(150.0, 0.0, 120.0, 20.0));
    assert_eq!(nodes[3].parent(), Some(UiNodeId::new(10)));
    ui::UiTree::new(nodes).unwrap();
}

#[test]
fn lifted_nodes_move_above_later_ones_under_a_copy_of_their_group() {
    let mut nodes = vec![
        group(1, 0.0, 0.0, 400.0, 300.0),
        leaf(2, 1, 0.0, 0.0, 10.0, 10.0),
        // The held stack, in the shared group.
        leaf(3, 1, 50.0, 50.0, 16.0, 16.0),
        leaf(4, 1, 5.0, 5.0, 10.0, 10.0),
        // A tooltip with its own group.
        group(5, 60.0, 60.0, 100.0, 20.0),
        leaf(6, 5, 0.0, 0.0, 100.0, 20.0),
    ];
    let lifted = take_lifted(&mut nodes, &[2..3, 4..6]);
    assert_eq!(ids(&nodes), [1, 2, 4]);
    // The overlay draws here, then the lifted nodes come back with fresh ids.
    let mut next = 10;
    restore_lifted(&mut nodes, lifted, &mut next);
    assert_eq!(ids(&nodes), [1, 2, 4, 10, 11, 12, 13]);
    assert_eq!(next, 14);
    // The held stack sits under a copy of its group.
    assert_eq!(nodes[3].parent(), None);
    assert!(nodes[3].clips_children());
    assert_eq!(nodes[3].bounds(), nodes[0].bounds());
    assert_eq!(nodes[4].parent(), Some(UiNodeId::new(10)));
    assert_eq!(nodes[4].bounds(), rect(50.0, 50.0, 16.0, 16.0));
    // The tooltip's own group moved with it.
    assert_eq!(nodes[5].bounds(), rect(60.0, 60.0, 100.0, 20.0));
    assert_eq!(nodes[6].parent(), Some(UiNodeId::new(12)));
    ui::UiTree::new(nodes).unwrap();
}

#[test]
fn hits_inside_a_forbidden_area_are_not_the_overlays() {
    let layout = ScreenLayout {
        screen: "crafting.inventory_screen".into(),
        size: server_experience::screen::GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        gui: server_experience::screen::Rect {
            x: 152.0,
            y: 52.0,
            width: 176.0,
            height: 166.0,
        },
        exclusions: vec![server_experience::screen::Rect {
            x: 400.0,
            y: 0.0,
            width: 80.0,
            height: 30.0,
        }],
        view: None,
    };
    assert!(overlay_hit_allowed(&layout, [340.0, 60.0, 18.0, 18.0]));
    assert!(!overlay_hit_allowed(&layout, [320.0, 60.0, 18.0, 18.0]));
    assert!(!overlay_hit_allowed(&layout, [420.0, 20.0, 18.0, 18.0]));
    assert!(!overlay_hit_allowed(&layout, [0.0, 0.0, 480.0, 270.0]));
}

fn view_layout() -> ScreenLayout {
    ScreenLayout {
        screen: "crafting.inventory_screen".into(),
        size: server_experience::screen::GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        gui: server_experience::screen::Rect {
            x: 152.0,
            y: 52.0,
            width: 176.0,
            height: 166.0,
        },
        exclusions: Vec::new(),
        view: Some(server_experience::screen::Rect {
            x: 100.0,
            y: 40.0,
            width: 250.0,
            height: 190.0,
        }),
    }
}

#[test]
fn an_open_view_stands_in_for_the_vanilla_panels() {
    let layout = view_layout();
    // Clear of the hidden container's panel but on the view.
    assert!(!overlay_hit_allowed(&layout, [330.0, 60.0, 10.0, 10.0]));
    // Beside the view.
    assert!(overlay_hit_allowed(&layout, [360.0, 60.0, 18.0, 18.0]));
    assert_eq!(
        forbidden_logical(&layout, 2.0),
        vec![[200.0, 80.0, 700.0, 460.0]]
    );
}

#[test]
fn the_views_bounds_are_its_drawn_nodes_but_full_screen_ones() {
    let nodes = [
        group(1, 0.0, 0.0, 960.0, 540.0),
        // A full-screen dim behind the view does not count.
        leaf(2, 1, 0.0, 0.0, 960.0, 540.0),
        leaf(3, 1, 200.0, 80.0, 400.0, 300.0),
        group(4, 600.0, 100.0, 100.0, 100.0),
        leaf(5, 4, 0.0, 0.0, 50.0, 20.0),
    ];
    let bounds = drawn_bounds(&nodes[..], [960.0, 540.0], 2.0).unwrap();
    assert_eq!(
        [bounds.x, bounds.y, bounds.width, bounds.height],
        [100.0, 40.0, 225.0, 150.0]
    );
    assert!(drawn_bounds(&nodes[..2], [960.0, 540.0], 2.0).is_none());
}

#[test]
fn the_row_under_the_pointer_is_the_topmost_controls_nearest_collection() {
    let regions = [
        ([0.0, 0.0, 100.0, 100.0], Some(("grid", 4)), true),
        ([10.0, 10.0, 18.0, 18.0], Some(("items", 7)), true),
        ([10.0, 10.0, 18.0, 18.0], None, true),
        ([40.0, 40.0, 18.0, 18.0], Some(("items", 9)), false),
    ];
    let row = |point| nearest_row(regions.iter().copied(), point);
    assert_eq!(row([12.0, 12.0]), Some(("items".to_owned(), 7)));
    assert_eq!(row([50.0, 50.0]), Some(("grid".to_owned(), 4)));
    assert_eq!(row([200.0, 200.0]), None);
}

#[test]
fn beside_an_open_view_the_pointer_is_the_overlays() {
    let layout = view_layout();
    // On the view.
    assert_eq!(
        pointer_surface(true, Some(&layout), [200.0, 100.0]),
        Surface::View
    );
    // Beside the view, where the overlay draws.
    assert_eq!(
        pointer_surface(true, Some(&layout), [380.0, 100.0]),
        Surface::Overlay
    );
    // Without a view the overlay takes what the container's panels leave it.
    assert_eq!(
        pointer_surface(false, Some(&layout), [200.0, 100.0]),
        Surface::Overlay
    );
}
