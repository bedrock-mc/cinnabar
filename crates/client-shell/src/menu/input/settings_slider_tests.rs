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
use launcher::menu::settings_options::SettingId;
use {
    client_ui::test_support::engine_presentation,
    launcher::menu::{MenuAction, MenuScreen, settings_options::SETTINGS_OPTIONS},
};

const PHYSICAL: [u32; 2] = [1920, 1080];

/// Swiping a toggle row scrolls its content and never toggles the row on release.
#[test]
fn settings_swipe_moves_content_without_activating_the_pressed_row() {
    let (mut app, window, option, value, bounds) = accessibility_fixture();
    let start = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    touch(&mut app, window, start, TouchPhase::Started);
    touch(&mut app, window, start - Vec2::Y * 150.0, TouchPhase::Moved);
    touch(&mut app, window, start - Vec2::Y * 150.0, TouchPhase::Ended);
    build(&mut app);
    let menu = app.world().resource::<MenuRuntime>();
    assert_eq!(menu.settings_options.value(option), value);
    let moved = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::SettingsOption(option.index() as u16, 1 - value))
        .unwrap();
    assert!(moved.min().y() < bounds.min().y());
}

/// A second finger cannot replace the first finger's pending control or release it.
#[test]
fn settings_second_finger_does_not_steal_a_pending_tap() {
    let (mut app, window, option, value, bounds) = accessibility_fixture();
    let start = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    touch(&mut app, window, start, TouchPhase::Started);
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        app.world_mut().write_message(TouchInput {
            phase,
            position: Vec2::ZERO,
            window,
            force: None,
            id: 8,
        });
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value(option),
        value
    );
    touch(&mut app, window, start, TouchPhase::Ended);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value(option),
        1 - value
    );
}

/// Touch menus wait for release, including screens outside Settings.
#[test]
fn add_server_fields_do_not_activate_on_touch_down_or_cancel() {
    let (mut app, window) = shader_fixture();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::AddServer));
    build(&mut app);
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::AddAddress)
        .unwrap();
    let point = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    let revision = app.world().resource::<MenuRuntime>().field_activation;
    touch(&mut app, window, point, TouchPhase::Started);
    assert_eq!(
        app.world().resource::<MenuRuntime>().field_activation,
        revision
    );
    touch(&mut app, window, point, TouchPhase::Canceled);
    assert_eq!(
        app.world().resource::<MenuRuntime>().field_activation,
        revision
    );
    touch(&mut app, window, point, TouchPhase::Started);
    touch(&mut app, window, point, TouchPhase::Ended);
    assert_eq!(
        app.world().resource::<MenuRuntime>().field,
        Some(MenuField::Address)
    );
    assert!(app.world().resource::<MenuRuntime>().field_activation > revision);
}

/// A short Add Server viewport scrolls under the finger instead of selecting a field.
#[test]
fn add_server_overflow_accepts_a_touch_scroll() {
    let (mut app, window) = shader_fixture();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::AddServer));
    app.world_mut()
        .resource_mut::<UiPresentationResource>()
        .set_safe_area(ui::SafeArea::new(0.0, 0.0, 0.0, 850.0).unwrap());
    build(&mut app);
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::AddName)
        .unwrap();
    let start = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    let revision = app.world().resource::<MenuRuntime>().field_activation;
    touch(&mut app, window, start, TouchPhase::Started);
    touch(&mut app, window, start - Vec2::Y * 100.0, TouchPhase::Moved);
    touch(&mut app, window, start - Vec2::Y * 100.0, TouchPhase::Ended);
    assert_eq!(
        app.world().resource::<MenuRuntime>().field_activation,
        revision
    );
    build(&mut app);
    let moved = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::AddName);
    assert!(moved.is_none_or(|moved| moved.min().y() < bounds.min().y()));
}

/// Opens a static settings column with an actionable row and overflowing content.
fn accessibility_fixture() -> (App, Entity, SettingId, i32, ui::UiRect) {
    let (mut app, window) = shader_fixture();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .sync_shader_options(None);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_named_option(SettingId::ScreenAnimations, 0);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsSection(
            launcher::menu::settings_sections::SettingSection::Accessibility
                .definition()
                .index,
        ));
    build(&mut app);
    let option = SettingId::CameraShake;
    let value = app
        .world()
        .resource::<MenuRuntime>()
        .settings_options
        .value(option);
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::SettingsOption(option.index() as u16, 1 - value))
        .unwrap();
    (app, window, option, value, bounds)
}

/// Retained focus can reject input even while the window remains focused.
#[test]
fn occlusion_cancels_a_settings_swipe_until_the_finger_is_lifted() {
    let (mut app, window, option, value, bounds) = accessibility_fixture();
    app.init_resource::<client_state::CursorFocus>();
    let start = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    touch(&mut app, window, start, TouchPhase::Started);
    touch(&mut app, window, start - Vec2::Y * 30.0, TouchPhase::Moved);
    build(&mut app);
    let before = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::SettingsOption(option.index() as u16, 1 - value))
        .unwrap();
    assert!(before.min().y() < bounds.min().y());
    app.world_mut()
        .resource_mut::<client_state::CursorFocus>()
        .occlusion_changed(true);
    touch(&mut app, window, start - Vec2::Y * 60.0, TouchPhase::Moved);
    {
        let mut focus = app.world_mut().resource_mut::<client_state::CursorFocus>();
        focus.occlusion_changed(false);
        focus.begin_frame(true);
    }
    touch(&mut app, window, start - Vec2::Y * 90.0, TouchPhase::Moved);
    build(&mut app);
    let after = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::SettingsOption(option.index() as u16, 1 - value))
        .unwrap();
    assert_eq!(after.min().y(), before.min().y());
}

/// Opens a shader slider using the redistributable font fixture and native menu painter.
fn shader_fixture() -> (App, Entity) {
    use launcher::shader_options::{Control, Element, Status, View};
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        launcher::menu::settings_sections::SettingSection::Shaders
            .definition()
            .index,
    ));
    let mut view = View::loading("Fixture");
    view.screen = 7;
    view.status = Status::Ready;
    view.elements = vec![Element {
        label: "Quality".into(),
        comment: None,
        control: Control::Slider {
            value: "2".into(),
            index: 1,
            count: 4,
        },
        modified: false,
    }];
    menu.sync_shader_options(Some(view));
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<TouchInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(menu)
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
        ))
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
    (app, window)
}

#[test]
fn shader_slider_drag_changes_steps_outside_the_track_and_releases_capture() {
    use launcher::shader_options::{Action, Input};
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let (mut app, window) = shader_fixture();
    let action = |step| {
        MenuAction::ShaderOptions(Action::Element {
            screen: 7,
            index: 0,
            input: Input::Select(step),
        })
    };
    let bounds = |step| {
        app.world()
            .resource::<UiPresentationResource>()
            .menu_action_bounds(action(step))
            .unwrap()
    };
    let first = bounds(0);
    let last = bounds(3);
    let start = Vec2::new(
        (first.min().x() + first.max().x()) * 0.5,
        (first.min().y() + first.max().y()) * 0.5,
    );
    let end = Vec2::new(last.max().x() + 100.0, last.max().y() + 100.0);
    mouse(&mut app, window, start, Some(ButtonState::Pressed));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .take_shader_option_actions();
    mouse(&mut app, window, end, None);
    assert_eq!(
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .take_shader_option_actions(),
        [Action::Element {
            screen: 7,
            index: 0,
            input: Input::Select(3)
        }]
    );
    mouse(&mut app, window, end, None);
    assert!(
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .take_shader_option_actions()
            .is_empty()
    );
    mouse(&mut app, window, end, Some(ButtonState::Released));
    mouse(&mut app, window, start, None);
    assert!(
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .take_shader_option_actions()
            .is_empty()
    );
}

#[test]
fn shader_slider_touch_drag_updates_steps_and_mouse_capture_rejects_new_screens() {
    use launcher::shader_options::{Action, Input};
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let (mut app, window) = shader_fixture();
    let action = |step| {
        MenuAction::ShaderOptions(Action::Element {
            screen: 7,
            index: 0,
            input: Input::Select(step),
        })
    };
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(action(1))
        .unwrap();
    let start = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    let end = Vec2::new(PHYSICAL[0] as f32, PHYSICAL[1] as f32);
    touch(&mut app, window, start, TouchPhase::Started);
    touch(&mut app, window, end, TouchPhase::Moved);
    assert_eq!(
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .take_shader_option_actions(),
        [Action::Element {
            screen: 7,
            index: 0,
            input: Input::Select(3)
        }]
    );
    touch(&mut app, window, end, TouchPhase::Moved);
    assert!(
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .take_shader_option_actions()
            .is_empty()
    );
    touch(&mut app, window, end, TouchPhase::Ended);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .take_shader_option_actions();
    mouse(&mut app, window, start, Some(ButtonState::Pressed));
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.take_shader_option_actions();
        let mut next = (*menu.shader_options_view().unwrap()).clone();
        next.screen += 1;
        menu.sync_shader_options(Some(next));
    }
    build(&mut app);
    mouse(&mut app, window, end, None);
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    assert!(menu.take_shader_option_actions().is_empty());
    assert!(menu.settings_input.shader_slider_drag.is_none());
    assert_eq!(
        native_release_action(action(0), Some(action(3))),
        Some(action(3))
    );
    assert_eq!(
        native_release_action(
            action(0),
            Some(MenuAction::ShaderOptions(Action::Element {
                screen: 8,
                index: 0,
                input: Input::Select(3),
            }))
        ),
        None
    );
}

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
        .position(|option| option.id == SettingId::Gamma)
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
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(menu)
        .insert_resource(UiPresentationResource(presentation))
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
        .resource::<UiPresentationResource>()
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
            .resource::<UiPresentationResource>()
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
        .resource_scope(|world, mut presentation: Mut<UiPresentationResource>| {
            presentation.set_menu_view(Some(view));
            presentation
                .build(
                    world.resource::<client_state::PlayerRuntime>(),
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
    let presentation = app.world().resource::<UiPresentationResource>();
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
        .settings
        .slider_pointer
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
            .resource::<UiPresentationResource>()
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
            .settings
            .slider_pointer
            .unwrap()
            .fraction,
        (persisted(&app) - option.min) as f32 / (option.max - option.min) as f32,
        "the thumb follows the pointer between saved integer values"
    );
    build(&mut app);
    let edge = rail.point(0.3749, rail.middle) + Vec2::new(rail.half_thumb - 0.01, 0.0);
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
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
            .settings
            .slider_pointer
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
            .resource::<UiPresentationResource>()
            .settings_slider_thumb_contains(gamma(), UiPoint::new(click.x, click.y).unwrap())
    );
    mouse(&mut app, window, click, Some(ButtonState::Pressed));
    assert_eq!(persisted(&app), original);
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings
            .slider_pointer
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
    assert!(view.settings.slider_pointer.is_none());
    assert_eq!(
        view.settings.control_activation.map(|(action, _)| action),
        Some(MenuAction::SettingsOption(gamma(), quantized(0.8)))
    );
    assert!(!view.settings.control_activation_navigation);
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
            .settings
            .slider_pointer
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
        .resource::<UiPresentationResource>()
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
    assert_eq!(view.settings.slider_hovered, None);
    let option = &SETTINGS_OPTIONS[usize::from(gamma())];
    let selected = (persisted(&app) - option.min) as f32 / (option.max - option.min) as f32;
    mouse(&mut app, window, rail.point(selected, rail.middle), None);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.settings.slider_hovered, Some(gamma()));
    assert!(
        matches!(view.focused_action, Some(MenuAction::SettingsOption(index, _)) if index == gamma())
    );
    app.world_mut().resource_mut::<MenuRuntime>().input_mode = MenuInputMode::Keyboard;
    app.update();
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .settings
            .slider_hovered,
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
    assert_eq!(menu.view().settings.control_activation, None);
    let action = MenuAction::SettingsOption(gamma(), option.max);
    menu.activate_from_input(action);
    let first = menu.view().settings.control_activation.unwrap();
    assert_eq!(first.0, action);
    assert!(!menu.view().settings.control_activation_navigation);
    menu.set_option(gamma(), option.min);
    assert_eq!(menu.view().settings.control_activation, Some(first));
    menu.activate(MenuAction::SettingsOption(gamma(), option.default));
    assert_eq!(menu.view().settings.control_activation, Some(first));
    menu.activate_from_navigation(action);
    let navigation = menu.view().settings.control_activation.unwrap();
    assert_eq!(navigation.0, action);
    assert!(navigation.1 > first.1);
    assert!(menu.view().settings.control_activation_navigation);
    menu.activate_from_input(MenuAction::SettingsOption(gamma(), option.min));
    let pointer = menu.view().settings.control_activation.unwrap();
    assert!(pointer.1 > navigation.1);
    assert!(!menu.view().settings.control_activation_navigation);
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
        .settings
        .control_activation;
    key(&mut app, window, KeyCode::Space);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.settings.slider_selected, Some(gamma()));
    assert_eq!(persisted(&app), original);
    assert_eq!(view.settings.control_activation, activation);
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
            .settings
            .slider_selected,
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
            .settings
            .slider_selected,
        Some(gamma())
    );
    key(&mut app, window, KeyCode::Backspace);
    let after = app.world().resource::<MenuRuntime>().view();
    assert_eq!(after.settings.slider_selected, None);
    assert_eq!(after.screen, MenuScreen::Settings);
    assert_eq!(after.settings.section, before.settings.section);
    assert_eq!(
        after.settings.advanced_graphics,
        before.settings.advanced_graphics
    );
    assert_eq!(
        persisted(&app),
        before.settings_options.get(usize::from(gamma()))
    );
    assert_eq!(
        after.settings.control_activation,
        before.settings.control_activation
    );
}

#[test]
fn native_picker_space_commits_the_focused_choice() {
    let Some((mut app, window)) = fixture("native_picker_space_commits_the_focused_choice") else {
        return;
    };
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::ThirdPerson)
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
    assert_eq!(view.settings.dropdown, None);
    assert_eq!(view.settings_options.get(usize::from(index)), 1);
    assert_eq!(
        view.settings.control_activation.map(|(action, _)| action),
        Some(MenuAction::SettingsOption(index, 1))
    );
    assert!(view.settings.control_activation_navigation);
}

#[test]
fn native_release_action_preserves_live_toggle_and_fullscreen_callbacks() {
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::AlwaysSprint)
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
            .settings
            .control_activation
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
    let toggle = MenuAction::SettingsOption(option("always_sprint"), 1);
    assert_eq!(
        native_release_action(
            toggle,
            Some(MenuAction::SettingsOption(
                option("controller_invert_y_axis"),
                0
            ))
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
