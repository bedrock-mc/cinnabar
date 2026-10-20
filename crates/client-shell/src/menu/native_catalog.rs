//! The opt-in Realms and friend-world catalog has one selected account and one caller-driven request owner.

use super::*;
use launcher::menu::CATALOG_REFRESH_INTERVAL;
use launcher_host::native_account::{
    CatalogCards, CatalogListing, CatalogWorker, NativeAccount, NativeFriendSelection,
    NativeFriendWorld,
};
use std::{sync::Arc, time::Instant};

const FAILURE_MESSAGE: &str = "Realms and friends: Refresh to try again.";
// Enable only after the application owner passes admission, cancellation and cleanup qualification.
const NATIVE_FRIEND_JOINS_QUALIFIED: bool = false;

#[derive(Default)]
pub(super) struct Controller {
    selected: Option<Selection>,
    request: Option<Request>,
    retiring: Option<CatalogWorker>,
    epoch: u64,
    next_refresh: Option<Instant>,
    friend_worlds: Vec<NativeFriendWorld>,
    join: Option<NativeFriendSelection>,
}

struct Selection {
    path: PathBuf,
    identity: Option<String>,
    account: Arc<NativeAccount>,
}
struct Request {
    epoch: u64,
    worker: CatalogWorker,
    /// Keeps this read's original rows while the selected account is temporarily checked out.
    completed: Option<CatalogListing>,
}

impl std::fmt::Debug for Controller {
    /// Contains no account or network identifiers in diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeCatalog")
    }
}

impl Controller {
    /// Retires an unconsumed service selection when a different session intent wins.
    pub(super) fn cancel_join(&mut self) {
        self.join = None;
    }
    /// Cancels the prior request before allowing another operation on the same SDK owner.
    pub(super) fn pause(&mut self) {
        self.reap_retiring();
        if let Some(request) = self.request.take() {
            self.retire_worker(request.worker);
        }
    }

    /// Releases only a worker whose actual thread and protected destructors have finished.
    fn reap_retiring(&mut self) {
        if self
            .retiring
            .as_ref()
            .is_some_and(CatalogWorker::is_finished)
        {
            self.retiring = None;
        }
    }

    /// Keeps the one canceled worker reachable; admission cannot refill until it actually retires.
    fn retire_worker(&mut self, mut worker: CatalogWorker) {
        self.reap_retiring();
        assert!(
            self.retiring.is_none(),
            "a retiring catalog retains the sole request slot"
        );
        worker.cancel();
        if !worker.is_finished() {
            self.retiring = Some(worker);
        }
    }

    /// Retires pending results and requests a fresh catalog on the next eligible frame.
    pub(super) fn refresh(&mut self) {
        self.pause();
        self.epoch = self.epoch.wrapping_add(1);
        self.next_refresh = None;
    }

    /// Releases the selected owner on account mutations; the old worker may only finish protected cleanup.
    fn retire(&mut self) {
        self.refresh();
        self.selected = None;
        self.friend_worlds.clear();
        self.join = None;
    }

    /// Borrows only the original selected owner, without canceling or restarting its catalog read.
    pub(super) fn selected_account(
        &self,
        path: &std::path::Path,
        identity: Option<&str>,
    ) -> Option<&Arc<NativeAccount>> {
        self.selected
            .as_ref()
            .filter(|selected| selected.path == path && selected.identity.as_deref() == identity)
            .map(|selected| &selected.account)
    }

    /// Reuses one account across listing, Realm resolution and final key mint without filesystem access.
    fn account(
        &mut self,
        path: PathBuf,
        identity: Option<String>,
        device: PathBuf,
    ) -> (Arc<NativeAccount>, bool) {
        let changed = self
            .selected
            .as_ref()
            .is_none_or(|selection| selection.path != path || selection.identity != identity);
        if changed {
            self.retire();
            self.selected = Some(Selection {
                account: Arc::new(NativeAccount::new(
                    launcher::accounts::AccountStore::new(path.clone()),
                    device,
                    identity.clone(),
                )),
                path,
                identity,
            });
        }
        (
            Arc::clone(&self.selected.as_ref().expect("selected account").account),
            changed,
        )
    }
}

impl MenuRuntime {
    /// Captures the selected opaque owner; the qualification gate keeps this action unavailable meanwhile.
    pub(super) fn request_native_friend(&mut self, index: usize) {
        if !NATIVE_FRIEND_JOINS_QUALIFIED {
            self.message = Some("Native friend-world joining is not available yet.".into());
            return;
        }
        let Some(selection) = self.native_friend_selection(index) else {
            self.message =
                Some("That friend world is no longer available. Refresh to try again.".into());
            return;
        };
        self.queue_native_friend(selection);
    }

    /// Sends the original selected owner through the same menu-to-session handoff.
    fn queue_native_friend(&mut self, selection: NativeFriendSelection) {
        self.remember_session_origin();
        self.stop_catalog();
        self.intents.join = None;
        self.intents.native_world = None;
        self.native_catalog.join = Some(selection);
        self.show_connecting();
    }

    /// Moves the original selected service/account owner to the application's session controller.
    pub fn take_native_friend_intent(&mut self) -> Option<NativeFriendSelection> {
        if self.account_change_pending() {
            self.native_catalog.cancel_join();
            return None;
        }
        self.native_catalog.join.take()
    }
    /// Borrows the selected row's opaque service/account owner; display text never resolves a handle.
    fn native_friend_selection(&mut self, index: usize) -> Option<NativeFriendSelection> {
        if !self.native_sign_in || self.account_change_pending() {
            return None;
        }
        let selected = self.native_catalog.selected.as_ref()?;
        if selected.identity.as_deref() != self.selected_account_id()
            || Some(&selected.path) != self.launcher_auth_cache().as_ref()
        {
            return None;
        }
        self.native_catalog.pause();
        self.native_catalog.friend_worlds.get(index)?.selection()
    }
    /// Takes the catalog's concrete account for a join after cancelling its request.
    /// The acquisition future waits off-frame until the old Operation finishes durable cleanup.
    pub fn native_account_for(&mut self, path: PathBuf) -> Arc<NativeAccount> {
        self.native_catalog.pause();
        let identity = self.selected_account_id().map(str::to_owned);
        let device = self.layout.device_profile_file();
        let (account, changed) = self.native_catalog.account(path, identity, device);
        if changed {
            self.realms.replace(Vec::new());
            self.friends.replace(Vec::new());
            self.feeds.selected_realm = None;
        }
        account
    }

    /// Replaces successful sections independently and retains rows when one service fails.
    fn apply_native_catalog(&mut self, cards: CatalogCards) {
        let mut failed = false;
        match cards.realms {
            Ok(cards) => {
                let selected = self
                    .feeds
                    .selected_realm
                    .and_then(|index| self.realms.get(index))
                    .map(|realm| realm.target.clone());
                self.realms.replace(cards);
                self.feeds.selected_realm = selected
                    .and_then(|target| self.realms.iter().position(|realm| realm.target == target));
            }
            Err(_) => failed = true,
        }
        match cards.friends {
            Ok(worlds) => {
                self.friends
                    .replace(worlds.iter().map(|world| world.card().clone()).collect());
                self.native_catalog.friend_worlds = worlds;
                self.native_catalog.epoch = self.native_catalog.epoch.wrapping_add(1);
            }
            Err(_) => failed = true,
        }
        if failed {
            self.catalog_message = Some(FAILURE_MESSAGE.into());
        } else if self.catalog_message.as_deref() == Some(FAILURE_MESSAGE) {
            self.catalog_message = None;
        }
    }

    /// Advances listing after account observations; Go catalog/control results never own this native feed.
    pub(super) fn poll_native_catalog(&mut self, now: Instant) {
        self.native_catalog.reap_retiring();
        if !self.native_sign_in {
            return;
        }
        let Some(path) = self
            .launcher_auth_cache()
            .filter(|_| !self.account_change_pending())
        else {
            if self.native_catalog.selected.is_some()
                || !self.realms.is_empty()
                || !self.friends.is_empty()
            {
                self.native_catalog.retire();
                self.realms.replace(Vec::new());
                self.friends.replace(Vec::new());
                self.feeds.selected_realm = None;
            }
            return;
        };
        if !self.visible || self.is_connecting() {
            self.native_catalog.pause();
            return;
        }
        let identity = self.selected_account_id().map(str::to_owned);
        let device = self.layout.device_profile_file();
        let (account, changed) = self.native_catalog.account(path, identity, device);
        if changed {
            self.realms.replace(Vec::new());
            self.friends.replace(Vec::new());
            self.feeds.selected_realm = None;
        }
        if let Some(mut request) = self.native_catalog.request.take() {
            let result = request
                .completed
                .take()
                .map(Ok)
                .or_else(|| request.worker.poll());
            let Some(result) = result else {
                self.native_catalog.request = Some(request);
                return;
            };
            let epoch = request.epoch;
            let cards = if epoch == self.native_catalog.epoch {
                match result {
                    Ok(listing) => match listing.into_current_cards(&account) {
                        Ok(cards) => cards,
                        Err(listing) => {
                            request.completed = Some(listing);
                            self.native_catalog.request = Some(request);
                            return;
                        }
                    },
                    Err(_) => None,
                }
            } else {
                None
            };
            self.native_catalog.retire_worker(request.worker);
            if epoch == self.native_catalog.epoch {
                match cards {
                    Some(cards) => self.apply_native_catalog(cards),
                    None => self.catalog_message = Some(FAILURE_MESSAGE.into()),
                }
            }
            self.native_catalog.next_refresh = Some(now + CATALOG_REFRESH_INTERVAL);
        }
        if self.native_catalog.request.is_none()
            && self.native_catalog.retiring.is_none()
            && self
                .native_catalog
                .next_refresh
                .is_none_or(|next| now >= next)
        {
            match self
                .io_execution()
                .and_then(|(runtime, tasks)| CatalogWorker::start(account, runtime, tasks))
            {
                Ok(worker) => {
                    self.native_catalog.request = Some(Request {
                        epoch: self.native_catalog.epoch,
                        worker,
                        completed: None,
                    })
                }
                Err(_) => {
                    self.catalog_message = Some(FAILURE_MESSAGE.into());
                    self.native_catalog.next_refresh = Some(now + CATALOG_REFRESH_INTERVAL);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "developer-control")]
mod control;
