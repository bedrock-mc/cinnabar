use crate::ui_resources::UiPresentationResource;
use bevy::{
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::MouseButtonInput,
        touch::{TouchInput, TouchPhase, touch_screen_input_system},
    },
    prelude::*,
    window::{CursorOptions, PrimaryWindow, WindowResolution},
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use ui::{DpiScale, UiPoint, UiRect};

use super::super::{MenuClipboard, drive_menu_input};
use {
    crate::menu::MenuRuntime,
    launcher::menu::{MenuAction, MenuScreen},
};

const PHYSICAL: [u32; 2] = [1280, 720];

fn fixture() -> (App, Entity) {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.set_player_preview_skin(None, Default::default());
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::DressingRoom));
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
    frame(&mut app);
    (app, window)
}

fn frame(app: &mut App) -> [f32; 4] {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationResource>| {
            presentation.set_menu_view(Some(view));
            presentation.set_player_preview_skin(None, Default::default());
            presentation
                .build(
                    world.resource::<client_state::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    PHYSICAL,
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            presentation.menu_player_preview_angles()
        })
}

fn centre(bounds: UiRect) -> Vec2 {
    Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    )
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
        if state == ButtonState::Pressed {
            buttons.press(MouseButton::Left);
        } else {
            buttons.release(MouseButton::Left);
        }
    }
    app.update();
}

#[test]
fn character_drag_owns_mouse_until_release_and_hover_only_moves_the_head() {
    let (mut app, window) = fixture();
    let initial = frame(&mut app);
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_player_preview_bounds()
        .unwrap();
    let start = centre(bounds);
    pointer(&mut app, window, start, Some(ButtonState::Pressed));
    let outside = Vec2::new(PHYSICAL[0] as f32 - 2.0, 2.0);
    pointer(&mut app, window, outside, None);
    let rotated = frame(&mut app);
    assert_ne!(rotated[0], initial[0]);
    assert_eq!(
        app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::DressingRoom
    );
    assert_eq!(app.world().resource::<MenuRuntime>().view().hovered, None);
    pointer(&mut app, window, outside, Some(ButtonState::Released));
    assert_eq!(frame(&mut app)[0], rotated[0]);
    pointer(&mut app, window, start, None);
    let hovering = frame(&mut app);
    assert_eq!(
        hovering[0], rotated[0],
        "release retains the final body turn"
    );
    assert_ne!(hovering[1] - hovering[0], rotated[1] - rotated[0]);
    assert_eq!(
        hovering[3], initial[3],
        "head tracking preserves the camera tilt"
    );
}

fn touch(app: &mut App, window: Entity, phase: TouchPhase, position: Vec2) {
    app.world_mut().write_message(TouchInput {
        phase,
        position,
        window,
        force: None,
        id: 17,
    });
    app.update();
}

#[test]
fn touch_character_drag_continues_outside_the_character_and_stops_on_release() {
    let (mut app, window) = fixture();
    let initial = frame(&mut app);
    let start = centre(
        app.world()
            .resource::<UiPresentationResource>()
            .menu_player_preview_bounds()
            .unwrap(),
    );
    touch(&mut app, window, TouchPhase::Started, start);
    let outside = Vec2::new(PHYSICAL[0] as f32 - 2.0, 2.0);
    touch(&mut app, window, TouchPhase::Moved, outside);
    let rotated = frame(&mut app);
    assert_ne!(rotated[0], initial[0]);
    touch(&mut app, window, TouchPhase::Ended, outside);
    assert_eq!(frame(&mut app)[0], rotated[0]);
    pointer(&mut app, window, start, None);
    assert_eq!(frame(&mut app)[0], rotated[0]);
    assert_eq!(
        app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::DressingRoom
    );
}

#[test]
fn pointer_hover_on_character_never_captures_a_later_drag_started_elsewhere() {
    let (mut app, window) = fixture();
    let initial = frame(&mut app);
    let start = centre(
        app.world()
            .resource::<UiPresentationResource>()
            .menu_player_preview_bounds()
            .unwrap(),
    );
    pointer(&mut app, window, start, None);
    let outside = Vec2::new(PHYSICAL[0] as f32 - 2.0, PHYSICAL[1] as f32 - 2.0);
    assert_eq!(
        app.world()
            .resource::<UiPresentationResource>()
            .hit_test_menu(UiPoint::new(outside.x, outside.y).unwrap()),
        None
    );
    pointer(&mut app, window, outside, Some(ButtonState::Pressed));
    pointer(&mut app, window, start, None);
    assert_eq!(frame(&mut app)[0], initial[0]);
    pointer(&mut app, window, start, Some(ButtonState::Released));
}

#[test]
fn a_modal_revokes_character_capture_after_the_background_repaints() {
    let (mut app, window) = fixture();
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_player_preview_bounds()
        .unwrap();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::OpenAccounts);
    let initial = frame(&mut app);
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
            .menu_player_preview_bounds()
            .is_none(),
        "the modal owns pointer presses over the background character"
    );
    pointer(&mut app, window, centre(bounds), Some(ButtonState::Pressed));
    pointer(
        &mut app,
        window,
        Vec2::new(PHYSICAL[0] as f32 - 2.0, 2.0),
        None,
    );
    assert_eq!(frame(&mut app)[0], initial[0]);
    pointer(
        &mut app,
        window,
        centre(bounds),
        Some(ButtonState::Released),
    );
}

/// A finger on a clipped gallery tile starts a possible pan without revealing or equipping it.
#[test]
fn cosmetic_swipe_does_not_jump_to_the_pressed_tile() {
    use launcher::dressing_room::{Action, DressingRoomSection};
    use launcher::menu::settings_options::SettingId;

    let items = render_model::hats::fixture::gallery();
    let (mut app, window) = fixture();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.set_named_option(SettingId::ScreenAnimations, 0);
        std::sync::Arc::make_mut(&mut menu.dressing_room).busy = false;
        menu.activate(MenuAction::DressingRoom(Action::SetSection(
            DressingRoomSection::Cosmetics,
        )));
        menu.focus_pointer(MenuAction::AddBack);
    }
    frame(&mut app);
    let action = MenuAction::DressingRoom(Action::SelectCosmetic(Some(items[0])));
    let initial = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(action)
        .unwrap();
    let filter = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(MenuAction::DressingRoom(Action::ToggleCosmeticFilter))
        .unwrap();
    let delta = initial.min().y() - filter.min().y() + initial.height() * 0.5;
    app.world_mut()
        .resource_mut::<UiPresentationResource>()
        .scroll_menu(
            UiPoint::new(centre(initial).x, centre(initial).y).unwrap(),
            -delta,
            true,
        );
    frame(&mut app);
    let clipped = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(action)
        .unwrap();
    assert!(clipped.height() < initial.height());
    let start = centre(clipped);
    touch(&mut app, window, TouchPhase::Started, start);
    frame(&mut app);
    assert_eq!(
        app.world()
            .resource::<UiPresentationResource>()
            .menu_action_bounds(action),
        Some(clipped),
        "touch-down must leave manually scrolled content under the finger"
    );
    touch(&mut app, window, TouchPhase::Moved, start - Vec2::Y * 80.0);
    frame(&mut app);
    touch(&mut app, window, TouchPhase::Ended, start - Vec2::Y * 80.0);
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .dressing_room
            .selected_cosmetics
            .values()
            .all(Option::is_none),
        "the swipe must not equip its starting tile"
    );
    assert!(!app.world().resource::<MenuRuntime>().dressing_room.busy);
    frame(&mut app);
    let target = items
        .iter()
        .find_map(|item| {
            let action = MenuAction::DressingRoom(Action::SelectCosmetic(Some(*item)));
            app.world()
                .resource::<UiPresentationResource>()
                .menu_action_bounds(action)
                .filter(|bounds| bounds.height() >= initial.height() - 0.5)
        })
        .expect("a fully visible tile remains tappable after the swipe");
    touch(&mut app, window, TouchPhase::Started, centre(target));
    frame(&mut app);
    touch(&mut app, window, TouchPhase::Ended, centre(target));
    assert!(
        app.world().resource::<MenuRuntime>().dressing_room.busy,
        "a completed tap still requests equipping the cosmetic"
    );
}
