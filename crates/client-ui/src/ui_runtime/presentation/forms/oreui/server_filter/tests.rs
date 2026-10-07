use super::super::review_tests::paint;
use super::*;
use launcher::menu::server_list::ServerGroup;
use std::collections::HashMap;

#[test]
fn filter_exposes_all_sections_and_only_available_adjacent_moves() {
    let mut view = MenuView::new(true, "Fixture".into());
    for reordered in [false, true] {
        if reordered {
            std::sync::Arc::make_mut(&mut view.settings_options).apply_server_list(
                ServerListAction::MoveBefore(ServerGroup::Saved, Some(ServerGroup::Featured)),
            );
        }
        let mut focus = Vec::new();
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, [1280.0, 720.0]).unwrap();
            focus.extend(canvas.focus_hits.iter().map(|(action, _)| *action));
        });
        for group in view.settings_options.server_list().order() {
            let toggle = MenuAction::ServerList(ServerListAction::ToggleVisibility(group));
            assert!(hits.iter().any(|(action, _)| *action == toggle));
            assert!(focus.contains(&toggle));
            for down in [false, true] {
                if let Some(action) = view.settings_options.server_list().move_action(group, down) {
                    assert!(
                        hits.iter()
                            .any(|(hit, _)| *hit == MenuAction::ServerList(action))
                    );
                    assert!(focus.contains(&MenuAction::ServerList(action)));
                }
            }
        }
        assert_eq!(hits.len(), 8);
        assert!(focus.contains(&MenuAction::DismissDialog));
        assert!(!hits.iter().any(|(action, _)| matches!(
            action,
            MenuAction::PlayFeatured(_) | MenuAction::SelectSaved(_)
        )));
    }
}

#[test]
fn short_filter_keeps_done_fixed_and_reveals_the_focused_lower_section() {
    let mut view = MenuView::new(true, "Fixture".into());
    let font = crate::ui_runtime::presentation::tests::fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let size = [800.0, 200.0];
    let (mut nodes, mut next) = (Vec::new(), 1);
    let hits = presentation
        .append_oreui_server_filter(&view, &mut nodes, &mut next, metrics, size)
        .unwrap();
    let done = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::DismissDialog)
        .unwrap()
        .1;
    assert!(done.min().y() >= 0.0 && done.max().y() <= size[1]);
    let lower = MenuAction::ServerList(ServerListAction::ToggleVisibility(ServerGroup::Saved));
    assert!(!hits.iter().any(|(action, _)| *action == lower));
    view.focused_action = Some(lower);
    nodes.clear();
    next = 1;
    let hits = presentation
        .append_oreui_server_filter(&view, &mut nodes, &mut next, metrics, size)
        .unwrap();
    let revealed = hits
        .iter()
        .find(|(action, _)| *action == lower)
        .expect("focused section scrolls into view")
        .1;
    assert!(revealed.min().y() >= 0.0 && revealed.max().y() <= done.min().y());
    assert!(presentation.menu_scrolls.offsets()[SCROLL] > 0.0);
    assert_eq!(
        hits.iter()
            .find(|(action, _)| *action == MenuAction::DismissDialog)
            .unwrap()
            .1,
        done
    );
    let offset = presentation.menu_scrolls.offsets()[SCROLL];
    view.focused_action = Some(MenuAction::DismissDialog);
    nodes.clear();
    next = 1;
    presentation
        .append_oreui_server_filter(&view, &mut nodes, &mut next, metrics, size)
        .unwrap();
    assert_eq!(presentation.menu_scrolls.offsets()[SCROLL], offset);
}
