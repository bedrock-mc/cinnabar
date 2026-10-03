//! Session connection systems: provisioning a core and network session for a
//! join, following transfers, and tearing sessions down after failures.

use bevy::{
    ecs::system::SystemParam,
    prelude::{AppExit, MessageWriter, Res, ResMut},
};

use crate::{
    local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset},
    movement::{LocalPhysicsController, MovementTicker},
    runtime::endpoint::bridge_endpoint_exists,
    runtime::{
        network::{NetworkConfig, NetworkHandle, ResourcePackAdmissionState},
        world::ClientWorld,
    },
    session_cleanup::SessionDirectoryGuard,
    ui_runtime::UiRuntime,
};

use std::{path::PathBuf, time::Instant};

use super::{
    CoreProcessGuard, LauncherCoreSlot, MenuRuntime, core_process::CORE_START_TIMEOUT,
    spawn_core_for_address,
};

#[derive(SystemParam)]
pub(crate) struct MenuSessionState<'w> {
    guard: ResMut<'w, CoreProcessGuard>,
    network: ResMut<'w, NetworkHandle>,
    resource_packs: ResMut<'w, ResourcePackAdmissionState>,
    runtime: ResMut<'w, UiRuntime>,
    client_world: ResMut<'w, ClientWorld>,
    movement: ResMut<'w, MovementTicker>,
    local_physics: ResMut<'w, LocalPhysicsController>,
    local_frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
    launcher: Option<ResMut<'w, LauncherCoreSlot>>,
    actor_artwork: Option<Res<'w, render::ActorArtworkPages>>,
}

type BlobCache = crate::app::ClientBlobCacheOwner;

impl MenuSessionState<'_> {
    fn retire(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        menu: &mut MenuRuntime,
    ) -> u64 {
        *self.network = NetworkHandle::disconnected();
        // The directories go only once their core has exited.
        let directory = menu.session_directory.take();
        let join = menu.join.take();
        self.guard.stop_detached(move || drop((join, directory)));
        let generation = menu.next_session_generation();
        self.resource_packs.begin_generation(generation);
        self.runtime.begin_session(player_runtime, generation);
        self.client_world.stream = None;
        self.client_world.pack_entities = None;
        self.client_world.prepared_actor_artwork = None;
        self.client_world.session_items = None;
        self.client_world.pending_surface_spawn = None;
        self.client_world.fatal_error = None;
        self.client_world.transfer_notice = None;
        self.movement.deactivate();
        self.local_physics.deactivate();
        self.local_frame.reset(LocalPlayerFrameReset::Session);
        self.interaction.invalidate();
        generation
    }
}

/// A join still provisioning while the connecting screen shows; polled each frame.
#[derive(Debug)]
pub(super) struct JoinAttempt {
    generation: u64,
    address: String,
    auth_cache: Option<PathBuf>,
    local_world: bool,
    stage: JoinStage,
}

#[derive(Debug)]
enum JoinStage {
    /// The launcher core selecting the target on a worker.
    Launcher(crossbeam_channel::Receiver<Result<PathBuf, String>>),
    /// A per-session core that must publish its endpoint by `deadline`.
    Core {
        socket_dir: PathBuf,
        directory: SessionDirectoryGuard,
        deadline: Instant,
    },
}

/// Starts provisioning a fresh core and network session for one address,
/// replacing any previous session; [`poll_join`] finishes it on later frames.
///
/// This is the single replacement-handoff path shared by user joins and
/// automatic server-transfer follows: identity-checked session directory,
/// bounded core start wait, old-network shutdown, and a fresh session
/// generation for every attempt. A running launcher core takes the join
/// instead: it selects the target over `connect.v1` and the session dials it.
fn attempt_connect(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    menu: &mut MenuRuntime,
    session: &mut MenuSessionState<'_>,
    cache: &BlobCache,
    address: String,
    auth_cache: Option<PathBuf>,
    local_world: bool,
) {
    // A replacement owns no route back into the old session, even when
    // provisioning the new endpoint fails before the connecting screen opens.
    menu.mark_disconnected();
    let generation = session.retire(player_runtime, menu);
    session.runtime.experiences.select_destination(&address);
    menu.feeds.join =
        super::view::JoinProgress::new(super::launcher_core::join_kind(&address, local_world));
    let launcher = session
        .launcher
        .as_deref()
        .and_then(|slot| slot.begin_join(&address, local_world, auth_cache.is_some()));
    let stage = match launcher {
        Some(receiver) => JoinStage::Launcher(receiver),
        // A local world exists only behind the launcher core.
        None if local_world => {
            fail_join(menu, format!("Could not open {address}: no launcher core"));
            return;
        }
        None => match start_core(
            menu,
            session,
            cache,
            &address,
            auth_cache.as_deref(),
            generation,
        ) {
            Ok(stage) => stage,
            Err(message) => {
                fail_join(menu, message);
                return;
            }
        },
    };
    menu.join = Some(JoinAttempt {
        generation,
        address,
        auth_cache,
        local_world,
        stage,
    });
    menu.mark_connecting();
}

/// Spawns a per-session core that dials `address` directly.
fn start_core(
    menu: &MenuRuntime,
    session: &mut MenuSessionState<'_>,
    cache: &BlobCache,
    address: &str,
    auth_cache: Option<&std::path::Path>,
    generation: u64,
) -> Result<JoinStage, String> {
    // Namespaced by process id like the `--address` path: a bare
    // generation counter restarts at the same value every launch, so a
    // previous run's directory would be reused for this session.
    let socket_dir = menu
        .layout
        .connect_socket_dir(std::process::id(), generation);
    // The guard owns the directory across every teardown path; an identity
    // conflict fails this connect loudly instead of reusing another
    // session's directory.
    let directory = SessionDirectoryGuard::bind(socket_dir.clone())
        .map_err(|error| format!("Could not start {address}: {error}"))?;
    let child = spawn_core_for_address(
        &menu.layout,
        &socket_dir,
        address,
        auth_cache,
        // Advertise upstream cache capability exactly because this same
        // connect hands the verified blob cache to the new network
        // session; that ownership is what makes the client answer
        // LoginSuccess with cache-enabled status downstream.
        cache.enables_upstream_client_cache(),
    )
    .map_err(|error| format!("Could not start {address}: {error}"))?;
    session.guard.replace(child);
    Ok(JoinStage::Core {
        socket_dir,
        directory,
        deadline: Instant::now() + CORE_START_TIMEOUT,
    })
}

/// Advances the pending join without blocking: starts its network session
/// once the endpoint is ready, or fails it back to the menu.
fn poll_join(
    commands: &mut bevy::prelude::Commands,
    menu: &mut MenuRuntime,
    session: &mut MenuSessionState<'_>,
    cache: &BlobCache,
) {
    let Some(mut attempt) = menu.join.take() else {
        return;
    };
    // A retired generation's attempt drops; `retire` already stopped its core.
    if attempt.generation != menu.session_generation || !menu.connecting {
        return;
    }
    let address = attempt.address.clone();
    match attempt.stage {
        JoinStage::Launcher(ref receiver) => {
            let selected = match receiver.try_recv() {
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    menu.join = Some(attempt);
                    return;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    Err("the launcher core did not answer".to_owned())
                }
                Ok(selected) => selected,
            };
            match selected {
                Ok(socket_dir) => {
                    if let Err(error) = start_network(
                        commands,
                        menu,
                        cache,
                        socket_dir,
                        session.actor_artwork.as_deref(),
                    ) {
                        fail_join(menu, format!("Could not connect: {error}"));
                    }
                }
                Err(error) if attempt.local_world => {
                    fail_join(menu, format!("Could not open {address}: {error}"));
                }
                // Otherwise a per-session core dials the address directly.
                Err(_) => {
                    let auth_cache = attempt.auth_cache.clone();
                    match start_core(
                        menu,
                        session,
                        cache,
                        &address,
                        auth_cache.as_deref(),
                        attempt.generation,
                    ) {
                        Ok(stage) => {
                            attempt.stage = stage;
                            menu.join = Some(attempt);
                        }
                        Err(message) => fail_join(menu, message),
                    }
                }
            }
        }
        JoinStage::Core {
            socket_dir,
            directory,
            deadline,
        } => {
            if !bridge_endpoint_exists(&socket_dir) {
                let error = if session.guard.exited() {
                    "bedrock-core exited before publishing its endpoint"
                } else if Instant::now() >= deadline {
                    "bedrock-core did not publish its endpoint"
                } else {
                    attempt.stage = JoinStage::Core {
                        socket_dir,
                        directory,
                        deadline,
                    };
                    menu.join = Some(attempt);
                    return;
                };
                session.guard.stop_detached(move || drop(directory));
                fail_join(
                    menu,
                    format!(
                        "Could not start {address}: {error} at {}",
                        socket_dir.display()
                    ),
                );
                return;
            }
            menu.bind_session_directory(directory);
            if let Err(error) = start_network(
                commands,
                menu,
                cache,
                socket_dir,
                session.actor_artwork.as_deref(),
            ) {
                let directory = menu.session_directory.take();
                session.guard.stop_detached(move || drop(directory));
                fail_join(menu, format!("Could not connect: {error}"));
            }
        }
    }
}

fn fail_join(menu: &mut MenuRuntime, message: String) {
    menu.message = Some(message);
    menu.connecting = false;
}

/// Starts the network session against the core serving `socket_dir`.
fn start_network(
    commands: &mut bevy::prelude::Commands,
    menu: &MenuRuntime,
    cache: &BlobCache,
    socket_dir: PathBuf,
    actor_artwork: Option<&render::ActorArtworkPages>,
) -> Result<(), String> {
    let replacement = crate::runtime::network::spawn_network(NetworkConfig {
        session_generation: menu.session_generation,
        socket_dir,
        display_name: menu.display_name.clone(),
        client_blob_cache: cache.cache(),
        player_skin: menu.player_skin.clone(),
        actor_artwork: actor_artwork.cloned(),
    })
    .map_err(|error| error.to_string())?;
    commands.insert_resource(replacement.movement_ticker());
    commands.insert_resource(replacement);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_connection(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut commands: bevy::prelude::Commands,
    mut exits: MessageWriter<AppExit>,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<BlobCache>,
    mut session: MenuSessionState,
    launcher_account: Option<ResMut<super::launcher_account::LauncherAccount>>,
    mut local_worlds: Option<ResMut<crate::local_worlds::LocalWorlds>>,
    audio_settings: Option<ResMut<crate::audio::AudioSettings>>,
    settings: Option<ResMut<crate::settings_runtime::RuntimeSettings>>,
) {
    menu.poll_catalog(launcher_account.is_some());
    menu.poll_saves();
    menu.sync_audio_settings(audio_settings);
    menu.sync_user_settings(settings);
    menu.sync_language(&mut session.runtime);
    let in_session = session.client_world.stream.is_some();
    let upstream_cache = client_blob_cache.enables_upstream_client_cache();
    if let Some(slot) = session.launcher.as_deref_mut() {
        let idle = menu.is_launcher() && !menu.is_connecting() && !in_session;
        slot.drive(
            &mut commands,
            &mut menu,
            idle,
            upstream_cache,
            local_worlds.as_deref_mut(),
        );
    }
    match launcher_account {
        Some(mut account) => menu.sync_account_control(&mut *account),
        None => menu.sign_out_locally(),
    }
    if let Some(worlds) = local_worlds.as_deref_mut() {
        menu.sync_local_worlds(worlds, in_session);
    }
    if menu.take_respawn_request()
        && let Some(runtime_id) = session.runtime.local_runtime_id(&player_runtime)
    {
        let generation = session.runtime.session_id();
        let packet = protocol::respawn_request_packet(runtime_id);
        let _ = session.network.send_form_packet(generation, packet);
    }
    if menu.take_exit_request() {
        // Exit stops the core inline, inside the shutdown watchdog's envelope.
        session.guard.stop();
        session.retire(&mut player_runtime, &mut menu);
        exits.write(AppExit::Success);
        return;
    }
    if menu.take_disconnect_request() {
        // A disconnect while connecting is a cancelled join, which returns to the play screen.
        let cancelled_join = menu.is_connecting();
        // Drop the old event receivers as well as stopping their worker: a
        // queued transfer must not undo this explicit disconnect later this frame.
        session.retire(&mut player_runtime, &mut menu);
        menu.mark_disconnected();
        if cancelled_join {
            menu.pending_connect = None;
            menu.enter(super::MenuScreen::Play);
        }
        return;
    }
    if menu.is_connecting() && in_session {
        menu.mark_connected();
    }
    if let Some(pending) = menu.take_pending_connect() {
        attempt_connect(
            &mut player_runtime,
            &mut menu,
            &mut session,
            &client_blob_cache,
            pending.address,
            pending.auth_cache,
            pending.local_world,
        );
    }
    poll_join(&mut commands, &mut menu, &mut session, &client_blob_cache);
}

/// Returns a failed launcher session to the menu instead of ending the process.
///
/// Runs late in the frame: the failure is recorded while network events are
/// drained, which is after the menu's own systems, so recovery has to happen
/// between that and the systems that exit on a fatal error.
pub(crate) fn recover_menu_session_failure(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut menu: ResMut<MenuRuntime>,
    mut session: MenuSessionState,
) {
    let Some(error) = session.client_world.fatal_error.clone() else {
        return;
    };
    if !menu.absorb_session_failure(&error) {
        return;
    }
    session.retire(&mut player_runtime, &mut menu);
}

/// Consumes a latched server-transfer notice and performs the bounded
/// replacement handoff.
///
/// Runs late in the frame, after the network drain latched the notice and
/// before fatal-error exits: launcher sessions tear the old session down and
/// rejoin the transferred target through the exact user-join machinery,
/// while `--address` runs keep their historical single-session behavior and
/// end with the transfer named explicitly. The automatic chain is bounded so
/// a transfer loop ends in a visible menu state instead of reconnecting
/// forever.
pub(crate) fn follow_server_transfer(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<BlobCache>,
    mut session: MenuSessionState,
) {
    let Some(notice) = session.client_world.transfer_notice.take() else {
        return;
    };
    let target = notice.host.clone();
    if !menu.is_launcher() {
        // No launcher exists to re-enter, so the one-session run ends with
        // the server-directed move named explicitly instead of followed.
        session.retire(&mut player_runtime, &mut menu);
        crate::runtime::shutdown::record_fatal_error(
            &mut session.client_world.fatal_error,
            format!(
                "server transferred to {}",
                crate::menu::format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    let Some((address, auth_cache)) = menu.transfer_handoff_target(&notice.host, notice.port)
    else {
        end_transfer_without_follow(
            &mut player_runtime,
            &mut menu,
            &mut session,
            format!("server sent an unusable transfer target ({target})"),
        );
        return;
    };
    if !menu.consume_transfer_chain_hop() {
        end_transfer_without_follow(
            &mut player_runtime,
            &mut menu,
            &mut session,
            format!(
                "the transfer chain limit was reached at {}",
                crate::menu::format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    // The shared replacement path tears down old transport and world/UI state
    // before provisioning, so every early failure leaves no stale session.
    attempt_connect(
        &mut player_runtime,
        &mut menu,
        &mut session,
        &client_blob_cache,
        address.clone(),
        auth_cache,
        false,
    );
    if menu.is_connecting() {
        menu.message = Some(format!("Transferring to {address}…"));
    }
}

/// Ends a transferred session in the explicit cannot-follow state: the old
/// session is torn down exactly like a failure recovery and the menu names
/// what happened instead of reconnecting again.
fn end_transfer_without_follow(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    menu: &mut MenuRuntime,
    session: &mut MenuSessionState<'_>,
    reason: String,
) {
    session.retire(player_runtime, menu);
    menu.absorb_session_failure(&reason);
}

#[cfg(test)]
mod session_failure_message_tests {
    use super::super::MenuRuntime;
    use super::super::disconnect::{DisconnectBody, describe};

    const KICK: &str =
        "server disconnected: We've detected movement cheats (network read failed: closed)";

    #[test]
    fn launcher_shows_the_server_reason_on_the_disconnect_screen() {
        let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
        assert!(menu.absorb_session_failure(KICK));
        let error = menu.view().disconnect_message.unwrap();
        assert_eq!(
            describe(&error).body,
            DisconnectBody::Server("We've detected movement cheats".to_owned())
        );
    }

    #[test]
    fn launcher_words_a_transport_failure_as_vanilla_does() {
        let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
        assert!(menu.absorb_session_failure("network session failed: closed"));
        let error = menu.view().disconnect_message.unwrap();
        assert_eq!(
            describe(&error).body,
            DisconnectBody::Key("disconnect.closed")
        );
    }
}

#[cfg(test)]
#[path = "transfer_follow_tests.rs"]
mod transfer_follow_tests;

#[cfg(test)]
#[path = "session_teardown_tests.rs"]
mod session_teardown_tests;
