//! Production adapter regressions for closing-frame ownership and cancellation.

use super::*;
use bevy::input::keyboard::{Key, NativeKey};
use semantic_input::Action;

mod controller_inventory;
mod network;

use crate::semantic_controls::{
    PendingDeviceFrame, SemanticInputRuntime, SemanticRouteState, SemanticTouchTargets,
    collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
};

struct Harness {
    app: App,
    window: Entity,
}

impl Harness {
    fn new() -> Self {
        let mut runtime = UiRuntime::new(1);
        let mut player = PlayerRuntime::new(1);
        runtime
            .publish_local_runtime_id(&mut player, 1, 42)
            .unwrap();
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Touches>()
            .init_resource::<SemanticTouchTargets>()
            .init_resource::<PendingDeviceFrame>()
            .init_resource::<SemanticRouteState>()
            .init_resource::<SemanticInputRuntime>()
            .init_resource::<SemanticInputSnapshot>()
            .init_resource::<EmoteInputConsumed>()
            .insert_resource(runtime)
            .insert_resource(player)
            .add_systems(
                Update,
                (
                    collect_raw_input,
                    route_semantic_input,
                    drive_emote_input,
                    crate::ui_runtime::interaction::drive_chat_keyboard_input,
                    finalize_semantic_input_after_ui_authority,
                    cancel_emote_from_gameplay,
                )
                    .chain(),
            );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..Default::default()
                },
                CursorOptions {
                    grab_mode: CursorGrabMode::Locked,
                    visible: false,
                    ..Default::default()
                },
                PrimaryWindow,
            ))
            .id();
        // Establish the adapter's observed session before opening its UI.
        app.update();
        Self { app, window }
    }

    fn queue(&mut self, key_code: KeyCode) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key_code);
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            state: ButtonState::Pressed,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            text: None,
            repeat: false,
            window: self.window,
        });
    }

    fn press(&mut self, key: KeyCode) {
        self.queue(key);
        self.app.update();
    }

    fn runtime(&self) -> &UiRuntime {
        self.app.world().resource::<UiRuntime>()
    }

    fn play(&mut self) {
        self.press(KeyCode::KeyB);
        assert!(self.runtime().emotes().is_open());
        self.press(KeyCode::Digit1);
        assert!(self.runtime().emotes().playback().is_some());
        assert!(!self.runtime().emotes().is_open());
    }
}

#[test]
fn closing_wheel_consumes_the_full_raw_keyboard_batch_before_chat_or_inventory() {
    for trailing_key in [KeyCode::KeyT, KeyCode::KeyE, KeyCode::Enter] {
        let mut h = Harness::new();
        h.queue(KeyCode::KeyB);
        h.queue(KeyCode::Digit1);
        h.queue(trailing_key);
        h.app
            .world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(7.0, 9.0);
        h.app.update();
        assert!(h.runtime().emotes().playback().is_some());
        assert!(!h.runtime().emotes().is_open());
        assert!(
            !h.runtime().chat_focused(),
            "owned batch opened chat with {trailing_key:?}"
        );
        assert!(
            !h.runtime().inventory_open(),
            "owned batch opened inventory with {trailing_key:?}"
        );
        assert!(h.app.world().resource::<EmoteInputConsumed>().0);
        assert!(
            h.app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .next()
                .is_none()
        );
        assert_eq!(
            h.app.world().resource::<AccumulatedMouseMotion>().delta,
            Vec2::ZERO
        );
        let cursor = h.app.world().get::<CursorOptions>(h.window).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
        assert!(!cursor.visible);
        // Consumption is frame-scoped, so a new physical chat edge still works.
        h.press(KeyCode::KeyT);
        assert!(h.runtime().chat_focused());
        assert!(!h.app.world().resource::<EmoteInputConsumed>().0);
    }
}

#[test]
fn cancel_closing_frame_also_consumes_later_raw_ui_shortcuts() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyB);
    h.queue(KeyCode::Escape);
    h.queue(KeyCode::KeyE);
    h.app.update();
    assert!(!h.runtime().emotes().is_open());
    assert!(!h.runtime().inventory_open());
    assert!(!h.runtime().chat_focused());
    assert!(h.app.world().resource::<EmoteInputConsumed>().0);
}

#[test]
fn same_frame_open_and_play_consumes_already_sampled_gameplay_controls() {
    let mut h = Harness::new();
    h.queue(KeyCode::KeyB);
    h.queue(KeyCode::Digit1);
    h.queue(KeyCode::KeyW);
    h.queue(KeyCode::Space);
    h.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    h.app.update();
    let input = h.app.world().resource::<SemanticInputSnapshot>();
    assert_eq!(input.raw_movement(), [0.0; 2]);
    for action in [Action::Jump, Action::Use] {
        assert!(!input.phase(action).held);
    }
    assert!(h.runtime().emotes().playback().is_some());
    // Once the owned physical controls are released, new gameplay still cancels.
    h.app.update();
    h.press(KeyCode::KeyW);
    assert!(h.runtime().emotes().playback().is_none());
}

#[test]
fn stationary_pointer_preserves_keyboard_selection_until_accept() {
    let mut h = Harness::new();
    {
        let mut runtime = h.app.world_mut().resource_mut::<UiRuntime>();
        let emotes = runtime.emotes_mut();
        emotes.open();
        emotes.change_emotes();
        emotes.activate_slot(1, 0);
        emotes.close();
        assert!(emotes.slots()[0].is_none());
        assert!(emotes.slots()[1].is_some());
    }
    h.app
        .world_mut()
        .get_mut::<Window>(h.window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(640.0, 360.0)));
    h.queue(KeyCode::KeyB);
    h.queue(KeyCode::ArrowRight);
    h.app.update();
    assert_eq!(h.runtime().emotes().selected_slot(), Some(1));
    h.app.update();
    assert_eq!(h.runtime().emotes().selected_slot(), Some(1));
    h.press(KeyCode::Enter);
    assert!(h.runtime().emotes().playback().is_some());
    assert!(!h.runtime().emotes().is_open());
    assert!(
        !h.runtime().chat_focused(),
        "wheel Enter cannot open the secondary chat binding"
    );
}

#[test]
fn native_controller_binding_opens_without_selecting_the_opening_direction() {
    let mut h = Harness::new();
    let pad = h.app.world_mut().spawn(Gamepad::default()).id();
    h.app.update();
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::DPadLeft);
    h.app.update();
    assert!(h.runtime().emotes().is_open());
    assert_eq!(h.runtime().emotes().selected_slot(), None);
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .reset_all();
    h.app.update();
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::DPadUp);
    h.app.update();
    assert_eq!(h.runtime().emotes().selected_slot(), Some(0));
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .reset_all();
    h.app.update();
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::South);
    h.app.update();
    assert!(h.runtime().emotes().playback().is_some());
    assert!(!h.runtime().emotes().is_open());
    assert!(!h.runtime().inventory_open());
    assert!(!h.runtime().chat_focused());
}

#[test]
fn controller_can_equip_and_play_the_left_slot_using_the_opening_button() {
    let mut h = Harness::new();
    let pad = h.app.world_mut().spawn(Gamepad::default()).id();
    h.app.update();
    let press = |h: &mut Harness, button| {
        h.app
            .world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .press(button);
        h.app.update();
        h.app
            .world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .reset_all();
        h.app.update();
    };
    press(&mut h, GamepadButton::DPadLeft);
    assert!(h.runtime().emotes().is_open());
    assert_eq!(h.runtime().emotes().selected_slot(), None);
    h.app
        .world_mut()
        .resource_mut::<UiRuntime>()
        .emotes_mut()
        .change_emotes();
    press(&mut h, GamepadButton::DPadLeft);
    assert!(h.runtime().emotes().is_equipping());
    assert_eq!(h.runtime().emotes().selected_slot(), Some(3));
    press(&mut h, GamepadButton::South);
    assert!(h.runtime().emotes().slots()[3].is_some());
    assert!(h.runtime().emotes().slots()[0].is_none());
    assert!(!h.runtime().emotes().is_equipping());
    press(&mut h, GamepadButton::DPadLeft);
    assert!(h.runtime().emotes().is_open());
    assert_eq!(h.runtime().emotes().selected_slot(), Some(3));
    press(&mut h, GamepadButton::South);
    assert!(h.runtime().emotes().playback().is_some());
    assert!(!h.runtime().emotes().is_open());
    assert!(!h.runtime().inventory_open());
    assert!(!h.runtime().chat_focused());
}

#[test]
fn movement_and_jump_cancel_against_the_current_finalized_gameplay_frame() {
    for key in [KeyCode::KeyW, KeyCode::Space] {
        let mut h = Harness::new();
        h.play();
        let before = h.app.world().resource::<SemanticInputSnapshot>();
        assert_eq!(before.raw_movement(), [0.0; 2]);
        assert!(!before.phase(Action::Jump).held);
        h.press(key);
        let current = h.app.world().resource::<SemanticInputSnapshot>();
        if key == KeyCode::KeyW {
            assert_ne!(current.raw_movement(), [0.0; 2]);
        } else {
            assert!(current.phase(Action::Jump).held);
        }
        assert!(
            h.runtime().emotes().playback().is_none(),
            "new {key:?} edge must cancel before physics"
        );
    }
}

#[test]
fn focus_loss_preserves_emotes_while_visible_menu_retires_them() {
    for lose_focus in [true, false] {
        let mut h = Harness::new();
        h.play();
        h.press(KeyCode::KeyB);
        assert!(h.runtime().emotes().is_open());
        assert!(h.runtime().emotes().playback().is_some());
        if lose_focus {
            h.app
                .world_mut()
                .get_mut::<Window>(h.window)
                .unwrap()
                .focused = false;
        } else {
            h.app
                .insert_resource(MenuRuntime::new(true, 2, "Tester".into()));
        }
        h.app.update();
        assert_eq!(h.runtime().emotes().is_open(), lose_focus);
        assert_eq!(h.runtime().emotes().playback().is_some(), lose_focus);
    }
}

#[test]
fn new_session_cannot_resume_an_old_emote() {
    let mut h = Harness::new();
    h.play();
    h.press(KeyCode::KeyB);
    h.app
        .world_mut()
        .resource_scope(|world, mut runtime: Mut<UiRuntime>| {
            let mut player = world.resource_mut::<PlayerRuntime>();
            player.begin_session(2);
            runtime.begin_session(2);
            runtime
                .publish_local_runtime_id(&mut player, 2, 43)
                .unwrap();
        });
    h.app.update();
    assert!(!h.runtime().emotes().is_open());
    assert!(h.runtime().emotes().playback().is_none());
    h.press(KeyCode::KeyB);
    assert!(
        h.runtime().emotes().is_open(),
        "new session's input remains usable"
    );
}
