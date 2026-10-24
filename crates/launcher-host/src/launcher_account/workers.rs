//! The existing account worker topology borrows one application reactor.

use super::*;

/// Keeps credential replacement behind actual workers and any unresolved original host membership.
/// Process shutdown waits only for the worker tracker, so uncertain remote cleanup cannot hang Quit.
#[derive(Clone)]
pub struct AccountWorkerLifetime {
    pub(super) workers: TaskTracker,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(super) host: native_host::Retirement,
}

impl AccountWorkerLifetime {
    /// Reports whether the original account's workers and retained host obligation have retired.
    pub fn is_empty(&self) -> bool {
        if !self.workers.is_empty() {
            return false;
        }
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if !self.host.is_empty() {
            return false;
        }
        true
    }
}

impl LauncherAccount {
    /// Starts existing workers, retaining final process-stop observation on the event owner.
    pub(super) fn start(
        artwork: PathBuf,
        initial: Snapshot,
        runtime: Handle,
        tasks: TaskTracker,
        process_stop: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Self {
        let workers = TaskTracker::new();
        // Startup stays counted until every initial worker is armed, including on unwind.
        let startup = workers.token();
        workers.close();
        let completion = workers.clone();
        let _account_completion = tasks.spawn_on(async move { completion.wait().await }, &runtime);
        let tasks = workers.clone();
        let (feed_wake, feed_changes) = bounded(1);
        let (home_wake, home_changes) = bounded(1);
        let (home_cancel, home_changed) = tokio::sync::watch::channel(home_feed::Wake::Account);
        let (featured_cancel, featured_changed) =
            tokio::sync::watch::channel(featured_feed::Wake::Account);
        let (profile_refresh, profile_requests) = profile_worker::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot {
            feed_wake: Some(feed_wake.clone()),
            home_wake: Some(home_wake.clone()),
            home_cancel: Some(home_cancel),
            featured_cancel: Some(featured_cancel),
            profile_wake: Some(profile_refresh.clone()),
            ..initial
        }));
        let presence_lifetime = TitlePresenceLifetime::new();
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        native_host::start(Arc::clone(&snapshot), runtime.clone(), &tasks);
        let (sign_out, requests) = bounded(1);
        let (alive, stop) = bounded(0);
        let message_reports = message_reports::start(
            Arc::clone(&snapshot),
            stop.clone(),
            home_changed.clone(),
            runtime.clone(),
            tasks.clone(),
        );
        snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .home_reports = Some(message_reports.clone());
        let invites = invites::start(
            artwork.clone(),
            Arc::clone(&snapshot),
            stop.clone(),
            runtime.clone(),
            tasks.clone(),
        );
        let realm_membership =
            realm_membership::start(Arc::clone(&snapshot), runtime.clone(), tasks.clone());
        let cinnabar_rosters =
            cinnabar_players::start(Arc::clone(&snapshot), runtime.clone(), tasks.clone());
        let shared = Arc::clone(&snapshot);
        let lifetime = presence_lifetime.clone();
        let executor = runtime.clone();
        thread::spawn(crate::worker::tracked(
            move || events::poll(&shared, &requests, &lifetime, &executor, process_stop),
            &tasks,
        ));
        let (shared, until) = (Arc::clone(&snapshot), stop.clone());
        let (art, wake) = (artwork.clone(), feed_wake.clone());
        let executor = runtime.clone();
        let descendants = tasks.clone();
        thread::spawn(crate::worker::tracked(
            move || {
                let art = artwork::FeedArt::start(
                    artwork::feed_images(&art),
                    wake,
                    executor.clone(),
                    descendants,
                );
                feeds::poll_featured(
                    &art,
                    &shared,
                    &until,
                    &feed_changes,
                    featured_changed,
                    &executor,
                )
            },
            &tasks,
        ));
        let (shared, until) = (Arc::clone(&snapshot), stop.clone());
        let (art, wake) = (artwork.clone(), home_wake.clone());
        let executor = runtime.clone();
        let descendants = tasks.clone();
        thread::spawn(crate::worker::tracked(
            move || {
                let art = artwork::FeedArt::start(
                    artwork::feed_images(&art),
                    wake,
                    executor.clone(),
                    descendants,
                );
                feeds::poll_home(
                    &art,
                    &shared,
                    &until,
                    &home_changes,
                    home_changed,
                    &executor,
                )
            },
            &tasks,
        ));
        let _ = shared_players::start(Arc::clone(&snapshot), stop.clone(), tasks.clone());
        let shared = Arc::clone(&snapshot);
        thread::spawn(crate::worker::tracked(
            move || profile_worker::poll(&artwork, &shared, profile_requests, &runtime),
            &tasks,
        ));
        drop(startup);
        Self {
            snapshot,
            sign_out,
            profile_refresh,
            message_reports,
            invites,
            realm_membership,
            cinnabar_rosters,
            _alive: alive,
            presence_lifetime,
            workers,
        }
    }
}
