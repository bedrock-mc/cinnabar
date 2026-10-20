//! Account-lifetime publication on the existing event worker's runtime.
use super::*;
use crate::native_account::{NativeAccount, presence::PresenceScope};
use std::io;

const REQUEST_BUDGET: Duration = Duration::from_secs(10);
const CLEANUP_BUDGET: Duration = Duration::from_secs(5);
const RETRY_INTERVAL: Duration = Duration::from_secs(15);
const FALLBACK_HEARTBEAT: Duration = Duration::from_secs(5 * 60);

/// The selected account's three leased service phases; no runtime or retry policy lives here.
trait Service {
    type Scope;
    /// Captures the current original generation before any title publication.
    async fn bind(&self) -> io::Result<Self::Scope>;
    /// Commits one latest activity without replacing its actual request on unrelated wakes.
    /// Sets caller-retained evidence only at the final signed HTTP dispatch boundary.
    async fn update(
        &self,
        scope: &Self::Scope,
        state: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>>;
    /// Attempts only the original account's cleanup before authority can be released.
    async fn remove(&self, scope: &Self::Scope) -> io::Result<()>;
}

struct NativeService<'a> {
    account: Arc<NativeAccount>,
    lifetime: &'a TitlePresenceLifetime,
}

impl Service for NativeService<'_> {
    type Scope = PresenceScope;
    async fn bind(&self) -> io::Result<PresenceScope> {
        let scope = Arc::clone(&self.account).bind_title_presence().await?;
        self.lifetime
            .bind_original(Arc::clone(&self.account), scope.clone());
        Ok(scope)
    }
    async fn update(
        &self,
        scope: &PresenceScope,
        state: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>> {
        Arc::clone(&self.account)
            .update_title_presence(scope, state, dispatched)
            .await
    }
    async fn remove(&self, scope: &PresenceScope) -> io::Result<()> {
        Arc::clone(&self.account).remove_title_presence(scope).await
    }
}

/// Retains one actual acquisition/update/removal future across all unrelated control wakes.
pub(super) async fn run(shared: &Mutex<Snapshot>, lifetime: &TitlePresenceLifetime) {
    #[cfg(feature = "test-support")]
    {
        let probe = shared.lock().unwrap().retirement_probe.clone();
        if let Some(probe) = probe {
            publish(&probe, shared, lifetime).await;
            return;
        }
    }
    let account = shared
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .native_presence_account
        .clone();
    let Some(account) = account else {
        lifetime.retiring().await;
        lifetime.complete(true);
        return;
    };
    publish(&NativeService { account, lifetime }, shared, lifetime).await;
}

/// Serializes one retained service phase, coalesced state and one original-account cleanup.
async fn publish(
    service: &impl Service,
    shared: &Mutex<Snapshot>,
    lifetime: &TitlePresenceLifetime,
) {
    let Some(scope) = bind(service, lifetime).await else {
        lifetime.complete(true); // Binding never publishes title presence.
        return;
    };
    let mut wrote = false;
    // The service supplies no ordering/reconciliation proof for a lost POST response.
    // Keep that possible remote write through every later update and removal.
    let mut uncertain_write = false;
    while !lifetime.retirement_requested() {
        let state = latest(shared);
        // Keep this actual future pinned by the enclosing async lifetime, even when state changes.
        // Retirement waits for its original finite budget before starting the sole cleanup.
        let mut dispatched = false;
        let result = bounded_until(
            tokio::time::Instant::now() + REQUEST_BUDGET,
            service.update(&scope, &state, &mut dispatched),
        )
        .await;
        wrote |= dispatched;
        let delay = match result {
            Ok(Ok(heartbeat)) => heartbeat
                .filter(|d| !d.is_zero())
                .unwrap_or(FALLBACK_HEARTBEAT),
            Ok(Err(error)) => {
                uncertain_write |= dispatched;
                tracing::warn!("native title presence update failed");
                if error.kind() == io::ErrorKind::PermissionDenied {
                    lifetime.retire();
                }
                RETRY_INTERVAL
            }
            Err(_) => {
                uncertain_write |= dispatched;
                tracing::warn!("native title presence update timed out");
                RETRY_INTERVAL
            }
        };
        if lifetime.retirement_requested() {
            break;
        }
        let now = tokio::time::Instant::now();
        let until = now.checked_add(delay).unwrap_or(now + FALLBACK_HEARTBEAT);
        wait_latest(shared, lifetime, &state, until).await;
    }
    let removed = !wrote
        || matches!(
            bounded_until(
                tokio::time::Instant::now() + CLEANUP_BUDGET,
                service.remove(&scope)
            )
            .await,
            Ok(Ok(()))
        );
    let confirmed = removed && !uncertain_write;
    if !confirmed {
        tracing::warn!("native title presence cleanup unconfirmed; replacement remains blocked");
    }
    lifetime.complete(confirmed);
}

/// Acquires one original identity; retirement cancels only this pre-publication phase.
async fn bind<S: Service>(service: &S, lifetime: &TitlePresenceLifetime) -> Option<S::Scope> {
    loop {
        let acquisition =
            bounded_until(tokio::time::Instant::now() + REQUEST_BUDGET, service.bind());
        tokio::select! {
            biased;
            _ = lifetime.retiring() => return None,
            result = acquisition => {
                if let Ok(Ok(scope)) = result { return Some(scope); }
                tracing::warn!("native title presence account unavailable");
            }
        }
        tokio::select! {
            _ = lifetime.retiring() => return None,
            _ = tokio::time::sleep(RETRY_INTERVAL) => {},
        }
    }
}

/// Checks the original absolute budget before polling even an already-ready service future.
/// The future remains in this one select across wakes, rather than starting another request.
async fn bounded_until<T>(
    until: tokio::time::Instant,
    future: impl std::future::Future<Output = T>,
) -> io::Result<T> {
    use std::future::{Future, poll_fn};
    let timer = tokio::time::sleep_until(until);
    tokio::pin!(timer, future);
    poll_fn(|cx| {
        if tokio::time::Instant::now() >= until || timer.as_mut().poll(cx).is_ready() {
            return std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native title presence deadline elapsed",
            )));
        }
        future.as_mut().poll(cx).map(Ok)
    })
    .await
}

/// Reads only the newest bounded application state, never account files or credentials.
fn latest(shared: &Mutex<Snapshot>) -> bridge::XboxPresenceState {
    shared
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .xbox_presence
        .clone()
}

/// Coalesces changed state while preserving the server's original heartbeat deadline.
async fn wait_latest(
    shared: &Mutex<Snapshot>,
    lifetime: &TitlePresenceLifetime,
    sent: &bridge::XboxPresenceState,
    until: tokio::time::Instant,
) {
    loop {
        let changed = lifetime.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if lifetime.retirement_requested() || latest(shared) != *sent {
            return;
        }
        tokio::select! {
            _ = changed => {},
            _ = tokio::time::sleep_until(until) => return,
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "test-support")]
impl Service for Arc<super::retirement_test_support::State> {
    type Scope = ();
    async fn bind(&self) -> io::Result<()> {
        Ok(())
    }
    async fn update(
        &self,
        _: &(),
        _: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>> {
        *dispatched = true;
        self.published();
        Ok(Some(FALLBACK_HEARTBEAT))
    }
    async fn remove(&self, _: &()) -> io::Result<()> {
        super::retirement_test_support::State::remove(self.as_ref()).await
    }
}
