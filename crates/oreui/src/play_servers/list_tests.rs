use super::super::{motion::Surface, transitions::Transitions};
use crate::{review_tests::paint, test_support::solids};
use launcher::menu::server_list::{ServerGroup, ServerListAction};
use {
    super::*,
    launcher::menu::{MenuAction, MenuServerCard, MenuView},
};

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
        launcher::menu::ServerDetails {
            group: "creator".into(),
            ..Default::default()
        },
    );
    view.servers = vec![launcher::menu::SavedServer {
        name: "Home".into(),
        address: "home.test:19132".into(),
        favorite: false,
        last_joined_unix: 0,
    }]
    .into();
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
        let font = ui_engine::test_support::fixture_font();
        let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
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
fn server_list_defaults_to_custom_rows_first_and_reorders_without_changing_actions() {
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
    let (_, hits, _) = render(&view);
    let top = |action| hits.iter().find(|(a, _)| *a == action).unwrap().1.min().y();
    assert!(top(MenuAction::SelectSaved(0)) < top(MenuAction::SelectFeatured(0)));
    assert!(top(MenuAction::SelectSaved(0)) < top(MenuAction::SelectFeatured(1)));
    assert!(
        std::sync::Arc::make_mut(&mut view.settings_options).apply_server_list(
            ServerListAction::MoveBefore(ServerGroup::Featured, Some(ServerGroup::Saved)),
        )
    );
    let (_, hits, _) = render(&view);
    let top = |action| hits.iter().find(|(a, _)| *a == action).unwrap().1.min().y();
    assert!(top(MenuAction::SelectFeatured(0)) < top(MenuAction::SelectSaved(0)));
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
fn every_server_category_header_has_a_muted_grip_that_brightens_on_hover() {
    let mut view = view();
    std::sync::Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Saved));
    for hovered in std::iter::once(None).chain(ServerGroup::ALL.into_iter().map(Some)) {
        view.hovered = hovered.map(|group| MenuAction::ServerList(ServerListAction::Toggle(group)));
        let mut dot = 0.0;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            dot = canvas.r(super::super::theme::EDGE);
            draw(
                canvas,
                &view,
                &Grid::new(canvas.rem, 1280.0),
                [20.0, 100.0, 1260.0, 700.0],
                &HashMap::new(),
            )
            .unwrap();
        });
        let fills = solids(&nodes);
        for group in ServerGroup::ALL {
            let action = MenuAction::ServerList(ServerListAction::Toggle(group));
            let headers: Vec<_> = hits.iter().filter(|(a, _)| *a == action).collect();
            assert_eq!(headers.len(), 1, "{group:?} keeps one click/drag target");
            let header = headers[0].1;
            let dots: Vec<_> = fills
                .iter()
                .filter(|(b, _)| {
                    b[0] >= header.min().x()
                        && b[1] >= header.min().y()
                        && b[2] <= header.max().x()
                        && b[3] <= header.max().y()
                        && (b[2] - b[0] - dot).abs() < 0.01
                        && (b[3] - b[1] - dot).abs() < 0.01
                })
                .collect();
            assert_eq!(dots.len(), 6, "{group:?} has a six-dot grip");
            let color = if hovered == Some(group) {
                TEXT_DIMMER
            } else {
                super::super::theme::TEXT_DIMMEST
            };
            assert!(dots.iter().all(|(_, c)| *c == color));
            let mut columns: Vec<_> = dots.iter().map(|(b, _)| b[0]).collect();
            let mut rows: Vec<_> = dots.iter().map(|(b, _)| b[1]).collect();
            columns.sort_by(f32::total_cmp);
            rows.sort_by(f32::total_cmp);
            columns.dedup();
            rows.dedup();
            assert_eq!(columns.len(), 2);
            assert_eq!(rows.len(), 3);
        }
    }
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
        let font = ui_engine::test_support::fixture_font();
        let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
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
