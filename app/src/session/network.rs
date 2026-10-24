//! Finishes the existing endpoint provisioning and starts the shared session worker.

use super::*;

/// Advances the pending join without blocking: starts its network session
/// once the endpoint is ready, or fails it back to the menu.
pub(super) fn poll_join(
    commands: &mut Commands,
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    cache: &BlobCache,
) {
    let controller = &mut *session.controller;
    if controller.join.is_none() {
        return;
    }
    // Capture admitted mods even when a cold cache or transfer delays their preparation.
    if session
        .mod_registration
        .as_ref()
        .is_some_and(|watcher| watcher.preparing())
    {
        return;
    }
    let Some(collisions) = session.collisions.as_deref() else {
        return;
    };
    let client_random_id = match controller.poll_client_random_id() {
        Ok(Some(id)) => id,
        Ok(None) => return,
        Err(message) => {
            controller.join = None;
            controller.fail_join(menu, message);
            return;
        }
    };
    let attempt = controller.join.take().expect("pending join");
    let server_address = attempt.address.clone();
    #[cfg(feature = "native-networking")]
    let native_auth_cache = attempt.auth_cache.clone();
    let Some(endpoint) = attempt.poll(controller.generation, controller.connecting) else {
        return;
    };
    let mut login_settings = controller.login_settings(menu, session.input.as_deref());
    login_settings.server_address = server_address;
    #[cfg(feature = "native-networking")]
    let native_account = if controller.native_friend.is_none()
        && controller.native_gathering.is_none()
        && controller.native_local_world.is_none()
    {
        controller
            .startup_account
            .take()
            .or_else(|| native_auth_cache.map(|path| controller.native_account_for(menu, path)))
    } else {
        None
    };
    #[cfg(feature = "native-networking")]
    let native_friend = match controller
        .native_friend
        .take()
        .map(|selection| {
            controller
                .friend_cleanup
                .request(selection, &controller.network_workers)
        })
        .transpose()
    {
        Ok(request) => request,
        Err(error) => {
            controller.fail_join(menu, error.to_string());
            return;
        }
    };
    if let Err(error) = start_network(
        commands,
        menu,
        controller.generation,
        cache,
        #[cfg(feature = "native-networking")]
        controller.network_workers.clone(),
        endpoint,
        #[cfg(feature = "native-networking")]
        native_account,
        #[cfg(feature = "native-networking")]
        native_friend,
        #[cfg(feature = "native-networking")]
        controller.native_gathering.take(),
        #[cfg(feature = "native-networking")]
        controller.native_local_world.take(),
        login_settings,
        client_random_id,
        session.actor_artwork.as_deref().map(|artwork| &artwork.0),
        session.ui_catalog.as_deref(),
        session
            .packet_mods
            .as_ref()
            .and_then(|mods| mods.session_relay()),
        crate::runtime::network::session_registries::RegistryBase::new(
            &session.state.client_world.runtime_assets,
            collisions,
        ),
    ) {
        controller.fail_join(menu, format!("Could not connect: {error}"));
    }
}

/// Starts the original native session worker against the admitted target.
#[allow(
    clippy::too_many_arguments,
    reason = "joining forwards captured session inputs and optional presentation assets"
)]
fn start_network(
    commands: &mut Commands,
    menu: &MenuRuntime,
    session_generation: u64,
    cache: &BlobCache,
    #[cfg(feature = "native-networking")] retirement: tokio_util::task::TaskTracker,
    endpoint: protocol::SessionEndpoint,
    #[cfg(feature = "native-networking")] native_account: Option<
        std::sync::Arc<launcher_host::native_account::NativeAccount>,
    >,
    #[cfg(feature = "native-networking")] native_friend: Option<
        crate::runtime::network::session::friend::FriendConnectionRequest,
    >,
    #[cfg(feature = "native-networking")] native_gathering: Option<
        launcher_host::native_account::NativeGatheringSelection,
    >,
    #[cfg(feature = "native-networking")] native_local_world: Option<
        crate::runtime::network::session::local_world::WorldConnectionRequest,
    >,
    login_settings: protocol::LoginSettings,
    client_random_id: protocol::ClientRandomId,
    actor_artwork: Option<&render_model::ActorArtworkPages>,
    ui_catalog: Option<&crate::runtime::network::PackUiCatalog>,
    packet_relay: Option<std::sync::Arc<dyn protocol::PacketRelay>>,
    registry_base: crate::runtime::network::session_registries::RegistryBase,
) -> Result<(), String> {
    bridge::join_profile::JoinProfile::current().mark("endpoint_ready");
    let execution = menu.io_execution().map_err(|error| error.to_string())?;
    let replacement = crate::runtime::network::session::spawn_network(
        NetworkConfig {
            registry_base,
            session_generation: simulation_clock::identity::SessionGeneration::new(
                session_generation,
            ),
            endpoint,
            #[cfg(feature = "native-networking")]
            native_account,
            #[cfg(feature = "native-networking")]
            native_friend,
            #[cfg(feature = "native-networking")]
            native_gathering,
            #[cfg(feature = "native-networking")]
            native_local_world,
            display_name: menu.display_name().to_owned(),
            client_random_id,
            client_blob_cache: cache.cache(),
            player_skin: menu.player_skin().clone(),
            login_settings,
            actor_artwork: actor_artwork.cloned(),
            ui_catalog: ui_catalog.map(|base| base.0.clone()),
            log_dir: menu.layout().log_dir(),
            record_session: menu.settings_snapshot().0.record_network_session(),
        },
        execution,
        packet_relay,
        #[cfg(feature = "native-networking")]
        retirement,
    )
    .map_err(|error| error.to_string())?;
    commands.insert_resource(
        client_interaction::movement::MovementTicker::with_epoch_publisher(
            replacement.physics_epoch_publisher(),
        ),
    );
    commands.insert_resource(replacement);
    Ok(())
}

/// Replaces the original native session and retains the new endpoint behind the join fence.
pub(super) fn attempt_connect(
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    intent: JoinIntent,
    native_transport_override: Option<protocol::NativeTransport>,
) {
    let profile = bridge::join_profile::JoinProfile::current();
    if profile.id().is_none() {
        bridge::join_profile::JoinProfile::begin();
    }
    bridge::join_profile::JoinProfile::current().mark("session_controller");
    let JoinIntent {
        address,
        auth_cache,
        local_world,
    } = intent;
    menu.remember_retry_target(&address, auth_cache.as_deref(), local_world);
    // A replacement owns no route back into the old session, even when
    // provisioning the new endpoint fails before the connecting screen opens.
    menu.show_home();
    let generation = session.retire();
    session
        .state
        .runtime
        .experiences
        .select_destination(&address);
    menu.begin_join_progress(&address, local_world);
    let presence = session.controller.select_remote_destination(&address);
    if local_world {
        *presence = presence_target(&address, true);
        presence.join = menu.hosted_world_address();
        presence.max_players = menu.hosted_world_max_players();
    } else {
        presence.badge = menu.featured_badge(&address);
        presence.max_players = menu.destination_max_players(&address);
    }
    session.controller.local_world = local_world;
    if let Some(id) = menu.local_world_identity().filter(|_| local_world) {
        session.controller.destination_identity = Some(format!("local:{id}"));
    }
    let controller = &mut *session.controller;
    if let Err(message) = controller.prepare_client_random_id(menu.layout()) {
        controller.fail_join(menu, message);
        return;
    }
    let endpoint = match controller.prepare_endpoint(
        menu,
        &address,
        auth_cache.as_deref(),
        native_transport_override,
    ) {
        Ok(endpoint) => endpoint,
        Err(message) => {
            controller.fail_join(menu, message);
            return;
        }
    };
    controller.join = Some(JoinAttempt {
        generation,
        address,
        auth_cache,
        local_world,
        endpoint,
    });
    controller.connecting = true;
    menu.show_connecting();
}
