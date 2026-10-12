//! Session lifecycle: the one owner of joins, transfers, the session core, its
//! runtime directory and teardown. The menu raises intents and renders the
//! [`SessionStatus`] published here.

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{AppExit, Commands, MessageWriter, Res, ResMut, Resource},
};
use bridge::CORE_START_TIMEOUT;
use client_runtime::network::NetworkHandle;
use client_runtime::session::{
    begin_session, format_transfer_address, release_off_frame, transfer_handoff_address,
};
use launcher_host::{
    core_process::{CoreProcessGuard, spawn_core_for_address},
    server_trust::SessionTrust,
    session_cleanup::SessionDirectoryGuard,
};

use client_shell::presence_target::presence_target;
use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::{
        network::{CompiledStacks, session::NetworkConfig},
        shutdown::record_fatal_error,
        world::{ClientWorld, TransferNotice},
    },
    client_interaction::movement::{LocalPhysicsController, MovementTicker},
    client_presentation::local_player::{
        InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset,
    },
    client_runtime::pack_admission::ResourcePackAdmissionState,
    client_shell::menu::{LauncherCoreSlot, MenuRuntime},
    client_state::PlayerRuntime,
};

use std::{path::PathBuf, time::Instant};

mod client_id;

/// Bounded number of consecutive automatic transfer-follow hops.
///
/// Mirrors the Go core's pre-login transfer-follower limit so a transfer loop
/// ends in a visible menu state; a user-initiated join starts a fresh chain.
pub(crate) const MAX_TRANSFER_CHAIN_HOPS: u32 = 8;

type BlobCache = client_runtime::blob_cache::ClientBlobCacheOwner;
type JoinAttempt = client_session::connection::JoinAttempt<SessionDirectoryGuard>;
pub(crate) type JoinStage = client_session::connection::JoinStage<SessionDirectoryGuard>;

use client_state::{JoinIntent, SessionStatus};

#[derive(Debug, Resource)]
pub(crate) struct SessionController {
    core: CoreProcessGuard, // declared first: the core stops before the directories below go
    /// Identity-checked owner of the live session's runtime directory.
    directory: Option<SessionDirectoryGuard>,
    /// The join provisioning behind the connecting screen.
    join: Option<JoinAttempt>,
    generation: u64,
    client_random_id: Option<protocol::ClientRandomId>,
    client_id_load: Option<
        std::sync::Mutex<std::sync::mpsc::Receiver<std::io::Result<protocol::ClientRandomId>>>,
    >,
    /// Where the current session plays, for Discord presence and invites.
    presence: Option<rich_presence::Target>,
    destination_identity: Option<String>,
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
            client_random_id: None,
            client_id_load: None,
            presence: None,
            destination_identity: None,
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
        self.destination_identity =
            address.map(|address| format!("remote:{}", address.trim().to_lowercase()));
        self
    }

    /// Stable selected destination, kept independent from invites and transient session ids.
    #[cfg(feature = "local-mods")]
    pub(crate) fn destination_identity(&self) -> Option<&str> {
        self.destination_identity.as_deref()
    }

    pub(crate) fn presence_target(&self) -> Option<&rich_presence::Target> {
        self.presence.as_ref()
    }

    /// Endpoint directory of the live session core; unavailable during teardown.
    pub(crate) fn socket_directory(&self) -> Option<&std::path::Path> {
        self.directory
            .as_ref()
            .map(SessionDirectoryGuard::directory)
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
        self.client_random_id = Some(protocol::ClientRandomId::new(-7).unwrap());
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

#[derive(SystemParam)]
pub(crate) struct SessionResources<'w> {
    controller: ResMut<'w, SessionController>,
    network: ResMut<'w, NetworkHandle<PackApplication>>,
    resource_packs: ResMut<'w, ResourcePackAdmissionState>,
    runtime: ResMut<'w, UiRuntimeResource>,
    player_runtime: ResMut<'w, PlayerRuntime>,
    client_world: ResMut<'w, ClientWorld>,
    movement: ResMut<'w, MovementTicker>,
    local_physics: ResMut<'w, LocalPhysicsController>,
    local_frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
    launcher: Option<Res<'w, LauncherCoreSlot>>,
    actor_artwork: Option<Res<'w, render_scene::ActorArtworkResource>>,
    ui_catalog: Option<Res<'w, crate::runtime::network::PackUiCatalog>>,
    input: Option<Res<'w, client_state::SemanticInputSnapshot>>,
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
        *self.network = NetworkHandle::<PackApplication>::disconnected();
        let controller = &mut *self.controller;
        let directory = controller.directory.take();
        let join = controller.join.take();
        controller
            .core
            .stop_detached(move || drop((join, directory)));
        controller.connecting = false;
        controller.presence = None;
        controller.destination_identity = None;
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
    session.controller.destination_identity = Some(format!(
        "{}:{}",
        if local_world { "local" } else { "remote" },
        if local_world {
            menu.local_world_identity().unwrap_or(&address).to_owned()
        } else {
            address.trim().to_lowercase()
        }
    ));
    let launcher = session.launcher.as_deref().and_then(|slot| {
        slot.begin_join(
            &address,
            local_world,
            auth_cache.is_some(),
            menu.is_launcher(),
        )
    });
    let controller = &mut *session.controller;
    if let Err(message) = controller.prepare_client_random_id(menu.layout()) {
        controller.fail_join(menu, message);
        return;
    }
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
    if controller.join.is_none() {
        return;
    }
    let client_random_id = match controller.poll_client_random_id() {
        Ok(Some(id)) => id,
        Ok(None) => return,
        Err(message) => {
            let join = controller.join.take();
            controller.core.stop_detached(move || drop(join));
            controller.fail_join(menu, message);
            return;
        }
    };
    let attempt = controller.join.take().expect("pending join");
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
                client_random_id,
                session.actor_artwork.as_deref().map(|artwork| &artwork.0),
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
    input: Option<&client_state::SemanticInputSnapshot>,
) -> protocol::LoginSettings {
    protocol::LoginSettings {
        language_code: pack_runtime::pack_language::active_language_code(),
        input_mode: input
            .and_then(|input| input.snapshot())
            .map_or(protocol::PlayerInputMode::Mouse, |snapshot| {
                gameplay::mining::protocol_input_mode(snapshot.input_mode)
            }),
        gui_scale_offset: menu.gui_scale_offset(),
        memory_tier: protocol::login_memory_tier(launcher_host::memory::physical_bytes()),
        graphics_mode: u8::from(menu.settings_snapshot().0.value("graphics_mode") != 0),
        filter_profanity: menu.settings_snapshot().0.value("filter_profanity") != 0,
        resource_pack_cache_dir: Some(menu.layout().resource_pack_cache_dir()),
    }
}

/// Starts the network session against the core serving `socket_dir`.
#[allow(
    clippy::too_many_arguments,
    reason = "joining forwards captured session inputs and optional presentation assets"
)]
fn start_network(
    commands: &mut Commands,
    menu: &MenuRuntime,
    session_generation: u64,
    cache: &BlobCache,
    socket_dir: PathBuf,
    login_settings: protocol::LoginSettings,
    client_random_id: protocol::ClientRandomId,
    actor_artwork: Option<&render_model::ActorArtworkPages>,
    ui_catalog: Option<&crate::runtime::network::PackUiCatalog>,
) -> Result<(), String> {
    let replacement = crate::runtime::network::session::spawn_network(NetworkConfig {
        session_generation,
        socket_dir,
        display_name: menu.display_name().to_owned(),
        client_random_id,
        client_blob_cache: cache.cache(),
        player_skin: menu.player_skin().clone(),
        login_settings,
        actor_artwork: actor_artwork.cloned(),
        ui_catalog: ui_catalog.map(|base| base.0.clone()),
    })
    .map_err(|error| error.to_string())?;
    commands.insert_resource(
        client_interaction::movement::MovementTicker::with_epoch_publisher(
            replacement.physics_epoch_publisher(),
        ),
    );
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

    #[test]
    fn login_profanity_preference_comes_from_saved_settings_with_a_hidden_menu() {
        use launcher::menu::settings_options::{SETTINGS_FILE, SETTINGS_OPTIONS, SettingsOptions};

        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "filter_profanity")
            .unwrap();
        for filter_profanity in [false, true] {
            let layout =
                launcher::test_support::scratch(&format!("login-profanity-{filter_profanity}"));
            let mut saved = SettingsOptions::default();
            saved.set(index, i32::from(filter_profanity));
            saved
                .save(&layout.user_config_root.join(SETTINGS_FILE))
                .unwrap();
            let menu = MenuRuntime::new_with_layout(
                false,
                None,
                "Direct".to_owned(),
                layout,
                client_shell::player_skin::LocalPlayerSkin::generated_default("Direct"),
            );
            let settings = login_settings(&menu, None);
            assert_eq!(settings.filter_profanity, filter_profanity);
        }
    }

    // A hidden-menu direct launch must still report the player's saved GUI scale.
    #[test]
    fn login_settings_carry_the_saved_gui_scale_without_a_visible_menu() {
        let layout = launcher::test_support::scratch("login-settings");
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
            client_shell::player_skin::LocalPlayerSkin::generated_default("Direct"),
        );
        let settings = login_settings(&menu, None);
        assert_eq!(settings.gui_scale_offset, -1);
        assert_eq!(settings.input_mode, protocol::PlayerInputMode::Mouse);
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
