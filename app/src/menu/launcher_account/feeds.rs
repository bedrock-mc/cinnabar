//! One round of the menu's launcher feeds, requested concurrently.

use std::sync::Mutex;

use protocol::launcher_control::{self, BridgeError, FeaturedServer, Friend, Home, Realm};

use super::{Snapshot, auth_generation, publish, publish_account, settle};

/// The launcher requests the menu workers make: the core in production, fakes in tests.
pub(super) trait FeedSource {
    async fn home(&self) -> Result<Home, BridgeError>;
    async fn featured(&self) -> Result<Vec<FeaturedServer>, BridgeError>;
    async fn realms(&self) -> Result<Vec<Realm>, BridgeError>;
    async fn friends(&self) -> Result<Vec<Friend>, BridgeError>;
}

pub(super) struct CoreFeeds<'a>(pub(super) &'a std::path::Path);

impl FeedSource for CoreFeeds<'_> {
    async fn home(&self) -> Result<Home, BridgeError> {
        launcher_control::home(self.0).await
    }
    async fn featured(&self) -> Result<Vec<FeaturedServer>, BridgeError> {
        launcher_control::list_featured_servers(self.0).await
    }
    async fn realms(&self) -> Result<Vec<Realm>, BridgeError> {
        launcher_control::list_realms(self.0).await
    }
    async fn friends(&self) -> Result<Vec<Friend>, BridgeError> {
        launcher_control::list_friends(self.0).await
    }
}

/// Requests Home and featured servers together; each publishes as soon as it
/// arrives. Returns Home for impression reporting and whether any feed failed.
pub(super) async fn feed_round(
    source: &impl FeedSource,
    shared: &Mutex<Snapshot>,
) -> (Option<Home>, bool) {
    let generation = auth_generation(shared);
    let ((home, home_failed), featured_failed) = tokio::join!(
        async {
            let mut failed = false;
            let home = settle("home", source.home().await, &mut failed);
            if let Some(home) = &home {
                publish_account(shared, generation, |snapshot| {
                    snapshot.home = Some(home.clone())
                });
            }
            (home, failed)
        },
        async {
            let mut failed = false;
            if let Some(featured) = settle("featured servers", source.featured().await, &mut failed)
            {
                publish(shared, |snapshot| snapshot.featured = Some(featured));
            }
            failed
        },
    );
    (home, home_failed || featured_failed)
}

/// Requests the account's Realms and friends together; each publishes as soon as it arrives,
/// and a failed list keeps its last value.
pub(super) async fn catalog_round(
    source: &impl FeedSource,
    shared: &Mutex<Snapshot>,
    generation: u64,
) {
    tokio::join!(
        async {
            if let Ok(realms) = source.realms().await {
                publish_account(shared, generation, |snapshot| {
                    snapshot.realms = Some(realms)
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
        async fn featured(&self) -> Result<Vec<FeaturedServer>, BridgeError> {
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
    fn every_feed_request_starts_before_any_answers_and_each_publishes_on_arrival() {
        let feeds = GatedFeeds::default();
        let shared = Mutex::new(Snapshot::default());
        let mut round = pin!(feed_round(&feeds, &shared));
        assert!(poll(round.as_mut()).is_pending());
        assert_eq!(feeds.started.load(Ordering::SeqCst), 2);

        feeds.home.notify_one();
        assert!(poll(round.as_mut()).is_pending());
        assert_eq!(published(&shared), [true, false, false, false]);
        feeds.featured.notify_one();
        let Poll::Ready((home, failed)) = poll(round.as_mut()) else {
            panic!("the round outlived its last answer");
        };
        assert!(home.is_some() && !failed);

        let mut catalog = pin!(catalog_round(&feeds, &shared, 0));
        assert!(poll(catalog.as_mut()).is_pending());
        assert_eq!(feeds.started.load(Ordering::SeqCst), 4);
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
        let mut round = pin!(feed_round(&feeds, &shared));
        assert!(poll(round.as_mut()).is_pending());
        feeds.home.notify_one();
        feeds.featured.notify_one();
        let Poll::Ready((home, failed)) = poll(round.as_mut()) else {
            panic!("the round outlived its answers");
        };
        assert!(home.is_none() && failed);
        assert_eq!(published(&shared), [false; 4]);
    }
}
