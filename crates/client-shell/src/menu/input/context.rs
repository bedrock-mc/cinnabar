//! Session, clock and desktop authority used by menu input.

use bevy::{
    input::keyboard::KeyboardInput,
    prelude::{MessageReader, Real, Res, ResMut, Time},
};

/// Groups the session and desktop authority read before menu actions.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MenuInputContext<'w, 's> {
    pub(super) player_runtime: Res<'w, client_state::PlayerRuntime>,
    pub(super) time: Option<Res<'w, Time<Real>>>,
    pub(super) keyboard_messages: MessageReader<'w, 's, KeyboardInput>,
    pub(super) focus: Option<ResMut<'w, client_state::CursorFocus>>,
    pub(super) driven: Option<Res<'w, client_state::DrivenInput>>,
}

/// Refreshes key labels while leaving the same messages available to menu navigation.
pub(crate) fn observe_keyboard_labels(
    mut messages: MessageReader<KeyboardInput>,
    mut menu: ResMut<crate::menu::MenuRuntime>,
) {
    for input in messages.read() {
        super::remapping::observe_label(&mut menu, input);
    }
}
