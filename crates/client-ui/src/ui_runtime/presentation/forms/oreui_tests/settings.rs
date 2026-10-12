//! Settings navigation and editing through the OreUI host without installed carriers.

use std::sync::Arc;

use super::super::super::UiPresentationRuntime;
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingsGroup};
use launcher::menu::{MenuAction, MenuView};
use oreui::test_support::{centre, option_index, settings_view};
use ui::UiPoint;
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

#[test]
fn native_video_settings_exposes_animation_choices_and_crosshair_preferences() {
    use launcher::menu::settings_options::{
        ANIMATIONS_OPTION, INVERT_CROSSHAIR_OPTION, THIRD_PERSON_CROSSHAIR_OPTION,
    };

    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("video_forced_index");
    let index = |name| {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap()
    };
    let animation = index(ANIMATIONS_OPTION.name);
    for value in [0, 1] {
        Arc::make_mut(&mut view.settings_options).set(animation, value);
        for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
            Arc::make_mut(&mut view.settings_options).set(index(option.name), value);
        }
        present(&mut presentation, &view, [1280.0, 720.0]);
        let actions: Vec<_> = presentation.visible_menu_actions().collect();
        for choice in ANIMATIONS_OPTION.min..=ANIMATIONS_OPTION.max {
            assert!(actions.contains(&MenuAction::SettingsOption(animation as u16, choice)));
        }
        for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
            assert!(
                actions.contains(&MenuAction::SettingsOption(
                    index(option.name) as u16,
                    1 - value
                )),
                "{} must remain reachable in both saved states",
                option.name,
            );
        }
    }
}

#[test]
fn video_chat_position_choices_are_clickable_in_both_saved_states() {
    use launcher::menu::settings_options::CHAT_POSITION_OPTION;

    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == CHAT_POSITION_OPTION.name)
        .unwrap();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("video_forced_index");
    for size in [[1280.0, 720.0], [1920.0, 1080.0]] {
        for value in CHAT_POSITION_OPTION.min..=CHAT_POSITION_OPTION.max {
            Arc::make_mut(&mut view.settings_options).set(index, value);
            present(&mut presentation, &view, size);
            for choice in CHAT_POSITION_OPTION.min..=CHAT_POSITION_OPTION.max {
                let action = MenuAction::SettingsOption(index as u16, choice);
                let bounds = presentation
                    .menu_hit_targets
                    .iter()
                    .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
                    .expect("Video exposes both chat placements");
                assert_eq!(presentation.hit_test_menu(centre(bounds)), Some(action));
            }
        }
    }
}

fn present(
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
    size: [f32; 2],
) -> Vec<ui::UiNode> {
    let metrics = TextMetrics::for_viewport(
        [size[0] as u32, size[1] as u32],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation.menu_hit_targets = presentation
        .append_oreui_screen(view, &mut nodes, &mut next, metrics, size, None, &|_| None)
        .unwrap()
        .expect("settings must draw through the OreUI host");
    nodes
}

#[test]
fn settings_route_draws_oreui_and_publishes_navigation_hits() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let view = settings_view("accessibility_forced_index");
    let nodes = present(&mut presentation, &view, [1280.0, 720.0]);
    assert!(!nodes.is_empty(), "settings must publish a visible screen");
    for action in [
        MenuAction::AddBack,
        MenuAction::SettingsSection(view.settings_section),
    ] {
        let target = presentation
            .menu_hit_targets
            .iter()
            .find_map(|(found, bounds)| (*found == action).then_some(*bounds))
            .expect("settings navigation must remain interactive");
        assert_eq!(presentation.hit_test_menu(centre(target)), Some(action));
    }
}

#[test]
fn settings_focus_includes_offscreen_edits_and_reveals_the_selected_control() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    let size = [1280.0, 720.0];
    present(&mut presentation, &view, size);
    let action = MenuAction::SettingsResetGroup(SettingsGroup::Accessibility);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .all(|(target, _)| *target != action),
        "the regression needs a control below the viewport"
    );
    assert!(
        presentation
            .visible_menu_actions()
            .any(|target| target == action),
        "keyboard/controller navigation must include controls below the viewport"
    );
    view.focused_action = Some(action);
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
        .expect("focusing an offscreen control must reveal it on that frame");
    assert_eq!(presentation.hit_test_menu(centre(bounds)), Some(action));
}

#[test]
fn command_macro_focus_reveals_the_entire_validation_error() {
    for size in [[1280.0, 720.0], [800.0, 450.0]] {
        for slot in [0, launcher::menu::settings_options::COMMAND_MACRO_COUNT - 1] {
            let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
            let mut view = settings_view("keyboard_and_mouse_forced_index");
            view.macro_commands = Some(core::array::from_fn(|_| String::new()));
            view.focused_action = Some(MenuAction::SettingsMacroCommand(slot));
            present(&mut presentation, &view, size);
            view.macro_commands.as_mut().unwrap()[slot] = "x".repeat(ui::MAX_CHAT_INPUT_BYTES);
            let nodes = present(&mut presentation, &view, size);
            let error = nodes
                .iter()
                .find(|node| match node.visual() {
                    ui::UiVisual::Text { layout, .. } => layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .collect::<String>()
                        .replace(' ', "")
                        .contains("Commandistoolong."),
                    _ => false,
                })
                .expect("an invalid draft shows a validation error");
            let mut top = error.bounds().min().y();
            let mut bottom = error.bounds().max().y();
            let mut parent = error.parent();
            while let Some(node) = parent.and_then(|id| nodes.iter().find(|node| node.id() == id)) {
                top += node.bounds().min().y();
                bottom += node.bounds().min().y();
                parent = node.parent();
            }
            assert!(
                top >= 0.0 && bottom <= size[1],
                "error is clipped: {top}..{bottom}"
            );
            let field = presentation
                .menu_hit_targets
                .iter()
                .find_map(|(action, bounds)| {
                    (*action == MenuAction::SettingsMacroCommand(slot)).then_some(*bounds)
                })
                .unwrap();
            assert!(presentation.scroll_menu(centre(field), 1000.0, false));
            presentation.menu_seconds += 0.2;
            present(&mut presentation, &view, size);
            assert!(
                presentation
                    .menu_hit_targets
                    .iter()
                    .all(|(action, _)| { *action != MenuAction::SettingsMacroCommand(slot) }),
                "a stable error must not undo later wheel scrolling"
            );
        }
    }
}

#[test]
fn settings_focus_returns_from_header_and_reveals_the_same_body_control() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    let size = [1280.0, 720.0];
    let action = MenuAction::SettingsResetGroup(SettingsGroup::Accessibility);
    view.focused_action = Some(action);
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
        .expect("focusing the body control must reveal it");
    view.focused_action = Some(MenuAction::AddBack);
    present(&mut presentation, &view, size);
    assert!(presentation.scroll_menu(centre(bounds), 1000.0, false));
    presentation.menu_seconds += 0.2;
    present(&mut presentation, &view, size);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .all(|(target, _)| *target != action)
    );
    view.focused_action = Some(action);
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
        .expect("returning from the header must reveal the previous body control");
    assert_eq!(presentation.hit_test_menu(centre(bounds)), Some(action));
}

#[test]
fn settings_header_focus_preserves_a_manually_scrolled_narrow_sidebar() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    let size = [640.0, 360.0];
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(action, bounds)| {
            matches!(action, MenuAction::SettingsSection(_)).then_some(*bounds)
        })
        .expect("the sidebar must accept scrolling");
    assert!(presentation.scroll_menu(centre(bounds), -1000.0, false));
    presentation.menu_seconds += 0.2;
    present(&mut presentation, &view, size);
    let offsets = presentation.menu_scrolls.offsets().clone();
    assert!(offsets.values().any(|offset| *offset > 0.0));
    view.focused_action = Some(MenuAction::AddBack);
    present(&mut presentation, &view, size);
    assert_eq!(presentation.menu_scrolls.offsets(), &offsets);
    view.focused_action = Some(MenuAction::SettingsSection(view.settings_section));
    present(&mut presentation, &view, size);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .any(|(action, _)| { *action == MenuAction::SettingsSection(view.settings_section) }),
        "returning from the header must reveal the first category"
    );
}

#[test]
fn settings_picker_close_focus_preserves_the_scrolled_choice_list() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("creator_forced_index");
    let index = option_index("content_log_gui_level");
    let option = &SETTINGS_OPTIONS[index];
    view.settings_dropdown = Some(index as u16);
    view.focused_action = Some(MenuAction::SettingsOption(index as u16, option.max));
    let size = [640.0, 120.0];
    present(&mut presentation, &view, size);
    let offsets = presentation.menu_scrolls.offsets().clone();
    assert!(
        offsets.values().any(|offset| *offset > 0.0),
        "the regression needs an overflowing picker"
    );
    view.focused_action = Some(MenuAction::SettingsDropdown(index as u16));
    present(&mut presentation, &view, size);
    assert_eq!(presentation.menu_scrolls.offsets(), &offsets);
}

#[test]
fn settings_slider_capture_clamps_outside_the_bar_on_both_axes() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let view = settings_view("video_forced_index");
    present(&mut presentation, &view, [1280.0, 720.0]);
    let index = option_index("field_of_view");
    let option = &SETTINGS_OPTIONS[index];
    for (x, y, expected) in [(-1000.0, -1000.0, option.min), (4000.0, 4000.0, option.max)] {
        assert_eq!(
            presentation.settings_slider_drag_action(index as u16, UiPoint::new(x, y).unwrap()),
            Some(MenuAction::SettingsOption(index as u16, expected)),
        );
    }
    assert_eq!(
        presentation.settings_slider_drag_action(
            option_index("main_volume") as u16,
            UiPoint::new(600.0, 600.0).unwrap()
        ),
        None,
        "a slider in another section cannot receive the capture",
    );
}

#[test]
fn settings_scale_choices_do_not_publish_slider_capture_geometry() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    view.gui_scale_choices = ui::DesktopGuiScale::for_window([1280, 720])
        .choices()
        .collect();
    view.focused_action = Some(MenuAction::SettingsScale(-1));
    present(&mut presentation, &view, [1280.0, 720.0]);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .any(|(action, _)| *action == MenuAction::SettingsScale(-1))
    );
    for point in [
        UiPoint::new(-1000.0, -1000.0).unwrap(),
        UiPoint::new(4000.0, 4000.0).unwrap(),
    ] {
        assert_eq!(
            presentation.gui_scale_drag_action(point),
            None,
            "native option buttons never acquire slider capture"
        );
    }
    assert_eq!(presentation.gui_scale_slider_track(), None);
}
