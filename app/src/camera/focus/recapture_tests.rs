use super::{tests::focus_app, *};
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    time::Real,
};
use client_shell::{
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use launcher::menu::MenuAction;
use protocol::{ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryEvent};
use ui_engine::test_support::fixture_font;

/// Builds the real pause-menu input boundary at a selected desktop scale.
fn pause_app(scale: f32) -> (App, Entity) {
    let (mut app, window) = focus_app();
    let mut menu = MenuRuntime::new(false, 2, "test".into());
    menu.open_pause();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.set_menu_view(Some(menu.view()));
    presentation
        .build(
            app.world().resource::<client_state::PlayerRuntime>(),
            app.world().resource::<UiRuntimeResource>(),
            0,
            [1280, 720],
            ui::DpiScale::new(scale).unwrap(),
        )
        .unwrap();
    app.init_resource::<Time<Real>>()
        .init_resource::<Touches>()
        .add_message::<KeyboardInput>()
        .insert_resource(menu)
        .insert_resource(UiPresentationResource(presentation))
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .add_systems(
            Update,
            drive_menu_input.before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    {
        let mut window = app.world_mut().get_mut::<Window>(window).unwrap();
        window.resolution.set_physical_resolution(1280, 720);
        window.resolution.set_scale_factor_override(Some(scale));
    }
    app.update();
    (app, window)
}

/// Checks the cursor request submitted to the desktop backend after a screen closes.
pub(super) fn assert_centred_capture(app: &App, entity: Entity) {
    let window = app.world().get::<Window>(entity).unwrap();
    let cursor = app.world().get::<CursorOptions>(entity).unwrap();
    assert!(window.focused);
    assert_eq!(window.cursor_position(), Some(window.size() * 0.5));
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
    assert_eq!(
        app.world().resource::<AccumulatedMouseMotion>().delta,
        Vec2::ZERO
    );
}

#[test]
fn escape_from_pause_recentres_a_pointer_outside_the_focused_window() {
    for scale in [1.0, 2.0] {
        for position in [None, Some(Vec2::new(-100.0, 300.0))] {
            let (mut app, window) = pause_app(scale);
            app.world_mut()
                .get_mut::<Window>(window)
                .unwrap()
                .set_cursor_position(position);
            assert_eq!(
                app.world().get::<Window>(window).unwrap().cursor_position(),
                None
            );
            app.world_mut()
                .resource_mut::<AccumulatedMouseMotion>()
                .delta = Vec2::new(25.0, -8.0);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });

            app.update();

            assert!(!app.world().resource::<MenuRuntime>().is_visible());
            assert_centred_capture(&app, window);
        }
    }
}

#[test]
fn resume_button_recentres_and_consumes_the_menu_click() {
    let (mut app, window) = pause_app(1.0);
    let presentation = app.world().resource::<UiPresentationResource>();
    let point = (0..720)
        .step_by(8)
        .flat_map(|y| {
            (0..1280)
                .step_by(8)
                .map(move |x| ui::UiPoint::new(x as f32, y as f32).unwrap())
        })
        .find(|point| presentation.hit_test_menu(*point) == Some(MenuAction::PauseResume))
        .unwrap();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(point.x(), point.y())));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    app.update();

    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    assert_centred_capture(&app, window);
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
}

/// Delivers one server container event through the UI's production inventory queue.
fn inventory_event(app: &mut App, sequence: u64, event: InventoryEvent) {
    app.world_mut()
        .resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
            let mut runtime = world.resource_mut::<UiRuntimeResource>();
            runtime
                .enqueue_inventory_event(&mut player, 1, sequence, event)
                .unwrap();
            runtime.drain_pending_inventory(&mut player);
        });
}

/// Opens a server-owned container with the pointer outside the still-focused window.
fn container_app() -> (App, Entity) {
    let (mut app, window) = focus_app();
    app.world_mut()
        .resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
            let mut runtime = world.resource_mut::<UiRuntimeResource>();
            runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
            runtime
                .publish_local_runtime_id(&mut player, 1, 42)
                .unwrap();
        });
    inventory_event(
        &mut app,
        1,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
    );
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(None);
    app.update();
    assert!(app.world().resource::<UiRuntimeResource>().inventory_open());
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::None
    );
    (app, window)
}

/// Closes the container from the server without a local input edge.
fn close_container(app: &mut App) {
    inventory_event(
        app,
        2,
        InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(2),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            server_initiated: true,
        }),
    );
    assert!(!app.world().resource::<UiRuntimeResource>().inventory_open());
}

#[test]
fn server_close_recentres_even_when_a_screen_adapter_already_requested_lock() {
    for already_locked in [false, true] {
        let (mut app, window) = container_app();
        close_container(&mut app);
        if already_locked {
            let mut cursor = app.world_mut().get_mut::<CursorOptions>(window).unwrap();
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(7.0, 3.0);

        app.update();

        assert_centred_capture(&app, window);
    }
}

#[test]
fn server_close_does_not_warp_or_capture_after_focus_loss() {
    let (mut app, window) = container_app();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    close_container(&mut app);
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();

    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        None
    );
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::None
    );
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
}

#[test]
fn continuing_gameplay_does_not_repeat_the_centre_reset() {
    let (mut app, window) = container_app();
    close_container(&mut app);
    app.update();
    assert_centred_capture(&app, window);
    let position = Some(Vec2::new(100.0, 80.0));
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(position);
    let delta = Vec2::new(3.0, 2.0);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = delta;

    app.update();

    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        position
    );
    assert_eq!(
        app.world().resource::<AccumulatedMouseMotion>().delta,
        delta
    );
}

#[test]
fn driven_screen_return_does_not_warp_the_desktop_pointer() {
    let (mut app, window) = container_app();
    app.init_resource::<DrivenInput>();
    close_container(&mut app);

    app.update();

    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        None
    );
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::None
    );
}
