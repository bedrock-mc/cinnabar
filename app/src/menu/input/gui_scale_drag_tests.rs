use bevy::{prelude::*, window::WindowResolution};
use ui::{DpiScale, UiRect};

use super::*;
use crate::{
    menu::{MenuAction, MenuScreen},
    ui_runtime::presentation::{
        apply_gui_scale_setting, tests::engine_hud_tests::engine_presentation,
    },
};
use client_ui::ui_runtime::UiRuntime;

const PHYSICAL: [u32; 2] = [1920, 1080];

fn frame(app: &mut App) -> UiRect {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
            presentation.set_menu_view(Some(view));
            // The pinned video section places this slider below its other controls.
            // Scroll its native pane into view before interacting with real hit regions.
            let pane = UiPoint::new(PHYSICAL[0] as f32 * 0.75, PHYSICAL[1] as f32 * 0.6).unwrap();
            for _ in 0..32 {
                presentation
                    .build(
                        world.resource::<crate::player_runtime::PlayerRuntime>(),
                        &UiRuntime::new(1),
                        0,
                        PHYSICAL,
                        DpiScale::new(1.0).unwrap(),
                    )
                    .unwrap();
                if let Some(track) = presentation.gui_scale_slider_track() {
                    return track;
                }
                assert!(
                    presentation.scroll_menu(pane, -20.0, false),
                    "the native video pane takes scrolling"
                );
            }
            panic!("the native video slider enters the viewport after scrolling");
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
        if state == ButtonState::Pressed {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
    }
    app.update();
}

fn relayout(app: &mut App) {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
            presentation.set_menu_view(Some(view));
            let first = presentation
                .build(
                    world.resource::<crate::player_runtime::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    PHYSICAL,
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            let left = UiPoint::new(0.0, 0.0).unwrap();
            let action = presentation.gui_scale_drag_action(left);
            let repeated = presentation
                .build(
                    world.resource::<crate::player_runtime::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    PHYSICAL,
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            assert_eq!(
                first.revision, repeated.revision,
                "the steady menu reuses its output"
            );
            assert_eq!(action, Some(MenuAction::SettingsScale(-2)));
            assert_eq!(
                presentation.gui_scale_drag_action(left),
                action,
                "cached menu output keeps the current unclipped drag geometry"
            );
        })
}

/// Finds a usable slider press through the same public hit test as pointer input.
fn settings_slider_point(presentation: &UiPresentationRuntime, index: u16) -> Option<Vec2> {
    (0..PHYSICAL[1]).step_by(8).find_map(|y| {
        (0..PHYSICAL[0]).step_by(8).find_map(|x| {
            let point = UiPoint::new(x as f32, y as f32).unwrap();
            matches!(presentation.hit_test_menu(point), Some(MenuAction::SettingsOption(candidate, _)) if candidate == index)
                .then_some(Vec2::new(x as f32, y as f32))
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
        super::super::settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "field_of_view")
            .unwrap(),
    )
    .unwrap();
    let definition = &super::super::settings_options::SETTINGS_OPTIONS[usize::from(index)];
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
    let view = app.world().resource::<MenuRuntime>().view();
    let middle =
        app.world_mut()
            .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
                presentation.set_menu_view(Some(view));
                for _ in 0..32 {
                    presentation
                        .build(
                            world.resource::<crate::player_runtime::PlayerRuntime>(),
                            &UiRuntime::new(1),
                            0,
                            PHYSICAL,
                            DpiScale::new(1.0).unwrap(),
                        )
                        .unwrap();
                    if let Some(point) = settings_slider_point(&presentation, index) {
                        return point;
                    }
                    assert!(presentation.scroll_menu(
                        UiPoint::new(PHYSICAL[0] as f32 * 0.75, PHYSICAL[1] as f32 * 0.6).unwrap(),
                        -20.0,
                        false
                    ));
                }
                panic!("the field of view slider enters the viewport after scrolling");
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
fn gui_scale_drag_keeps_capture_through_relayout_clamps_ends_and_releases() {
    let Some(presentation) = engine_presentation() else {
        eprintln!(
            "skipping gui_scale_drag_keeps_capture_through_relayout_clamps_ends_and_releases: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::SettingsScale(0));
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
    let track = frame(&mut app);
    let middle = Vec2::new(
        (track.min().x() + track.max().x()) / 2.0,
        (track.min().y() + track.max().y()) / 2.0,
    );

    pointer(&mut app, window, Vec2::ZERO, Some(ButtonState::Pressed));
    pointer(&mut app, window, middle, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_offset(),
        0,
        "a press outside the slider does not capture it"
    );
    pointer(&mut app, window, middle, Some(ButtonState::Released));
    pointer(&mut app, window, middle, Some(ButtonState::Pressed));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -1);
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left),
        "menu consumption clears Bevy input while raw-button capture remains held"
    );

    // The slider may move or become clipped after relayout. Capture still
    // follows its current full geometry, without scrolling it back into view.
    relayout(&mut app);
    pointer(&mut app, window, Vec2::ZERO, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_offset(),
        -2,
        "dragging beyond the left end clamps to its native value despite leaving the slider vertically"
    );
    relayout(&mut app);
    pointer(
        &mut app,
        window,
        Vec2::new(PHYSICAL[0] as f32 - 1.0, PHYSICAL[1] as f32 - 1.0),
        None,
    );
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_offset(),
        0,
        "capture follows the new layout and clamps beyond the right end"
    );

    pointer(&mut app, window, middle, Some(ButtonState::Released));
    let track = frame(&mut app);
    pointer(
        &mut app,
        window,
        Vec2::new(track.min().x(), (track.min().y() + track.max().y()) / 2.0),
        None,
    );
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_offset(),
        0,
        "release ends capture before subsequent pointer movement"
    );

    use crate::server_experiences::input::{ConsentInput, consume};
    app.insert_resource(ConsentInput(false))
        .add_systems(Update, consume.before(drive_menu_input));
    let track = frame(&mut app);
    let middle = Vec2::new(
        (track.min().x() + track.max().x()) / 2.0,
        (track.min().y() + track.max().y()) / 2.0,
    );
    pointer(&mut app, window, middle, Some(ButtonState::Pressed));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -1);
    relayout(&mut app);
    app.world_mut().resource_mut::<ConsentInput>().0 = true;
    pointer(&mut app, window, Vec2::ZERO, Some(ButtonState::Pressed));
    assert_eq!(app.world().resource::<MenuRuntime>().gui_scale_offset(), -1);
    app.world_mut().resource_mut::<ConsentInput>().0 = false;
    pointer(&mut app, window, Vec2::ZERO, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_offset(),
        -1,
        "consent cancels retained slider capture and consumes its dismissal click"
    );
}
