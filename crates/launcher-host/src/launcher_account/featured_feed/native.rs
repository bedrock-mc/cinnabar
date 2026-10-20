//! Native featured rows retain their original account selection through artwork and populations.

use super::*;
use crate::native_account::{FeaturedEntries, FeaturedIdentity, NativeAccount};
use tokio::time::Instant;

const REFRESH: Duration = Duration::from_secs(bridge::contract::HOME_FEED_REFRESH_SECONDS as u64);
const CACHE_TTL: Duration =
    Duration::from_secs(bridge::contract::FEATURED_FEED_CACHE_SECONDS as u64);

#[derive(Default)]
pub(super) struct Cache {
    generation: Option<u64>,
    account: Option<Arc<NativeAccount>>,
    entries: Option<FeaturedEntries>,
    identity: Option<FeaturedIdentity>,
    attempted: Option<Instant>,
    fetched: Option<Instant>,
}

impl Cache {
    /// Reuses the original five-minute layout interval and same-account stale rows on failure.
    pub(super) async fn round(
        &mut self,
        account: &Option<Arc<NativeAccount>>,
        art: Option<&artwork::FeedArt>,
        shared: &Mutex<Snapshot>,
        selected: &Selection,
        changed: &mut watch::Receiver<Wake>,
    ) -> bool {
        let Some(account) = account else {
            *self = Self::default();
            return true;
        };
        if self.generation != Some(selected.generation)
            || self
                .account
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, account))
        {
            *self = Self {
                generation: Some(selected.generation),
                account: Some(Arc::clone(account)),
                ..Default::default()
            };
        }
        let now = Instant::now();
        let deadline = now + REFRESH;
        if self.entries.is_none()
            || (self
                .fetched
                .is_none_or(|at| now.duration_since(at) >= CACHE_TTL)
                && self
                    .attempted
                    .is_none_or(|at| now.duration_since(at) >= CACHE_TTL))
        {
            self.attempted = Some(now);
            let Some(result) = wait(
                Arc::clone(account).read_featured(deadline),
                shared,
                selected,
                changed,
            )
            .await
            else {
                return true;
            };
            match result {
                Ok(mut loaded) => {
                    if !loaded.current() {
                        return true;
                    }
                    if let Some(art) = art {
                        art.fill_cached(artwork::featured_slots(&mut loaded.entries.servers));
                    }
                    let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                    if *changed.borrow() == Wake::Stop || !selected.current(&snapshot) {
                        return true;
                    }
                    self.entries = Some(loaded.entries.clone());
                    self.identity = Some(loaded.identity.clone());
                    self.fetched = Some(Instant::now());
                    // The completed SDK Operation remains held through the atomic row/selection publish.
                    if !snapshot.player_counts_visible {
                        publish(&mut snapshot, &loaded.entries);
                    }
                }
                Err(_) => tracing::warn!("native featured refresh unavailable"),
            }
        }
        let (Some(entries), Some(identity)) = (&mut self.entries, &self.identity) else {
            return true;
        };
        let visible = shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .player_counts_visible;
        if !visible {
            entries.apply_counts(&[]);
        }
        if visible && entries.gatherings.iter().any(Option::is_some) {
            let Some(result) = wait(
                Arc::clone(account).read_featured_counts(identity, deadline),
                shared,
                selected,
                changed,
            )
            .await
            else {
                return true;
            };
            if let Ok(loaded) = result {
                if !loaded.current() {
                    return true;
                }
                entries.apply_counts(&loaded.counts);
                if let Some(art) = art {
                    art.fill_cached(artwork::featured_slots(&mut entries.servers));
                }
                let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                if *changed.borrow() != Wake::Stop && selected.current(&snapshot) {
                    publish(&mut snapshot, entries);
                }
                return false;
            }
        }
        // A busy lease delays cached publication under the same cutoff; it never invalidates the layout.
        if !matches!(
            wait(
                tokio::time::timeout_at(deadline, identity.wait_current()),
                shared,
                selected,
                changed
            )
            .await,
            Some(Ok(true))
        ) {
            return false;
        }
        if let Some(art) = art {
            art.fill_cached(artwork::featured_slots(&mut entries.servers));
        }
        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
        if *changed.borrow() != Wake::Stop && selected.current(&snapshot) {
            publish(&mut snapshot, entries);
        }
        false
    }
}

/// Cards and their original opaque selections cross the existing worker boundary together.
fn publish(snapshot: &mut Snapshot, entries: &FeaturedEntries) {
    snapshot.featured = Some(entries.servers.clone());
    snapshot.featured_gatherings = entries.gatherings.clone();
}
