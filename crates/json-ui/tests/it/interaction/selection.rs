//! Transfers between edit and slider components keep one consistent selection owner.

use json_ui::ScreenEvent;
use serde_json::json;

use super::harness::*;

#[test]
fn selecting_another_edit_cleans_up_the_previous_box_in_either_hit_order() {
    for reverse in [false, true] {
        let mut first = edit_box(json!({"text_box_name":"first"}));
        first.name = "first".into();
        let mut second = edit_box(json!({"text_box_name":"second", "offset":[0,30]}));
        second.name = "second".into();
        let mut controls = vec![first, second];
        if reverse {
            controls.reverse();
        }
        let mut screen = Screen::new(page(controls));
        let regions = screen.regions();
        let first = regions
            .iter()
            .find(|region| region.control_name.as_deref() == Some("first"))
            .unwrap();
        let second = regions
            .iter()
            .find(|region| region.control_name.as_deref() == Some("second"))
            .unwrap();
        let meta = first.widget.edit.as_ref().unwrap();
        let target = &meta.text_target.as_ref().unwrap().0;
        let hint = meta.placeholder.as_ref().unwrap();
        screen.click([5.0, 5.0]);
        assert_eq!(screen.view.components.selected(), Some(first.key.as_str()));

        screen.hover([5.0, 35.0]);
        for down in [true, false] {
            let dispatch = screen.press(
                "button.menu_select",
                down,
                [5.0, 35.0],
                if down { 1.0 } else { 1.1 },
            );
            assert_eq!(screen.view.components.selected(), Some(second.key.as_str()));
            assert_eq!(
                screen.view.components.bag(&first.key).unwrap()["#text_edit_selected"],
                json!(false)
            );
            assert_eq!(
                screen.view.components.bag(target).unwrap()["#text_edit_selected"],
                json!(false)
            );
            assert_eq!(
                screen.view.components.bag(target).unwrap()["#text_edit_caret"],
                json!(false)
            );
            assert_eq!(
                screen.view.components.bag(hint).unwrap()["visible"],
                json!(true)
            );
            assert_eq!(
                screen.view.components.bag(&second.key).unwrap()["#text_edit_selected"],
                json!(true)
            );
            assert_eq!(
                dispatch
                    .events
                    .iter()
                    .filter(|event| matches!(event,
                        ScreenEvent::TextEditSelected {key,selected:false} if key == &first.key
                    ))
                    .count(),
                usize::from(down)
            );
            assert_eq!(
                dispatch
                    .events
                    .iter()
                    .filter(|event| matches!(event,
                        ScreenEvent::TextEdit {key,finished:true,..} if key == &first.key
                    ))
                    .count(),
                usize::from(down)
            );
        }
        let regions = screen.regions();
        screen.dispatcher.tick(&regions, &mut screen.view, 1.0);
        assert_eq!(
            screen.view.components.bag(target).unwrap()["#text_edit_caret"],
            json!(false)
        );
    }
}

#[test]
fn edit_and_slider_selection_transfer_through_buttons_and_hover() {
    for hover in [false, true] {
        let mut screen = Screen::new(page(vec![
            edit_box(json!({})),
            slider(
                json!({
                    "offset":[0,40], "slider_select_on_hover":hover,
                    "slider_selected_button":"button.choose",
                    "button_mappings":[{"from_button_id":"button.menu_ok","to_button_id":"button.choose","mapping_type":"pressed"}]
                }),
                vec![],
            ),
        ]));
        let regions = screen.regions();
        let edit = regions
            .iter()
            .find(|region| region.widget.edit.is_some())
            .unwrap();
        let slider = regions
            .iter()
            .find(|region| region.widget.slider.is_some())
            .unwrap();
        screen.click([5.0, 5.0]);
        let mut selected = screen.hover([5.0, 45.0]);
        if !hover {
            selected = screen.press("button.menu_ok", true, [5.0, 45.0], 1.0);
            screen.press("button.menu_ok", false, [5.0, 45.0], 1.1);
        }
        assert_eq!(screen.view.components.selected(), Some(slider.key.as_str()));
        assert_eq!(
            screen.view.components.bag(&slider.key).unwrap()["#slider_box_selected"],
            json!(true)
        );
        assert_eq!(
            screen.view.components.bag(&edit.key).unwrap()["#text_edit_selected"],
            json!(false)
        );
        assert!(selected.events.iter().any(|event| matches!(event,
            ScreenEvent::TextEditSelected {key,selected:false} if key == &edit.key
        )));
        screen.click([5.0, 5.0]);
        assert_eq!(screen.view.components.selected(), Some(edit.key.as_str()));
        assert_eq!(
            screen.view.components.bag(&slider.key).unwrap()["#slider_box_selected"],
            json!(false)
        );
    }
}
