use super::*;
use std::sync::Arc;
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiPoint, UiVisual};
use ui_engine::{test_support::fixture_font, text_metrics::TextMetrics};

const SIZE: [f32; 2] = [1280.0, 720.0];

/// Paints the real import renderer with synthetic fonts and retained scroll state.
fn paint(
    screen: &mut OreUiScreen,
    view: &MenuView,
    scrolls: &mut MenuScrolls<MenuAction>,
) -> (Vec<(MenuAction, UiRect)>, Vec<UiNode>) {
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, TextLayoutCache::new(128, 1024 * 1024));
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: SIZE,
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    let hits = screen
        .paint_minecraft_import(view, SIZE, scrolls, &mut frame)
        .unwrap();
    (hits, nodes)
}

/// Provides an overflowing list with no initial focused profile to trigger a reveal.
fn profiles() -> MenuView {
    let mut view = MenuView::new(true, "Fixture".into());
    view.focused_action = None;
    let state = Arc::make_mut(&mut view.minecraft_import);
    state.phase = Phase::Profiles;
    state.profiles = (0..40).map(|index| format!("Profile {index}")).collect();
    view
}

/// Resolves text's vertical bounds through its scroll and animation parents.
fn window_vertical_bounds(nodes: &[UiNode], node: &UiNode) -> [f32; 2] {
    let mut offset = 0.0;
    let mut parent = node.parent();
    while let Some(id) = parent {
        let ancestor = nodes.iter().find(|node| node.id() == id).unwrap();
        offset += ancestor.bounds().min().y();
        parent = ancestor.parent();
    }
    [
        node.bounds().min().y() + offset,
        node.bounds().max().y() + offset,
    ]
}

#[test]
fn first_launch_title_fits_the_header_and_import_details_have_a_body() {
    let mut view = MenuView::new(true, "Fixture".into());
    Arc::make_mut(&mut view.minecraft_import).phase = Phase::Offered;
    let mut screen = OreUiScreen::default();
    let mut scrolls = MenuScrolls::default();
    let (hits, nodes) = paint(&mut screen, &view, &mut scrolls);
    let title = nodes
        .iter()
        .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
        .expect("the offer has a title");
    let UiVisual::Text { layout, .. } = title.visual() else {
        unreachable!();
    };
    assert_eq!(layout.line_count(), 1, "the title must fit on one line");
    let header = crate::test_support::solids(&nodes)
        .into_iter()
        .find_map(|(bounds, color)| (color == crate::theme::NEUTRAL.fill).then_some(bounds))
        .expect("the offer has a header");
    let title_bounds = window_vertical_bounds(&nodes, title);
    assert!(title_bounds[0] >= header[1]);
    assert!(title_bounds[1] <= header[3]);
    assert!(!view.minecraft_import.body().is_empty());
    let body = nodes
        .iter()
        .rev()
        .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
        .expect("the offer explains what will be imported");
    let body_bounds = window_vertical_bounds(&nodes, body);
    assert!(body_bounds[0] >= header[3]);
    assert!(body_bounds[1] <= SIZE[1]);
    for action in [Action::Begin, Action::NotNow] {
        assert!(
            hits.iter()
                .any(|(found, _)| *found == MenuAction::MinecraftImport(action))
        );
    }
}

#[test]
fn wheel_reaches_the_last_profile_and_survives_repainting() {
    let view = profiles();
    let mut screen = OreUiScreen::default();
    let mut scrolls = MenuScrolls::default();
    let last =
        MenuAction::MinecraftImport(Action::Select(view.minecraft_import.profiles.len() - 1));
    let (initial, _) = paint(&mut screen, &view, &mut scrolls);
    assert!(!initial.iter().any(|(action, _)| *action == last));
    assert!(scrolls.wheel(UiPoint::new(640.0, 360.0).unwrap(), -100000.0, true));
    let (scrolled, _) = paint(&mut screen, &view, &mut scrolls);
    assert!(scrolled.iter().any(|(action, _)| *action == last));
    assert_eq!(paint(&mut screen, &view, &mut scrolls).0, scrolled);
}

#[test]
fn keyboard_focus_reveals_the_last_profile_without_undoing_later_wheel_input() {
    let mut view = profiles();
    let mut screen = OreUiScreen::default();
    let mut scrolls = MenuScrolls::default();
    paint(&mut screen, &view, &mut scrolls);
    let last =
        MenuAction::MinecraftImport(Action::Select(view.minecraft_import.profiles.len() - 1));
    view.focused_action = Some(last);
    let (revealed, _) = paint(&mut screen, &view, &mut scrolls);
    assert!(revealed.iter().any(|(action, _)| *action == last));
    assert!(scrolls.offsets()[modal::SCROLL] > 0.0);
    assert!(scrolls.wheel(UiPoint::new(640.0, 360.0).unwrap(), 100000.0, true));
    let (scrolled, _) = paint(&mut screen, &view, &mut scrolls);
    assert_eq!(scrolls.offsets()[modal::SCROLL], 0.0);
    assert!(!scrolled.iter().any(|(action, _)| *action == last));
}

#[test]
fn long_preview_scrolls_its_text_while_the_apply_button_stays_fixed() {
    let mut view = MenuView::new(true, "Fixture".into());
    let state = Arc::make_mut(&mut view.minecraft_import);
    state.phase = Phase::Preview;
    state.body = "Preview entry\n".repeat(80);
    let mut screen = OreUiScreen::default();
    let mut scrolls = MenuScrolls::default();
    let (initial_hits, initial_nodes) = paint(&mut screen, &view, &mut scrolls);
    let text_bounds = |nodes: &[UiNode]| {
        nodes
            .iter()
            .rev()
            .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
            .unwrap()
            .bounds()
    };
    let initial_text = text_bounds(&initial_nodes);
    assert!(scrolls.wheel(UiPoint::new(640.0, 360.0).unwrap(), -100000.0, true));
    let (scrolled_hits, scrolled_nodes) = paint(&mut screen, &view, &mut scrolls);
    let scrolled_text = text_bounds(&scrolled_nodes);
    assert!(scrolled_text.min().y() < initial_text.min().y());
    let apply = MenuAction::MinecraftImport(Action::Apply);
    let button =
        |hits: &[(MenuAction, UiRect)]| hits.iter().find(|(action, _)| *action == apply).unwrap().1;
    assert_eq!(button(&initial_hits), button(&scrolled_hits));
}
