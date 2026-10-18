use crate::{review_tests::paint, test_support::solids};
use std::collections::HashMap;
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

/// Paints the real channel picker with a caller-provided publication view.
fn channel_picker_actions(updates: launcher::menu::updates::View) -> (u16, Vec<MenuAction>) {
    use launcher::menu::settings_options::{SETTINGS_OPTIONS, UPDATE_CHANNEL_OPTION};
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == UPDATE_CHANNEL_OPTION.name)
        .unwrap() as u16;
    let mut view = MenuView::new(true, "Player".into());
    view.settings.dropdown = Some(index);
    view.updates = updates;
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    (index, hits.into_iter().map(|(action, _)| action).collect())
}

#[test]
fn shipped_channel_picker_keeps_unpublished_stable_hidden() {
    use launcher::menu::settings_options::UPDATE_CHANNEL_OPTION;
    let SettingKind::Dropdown(choices) = UPDATE_CHANNEL_OPTION.kind else {
        unreachable!()
    };
    for published in [false, true] {
        let updates = launcher::menu::updates::View {
            stable_available: published,
            ..Default::default()
        };
        let (index, actions) = channel_picker_actions(updates);
        for (choice, metadata) in choices.iter().enumerate() {
            let action = MenuAction::SettingsOption(index, choice as i32);
            assert_eq!(actions.contains(&action), metadata.label != "Stable");
        }
    }
}

#[test]
fn a_catalog_with_stable_reveals_its_picker_action_after_verified_publication() {
    use launcher::menu::settings_options::UPDATE_CHANNEL_OPTION;
    let SettingKind::Dropdown(choices) = UPDATE_CHANNEL_OPTION.kind else {
        unreachable!()
    };
    let additional_publications = serde_json::json!({
        "stable": {
            "track": "rolling", "tag": "fixture-stable", "manifest": "fixture-stable.json"
        }
    });
    for verified in [false, true] {
        let mut updates = launcher::menu::updates::View::with_publications(|channel| {
            channel.is_published() || additional_publications.get(channel.name()).is_some()
        });
        updates.stable_available = verified;
        let (index, actions) = channel_picker_actions(updates);
        for (choice, metadata) in choices.iter().enumerate() {
            assert_eq!(
                actions.contains(&MenuAction::SettingsOption(index, choice as i32)),
                metadata.label != "Stable" || verified
            );
        }
    }
}

fn same_bounds(left: Bounds, right: Bounds) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() < 0.001)
}

#[test]
fn closed_picker_uses_the_secondary_role_and_preserves_flow_when_pressed() {
    let action = MenuAction::SettingsScalePicker;
    for (hovered, pressed) in [(false, false), (true, false), (true, true)] {
        let mut view = MenuView::new(true, "Player".into());
        view.hovered = hovered.then_some(action);
        view.pressed = pressed.then_some(action);
        let mut expected_face = [0.0; 4];
        let mut expected_outer = [0.0; 4];
        let mut expected_hit = [0.0; 4];
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            let flow = [100.0, 100.0, 500.0, 100.0 + canvas.r(SELECT_HEIGHT)];
            expected_hit = [flow[0], flow[1] - canvas.r(0.4), flow[2], flow[3]];
            let edge = canvas.r(EDGE);
            let shadow = canvas.r(if pressed { 0.0 } else { 0.4 });
            expected_outer = [flow[0], flow[1] - shadow, flow[2], flow[3]];
            expected_face = [
                expected_outer[0] + edge,
                expected_outer[1] + edge,
                expected_outer[2] - edge,
                expected_outer[3] - edge - shadow,
            ];
            assert!((expected_face[3] - expected_face[1] - canvas.r(4.2)).abs() < 0.001);
            select(canvas, &view, flow, "50%", action).unwrap();
        });
        let drawn = solids(&nodes);
        let role = theme::SECONDARY;
        let fill = if pressed {
            role.pressed
        } else if hovered {
            role.hovered
        } else {
            role.fill
        };
        assert!(
            drawn
                .iter()
                .any(|(bounds, color)| { same_bounds(*bounds, expected_face) && *color == fill })
        );
        let specular = if hovered {
            role.specular_hovered
        } else {
            role.specular
        };
        for color in specular {
            assert!(drawn.iter().any(|(_, drawn)| *drawn == color));
        }
        let bounds = hits
            .iter()
            .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
            .unwrap();
        assert!(same_bounds(
            [
                bounds.min().x(),
                bounds.min().y(),
                bounds.max().x(),
                bounds.max().y(),
            ],
            expected_hit
        ));
    }
}
