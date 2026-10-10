use super::*;

/// Installs native row bounds and refreshes the menu focus geometry.
fn native_rows(menu: &mut MenuRuntime, rows: &[(MenuAction, [f32; 4])]) {
    let rect = |[left, top, right, bottom]: [f32; 4]| {
        ui::UiRect::new(
            ui::UiPoint::new(left, top).unwrap(),
            ui::UiPoint::new(right, bottom).unwrap(),
        )
        .unwrap()
    };
    let targets = rows
        .iter()
        .map(|(action, bounds)| SettingsFocusTarget {
            action: *action,
            bounds: rect(*bounds),
            landmark: Some(0),
        })
        .collect::<Vec<_>>();
    let landmarks = [SettingsFocusLandmark {
        id: 0,
        parent: None,
        bounds: rect([0.0, 0.0, 100.0, 100.0]),
        scroll_axis: None,
        delegate: None,
        delegate_landmark: None,
        remember: false,
        trap: false,
        focus_control_disabled: false,
    }];
    menu.refresh_settings_focus_geometry(&targets, &landmarks);
}

#[test]
fn native_inline_options_navigate_without_commit_and_keep_each_choice_focus() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.screen = MenuScreen::Settings;
    let index = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "third_person")
        .unwrap() as u16;
    let original = menu.settings_options.get(usize::from(index));
    let rows = [
        (MenuAction::AddBack, [0.0, 0.0, 90.0, 10.0]),
        (
            MenuAction::SettingsOption(index, 0),
            [0.0, 20.0, 30.0, 30.0],
        ),
        (
            MenuAction::SettingsOption(index, 1),
            [30.0, 20.0, 60.0, 30.0],
        ),
        (
            MenuAction::SettingsOption(index, 2),
            [60.0, 20.0, 90.0, 30.0],
        ),
    ];
    native_rows(&mut menu, &rows);
    menu.focus_pointer(MenuAction::SettingsOption(index, 0));
    menu.move_horizontal_focus(1);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsOption(index, 1)
    );
    assert_eq!(menu.settings_options.get(usize::from(index)), original);
    menu.activate_focused();
    assert_eq!(menu.settings_options.get(usize::from(index)), 1);
    native_rows(&mut menu, &rows);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsOption(index, 1)
    );
    menu.focus_pointer(MenuAction::SettingsOption(index, 2));
    menu.move_directional_focus(SettingsFocusAxis::Vertical, -1);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::AddBack,
        "Up leaves the row instead of stepping to an inline sibling"
    );
    menu.move_directional_focus(SettingsFocusAxis::Vertical, -1);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::AddBack,
        "directional navigation does not wrap"
    );
}

#[test]
fn native_gui_scale_choices_commit_only_on_select() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.screen = MenuScreen::Settings;
    menu.sync_gui_scale(
        0,
        ui::DesktopGuiScale::for_window([1920, 1080])
            .choices()
            .collect(),
    );
    let rows = [
        (MenuAction::SettingsScale(-2), [0.0, 0.0, 30.0, 10.0]),
        (MenuAction::SettingsScale(-1), [30.0, 0.0, 60.0, 10.0]),
        (MenuAction::SettingsScale(0), [60.0, 0.0, 90.0, 10.0]),
    ];
    native_rows(&mut menu, &rows);
    menu.focus_pointer(MenuAction::SettingsScale(-2));
    menu.move_horizontal_focus(1);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsScale(-1)
    );
    assert_eq!(menu.gui_scale_offset(), 0);
    menu.activate_focused();
    assert_eq!(menu.gui_scale_offset(), -1);
    native_rows(&mut menu, &rows);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsScale(-1)
    );
}

#[test]
fn native_slider_selection_exits_on_back_focus_loss_and_empty_layout() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.screen = MenuScreen::Settings;
    let index = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "gamma")
        .unwrap() as u16;
    let value = menu.settings_options.get(usize::from(index));
    let rows = [
        (
            MenuAction::SettingsOption(index, value),
            [0.0, 0.0, 90.0, 10.0],
        ),
        (MenuAction::AddBack, [0.0, 20.0, 90.0, 30.0]),
    ];
    native_rows(&mut menu, &rows);
    menu.activate_focused();
    assert_eq!(menu.settings_slider_selected, Some(index));
    assert_eq!(menu.settings_control_activation, None);
    assert!(menu.clear_settings_slider_selection());
    assert_eq!(menu.settings_options.get(usize::from(index)), value);
    menu.activate_focused();
    menu.move_directional_focus(SettingsFocusAxis::Vertical, 1);
    assert_eq!(menu.settings_slider_selected, None);
    assert_eq!(menu.focus_actions()[menu.focused], MenuAction::AddBack);
    menu.focus_pointer(MenuAction::SettingsOption(index, value));
    menu.activate_focused();
    menu.refresh_settings_focus_geometry(&[], &[]);
    assert_eq!(menu.settings_slider_selected, None);
    assert!(menu.focus_actions().is_empty());
    menu.move_horizontal_focus(1);
    menu.activate_focused();
    assert_eq!(menu.settings_options.get(usize::from(index)), value);
}
#[test]
fn toggle_activation_reads_the_current_value_between_paints_and_arrows_do_not_commit() {
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let index = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "keyboard_mouse_autojump")
        .unwrap();
    let initial = menu.settings_options.get(index);
    menu.refresh_settings_focus([MenuAction::SettingsOption(index as u16, 1 - initial)]);
    menu.activate_focused();
    assert_eq!(menu.settings_options.get(index), 1 - initial);
    menu.activate_focused();
    assert_eq!(
        menu.settings_options.get(index),
        initial,
        "each keydown reads the live value before another frame is drawn"
    );
    menu.set_option(index as u16, 0);
    menu.refresh_settings_focus([
        MenuAction::SettingsOption(index as u16, 1),
        MenuAction::AddBack,
    ]);
    menu.move_horizontal_focus(1);
    assert_eq!(
        menu.settings_options.get(index),
        0,
        "switch arrows navigate without committing a value"
    );
}
#[test]
fn gui_scale_picker_commits_once_and_keeps_focus_through_responsive_layouts() {
    let mut menu = MenuRuntime::new(true, 2, "Test".into());
    menu.screen = MenuScreen::Settings;
    menu.sync_gui_scale(
        0,
        ui::DesktopGuiScale::for_window([1920, 1080])
            .choices()
            .collect(),
    );
    menu.settings_focus = vec![MenuAction::SettingsScale(0)];
    menu.refresh_settings_focus([MenuAction::AddBack, MenuAction::SettingsScalePicker]);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsScalePicker
    );
    menu.activate_focused();
    menu.refresh_settings_focus([
        MenuAction::SettingsScalePicker,
        MenuAction::SettingsScale(-2),
        MenuAction::SettingsScale(-1),
        MenuAction::SettingsScale(0),
    ]);
    menu.move_focus(-1);
    assert_eq!(menu.gui_scale_offset(), 0);
    menu.activate_focused();
    assert_eq!(menu.gui_scale_offset(), -1);
    assert!(!menu.settings_scale_picker);
    menu.refresh_settings_focus([MenuAction::AddBack, MenuAction::SettingsScalePicker]);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsScalePicker
    );
    menu.refresh_settings_focus([
        MenuAction::AddBack,
        MenuAction::SettingsScale(-2),
        MenuAction::SettingsScale(-1),
        MenuAction::SettingsScale(0),
    ]);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsScale(-1)
    );
    menu.activate(MenuAction::SettingsScalePicker);
    menu.go_back();
    assert!(!menu.settings_scale_picker);
    assert_eq!(menu.gui_scale_offset(), -1);
    assert_eq!(menu.screen, MenuScreen::Settings);
}
#[test]
fn review_settings_focus_reaches_visible_ordinary_controls() {
    let mut menu = MenuRuntime::new(true, 2, "Test".into());
    menu.screen = MenuScreen::Settings;
    let gamma = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "gamma")
        .unwrap();
    let value = menu.settings_options.value("gamma");
    menu.settings_focus = vec![MenuAction::SettingsOption(gamma as u16, value)];
    assert_eq!(menu.focus_actions(), menu.settings_focus);
    menu.refresh_settings_focus([
        MenuAction::SettingsOption(gamma as u16, 0),
        MenuAction::SettingsOption(gamma as u16, 100),
    ]);
    assert_eq!(
        menu.focus_actions().len(),
        1,
        "a segmented slider is one focus control"
    );
    menu.move_horizontal_focus(1);
    assert_eq!(
        menu.settings_options.value("gamma"),
        value,
        "an unselected slider does not adjust"
    );
    menu.activate_focused();
    assert_eq!(menu.settings_slider_selected, Some(gamma as u16));
    assert_eq!(
        menu.settings_control_activation, None,
        "selecting adjustment mode does not commit a value"
    );
    menu.move_horizontal_focus(1);
    assert_eq!(menu.settings_options.value("gamma"), value + 1);
}

#[test]
fn settings_picker_retains_choices_and_commits_only_when_activated() {
    let mut menu = MenuRuntime::new(true, 2, "Test".into());
    menu.screen = MenuScreen::Settings;
    let index = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "third_person")
        .unwrap() as u16;
    let original = menu.settings_options.get(usize::from(index));
    menu.activate(MenuAction::SettingsDropdown(index));
    let choices = std::iter::once(MenuAction::SettingsDropdown(index))
        .chain((0..3).map(|value| MenuAction::SettingsOption(index, value)))
        .collect::<Vec<_>>();
    menu.refresh_settings_focus(choices.clone());
    assert_eq!(menu.focus_actions(), choices);
    assert_eq!(menu.focused, original as usize + 1);
    menu.move_focus(1);
    assert_eq!(menu.settings_options.get(usize::from(index)), original);
    menu.activate_focused();
    assert_eq!(
        menu.settings_options.get(usize::from(index)),
        (original + 1) % 3
    );
    assert_eq!(menu.settings_dropdown, None);
    menu.refresh_settings_focus([MenuAction::AddBack, MenuAction::SettingsDropdown(index)]);
    assert_eq!(
        menu.focus_actions()[menu.focused],
        MenuAction::SettingsDropdown(index)
    );
}

#[test]
fn settings_picker_back_keeps_the_settings_page_and_value() {
    let mut menu = MenuRuntime::new(true, 2, "Test".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let index = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "third_person")
        .unwrap() as u16;
    let value = menu.settings_options.get(usize::from(index));
    menu.activate(MenuAction::SettingsDropdown(index));
    menu.go_back();
    assert_eq!(menu.screen, MenuScreen::Settings);
    assert_eq!(menu.settings_dropdown, None);
    assert_eq!(menu.settings_options.get(usize::from(index)), value);
}

#[test]
fn settings_pack_overlay_with_no_controls_keeps_background_input_closed() {
    let mut menu = MenuRuntime::new(true, 2, "Test".into());
    menu.screen = MenuScreen::Settings;
    let index = settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "gamma")
        .unwrap() as u16;
    let value = menu.settings_options.get(usize::from(index));
    menu.settings_focus = vec![MenuAction::SettingsOption(index, value)];
    std::sync::Arc::make_mut(&mut menu.global_resources).settings = Some(0);
    menu.refresh_settings_focus([]);
    menu.move_horizontal_focus(1);
    menu.activate_focused();
    assert!(menu.focus_actions().is_empty());
    assert_eq!(menu.settings_options.get(usize::from(index)), value);
}

/// Keyboard and controller navigation retain each radio row's selected value.
#[test]
fn settings_dropdown_focus_reaches_and_activates_each_radio_choice() {
    for name in ["animations", "graphics_mode"] {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Settings;
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap() as u16;
        let actions = [
            MenuAction::SettingsDropdown(index),
            MenuAction::SettingsOption(index, 0),
            MenuAction::SettingsOption(index, 1),
        ];
        menu.refresh_settings_focus(actions);
        assert_eq!(
            menu.focus_actions(),
            actions,
            "each radio row is a distinct focus control"
        );
        menu.focused = 0;
        menu.activate_focused();
        assert_eq!(menu.settings_dropdown, Some(index));
        menu.refresh_settings_focus(actions);
        menu.focus_pointer(actions[1]);
        assert_eq!(menu.focus_actions()[menu.focused], actions[1]);
        menu.move_focus(1);
        assert_eq!(menu.focus_actions()[menu.focused], actions[2]);
        menu.refresh_settings_focus(actions);
        assert_eq!(menu.focus_actions()[menu.focused], actions[2]);
        menu.activate_focused();
        assert_eq!(menu.settings_options.value(name), 1);
        assert_eq!(menu.settings_dropdown, None);
        menu.focused = 0;
        menu.activate_focused();
        menu.refresh_settings_focus(actions);
        menu.focus_pointer(actions[1]);
        menu.activate_focused();
        assert_eq!(menu.settings_options.value(name), 0);
        assert_eq!(menu.settings_dropdown, None);
    }
}
