//! [`AccountControl`] over the core's launcher control endpoint. Workers poll
//! auth/events often, catalogs and Home/public feeds rarely. Profile has an
//! independent worker woken by opening, retry and account changes, so it never
//! waits on unrelated feeds. Sign-out requests queue to the events worker.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender, bounded};
use protocol::launcher_control::{
    self, Account, AuthState as CoreAuth, ConnectProgress, ConnectStage, FeaturedServer, Friend,
    Gathering, Home, Message, MessageEvent, Profile, Realm, ServerPing,
};

use super::account_control::{AccountControl, AccountEvent};
use super::{AuthState, MenuFriendCard, MenuRealmCard, MenuServerCard};
use launcher::menu::view::{
    ButtonArt, InboxItem, JoinStage, LiveEventCard, MenuGameCard, MenuHome, MenuProfile, PingInfo,
    ServerDetails,
};

#[cfg(test)]
mod home_promo;

mod message_reports;
pub(super) mod profile_worker;
mod server_preparation;

#[cfg(all(test, unix))]
mod profile_polling_tests;

/// How often auth state and events refresh.
const EVENT_INTERVAL: Duration = Duration::from_secs(1);
/// How often events refresh while a join is under way, so its progress bar moves smoothly.
const JOIN_EVENT_INTERVAL: Duration = Duration::from_millis(250);
/// How often the catalog lists refresh (they can take tens of seconds).
const CATALOG_INTERVAL: Duration = Duration::from_secs(30);
/// How often the screen feeds are read; the core answers from its catalog cache
/// and refreshes upstream on its own schedule, so this only picks up fresh data.
const FEED_INTERVAL: Duration = Duration::from_secs(30);
/// How soon a feed that failed is asked again.
const FEED_RETRY: Duration = Duration::from_secs(15);
/// How often shown server rows are pinged.
const PING_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Snapshot {
    auth_generation: u64,
    /// Wakes the catalog worker when its account identity changes.
    catalog_wake: Option<Sender<()>>,
    /// Wakes Profile independently when its account identity changes.
    profile_wake: Option<Sender<()>>,
    account: Option<Account>,
    realms: Option<Vec<Realm>>,
    friends: Option<Vec<Friend>>,
    /// Delivered once per fetch.
    featured: Option<Vec<FeaturedServer>>,
    gatherings: Option<Vec<Gathering>>,
    profile: Option<Result<Profile, ()>>,
    ping_targets: Vec<String>,
    pings: Option<Vec<ServerPing>>,
    home: Option<Home>,
    events: Vec<AccountEvent>,
    last_disconnect: Option<u64>,
    connect: Option<ConnectProgress>,
    /// The menu is connecting, so the events worker polls faster.
    joining: bool,
}

impl Snapshot {
    /// Retires values tied to the old account before accepting responses for another identity.
    fn retire_account_data(&mut self) {
        self.auth_generation = self.auth_generation.wrapping_add(1);
        self.realms = None;
        self.friends = None;
        self.profile = None;
        self.home = None;
        self.gatherings = None;
        if let Some(wake) = &self.catalog_wake {
            // A queued wake already covers the newest snapshot; never block a frame.
            let _ = wake.try_send(());
        }
        if let Some(wake) = &self.profile_wake {
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

/// The menu's link to a running core's launcher control endpoint.
#[derive(Resource)]
pub(crate) struct LauncherAccount {
    snapshot: Arc<Mutex<Snapshot>>,
    sign_out: Sender<()>,
    profile_refresh: Sender<()>,
    message_reports: Sender<MessageEvent>,
    server_preparation: server_preparation::ServerPreparation,
    /// Dropping it stops the catalog and feed workers.
    _alive: Sender<()>,
    socket_dir: PathBuf,
}

impl LauncherAccount {
    /// Start polling the control endpoint under `socket_dir`; the workers stop
    /// when this is dropped. Events, the slow catalog and the screen feeds each
    /// poll on their own worker, publishing every answer as it arrives.
    pub(crate) fn new(socket_dir: PathBuf) -> Self {
        let (catalog_wake, catalog_changes) = bounded(1);
        let (profile_refresh, profile_requests) = bounded(1);
        let snapshot = Arc::new(Mutex::new(Snapshot {
            catalog_wake: Some(catalog_wake),
            profile_wake: Some(profile_refresh.clone()),
            ..Default::default()
        }));
        let (sign_out, requests) = bounded(1);
        let (alive, stop) = bounded(0);
        let message_reports = message_reports::start(socket_dir.clone(), stop.clone());
        let server_preparation =
            server_preparation::ServerPreparation::start(socket_dir.clone(), stop.clone());
        let shared = Arc::clone(&snapshot);
        let dir = socket_dir.clone();
        thread::spawn(move || poll_events(&dir, &shared, &requests));
        let (shared, dir, until) = (Arc::clone(&snapshot), socket_dir.clone(), stop.clone());
        thread::spawn(move || poll_catalog(&dir, &shared, &until, &catalog_changes));
        let (shared, dir, until) = (Arc::clone(&snapshot), socket_dir.clone(), stop.clone());
        thread::spawn(move || poll_feeds(&dir, &shared, &until));
        let (shared, dir) = (Arc::clone(&snapshot), socket_dir.clone());
        thread::spawn(move || profile_worker::poll(&dir, &shared, &stop, &profile_requests));
        Self {
            snapshot,
            sign_out,
            profile_refresh,
            message_reports,
            server_preparation,
            _alive: alive,
            socket_dir,
        }
    }

    /// The control endpoint directory this link polls.
    pub(crate) fn socket_dir(&self) -> &std::path::Path {
        &self.socket_dir
    }

    fn with<T>(&self, read: impl FnOnce(&mut Snapshot) -> T) -> T {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        read(&mut snapshot)
    }
}

fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
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
        default(CATALOG_INTERVAL) => true,
    }
}

fn poll_events(socket_dir: &std::path::Path, shared: &Mutex<Snapshot>, requests: &Receiver<()>) {
    let Some(runtime) = runtime() else {
        return;
    };
    let mut ping_due = Instant::now();
    let mut pinged: Vec<String> = Vec::new();
    loop {
        let joining = shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .joining;
        let interval = if joining {
            JOIN_EVENT_INTERVAL
        } else {
            EVENT_INTERVAL
        };
        match requests.recv_timeout(interval) {
            Ok(()) => {
                let _ = runtime.block_on(launcher_control::sign_out(socket_dir));
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        let generation = auth_generation(shared);
        if let Ok(events) = runtime.block_on(launcher_control::poll_events(socket_dir)) {
            publish(shared, |snapshot| {
                if let Some(disconnect) = events.disconnect
                    && snapshot.last_disconnect != Some(disconnect.sequence)
                {
                    // The first poll only records the standing disconnect.
                    if snapshot.last_disconnect.is_some() {
                        // An empty message reads as vanilla's no-reason line.
                        let reason = disconnect.message.trim().to_owned();
                        snapshot.events.push(AccountEvent::Disconnected { reason });
                    }
                    snapshot.last_disconnect = Some(disconnect.sequence);
                }
                snapshot.last_disconnect.get_or_insert(0);
                snapshot.connect = events.connect;
            });
            publish_account(shared, generation, |snapshot| {
                snapshot.set_account(events.auth);
            });
        }
        let targets = shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .ping_targets
            .clone();
        // The feeds worker publishes first, so rows it adds are new targets here
        // and are pinged on the next tick instead of a round later.
        if targets != pinged {
            ping_due = Instant::now();
        }
        if !targets.is_empty() && Instant::now() >= ping_due {
            ping_due = Instant::now() + PING_INTERVAL;
            pinged = targets.clone();
            let pongs = runtime.block_on(launcher_control::ping_servers(socket_dir, &targets));
            let pings = round_results(
                &targets,
                settle("ping", pongs, &mut false).unwrap_or_default(),
            );
            publish(shared, |snapshot| snapshot.pings = Some(pings));
        }
    }
}

fn poll_catalog(
    socket_dir: &std::path::Path,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    changes: &Receiver<()>,
) {
    let Some(runtime) = runtime() else {
        return;
    };
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
            if let Ok(realms) = runtime.block_on(launcher_control::list_realms(socket_dir)) {
                publish_account(shared, generation, |snapshot| {
                    snapshot.realms = Some(realms)
                });
            }
            if let Ok(friends) = runtime.block_on(launcher_control::list_friends(socket_dir)) {
                publish_account(shared, generation, |snapshot| {
                    snapshot.friends = Some(friends)
                });
            }
        }
        if !wait_catalog(stop, changes) {
            return;
        }
    }
}

/// Polls Home and public catalogs without delaying the independent Profile worker.
fn poll_feeds(socket_dir: &std::path::Path, shared: &Mutex<Snapshot>, stop: &Receiver<()>) {
    let Some(runtime) = runtime() else {
        return;
    };
    let mut reported = HashSet::new();
    loop {
        let mut failed = false;
        let generation = auth_generation(shared);
        let home = runtime.block_on(launcher_control::home(socket_dir));
        if let Some(home) = settle("home", home, &mut failed) {
            publish_account(shared, generation, |snapshot| {
                snapshot.home = Some(home.clone())
            });
            report_impressions(&runtime, socket_dir, &home, &mut reported);
        }
        let featured = runtime.block_on(launcher_control::list_featured_servers(socket_dir));
        if let Some(featured) = settle("featured servers", featured, &mut failed) {
            publish(shared, |snapshot| snapshot.featured = Some(featured));
        }
        let generation = auth_generation(shared);
        let gatherings = runtime.block_on(launcher_control::list_gatherings(socket_dir));
        if let Some(gatherings) = settle("gatherings", gatherings, &mut failed) {
            publish_account(shared, generation, |snapshot| {
                snapshot.gatherings = Some(gatherings)
            });
        }
        if !wait(stop, if failed { FEED_RETRY } else { FEED_INTERVAL }) {
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
            bevy::log::warn!(feed, %error, "launcher feed failed; retrying soon");
            None
        }
    }
}

/// Button-art surfaces the start screen shows, reported once per message instance.
const SHOWN_SURFACES: [&str; 2] = ["PlayButton", "MarketplaceButton"];

fn report_impressions(
    runtime: &tokio::runtime::Runtime,
    socket_dir: &std::path::Path,
    home: &Home,
    reported: &mut HashSet<String>,
) {
    for message in &home.messages {
        if !SHOWN_SURFACES.contains(&message.surface.as_str())
            || !reported.insert(message.instance_id.clone())
        {
            continue;
        }
        let event = MessageEvent {
            event_type: "Impression".to_owned(),
            instance_id: message.instance_id.clone(),
            report_id: message.report_id.clone(),
            button_id: String::new(),
        };
        let _ = runtime.block_on(launcher_control::report_message_event(socket_dir, &event));
    }
}

/// The start screen's view of the core's home feed.
fn menu_home(home: &Home, now_unix: i64) -> MenuHome {
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
        play_art: art("PlayButton"),
        store_art: art("MarketplaceButton"),
        inbox_unread: home.inbox.unread,
        inbox_counts: home
            .inbox
            .categories
            .iter()
            .filter_map(|category| {
                Some((
                    super::inbox::category_index(&category.kind)?,
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
            .filter(|message| message.surface == "InboxMessage")
            .map(|message| InboxItem {
                instance_id: message.instance_id.clone(),
                report_id: message.report_id.clone(),
                received: message.received.clone(),
                source: message.sender.clone(),
                header: message.header.clone(),
                body: message.body.clone(),
                category: message.category.clone(),
                unread: !message.status.eq_ignore_ascii_case("read"),
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
        if id.contains("banner") {
            art.banner_texture = image.path.clone();
            continue;
        }
        let hover = id.contains("hover");
        let foreground = id.contains("fore") || id.contains("fg");
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
        ConnectStage::Connecting => JoinStage::Connecting,
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
    }
}

impl AccountControl for LauncherAccount {
    fn prepare_selected_server(&mut self, address: Option<&str>) {
        let generation = self.with(|snapshot| {
            snapshot.account.as_ref().and_then(|account| {
                (account.state == CoreAuth::SignedIn).then_some(snapshot.auth_generation)
            })
        });
        self.server_preparation.select(address, generation);
    }

    fn account_status(&mut self) -> Option<AuthState> {
        self.with(|snapshot| snapshot.account.as_ref().and_then(auth_state))
    }

    fn join_stage(&mut self) -> Option<JoinStage> {
        self.with(|snapshot| snapshot.connect.as_ref().map(join_stage))
    }

    fn set_joining(&mut self, joining: bool) {
        self.server_preparation.set_joining(joining);
        self.with(|snapshot| snapshot.joining = joining);
    }

    fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
        self.with(|snapshot| {
            snapshot.realms.as_ref().map(|realms| {
                realms
                    .iter()
                    .map(|realm| MenuRealmCard {
                        name: realm.name.clone(),
                        state: realm.state.clone(),
                        target: realm.target.clone(),
                        address: realm.address.clone().unwrap_or_default(),
                        owner: realm.owner.clone(),
                        online_players: realm.online_players,
                        max_players: realm.max_players,
                        days_left: realm.days_left,
                        expired: realm.expired,
                        member: realm.member,
                    })
                    .collect()
            })
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

    fn featured(&mut self) -> Option<Vec<(MenuServerCard, ServerDetails)>> {
        let servers = self.with(|snapshot| snapshot.featured.take())?;
        Some(servers.iter().map(featured_card).collect())
    }

    fn gatherings(&mut self) -> Option<Vec<(MenuServerCard, ServerDetails)>> {
        let gatherings = self.with(|snapshot| snapshot.gatherings.take())?;
        Some(
            gatherings
                .iter()
                .filter(|gathering| !gathering.id.is_empty())
                .map(|gathering| {
                    let card = MenuServerCard {
                        name: gathering.name.clone(),
                        address: format!(
                            "{}{}",
                            super::launcher_core::GATHERING_ADDRESS_PREFIX,
                            gathering.id
                        ),
                        caption: gathering.caption.clone(),
                        image_path: gathering.image.path.clone(),
                        icon: None,
                    };
                    let details = ServerDetails {
                        description: gathering.description.clone(),
                        ..ServerDetails::default()
                    };
                    (card, details)
                })
                .collect(),
        )
    }

    /// Queues an inbox action on the dedicated reporting worker.
    fn report_message(&mut self, event: MessageEvent) {
        let _ = self.message_reports.send(event);
    }

    fn home(&mut self) -> Option<MenuHome> {
        let home = self.with(|snapshot| snapshot.home.take())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        Some(menu_home(&home, now))
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
        if matches!(
            self.profile_refresh.try_send(()),
            Err(crossbeam_channel::TrySendError::Disconnected(_))
        ) {
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
}

fn featured_card(server: &FeaturedServer) -> (MenuServerCard, ServerDetails) {
    let card = MenuServerCard {
        name: server.name.clone(),
        address: server.address.clone(),
        caption: server.caption.clone(),
        image_path: server.logo.path.clone(),
        icon: None,
    };
    let details = ServerDetails {
        description: server.description.clone(),
        news_title: server.news_title.clone(),
        news: server.news.clone(),
        screenshots: server
            .screenshots
            .iter()
            .filter(|shot| !shot.path.is_empty())
            .map(|shot| shot.path.clone())
            .collect(),
        games: server
            .games
            .iter()
            .map(|game| MenuGameCard {
                title: game.title.clone(),
                subtitle: game.subtitle.clone(),
                description: game.description.clone(),
                image_path: game.image.path.clone(),
            })
            .collect(),
    };
    (card, details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_ui_account_changes_wake_catalog_without_repeated_poll_wakes() {
        let (wake, changes) = bounded(1);
        let mut snapshot = Snapshot {
            catalog_wake: Some(wake),
            ..Default::default()
        };
        let account = Account {
            state: CoreAuth::SignedIn,
            gamertag: Some("Alex".into()),
            verification_uri: None,
            user_code: None,
            reason: None,
        };
        snapshot.set_account(account.clone());
        assert_eq!(
            changes.try_recv(),
            Ok(()),
            "the first account must wake catalogs"
        );
        snapshot.set_account(account.clone());
        assert_eq!(
            changes.try_recv(),
            Err(crossbeam_channel::TryRecvError::Empty)
        );
        snapshot.set_account(Account {
            gamertag: Some("Steve".into()),
            ..account
        });
        assert_eq!(
            changes.try_recv(),
            Ok(()),
            "another identity must wake catalogs"
        );
        snapshot.retire_account_data();
        assert_eq!(changes.try_recv(), Ok(()), "sign-out must wake catalogs");
    }

    #[test]
    fn review_ui_catalog_wait_handles_wakes_and_shutdown() {
        let (alive, stop) = bounded(0);
        let (wake, changes) = bounded(1);
        wake.try_send(()).unwrap();
        assert!(wait_catalog(&stop, &changes));
        drop(alive);
        assert!(!wait_catalog(&stop, &changes));
    }

    // A pinged server that sent no pong reads offline instead of loading.
    #[test]
    fn a_round_answers_for_every_target() {
        let pong = ServerPing {
            address: "a:1".to_owned(),
            online: true,
            ping_ms: 40,
            ..ServerPing::default()
        };
        let targets = ["a:1".to_owned(), "b:2".to_owned()];
        let round = round_results(&targets, vec![pong.clone()]);
        assert_eq!(round[0], pong);
        assert_eq!(round[1].address, "b:2");
        assert!(!round[1].online);
        assert!(
            round_results(&targets, Vec::new())
                .iter()
                .all(|ping| !ping.online)
        );
    }

    #[test]
    fn tile_images_sort_into_button_layers() {
        let image = |id: &str| protocol::launcher_control::MessageImage {
            id: id.into(),
            url: String::new(),
            path: format!("/art/{id}.img"),
        };
        let message = Message {
            surface: "PlayButton".into(),
            banner: "New".into(),
            images: vec![
                image("background"),
                image("hoverForeground"),
                image("hover"),
            ],
            ..Message::default()
        };
        let home = Home {
            messages: vec![message],
            realm_invites: 2,
            ..Home::default()
        };
        let menu = menu_home(&home, 0);
        let art = menu.play_art.expect("play art");
        assert_eq!(art.default_background, "/art/background.img");
        assert_eq!(art.hover_foreground, "/art/hoverForeground.img");
        assert_eq!(art.hover_background, "/art/hover.img");
        assert_eq!(art.banner, "New");
        assert!(menu.store_art.is_none());
        assert_eq!(menu.realm_invites, 2);
    }

    #[test]
    fn featured_servers_split_into_cards_and_details() {
        let server = FeaturedServer {
            name: "S".into(),
            address: "a.test:19132".into(),
            news: "Update".into(),
            screenshots: vec![
                protocol::launcher_control::Artwork {
                    url: "https://a.test/s.png".into(),
                    path: String::new(),
                },
                protocol::launcher_control::Artwork {
                    url: "https://a.test/t.png".into(),
                    path: "/art/t.img".into(),
                },
            ],
            ..FeaturedServer::default()
        };
        let (card, details) = featured_card(&server);
        assert_eq!(card.address, "a.test:19132");
        assert_eq!(details.news, "Update");
        assert_eq!(details.screenshots, vec!["/art/t.img".to_owned()]);
    }

    #[test]
    fn core_connect_stages_map_to_join_stages() {
        let progress = |stage| ConnectProgress {
            stage,
            packs_done: 1,
            packs_total: 2,
            received_bytes: 3,
            total_bytes: 4,
        };
        assert_eq!(join_stage(&progress(ConnectStage::Realm)), JoinStage::Realm);
        assert_eq!(
            join_stage(&progress(ConnectStage::Connecting)),
            JoinStage::Connecting
        );
        assert_eq!(
            join_stage(&progress(ConnectStage::Packs)),
            JoinStage::Packs {
                done: 1,
                total: 2,
                received_bytes: 3,
                total_bytes: 4
            }
        );
    }

    #[test]
    fn core_account_states_map_to_menu_sign_in_states() {
        let account = |state| Account {
            state,
            verification_uri: Some("https://aka.ms/remoteconnect".into()),
            user_code: Some("ABCD".into()),
            gamertag: None,
            reason: Some("expired".into()),
        };
        assert_eq!(auth_state(&account(CoreAuth::Offline)), None);
        assert_eq!(
            auth_state(&account(CoreAuth::SignedOut)),
            Some(AuthState::SignedOut)
        );
        assert_eq!(
            auth_state(&account(CoreAuth::AwaitingCode)),
            Some(AuthState::AwaitingCode {
                uri: "https://aka.ms/remoteconnect".into(),
                code: "ABCD".into()
            })
        );
        assert_eq!(
            auth_state(&account(CoreAuth::SignedIn)),
            Some(AuthState::Authenticated)
        );
        assert_eq!(
            auth_state(&account(CoreAuth::Failed)),
            Some(AuthState::Failed("expired".into()))
        );
    }
    #[test]
    fn duplicate_ping_targets_keep_the_same_online_result() {
        let targets = vec!["server.test".into(), "server.test".into()];
        let results = round_results(
            &targets,
            vec![ServerPing {
                address: targets[0].clone(),
                online: true,
                ..Default::default()
            }],
        );
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|ping| ping.online));
    }
    #[test]
    fn account_responses_from_before_sign_out_are_discarded() {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let generation = auth_generation(&snapshot);
        let (sign_out, _requests) = bounded(1);
        let (alive, _stop) = bounded(0);
        let mut account = LauncherAccount {
            snapshot: Arc::clone(&snapshot),
            sign_out,
            profile_refresh: crossbeam_channel::bounded(1).0,
            _alive: alive,
            socket_dir: PathBuf::new(),
            message_reports: crossbeam_channel::unbounded().0,
            server_preparation: server_preparation::ServerPreparation::disconnected(),
        };
        assert!(account.sign_out());
        publish_account(&snapshot, generation, |snapshot| {
            snapshot.realms = Some(Vec::new());
            snapshot.friends = Some(Vec::new());
        });
        let retained = snapshot.lock().unwrap();
        assert!(retained.realms.is_none() && retained.friends.is_none());
    }
}
