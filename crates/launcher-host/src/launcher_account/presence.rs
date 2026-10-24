//! One account worker's explicit title-presence retirement proof.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::sync::Notify;

const ACTIVE: u8 = 0;
const RETIRING: u8 = 1;
const COMPLETE: u8 = 2;
const UNCONFIRMED: u8 = 3;

/// Whether this lifetime finished its only cleanup before another publisher may start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlePresenceRetirement {
    /// No native write began, or removal succeeded without an earlier uncertain update.
    Complete,
    /// Cleanup or worker termination was uncertain; replacement remains blocked.
    Unconfirmed,
}

struct Shared {
    state: AtomicU8,
    worker_finished: AtomicBool,
    changed: Notify,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    original: std::sync::Mutex<Option<OriginalAccount>>,
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
type OriginalAccount = (
    Arc<crate::native_account::NativeAccount>,
    crate::native_account::presence::PresenceScope,
);

/// Keeps one existing event worker's cleanup outcome alive after its resource is detached.
/// Retirement is one-way; an uncertain lifetime cannot be reset or reused.
#[derive(Clone)]
pub struct TitlePresenceLifetime(Arc<Shared>);

impl TitlePresenceLifetime {
    /// Creates the lifetime before its sole event worker is started.
    pub(super) fn new() -> Self {
        Self(Arc::new(Shared {
            state: AtomicU8::new(ACTIVE),
            worker_finished: AtomicBool::new(false),
            changed: Notify::new(),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            original: std::sync::Mutex::new(None),
        }))
    }

    /// Requests bounded cleanup without blocking the caller or cancelling an accepted write.
    pub fn retire(&self) {
        let _ =
            self.0
                .state
                .compare_exchange(ACTIVE, RETIRING, Ordering::AcqRel, Ordering::Acquire);
        self.wake();
    }

    /// Reports whether this worker may no longer start another update.
    pub fn retirement_requested(&self) -> bool {
        self.0.state.load(Ordering::Acquire) != ACTIVE
    }

    /// Returns only a finished outcome; callback completion is never substituted for it.
    pub fn retirement(&self) -> Option<TitlePresenceRetirement> {
        match self.0.state.load(Ordering::Acquire) {
            COMPLETE => Some(TitlePresenceRetirement::Complete),
            UNCONFIRMED => Some(TitlePresenceRetirement::Unconfirmed),
            _ => None,
        }
    }

    /// The original event owner has dropped its control, publisher and Product futures.
    /// Presence cleanup alone does not prove that this enclosing worker has finished.
    pub fn worker_finished(&self) -> bool {
        self.0.worker_finished.load(Ordering::Acquire)
    }

    /// Wakes both the coalesced-state reader and the control lifetime without storing payloads.
    pub(super) fn wake(&self) {
        self.0.changed.notify_waiters();
    }

    /// Registers for a state or retirement wake; callers recheck their condition after registration.
    pub(super) fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.0.changed.notified()
    }

    /// Waits for a retirement request without losing a concurrent notification.
    pub(super) async fn retiring(&self) {
        loop {
            let changed = self.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.retirement_requested() {
                return;
            }
            changed.await;
        }
    }

    /// Waits for the sole cleanup's result before an explicit sign-out may mutate credentials.
    pub(super) async fn finished(&self) {
        loop {
            let changed = self.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.retirement().is_some() {
                return;
            }
            changed.await;
        }
    }

    /// Publishes the terminal outcome only after the actual service future and leased owner retired.
    pub(super) fn complete(&self, confirmed: bool) {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if confirmed {
            let _ = self
                .0
                .original
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
        }
        self.0.state.store(
            if confirmed { COMPLETE } else { UNCONFIRMED },
            Ordering::Release,
        );
        self.wake();
    }

    /// Keeps one original account/generation cleanup obligation when removal or worker exit is uncertain.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(super) fn bind_original(
        &self,
        account: Arc<crate::native_account::NativeAccount>,
        scope: crate::native_account::presence::PresenceScope,
    ) {
        *self.0.original.lock().unwrap_or_else(|p| p.into_inner()) = Some((account, scope));
    }

    /// Marks a panic, missing runtime or unexpectedly ended worker as an uncertain retirement.
    pub(super) fn worker_guard(&self) -> WorkerGuard {
        WorkerGuard(self.clone())
    }
}

pub(super) struct WorkerGuard(TitlePresenceLifetime);
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if self.0.retirement().is_none() {
            self.0.complete(false);
        }
        self.0.0.worker_finished.store(true, Ordering::Release);
        self.0.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_worker_end_holds_replacement_without_reset() {
        let lifetime = TitlePresenceLifetime::new();
        let worker = lifetime.worker_guard();
        lifetime.retire();
        assert_eq!(lifetime.retirement(), None);
        drop(worker);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Unconfirmed)
        );
        lifetime.retire();
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Unconfirmed)
        );
    }

    /// A completed publisher cannot substitute for the enclosing original event owner's exit.
    #[test]
    fn completed_presence_still_waits_for_the_original_worker_guard() {
        let lifetime = TitlePresenceLifetime::new();
        let worker = lifetime.worker_guard();
        lifetime.retire();
        lifetime.complete(true);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
        assert!(!lifetime.worker_finished());
        drop(worker);
        assert!(lifetime.worker_finished());
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
    }
}
