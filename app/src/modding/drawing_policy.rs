//! Installs the current connection fence before any gameplay observation is delivered.

use client_extensions::modding::{drawing_policy::remote_permissions, multi::ModHosts};
use mod_api::drawing::{DrawingPermissions, DrawingSession};

/// Uses trusted session state, never a loopback address, to select local authority.
pub(super) fn bind(
    hosts: &mut ModHosts,
    gameplay: &super::gameplay::GameplayContext<'_>,
    ui: &client_ui::ui_runtime::UiRuntime,
) {
    let fence = gameplay
        .world
        .as_ref()
        .and_then(|world| (!world.dimension_transfer.active()).then_some(world.stream.as_ref()?))
        .map(|stream| (ui.session_id(), stream.authority().current_dimension()));
    let grant = match &ui.experiences.session.state {
        server_experience::session::State::Granted(grant) => Some(grant),
        _ => None,
    };
    for index in 0..hosts.host_count() {
        let host = hosts.host_mut(index);
        let (connection, dimension) = fence.unwrap_or_default();
        let local = gameplay
            .session
            .as_ref()
            .is_some_and(|session| session.is_local_world());
        let permissions = if connection == 0 {
            DrawingPermissions::default()
        } else if local {
            DrawingPermissions::ALL
        } else {
            remote_permissions(
                grant,
                &host.source_digest(),
                client_ui::experience_session::unix_seconds(),
            )
        };
        host.bind_drawing_session(DrawingSession {
            connection,
            dimension,
            local,
            permissions,
        });
    }
}
