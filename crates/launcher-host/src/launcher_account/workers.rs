//! The existing account worker topology borrows one application reactor.

use super::*;

impl LauncherAccount {
    /// Starts the existing feed workers and one event/runtime owner.
    pub(super) fn start(
        socket_dir: Option<PathBuf>,
        artwork: PathBuf,
        initial: Snapshot,
        runtime: Handle,
        tasks: TaskTracker,
    ) -> Self {
        let (catalog_wake, catalog_changes) = bounded(1);
        let (feed_wake, feed_changes) = bounded(1);
        let (home_wake, home_changes) = bounded(1);
        let (home_cancel, home_changed) = tokio::sync::watch::channel(home_feed::Wake::Account);
        let (featured_cancel, featured_changed) =
            tokio::sync::watch::channel(featured_feed::Wake::Account);
        let (profile_refresh, profile_requests) = profile_worker::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot {
            catalog_wake: Some(catalog_wake),
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
            socket_dir.clone(),
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
            socket_dir.clone(),
            artwork.clone(),
            Arc::clone(&snapshot),
            stop.clone(),
            runtime.clone(),
            tasks.clone(),
        );
        let realm_membership = realm_membership::start(
            socket_dir.clone(),
            Arc::clone(&snapshot),
            runtime.clone(),
            tasks.clone(),
        );
        let cinnabar_rosters = cinnabar_players::start(
            socket_dir.clone(),
            Arc::clone(&snapshot),
            runtime.clone(),
            tasks.clone(),
        );
        let shared = Arc::clone(&snapshot);
        let dir = socket_dir.clone();
        let lifetime = presence_lifetime.clone();
        let executor = runtime.clone();
        thread::spawn(crate::worker::tracked(
            move || events::poll(dir.as_deref(), &shared, &requests, &lifetime, &executor),
            &tasks,
        ));
        let (shared, dir, until) = (Arc::clone(&snapshot), socket_dir.clone(), stop.clone());
        let executor = runtime.clone();
        thread::spawn(crate::worker::tracked(
            move || poll_catalog(dir.as_deref(), &shared, &until, &catalog_changes, &executor),
            &tasks,
        ));
        let (shared, dir, until) = (Arc::clone(&snapshot), socket_dir.clone(), stop.clone());
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
                    dir.as_deref(),
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
        let (shared, dir, until) = (Arc::clone(&snapshot), socket_dir.clone(), stop.clone());
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
                    dir.as_deref(),
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
        let _ = shared_players::start(
            socket_dir.clone(),
            Arc::clone(&snapshot),
            stop.clone(),
            runtime.clone(),
            tasks.clone(),
        );
        let (shared, dir) = (Arc::clone(&snapshot), socket_dir.clone());
        thread::spawn(crate::worker::tracked(
            move || {
                profile_worker::poll(
                    dir.as_deref(),
                    &artwork,
                    &shared,
                    profile_requests,
                    &runtime,
                )
            },
            &tasks,
        ));
        Self {
            snapshot,
            sign_out,
            profile_refresh,
            message_reports,
            invites,
            realm_membership,
            cinnabar_rosters,
            _alive: alive,
            socket_dir,
            presence_lifetime,
        }
    }
}

/// Rejects an unavailable legacy transport before constructing an RPC; no dummy path is used.
pub(super) fn require_core_directory(
    directory: Option<&std::path::Path>,
) -> Result<&std::path::Path, bridge::BridgeError> {
    directory.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "Core control transport is unavailable",
        )
        .into()
    })
}
