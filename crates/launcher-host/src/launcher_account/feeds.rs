//! Independent native Home and featured workers on the application's original reactor.
use super::{FEED_INTERVAL, FEED_RETRY, Snapshot, artwork::FeedArt};
use crossbeam_channel::Receiver;
use std::sync::Mutex;

/// Refreshes featured rows independently of Home and its ordered reports.
pub(super) fn poll_featured(
    art: &FeedArt,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    changes: &Receiver<()>,
    mut changed: tokio::sync::watch::Receiver<super::featured_feed::Wake>,
    runtime: &tokio::runtime::Handle,
) {
    let mut feed = super::featured_feed::Feed::default();
    loop {
        while changes.try_recv().is_ok() {}
        let failed = runtime.block_on(feed.round(Some(art), shared, &mut changed));
        if !wait_changes(
            stop,
            changes,
            if failed { FEED_RETRY } else { FEED_INTERVAL },
        ) {
            return;
        }
    }
}

/// Retains the original native Home cache and its scoped impression owner.
pub(super) fn poll_home(
    art: &FeedArt,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    changes: &Receiver<()>,
    mut changed: tokio::sync::watch::Receiver<super::home_feed::Wake>,
    runtime: &tokio::runtime::Handle,
) {
    let mut feed = super::home_feed::Feed::default();
    loop {
        while changes.try_recv().is_ok() {}
        let (_, failed) = runtime.block_on(feed.round(Some(art), shared, &mut changed));
        if !wait_changes(
            stop,
            changes,
            if failed { FEED_RETRY } else { FEED_INTERVAL },
        ) {
            return;
        }
    }
}

/// Wakes for screen/account changes, the next bounded refresh, or original shutdown.
pub(super) fn wait_changes(
    stop: &Receiver<()>,
    changes: &Receiver<()>,
    interval: std::time::Duration,
) -> bool {
    crossbeam_channel::select! {
        recv(stop) -> _ => false,
        recv(changes) -> result => result.is_ok(),
        default(interval) => true,
    }
}
