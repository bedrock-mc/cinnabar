//! Native macro fields retain mouse and touch caret behavior.
use super::*;
use bevy::input::touch::{TouchInput, TouchPhase, touch_screen_input_system};

/// Builds a native command field and finds a visible point inside its existing text.
fn command_field() -> (App, Entity, ui::UiPoint, usize) {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation
        .enable_json_ui(client_ui::test_support::mini_carrier())
        .unwrap();
    let (mut app, window) =
        menu_input_app_with(crate::menu::MenuClipboard::default(), presentation);
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.screen = MenuScreen::Settings;
        menu.settings_section =
            client_ui::test_support::settings_section_index("keyboard_and_mouse_forced_index")
                .unwrap();
        menu.settings_focus = vec![MenuAction::SettingsMacroCommand(0)];
        menu.activate(MenuAction::SettingsMacroCommand(0));
        menu.edit_text("time set day");
    }
    let mut view = app.world().resource::<MenuRuntime>().view();
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    let player = app
        .world()
        .resource::<client_state::PlayerRuntime>()
        .clone();
    let (point, byte) = {
        let mut presentation = app.world_mut().resource_mut::<UiPresentationResource>();
        presentation.set_menu_view(Some(view));
        presentation
            .build(
                &player,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(presentation.uses_oreui_settings());
        (0..720)
            .step_by(4)
            .flat_map(|y| {
                (0..1280)
                    .step_by(4)
                    .map(move |x| ui::UiPoint::new(x as f32, y as f32).unwrap())
            })
            .find_map(|point| {
                if presentation.hit_test_menu(point) != Some(MenuAction::SettingsMacroCommand(0)) {
                    return None;
                }
                let byte = presentation.menu_caret_at(
                    point,
                    MenuField::MacroCommand(0),
                    "time set day",
                )?;
                (byte > 0 && byte < 12).then_some((point, byte))
            })
            .expect("native macro field publishes an interior caret target")
    };
    (app, window, point, byte)
}

#[test]
fn command_macro_mouse_release_places_caret_at_the_clicked_character() {
    let (mut app, window, point, byte) = command_field();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(point.x(), point.y())));
    mouse(&mut app, MouseButton::Left, false);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    let mut view = app.world().resource::<MenuRuntime>().view();
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    client_ui::test_support::snapshot_menu(
        app.world().resource::<client_state::PlayerRuntime>(),
        &view,
        "command-macro-caret",
    );
    assert_eq!(view.caret.byte, byte);
    press_key(&mut app, window, KeyCode::KeyX, Some("x"));
    let mut expected = "time set day".to_owned();
    expected.insert(byte, 'x');
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .field_text(MenuField::MacroCommand(0)),
        expected
    );
}

#[test]
fn command_macro_touch_release_places_caret_at_the_tapped_character() {
    let (mut app, window, point, byte) = command_field();
    app.add_message::<TouchInput>()
        .add_systems(PreUpdate, touch_screen_input_system);
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        app.world_mut().write_message(TouchInput {
            phase,
            position: Vec2::new(point.x(), point.y()),
            window,
            force: None,
            id: 1,
        });
        app.update();
    }
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().caret.byte,
        byte
    );
    press_key(&mut app, window, KeyCode::KeyX, Some("x"));
    let mut expected = "time set day".to_owned();
    expected.insert(byte, 'x');
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .field_text(MenuField::MacroCommand(0)),
        expected
    );
}
