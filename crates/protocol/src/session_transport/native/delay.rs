//! One opt-in post-login delay lease; application envelopes retain their own immutable deadlines.
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// Control of the original native play connection's finite application-packet delay.
/// Clones retain only policy/observations, never a socket, actor, reader or runtime.
#[derive(Clone)]
pub struct NativePacketDelay {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    changed: Notify,
}

struct State {
    live: bool,
    actor: u64,
    delay: Duration,
    expires: Instant,
    released: u64,
    show: bool,
    position_epoch: u64,
    position: Option<[f32; 3]>,
}

/// One actual FIFO admission's original deadline and backlog release generation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    due: Instant,
    released: u64,
}

/// A position opt-in captured immediately before the original physical write.
#[derive(Clone, Copy)]
pub(crate) struct Tracking {
    pub(crate) actor: u64,
    epoch: u64,
}

impl State {
    /// Clears an expired lease BEFORE later configuration can refresh it.
    fn expire(&mut self, now: Instant) -> bool {
        if self.delay.is_zero() || now < self.expires {
            return false;
        }
        self.reset();
        true
    }

    /// Releases original backlog and invalidates in-flight position publication.
    fn reset(&mut self) {
        self.delay = Duration::ZERO;
        self.released = self.released.wrapping_add(1);
        self.show = false;
        self.clear_position();
    }

    /// Invalidates observations captured before hide/reset/expiry.
    fn clear_position(&mut self) {
        self.position = None;
        self.position_epoch = self.position_epoch.wrapping_add(1);
    }
}

impl NativePacketDelay {
    /// Starts disabled on the original connection, before any play capability is exposed.
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    live: true,
                    actor: 0,
                    delay: Duration::ZERO,
                    expires: now(),
                    released: 0,
                    show: false,
                    position_epoch: 0,
                    position: None,
                }),
                changed: Notify::new(),
            }),
        }
    }

    /// Binds only the actual SpawnReady actor on this same connection.
    pub(crate) fn bind(&self, actor: u64) {
        let mut state = self.state();
        if state.live && state.actor == 0 {
            state.actor = actor;
        }
    }

    /// Refreshes one finite heartbeat; zero disables and releases already accepted backlog.
    ///
    /// # Errors
    /// Rejects an excessive delay or a play connection that has not spawned or has retired.
    pub fn configure(&self, delay_ms: u32, show_position: bool) -> io::Result<()> {
        self.configure_at(delay_ms, show_position, now())
    }

    /// Applies policy with caller time, keeping tests and state transitions deterministic.
    fn configure_at(&self, delay_ms: u32, show: bool, now: Instant) -> io::Result<()> {
        if delay_ms > bridge::contract::MAX_PACKET_DELAY_MS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "packet delay exceeds limit",
            ));
        }
        let expires = now
            .checked_add(Duration::from_millis(u64::from(
                bridge::contract::PACKET_DELAY_LEASE_MILLIS,
            )))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "packet delay lease exceeds clock range",
                )
            })?;
        let mut state = self.state();
        if !state.live || state.actor == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "native play connection is unavailable",
            ));
        }
        let expired = state.expire(now);
        let delay = Duration::from_millis(u64::from(delay_ms));
        if delay.is_zero() || !show || state.show != show {
            state.clear_position();
        }
        if delay.is_zero() && !state.delay.is_zero() {
            state.released = state.released.wrapping_add(1);
        }
        let changed = expired || state.delay != delay;
        state.delay = delay;
        state.show = show && !delay.is_zero();
        state.expires = expires;
        drop(state);
        if changed {
            self.inner.changed.notify_waiters();
        }
        Ok(())
    }

    /// Returns the last finite own network anchor whose original write completed under this opt-in.
    /// Absence includes hide, expiry and connection retirement; this is not a peer acknowledgment.
    pub fn position(&self) -> Option<[f32; 3]> {
        let mut state = self.state();
        let expired = state.expire(now());
        let position = (state.live && state.show && !state.delay.is_zero())
            .then_some(state.position)
            .flatten();
        drop(state);
        if expired {
            self.inner.changed.notify_waiters();
        }
        position
    }

    /// Captures one immutable admission deadline without changing the FIFO's payload owner.
    pub(crate) fn capture(&self, now: Instant) -> Option<Deadline> {
        let mut state = self.state();
        let expired = state.expire(now);
        let deadline = (state.live && !state.delay.is_zero())
            .then(|| now.checked_add(state.delay))
            .flatten()
            .map(|due| Deadline {
                due,
                released: state.released,
            });
        drop(state);
        if expired {
            self.inner.changed.notify_waiters();
        }
        deadline
    }

    /// Computes the current wait without retiming the original accepted envelope.
    pub(crate) fn remaining(&self, deadline: Deadline, now: Instant) -> Option<Duration> {
        let mut state = self.state();
        let expired = state.expire(now);
        let remaining =
            (state.live && !state.delay.is_zero() && deadline.released == state.released)
                .then(|| {
                    deadline
                        .due
                        .min(state.expires)
                        .saturating_duration_since(now)
                })
                .filter(|duration| !duration.is_zero());
        drop(state);
        if expired {
            self.inner.changed.notify_waiters();
        }
        remaining
    }

    /// Waits only for the original immutable due time or an actual configuration release.
    pub(crate) async fn wait(&self, deadline: Option<Deadline>) {
        let Some(deadline) = deadline else { return };
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let Some(remaining) = self.remaining(deadline, now()) else {
                return;
            };
            tokio::select! {
                _ = &mut changed => {},
                _ = tokio::time::sleep(remaining) => {},
            }
        }
    }

    /// Captures current opt-in only at the original write boundary.
    pub(crate) fn tracking(&self, now: Instant) -> Option<Tracking> {
        let mut state = self.state();
        let expired = state.expire(now);
        let tracking = (state.live && state.show && !state.delay.is_zero()).then_some(Tracking {
            actor: state.actor,
            epoch: state.position_epoch,
        });
        drop(state);
        if expired {
            self.inner.changed.notify_waiters();
        }
        tracking
    }

    /// Commits only an actually completed original write while its captured opt-in remains current.
    pub(crate) fn forwarded(&self, tracking: Tracking, position: [f32; 3], now: Instant) {
        let mut state = self.state();
        let expired = state.expire(now);
        if state.live
            && state.show
            && !state.delay.is_zero()
            && state.actor == tracking.actor
            && state.position_epoch == tracking.epoch
            && position.iter().all(|value| value.is_finite())
        {
            state.position = Some(position);
        }
        drop(state);
        if expired {
            self.inner.changed.notify_waiters();
        }
    }

    /// Seals this exact connection's policy and releases original accepted work for teardown.
    pub(crate) fn retire(&self) {
        let mut state = self.state();
        state.live = false;
        state.reset();
        drop(state);
        self.inner.changed.notify_waiters();
    }

    /// Holds only synchronous policy work; no caller keeps the guard across I/O or a timer.
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Reads the existing native timer's monotonic clock, including deterministic paused time.
pub(crate) fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expiry before a same-value heartbeat releases old backlog and invalidates its write token.
    #[test]
    fn expiry_cannot_be_erased_by_immediate_reconfiguration() {
        let delay = NativePacketDelay::new();
        delay.bind(42);
        let start = Instant::now();
        let max_ms = bridge::contract::MAX_PACKET_DELAY_MS;
        let maximum = Duration::from_millis(u64::from(max_ms));
        let lease = Duration::from_millis(u64::from(bridge::contract::PACKET_DELAY_LEASE_MILLIS));
        delay.configure_at(max_ms, true, start).unwrap();
        let admitted = start + lease.saturating_sub(maximum / 2);
        let deadline = delay.capture(admitted).unwrap();
        let tracking = delay.tracking(admitted).unwrap();
        let expired = start + lease;
        delay.configure_at(max_ms, true, expired).unwrap();
        assert!(
            delay.remaining(deadline, expired).is_none(),
            "a heartbeat erased the original expiry release"
        );
        delay.forwarded(tracking, [1.0, 2.0, 3.0], expired);
        assert_eq!(
            delay.state().position,
            None,
            "expired opt-in published a later physical write"
        );
        let fresh = delay.capture(expired).unwrap();
        assert_eq!(delay.remaining(fresh, expired), Some(maximum));
        delay.retire();
        assert!(delay.remaining(fresh, expired).is_none());
        assert!(delay.configure_at(max_ms, true, expired).is_err());
    }
}
