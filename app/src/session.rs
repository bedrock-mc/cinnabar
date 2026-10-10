//! Session lifecycle: the one owner of joins, transfers, the session core, its
//! runtime directory and teardown. The menu raises intents and renders the
//! [`SessionStatus`] published here.

use bevy::{
    ecs::system::SystemParam,
    prelude::{AppExit, Commands, MessageWriter, Res, ResMut, Resource},
};

use client_ui::ui_runtime::UiRuntime;
use {
    crate::{
        menu::{
            CoreProcessGuard, LauncherCoreSlot, MenuRuntime, core_process::CORE_START_TIMEOUT,
            server_trust::SessionTrust, spawn_core_for_address,
        },
        movement::{LocalPhysicsController, MovementTicker},
        player_runtime::PlayerRuntime,
        runtime::{
            network::{CompiledStacks, NetworkConfig, NetworkHandle, ResourcePackAdmissionState},
            shutdown::record_fatal_error,
            world::{ClientWorld, TransferNotice},
        },
        session_cleanup::SessionDirectoryGuard,
    },
    client_presentation::local_player::{
        InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset,
    },
};

use std::{path::PathBuf, time::Instant};

/// Bounded number of consecutive automatic transfer-follow hops.
///
/// Mirrors the Go core's pre-login transfer-follower limit so a transfer loop
/// ends in a visible menu state; a user-initiated join starts a fresh chain.
pub(crate) const MAX_TRANSFER_CHAIN_HOPS: u32 = 8;

type BlobCache = crate::app::ClientBlobCacheOwner;
type JoinAttempt = client_session::connection::JoinAttempt<SessionDirectoryGuard>;
pub(crate) type JoinStage = client_session::connection::JoinStage<SessionDirectoryGuard>;

/// A join the menu asks the controller to make.
#[derive(Debug)]
pub(crate) struct JoinIntent {
    pub(crate) address: String,
    pub(crate) auth_cache: Option<PathBuf>,
    /// Joins the launcher core's opened local world instead of `address`.
    pub(crate) local_world: bool,
}

/// What the menu gates and renders of the session; only the controller writes it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SessionStatus {
    pub(crate) connecting: bool,
    /// A session runtime directory is bound.
    pub(crate) owns_directory: bool,
}

#[derive(Debug, Resource)]
pub(crate) struct SessionController {
    core: CoreProcessGuard, // declared first: the core stops before the directories below go
    /// Identity-checked owner of the live session's runtime directory.
    directory: Option<SessionDirectoryGuard>,
    /// The join provisioning behind the connecting screen.
    join: Option<JoinAttempt>,
    generation: u64,
    /// Where the current session plays, for Discord presence and invites.
    presence: Option<rich_presence::Target>,
    /// Automatic transfer-follow hops remaining in the current chain.
    transfer_hops_remaining: u32,
    connecting: bool,
    /// Polls the per-session core this join started for its server trust question.
    trust: Option<SessionTrust>,
    /// Server packs recent joins compiled; released once no join follows.
    kept_packs: &'static CompiledStacks,
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new(CoreProcessGuard::default())
    }
}

impl SessionController {
    /// Adopts `core`, already started for a direct `--address` session.
    pub(crate) fn new(core: CoreProcessGuard) -> Self {
        Self {
            core,
            directory: None,
            join: None,
            generation: 1,
            presence: None,
            transfer_hops_remaining: MAX_TRANSFER_CHAIN_HOPS,
            connecting: false,
            trust: None,
            kept_packs: crate::runtime::network::compiled_stacks(),
        }
    }

    /// A controller that releases `kept` instead of the stacks joins share.
    #[cfg(test)]
    pub(crate) fn with_kept_packs(mut self, kept: &'static CompiledStacks) -> Self {
        self.kept_packs = kept;
        self
    }

    /// Names a direct `--address` session's destination.
    pub(crate) fn with_address(mut self, address: Option<&str>) -> Self {
        self.presence = address.map(|address| presence_target(address, false));
        self
    }

    pub(crate) fn presence_target(&self) -> Option<&rich_presence::Target> {
        self.presence.as_ref()
    }

    pub(crate) fn status(&self) -> SessionStatus {
        SessionStatus {
            connecting: self.connecting,
            owns_directory: self.directory.is_some(),
        }
    }

    fn publish(&self, menu: &mut MenuRuntime) {
        menu.observe_session(self.status());
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1).max(1);
        self.generation
    }

    /// Binds the live session's directory, releasing any previous one first.
    pub(crate) fn bind_directory(&mut self, directory: SessionDirectoryGuard) {
        self.directory = Some(directory);
    }

    fn begin_fresh_transfer_chain(&mut self) {
        self.transfer_hops_remaining = MAX_TRANSFER_CHAIN_HOPS;
    }

    /// Spends one automatic transfer-follow hop; `false` once the chain is exhausted.
    fn consume_transfer_chain_hop(&mut self) -> bool {
        if self.transfer_hops_remaining == 0 {
            return false;
        }
        self.transfer_hops_remaining -= 1;
        true
    }

    /// Returns a failed join to the menu, where no join follows to reuse the kept packs.
    fn fail_join(&mut self, menu: &mut MenuRuntime, message: String) {
        menu.show_join_failure(message);
        self.connecting = false;
        self.kept_packs.release();
    }

    /// Spawns a per-session core that dials `address` directly.
    fn start_core(
        &mut self,
        menu: &MenuRuntime,
        cache: &BlobCache,
        address: &str,
        auth_cache: Option<&std::path::Path>,
        generation: u64,
    ) -> Result<JoinStage, String> {
        // Namespaced by process id like the `--address` path: a bare
        // generation counter restarts at the same value every launch, so a
        // previous run's directory would be reused for this session.
        let socket_dir = menu
            .layout()
            .connect_socket_dir(std::process::id(), generation);
        // The guard owns the directory across every teardown path; an identity
        // conflict fails this connect loudly instead of reusing another
        // session's directory.
        let directory = SessionDirectoryGuard::bind(socket_dir.clone())
            .map_err(|error| format!("Could not start {address}: {error}"))?;
        let child = spawn_core_for_address(
            menu.layout(),
            &socket_dir,
            address,
            auth_cache,
            // Advertise upstream cache capability exactly because this same
            // connect hands the verified blob cache to the new network
            // session; that ownership is what makes the client answer
            // LoginSuccess with cache-enabled status downstream.
            cache.enables_upstream_client_cache(),
            true,
        )
        .map_err(|error| format!("Could not start {address}: {error}"))?;
        self.core.replace(child);
        self.trust = Some(SessionTrust::watch(socket_dir.clone()));
        Ok(JoinStage::Core {
            socket_dir,
            directory,
            deadline: Instant::now() + CORE_START_TIMEOUT,
        })
    }

    #[cfg(test)]
    pub(crate) fn release_directory(&mut self) {
        self.directory = None;
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(all(test, unix))]
    pub(crate) fn join_pending(&self) -> bool {
        self.join.is_some()
    }

    /// Installs a connecting join at the current generation.
    #[cfg(all(test, unix))]
    pub(crate) fn adopt_join(&mut self, address: &str, local_world: bool, stage: JoinStage) {
        self.join = Some(JoinAttempt {
            generation: self.generation,
            address: address.to_owned(),
            auth_cache: None,
            local_world,
            stage,
        });
        self.connecting = true;
    }

    #[cfg(test)]
    pub(crate) fn set_connecting(&mut self, connecting: bool) {
        self.connecting = connecting;
    }

    #[cfg(all(test, unix))]
    pub(crate) fn core_mut(&mut self) -> &mut CoreProcessGuard {
        &mut self.core
    }
}

/// Moves UI and player authority to `generation` together; repeating it is a no-op.
pub(crate) fn begin_session(ui: &mut UiRuntime, player: &mut PlayerRuntime, generation: u64) {
    if ui.session_id() != generation {
        player.begin_session(generation);
        ui.begin_session(generation);
    }
}

/// Stops local movement authority and drops the ended session's frame and interaction samples.
pub(crate) fn quiesce_local_player(
    movement: &mut MovementTicker,
    local_physics: &mut LocalPhysicsController,
    frame: &mut LocalPlayerFrameCarrier,
    interaction: &mut InteractionOriginSnapshot,
) {
    movement.deactivate();
    local_physics.deactivate();
    frame.reset(LocalPlayerFrameReset::Session);
    interaction.invalidate();
}

/// Renders a validated transfer host and port as a dialable address.
///
/// IPv6 literals are bracketed the way the Go core's dialer expects.
pub(crate) fn format_transfer_address(host: &str, port: u16) -> String {
    if host.starts_with('[') && host.ends_with(']') {
        format!("{host}:{port}")
    } else if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// The address a server-directed transfer dials, or `None` for an unusable host.
///
/// Well-formedness only, like the protocol boundary: vanilla servers
/// legitimately transfer across unrelated hosts, so there is no allowlist.
fn transfer_handoff_address(host: &str, port: u16) -> Option<String> {
    let trimmed = host.trim();
    (!trimmed.is_empty()).then(|| format_transfer_address(trimmed, port))
}

/// Where a join to `address` plays and the address a Discord invite joins. Servers (by endpoint
/// with a port), experiences and friends' worlds are joinable; a friend's world still needs the
/// joiner to see it through Xbox. Realms carry no invite, a local world's comes from its host
/// (see `MenuRuntime::hosted_world_address`), and no identifier is ever shown on the card.
fn presence_target(address: &str, local_world: bool) -> rich_presence::Target {
    use protocol::launcher_control::ConnectTarget;
    use rich_presence::{Destination, Target};
    if local_world {
        return Target {
            destination: Destination::LocalWorld(address.to_owned()),
            join: None,
            badge: None,
            max_players: None,
        };
    }
    let address = address.trim();
    let (destination, join) = match crate::menu::target_for(address) {
        ConnectTarget::RakNet(endpoint) => (Destination::Server(endpoint.clone()), Some(endpoint)),
        ConnectTarget::Gathering(_) => (Destination::Experience, Some(address.to_owned())),
        ConnectTarget::Friend(_) => (Destination::FriendWorld, Some(address.to_owned())),
        ConnectTarget::Realm(_) => (Destination::Realm, None),
    };
    Target {
        destination,
        join,
        badge: None,
        max_players: None,
    }
}

/// Whether a received invite names a destination Cinnabar itself makes joinable.
pub(crate) fn invite_joinable(address: &str) -> bool {
    presence_target(address, false).join.as_deref() == Some(address)
}

#[derive(SystemParam)]
pub(crate) struct SessionResources<'w> {
    controller: ResMut<'w, SessionController>,
    network: ResMut<'w, NetworkHandle>,
    resource_packs: ResMut<'w, ResourcePackAdmissionState>,
    runtime: ResMut<'w, UiRuntime>,
    player_runtime: ResMut<'w, PlayerRuntime>,
    client_world: ResMut<'w, ClientWorld>,
    movement: ResMut<'w, MovementTicker>,
    local_physics: ResMut<'w, LocalPhysicsController>,
    local_frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
    launcher: Option<Res<'w, LauncherCoreSlot>>,
    actor_artwork: Option<Res<'w, render::ActorArtworkPages>>,
    ui_catalog: Option<Res<'w, crate::runtime::network::PackUiCatalog>>,
    input: Option<Res<'w, crate::semantic_controls::SemanticInputSnapshot>>,
}

impl SessionResources<'_> {
    /// Retires the live session with no join to follow, also releasing the
    /// recently compiled server packs a following join would have reused.
    fn leave(&mut self) {
        self.retire();
        self.controller.kept_packs.release();
    }

    /// Ends the live session and fences a fresh generation. The core stops off
    /// the frame, and its directories go only once it has exited.
    fn retire(&mut self) -> u64 {
        *self.network = NetworkHandle::disconnected();
        let controller = &mut *self.controller;
        let directory = controller.directory.take();
        let join = controller.join.take();
        controller
            .core
            .stop_detached(move || drop((join, directory)));
        controller.connecting = false;
        controller.presence = None;
        let generation = controller.next_generation();
        self.resource_packs.begin_generation(generation);
        begin_session(&mut self.runtime, &mut self.player_runtime, generation);
        release_off_frame((
            self.client_world.stream.take(),
            self.client_world.pack_entities.take(),
            self.client_world.prepared_actor_artwork.take(),
            self.client_world.session_items.take(),
        ));
        self.client_world.pending_surface_spawn = None;
        self.client_world.fatal_error = None;
        self.client_world.transfer_notice = None;
        quiesce_local_player(
            &mut self.movement,
            &mut self.local_physics,
            &mut self.local_frame,
            &mut self.interaction,
        );
        generation
    }
}

/// Drops retired world and pack snapshots on a worker to avoid frame-thread destruction.
/// Falls back to dropping here only when the worker cannot start.
fn release_off_frame(retired: impl Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("session-release".to_owned())
        .spawn(move || drop(retired));
    if let Err(error) = spawned {
        bevy::log::warn!("session release thread unavailable, released on the frame: {error}");
    }
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
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    cache: &BlobCache,
    intent: JoinIntent,
) {
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
    session.controller.trust = None;
    session.runtime.experiences.select_destination(&address);
    menu.begin_join_progress(&address, local_world);
    let mut presence = presence_target(&address, local_world);
    if local_world {
        presence.join = menu.hosted_world_address();
        presence.max_players = menu.hosted_world_max_players();
    } else {
        presence.badge = menu.featured_badge(&address);
        presence.max_players = menu.destination_max_players(&address);
    }
    session.controller.presence = Some(presence);
    let launcher = session.launcher.as_deref().and_then(|slot| {
        slot.begin_join(
            &address,
            local_world,
            auth_cache.is_some(),
            menu.is_launcher(),
        )
    });
    let controller = &mut *session.controller;
    let stage = match launcher {
        Some(receiver) => JoinStage::Launcher(receiver),
        // A local world exists only behind the launcher core.
        None if local_world => {
            controller.fail_join(menu, format!("Could not open {address}: no launcher core"));
            return;
        }
        None => {
            match controller.start_core(menu, cache, &address, auth_cache.as_deref(), generation) {
                Ok(stage) => stage,
                Err(message) => {
                    controller.fail_join(menu, message);
                    return;
                }
            }
        }
    };
    controller.join = Some(JoinAttempt {
        generation,
        address,
        auth_cache,
        local_world,
        stage,
    });
    controller.connecting = true;
    menu.show_connecting();
}

/// Advances the pending join without blocking: starts its network session
/// once the endpoint is ready, or fails it back to the menu.
fn poll_join(
    commands: &mut Commands,
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    cache: &BlobCache,
) {
    let controller = &mut *session.controller;
    let Some(attempt) = controller.join.take() else {
        return;
    };
    use client_session::connection::JoinPoll;
    match attempt.poll(controller.generation, controller.connecting, || {
        controller.core.exited()
    }) {
        JoinPoll::Retired => {}
        JoinPoll::Pending(attempt) => controller.join = Some(attempt),
        JoinPoll::StartCore(mut attempt) => {
            match controller.start_core(
                menu,
                cache,
                &attempt.address,
                attempt.auth_cache.as_deref(),
                attempt.generation,
            ) {
                Ok(stage) => {
                    attempt.stage = stage;
                    controller.join = Some(attempt);
                }
                Err(message) => controller.fail_join(menu, message),
            }
        }
        JoinPoll::Ready {
            socket_dir,
            directory,
        } => {
            let owned_core = directory.is_some();
            if let Some(directory) = directory {
                controller.bind_directory(directory);
            }
            let login_settings = login_settings(menu, session.input.as_deref());
            if let Err(error) = start_network(
                commands,
                menu,
                controller.generation,
                cache,
                socket_dir,
                login_settings,
                session.actor_artwork.as_deref(),
                session.ui_catalog.as_deref(),
            ) {
                if owned_core {
                    let directory = controller.directory.take();
                    controller.core.stop_detached(move || drop(directory));
                }
                controller.fail_join(menu, format!("Could not connect: {error}"));
            }
        }
        JoinPoll::Failed { message, directory } => {
            if let Some(directory) = directory {
                controller.core.stop_detached(move || drop(directory));
            }
            controller.fail_join(menu, message);
        }
    }
}

/// The language, input and GUI scale the player has as the join starts, as vanilla reports them.
pub(crate) fn login_settings(
    menu: &MenuRuntime,
    input: Option<&crate::semantic_controls::SemanticInputSnapshot>,
) -> protocol::LoginSettings {
    protocol::LoginSettings {
        language_code: client_session::pack_language::active_language_code(),
        input_mode: input
            .and_then(crate::semantic_controls::SemanticInputSnapshot::snapshot)
            .map_or(protocol::PlayerInputMode::Mouse, |snapshot| {
                crate::mining::protocol_input_mode(snapshot.input_mode)
            }),
        gui_scale_offset: menu.gui_scale_offset(),
    }
}

/// Starts the network session against the core serving `socket_dir`.
fn start_network(
    commands: &mut Commands,
    menu: &MenuRuntime,
    session_generation: u64,
    cache: &BlobCache,
    socket_dir: PathBuf,
    login_settings: protocol::LoginSettings,
    actor_artwork: Option<&render::ActorArtworkPages>,
    ui_catalog: Option<&crate::runtime::network::PackUiCatalog>,
) -> Result<(), String> {
    let replacement = crate::runtime::network::spawn_network(NetworkConfig {
        session_generation,
        socket_dir,
        display_name: menu.display_name().to_owned(),
        client_blob_cache: cache.cache(),
        player_skin: menu.player_skin().clone(),
        login_settings,
        actor_artwork: actor_artwork.cloned(),
        ui_catalog: ui_catalog.map(|base| base.0.clone()),
    })
    .map_err(|error| error.to_string())?;
    commands.insert_resource(replacement.movement_ticker());
    commands.insert_resource(replacement);
    Ok(())
}

/// Acts on the menu's session intents and advances the pending join.
pub(crate) fn drive_session(
    mut commands: Commands,
    mut exits: MessageWriter<AppExit>,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<BlobCache>,
    mut session: SessionResources,
) {
    session.controller.publish(&mut menu);
    // A queued answer reaches its core before a decline below retires that core.
    if let Some(trust) = session.controller.trust.as_ref() {
        menu.sync_session_trust(trust);
    }
    drive_intents(
        &mut commands,
        &mut exits,
        &mut menu,
        &client_blob_cache,
        &mut session,
    );
    session.controller.publish(&mut menu);
    if !session.controller.connecting {
        session.controller.trust = None;
    }
    match session.controller.trust.as_ref() {
        Some(trust) => menu.sync_session_trust(trust),
        None => menu.forget_session_trust(),
    }
}

fn drive_intents(
    commands: &mut Commands,
    exits: &mut MessageWriter<AppExit>,
    menu: &mut MenuRuntime,
    cache: &BlobCache,
    session: &mut SessionResources<'_>,
) {
    menu.send_respawn_request(|| {
        let Some(runtime_id) = session.runtime.local_runtime_id(&session.player_runtime) else {
            return false;
        };
        let generation = session.runtime.session_id();
        let packet = protocol::respawn_request_packet(runtime_id);
        session.network.send_form_packet(generation, packet).is_ok()
    });
    if menu.take_exit_request() {
        // Exit stops the core inline, inside the shutdown watchdog's envelope.
        session.controller.core.stop();
        session.retire();
        exits.write(AppExit::Success);
        return;
    }
    if menu.take_disconnect_request() {
        // Loading and pause screens retain the Play page that opened the session.
        let cancelled_join = menu.is_connecting();
        // Drop the old event receivers as well as stopping their worker: a
        // queued transfer must not undo this explicit disconnect later this frame.
        session.leave();
        if cancelled_join {
            menu.cancel_join();
        } else {
            menu.show_after_disconnect();
        }
        return;
    }
    if menu.is_connecting() && session.client_world.stream.is_some() {
        session.controller.connecting = false;
        menu.show_world();
    }
    if let Some(intent) = menu.take_join_intent() {
        // A user-initiated join always starts a fresh transfer chain.
        session.controller.begin_fresh_transfer_chain();
        attempt_connect(menu, session, cache, intent);
    }
    poll_join(commands, menu, session, cache);
}

/// Returns a failed launcher session to the menu instead of ending the process.
///
/// Runs late in the frame: the failure is recorded while network events are
/// drained, which is after the menu's own systems, so recovery has to happen
/// between that and the systems that exit on a fatal error.
pub(crate) fn recover_session_failure(
    mut menu: ResMut<MenuRuntime>,
    mut session: SessionResources,
) {
    let Some(error) = session.client_world.fatal_error.clone() else {
        return;
    };
    if !menu.absorb_session_failure(&error) {
        return;
    }
    session.leave();
    session.controller.publish(&mut menu);
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
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<BlobCache>,
    mut session: SessionResources,
) {
    let Some(notice) = session.client_world.transfer_notice.take() else {
        return;
    };
    follow_transfer(&mut menu, &client_blob_cache, &mut session, notice);
    session.controller.publish(&mut menu);
}

fn follow_transfer(
    menu: &mut MenuRuntime,
    cache: &BlobCache,
    session: &mut SessionResources<'_>,
    notice: TransferNotice,
) {
    if !menu.is_launcher() {
        // No launcher exists to re-enter, so the one-session run ends with
        // the server-directed move named explicitly instead of followed.
        session.leave();
        record_fatal_error(
            &mut session.client_world.fatal_error,
            format!(
                "server transferred to {}",
                format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    let Some(address) = transfer_handoff_address(&notice.host, notice.port) else {
        menu.clear_retry_target();
        end_transfer_without_follow(
            menu,
            session,
            format!("server sent an unusable transfer target ({})", notice.host),
        );
        return;
    };
    if !session.controller.consume_transfer_chain_hop() {
        menu.clear_retry_target();
        end_transfer_without_follow(
            menu,
            session,
            format!(
                "the transfer chain limit was reached at {}",
                format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    // The shared replacement path tears down old transport and world/UI state
    // before provisioning, so every early failure leaves no stale session.
    let auth_cache = menu.launcher_auth_cache();
    attempt_connect(
        menu,
        session,
        cache,
        JoinIntent {
            address: address.clone(),
            auth_cache,
            local_world: false,
        },
    );
    if session.controller.connecting {
        menu.show_transfer(&address);
    }
}

/// Ends a transferred session in the explicit cannot-follow state: the old
/// session is torn down exactly like a failure recovery and the menu names
/// what happened instead of reconnecting again.
fn end_transfer_without_follow(
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    reason: String,
) {
    session.leave();
    menu.absorb_session_failure(&reason);
}

#[cfg(test)]
mod tests {
    use super::*;

    // A hidden-menu direct launch must still report the player's saved GUI scale.
    #[test]
    fn login_settings_carry_the_saved_gui_scale_without_a_visible_menu() {
        let layout = crate::install_layout::scratch("login-settings");
        std::fs::create_dir_all(&layout.user_config_root).unwrap();
        std::fs::write(
            layout.user_config_root.join("video-settings.json"),
            r#"{"gui_scale_offset":-1}"#,
        )
        .unwrap();
        let menu = MenuRuntime::new_with_layout(
            false,
            None,
            "Direct".to_owned(),
            layout,
            crate::player_skin::LocalPlayerSkin::generated_default("Direct"),
        );
        let settings = login_settings(&menu, None);
        assert_eq!(settings.gui_scale_offset, -1);
        assert_eq!(settings.input_mode, protocol::PlayerInputMode::Mouse);
    }

    #[test]
    fn joinable_destinations_and_ids_stay_off_the_card() {
        use rich_presence::{Destination, Target};
        let joinable = |destination, join: &str| Target {
            destination,
            join: Some(join.to_owned()),
            badge: None,
            max_players: None,
        };
        let server = |endpoint: &str| joinable(Destination::Server(endpoint.to_owned()), endpoint);
        let private = |destination| Target {
            destination,
            join: None,
            badge: None,
            max_players: None,
        };
        assert_eq!(
            presence_target(" play.example.net ", false),
            server(&format!(
                "play.example.net:{}",
                launcher::menu::DEFAULT_PORT
            ))
        );
        assert_eq!(presence_target("[::1]:19134", false), server("[::1]:19134"));
        assert_eq!(
            presence_target("::1", false),
            server(&format!("[::1]:{}", launcher::menu::DEFAULT_PORT))
        );
        assert_eq!(
            presence_target(" realm_id/123", false),
            private(Destination::Realm)
        );
        assert_eq!(
            presence_target("friend_xuid/2535400000000000", false),
            joinable(Destination::FriendWorld, "friend_xuid/2535400000000000")
        );
        let experience = format!("{}fixture", launcher::menu::EXPERIENCE_ADDRESS_PREFIX);
        assert_eq!(
            presence_target(&experience, false),
            joinable(Destination::Experience, &experience)
        );
        assert_eq!(
            presence_target("My World", true),
            private(Destination::LocalWorld("My World".into()))
        );
        assert!(invite_joinable("play.example.net:19132"));
        assert!(invite_joinable(&experience));
        assert!(invite_joinable("friend_xuid/2535400000000000"));
        assert!(!invite_joinable("realm_id/123"));
    }

    #[test]
    fn transfer_addresses_bracket_ipv6_and_leave_ordinary_hosts_untouched() {
        assert_eq!(
            format_transfer_address("game.example.net", 19133),
            "game.example.net:19133"
        );
        assert_eq!(format_transfer_address("::1", 19132), "[::1]:19132");
        assert_eq!(
            format_transfer_address("2001:db8::10", 25565),
            "[2001:db8::10]:25565"
        );
        assert_eq!(
            format_transfer_address("[2001:db8::10]", 25565),
            "[2001:db8::10]:25565",
            "an already-bracketed transfer literal must not be bracketed twice",
        );
    }

    #[test]
    fn handoff_targets_are_well_formed_without_any_host_allowlist() {
        assert_eq!(
            transfer_handoff_address(" game.example.net ", 19133).as_deref(),
            Some("game.example.net:19133"),
            "a trimmed well-formed host is a valid target"
        );
        assert_eq!(
            transfer_handoff_address("minigames.other-host.example", 19321).as_deref(),
            Some("minigames.other-host.example:19321"),
            "cross-host transfers are legitimate vanilla behavior"
        );
        assert!(transfer_handoff_address("", 19132).is_none());
        assert!(transfer_handoff_address("   ", 19132).is_none());
    }

    /// A retired world is freed on another thread, never by the frame that leaves the server.
    #[test]
    fn a_retired_session_is_released_off_the_calling_thread() {
        struct Probe(std::sync::mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }
        let (dropped, dropper) = std::sync::mpsc::channel();
        release_off_frame((Probe(dropped), vec![0_u8; 1024]));
        let thread = dropper
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the retired session is released");
        assert_ne!(thread, std::thread::current().id());
    }

    #[test]
    fn the_automatic_transfer_chain_is_bounded() {
        let mut controller = SessionController::default();

        // A user-initiated join always starts a fresh bounded chain.
        controller.begin_fresh_transfer_chain();
        for _ in 0..MAX_TRANSFER_CHAIN_HOPS {
            assert!(controller.consume_transfer_chain_hop());
        }
        assert!(
            !controller.consume_transfer_chain_hop(),
            "an exhausted chain must refuse to follow again"
        );

        // And another user join renews it after exhaustion.
        controller.begin_fresh_transfer_chain();
        assert!(controller.consume_transfer_chain_hop());
    }
}
