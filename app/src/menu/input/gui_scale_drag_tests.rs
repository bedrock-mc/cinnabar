use bevy::{prelude::*, window::WindowResolution};
use ui::{DpiScale, UiRect};

use super::*;
use client_ui::ui_runtime::UiRuntime;
use {
    crate::ui_runtime::presentation::apply_gui_scale_setting,
    client_ui::test_support::engine_presentation,
    launcher::menu::{MenuAction, MenuScreen},
};

const PHYSICAL: [u32; 2] = [1920, 1080];

#[test]
fn option_slider_drag_keeps_capture_outside_track_and_releases() {
    let presentation = client_ui::test_support::mini_engine_presentation();
    check_option_slider_drag(presentation);
}

#[test]
fn installed_audio_slider_drag_keeps_capture_outside_track_and_releases() {
    let Some(presentation) = engine_presentation() else {
        eprintln!(
            "skipping installed_audio_slider_drag_keeps_capture_outside_track_and_releases: missing installed UI carrier; make assets"
        );
        return;
    };
    check_option_slider_drag(presentation);
}

fn check_option_slider_drag(presentation: UiPresentationRuntime) {
    use client_ui::test_support::{menu_hit_targets, settings_section_index};

    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        settings_section_index("sound_forced_index").unwrap(),
    ));
    let index = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "main_volume")
        .unwrap() as u16;
    menu.set_option(index, 50);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(menu)
        .insert_resource(presentation)
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
    let view = app.world().resource::<MenuRuntime>().view();
    let player = crate::player_runtime::PlayerRuntime::new(1);
    let presentation = &mut *app.world_mut().resource_mut::<UiPresentationRuntime>();
    presentation.set_menu_view(Some(view));
    presentation
        .build(
            &player,
            &UiRuntime::new(1),
            0,
            PHYSICAL,
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let middle = menu_hit_targets(presentation)
        .iter()
        .find_map(|(action, bounds)| {
            (*action == MenuAction::SettingsOption(index, 50)).then_some(Vec2::new(
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            ))
        })
        .expect("rendered volume slider midpoint");
    let music_index = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "music_volume")
        .unwrap() as u16;
    let music_point = menu_hit_targets(presentation)
        .iter()
        .find_map(|(action, bounds)| {
            (*action == MenuAction::SettingsOption(music_index, 25)).then_some(Vec2::new(
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            ))
        })
        .expect("rendered second slider quarter point");
    let music = app
        .world()
        .resource::<MenuRuntime>()
        .settings_options
        .value("music_volume");
    let initial = app
        .world()
        .resource::<MenuRuntime>()
        .settings_options
        .value("main_volume");
    pointer(&mut app, window, Vec2::ZERO, Some(ButtonState::Pressed));
    pointer(&mut app, window, middle, None);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("main_volume"),
        initial,
        "pressing outside the slider cannot capture it by moving over it"
    );
    pointer(&mut app, window, music_point, None);
    let options = &app.world().resource::<MenuRuntime>().settings_options;
    assert_eq!(
        (options.value("main_volume"), options.value("music_volume")),
        (initial, music),
        "a held outside press stays uncaptured over values that differ from the current ones"
    );
    pointer(&mut app, window, middle, Some(ButtonState::Released));
    pointer(&mut app, window, middle, Some(ButtonState::Pressed));
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("main_volume"),
        50
    );
    pointer(&mut app, window, music_point, None);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("main_volume"),
        25,
        "dragging over another slider keeps updating the captured slider"
    );
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("music_volume"),
        music
    );
    pointer(&mut app, window, Vec2::ZERO, None);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("main_volume"),
        0,
        "held drag leaves the track vertically and clamps to its left end"
    );
    pointer(
        &mut app,
        window,
        Vec2::new(PHYSICAL[0] as f32 - 1.0, PHYSICAL[1] as f32 - 1.0),
        None,
    );
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("main_volume"),
        100
    );
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("music_volume"),
        music
    );
    pointer(&mut app, window, middle, Some(ButtonState::Released));
    pointer(&mut app, window, Vec2::ZERO, None);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .value("main_volume"),
        100,
        "release prevents subsequent pointer movement from changing volume"
    );
}

fn frame(app: &mut App, action: MenuAction) -> UiRect {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
            presentation.set_menu_view(Some(view));
            let pane = UiPoint::new(PHYSICAL[0] as f32 * 0.75, PHYSICAL[1] as f32 * 0.6).unwrap();
            for tick in 0..32_u64 {
                presentation
                    .build(
                        world.resource::<crate::player_runtime::PlayerRuntime>(),
                        &UiRuntime::new(1),
                        tick * 250,
                        PHYSICAL,
                        DpiScale::new(1.0).unwrap(),
                    )
                    .unwrap();
                assert_eq!(presentation.gui_scale_slider_track(), None);
                assert_eq!(
                    presentation.gui_scale_drag_action(UiPoint::new(0.0, 0.0).unwrap()),
                    None
                );
                if let Some(bounds) = presentation.menu_action_bounds(action) {
                    return bounds;
                }
                assert!(presentation.scroll_menu(pane, -20.0, false));
            }
            panic!("the native GUI-scale option enters the viewport after scrolling");
        })
}

fn pointer(app: &mut App, window: Entity, position: Vec2, event: Option<ButtonState>) {
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .set_cursor_position(Some(position));
    if let Some(state) = event {
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

/// Finds a usable slider press through the same public hit test as pointer input.
fn settings_slider_point(presentation: &UiPresentationRuntime, index: u16) -> Option<Vec2> {
    (0..PHYSICAL[1]).step_by(8).find_map(|y| {
        (0..PHYSICAL[0]).step_by(8).find_map(|x| {
            let point = UiPoint::new(x as f32, y as f32).unwrap();
            matches!(presentation.hit_test_menu(point), Some(MenuAction::SettingsOption(candidate, _)) if candidate == index)
                .then(|| presentation.settings_slider_thumb_contains(index, point))
                .filter(|on_thumb| *on_thumb)
                .map(|_| Vec2::new(x as f32, y as f32))
        })
    })
}

#[test]
fn settings_slider_drag_keeps_tracking_outside_hover_until_released() {
    let Some(presentation) = engine_presentation() else {
        eprintln!(
            "skipping settings_slider_drag_keeps_tracking_outside_hover_until_released: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let index = u16::try_from(
        launcher::menu::settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "field_of_view")
            .unwrap(),
    )
    .unwrap();
    let definition = &launcher::menu::settings_options::SETTINGS_OPTIONS[usize::from(index)];
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(menu)
        .insert_resource(presentation)
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
    let middle =
        app.world_mut()
            .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
                presentation.set_menu_view(Some(world.resource::<MenuRuntime>().view()));
                let build = |presentation: &mut UiPresentationRuntime, world: &World| {
                    presentation
                        .build(
                            world.resource::<crate::player_runtime::PlayerRuntime>(),
                            &UiRuntime::new(1),
                            0,
                            PHYSICAL,
                            DpiScale::new(1.0).unwrap(),
                        )
                        .unwrap();
                };
                build(&mut presentation, world);
                let sections = presentation
                    .visible_menu_actions()
                    .filter(|action| matches!(action, MenuAction::SettingsSection(_)))
                    .collect::<Vec<_>>();
                for section in sections {
                    {
                        let mut menu = world.resource_mut::<MenuRuntime>();
                        menu.activate(section);
                        let value = menu.settings_options.get(usize::from(index));
                        menu.refresh_settings_focus([MenuAction::SettingsOption(index, value)]);
                    }
                    presentation.set_menu_view(Some(world.resource::<MenuRuntime>().view()));
                    build(&mut presentation, world);
                    if let Some(point) = settings_slider_point(&presentation, index) {
                        return point;
                    }
                }
                panic!("native settings category navigation exposes the field of view slider");
            });
    pointer(&mut app, window, middle, Some(ButtonState::Pressed));
    pointer(&mut app, window, Vec2::ZERO, None);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .get(usize::from(index)),
        definition.min
    );
    pointer(
        &mut app,
        window,
        Vec2::new(PHYSICAL[0] as f32 - 1.0, PHYSICAL[1] as f32 - 1.0),
        None,
    );
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .get(usize::from(index)),
        definition.max
    );
    pointer(&mut app, window, Vec2::ZERO, Some(ButtonState::Released));
    pointer(&mut app, window, Vec2::ZERO, None);
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .get(usize::from(index)),
        definition.max
    );
}

#[test]
fn native_gui_scale_click_relayouts_and_held_pointer_never_drags_choices() {
    let Some(presentation) = engine_presentation() else {
        eprintln!(
            "skipping native_gui_scale_click_relayouts_and_held_pointer_never_drags_choices: missing installed local carriers (make assets)"
        );
        return;
    };
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.set_gui_scale_preference(None);
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(menu)
        .insert_resource(presentation)
        .add_systems(Update, (drive_menu_input, apply_gui_scale_setting).chain());
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
    let bounds = frame(&mut app, MenuAction::SettingsScale(-1));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .refresh_settings_focus([MenuAction::SettingsScale(0)]);
    let centre = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    pointer(&mut app, window, centre, Some(ButtonState::Pressed));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), 0);
    pointer(&mut app, window, centre, Some(ButtonState::Released));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -1);
    assert_eq!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .gui_scale_preference(),
        Some(3)
    );
    let bounds = frame(&mut app, MenuAction::SettingsScale(-1));
    let centre = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    pointer(&mut app, window, centre, Some(ButtonState::Pressed));
    let inert = Vec2::new(PHYSICAL[0] as f32 * 0.75, 1.0);
    assert_eq!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .hit_test_menu(UiPoint::new(inert.x, inert.y).unwrap()),
        None
    );
    pointer(&mut app, window, inert, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_offset(),
        -1,
        "holding the pointer across relayout does not capture option buttons"
    );
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().focused_action,
        Some(MenuAction::SettingsScale(-1))
    );
    assert!(
        !app.world()
            .resource::<MenuRuntime>()
            .view()
            .navigation_focus_visible
    );
    pointer(&mut app, window, inert, Some(ButtonState::Released));
    let bounds = frame(&mut app, MenuAction::SettingsScale(0));
    let centre = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    pointer(&mut app, window, centre, None);
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -1);
    pointer(&mut app, window, centre, Some(ButtonState::Pressed));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -1);
    pointer(&mut app, window, centre, Some(ButtonState::Released));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), 0);
    assert_eq!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .gui_scale_preference(),
        Some(4)
    );
}
