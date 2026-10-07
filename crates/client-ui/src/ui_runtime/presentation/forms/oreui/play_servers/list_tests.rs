use super::super::{motion::Surface, transitions::Transitions};
use super::*;
use crate::ui_runtime::presentation::forms::oreui::review_tests::{paint, solids};
use launcher::menu::server_list::{ServerGroup, ServerListAction};

fn view() -> MenuView {
    let mut view = MenuView::new(true, "Fixture".into());
    view.featured = ["Experience", "Creator"]
        .into_iter()
        .map(|name| MenuServerCard {
            name: name.into(),
            address: format!("{name}.test:19132"),
            caption: "Live".into(),
            image_path: String::new(),
            icon: None,
        })
        .collect();
    view.feeds.details.insert(
        view.featured[1].address.clone(),
        crate::menu::ServerDetails {
            group: "creator".into(),
            ..Default::default()
        },
    );
    view.servers = vec![crate::menu::SavedServer {
        name: "Home".into(),
        address: "home.test:19132".into(),
        favorite: false,
        last_joined_unix: 0,
    }];
    view
}

#[test]
fn server_list_collapsing_hides_only_its_rows_and_keeps_the_selected_details() {
    let mut view = view();
    view.feeds.select(0);
    let render = |view: &MenuView| {
        paint(HashMap::new(), |canvas| {
            draw(
                canvas,
                view,
                &Grid::new(canvas.rem, 1280.0),
                [20.0, 100.0, 1260.0, 700.0],
                &HashMap::new(),
            )
            .unwrap();
        })
    };
    let (_, before, _) = render(&view);
    assert!(
        before
            .iter()
            .any(|(a, _)| *a == MenuAction::SelectFeatured(0))
    );
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Featured));
    let (_, after, _) = render(&view);
    assert!(
        !after
            .iter()
            .any(|(a, _)| *a == MenuAction::SelectFeatured(0))
    );
    assert!(
        after
            .iter()
            .any(|(a, _)| *a == MenuAction::SelectFeatured(1))
    );
    assert!(after.iter().any(|(a, _)| *a == MenuAction::SelectSaved(0)));
    assert!(after.iter().any(|(a, _)| *a == MenuAction::PlayFeatured(0)));
    assert_eq!(view.feeds.selected_featured, Some(0));
}

#[test]
fn section_height_interpolates_on_reopening_and_collapse_and_respects_motion_setting() {
    let mut view = view();
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Featured));
    let mut transitions = Transitions::default();
    let mut frame = |view: &MenuView, seconds: f64, animated: bool| {
        transitions.configure_motion(animated);
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
        let font = crate::ui_runtime::presentation::tests::fixture_font();
        let metrics = crate::ui_runtime::presentation::TextMetrics::for_viewport(
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.surface = Surface::Play(2);
        canvas.seconds = seconds;
        canvas.transitions = Some(&mut transitions);
        let grid = Grid::new(canvas.rem, 1280.0);
        draw(
            &mut canvas,
            view,
            &grid,
            [20.0, 100.0, 1260.0, 700.0],
            &HashMap::new(),
        )
        .unwrap();
        let hits = std::mem::take(&mut canvas.hits);
        drop(canvas);
        transitions.motion.end_frame(seconds);
        hits.into_iter()
            .find(|(action, _)| {
                *action == MenuAction::ServerList(ServerListAction::Toggle(ServerGroup::Creator))
            })
            .unwrap()
            .1
            .min()
            .y()
    };
    let closed = frame(&view, 0.0, true);
    frame(&view, 0.5, true);
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Featured));
    assert_eq!(frame(&view, 1.0, true), closed);
    let middle = frame(&view, 1.06, true);
    let open = frame(&view, 1.3, true);
    assert!(closed < middle && middle < open);
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Featured));
    assert_eq!(frame(&view, 2.0, true), open);
    let closing = frame(&view, 2.06, true);
    assert!(closed < closing && closing < open);
    assert_eq!(frame(&view, 2.3, true), closed);
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Featured));
    assert_eq!(frame(&view, 3.0, false), open);
}

#[test]
fn server_list_order_moves_custom_rows_above_catalog_without_changing_actions() {
    let mut view = view();
    std::sync::Arc::make_mut(&mut view.settings_options).apply_server_list(
        ServerListAction::MoveBefore(ServerGroup::Saved, Some(ServerGroup::Featured)),
    );
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        draw(
            canvas,
            &view,
            &Grid::new(canvas.rem, 1280.0),
            [20.0, 100.0, 1260.0, 700.0],
            &HashMap::new(),
        )
        .unwrap();
    });
    let top = |action| hits.iter().find(|(a, _)| *a == action).unwrap().1.min().y();
    assert!(top(MenuAction::SelectSaved(0)) < top(MenuAction::SelectFeatured(0)));
    assert!(top(MenuAction::SelectSaved(0)) < top(MenuAction::SelectFeatured(1)));
    // Each header is one click/drag target, with no separate reorder buttons.
    assert_eq!(
        hits.iter()
            .filter(|(action, _)| matches!(
                action,
                MenuAction::ServerList(ServerListAction::MoveBefore(_, _))
            ))
            .count(),
        0
    );
    assert!(
        hits.iter().any(
            |(a, _)| *a == MenuAction::ServerList(ServerListAction::Toggle(ServerGroup::Saved))
        )
    );
}

#[test]
fn server_list_highlights_fade_for_hover_selection_and_focus_and_can_be_disabled() {
    let mut view = view();
    let mut transitions = Transitions::default();
    transitions.motion.end_frame(0.0);
    let b = [100.0, 100.0, 300.0, 150.0];
    let action = MenuAction::SelectSaved(0);
    let mut frame = |view: &MenuView, selected: bool, seconds: f64, animated: bool| {
        transitions.configure_motion(animated);
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(16, 65536));
        let font = crate::ui_runtime::presentation::tests::fixture_font();
        let metrics = crate::ui_runtime::presentation::TextMetrics::for_viewport(
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.surface = Surface::Play(2);
        canvas.seconds = seconds;
        canvas.transitions = Some(&mut transitions);
        server_row(&mut canvas, view, b, selected, action).unwrap();
        drop(canvas);
        transitions.motion.end_frame(seconds);
        solids(&nodes)
    };
    view.hovered = Some(action);
    let start = frame(&view, false, 1.0, true);
    let first_frame = frame(&view, false, 1.0 + 1.0 / 60.0, true);
    let middle = frame(&view, false, 1.035, true);
    let end = frame(&view, false, 1.2, true);
    let alpha = |nodes: &[([f32; 4], [u8; 4])]| {
        nodes
            .iter()
            .find(|(bounds, _)| *bounds == b)
            .map_or(0, |(_, color)| color[3])
    };
    assert_eq!(alpha(&start), 0);
    assert!(f32::from(alpha(&first_frame)) < f32::from(alpha(&end)) * 0.4);
    assert!(alpha(&middle) > 0 && alpha(&middle) < alpha(&end));
    view.hovered = None;
    frame(&view, true, 1.2, true);
    frame(&view, true, 1.4, true);
    let selected_start = frame(&view, false, 1.4, true);
    let selected_middle = frame(&view, false, 1.435, true);
    let selected_end = frame(&view, false, 1.6, true);
    assert!(alpha(&selected_middle) < alpha(&selected_start));
    assert_eq!(alpha(&selected_end), 0);
    view.navigation_focus_visible = true;
    view.focused_action = Some(action);
    frame(&view, false, 2.0, true);
    let focus = frame(&view, false, 2.035, true);
    assert!(
        focus
            .iter()
            .any(|(_, c)| c[..3] == super::super::theme::OUTLINE[..3] && c[3] > 0 && c[3] < 255)
    );
    view.hovered = Some(action);
    assert_eq!(alpha(&frame(&view, false, 3.0, false)), alpha(&end));
}

#[test]
fn hidden_sections_remove_headers_rows_and_height_while_filter_remains_reachable() {
    let mut view = view();
    let render = |view: &MenuView| {
        paint(HashMap::new(), |canvas| {
            draw(
                canvas,
                view,
                &Grid::new(canvas.rem, 1280.0),
                [20.0, 100.0, 1260.0, 700.0],
                &HashMap::new(),
            )
            .unwrap();
        })
    };
    let (_, before, _) = render(&view);
    let creator = MenuAction::SelectFeatured(1);
    let before_y = before
        .iter()
        .find(|(action, _)| *action == creator)
        .unwrap()
        .1
        .min()
        .y();
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::ToggleVisibility(ServerGroup::Featured));
    let (_, after, _) = render(&view);
    assert!(!after.iter().any(
        |(action, _)| matches!(action, MenuAction::SelectFeatured(0))
            || *action == MenuAction::ServerList(ServerListAction::Toggle(ServerGroup::Featured))
    ));
    assert!(
        after
            .iter()
            .find(|(action, _)| *action == creator)
            .unwrap()
            .1
            .min()
            .y()
            < before_y
    );

    let add = after
        .iter()
        .find(|(action, _)| *action == MenuAction::PlayAddServer)
        .unwrap()
        .1;
    let filter = after
        .iter()
        .find(|(action, _)| *action == MenuAction::OpenServerFilter)
        .unwrap()
        .1;
    assert_eq!(add.min().y(), filter.min().y());
    assert_eq!(add.max().y(), filter.max().y());
    assert!(filter.min().x() > add.max().x());
}

#[test]
fn server_navigation_keeps_offscreen_rows_without_pointer_hits() {
    let mut view = view();
    let mut focus = Vec::new();
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        canvas.capture_focus = true;
        draw(
            canvas,
            &view,
            &Grid::new(canvas.rem, 1280.0),
            [20.0, 100.0, 1260.0, 180.0],
            &HashMap::new(),
        )
        .unwrap();
        focus.extend(canvas.focus_hits.iter().map(|(action, _)| *action));
    });
    let offscreen = MenuAction::SelectSaved(0);
    assert!(focus.contains(&offscreen));
    assert!(!hits.iter().any(|(action, _)| *action == offscreen));
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::ToggleVisibility(ServerGroup::Saved));
    focus.clear();
    paint(HashMap::new(), |canvas| {
        canvas.capture_focus = true;
        draw(
            canvas,
            &view,
            &Grid::new(canvas.rem, 1280.0),
            [20.0, 100.0, 1260.0, 180.0],
            &HashMap::new(),
        )
        .unwrap();
        focus.extend(canvas.focus_hits.iter().map(|(action, _)| *action));
    });
    assert!(!focus.contains(&offscreen));
    assert!(
        !focus.contains(&MenuAction::ServerList(ServerListAction::Toggle(
            ServerGroup::Saved
        )))
    );
}

#[test]
fn server_play_route_captures_offscreen_controls_under_native_focus_landmarks() {
    let mut view = view();
    view.screen = crate::menu::MenuScreen::Servers;
    let saved = view.servers[0].clone();
    view.servers = vec![saved; 40];
    let mut targets = Vec::new();
    let mut landmarks = Vec::new();
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        canvas.capture_focus = true;
        super::super::play::draw(canvas, &view, [1280.0, 720.0], &HashMap::new()).unwrap();
        targets.extend(canvas.focus_targets.iter().copied());
        landmarks.extend(canvas.focus_landmarks.iter().cloned());
    });
    let offscreen = MenuAction::SelectSaved(view.servers.len() - 1);
    let target = targets
        .iter()
        .find(|target| target.action == offscreen)
        .expect("navigation keeps rows outside the pointer viewport");
    assert!(!hits.iter().any(|(action, _)| *action == offscreen));
    assert!(
        landmarks
            .iter()
            .any(|region| Some(region.id) == target.landmark)
    );
    assert!(
        targets
            .iter()
            .any(|target| target.action == MenuAction::OpenServerFilter)
    );
}

#[test]
fn hidden_sections_remove_selected_details_and_fall_back_to_visible_order() {
    let mut view = view();
    view.feeds.select_saved(0);
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::ToggleVisibility(ServerGroup::Saved));
    assert_eq!(selection(&view), Some(Selection::Featured(0)));
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        draw(
            canvas,
            &view,
            &Grid::new(canvas.rem, 1280.0),
            [20.0, 100.0, 1260.0, 700.0],
            &HashMap::new(),
        )
        .unwrap();
    });
    assert!(!hits.iter().any(|(action, _)| matches!(
        action,
        MenuAction::SelectSaved(_)
            | MenuAction::PlaySaved(_)
            | MenuAction::EditSaved(_)
            | MenuAction::RemoveSavedDialog(_)
    )));
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::ToggleVisibility(ServerGroup::Featured));
    assert_eq!(selection(&view), Some(Selection::Featured(1)));
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::ToggleVisibility(ServerGroup::Creator));
    assert_eq!(selection(&view), None);
    assert_eq!(view.feeds.selected_saved, Some(0));
}
