//! Featured cache/count progress on the existing account worker and application reactor.

use super::*;
use launcher::menu::view::{MenuGameCard, MenuServerCard, ServerDetails};
use tokio::sync::watch;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[derive(Clone, Default)]
pub(super) enum Source {
    #[default]
    Core,
    Native(Option<Arc<crate::native_account::NativeAccount>>),
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl Source {
    /// Compares the selected concrete owner without opening an account lease on a frame.
    fn same_account(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Core, Self::Core) | (Self::Native(None), Self::Native(None)) => true,
            (Self::Native(Some(first)), Self::Native(Some(second))) => Arc::ptr_eq(first, second),
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
    /// Selects the existing Core source or the original native account; absent native stays absent.
    pub(super) async fn round(
        &mut self,
        source: &impl super::feeds::FeedSource,
        art: Option<&artwork::FeedArt>,
        shared: &Mutex<Snapshot>,
        changed: &mut watch::Receiver<Wake>,
    ) -> bool {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            let selected = Selection::capture(&shared.lock().unwrap_or_else(|p| p.into_inner()));
            if let Source::Native(account) = &selected.source {
                return self
                    .native
                    .round(account, art, shared, &selected, changed)
                    .await;
            }
            self.native = native::Cache::default();
        }
        let _ = (changed, art);
        super::feeds::featured_round(source, shared).await
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

/// Splits the core's featured entry into its menu card and selected details.
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
