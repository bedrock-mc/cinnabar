use ServerGroup::{Creator, Featured, Saved};
use {super::*, launcher::menu::MenuAction};

fn point(x: f32, y: f32) -> Option<UiPoint> {
    Some(UiPoint::new(x, y).unwrap())
}

fn layout(state: &mut DragState) {
    state.begin_layout([10.0, 10.0, 300.0, 400.0]);
    for (group, top, bottom) in [
        (Featured, 30.0, 180.0),
        (Creator, 180.0, 300.0),
        (Saved, 300.0, 380.0),
    ] {
        state.section(group, [10.0, top, 300.0, top + 30.0], bottom);
    }
}

#[test]
fn header_click_toggles_on_release_but_drag_moves_without_toggling() {
    let mut state = DragState::default();
    layout(&mut state);
    assert_eq!(
        state.pointer(point(50.0, 310.0), true, true, 0.0),
        (true, None, 0.0)
    );
    assert_eq!(
        state.pointer(point(52.0, 310.0), false, false, 0.1).1,
        Some(MenuAction::ServerList(ServerListAction::Toggle(Saved)))
    );
    state.pointer(point(50.0, 310.0), true, true, 0.2);
    assert_eq!(state.pointer(point(50.0, 40.0), true, false, 0.3).1, None);
    assert_eq!(state.preview().unwrap().marker, Some(30.0));
    assert_eq!(state.dragged_group(), Some(Saved));
    state.end_frame();
    layout(&mut state);
    assert_eq!(
        state.pointer(point(50.0, 40.0), false, false, 0.4).1,
        Some(MenuAction::ServerList(ServerListAction::MoveBefore(
            Saved,
            Some(Featured)
        )))
    );
    assert!(state.preview().is_none());
}

#[test]
fn drop_between_sections_or_at_bottom_preserves_the_chosen_slot() {
    let mut state = DragState::default();
    layout(&mut state);
    state.pointer(point(50.0, 310.0), true, true, 0.0);
    assert_eq!(
        state.pointer(point(50.0, 190.0), false, false, 0.1).1,
        Some(MenuAction::ServerList(ServerListAction::MoveBefore(
            Saved,
            Some(Creator)
        )))
    );
    state.pointer(point(50.0, 40.0), true, true, 0.2);
    assert_eq!(
        state.pointer(point(50.0, 360.0), false, false, 0.3).1,
        Some(MenuAction::ServerList(ServerListAction::MoveBefore(
            Featured, None
        )))
    );
}

#[test]
fn drag_cancels_outside_viewport_on_unmount_or_loss_of_pointer() {
    let mut state = DragState::default();
    layout(&mut state);
    state.pointer(point(50.0, 310.0), true, true, 0.0);
    assert_eq!(
        state.pointer(point(350.0, 40.0), false, false, 0.1),
        (true, None, 0.0)
    );
    state.pointer(point(50.0, 310.0), true, true, 0.2);
    assert_eq!(state.pointer(None, true, false, 0.3), (true, None, 0.0));
    state.pointer(point(50.0, 310.0), true, true, 0.4);
    state.end_frame();
    state.end_frame();
    assert_eq!(
        state.pointer(point(50.0, 40.0), false, false, 0.5),
        (false, None, 0.0)
    );
    layout(&mut state);
    assert_eq!(
        state.pointer(point(50.0, 100.0), true, true, 0.6),
        (false, None, 0.0)
    );
}

#[test]
fn dragging_near_viewport_edges_scrolls_and_tracks_new_header_geometry() {
    let mut state = DragState::default();
    layout(&mut state);
    state.pointer(point(50.0, 190.0), true, true, 0.0);
    assert!(state.pointer(point(50.0, 390.0), true, false, 0.02).2 < 0.0);
    assert!(state.pointer(point(50.0, 15.0), true, false, 0.04).2 > 0.0);
    layout(&mut state);
    assert_eq!(state.dragged_group(), Some(Creator));
    assert_eq!(state.preview().unwrap().bounds[1], 10.0);
}
