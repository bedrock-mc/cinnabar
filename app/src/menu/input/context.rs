//! Session, clock and desktop authority used by menu input.

use bevy::{
    input::keyboard::KeyboardInput,
    prelude::{MessageReader, Real, Res, ResMut, Time},
};

/// Groups the session and desktop authority read before menu actions.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct MenuInputContext<'w, 's> {
    pub(super) player_runtime: Res<'w, crate::player_runtime::PlayerRuntime>,
    pub(super) time: Option<Res<'w, Time<Real>>>,
    pub(super) keyboard_messages: MessageReader<'w, 's, KeyboardInput>,
    pub(super) focus: Option<ResMut<'w, client_presentation::camera::CursorFocus>>,
    pub(super) driven: Option<Res<'w, crate::camera::DrivenInput>>,
}
