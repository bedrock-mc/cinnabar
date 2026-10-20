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
use client_runtime::session::{format_transfer_address, transfer_handoff_address};
use launcher::menu::settings_options::SettingId;
use launcher_host::{
    core_process::{CoreProcessGuard, spawn_core_for_address},
    server_trust::SessionTrust,
    session_cleanup::SessionDirectoryGuard,
};

use client_shell::presence_target::presence_target;
use {
    crate::runtime::{
        network::{CompiledStacks, session::NetworkConfig},
        shutdown::record_fatal_error,
        world::TransferNotice,
    },
    client_shell::menu::{LauncherServices, MenuRuntime},
};

use std::time::Instant;

pub(crate) mod account_retirement;
mod client_id;
#[cfg(feature = "native-networking")]
pub(crate) mod destination;
#[cfg(feature = "native-networking")]
mod native_account;
#[cfg(feature = "native-networking")]
mod native_friend;
#[cfg(feature = "native-networking")]
mod native_gathering;
#[cfg(feature = "native-networking")]
mod native_local_world;
#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
mod native_transfer_tests;
#[cfg(feature = "native-networking")]
pub(crate) mod native_trust;
mod network;
use network::attempt_connect;
mod startup;
mod url_policy;

use client_session::MAX_TRANSFER_CHAIN_HOPS;

type BlobCache = client_runtime::blob_cache::ClientBlobCacheOwner;
type JoinAttempt = client_session::connection::JoinAttempt<SessionDirectoryGuard>;
pub(crate) type JoinStage = client_session::connection::JoinStage<SessionDirectoryGuard>;
pub(crate) type TrustOwner = Box<dyn launcher_host::server_trust::TrustSource + Send + Sync>;

use client_state::{JoinIntent, SessionStatus};

#[derive(Resource)]
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
    local_world: bool,
    /// Automatic transfer-follow hops remaining in the current chain.
    transfer_hops_remaining: u32,
    connecting: bool,
    account_retirement_started: bool,
    parallel_resource_pack_urls: bool,
    #[cfg(feature = "native-networking")]
    native_transport: Option<protocol::NativeTransport>,
    #[cfg(feature = "native-networking")]
    native_account: Option<native_account::SelectedAccount>,
    /// The original startup selection retained while packet mods are admitted.
    #[cfg(feature = "native-networking")]
    startup_account: Option<std::sync::Arc<launcher_host::native_account::NativeAccount>>,
    #[cfg(feature = "native-networking")]
    native_friend: Option<launcher_host::native_account::NativeFriendSelection>,
    #[cfg(feature = "native-networking")]
    native_friend_session: bool,
    #[cfg(feature = "native-networking")]
    native_gathering: Option<launcher_host::native_account::NativeGatheringSelection>,
    #[cfg(feature = "native-networking")]
    native_local_world:
        Option<crate::runtime::network::session::local_world::WorldConnectionRequest>,
    #[cfg(feature = "native-networking")]
    friend_cleanup: crate::runtime::network::session::friend::FriendCleanupOwner,
    /// Retains this join's original Core or native server trust question and answer lifetime.
    trust: Option<TrustOwner>,
    /// Server packs recent joins compiled; released once no join follows.
    kept_packs: &'static CompiledStacks,
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new(CoreProcessGuard::default())
    }
}

impl SessionController {
    /// Retains the direct backend for retries and server transfer follows.
    pub(crate) fn with_native_transport(
        self,
        transport: Option<protocol::NativeTransport>,
    ) -> Self {
        #[cfg(feature = "native-networking")]
        {
            let mut controller = self;
            controller.native_transport = transport;
            controller
        }
        #[cfg(not(feature = "native-networking"))]
        {
            let _ = transport;
            self
        }
    }

    /// Keeps explicit native requests, including local refusals, out of Core provisioning.
    fn native_provisioning_selected(&self, address: &str, local_world: bool) -> bool {
        #[cfg(feature = "native-networking")]
        {
            self.native_transport.is_some_and(|transport| {
                !matches!(
                    destination::classify(address, transport, local_world),
                    destination::Destination::LocalWorld
                )
            })
        }
        #[cfg(not(feature = "native-networking"))]
        {
            let _ = (address, local_world);
            false
        }
    }
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
            local_world: false,
            transfer_hops_remaining: MAX_TRANSFER_CHAIN_HOPS,
            connecting: false,
            account_retirement_started: false,
            parallel_resource_pack_urls: false,
            #[cfg(feature = "native-networking")]
            native_transport: None,
            #[cfg(feature = "native-networking")]
            native_account: None,
            #[cfg(feature = "native-networking")]
            startup_account: None,
            #[cfg(feature = "native-networking")]
            native_friend: None,
            #[cfg(feature = "native-networking")]
            native_friend_session: false,
            #[cfg(feature = "native-networking")]
            native_gathering: None,
            #[cfg(feature = "native-networking")]
            native_local_world: None,
            #[cfg(feature = "native-networking")]
            friend_cleanup: Default::default(),
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

    /// Retains a direct startup join's original trust source while the same network worker connects.
    pub(crate) fn with_initial_trust(mut self, trust: Option<TrustOwner>) -> Self {
        self.connecting |= trust.is_some();
        self.trust = trust;
        self
    }

    /// Whether the trusted launcher selected a local world for this connection.
    pub(crate) fn is_local_world(&self) -> bool {
        self.local_world
    }

    /// Stable selected destination, kept independent from invites and transient session ids.
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

    /// Returns a failed join to the menu, keeping only the newest compiled packs for a retry.
    fn fail_join(&mut self, menu: &mut MenuRuntime, message: String) {
        bridge::join_profile::JoinProfile::current().finish("failed");
        menu.show_join_failure(message);
        self.connecting = false;
        #[cfg(feature = "native-networking")]
        {
            self.startup_account = None;
            self.native_friend = None;
            self.native_friend_session = false;
            self.native_gathering = None;
            self.native_local_world = None;
        }
        self.kept_packs.keep_for_rejoin();
    }

    /// Provisions one endpoint, applying any native override only to this attempt.
    fn start_core(
        &mut self,
        menu: &MenuRuntime,
        cache: &BlobCache,
        address: &str,
        auth_cache: Option<&std::path::Path>,
        generation: u64,
        native_transport_override: Option<protocol::NativeTransport>,
    ) -> Result<JoinStage, String> {
        #[cfg(not(feature = "native-networking"))]
        let _ = native_transport_override;
        #[cfg(feature = "native-networking")]
        match self.native_transport.map(|configured| {
            destination::classify(
                address,
                native_transport_override.unwrap_or(configured),
                self.local_world,
            )
        }) {
            Some(destination::Destination::FriendSelectionRequired) => {
                return Err(crate::args::ArgsError::NativeFriendSelectionRequired.to_string());
            }
            Some(destination::Destination::LocalWorld) | None => {}
            Some(destination::Destination::Network(mut target)) => {
                let service = launcher_host::native_account::is_realm_target(address)
                    || protocol::NativeRawTarget::recognizes(address)
                    || matches!(
                        launcher_host::launcher_core::target_for(address),
                        bridge::ConnectTarget::Gathering(_)
                    );
                if service && auth_cache.is_none() {
                    return Err("Joining this service requires a verified native account.".into());
                }
                if auth_cache.is_some()
                    && !matches!(
                        target.transport,
                        protocol::NativeTransport::RakNet
                            | protocol::NativeTransport::Addressed
                            | protocol::NativeTransport::NetherNetLan
                    )
                {
                    return Err(
                        "The selected native transport does not support account preparation."
                            .into(),
                    );
                }
                if target.transport == protocol::NativeTransport::Addressed {
                    let (selected, trust) = native_trust::bind(target, menu.layout())
                        .map_err(|_| "Could not prepare native server trust.".to_owned())?;
                    target = selected;
                    self.trust = Some(Box::new(trust));
                }
                return Ok(JoinStage::Native(target));
            }
        }
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
        self.trust = Some(Box::new(SessionTrust::watch(socket_dir.clone())));
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

    /// Advances with every join, transfer and leave, so each session is told apart even on
    /// one address.
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
        self.local_world = local_world;
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

    /// Borrows the original process owner for cleanup fixtures.
    #[cfg(test)]
    pub(crate) fn core_mut(&mut self) -> &mut CoreProcessGuard {
        &mut self.core
    }
}

#[derive(SystemParam)]
pub(crate) struct SessionResources<'w> {
    controller: ResMut<'w, SessionController>,
    state: crate::game_session::SessionWorld<'w>,
    network: ResMut<'w, NetworkHandle<PackApplication>>,
    collisions: Res<'w, client_interaction::movement::PhysicsCollisionRegistries>,
    launcher: Option<Res<'w, LauncherServices>>,
    actor_artwork: Option<Res<'w, render_scene::ActorArtworkResource>>,
    ui_catalog: Option<Res<'w, crate::runtime::network::PackUiCatalog>>,
    input: Option<Res<'w, client_state::SemanticInputSnapshot>>,
    far_terrain: Option<ResMut<'w, crate::far_terrain::FarTerrain>>,
    packet_mods: Option<Res<'w, crate::modding::packets::PacketMods>>,
    mod_registration: Option<Res<'w, crate::modding::registration::Watcher>>,
}

impl SessionResources<'_> {
    /// Retires the live session with no join to follow, keeping only the newest
    /// compiled server packs for a rejoin from the menu.
    fn leave(&mut self) {
        self.retire();
        self.controller.kept_packs.keep_for_rejoin();
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
        controller.local_world = false;
        #[cfg(feature = "native-networking")]
        {
            controller.startup_account = None;
            controller.native_friend = None;
            controller.native_friend_session = false;
            controller.native_gathering = None;
            controller.native_local_world = None;
        }
        let generation = controller.next_generation();
        if let (Some(far), Some(stream)) = (
            self.far_terrain.as_mut(),
            self.state.client_world.stream.as_mut(),
        ) {
            far.retire_stream(stream);
        }
        self.state.retire(generation);
        generation
    }
}

/// Captures the player's settings as the join starts, including a hidden-menu launch.
pub(crate) fn login_settings(
    menu: &MenuRuntime,
    input: Option<&client_state::SemanticInputSnapshot>,
    parallel_resource_pack_urls: bool,
) -> protocol::LoginSettings {
    protocol::LoginSettings {
        server_address: String::new(),
        language_code: pack_runtime::pack_language::active_language_code(),
        input_mode: input
            .and_then(|input| input.snapshot())
            .map_or(protocol::PlayerInputMode::Mouse, |snapshot| {
                gameplay::mining::protocol_input_mode(snapshot.input_mode)
            }),
        gui_scale_offset: menu.gui_scale_offset(),
        memory_tier: protocol::login_memory_tier(launcher_host::memory::physical_bytes()),
        graphics_mode: u8::from(menu.settings_snapshot().0.value(SettingId::GraphicsMode) != 0),
        filter_profanity: menu.settings_snapshot().0.value(SettingId::FilterProfanity) != 0,
        view_distance_chunks: menu.settings_snapshot().0.value(SettingId::RenderDistance) as u8,
        resource_pack_cache_dir: Some(menu.layout().resource_pack_cache_dir()),
        parallel_resource_pack_urls,
    }
}

/// Acts on the menu's session intents and advances the pending join.
pub(crate) fn drive_session(
    mut commands: Commands,
    mut exits: MessageWriter<AppExit>,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<BlobCache>,
    mut session: SessionResources,
) {
    #[cfg(feature = "native-networking")]
    if menu.account_operation_pending()
        && (session.controller.native_account.is_some() || session.controller.native_friend_session)
    {
        session.leave();
        session.controller.native_account = None;
    }
    session.controller.publish(&mut menu);
    // A queued answer reaches its core before a decline below retires that core.
    if let Some(trust) = session.controller.trust.as_ref() {
        menu.sync_session_trust(trust.as_ref());
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
        Some(trust) => menu.sync_session_trust(trust.as_ref()),
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
        let Some(runtime_id) = session
            .state
            .runtime
            .local_runtime_id(&session.state.player_runtime)
        else {
            return false;
        };
        let generation = session.state.runtime.session_id();
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
        bridge::join_profile::JoinProfile::current().finish("cancelled");
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
    if menu.is_connecting() && session.state.client_world.stream.is_some() {
        session.controller.connecting = false;
        menu.show_world();
    }
    if let Some(intent) = menu.take_join_intent() {
        // A user-initiated join always starts a fresh transfer chain.
        session.controller.begin_fresh_transfer_chain();
        attempt_connect(menu, session, cache, intent, None);
    }
    #[cfg(feature = "native-networking")]
    if let Some(selection) = menu.take_native_friend_intent() {
        native_friend::begin(menu, session, selection);
    }
    #[cfg(feature = "native-networking")]
    if let Some(selection) = menu.take_native_gathering_intent() {
        native_gathering::begin(menu, session, selection);
    }
    #[cfg(feature = "native-networking")]
    if let Some(intent) = menu.take_native_local_world_intent() {
        native_local_world::begin(menu, session, intent);
    }
    network::poll_join(commands, menu, session, cache);
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
    let Some(error) = session.state.fatal_error.0.clone() else {
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
    let Some(notice) = session.state.client_world.transfer_notice.take() else {
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
            &mut session.state.fatal_error.0,
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
        Some(protocol::NativeTransport::Addressed),
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

    /// Native service provisioning and unverified friend refusals cannot select Core.
    #[cfg(feature = "native-networking")]
    #[test]
    fn native_routing_preserves_gathering_and_friend_provisioning_owners() {
        let controller = SessionController::default()
            .with_native_transport(Some(protocol::NativeTransport::RakNet));
        assert!(controller.native_provisioning_selected("play.example.net", false));
        assert!(controller.native_provisioning_selected(
            &format!("{}7", bridge::contract::REALM_TARGET_PREFIX),
            false
        ));
        assert!(!controller.native_provisioning_selected("play.example.net", true));
        assert!(controller.native_provisioning_selected(
            &format!("{}123", bridge::contract::FRIEND_TARGET_PREFIX),
            false
        ));
        let gathering = format!(
            "{}{}",
            launcher::menu::EXPERIENCE_ADDRESS_PREFIX,
            "00000000-0000-0000-0000-000000000007"
        );
        assert!(matches!(
            launcher_host::launcher_core::target_for(&gathering),
            bridge::ConnectTarget::Gathering(_)
        ));
        assert!(
            controller.native_provisioning_selected(&gathering, false),
            "gathering destinations must reach their native account allocation owner"
        );
    }

    /// The real native provisioning branch preserves the ordinary target owner's endpoint grammar.
    #[cfg(feature = "native-networking")]
    #[test]
    fn native_start_core_normalizes_saved_direct_host_and_ipv6_destinations() {
        let layout = launcher::test_support::scratch("native-direct-addresses");
        let menu = MenuRuntime::new_with_layout(
            false,
            None,
            "Direct".into(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Direct"),
        );
        let mut controller = SessionController::default()
            .with_native_transport(Some(protocol::NativeTransport::RakNet));
        let cache = BlobCache::default();
        for address in [
            "play.example.net:19133",
            "[2001:db8::7]:19134",
            "play.example.net",
            "2001:db8::7",
            " play.example.net ",
        ] {
            let bridge::ConnectTarget::RakNet(expected) =
                launcher_host::launcher_core::target_for(address)
            else {
                panic!("direct fixture must use the ordinary endpoint owner");
            };
            let JoinStage::Native(target) = controller
                .start_core(&menu, &cache, address, None, 1, None)
                .expect("direct native provisioning must not start a Go process")
            else {
                panic!("explicit native direct destination did not retain native I/O");
            };
            assert_eq!(target.transport, protocol::NativeTransport::RakNet);
            assert_eq!(
                target.address, expected,
                "native provisioning lost the existing endpoint grammar for {address}"
            );
            assert!(controller.directory.is_none());
        }
        let realm = format!("{}7", bridge::contract::REALM_TARGET_PREFIX);
        assert!(
            controller
                .start_core(&menu, &cache, &realm, None, 1, None)
                .is_err()
        );
        let JoinStage::Native(target) = controller
            .start_core(
                &menu,
                &cache,
                &realm,
                Some(std::path::Path::new("unread.json")),
                1,
                None,
            )
            .unwrap()
        else {
            panic!("Realm keeps native account resolution");
        };
        assert_eq!(target.address, realm, "Realm locator is not a DNS endpoint");

        let friend = format!("{}123", bridge::contract::FRIEND_TARGET_PREFIX);
        assert!(matches!(
            controller.start_core(&menu, &cache, &friend, None, 1, None),
            Err(message)
                if message == crate::args::ArgsError::NativeFriendSelectionRequired.to_string()
        ));
        assert!(controller.directory.is_none());
        assert!(controller.core.id().is_none());
    }

    #[test]
    fn login_profanity_preference_comes_from_saved_settings_with_a_hidden_menu() {
        use launcher::menu::settings_options::{SETTINGS_FILE, SETTINGS_OPTIONS, SettingsOptions};

        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.id == SettingId::FilterProfanity)
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
            let settings = login_settings(&menu, None, false);
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
        let settings = login_settings(&menu, None, false);
        assert_eq!(settings.gui_scale_offset, -1);
        assert_eq!(settings.input_mode, protocol::PlayerInputMode::Mouse);
    }

    #[test]
    fn login_settings_carry_the_saved_view_radius_without_a_visible_menu() {
        use launcher::menu::settings_options::{SETTINGS_FILE, SETTINGS_OPTIONS, SettingsOptions};

        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.id == SettingId::RenderDistance)
            .unwrap();
        for radius in [8, 35] {
            let layout = launcher::test_support::scratch(&format!("login-radius-{radius}"));
            let mut saved = SettingsOptions::default();
            saved.set(index, radius);
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
            assert_eq!(
                login_settings(&menu, None, false).view_distance_chunks,
                radius as u8
            );
        }
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
