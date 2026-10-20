//! Direct local-player joins retain the existing manager opening through native play.

use super::*;
use crate::runtime::network::session::local_world::WorldConnectionRequest;

/// Adopts the selected process directly, preserving local UI/presence and the original session generation.
pub(super) fn begin(
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    intent: client_shell::menu::native_local_world::NativeLocalWorldIntent,
) {
    let generation = session.retire();
    let (selection, account, retirement) = intent.into_parts();
    let controller = &mut *session.controller;
    controller.begin_fresh_transfer_chain();
    controller.native_account = None;
    if let Some(account) = &account {
        let Some(path) = menu.launcher_auth_cache() else {
            controller.fail_join(menu, "Selected local world account is unavailable.".into());
            return;
        };
        if !std::sync::Arc::ptr_eq(account, &controller.native_account_for(menu, path)) {
            controller.fail_join(menu, "Local world account changed before joining.".into());
            return;
        }
    }
    let target = match selection.connection_target() {
        Ok(target) => target,
        Err(error) => {
            controller.fail_join(menu, error.to_string());
            return;
        }
    };
    if let Err(error) = controller.prepare_client_random_id(menu.layout()) {
        controller.fail_join(menu, error);
        return;
    }
    let name = selection.world_name().to_owned();
    let mut presence = presence_target(&name, true);
    presence.join = menu.hosted_world_address();
    presence.max_players = menu.hosted_world_max_players();
    controller.presence = Some(presence);
    controller.local_world = true;
    controller.destination_identity = Some(format!("local:{}", selection.world_id()));
    controller.native_local_world = Some(WorldConnectionRequest {
        selection,
        account,
        retirement,
    });
    controller.connecting = true;
    menu.begin_join_progress(&name, true);
    controller.join = Some(JoinAttempt {
        generation,
        address: name,
        auth_cache: None,
        local_world: true,
        stage: JoinStage::Prepared(protocol::SessionEndpoint::Native(target)),
    });
}
