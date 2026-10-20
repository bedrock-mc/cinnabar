//! One account lifetime, independent of its replaceable control connection.

use super::{account::Account, bearer::Bearer};
use bridge::{Account as Observation, AuthState, contract::*, relay_control::Capability};
use protocol::{ClientDataRefusal, NativeDeviceProfile, RawClientData, ValidatedClientData};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;

pub(super) struct Session {
    pub(super) state: Mutex<State>,
    pub(super) observation: watch::Sender<Observation>,
    pub(super) progress: watch::Sender<bridge::ConnectProgress>,
    pub(super) changed: Notify,
    guest: NativeDeviceProfile,
}

pub(super) struct State {
    pub(super) generation: u64,
    pub(super) control: CancellationToken,
    pub(super) attached: usize,
    pub(super) detached_at: Instant,
    pub(super) closed: bool,
    pub(super) current: Bearer,
    pub(super) previous: Option<Bearer>,
    pub(super) token_pending: bool,
    pub(super) client: std::net::IpAddr,
    pub(super) account: Option<Account>,
    pub(super) revision: u64,
    pub(super) sign_in: Option<(u64, CancellationToken)>,
    pub(super) sign_in_sequence: u64,
    pub(super) play: Option<(u64, u64)>,
    pub(super) last_join: u64,
}

impl Session {
    /// Starts one signed-out memory-only account without acquiring credentials.
    pub(super) fn new(client: std::net::IpAddr, now: Instant) -> io::Result<Arc<Self>> {
        let (observation, _) = watch::channel(signed_out());
        let (progress, _) = watch::channel(progress(bridge::ConnectStage::Idle));
        Ok(Arc::new(Self {
            state: Mutex::new(State {
                generation: 0,
                control: CancellationToken::new(),
                attached: 0,
                detached_at: now,
                closed: false,
                current: Bearer::new(now)?,
                previous: None,
                token_pending: true,
                client,
                account: None,
                revision: 0,
                sign_in: None,
                sign_in_sequence: 0,
                play: None,
                last_join: 0,
            }),
            observation,
            progress,
            changed: Notify::new(),
            guest: NativeDeviceProfile::fresh(),
        }))
    }

    /// Cancels actual work and prevents a pending account lease from restoring credentials.
    pub(super) fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return;
        }
        state.closed = true;
        state.control.cancel();
        if let Some((_, cancel)) = state.sign_in.take() {
            cancel.cancel();
        }
        state.account = None;
        self.observation.send_replace(signed_out());
        drop(state);
        self.changed.notify_waiters();
    }

    /// Publishes only the original still-wanted session-scoped sign-in, including after resume.
    pub(super) fn publish_sign_in(
        &self,
        sequence: u64,
        observation: Observation,
    ) -> io::Result<()> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed
            || !state
                .sign_in
                .as_ref()
                .is_some_and(|(wanted, cancel)| *wanted == sequence && !cancel.is_cancelled())
        {
            return Err(closed());
        }
        self.observation.send_replace(observation);
        Ok(())
    }

    /// Commits the one SDK result only while its original sign-in remains wanted and play is idle.
    pub(super) fn finish_sign_in(&self, sequence: u64, result: io::Result<Account>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed
            || !state
                .sign_in
                .as_ref()
                .is_some_and(|(wanted, cancel)| *wanted == sequence && !cancel.is_cancelled())
        {
            return;
        }
        state.sign_in = None;
        if state.play.is_some() {
            return;
        }
        match result {
            Ok(account) => {
                let Some(revision) = state.revision.checked_add(1) else {
                    drop(state);
                    self.close();
                    return;
                };
                state.revision = revision;
                self.observation.send_replace(account.observation());
                state.account = Some(account);
            }
            Err(_) => {
                let mut observation = signed_out();
                observation.state = AuthState::Failed;
                observation.reason = Some("Microsoft sign-in did not complete.".into());
                self.observation.send_replace(observation);
            }
        }
    }

    /// Refuses account replacement during play and cancels its actual pending acquisition otherwise.
    pub(super) fn sign_out(&self, generation: u64) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.check(generation)?;
        if state.play.is_some() {
            return Err(busy());
        }
        let revision = state.revision.checked_add(1).ok_or_else(closed)?;
        if let Some((_, cancel)) = state.sign_in.take() {
            cancel.cancel();
        }
        state.account = None;
        state.revision = revision;
        self.observation.send_replace(signed_out());
        Ok(())
    }

    /// Claims a newer join only after the previous concrete play owner has retired.
    pub(super) async fn begin_play(
        self: &Arc<Self>,
        generation: u64,
        join: u64,
    ) -> io::Result<PlayLease> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let cancelled = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.check(generation)?;
                if join == 0 || join <= state.last_join || state.sign_in.is_some() {
                    return Err(busy());
                }
                if state.play.is_none() {
                    state.last_join = join;
                    state.play = Some((generation, join));
                    return Ok(PlayLease {
                        session: Arc::clone(self),
                        generation,
                        join,
                        revision: state.revision,
                        account: state.account.take(),
                        cancel: state.control.clone(),
                    });
                }
                state.control.clone()
            };
            tokio::select! { biased; () = cancelled.cancelled() => return Err(closed()), () = &mut changed => {} }
        }
    }

    /// Validates against the same memory-only guest profile before any transport is authorized.
    pub(super) fn validate_guest(
        &self,
        claims: RawClientData,
        protocol: i32,
    ) -> Result<ValidatedClientData, ClientDataRefusal> {
        self.guest.validate_relay(claims, protocol)
    }

    /// Publishes only the already validated guest presentation through the shared identity overlay.
    pub(super) fn guest_claims(&self, claims: ValidatedClientData) -> io::Result<RawClientData> {
        let name = claims.requested_name().to_owned();
        claims.into_relay_claims(&name)
    }
}

impl State {
    /// Checks this actual controller before any mutation, mint or handoff boundary.
    pub(super) fn check(&self, generation: u64) -> io::Result<()> {
        if self.closed || self.generation != generation || self.control.is_cancelled() {
            return Err(closed());
        }
        Ok(())
    }

    /// Returns only an independently unexpired current or retained previous capability.
    pub(super) fn admits(&self, bearer: &str, now: Instant) -> bool {
        !self.closed
            && (self.current.matches(bearer, now)
                || self
                    .previous
                    .as_ref()
                    .is_some_and(|old| old.matches(bearer, now)))
    }

    /// Rotates while preserving the originally presented capability and its unchanged expiry.
    pub(super) fn rotate(&mut self, retain: &str, now: Instant) -> io::Result<()> {
        let replacement = Bearer::new(now)?;
        self.previous = if self.current.matches(retain, now) {
            Some(std::mem::replace(&mut self.current, replacement))
        } else {
            self.current = replacement;
            self.previous.take().filter(|old| old.matches(retain, now))
        };
        self.token_pending = true;
        Ok(())
    }

    /// Keeps lost welcome/pong retries on the original bearer deadline instead of repeatedly extending it.
    pub(super) fn capability(
        &mut self,
        generation: u64,
        refresh: bool,
        now: Instant,
    ) -> io::Result<Capability> {
        self.check(generation)?;
        if refresh && !self.token_pending {
            self.rotate(&self.current.wire(), now)?;
        }
        let remaining = self.current.expires.saturating_duration_since(now);
        Ok(Capability {
            version: RELAY_VERSION,
            resume: self.current.wire(),
            generation,
            expires_seconds: remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0),
        })
    }

    /// A delayed pong cannot acknowledge a newer capability that the browser has not received.
    pub(super) fn acknowledge(&mut self, generation: u64, bearer: &str, now: Instant) -> bool {
        if self.check(generation).is_err() || !self.current.matches(bearer, now) {
            return false;
        }
        self.token_pending = false;
        true
    }

    /// Starts the original idle budget only after every control/play attachment has ended.
    pub(super) fn expired(&self, now: Instant) -> bool {
        self.attached == 0
            && now
                .checked_duration_since(self.detached_at)
                .is_some_and(|elapsed| elapsed > Duration::from_secs(u64::from(RELAY_IDLE_SECONDS)))
    }
}

/// The sole account lease moves its actual SDK owner through mint/play and restores it on cancellation.
pub(super) struct PlayLease {
    pub(super) session: Arc<Session>,
    pub(super) generation: u64,
    pub(super) join: u64,
    revision: u64,
    pub(super) account: Option<Account>,
    pub(super) cancel: CancellationToken,
}

impl PlayLease {
    /// Publishes only the still-current join without exposing upstream addresses or provider causes.
    pub(super) fn progress(&self, value: bridge::ConnectProgress) -> io::Result<()> {
        let state = self
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check(&state)?;
        self.session.progress.send_replace(value);
        Ok(())
    }

    /// Rejects original-controller/account retirement immediately before output admission.
    pub(super) fn current(&self) -> io::Result<()> {
        let state = self
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check(&state)
    }

    /// Checks the original tuple while the caller holds the account mutation boundary.
    fn check(&self, state: &State) -> io::Result<()> {
        state.check(self.generation)?;
        if state.revision != self.revision || state.play != Some((self.generation, self.join)) {
            return Err(closed());
        }
        Ok(())
    }
}

impl Drop for PlayLease {
    fn drop(&mut self) {
        let mut state = self
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.play == Some((self.generation, self.join)) {
            state.play = None;
            self.session
                .progress
                .send_replace(progress(bridge::ConnectStage::Idle));
            if !state.closed && state.revision == self.revision {
                state.account = self.account.take();
            }
        }
        drop(state);
        self.session.changed.notify_waiters();
    }
}

/// Uses the existing public signed-out observation without paths, identifiers or credentials.
pub(super) fn signed_out() -> Observation {
    Observation {
        state: AuthState::SignedOut,
        verification_uri: None,
        user_code: None,
        gamertag: None,
        reason: None,
    }
}

/// Identifies controller/session retirement without exposing capability or account detail.
pub(super) fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "gateway session closed")
}

/// Identifies the original account/play mutual exclusion without exposing its state.
pub(super) fn busy() -> io::Error {
    io::Error::new(io::ErrorKind::WouldBlock, "gateway session is busy")
}

/// Starts a stage observation without inventing unavailable download counters.
pub(super) fn progress(stage: bridge::ConnectStage) -> bridge::ConnectProgress {
    bridge::ConnectProgress {
        stage,
        packs_done: 0,
        packs_total: 0,
        received_bytes: 0,
        total_bytes: 0,
    }
}
