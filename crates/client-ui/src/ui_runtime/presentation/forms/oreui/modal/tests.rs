use super::super::review_tests::{paint, solids};
use std::collections::HashMap;
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

fn option_modal() -> Modal<'static> {
    Modal {
        title: "UI scale",
        items: vec![
            MenuItem {
                label: "50%",
                picture_slot: false,
                picture: None,
                selected: false,
                enabled: true,
                action: Some(MenuAction::SettingsScale(-1)),
            },
            MenuItem {
                label: "100%",
                picture_slot: false,
                picture: None,
                selected: true,
                enabled: true,
                action: Some(MenuAction::SettingsScale(0)),
            },
        ],
        body: Cow::Borrowed(""),
        body_color: TEXT,
        buttons: Vec::new(),
        close: Some(MenuAction::SettingsScalePicker),
    }
}

#[test]
fn picker_overlaps_the_bottom_border_without_changing_other_modals() {
    let view = MenuView::new(true, "Player".into());
    let mut result = Vec::new();
    let mut edge = 0.0;
    for picker in [false, true] {
        let (areas, _, nodes) = paint(HashMap::new(), |canvas| {
            edge = canvas.r(EDGE);
            if picker {
                draw_picker(canvas, &view, [1280.0, 720.0], &option_modal()).unwrap();
            } else {
                draw(canvas, &view, [1280.0, 720.0], &option_modal()).unwrap();
            }
        });
        let drawn = solids(&nodes);
        let panel = drawn
            .iter()
            .find_map(|(bounds, color)| (*color == BORDER).then_some(*bounds))
            .unwrap();
        let viewport_bottom = areas
            .iter()
            .find(|area| area.key == SCROLL)
            .unwrap()
            .viewport
            .max()
            .y();
        let list_closing = drawn
            .iter()
            .filter(|(bounds, color)| {
                *color == MENU_ITEM.border && (bounds[3] - bounds[1] - edge).abs() < 0.001
            })
            .map(|(bounds, _)| bounds[1])
            .reduce(f32::max)
            .unwrap();
        assert!(list_closing < viewport_bottom);
        result.push((panel, viewport_bottom));
    }
    let [(general, general_bottom), (picker, picker_bottom)] = result.as_slice() else {
        unreachable!();
    };
    assert!((general[3] - general[1] - (picker[3] - picker[1]) - edge).abs() < 0.001);
    assert!((general[3] - general_bottom - edge).abs() < 0.001);
    assert!((picker[3] - picker_bottom).abs() < 0.001);
}

#[test]
fn gamepad_picker_keeps_back_dismissal_but_hides_the_pointer_close_button() {
    let close = MenuAction::SettingsScalePicker;
    for gamepad in [false, true] {
        let mut view = MenuView::new(true, "Player".into());
        view.gamepad_input = gamepad;
        let mut focus = Vec::new();
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            canvas.capture_focus = true;
            draw_picker(canvas, &view, [1280.0, 720.0], &option_modal()).unwrap();
            focus.extend(canvas.focus_hits.iter().map(|(action, _)| *action));
        });
        let panel = solids(&nodes)
            .iter()
            .find_map(|(bounds, color)| (*color == BORDER).then_some(*bounds))
            .unwrap();
        let inside_close = hits
            .iter()
            .filter(|(action, bounds)| {
                *action == close
                    && bounds.min().x() >= panel[0]
                    && bounds.min().y() >= panel[1]
                    && bounds.max().x() <= panel[2]
                    && bounds.max().y() <= panel[3]
            })
            .count();
        assert_eq!(inside_close, usize::from(!gamepad));
        assert_eq!(focus.contains(&close), !gamepad);
        assert!(hits.iter().any(|(action, bounds)| {
            *action == close && bounds.contains(ui::UiPoint::new(1.0, 1.0).unwrap())
        }));
        assert!(focus.contains(&MenuAction::SettingsScale(-1)));
        assert!(focus.contains(&MenuAction::SettingsScale(0)));
    }
}
