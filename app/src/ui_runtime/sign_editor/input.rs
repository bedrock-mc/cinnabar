//! Keyboard and pointer handling for the sign editor, and the request that opens it.

use bevy::{
    input::{ButtonState, keyboard::KeyboardInput, mouse::AccumulatedMouseMotion},
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Res, ResMut, Single, Window, With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use crate::{
    block_entities::{BlockEntityFont, BlockEntityRuntime},
    menu::MenuRuntime,
    runtime::{network::NetworkHandle, world::ClientWorld},
};
use client_ui::ui_runtime::sign_editor::{MAX_LINE_DESIGN_PIXELS, SignEdit};
use client_ui::ui_runtime::{
    UiRuntime, interaction::restore_gameplay_input_after_chat, presentation::UiPresentationRuntime,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_sign_editor(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut keyboard: MessageReader<KeyboardInput>,
    menu: Option<Res<MenuRuntime>>,
    presentation: Res<UiPresentationRuntime>,
    network: Option<Res<NetworkHandle>>,
    font: Option<Res<BlockEntityFont>>,
    measure: Option<ResMut<BlockEntityRuntime>>,
    mut client_world: ResMut<ClientWorld>,
    mut runtime: ResMut<UiRuntime>,
    mut owned_last_frame: Local<bool>,
    collisions: Option<Res<crate::movement::PhysicsCollisionRegistries>>,
) {
    let (window, mut cursor) = window.into_inner();
    if menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        runtime.sign_editor_mut().close();
        keyboard.clear();
        *owned_last_frame = false;
        return;
    }
    if !runtime.sign_editor().is_open()
        && let Some(stream) = client_world.stream.as_mut()
        && let Some(request) = stream.take_pending_sign_edit()
    {
        let [x, y, z] = request.position;
        let block = collisions.as_deref().and_then(|collisions| {
            let key = world::BlockEntityKey::new(stream.current_dimension(), x, y, z);
            let runtime_id = stream
                .collision_store()
                .sub_chunk(key.sub_chunk())?
                .runtime_id(0, (x & 15) as u8, (y & 15) as u8, (z & 15) as u8)?;
            collisions.block_identifier(stream.network_id_mode(), runtime_id)
        });
        let base = stream
            .collision_store()
            .block_entity(world::BlockEntityKey::new(
                stream.current_dimension(),
                x,
                y,
                z,
            ))
            .and_then(|nbt| nbt.parse())
            .unwrap_or_default();
        // The keyboard events of the frame that opened the editor belong to the game.
        keyboard.clear();
        runtime
            .sign_editor_mut()
            .open(SignEdit::new(request.position, request.front, base).with_block(block));
    }
    if !runtime.sign_editor().is_open() {
        keyboard.clear();
        if *owned_last_frame && !runtime.ui_focused(&player_runtime) && window.focused {
            restore_gameplay_input_after_chat(&mut cursor, &mut keys, &mut mouse, &mut motion);
        }
        *owned_last_frame = false;
        return;
    }
    *owned_last_frame = true;
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    let mut measure = measure;
    let mut finish = runtime.sign_editor_mut().take_finish_request();
    if window.focused {
        for event in keyboard
            .read()
            .filter(|event| event.state == ButtonState::Pressed)
        {
            let Some(edit) = runtime.sign_editor_mut().active_mut() else {
                break;
            };
            match event.key_code {
                KeyCode::Escape => finish = true,
                KeyCode::Enter | KeyCode::NumpadEnter => finish |= edit.newline(),
                KeyCode::Backspace => edit.backspace(),
                KeyCode::Delete => edit.delete(),
                KeyCode::ArrowLeft => edit.left(),
                KeyCode::ArrowRight => edit.right(),
                KeyCode::ArrowUp => edit.up(),
                KeyCode::ArrowDown => edit.down(),
                KeyCode::Home => edit.home(),
                KeyCode::End => edit.end(),
                _ => {
                    for character in event.text.iter().flat_map(|text| text.chars()) {
                        edit.insert(character, |line| {
                            match (font.as_deref(), measure.as_deref_mut()) {
                                (Some(font), Some(measure)) => measure
                                    .line_width_design_pixels(&font.0, line)
                                    .is_some_and(|width| width <= MAX_LINE_DESIGN_PIXELS),
                                // Without a font there is nothing to measure against.
                                _ => line.chars().count() <= 15,
                            }
                        });
                    }
                }
            }
        }
        if mouse.just_pressed(MouseButton::Left)
            && let Some(point) = window
                .cursor_position()
                .and_then(|point| ui::UiPoint::new(point.x, point.y).ok())
            && presentation.sign_editor_exit_hit(point)
        {
            finish = true;
        }
    } else {
        keyboard.clear();
    }
    keys.reset_all();
    motion.delta = bevy::math::Vec2::ZERO;
    if !finish {
        return;
    }
    if !runtime.sign_editor_mut().finish(|packet| {
        network
            .as_deref()
            .ok_or(())?
            .send_inventory_packet(packet)
            .map_err(|_| ())
    }) {
        return;
    }
    if !runtime.ui_focused(&player_runtime) {
        restore_gameplay_input_after_chat(&mut cursor, &mut keys, &mut mouse, &mut motion);
        *owned_last_frame = false;
    }
}
