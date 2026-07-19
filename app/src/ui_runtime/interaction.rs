use bevy::{
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        mouse::AccumulatedMouseMotion,
        touch::Touches,
    },
    prelude::{
        ButtonInput, KeyCode, MessageReader, MouseButton, Query, Res, ResMut, Single, Time, With,
    },
    time::Real,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use protocol::{ChatPacketError, Packet};
use ui::{ChatClipboard, ChatEditor, PointerPhase, UiAction, UiPoint};

use super::{PlatformClipboard, UiRuntime, presentation};

#[derive(Debug, PartialEq, Eq)]
pub enum ChatFlushError<E> {
    Packet(ChatPacketError),
    Transport(E),
    SessionChanged { expected: u64, actual: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastTransferAction {
    TransferSm3,
}

impl FastTransferAction {
    fn classify(message: &str) -> Option<Self> {
        (message == "/transfer sm3").then_some(Self::TransferSm3)
    }

    pub(crate) fn marker(
        self,
        session_generation: u64,
        action_ordinal: u64,
        sent_unix_ms: u64,
    ) -> String {
        let command = match self {
            Self::TransferSm3 => "/transfer sm3",
        };
        format!(
            "RUST_MCBE_FAST_TRANSFER_ACTION={}",
            serde_json::json!({
                "schema": "rust-mcbe-fast-transfer-action-v1",
                "kind": "command_sent",
                "session_generation": session_generation,
                "action_ordinal": action_ordinal,
                "command": command,
                "sent_unix_ms": sent_unix_ms,
            })
        )
    }
}

pub fn flush_chat_sends<E>(
    runtime: &mut UiRuntime,
    budget: usize,
    mut send: impl FnMut(u64, u64, Option<FastTransferAction>, Packet) -> Result<(), E>,
) -> Result<usize, ChatFlushError<E>> {
    if budget == 0 || runtime.in_flight_chat_send().is_some() {
        return Ok(0);
    }
    let mut sent = 0;
    for _ in 0..budget.min(1) {
        let Some(request) = runtime.pending_chat_sends().front() else {
            break;
        };
        if request.session != runtime.session_id() {
            return Err(ChatFlushError::SessionChanged {
                expected: runtime.session_id(),
                actual: request.session,
            });
        }
        let (sequence, packet) = runtime
            .front_chat_packet()
            .map_err(ChatFlushError::Packet)?
            .expect("the pending front was observed above");
        send(
            request.session,
            sequence,
            FastTransferAction::classify(&request.message),
            packet,
        )
        .map_err(ChatFlushError::Transport)?;
        let enqueued = runtime.mark_chat_send_enqueued(request.session, sequence);
        debug_assert!(
            enqueued,
            "only the observed FIFO front can become in flight"
        );
        sent += 1;
    }
    Ok(sent)
}

pub(crate) fn flush_chat_network(
    mut runtime: ResMut<UiRuntime>,
    network: Res<crate::runtime::network::NetworkHandle>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    runtime.service_pending_chat_autocomplete();
    match flush_chat_sends(&mut runtime, 8, |session, sequence, action, packet| {
        network.send_chat_packet(session, sequence, action, packet)
    }) {
        Ok(_)
        | Err(ChatFlushError::Transport(crate::runtime::network::PacketSendError::Full(_))) => {}
        Err(ChatFlushError::Transport(crate::runtime::network::PacketSendError::Closed(_))) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut client_world.fatal_error,
                "chat send failed because the network command channel closed".to_owned(),
            );
        }
        Err(ChatFlushError::Packet(error)) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut client_world.fatal_error,
                format!("queued chat packet became invalid: {error}"),
            );
        }
        Err(ChatFlushError::SessionChanged { expected, actual }) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut client_world.fatal_error,
                format!(
                    "queued chat packet crossed a session boundary: expected {expected}, got {actual}"
                ),
            );
        }
    }
}

pub(crate) fn drive_chat_ui_actions(
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    gamepads: Query<&Gamepad>,
    presentation: Res<presentation::UiPresentationRuntime>,
    mut runtime: ResMut<UiRuntime>,
) {
    if !runtime.chat_focused() || !window.focused {
        return;
    }
    let logical_size = [window.width(), window.height()];
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);

    if mouse_buttons.just_pressed(MouseButton::Left)
        && let Some(position) = window.cursor_position()
        && let Ok(position) = UiPoint::new(position.x, position.y)
    {
        dispatch_chat_ui_action(
            &mut runtime,
            UiAction::PointerPrimary {
                position,
                phase: PointerPhase::Pressed,
            },
            presentation.hit_test_chat_suggestion(position, logical_size),
            now_millis,
        );
    }
    for touch in touches.iter_just_pressed() {
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y) {
            dispatch_chat_ui_action(
                &mut runtime,
                UiAction::PointerPrimary {
                    position,
                    phase: PointerPhase::Pressed,
                },
                presentation.hit_test_chat_suggestion(position, logical_size),
                now_millis,
            );
        }
    }
    for gamepad in &gamepads {
        for button in [
            GamepadButton::DPadUp,
            GamepadButton::DPadDown,
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::RightTrigger,
            GamepadButton::LeftTrigger,
        ] {
            if gamepad.just_pressed(button) {
                dispatch_chat_ui_action(
                    &mut runtime,
                    gamepad_chat_action(button).expect("the mapped button list is exhaustive"),
                    None,
                    now_millis,
                );
            }
        }
    }
}

pub(crate) const fn gamepad_chat_action(button: GamepadButton) -> Option<UiAction> {
    match button {
        GamepadButton::DPadUp => Some(UiAction::Navigate([0, -1])),
        GamepadButton::DPadDown => Some(UiAction::Navigate([0, 1])),
        GamepadButton::South => Some(UiAction::Accept),
        GamepadButton::East => Some(UiAction::Cancel),
        GamepadButton::RightTrigger => Some(UiAction::TabNext),
        GamepadButton::LeftTrigger => Some(UiAction::TabPrevious),
        _ => None,
    }
}

pub(crate) fn dispatch_chat_ui_action(
    runtime: &mut UiRuntime,
    action: UiAction,
    suggestion_hit: Option<usize>,
    now_millis: u64,
) -> bool {
    match action {
        UiAction::Cancel => {
            runtime.close_chat();
            true
        }
        UiAction::Accept if runtime.chat_suggestions().is_empty() => {
            if runtime.queue_chat_send(now_millis).is_err() {
                return false;
            }
            runtime.close_chat();
            true
        }
        _ => runtime.handle_chat_ui_action_with_suggestion_hit(action, suggestion_hit),
    }
}

fn is_chat_paste_shortcut(key: KeyCode, keys: &ButtonInput<KeyCode>) -> bool {
    key == KeyCode::KeyV
        && (keys.pressed(KeyCode::ControlLeft)
            || keys.pressed(KeyCode::ControlRight)
            || keys.pressed(KeyCode::SuperLeft)
            || keys.pressed(KeyCode::SuperRight))
        && !keys.pressed(KeyCode::AltLeft)
        && !keys.pressed(KeyCode::AltRight)
}

pub(crate) fn paste_chat_shortcut<C: ChatClipboard>(
    runtime: &mut UiRuntime,
    key: KeyCode,
    keys: &ButtonInput<KeyCode>,
    clipboard: &mut C,
) -> bool {
    if !is_chat_paste_shortcut(key, keys) {
        return false;
    }
    let _ = runtime.paste_chat_text(clipboard);
    true
}

pub(crate) fn drive_chat_keyboard_input(
    mut keyboard_messages: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut runtime: ResMut<UiRuntime>,
) {
    let (window, mut cursor) = window.into_inner();
    if !window.focused {
        if runtime.chat_focused() {
            runtime.close_chat();
        }
        return;
    }

    let mut consumed_gameplay = runtime.chat_focused();
    for input in keyboard_messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        if !runtime.chat_focused() {
            match input.key_code {
                KeyCode::KeyT => {
                    runtime.open_chat();
                    consumed_gameplay = true;
                }
                KeyCode::Slash => {
                    runtime.open_chat();
                    let _ = runtime.insert_chat_text("/");
                    consumed_gameplay = true;
                }
                _ => {}
            }
            continue;
        }

        consumed_gameplay = true;
        let selecting = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if paste_chat_shortcut(&mut runtime, input.key_code, &keys, &mut PlatformClipboard) {
            continue;
        }
        match input.key_code {
            KeyCode::Escape => {
                runtime.close_chat();
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if runtime.chat_suggestions().is_empty() {
                    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if runtime.queue_chat_send(now_millis).is_ok() {
                        runtime.close_chat();
                    }
                } else {
                    runtime.handle_chat_ui_action(UiAction::Accept);
                }
            }
            KeyCode::Backspace => runtime.backspace_chat_text(),
            KeyCode::Delete => runtime.delete_chat_text(),
            KeyCode::ArrowLeft => {
                if selecting {
                    runtime.mutate_chat_editor(ChatEditor::select_left);
                } else {
                    runtime.move_chat_cursor_left();
                }
            }
            KeyCode::ArrowRight => {
                if selecting {
                    runtime.mutate_chat_editor(ChatEditor::select_right);
                } else {
                    runtime.move_chat_cursor_right();
                }
            }
            KeyCode::Home => runtime.move_chat_cursor_home(selecting),
            KeyCode::End => runtime.move_chat_cursor_end(selecting),
            KeyCode::ArrowUp => {
                if runtime.chat_suggestions().is_empty() {
                    runtime.show_older_chat_history();
                } else {
                    runtime.handle_chat_ui_action(UiAction::Navigate([0, -1]));
                }
            }
            KeyCode::ArrowDown => {
                if runtime.chat_suggestions().is_empty() {
                    runtime.show_newer_chat_history();
                } else {
                    runtime.handle_chat_ui_action(UiAction::Navigate([0, 1]));
                }
            }
            KeyCode::Tab => {
                runtime.handle_chat_ui_action(if selecting {
                    UiAction::TabPrevious
                } else {
                    UiAction::TabNext
                });
            }
            _ => {
                let modified = keys.pressed(KeyCode::ControlLeft)
                    || keys.pressed(KeyCode::ControlRight)
                    || keys.pressed(KeyCode::AltLeft)
                    || keys.pressed(KeyCode::AltRight)
                    || keys.pressed(KeyCode::SuperLeft)
                    || keys.pressed(KeyCode::SuperRight);
                if !modified
                    && let Some(text) = input.text.as_deref()
                    && !text.chars().any(char::is_control)
                {
                    let _ = runtime.insert_chat_text(text);
                }
            }
        }
    }

    if consumed_gameplay {
        suppress_gameplay_input_for_chat(
            &runtime,
            &mut cursor,
            &mut keys,
            &mut mouse_buttons,
            &mut mouse_motion,
        );
        // A send/cancel closes chat before suppression, but that same physical
        // key must still be consumed for the current frame.
        if !runtime.chat_focused() {
            restore_gameplay_input_after_chat(
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
    }
}

pub(crate) fn restore_gameplay_input_after_chat(
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = bevy::math::Vec2::ZERO;
}

pub(crate) fn suppress_gameplay_input_for_chat(
    runtime: &UiRuntime,
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    if !runtime.chat_focused() {
        return;
    }
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = bevy::math::Vec2::ZERO;
}
