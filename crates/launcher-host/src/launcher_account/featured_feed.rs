//! Featured cache/count progress on the existing account worker and application reactor.

use super::*;
use launcher::menu::view::{MenuGameCard, MenuServerCard, ServerDetails};
use tokio::sync::watch;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native;

#[derive(Clone, Default)]
pub(super) struct Source(pub(super) Option<Arc<crate::native_account::NativeAccount>>);

impl Source {
    /// Compares the captured concrete account without acquiring credentials on a frame.
    fn same_account(&self, current: &Self) -> bool {
        match (&self.0, &current.0) {
            (Some(first), Some(second)) => Arc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wake {
    Account,
    Stop,
}

/// Coalesces account retirement without overwriting a terminal worker stop.
pub(super) fn retire(sender: &watch::Sender<Wake>) {
    sender.send_if_modified(|wake| *wake != Wake::Stop);
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
struct Selection {
    generation: u64,
    source: Source,
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl Selection {
    /// Captures publication identity before any actual credential/service work begins.
    fn capture(snapshot: &Snapshot) -> Self {
        Self {
            generation: snapshot.auth_generation,
            source: snapshot.featured_source.clone(),
        }
    }

    /// Account replacement retires the old response independently of its service success.
    fn current(&self, snapshot: &Snapshot) -> bool {
        self.generation == snapshot.auth_generation
            && self.source.same_account(&snapshot.featured_source)
    }
}

/// Keeps one actual pending read through unrelated coalesced notifications.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
async fn wait<T>(
    future: impl std::future::Future<Output = T>,
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    changed: &mut watch::Receiver<Wake>,
) -> Option<T> {
    tokio::pin!(future);
    loop {
        if *changed.borrow_and_update() == Wake::Stop
            || !selected.current(&shared.lock().unwrap_or_else(|p| p.into_inner()))
        {
            return None;
        }
        tokio::select! { biased;
            wake = changed.changed() => if wake.is_err() { return None; },
            result = &mut future => return Some(result),
        }
    }
}

#[derive(Default)]
pub(super) struct Feed {
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native: native::Cache,
}

impl Feed {
    /// Reads only the captured native account; an absent owner remains unavailable.
    pub(super) async fn round(
        &mut self,
        art: Option<&artwork::FeedArt>,
        shared: &Mutex<Snapshot>,
        changed: &mut watch::Receiver<Wake>,
    ) -> bool {
        let selected = Selection::capture(&shared.lock().unwrap_or_else(|p| p.into_inner()));
        self.native
            .round(&selected.source.0, art, shared, &selected, changed)
            .await
    }
}

/// Takes the original cards and selections under one snapshot lock.
pub(super) fn take_rows(
    snapshot: &mut Snapshot,
) -> Option<Vec<crate::account_control::FeaturedRow>> {
    let servers = snapshot.featured.take()?;
    Some(
        servers
            .iter()
            .enumerate()
            .map(|(index, server)| {
                let row = card(server).into();
                #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                let row = crate::account_control::FeaturedRow {
                    gathering: snapshot.featured_gatherings.get(index).cloned().flatten(),
                    ..row
                };
                #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
                let _ = index;
                row
            })
            .collect(),
    )
}

/// Splits the acquired featured entry into its menu card and selected details.
pub(super) fn card(server: &FeaturedServer) -> (MenuServerCard, ServerDetails) {
    let card = MenuServerCard {
        name: server.name.clone(),
        address: server.address.clone(),
        caption: server.caption.clone(),
        image_path: server.logo.path.clone(),
        icon: None,
    };
    let details = ServerDetails {
        group: server.group.clone(),
        player_count: server.player_count,
        description: server.description.clone(),
        banner: server.background.path.clone(),
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
        logo_url: server.logo.url.clone(),
    };
    (card, details)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absent selected owner completes locally without publishing unrelated account data.
    #[tokio::test]
    async fn missing_native_featured_owner_remains_unavailable() {
        let shared = Mutex::new(Snapshot::native(None, ""));
        let (_wake, mut changed) = watch::channel(Wake::Account);
        assert!(Feed::default().round(None, &shared, &mut changed).await);
        assert!(shared.lock().unwrap().featured.is_none());
    }
}
