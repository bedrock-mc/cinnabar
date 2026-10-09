//! Independent Home and featured workers, plus concurrent account catalog requests.

use std::{collections::HashSet, path::Path, sync::Mutex};

use crossbeam_channel::Receiver;
use protocol::launcher_control::{self, BridgeError, FeaturedServer, Friend, Home, Realm};

use super::{FEED_INTERVAL, FEED_RETRY, Snapshot, publish_account, settle};

/// The launcher requests the menu workers make: the core in production, fakes in tests.
pub(super) trait FeedSource {
    async fn home(&self) -> Result<Home, BridgeError>;
    async fn featured(
        &self,
        include_player_counts: bool,
    ) -> Result<Vec<FeaturedServer>, BridgeError>;
    async fn realms(&self) -> Result<Vec<Realm>, BridgeError>;
    async fn friends(&self) -> Result<Vec<Friend>, BridgeError>;
}

pub(super) struct CoreFeeds<'a>(pub(super) &'a std::path::Path);

impl FeedSource for CoreFeeds<'_> {
    async fn home(&self) -> Result<Home, BridgeError> {
        launcher_control::home(self.0).await
    }
    async fn featured(
        &self,
        include_player_counts: bool,
    ) -> Result<Vec<FeaturedServer>, BridgeError> {
        if include_player_counts {
            launcher_control::list_featured_servers_with_counts(self.0).await
        } else {
            launcher_control::list_featured_servers(self.0).await
        }
    }
    async fn realms(&self) -> Result<Vec<Realm>, BridgeError> {
        launcher_control::list_realms(self.0).await
    }
    async fn friends(&self) -> Result<Vec<Friend>, BridgeError> {
        launcher_control::list_friends(self.0).await
    }
}

/// Polls featured data independently so opening details never waits for Home or its reports.
pub(super) fn poll_featured(
    socket_dir: &Path,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    changes: &Receiver<()>,
) {
    let Some(runtime) = super::runtime() else {
        return;
    };
    loop {
        while changes.try_recv().is_ok() {}
        let failed = runtime.block_on(featured_round(&CoreFeeds(socket_dir), shared));
        if !wait_changes(
            stop,
            changes,
            if failed { FEED_RETRY } else { FEED_INTERVAL },
        ) {
            return;
        }
    }
}

/// Polls Home and reports its impressions without blocking other screen feeds.
pub(super) fn poll_home(
    socket_dir: &Path,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    changes: &Receiver<()>,
) {
    let Some(runtime) = super::runtime() else {
        return;
    };
    let mut reported = HashSet::new();
    loop {
        while changes.try_recv().is_ok() {}
        let (home, failed) = runtime.block_on(home_round(&CoreFeeds(socket_dir), shared));
        if let Some(home) = home {
            super::report_impressions(&runtime, socket_dir, &home, &mut reported);
        }
        if !wait_changes(
            stop,
            changes,
            if failed { FEED_RETRY } else { FEED_INTERVAL },
        ) {
            return;
        }
    }
}

/// Publishes Home under its requesting account and returns it for impression reporting.
async fn home_round(source: &impl FeedSource, shared: &Mutex<Snapshot>) -> (Option<Home>, bool) {
    let generation = super::auth_generation(shared);
    let mut failed = false;
    let home = settle("home", source.home().await, &mut failed);
    if let Some(home) = &home {
        publish_account(shared, generation, |snapshot| {
            snapshot.home = Some(home.clone())
        });
    }
    (home, failed)
}

/// Publishes featured details without allowing an older layout read to erase visible counts.
async fn featured_round(source: &impl FeedSource, shared: &Mutex<Snapshot>) -> bool {
    let (generation, include_player_counts) = {
        let snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
        (snapshot.auth_generation, snapshot.player_counts_visible)
    };
    let mut failed = false;
    if let Some(featured) = settle(
        "featured servers",
        source.featured(include_player_counts).await,
        &mut failed,
    ) {
        publish_account(shared, generation, |snapshot| {
            if include_player_counts || !snapshot.player_counts_visible {
                snapshot.featured = Some(featured);
            }
        });
    }
    failed
}

/// Wakes the feed worker for a screen/account change, its next read, or shutdown.
pub(super) fn wait_changes(
    stop: &crossbeam_channel::Receiver<()>,
    changes: &crossbeam_channel::Receiver<()>,
    interval: std::time::Duration,
) -> bool {
    crossbeam_channel::select! {
        recv(stop) -> _ => false,
        recv(changes) -> result => result.is_ok(),
        default(interval) => true,
    }
}

/// Requests the account's Realms and friends together; each publishes as soon as it arrives,
/// and a failed list keeps its last value.
pub(super) async fn catalog_round(
    source: &impl FeedSource,
    shared: &Mutex<Snapshot>,
    generation: u64,
) {
    let revision = shared
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .realm_catalog_revision;
    tokio::join!(
        async {
            if let Ok(realms) = source.realms().await {
                publish_account(shared, generation, |snapshot| {
                    if snapshot.realm_catalog_revision == revision {
                        snapshot.realms = Some(realms);
                    }
                });
            }
        },
        async {
            if let Ok(friends) = source.friends().await {
                publish_account(shared, generation, |snapshot| {
                    snapshot.friends = Some(friends)
                });
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        pin::pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Poll, Waker},
    };

    use tokio::sync::Notify;

    use super::*;

    /// Each request counts its start, then waits until its own gate is released.
    #[derive(Default)]
    struct GatedFeeds {
        started: AtomicUsize,
        counted: AtomicUsize,
        home: Notify,
        featured: Notify,
        realms: Notify,
        friends: Notify,
        fail: bool,
    }

    impl GatedFeeds {
        async fn answer<T: Default>(&self, gate: &Notify) -> Result<T, BridgeError> {
            self.started.fetch_add(1, Ordering::SeqCst);
            gate.notified().await;
            if self.fail {
                Err(BridgeError::ControlClosed)
            } else {
                Ok(T::default())
            }
        }
    }

    impl FeedSource for GatedFeeds {
        async fn home(&self) -> Result<Home, BridgeError> {
            self.answer(&self.home).await
        }
        async fn featured(
            &self,
            include_player_counts: bool,
        ) -> Result<Vec<FeaturedServer>, BridgeError> {
            if include_player_counts {
                self.counted.fetch_add(1, Ordering::SeqCst);
            }
            self.answer(&self.featured).await
        }
        async fn realms(&self) -> Result<Vec<Realm>, BridgeError> {
            self.answer(&self.realms).await
        }
        async fn friends(&self) -> Result<Vec<Friend>, BridgeError> {
            self.answer(&self.friends).await
        }
    }

    fn poll<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    fn published(shared: &Mutex<Snapshot>) -> [bool; 4] {
        let snapshot = shared.lock().unwrap();
        [
            snapshot.home.is_some(),
            snapshot.featured.is_some(),
            snapshot.realms.is_some(),
            snapshot.friends.is_some(),
        ]
    }

    #[test]
    fn featured_reloads_while_home_waits_and_each_feed_publishes_on_arrival() {
        let feeds = GatedFeeds::default();
        let shared = Mutex::new(Snapshot::default());
        let mut home = pin!(home_round(&feeds, &shared));
        let mut featured = pin!(featured_round(&feeds, &shared));
        assert!(poll(home.as_mut()).is_pending());
        assert!(poll(featured.as_mut()).is_pending());
        assert_eq!(feeds.started.load(Ordering::SeqCst), 2);
        assert_eq!(feeds.counted.load(Ordering::SeqCst), 0);

        feeds.featured.notify_one();
        assert_eq!(poll(featured.as_mut()), Poll::Ready(false));
        assert_eq!(published(&shared), [false, true, false, false]);
        shared.lock().unwrap().player_counts_visible = true;
        let mut counted = pin!(featured_round(&feeds, &shared));
        assert!(poll(counted.as_mut()).is_pending());
        assert_eq!(feeds.counted.load(Ordering::SeqCst), 1);
        feeds.featured.notify_one();
        assert_eq!(poll(counted.as_mut()), Poll::Ready(false));
        assert!(poll(home.as_mut()).is_pending());
        feeds.home.notify_one();
        let Poll::Ready((value, failed)) = poll(home.as_mut()) else {
            panic!("Home outlived its answer");
        };
        assert!(value.is_some() && !failed);

        let mut catalog = pin!(catalog_round(&feeds, &shared, 0));
        assert!(poll(catalog.as_mut()).is_pending());
        assert_eq!(feeds.started.load(Ordering::SeqCst), 5);
        feeds.friends.notify_one();
        assert!(poll(catalog.as_mut()).is_pending());
        assert_eq!(published(&shared), [true, true, false, true]);
        feeds.realms.notify_one();
        assert!(poll(catalog.as_mut()).is_ready());
        assert_eq!(published(&shared), [true; 4]);
    }

    #[test]
    fn a_failed_feed_reports_failure_without_publishing() {
        let feeds = GatedFeeds {
            fail: true,
            ..Default::default()
        };
        let shared = Mutex::new(Snapshot::default());
        let mut home = pin!(home_round(&feeds, &shared));
        let mut featured = pin!(featured_round(&feeds, &shared));
        assert!(poll(home.as_mut()).is_pending());
        assert!(poll(featured.as_mut()).is_pending());
        feeds.home.notify_one();
        feeds.featured.notify_one();
        let Poll::Ready((value, failed)) = poll(home.as_mut()) else {
            panic!("Home outlived its answer");
        };
        assert!(value.is_none() && failed);
        assert_eq!(poll(featured.as_mut()), Poll::Ready(true));
        assert_eq!(published(&shared), [false; 4]);
    }

    #[test]
    fn visible_details_request_counts_and_old_account_answers_are_discarded() {
        let feeds = GatedFeeds::default();
        let shared = Mutex::new(Snapshot {
            player_counts_visible: true,
            ..Default::default()
        });
        let mut home = pin!(home_round(&feeds, &shared));
        let mut featured = pin!(featured_round(&feeds, &shared));
        assert!(poll(home.as_mut()).is_pending());
        assert!(poll(featured.as_mut()).is_pending());
        assert_eq!(feeds.counted.load(Ordering::SeqCst), 1);
        shared.lock().unwrap().retire_account_data();
        feeds.home.notify_one();
        feeds.featured.notify_one();
        assert!(poll(home.as_mut()).is_ready());
        assert!(poll(featured.as_mut()).is_ready());
        assert_eq!(published(&shared), [false; 4]);
    }

    #[test]
    fn uncounted_reply_cannot_erase_counts_after_details_open() {
        let feeds = GatedFeeds::default();
        let previous = vec![FeaturedServer {
            player_count: Some(12_345),
            ..Default::default()
        }];
        let shared = Mutex::new(Snapshot {
            featured: Some(previous.clone()),
            ..Default::default()
        });
        let mut featured = pin!(featured_round(&feeds, &shared));
        assert!(poll(featured.as_mut()).is_pending());
        shared.lock().unwrap().player_counts_visible = true;
        feeds.featured.notify_one();
        assert_eq!(poll(featured.as_mut()), Poll::Ready(false));
        assert_eq!(shared.lock().unwrap().featured, Some(previous));
    }

    #[test]
    fn realm_membership_rejects_a_catalog_reply_started_before_acceptance() {
        let feeds = GatedFeeds::default();
        let shared = Mutex::new(Snapshot::default());
        let mut catalog = pin!(catalog_round(&feeds, &shared, 0));
        assert!(poll(catalog.as_mut()).is_pending());
        {
            let mut snapshot = shared.lock().unwrap();
            snapshot.realm_catalog_revision += 1;
            snapshot.realms = Some(vec![
                serde_json::from_value(serde_json::json!({
                    "name": "Accepted Realm", "state": "OPEN", "target": "realm_id/7"
                }))
                .unwrap(),
            ]);
        }
        feeds.realms.notify_one();
        feeds.friends.notify_one();
        assert!(poll(catalog.as_mut()).is_ready());
        let snapshot = shared.lock().unwrap();
        assert_eq!(snapshot.realms.as_ref().unwrap()[0].target, "realm_id/7");
    }

    #[test]
    fn feed_changes_wake_immediately_and_shutdown_stops_waiting() {
        let (alive, stop) = crossbeam_channel::bounded(0);
        let (wake, changes) = crossbeam_channel::bounded(1);
        wake.try_send(()).unwrap();
        assert!(wait_changes(&stop, &changes, super::super::FEED_INTERVAL));
        drop(alive);
        assert!(!wait_changes(&stop, &changes, super::super::FEED_INTERVAL));
    }
}
