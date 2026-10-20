//! Opaque Gathering selection through the existing session generation and worker.

use super::*;

/// Starts one explicit allocation without interpreting its display card as a live endpoint.
pub(super) fn begin(
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    selection: launcher_host::native_account::NativeGatheringSelection,
) {
    let generation = session.retire();
    let controller = &mut *session.controller;
    controller.begin_fresh_transfer_chain();
    if let Err(error) = controller.prepare_client_random_id(menu.layout()) {
        controller.fail_join(menu, error);
        return;
    }
    let target = selection.connection_target();
    controller.native_gathering = Some(selection);
    controller.connecting = true;
    menu.begin_join_progress(&target.address, false);
    controller.join = Some(JoinAttempt {
        generation,
        address: target.address.clone(),
        auth_cache: None,
        local_world: false,
        stage: JoinStage::Prepared(protocol::SessionEndpoint::Native(target)),
    });
}

#[cfg(test)]
mod tests;
