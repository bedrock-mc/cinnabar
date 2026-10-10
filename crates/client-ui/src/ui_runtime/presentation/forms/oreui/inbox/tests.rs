use super::*;
use crate::ui_runtime::presentation::{TextMetrics, UiPresentationRuntime, tests::fixture_font};
use ui::{DpiScale, UiNode, UiVisual};

fn frame(runtime: &mut UiPresentationRuntime, view: &MenuView, seconds: f64) -> Vec<UiNode> {
    runtime.menu_seconds = seconds;
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    runtime.menu_hit_targets = runtime
        .append_oreui_screen(
            view,
            &mut nodes,
            &mut 1,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap()
        .unwrap();
    runtime.end_animation_frame();
    nodes
}

fn text_node(nodes: &[UiNode], words: &str) -> UiNode {
    nodes.iter().find(|node| matches!(node.visual(), UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>().replace(' ', "") == words.replace(' ', "")))
        .unwrap_or_else(|| panic!("missing text {words}")).clone()
}

#[test]
fn category_highlight_fades_after_pointer_leaves() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Inbox;
    frame(&mut runtime, &view, 0.0);
    view.hovered = Some(MenuAction::Inbox(Action::Category(1)));
    frame(&mut runtime, &view, 1.0);
    frame(&mut runtime, &view, 1.2);
    let bounds = runtime
        .menu_hit_targets
        .iter()
        .find(|(action, _)| *action == view.hovered.unwrap())
        .unwrap()
        .1;
    view.hovered = None;
    frame(&mut runtime, &view, 2.0);
    let fading = frame(&mut runtime, &view, 2.04);
    let idle = super::super::theme::NEUTRAL80.fill;
    let bounds = [
        bounds.min().x(),
        bounds.min().y(),
        bounds.max().x(),
        bounds.max().y(),
    ];
    assert!(
        super::super::review_tests::solids(&fading)
            .iter()
            .any(|(row, color)| *row == bounds && *color != idle),
        "the departing highlight must keep rendering while it fades"
    );
}

fn art_runtime() -> (UiPresentationRuntime, u16) {
    use crate::ui_runtime::oreui_assets::{
        INBOX_EMPTY_IMAGES, INBOX_ICONS, OreUiImages, OreUiPage, OreUiSprite,
        SETTINGS_ICON_HIGHLIGHT_IMAGE,
    };
    use std::{collections::HashMap, sync::Arc};
    let mut sprites: HashMap<_, _> = INBOX_ICONS
        .into_iter()
        .chain(INBOX_EMPTY_IMAGES)
        .map(|key| {
            (
                key.into(),
                OreUiSprite {
                    page: 0,
                    bounds: [0, 0, 24, 24],
                },
            )
        })
        .collect();
    sprites.insert(
        SETTINGS_ICON_HIGHLIGHT_IMAGE.into(),
        OreUiSprite {
            page: 1,
            bounds: [0, 0, 216, 24],
        },
    );
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let page = runtime.textures.dynamic_start() as u16 + 1;
    runtime
        .enable_oreui_originals(OreUiImages {
            pages: vec![
                OreUiPage {
                    dimensions: [24; 2],
                    pixels: vec![255; 24 * 24 * 4].into(),
                },
                OreUiPage {
                    dimensions: [216, 24],
                    pixels: vec![255; 216 * 24 * 4].into(),
                },
            ],
            sprites: Arc::new(sprites),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        })
        .unwrap();
    (runtime, page)
}

#[test]
fn inbox_selection_glimmers_once_and_obeys_the_animation_setting() {
    let (mut runtime, page) = art_runtime();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Inbox;
    let crops = |nodes: Vec<UiNode>| {
        nodes
            .into_iter()
            .filter_map(|node| match node.visual() {
                UiVisual::Sprite {
                    texture_page, uv, ..
                } if *texture_page == page => Some(*uv),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(crops(frame(&mut runtime, &view, 0.0)), [[0, 0, 24, 24]]);
    assert_eq!(crops(frame(&mut runtime, &view, 0.051)), [[48, 0, 72, 24]]);
    assert_eq!(crops(frame(&mut runtime, &view, 0.3)), [[192, 0, 216, 24]]);
    assert_eq!(crops(frame(&mut runtime, &view, 3.0)), [[192, 0, 216, 24]]);
    view.feeds.inbox_state.category = 2;
    assert_eq!(crops(frame(&mut runtime, &view, 4.0)), [[0, 0, 24, 24]]);
    runtime
        .form_presentation
        .oreui_transitions
        .configure_motion(false);
    assert!(crops(frame(&mut runtime, &view, 4.05)).is_empty());
    assert!(
        matches!(text_node(&frame(&mut runtime, &view, 5.0), "No new invites").visual(), UiVisual::Text { color, .. } if color[3] == 255)
    );
}

#[test]
fn maintenance_and_confirmation_only_expose_their_own_inputs() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Inbox;
    view.feeds.inbox_state.filters = true;
    frame(&mut runtime, &view, 0.0);
    for action in [Action::Filters, Action::MarkAllRead, Action::DeleteAllRead] {
        assert!(
            runtime
                .form_presentation
                .menu_focus
                .contains(&MenuAction::Inbox(action))
        );
    }
    assert!(
        !runtime
            .menu_hit_targets
            .iter()
            .any(|(action, _)| matches!(action, MenuAction::Inbox(Action::Category(_))))
    );
    view.feeds.inbox_state.delete_pending = Some(vec!["message".into()]);
    frame(&mut runtime, &view, 1.0);
    assert!(runtime.menu_hit_targets.iter().all(|(action, _)| matches!(
        action,
        MenuAction::Inbox(Action::Cancel | Action::ConfirmDelete)
    )));
    assert!(
        runtime
            .form_presentation
            .menu_focus
            .iter()
            .all(|action| matches!(
                action,
                MenuAction::Inbox(Action::Cancel | Action::ConfirmDelete)
            ))
    );
}

#[test]
fn empty_cards_keep_wrapped_copy_inside_their_padding() {
    use std::collections::HashMap;
    let mut bottom = 0.0;
    let (_, _, nodes) = super::super::review_tests::paint(HashMap::new(), |canvas| {
        bottom =
            empty::draw(canvas, 0, [0.0, 0.0, 300.0, 600.0], &|_| None).unwrap() - canvas.r(1.6);
    });
    let mut wrapped = false;
    for node in nodes {
        if let UiVisual::Text { layout, .. } = node.visual() {
            wrapped |= layout.line_count() > 1;
            assert!(
                node.bounds().max().y() <= bottom,
                "all wrapped copy must stay above the card's bottom padding"
            );
        }
    }
    assert!(wrapped, "the small card exercises wrapping");
}

#[test]
fn switching_inbox_categories_animates_only_message_content() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Inbox;
    frame(&mut runtime, &view, 0.0);
    let before = frame(&mut runtime, &view, 0.2);
    view.feeds.inbox_state.category = 1;
    frame(&mut runtime, &view, 1.0);
    let changing = frame(&mut runtime, &view, 1.04);
    let header = text_node(&before, "INBOX");
    assert_eq!(header.bounds(), text_node(&changing, "INBOX").bounds());
    assert_eq!(header.visual(), text_node(&changing, "INBOX").visual());
    let empty = changing.iter().find(|node| matches!(node.visual(), UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>().replace(' ', "").starts_with("No"))).unwrap();
    assert!(
        matches!(empty.visual(), UiVisual::Text { color, .. } if color[3] > 0 && color[3] < 255),
        "the category body must enter without reanimating its header"
    );
}
