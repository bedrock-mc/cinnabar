use super::super::{Content, services};
use crate::ui_runtime::presentation::forms::oreui::review_tests::{paint, solids};
use std::collections::HashMap;
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

#[test]
fn external_actions_reserve_the_native_icon_and_label_gap() {
    use launcher::menu::settings_support::{SupportAction, SupportLink};
    let view = MenuView::new(true, "Player".into());
    let external = MenuAction::SettingsSupport(SupportAction::Open(SupportLink::Help));
    let mut widths = Vec::new();
    let mut icon_and_gap = 0.0;
    for action in [MenuAction::SettingsResetChat, external] {
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            icon_and_gap = canvas.r(3.2);
            let span = [canvas.r(1.0), canvas.r(120.0)];
            let mut content = Content {
                canvas,
                view: &view,
                span,
                column_width: span[1] - span[0],
                y: 100.0,
                translate: &|_| None,
                nested: false,
            };
            services::action_row(&mut content, "", "", "Open the help center", Some(action))
                .unwrap();
        });
        let bounds = hits
            .iter()
            .find_map(|(at, bounds)| (*at == action).then_some(bounds))
            .unwrap();
        widths.push(bounds.max().x() - bounds.min().x());
    }
    assert!((widths[1] - widths[0] - icon_and_gap).abs() < 0.001);
}

#[test]
fn action_rows_keep_the_native_minimum_button_width_and_elevation_height() {
    let view = MenuView::new(true, "Player".into());
    let action = MenuAction::SettingsResetChat;
    let mut expected = [0.0; 2];
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        let span = [canvas.r(10.0), canvas.r(100.0)];
        expected = [canvas.r(ACTION_MIN_WIDTH), canvas.r(ACTION_HEIGHT)];
        let start = 100.0;
        let mut content = Content {
            canvas,
            view: &view,
            span,
            column_width: span[1] - span[0],
            y: start,
            translate: &|_| None,
            nested: false,
        };
        services::action_row(&mut content, "Reset settings", "", "Reset", Some(action)).unwrap();
        assert!((content.y - start - content.canvas.r(ACTION_HEIGHT + 2.4)).abs() < 0.001);
    });
    let bounds = hits
        .iter()
        .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
        .unwrap();
    assert!((bounds.max().x() - bounds.min().x() - expected[0]).abs() < 0.001);
    assert!((bounds.max().y() - bounds.min().y() - expected[1]).abs() < 0.001);
}

#[test]
fn compact_binding_reset_uses_role_speculars_and_moves_down_when_pressed() {
    let action = MenuAction::SettingsResetKey(0);
    let mut extents = Vec::new();
    let mut faces = Vec::new();
    let mut descent = 0.0;
    for pressed in [false, true] {
        let mut view = MenuView::new(true, "Player".into());
        view.pressed = pressed.then_some(action);
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            descent = canvas.r(0.4);
            let size = canvas.r(4.4);
            binding_reset(
                canvas,
                &view,
                [100.0, 100.0, 100.0 + size, 100.0 + size],
                action,
            )
            .unwrap();
        });
        let drawn = solids(&nodes);
        assert!(
            drawn
                .iter()
                .any(|(_, color)| *color == theme::SECONDARY.specular[0])
        );
        assert!(
            drawn
                .iter()
                .any(|(_, color)| *color == theme::SECONDARY.specular[1])
        );
        faces.push(
            drawn
                .iter()
                .find_map(|(bounds, color)| {
                    (*color
                        == if pressed {
                            theme::SECONDARY.pressed
                        } else {
                            theme::SECONDARY.fill
                        })
                    .then_some(*bounds)
                })
                .unwrap(),
        );
        let bounds = hits
            .iter()
            .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
            .unwrap();
        extents.push(bounds);
    }
    assert!((faces[1][1] - faces[0][1] - descent).abs() < 0.001);
    assert_eq!(extents[0], extents[1]);
}
