//! [`AccountControl`] over the selected native services or a real Core control endpoint.
//! Workers poll auth/events often, catalogs and Home/public feeds rarely. Profile has an
//! independent worker woken by opening, retry and account changes, so it never
//! waits on unrelated feeds. Sign-out requests queue to the events worker.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use bridge::{
    self, Account, AuthState as CoreAuth, ConnectProgress, ConnectStage, FeaturedServer, Friend,
    Home, Message, MessageEvent, Profile, Realm, ServerPing,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use tokio::runtime::Handle;
use tokio_util::task::TaskTracker;

use super::account_control::{AccountControl, AccountEvent, RealmMembershipResponse};
use launcher::menu::view::{
    ButtonArt, InboxItem, JoinStage, LiveEventCard, MenuHome, MenuProfile, PingInfo,
    ServerTrustPrompt,
};
use launcher::menu::{
    CATALOG_REFRESH_INTERVAL,
    auth::AuthState,
    view::{MenuFriendCard, MenuRealmCard},
};

#[cfg(test)]
mod home_promo;

mod artwork;
mod cinnabar_players;
mod featured_feed;
mod feeds;
mod home_feed;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod server_ping;
mod shared_players;
mod workers;
#[cfg(test)]
use featured_feed::card as featured_card;
use feeds::{CoreFeeds, catalog_round};
mod events;
mod invites;
mod message_reports;
mod presence;
pub mod profile_worker;
mod realm_membership;
pub use presence::{TitlePresenceLifetime, TitlePresenceRetirement};
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native_host;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native_presence;
#[cfg(all(
    feature = "test-support",
    feature = "native-networking",
    not(target_arch = "wasm32")
))]
pub mod retirement_test_support;

#[cfg(all(test, unix))]
mod presence_polling_tests;
#[cfg(all(test, unix))]
mod profile_polling_tests;

/// How often auth state and events refresh.
const EVENT_INTERVAL: Duration = Duration::from_secs(1);
/// How often events refresh while a join is under way, so its progress bar moves smoothly.
const JOIN_EVENT_INTERVAL: Duration = Duration::from_millis(250);
/// How often the screen feeds are read; the core answers from its catalog cache
/// and refreshes upstream on its own schedule, so this only picks up fresh data.
const FEED_INTERVAL: Duration = Duration::from_secs(30);
/// How soon a feed that failed is asked again.
const FEED_RETRY: Duration = Duration::from_secs(15);
/// The longest one feed waits for its artwork downloads, as the core bounded a refresh.
pub const ARTWORK_BUDGET: Duration = Duration::from_secs(40);
/// How often shown server rows are pinged.
const PING_INTERVAL: Duration = Duration::from_secs(15);

/// The concrete account constructor owns authentication independently of session/presence routes.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AccountSource {
    #[default]
    Core,
    Native,
}

#[derive(Default)]
struct Snapshot {
    #[cfg(all(
        feature = "test-support",
        feature = "native-networking",
        not(target_arch = "wasm32")
    ))]
    retirement_probe: Option<Arc<retirement_test_support::State>>,
    account_source: AccountSource,
    auth_generation: u64,
    /// Wakes the catalog worker when its account identity changes.
    catalog_wake: Option<Sender<()>>,
    /// Wakes featured details when their count subscription changes.
    feed_wake: Option<Sender<()>>,
    /// Wakes Home independently when its account identity changes.
    home_wake: Option<Sender<()>>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    home_source: home_feed::Source,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    home_language: String,
    home_cancel: Option<tokio::sync::watch::Sender<home_feed::Wake>>,
    featured_cancel: Option<tokio::sync::watch::Sender<featured_feed::Wake>>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    featured_source: featured_feed::Source,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    featured_gatherings: Vec<Option<crate::native_account::NativeGatheringSelection>>,
    pending_home_scope: home_feed::Scope,
    delivered_home_scope: home_feed::Scope,
    home_reports: Option<Sender<home_feed::Report>>,
    /// Wakes Profile independently when its account identity changes.
    profile_wake: Option<tokio::sync::watch::Sender<profile_worker::Wake>>,
    /// Selects one presentation source for this account's existing profile worker.
    profile_source: profile_worker::Source,
    /// The invite picker uses the same selected account as native profile/presence.
    people_source: invites::Source,
    people_wake: Option<tokio::sync::watch::Sender<invites::Wake>>,
    realm_source: realm_membership::Source,
    realm_wake: Option<tokio::sync::watch::Sender<realm_membership::Wake>>,
    xbox_presence: bridge::XboxPresenceState,
    title_presence_owner: crate::launcher_core::TitlePresenceOwner,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_presence_account: Option<Arc<crate::native_account::NativeAccount>>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_host_wake: Option<tokio::sync::watch::Sender<native_host::Desired>>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_host_invitation: Option<crate::native_account::HostedInvitation>,
    /// The Cinnabar API identity and visibility the events worker keeps the core configured with.
    cinnabar_api: Option<bridge::CinnabarApiConfig>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_product_api: Option<crate::product_api::Client>,
    shared_players: shared_players::Exchange,
    /// Roster identifiers the API reports as Cinnabar players; replaced whole on change.
    cinnabar_players: Arc<std::collections::HashSet<String>>,
    /// The `(session, revision)` roster last sent to the lookup worker.
    cinnabar_roster_key: Option<(u64, u64)>,
    account: Option<Account>,
    realms: Option<Vec<Realm>>,
    /// Prevents a catalog request started before acceptance from removing the new membership.
    realm_catalog_revision: u64,
    /// Native mode owns Realms and friend-world reads; other account services remain enabled.
    native_catalog: bool,
    friends: Option<Vec<Friend>>,
    /// Delivered once per fetch.
    featured: Option<Vec<FeaturedServer>>,
    player_counts_visible: bool,
    profile: Option<Result<Profile, ()>>,
    ping_targets: Vec<String>,
    pings: Option<Vec<ServerPing>>,
    home: Option<Home>,
    events: Vec<AccountEvent>,
    last_disconnect: Option<u64>,
    connect: Option<ConnectProgress>,
    server_trust: Option<bridge::ServerTrustPrompt>,
    /// The prompt last answered, hidden until the core withdraws it.
    answered_trust: Option<u64>,
    /// The menu is connecting, so the events worker polls faster.
    joining: bool,
    /// The invite screen's friends list, delivered once per request.
    people: Option<Result<Vec<bridge::Person>, ()>>,
    realm_membership: Option<RealmMembershipResponse>,
    /// The current explicit UI intent; cancellation revokes publication before waking its worker.
    realm_ticket: Option<u64>,
}

impl Snapshot {
    /// An asynchronous account reply belongs only to the exact source and generation that began it.
    fn same_account_source(&self, generation: u64, source: AccountSource) -> bool {
        self.auth_generation == generation && self.account_source == source
    }

    /// The concrete native account-service source shared by its original constructor and owner.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    fn native(account: Option<Arc<crate::native_account::NativeAccount>>, language: &str) -> Self {
        Snapshot {
            account_source: AccountSource::Native,
            title_presence_owner: crate::launcher_core::TitlePresenceOwner::Native,
            profile_source: profile_worker::Source::Native(account.clone()),
            people_source: invites::Source::Native(account.clone()),
            realm_source: realm_membership::Source::Native(account.clone()),
            home_source: home_feed::Source::Native(account.clone()),
            featured_source: featured_feed::Source::Native(account.clone()),
            native_product_api: Some(crate::product_api::Client::default()),
            home_language: if language.is_empty() {
                bridge::contract::DEFAULT_SERVICE_LANGUAGE.to_owned()
            } else {
                language.replace('_', "-")
            },
            native_presence_account: account,
            ..Default::default()
        }
    }

    /// Retires values tied to the old account before accepting responses for another identity.
    fn retire_account_data(&mut self) {
        self.shared_players = shared_players::Exchange::default();
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Some(api) = &self.native_product_api {
            api.retire_account();
            self.cinnabar_players = Arc::default();
            self.cinnabar_roster_key = None;
        }
        self.auth_generation = self.auth_generation.wrapping_add(1);
        self.realms = None;
        self.friends = None;
        self.featured = None;
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        self.featured_gatherings.clear();
        if let Some(wake) = &self.featured_cancel {
            featured_feed::retire(wake);
        }
        self.profile = None;
        self.people = None;
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            self.native_host_invitation = None;
            if let Some(wake) = &self.native_host_wake {
                native_host::retire(wake);
            }
        }
        self.realm_membership = None;
        self.realm_ticket = None;
        self.home = None;
        self.pending_home_scope = home_feed::Scope::default();
        self.delivered_home_scope = home_feed::Scope::default();
        if let Some(wake) = &self.home_cancel {
            home_feed::retire(wake);
        }
        if let Some(wake) = &self.people_wake {
            invites::retire(wake);
        }
        if let Some(wake) = &self.realm_wake {
            realm_membership::retire(wake);
        }
        if let Some(wake) = &self.catalog_wake {
            // A queued wake already covers the newest snapshot; never block a frame.
            let _ = wake.try_send(());
        }
        if let Some(wake) = &self.profile_wake {
            profile_worker::refresh(wake);
        }
        if let Some(wake) = &self.feed_wake {
            let _ = wake.try_send(());
        }
        if let Some(wake) = &self.home_wake {
            let _ = wake.try_send(());
        }
    }

    /// Advances the identity boundary when the core changes account or sign-in state.
    fn set_account(&mut self, account: Account) {
        if self
            .account
            .as_ref()
            .is_none_or(|old| old.state != account.state || old.gamertag != account.gamertag)
        {
            self.retire_account_data();
        }
        self.account = Some(account);
    }
}

/// The menu's selected account services and their optional real legacy control transport.
pub struct LauncherAccount {
    snapshot: Arc<Mutex<Snapshot>>,
    sign_out: Sender<()>,
    profile_refresh: tokio::sync::watch::Sender<profile_worker::Wake>,
    message_reports: Sender<home_feed::Report>,
    invites: invites::Requests,
    realm_membership: realm_membership::Requests,
    cinnabar_rosters: Sender<cinnabar_players::Roster>,
    /// Dropping it stops the catalog and feed workers.
    _alive: Sender<()>,
    socket_dir: Option<PathBuf>,
    presence_lifetime: TitlePresenceLifetime,
}

impl Drop for LauncherAccount {
    fn drop(&mut self) {
        profile_worker::stop(&self.profile_refresh);
        self.with(|snapshot| {
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            if let Some(api) = &snapshot.native_product_api {
                api.stop();
            }
            if let Some(wake) = &snapshot.home_cancel {
                wake.send_replace(home_feed::Wake::Stop);
            }
            if let Some(wake) = &snapshot.featured_cancel {
                wake.send_replace(featured_feed::Wake::Stop);
            }
            snapshot.home_reports = None;
        });
        self.presence_lifetime.retire();
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        self.with(|snapshot| {
            snapshot.native_host_invitation = None;
            if let Some(wake) = &snapshot.native_host_wake {
                native_host::stop(wake);
            }
        });
    }
}

impl LauncherAccount {
    /// Start polling the control endpoint under `socket_dir`, caching artwork under `artwork`;
    /// the workers stop when this is dropped. Events, the slow catalog and the screen feeds
    /// each poll on their own worker, publishing every answer as it arrives.
    pub fn new(socket_dir: PathBuf, artwork: PathBuf, runtime: Handle, tasks: TaskTracker) -> Self {
        Self::start(
            Some(socket_dir),
            artwork,
            Snapshot::default(),
            runtime,
            tasks,
        )
    }

    /// Starts native services without a Core endpoint on the application's existing reactor.
    /// Account operations share the menu's original selected owner; absence never selects Core.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub fn native(
        artwork: PathBuf,
        account: Option<Arc<crate::native_account::NativeAccount>>,
        language: &str,
        runtime: Handle,
        tasks: TaskTracker,
    ) -> Self {
        Self::start(
            None,
            artwork,
            Snapshot::native(account, language),
            runtime,
            tasks,
        )
    }

    /// Retains the existing worker's retirement proof independently of the Bevy resource.
    pub fn title_presence_lifetime(&self) -> TitlePresenceLifetime {
        self.presence_lifetime.clone()
    }

    /// Retires Go Realms/friend-world results and polling when the native menu owns discovery.
    pub fn set_native_catalog(&self, native: bool) {
        self.with(|snapshot| {
            if snapshot.native_catalog != native {
                snapshot.native_catalog = native;
                snapshot.realm_catalog_revision = snapshot.realm_catalog_revision.wrapping_add(1);
                snapshot.realms = None;
                snapshot.friends = None;
                if let Some(wake) = &snapshot.catalog_wake {
                    let _ = wake.try_send(());
                }
            }
        });
    }

    /// Supplies only the actual played opening; the existing native worker owns all service IO.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub fn set_native_host_intent(&self, intent: Option<crate::local_worlds::NativeHostIntent>) {
        self.with(|snapshot| {
            if let Some(wake) = &snapshot.native_host_wake {
                native_host::set(wake, intent);
            }
        });
    }

    /// Publishes the newest committed world activity for the control worker.
    pub fn set_xbox_presence(&self, state: bridge::XboxPresenceState) {
        let changed = self.with(|snapshot| {
            if snapshot.xbox_presence == state {
                return false;
            }
            snapshot.xbox_presence = state;
            true
        });
        if changed {
            self.presence_lifetime.wake();
        }
    }

    /// Keeps the core's Cinnabar API connection on `channel`, showing this player when `visible`.
    pub fn set_cinnabar_api(&self, channel: update_manifest::channel::Channel, visible: bool) {
        self.with(|snapshot| {
            if snapshot
                .cinnabar_api
                .as_ref()
                .is_some_and(|config| config.channel == channel.name() && config.visible == visible)
            {
                return;
            }
            snapshot.cinnabar_api = Some(bridge::CinnabarApiConfig {
                client_version: env!("CARGO_PKG_VERSION").to_owned(),
                client_build: update_manifest::build_number(),
                channel: channel.name().to_owned(),
                visible,
            });
        });
    }

    /// Asks which of the server roster's identities are Cinnabar players when `(session, revision)` is new;
    /// answers are cached per session.
    pub fn set_cinnabar_roster(
        &self,
        session: u64,
        revision: u64,
        identities: impl IntoIterator<Item = String>,
    ) {
        let key = Some((session, revision));
        let changed = self.with(|snapshot| {
            let previous = std::mem::replace(&mut snapshot.cinnabar_roster_key, key);
            if previous.map(|key| key.0) != Some(session) {
                snapshot.cinnabar_players = Arc::default();
            }
            previous != key
        });
        if changed {
            let identities: Vec<String> = identities.into_iter().collect();
            tracing::debug!(players = identities.len(), "Cinnabar roster submitted");
            if self
                .cinnabar_rosters
                .send(cinnabar_players::Roster {
                    session,
                    identities,
                })
                .is_err()
            {
                tracing::debug!("Cinnabar roster worker stopped");
            }
        }
    }

    /// Roster identifiers the API reports as Cinnabar players; the `Arc` changes only with the set.
    pub fn cinnabar_players(&self) -> Arc<std::collections::HashSet<String>> {
        self.with(|snapshot| Arc::clone(&snapshot.cinnabar_players))
    }

    /// The actual legacy control directory; native services have no Core transport.
    pub fn core_directory(&self) -> Option<&std::path::Path> {
        self.socket_dir.as_deref()
    }

    fn with<T>(&self, read: impl FnOnce(&mut Snapshot) -> T) -> T {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        read(&mut snapshot)
    }
}

/// Supplies the original direct async fixtures without creating production runtimes.
#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
}

/// Owns a live reactor for existing OS-worker fixtures that borrow its handle.
#[cfg(test)]
fn worker_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("fixture I/O runtime")
}

/// Completes the actual counted fixture workers before releasing their reactor.
#[cfg(all(test, unix))]
fn finish_workers(runtime: tokio::runtime::Runtime, tasks: TaskTracker) {
    tasks.close();
    runtime.block_on(tasks.wait());
    runtime.shutdown_background();
}

fn publish(shared: &Mutex<Snapshot>, write: impl FnOnce(&mut Snapshot)) {
    write(&mut shared.lock().unwrap_or_else(|poison| poison.into_inner()));
}

/// Captures the identity boundary before an account-dependent request starts.
fn auth_generation(shared: &Mutex<Snapshot>) -> u64 {
    shared
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .auth_generation
}

/// Publishes a worker response under the identity that requested it.
fn publish_account(shared: &Mutex<Snapshot>, generation: u64, write: impl FnOnce(&mut Snapshot)) {
    let mut snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
    if snapshot.auth_generation == generation {
        write(&mut snapshot);
    }
}

/// Waits `interval`; `false` once the link is gone.
fn wait(stop: &Receiver<()>, interval: Duration) -> bool {
    !matches!(
        stop.recv_timeout(interval),
        Err(crossbeam_channel::RecvTimeoutError::Disconnected)
    )
}

/// Waits for the next catalog poll, an identity change, or the link being dropped.
fn wait_catalog(stop: &Receiver<()>, changes: &Receiver<()>) -> bool {
    crossbeam_channel::select! {
        recv(stop) -> _ => false,
        recv(changes) -> result => result.is_ok(),
        default(CATALOG_REFRESH_INTERVAL) => true,
    }
}

#[cfg(test)]
fn poll_events(socket_dir: &std::path::Path, shared: &Mutex<Snapshot>, requests: &Receiver<()>) {
    let runtime = worker_runtime();
    events::poll(
        Some(socket_dir),
        shared,
        requests,
        &TitlePresenceLifetime::new(),
        runtime.handle(),
    );
}

/// The announced release when its identity differs from the last one seen.
fn new_release<'a>(
    seen: &mut Option<bridge::CinnabarRelease>,
    latest: Option<&'a bridge::CinnabarRelease>,
) -> Option<&'a bridge::CinnabarRelease> {
    let latest = latest?;
    if seen.as_ref() == Some(latest) {
        return None;
    }
    *seen = Some(latest.clone());
    Some(latest)
}

fn poll_catalog(
    socket_dir: Option<&std::path::Path>,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    changes: &Receiver<()>,
    runtime: &Handle,
) {
    loop {
        // Adopt all changes already present before starting this account's requests.
        while changes.try_recv().is_ok() {}
        let generation = {
            let snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
            snapshot
                .account
                .as_ref()
                .filter(|account| account.state == CoreAuth::SignedIn)
                .map(|_| snapshot.auth_generation)
        };
        if let Some(generation) = generation {
            let feeds = CoreFeeds {
                socket_dir,
                art: None,
            };
            runtime.block_on(catalog_round(&feeds, shared, generation));
        }
        if !wait_catalog(stop, changes) {
            return;
        }
    }
}

/// One result per pinged address: a server that sent no pong reads offline, as
/// vanilla's red offline icon shows it, never as still loading.
fn round_results(targets: &[String], pongs: Vec<ServerPing>) -> Vec<ServerPing> {
    let pongs: std::collections::HashMap<String, ServerPing> = pongs
        .into_iter()
        .map(|pong| (pong.address.clone(), pong))
        .collect();
    targets
        .iter()
        .map(|address| {
            pongs.get(address).cloned().unwrap_or_else(|| ServerPing {
                address: address.clone(),
                ..ServerPing::default()
            })
        })
        .collect()
}

/// A feed's value, or `None` after logging which feed failed; the core logs the
/// upstream cause, redacted.
fn settle<T, E: std::fmt::Display>(
    feed: &str,
    result: Result<T, E>,
    failed: &mut bool,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            *failed = true;
            tracing::warn!(feed, %error, "launcher feed failed; retrying soon");
            None
        }
    }
}

/// Button-art surfaces the start screen shows, reported once per message instance.
const SHOWN_SURFACES: [&str; 1] = [bridge::contract::MESSAGE_SURFACE_PLAY_BUTTON];

fn report_impressions(
    runtime: &Handle,
    socket_dir: Option<&std::path::Path>,
    home: &Home,
    reported: &mut HashSet<String>,
) {
    let Some(socket_dir) = socket_dir else { return };
    for message in &home.messages {
        if !SHOWN_SURFACES.contains(&message.surface.as_str())
            || !reported.insert(message.instance_id.clone())
        {
            continue;
        }
        let event = MessageEvent {
            event_type: bridge::contract::MESSAGE_EVENT_IMPRESSION.to_owned(),
            instance_id: message.instance_id.clone(),
            report_id: message.report_id.clone(),
            button_id: String::new(),
        };
        let _ = runtime.block_on(bridge::report_message_event(socket_dir, &event));
    }
}

/// The start screen's view of the core's home feed.
pub fn menu_home(home: &Home, now_unix: i64) -> MenuHome {
    let art = |surface: &str| {
        home.messages
            .iter()
            .find(|message| message.surface == surface)
            .map(button_art)
    };
    let live_event = home
        .live_events
        .iter()
        .find(|event| event.end_unix == 0 || now_unix < event.end_unix)
        .map(|event| LiveEventCard {
            button_text: if event.button_text.is_empty() {
                "gathering.button.liveEventFallback".to_owned()
            } else {
                event.button_text.clone()
            },
            caption: event.caption_text.clone(),
            countdown: event.caption_countdown,
            start_unix: event.start_unix,
            badge_path: event.badge.path.clone(),
            address: event.address.clone(),
            route_to_servers: event.route_to_servers,
        });
    MenuHome {
        message_session: None,
        play_art: art(bridge::contract::MESSAGE_SURFACE_PLAY_BUTTON),
        inbox_unread: home
            .inbox
            .categories
            .iter()
            .filter(|category| launcher::menu::inbox::category_index(&category.kind).is_some())
            .fold(0_u32, |total, category| {
                total.saturating_add(category.unread)
            }),
        inbox_counts: home
            .inbox
            .categories
            .iter()
            .filter_map(|category| {
                Some((
                    launcher::menu::inbox::category_index(&category.kind)?,
                    category.unread,
                ))
            })
            .collect(),
        realm_invites: home.realm_invites,
        live_event,
        persona_head: home.persona_head.path.clone(),
        inbox: home
            .messages
            .iter()
            .filter(|message| {
                message.surface == bridge::contract::MESSAGE_SURFACE_INBOX_MESSAGE
                    && launcher::menu::inbox::category_index(&message.category).is_some()
            })
            .map(|message| InboxItem {
                instance_id: message.instance_id.clone(),
                report_id: message.report_id.clone(),
                received: message.received.clone(),
                source: message.sender.clone(),
                header: message.header.clone(),
                body: message.body.clone(),
                category: message.category.clone(),
                unread: !message
                    .status
                    .eq_ignore_ascii_case(bridge::contract::MESSAGE_STATUS_READ),
            })
            .collect(),
    }
}

/// Sorts a tile's images into the button's layers by their ids (hover, foreground).
fn button_art(message: &Message) -> ButtonArt {
    let mut art = ButtonArt {
        banner: message.banner.clone(),
        colors: message.colors.clone(),
        ..ButtonArt::default()
    };
    for image in message.images.iter().filter(|image| !image.path.is_empty()) {
        let id = image.id.to_ascii_lowercase();
        if id.contains(bridge::contract::MESSAGE_ART_BANNER) {
            art.banner_texture = image.path.clone();
            continue;
        }
        let hover = id.contains(bridge::contract::MESSAGE_ART_HOVER);
        let foreground = id.contains(bridge::contract::MESSAGE_ART_FORE)
            || id.contains(bridge::contract::MESSAGE_ART_FG);
        let slot = match (hover, foreground) {
            (true, true) => &mut art.hover_foreground,
            (true, false) => &mut art.hover_background,
            (false, true) => &mut art.default_foreground,
            (false, false) => &mut art.default_background,
        };
        if slot.is_empty() {
            *slot = image.path.clone();
        }
    }
    art
}

/// The core's account state as the menu's sign-in state; an offline core
/// knows nothing about the account, so the auth supervisor's state stands.
fn auth_state(account: &Account) -> Option<AuthState> {
    Some(match account.state {
        CoreAuth::Offline => return None,
        CoreAuth::SignedOut => AuthState::SignedOut,
        CoreAuth::AwaitingCode => AuthState::AwaitingCode {
            uri: account.verification_uri.clone().unwrap_or_default(),
            code: account.user_code.clone().unwrap_or_default(),
        },
        CoreAuth::SignedIn => AuthState::Authenticated,
        CoreAuth::Failed => AuthState::Failed(account.reason.clone().unwrap_or_default()),
    })
}

fn join_stage(progress: &ConnectProgress) -> JoinStage {
    match progress.stage {
        ConnectStage::Realm => JoinStage::Realm,
        ConnectStage::Idle | ConnectStage::Connecting => JoinStage::Connecting,
        ConnectStage::Packs => JoinStage::Packs {
            done: progress.packs_done,
            total: progress.packs_total,
            received_bytes: progress.received_bytes,
            total_bytes: progress.total_bytes,
        },
    }
}

fn friend_card(friend: &Friend) -> MenuFriendCard {
    let members = if friend.max_members > 0 {
        format!("{}/{} players", friend.members, friend.max_members)
    } else {
        format!("{} players", friend.members)
    };
    MenuFriendCard {
        gamertag: friend.gamertag.clone(),
        world_name: friend.world_name.clone(),
        members,
        xuid: friend.xuid.clone(),
        max_members: friend.max_members,
    }
}

impl AccountControl for LauncherAccount {
    fn set_player_counts_visible(&mut self, visible: bool) {
        self.with(|snapshot| {
            if snapshot.player_counts_visible != visible {
                snapshot.player_counts_visible = visible;
                if let Some(wake) = &snapshot.feed_wake {
                    let _ = wake.try_send(());
                }
            }
        });
    }

    fn account_status(&mut self) -> Option<AuthState> {
        self.with(|snapshot| snapshot.account.as_ref().and_then(auth_state))
    }

    fn join_stage(&mut self) -> Option<JoinStage> {
        self.with(|snapshot| snapshot.connect.as_ref().map(join_stage))
    }

    fn set_joining(&mut self, joining: bool) {
        self.with(|snapshot| snapshot.joining = joining);
    }

    fn server_trust(&mut self) -> Option<ServerTrustPrompt> {
        self.with(|snapshot| {
            let prompt = snapshot.server_trust.as_ref()?;
            (snapshot.answered_trust != Some(prompt.id)).then(|| ServerTrustPrompt {
                id: prompt.id,
                url: prompt.url.clone(),
                from_session_core: false,
            })
        })
    }

    fn answer_server_trust(&mut self, id: u64, trusted: bool) {
        let Some(directory) = self.socket_dir.clone() else {
            return;
        };
        self.with(|snapshot| snapshot.answered_trust = Some(id));
        let snapshot = Arc::clone(&self.snapshot);
        super::server_trust::send(directory, id, trusted, move || {
            let mut snapshot = snapshot.lock().unwrap_or_else(|poison| poison.into_inner());
            if snapshot.answered_trust == Some(id) {
                snapshot.answered_trust = None;
            }
        });
    }

    fn account_generation(&mut self) -> Option<u64> {
        Some(self.with(|snapshot| snapshot.auth_generation))
    }

    fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
        self.with(|snapshot| {
            snapshot
                .realms
                .as_ref()
                .map(|realms| realms.iter().map(realm_membership::realm_card).collect())
        })
    }

    fn friends(&mut self) -> Option<Vec<MenuFriendCard>> {
        self.with(|snapshot| {
            snapshot
                .friends
                .as_ref()
                .map(|friends| friends.iter().map(friend_card).collect())
        })
    }

    fn sign_out(&mut self) -> bool {
        let queued = self.sign_out.try_send(()).is_ok();
        if queued {
            // The signed-in lists and status are stale from here on.
            self.with(|snapshot| {
                snapshot.retire_account_data();
                snapshot.account = None;
            });
        }
        queued
    }

    fn poll_event(&mut self) -> Option<AccountEvent> {
        self.with(|snapshot| (!snapshot.events.is_empty()).then(|| snapshot.events.remove(0)))
    }

    fn featured(&mut self) -> Option<Vec<crate::account_control::FeaturedRow>> {
        self.with(featured_feed::take_rows)
    }

    /// Queues an inbox action on the dedicated reporting worker.
    fn report_message(&mut self, event: MessageEvent) {
        let report = self.with(|snapshot| home_feed::Report::capture(snapshot, event));
        let _ = self.message_reports.send(report);
    }

    fn home(&mut self) -> Option<MenuHome> {
        let (home, scope) = self.with(|snapshot| {
            let home = snapshot.home.take()?;
            snapshot.delivered_home_scope = snapshot.pending_home_scope.clone();
            Some((home, snapshot.delivered_home_scope.clone()))
        })?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let mut view = menu_home(&home, now);
        view.message_session = scope.marker();
        Some(view)
    }

    fn set_ping_targets(&mut self, targets: Vec<String>) {
        self.with(|snapshot| {
            if snapshot.ping_targets != targets {
                snapshot.ping_targets = targets;
            }
        });
    }

    fn pings(&mut self) -> Option<Vec<(String, PingInfo)>> {
        let pings = self.with(|snapshot| snapshot.pings.take())?;
        Some(
            pings
                .into_iter()
                .map(|ping| {
                    let info = PingInfo {
                        motd: ping.motd,
                        online: ping.online,
                        players: ping.players,
                        max_players: ping.max_players,
                        ping_ms: ping.ping_ms,
                    };
                    (ping.address, info)
                })
                .collect(),
        )
    }

    /// Wakes only Profile when opened or retried, without replaying Home impressions.
    fn refresh_profile(&mut self) {
        if !profile_worker::refresh(&self.profile_refresh) {
            self.with(|snapshot| snapshot.profile = Some(Err(())));
            profile_worker::log_unavailable("worker_unavailable");
        }
    }

    fn profile(&mut self) -> Option<MenuProfile> {
        let profile = self.with(|snapshot| snapshot.profile.take())?;
        let Ok(profile) = profile else {
            return Some(MenuProfile::unavailable());
        };
        Some(MenuProfile {
            loaded: true,
            unavailable: false,
            xuid: profile.xuid,
            statistics_loaded: true,
            statistics_error: profile.statistics.is_none(),
            achievements_loaded: true,
            achievements_error: profile.achievements.is_none(),
            achievements: profile.achievements,
            gamertag: profile.gamertag,
            picture_path: profile.gamerpic.path,
            avatar_path: profile.avatar.path,
            avatar_loaded: true,
            avatar_error: profile.avatar_error,
            featured_screenshot_path: profile.featured_screenshot.path,
            featured_screenshot_loaded: true,
            featured_screenshot_error: profile.featured_screenshot_error,
            real_name: profile.real_name,
            presence: profile.presence_text,
            gamerscore: profile.gamerscore,
            friends: profile.friends,
            followers: profile.followers,
            statistics: profile.statistics,
        })
    }

    fn request_realm_membership(&mut self, ticket: u64, code: String, accept: bool) -> bool {
        self.request_realm_membership_control(ticket, code, accept)
    }
    fn cancel_realm_membership(&mut self) {
        self.cancel_realm_membership_control();
    }
    fn realm_membership(&mut self) -> Option<RealmMembershipResponse> {
        self.realm_membership_control()
    }

    fn request_people(&mut self) {
        self.invites.people(&self.snapshot);
    }

    fn people(&mut self) -> Option<Result<Vec<launcher::menu::invite::Friend>, ()>> {
        let people = self.with(|snapshot| snapshot.people.take())?;
        Some(people.map(|people| people.into_iter().map(Into::into).collect()))
    }

    fn send_invites(&mut self, xuids: Vec<String>) {
        self.invites.send(&self.snapshot, xuids);
    }
}

#[cfg(test)]
mod tests;
