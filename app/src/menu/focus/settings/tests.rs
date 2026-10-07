use super::*;
use ui::UiPoint;

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
    UiRect::new(
        UiPoint::new(left, top).unwrap(),
        UiPoint::new(right, bottom).unwrap(),
    )
    .unwrap()
}

fn target(action: MenuAction, bounds: UiRect, landmark: Option<u16>) -> SettingsFocusTarget {
    SettingsFocusTarget {
        action,
        bounds,
        landmark,
    }
}

fn group(id: u16, parent: Option<u16>, bounds: UiRect) -> SettingsFocusLandmark {
    SettingsFocusLandmark {
        id,
        parent,
        bounds,
        scroll_axis: None,
        delegate: None,
        delegate_landmark: None,
        remember: false,
        trap: false,
        focus_control_disabled: false,
    }
}

#[test]
fn vertical_navigation_skips_inline_siblings_and_retains_its_column_through_wide_controls() {
    let mut geometry = SettingsFocusGeometry::default();
    let mut targets = Vec::new();
    for index in 0..3 {
        let left = f32::from(index) * 10.0;
        targets.push(target(
            MenuAction::SettingsLanguage(index),
            rect(left, 0.0, left + 10.0, 10.0),
            None,
        ));
    }
    targets.push(target(
        MenuAction::AddBack,
        rect(0.0, 20.0, 30.0, 30.0),
        None,
    ));
    for index in 0..3 {
        let left = f32::from(index) * 10.0;
        targets.push(target(
            MenuAction::SettingsLanguage(index + 3),
            rect(left, 40.0, left + 10.0, 50.0),
            None,
        ));
    }
    geometry.update(&targets, &[]);
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(2),
            SettingsFocusAxis::Vertical,
            1
        ),
        Some(MenuAction::AddBack)
    );
    assert_eq!(
        geometry.directional(MenuAction::AddBack, SettingsFocusAxis::Vertical, 1),
        Some(MenuAction::SettingsLanguage(5))
    );
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(5),
            SettingsFocusAxis::Vertical,
            1
        ),
        Some(MenuAction::SettingsLanguage(5)),
        "directional navigation stops at the edge"
    );
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(5),
            SettingsFocusAxis::Horizontal,
            -1
        ),
        Some(MenuAction::SettingsLanguage(4))
    );
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(4),
            SettingsFocusAxis::Vertical,
            -1
        ),
        Some(MenuAction::AddBack)
    );
    assert_eq!(
        geometry.directional(MenuAction::AddBack, SettingsFocusAxis::Vertical, -1),
        Some(MenuAction::SettingsLanguage(1)),
        "changing axes starts a new perpendicular anchor"
    );
}

#[test]
fn leaving_a_panel_delegates_into_the_foreign_panel_instead_of_picking_its_nearest_child() {
    let mut geometry = SettingsFocusGeometry::default();
    let root = group(0, None, rect(0.0, 0.0, 100.0, 100.0));
    let mut sidebar = group(1, Some(0), rect(0.0, 20.0, 30.0, 100.0));
    sidebar.delegate = Some(MenuAction::SettingsLanguage(2));
    let body = group(2, Some(0), rect(50.0, 20.0, 100.0, 100.0));
    let targets = [
        target(
            MenuAction::SettingsLanguage(1),
            rect(0.0, 40.0, 30.0, 50.0),
            Some(1),
        ),
        target(
            MenuAction::SettingsLanguage(2),
            rect(0.0, 70.0, 30.0, 80.0),
            Some(1),
        ),
        target(
            MenuAction::SettingsLanguage(3),
            rect(50.0, 40.0, 60.0, 50.0),
            Some(2),
        ),
    ];
    geometry.update(&targets, &[root, sidebar, body]);
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(3),
            SettingsFocusAxis::Horizontal,
            -1
        ),
        Some(MenuAction::SettingsLanguage(2))
    );
    sidebar.remember = true;
    geometry.update(&targets, &[root, sidebar, body]);
    geometry.remember(MenuAction::SettingsLanguage(1));
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(3),
            SettingsFocusAxis::Horizontal,
            -1
        ),
        Some(MenuAction::SettingsLanguage(1)),
        "panel memory precedes its selected alias"
    );
}

#[test]
fn same_scroll_region_wins_and_unclipped_targets_remain_reachable() {
    let mut geometry = SettingsFocusGeometry::default();
    let root = group(0, None, rect(0.0, 0.0, 100.0, 100.0));
    let mut body = group(1, Some(0), rect(40.0, 0.0, 100.0, 30.0));
    body.scroll_axis = Some(SettingsFocusAxis::Vertical);
    let foreign = group(2, Some(0), rect(0.0, 30.0, 30.0, 50.0));
    let targets = [
        target(
            MenuAction::SettingsLanguage(1),
            rect(40.0, 10.0, 60.0, 20.0),
            Some(1),
        ),
        target(
            MenuAction::SettingsLanguage(2),
            rect(40.0, 100.0, 60.0, 110.0),
            Some(1),
        ),
        target(
            MenuAction::SettingsLanguage(3),
            rect(0.0, 30.0, 30.0, 50.0),
            Some(2),
        ),
    ];
    geometry.update(&targets, &[root, body, foreign]);
    assert_eq!(
        geometry.directional(
            MenuAction::SettingsLanguage(1),
            SettingsFocusAxis::Vertical,
            1
        ),
        Some(MenuAction::SettingsLanguage(2))
    );
}

#[test]
fn initial_entry_uses_the_content_alias_before_the_header_and_selected_sidebar_before_first_item() {
    let mut geometry = SettingsFocusGeometry::default();
    let mut root = group(0, None, rect(0.0, 0.0, 100.0, 100.0));
    root.delegate_landmark = Some(2);
    let header = group(1, Some(0), rect(0.0, 0.0, 100.0, 10.0));
    let content = group(2, Some(0), rect(0.0, 10.0, 100.0, 100.0));
    let mut sidebar = group(3, Some(2), rect(0.0, 10.0, 30.0, 100.0));
    sidebar.delegate = Some(MenuAction::SettingsLanguage(2));
    let body = group(4, Some(2), rect(30.0, 10.0, 100.0, 100.0));
    geometry.update(
        &[
            target(MenuAction::AddBack, header.bounds, Some(1)),
            target(
                MenuAction::SettingsLanguage(1),
                rect(0.0, 10.0, 30.0, 20.0),
                Some(3),
            ),
            target(
                MenuAction::SettingsLanguage(2),
                rect(0.0, 20.0, 30.0, 30.0),
                Some(3),
            ),
            target(MenuAction::SettingsLanguage(3), body.bounds, Some(4)),
        ],
        &[root, header, content, sidebar, body],
    );
    assert_eq!(geometry.entry(), Some(MenuAction::SettingsLanguage(2)));
}

#[test]
fn picker_memory_skips_non_delegating_scroll_groups_and_expires_when_the_picker_unmounts() {
    let mut geometry = SettingsFocusGeometry::default();
    let mut root = group(7, None, rect(0.0, 0.0, 100.0, 100.0));
    root.trap = true;
    root.remember = true;
    root.delegate = Some(MenuAction::SettingsLanguage(2));
    let mut list = group(8, Some(7), rect(0.0, 10.0, 100.0, 100.0));
    list.focus_control_disabled = true;
    let mut scroll = group(9, Some(8), list.bounds);
    scroll.focus_control_disabled = true;
    scroll.scroll_axis = Some(SettingsFocusAxis::Vertical);
    let targets = [
        target(
            MenuAction::SettingsLanguage(1),
            rect(0.0, 10.0, 100.0, 20.0),
            Some(9),
        ),
        target(
            MenuAction::SettingsLanguage(2),
            rect(0.0, 20.0, 100.0, 30.0),
            Some(9),
        ),
    ];
    geometry.update(&targets, &[root, list, scroll]);
    assert_eq!(geometry.entry(), Some(MenuAction::SettingsLanguage(2)));
    geometry.remember(MenuAction::SettingsLanguage(1));
    assert_eq!(geometry.entry(), Some(MenuAction::SettingsLanguage(1)));
    geometry.update(&[], &[]);
    geometry.update(&targets, &[root, list, scroll]);
    assert_eq!(geometry.entry(), Some(MenuAction::SettingsLanguage(2)));
}
