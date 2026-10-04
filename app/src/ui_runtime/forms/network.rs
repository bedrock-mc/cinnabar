use super::{FormTransportError, flush_form_response};
use crate::runtime::network::{NetworkHandle, PacketSendError};
use bevy::prelude::{Res, ResMut};
use client_ui::ui_runtime::UiRuntime;

pub(crate) fn flush_server_form_network(
    mut runtime: ResMut<UiRuntime>,
    network: Res<NetworkHandle>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
) {
    let session = runtime.session_id();
    // Terminal controls retire the session before another enqueue attempt.
    if network.closed_command_has_pending_control() {
        return;
    }
    // Opening settings in a session asks the server for its settings form once.
    let in_settings = menu.as_ref().is_some_and(|menu| {
        menu.is_visible() && menu.screen() == crate::menu::MenuScreen::Settings
    });
    let store = runtime.server_forms_mut();
    store.flush_settings_request(in_settings, || {
        network
            .send_form_packet(session, protocol::server_settings_request_packet())
            .is_ok()
    });
    let _ = flush_form_response(&mut runtime, |packet| {
        network
            .send_form_packet(session, packet)
            .map_err(|error| match error {
                PacketSendError::Full(_) => FormTransportError::Full,
                PacketSendError::Closed(_) => FormTransportError::Closed,
            })
    });
}
