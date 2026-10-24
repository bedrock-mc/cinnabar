//! [`AccountControl`] over the selected native account and its original service lifetimes.
//! Workers poll auth/events often, catalogs and Home/public feeds rarely. Profile has an
//! independent worker woken by opening, retry and account changes, so it never
//! waits on unrelated feeds. Sign-out requests queue to the events worker.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use bridge::{
    self, Account, AuthState as AccountAuth, FeaturedServer, Home, MessageEvent, Profile,
    ServerPing,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use tokio::runtime::Handle;
use tokio_util::task::TaskTracker;

use super::account_control::{AccountControl, RealmMembershipResponse, menu_home};
use launcher::menu::auth::AuthState;
use launcher::menu::view::{MenuHome, MenuProfile, PingInfo};

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
pub use workers::AccountWorkerLifetime;
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

/// How often auth state and events refresh.
const EVENT_INTERVAL: Duration = Duration::from_secs(1);
/// How often events refresh while a join is under way, so its progress bar moves smoothly.
const JOIN_EVENT_INTERVAL: Duration = Duration::from_millis(250);
/// How often screen feeds adopt freshly acquired service data.
const FEED_INTERVAL: Duration = Duration::from_secs(30);
/// How soon a feed that failed is asked again.
const FEED_RETRY: Duration = Duration::from_secs(15);
/// How often shown server rows are pinged.
const PING_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Snapshot {
    #[cfg(all(
        test,
        feature = "test-support",
        feature = "native-networking",
        not(target_arch = "wasm32")
    ))]
    native_host_selected: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
    pending_presence_probe: Option<Arc<native_presence::tests::final_stop::PendingPresence>>,
    #[cfg(all(
        feature = "test-support",
        feature = "native-networking",
        not(target_arch = "wasm32")
    ))]
    retirement_probe: Option<Arc<retirement_test_support::State>>,
    auth_generation: u64,
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
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_presence_account: Option<Arc<crate::native_account::NativeAccount>>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_host_wake: Option<tokio::sync::watch::Sender<native_host::Desired>>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_host_retirement: native_host::Retirement,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_host_invitation: Option<crate::native_account::HostedInvitation>,
    /// The Cinnabar API identity and visibility applied by the original event worker.
    cinnabar_api: Option<bridge::CinnabarApiConfig>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_product_api: Option<crate::product_api::Client>,
    shared_players: shared_players::Exchange,
    /// Roster identifiers the API reports as Cinnabar players; replaced whole on change.
    cinnabar_players: Arc<std::collections::HashSet<String>>,
    /// The `(session, revision)` roster last sent to the lookup worker.
    cinnabar_roster_key: Option<(u64, u64)>,
    account: Option<Account>,
    /// Delivered once per fetch.
    featured: Option<Vec<FeaturedServer>>,
    player_counts_visible: bool,
    profile: Option<Result<Profile, ()>>,
    ping_targets: Vec<String>,
    pings: Option<Vec<ServerPing>>,
    home: Option<Home>,
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
    fn same_generation(&self, generation: u64) -> bool {
        self.auth_generation == generation
    }

    /// The concrete native account-service source shared by its original constructor and owner.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    fn native(account: Option<Arc<crate::native_account::NativeAccount>>, language: &str) -> Self {
        Snapshot {
            profile_source: profile_worker::Source(account.clone()),
            people_source: invites::Source(account.clone()),
            realm_source: realm_membership::Source(account.clone()),
            home_source: home_feed::Source(account.clone()),
            featured_source: featured_feed::Source(account.clone()),
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

    /// Advances the identity boundary when the selected account or sign-in state changes.
    #[cfg(test)]
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

/// The menu's selected account services and their original cancellation owners.
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
    presence_lifetime: TitlePresenceLifetime,
    workers: TaskTracker,
}

impl Drop for LauncherAccount {
    fn drop(&mut self) {
        tracing::info!(shutdown_phase = "account_drop_body_begin");
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
        tracing::info!(shutdown_phase = "account_drop_body_done");
    }
}

impl LauncherAccount {
    /// Starts native services on the application's existing reactor.
    /// Account operations share the menu's original selected owner; absence remains unavailable.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub fn native(
        artwork: PathBuf,
        account: Option<Arc<crate::native_account::NativeAccount>>,
        language: &str,
        execution: bridge::tasks::NativeExecution,
    ) -> Self {
        let runtime = execution.runtime().clone();
        let tasks = execution.tasks().clone();
        Self::start(
            artwork,
            Snapshot::native(account, language),
            runtime,
            tasks,
            async move { execution.stopped().await },
        )
    }

    /// Retains the existing worker's retirement proof independently of the Bevy resource.
    pub fn title_presence_lifetime(&self) -> TitlePresenceLifetime {
        self.presence_lifetime.clone()
    }

    /// Retains original worker completion and hosted membership cleanup after resource removal.
    pub fn worker_lifetime(&self) -> AccountWorkerLifetime {
        AccountWorkerLifetime {
            workers: self.workers.clone(),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            host: self.with(|snapshot| snapshot.native_host_retirement.clone()),
        }
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

    /// Configures the original Cinnabar API connection and explicit visibility.
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
#[cfg(test)]
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

/// Projects verified account presentation, leaving unavailable state with the auth supervisor.
fn auth_state(account: &Account) -> Option<AuthState> {
    Some(match account.state {
        AccountAuth::Offline => return None,
        AccountAuth::SignedOut => AuthState::SignedOut,
        AccountAuth::AwaitingCode => AuthState::AwaitingCode {
            uri: account.verification_uri.clone().unwrap_or_default(),
            code: account.user_code.clone().unwrap_or_default(),
        },
        AccountAuth::SignedIn => AuthState::Authenticated,
        AccountAuth::Failed => AuthState::Failed(account.reason.clone().unwrap_or_default()),
    })
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

    fn set_joining(&mut self, joining: bool) {
        self.with(|snapshot| snapshot.joining = joining);
    }

    fn account_generation(&mut self) -> Option<u64> {
        Some(self.with(|snapshot| snapshot.auth_generation))
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
