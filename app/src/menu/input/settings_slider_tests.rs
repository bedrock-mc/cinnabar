use bevy::{
    input::{
        keyboard::Key,
        touch::{TouchInput, TouchPhase, touch_screen_input_system},
    },
    prelude::*,
    window::WindowResolution,
};
use ui::DpiScale;

use super::*;
use client_ui::ui_runtime::UiRuntime;
use {
    client_ui::test_support::engine_presentation,
    launcher::menu::{MenuAction, MenuScreen, settings_options::SETTINGS_OPTIONS},
};

const PHYSICAL: [u32; 2] = [1920, 1080];

struct Rail {
    left: f32,
    right: f32,
    middle: f32,
    half_thumb: f32,
}

impl Rail {
    fn point(&self, fraction: f32, y: f32) -> Vec2 {
        Vec2::new(self.left + (self.right - self.left) * fraction, y)
    }
}

fn gamma() -> u16 {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "gamma")
        .unwrap() as u16
}

fn fixture(test: &str) -> Option<(App, Entity)> {
    let Some(presentation) = engine_presentation() else {
        eprintln!("skipping {test}: missing installed local carriers (make assets)");
        return None;
    };
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.input_mode = MenuInputMode::Keyboard;
    menu.set_gui_scale_preference(None);
    menu.set_option(gamma(), SETTINGS_OPTIONS[usize::from(gamma())].default);
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<TouchInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(menu)
        .insert_resource(presentation)
        .add_systems(PreUpdate, touch_screen_input_system)
        .add_systems(Update, drive_menu_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                resolution: WindowResolution::new(PHYSICAL[0], PHYSICAL[1]),
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.update();
    build(&mut app);
    let sections: Vec<_> = app
        .world()
        .resource::<UiPresentationRuntime>()
        .visible_menu_actions()
        .filter(|action| matches!(action, MenuAction::SettingsSection(_)))
        .collect();
    for section in sections {
        let value = app
            .world()
            .resource::<MenuRuntime>()
            .view()
            .settings_options
            .get(usize::from(gamma()));
        let action = MenuAction::SettingsOption(gamma(), value);
        {
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            menu.activate(section);
            menu.refresh_settings_focus([action]);
        }
        build(&mut app);
        if app
            .world()
            .resource::<UiPresentationRuntime>()
            .menu_action_bounds(action)
            .is_some()
        {
            return Some((app, window));
        }
    }
    panic!("native category navigation exposes the brightness slider");
}

fn build(app: &mut App) {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
            presentation.set_menu_view(Some(view));
            presentation
                .build(
                    world.resource::<crate::player_runtime::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    PHYSICAL,
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        });
}

fn rail(app: &App) -> Rail {
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    let presentation = app.world().resource::<UiPresentationRuntime>();
    let first = presentation
        .menu_action_bounds(MenuAction::SettingsOption(gamma(), option.min))
        .unwrap();
    let last = presentation
        .menu_action_bounds(MenuAction::SettingsOption(gamma(), option.max))
        .unwrap();
    let half_thumb = (first.max().y() - first.min().y()) * 0.5;
    Rail {
        left: first.min().x() + half_thumb,
        right: last.max().x() - half_thumb,
        middle: (first.min().y() + first.max().y()) * 0.5,
        half_thumb,
    }
}

fn persisted(app: &App) -> i32 {
    app.world()
        .resource::<MenuRuntime>()
        .view()
        .settings_options
        .get(usize::from(gamma()))
}

fn quantized(fraction: f32) -> i32 {
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    let stops = (option.max - option.min) / option.step;
    option.min + (fraction.clamp(0.0, 1.0) * stops as f32).round() as i32 * option.step
}

fn mouse(app: &mut App, window: Entity, position: Vec2, state: Option<ButtonState>) {
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .set_cursor_position(Some(position));
    if let Some(state) = state {
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        });
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        match state {
            ButtonState::Pressed => buttons.press(MouseButton::Left),
            ButtonState::Released => buttons.release(MouseButton::Left),
        }
    }
    app.update();
}

fn touch(app: &mut App, window: Entity, position: Vec2, phase: TouchPhase) {
    app.world_mut().write_message(TouchInput {
        phase,
        position,
        window,
        force: None,
        id: 7,
    });
    app.update();
}

fn key(app: &mut App, window: Entity, key_code: KeyCode) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

fn focus_gamma(app: &mut App) {
    app.update();
    let action = MenuAction::SettingsOption(gamma(), persisted(app));
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    menu.focus_pointer(action);
    menu.input_mode = MenuInputMode::Keyboard;
    assert_eq!(menu.view().focused_action, Some(action));
}

fn assert_pointer(app: &App, fraction: f32, mouse_input: bool) {
    let pointer = app
        .world()
        .resource::<MenuRuntime>()
        .view()
        .settings_slider_pointer
        .expect("a held thumb retains continuous pointer state");
    assert_eq!(pointer.option, gamma());
    assert_eq!(pointer.mouse_input, mouse_input);
    assert!((pointer.fraction - fraction).abs() < 0.00001);
    assert_eq!(persisted(app), quantized(fraction));
}

#[test]
fn native_slider_thumb_retains_raw_mouse_position_until_release() {
    let Some((mut app, window)) =
        fixture("native_slider_thumb_retains_raw_mouse_position_until_release")
    else {
        return;
    };
    let rail = rail(&app);
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    let selected = (persisted(&app) - option.min) as f32 / (option.max - option.min) as f32;
    let thumb = rail.point(selected, rail.middle);
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .settings_slider_thumb_contains(gamma(), UiPoint::new(thumb.x, thumb.y).unwrap())
    );
    mouse(&mut app, window, thumb, Some(ButtonState::Pressed));
    assert_pointer(&app, selected, true);
    let view = app.world().resource::<MenuRuntime>().view();
    assert!(!view.navigation_focus_visible);
    assert!(matches!(
        view.focused_action,
        Some(MenuAction::SettingsOption(index, _)) if index == gamma()
    ));
    for fraction in [0.3725, 0.3749] {
        mouse(&mut app, window, rail.point(fraction, 1.0), None);
        assert_pointer(&app, fraction, true);
    }
    assert_ne!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_pointer
            .unwrap()
            .fraction,
        (persisted(&app) - option.min) as f32 / (option.max - option.min) as f32,
        "the thumb follows the pointer between saved integer values"
    );
    build(&mut app);
    let edge = rail.point(0.3749, rail.middle) + Vec2::new(rail.half_thumb - 0.01, 0.0);
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .settings_slider_thumb_contains(gamma(), UiPoint::new(edge.x, edge.y).unwrap()),
        "relayout keeps the thumb at its raw position"
    );
    for (x, expected) in [(1.0, 0.0), (PHYSICAL[0] as f32 - 1.0, 1.0)] {
        mouse(&mut app, window, Vec2::new(x, 1.0), None);
        assert_pointer(&app, expected, true);
    }
    mouse(
        &mut app,
        window,
        Vec2::new(PHYSICAL[0] as f32 - 1.0, 1.0),
        Some(ButtonState::Released),
    );
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_pointer
            .is_none()
    );
    assert_eq!(persisted(&app), option.max);
}

#[test]
fn native_slider_track_click_commits_on_release_without_capturing_motion() {
    let Some((mut app, window)) =
        fixture("native_slider_track_click_commits_on_release_without_capturing_motion")
    else {
        return;
    };
    let rail = rail(&app);
    let original = persisted(&app);
    let click = rail.point(0.8, rail.middle);
    assert!(
        !app.world()
            .resource::<UiPresentationRuntime>()
            .settings_slider_thumb_contains(gamma(), UiPoint::new(click.x, click.y).unwrap())
    );
    mouse(&mut app, window, click, Some(ButtonState::Pressed));
    assert_eq!(persisted(&app), original);
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_pointer
            .is_none()
    );
    mouse(&mut app, window, Vec2::ONE, None);
    assert_eq!(
        persisted(&app),
        original,
        "the rail never acquires thumb drag"
    );
    mouse(&mut app, window, click, Some(ButtonState::Released));
    assert_eq!(persisted(&app), quantized(0.8));
    let view = app.world().resource::<MenuRuntime>().view();
    assert!(view.settings_slider_pointer.is_none());
    assert_eq!(
        view.settings_control_activation.map(|(action, _)| action),
        Some(MenuAction::SettingsOption(gamma(), quantized(0.8)))
    );
    assert!(!view.settings_control_activation_navigation);
}

#[test]
fn native_slider_touch_drag_keeps_continuous_position_and_releases_capture() {
    let Some((mut app, window)) =
        fixture("native_slider_touch_drag_keeps_continuous_position_and_releases_capture")
    else {
        return;
    };
    let rail = rail(&app);
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    let selected = (persisted(&app) - option.min) as f32 / (option.max - option.min) as f32;
    touch(
        &mut app,
        window,
        rail.point(selected, rail.middle),
        TouchPhase::Started,
    );
    assert_pointer(&app, selected, false);
    let view = app.world().resource::<MenuRuntime>().view();
    assert!(!view.navigation_focus_visible);
    assert!(matches!(
        view.focused_action,
        Some(MenuAction::SettingsOption(index, _)) if index == gamma()
    ));
    let moved = rail.point(0.5833, 1.0);
    touch(&mut app, window, moved, TouchPhase::Moved);
    assert_pointer(&app, 0.5833, false);
    touch(&mut app, window, moved, TouchPhase::Ended);
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_pointer
            .is_none()
    );
    assert_eq!(persisted(&app), quantized(0.5833));
}

#[test]
fn native_slider_rail_hover_keeps_focus_and_thumb_hover_is_independent() {
    let Some((mut app, window)) =
        fixture("native_slider_rail_hover_keeps_focus_and_thumb_hover_is_independent")
    else {
        return;
    };
    let rail = rail(&app);
    app.update();
    let category = app
        .world()
        .resource::<UiPresentationRuntime>()
        .visible_menu_actions()
        .find(|action| matches!(action, MenuAction::SettingsSection(_)))
        .unwrap();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .focus_pointer(category);
    mouse(&mut app, window, rail.point(0.8, rail.middle), None);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(
        view.focused_action,
        Some(category),
        "the rail has no mouse-enter focus listener"
    );
    assert_eq!(view.settings_slider_hovered, None);
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    let selected = (persisted(&app) - option.min) as f32 / (option.max - option.min) as f32;
    mouse(&mut app, window, rail.point(selected, rail.middle), None);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.settings_slider_hovered, Some(gamma()));
    assert!(
        matches!(view.focused_action, Some(MenuAction::SettingsOption(index, _)) if index == gamma())
    );
    app.world_mut().resource_mut::<MenuRuntime>().input_mode = MenuInputMode::Keyboard;
    app.update();
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_hovered,
        Some(gamma()),
        "keyboard modality does not synthesize a thumb mouse-leave event"
    );
}

#[test]
fn settings_activation_revisions_distinguish_input_from_external_setters() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    menu.set_option(gamma(), option.min);
    assert_eq!(menu.view().settings_control_activation, None);
    let action = MenuAction::SettingsOption(gamma(), option.max);
    menu.activate_from_input(action);
    let first = menu.view().settings_control_activation.unwrap();
    assert_eq!(first.0, action);
    assert!(!menu.view().settings_control_activation_navigation);
    menu.set_option(gamma(), option.min);
    assert_eq!(menu.view().settings_control_activation, Some(first));
    menu.activate(MenuAction::SettingsOption(gamma(), option.default));
    assert_eq!(menu.view().settings_control_activation, Some(first));
    menu.activate_from_navigation(action);
    let navigation = menu.view().settings_control_activation.unwrap();
    assert_eq!(navigation.0, action);
    assert!(navigation.1 > first.1);
    assert!(menu.view().settings_control_activation_navigation);
    menu.activate_from_input(MenuAction::SettingsOption(gamma(), option.min));
    let pointer = menu.view().settings_control_activation.unwrap();
    assert!(pointer.1 > navigation.1);
    assert!(!menu.view().settings_control_activation_navigation);
}

#[test]
fn native_slider_space_selects_adjustment_without_committing_a_value() {
    let Some((mut app, window)) =
        fixture("native_slider_space_selects_adjustment_without_committing_a_value")
    else {
        return;
    };
    focus_gamma(&mut app);
    let original = persisted(&app);
    let activation = app
        .world()
        .resource::<MenuRuntime>()
        .view()
        .settings_control_activation;
    key(&mut app, window, KeyCode::Space);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.settings_slider_selected, Some(gamma()));
    assert_eq!(persisted(&app), original);
    assert_eq!(view.settings_control_activation, activation);
    key(&mut app, window, KeyCode::ArrowRight);
    assert_eq!(
        persisted(&app),
        (original + SETTINGS_OPTIONS[usize::from(gamma())].step)
            .min(SETTINGS_OPTIONS[usize::from(gamma())].max)
    );
    key(&mut app, window, KeyCode::Space);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_selected,
        None
    );
}

#[test]
fn native_slider_backspace_cancels_adjustment_before_page_navigation() {
    let Some((mut app, window)) =
        fixture("native_slider_backspace_cancels_adjustment_before_page_navigation")
    else {
        return;
    };
    focus_gamma(&mut app);
    let before = app.world().resource::<MenuRuntime>().view();
    key(&mut app, window, KeyCode::Enter);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings_slider_selected,
        Some(gamma())
    );
    key(&mut app, window, KeyCode::Backspace);
    let after = app.world().resource::<MenuRuntime>().view();
    assert_eq!(after.settings_slider_selected, None);
    assert_eq!(after.screen, MenuScreen::Settings);
    assert_eq!(after.settings_section, before.settings_section);
    assert_eq!(
        after.settings_advanced_graphics,
        before.settings_advanced_graphics
    );
    assert_eq!(
        persisted(&app),
        before.settings_options.get(usize::from(gamma()))
    );
    assert_eq!(
        after.settings_control_activation,
        before.settings_control_activation
    );
}

#[test]
fn native_picker_space_commits_the_focused_choice() {
    let Some((mut app, window)) = fixture("native_picker_space_commits_the_focused_choice") else {
        return;
    };
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "third_person")
        .unwrap() as u16;
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.set_option(index, 0);
        menu.activate(MenuAction::SettingsDropdown(index));
    }
    build(&mut app);
    app.update();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.focus_pointer(MenuAction::SettingsOption(index, 1));
        menu.input_mode = MenuInputMode::Keyboard;
        assert_eq!(
            menu.view().focused_action,
            Some(MenuAction::SettingsOption(index, 1))
        );
    }
    key(&mut app, window, KeyCode::Space);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.settings_dropdown, None);
    assert_eq!(view.settings_options.get(usize::from(index)), 1);
    assert_eq!(
        view.settings_control_activation.map(|(action, _)| action),
        Some(MenuAction::SettingsOption(index, 1))
    );
    assert!(view.settings_control_activation_navigation);
}

#[test]
fn native_release_action_preserves_live_toggle_and_fullscreen_callbacks() {
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "keyboard_mouse_autojump")
        .unwrap() as u16;
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.set_option(index, 1);
    let hovered = MenuAction::SettingsOption(index, 0);
    let release = native_release_action(MenuAction::SettingsOption(index, 1), Some(hovered));
    assert_eq!(release, Some(hovered));
    menu.activate_from_input(release.unwrap());
    assert_eq!(menu.view().settings_options.get(usize::from(index)), 0);
    assert_eq!(
        menu.view()
            .settings_control_activation
            .map(|(action, _)| action),
        Some(hovered)
    );

    menu.activate(MenuAction::SettingsFullscreen(true));
    let release = native_release_action(
        MenuAction::SettingsFullscreen(true),
        Some(MenuAction::SettingsFullscreen(false)),
    );
    assert_eq!(release, Some(MenuAction::SettingsFullscreen(false)));
    menu.activate_from_input(release.unwrap());
    assert!(!menu.view().fullscreen);
}

#[test]
fn native_release_action_rejects_other_controls_and_other_choice_buttons() {
    let option = |name| {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap() as u16
    };
    let toggle = MenuAction::SettingsOption(option("keyboard_mouse_autojump"), 1);
    assert_eq!(
        native_release_action(
            toggle,
            Some(MenuAction::SettingsOption(option("controller_autojump"), 0))
        ),
        None
    );
    assert_eq!(native_release_action(toggle, None), None);
    let index = option("third_person");
    assert_eq!(
        native_release_action(
            MenuAction::SettingsOption(index, 0),
            Some(MenuAction::SettingsOption(index, 1))
        ),
        None
    );
    assert_eq!(
        native_release_action(
            MenuAction::SettingsFullscreen(true),
            Some(MenuAction::SettingsOption(gamma(), 0))
        ),
        None
    );
}
