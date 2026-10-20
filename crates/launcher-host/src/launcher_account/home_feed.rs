//! The existing Home worker selects one account source and retains native refresh state.

use super::*;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
use std::future::Future;
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
    /// Compares the selected owner without acquiring credentials or blocking a frame.
    fn same_account(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Core, Self::Core) => true,
            (Self::Native(first), Self::Native(second)) => match (first, second) {
                (Some(first), Some(second)) => Arc::ptr_eq(first, second),
                (None, None) => true,
                _ => false,
            },
            _ => false,
        }
    }
}

#[derive(Clone, Default)]
pub(super) enum Scope {
    #[default]
    Core,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(crate::native_account::NativeHomeScope),
}

impl Scope {
    /// Only native service-session replacement resets optimistic marketing interactions.
    pub(super) fn marker(&self) -> Option<String> {
        match self {
            Self::Core => None,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(scope) => Some(scope.marker()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wake {
    Account,
    Stop,
}

/// Coalesces account changes, preserving a terminal stop for both existing workers.
pub(super) fn retire(sender: &watch::Sender<Wake>) {
    sender.send_if_modified(|wake| *wake != Wake::Stop);
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub(super) struct Selection {
    generation: u64,
    source: Source,
    language: String,
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl Selection {
    /// Captures the same UI generation and concrete account before asynchronous work.
    pub(super) fn capture(snapshot: &Snapshot) -> Self {
        Self {
            generation: snapshot.auth_generation,
            source: snapshot.home_source.clone(),
            language: snapshot.home_language.clone(),
        }
    }

    /// Retires publication and undispatched reports before changing account credentials.
    fn current(&self, snapshot: &Snapshot) -> bool {
        self.generation == snapshot.auth_generation
            && self.source.same_account(&snapshot.home_source)
            && self.language == snapshot.home_language
    }
}

/// Keeps the same actual read/preparation future through unrelated bounded watch notifications.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
async fn wait<T>(
    phase: impl Future<Output = T>,
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    changed: &mut watch::Receiver<Wake>,
) -> Option<T> {
    tokio::pin!(phase);
    loop {
        if *changed.borrow_and_update() == Wake::Stop
            || !selected.current(&shared.lock().unwrap_or_else(|p| p.into_inner()))
        {
            return None;
        }
        tokio::select! { biased;
            wake = changed.changed() => if wake.is_err() { return None; },
            result = &mut phase => return Some(result),
        }
    }
}

/// Retains native cache state in the original Home worker, without a new task or runtime.
#[derive(Default)]
pub(super) struct Feed {
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native: native::Cache,
}

impl Feed {
    /// Leaves the original Core read/report path intact; a missing native owner never uses Core.
    pub(super) async fn round(
        &mut self,
        source: &CoreFeeds<'_>,
        shared: &Mutex<Snapshot>,
        changed: &mut watch::Receiver<Wake>,
    ) -> (Option<Home>, bool) {
        #[cfg(all(
            feature = "test-support",
            feature = "native-networking",
            not(target_arch = "wasm32")
        ))]
        {
            let probe = shared.lock().unwrap().retirement_probe.clone();
            if let Some(probe) = probe {
                let selected = Selection::capture(&shared.lock().unwrap());
                let _ = wait(probe.read(), shared, &selected, changed).await;
                return (None, false);
            }
        }
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            let selected = Selection::capture(&shared.lock().unwrap_or_else(|p| p.into_inner()));
            if let Source::Native(account) = &selected.source {
                return self
                    .native
                    .round(account, source.art, shared, &selected, changed)
                    .await;
            }
        }
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            self.native = native::Cache::default();
        }
        let _ = changed;
        super::feeds::home_round(source, shared).await
    }
}

/// Separates local retirement from a successful remote report and a retryable failure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReportOutcome {
    Complete,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Retired,
    Retry,
}

/// One original interaction, carrying the actual delivered service scope instead of a later feed.
pub(super) struct Report {
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    selected: Selection,
    scope: Scope,
    event: MessageEvent,
}

impl Report {
    /// Captures the displayed Home scope at enqueue, before a replacement result is consumed.
    pub(super) fn capture(snapshot: &Snapshot, event: MessageEvent) -> Self {
        Self {
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            selected: Selection::capture(snapshot),
            scope: snapshot.delivered_home_scope.clone(),
            event,
        }
    }

    /// Uses the original ordered reporter; retirement ends only this obsolete request's retries.
    pub(super) async fn send(
        &self,
        socket_dir: Option<&std::path::Path>,
        shared: &Mutex<Snapshot>,
        changed: &mut watch::Receiver<Wake>,
    ) -> ReportOutcome {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Source::Native(account) = &self.selected.source {
            if *changed.borrow_and_update() == Wake::Stop
                || !self
                    .selected
                    .current(&shared.lock().unwrap_or_else(|p| p.into_inner()))
            {
                return ReportOutcome::Retired;
            }
            let Some(account) = account else {
                return ReportOutcome::Retired;
            };
            let Scope::Native(scope) = &self.scope else {
                return ReportOutcome::Retired;
            };
            return native::report(account, scope, &self.event, shared, &self.selected, changed)
                .await;
        }
        // The existing Core dispatcher retains its own account and report semantics.
        let _ = (shared, changed);
        let _ = &self.scope;
        let Ok(socket_dir) = super::workers::require_core_directory(socket_dir) else {
            return ReportOutcome::Retry;
        };
        match bridge::report_message_event(socket_dir, &self.event).await {
            Ok(()) => ReportOutcome::Complete,
            Err(error) => {
                tracing::warn!(%error, "inbox event failed; retaining for retry");
                ReportOutcome::Retry
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
