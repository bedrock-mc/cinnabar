//! Bounded application admission and one caller-driven service owner.

use super::{
    account::Account,
    budget::Budget,
    session::{Session, busy, closed},
};
use bridge::contract::*;
use futures::{StreamExt, stream::FuturesUnordered};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    io,
    net::IpAddr,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

#[cfg(test)]
pub(super) mod tests;

type Work = Pin<Box<dyn Future<Output = Completion> + Send>>;
type ServiceLoop = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

/// Owns cleanup after service completion until the retained physical connections retire.
pub(super) struct RegistryDriver {
    operation: Option<ServiceLoop>,
    lifetime: Option<Driver>,
    registry: Arc<Registry>,
}

struct Completion {
    session: Weak<Session>,
    sequence: u64,
    account: io::Result<Account>,
}

pub(super) struct Registry {
    state: Mutex<State>,
    pub(super) cancelled: CancellationToken,
    changed: Notify,
    driving: AtomicBool,
    accounts: Arc<Semaphore>,
}

struct State {
    sessions: Vec<Arc<Session>>,
    connections: HashMap<IpAddr, usize>,
    active: usize,
    limiters: HashMap<IpAddr, Limiter>,
    pending: VecDeque<Work>,
}

struct Limiter {
    last: Instant,
    budget: Budget,
}

impl Registry {
    /// Creates bounded application state without a worker, runtime or credential acquisition.
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                sessions: Vec::new(),
                connections: HashMap::new(),
                active: 0,
                limiters: HashMap::new(),
                pending: VecDeque::new(),
            }),
            cancelled: CancellationToken::new(),
            changed: Notify::new(),
            driving: AtomicBool::new(false),
            accounts: Arc::new(Semaphore::new(RELAY_MAX_SESSIONS)),
        })
    }

    /// Retains actual connection capacity before Hello until its complete stream handler retires.
    pub(super) fn admit(
        self: &Arc<Self>,
        client: IpAddr,
        now: Instant,
    ) -> io::Result<ConnectionPermit> {
        let client = client.to_canonical();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.is_cancelled()
            || state.active >= RELAY_MAX_SESSIONS
            || state.connections.get(&client).copied().unwrap_or_default() >= RELAY_MAX_PER_IP
            || !state.spend(client, now)
        {
            return Err(admission());
        }
        state.active += 1;
        *state.connections.entry(client).or_default() += 1;
        Ok(ConnectionPermit {
            registry: Arc::clone(self),
            client,
        })
    }

    /// Resolves only original capabilities and retires the previous controller before replacement.
    pub(super) fn open_control(
        self: &Arc<Self>,
        client: IpAddr,
        resume: &str,
        now: Instant,
    ) -> io::Result<Attachment> {
        let client = client.to_canonical();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.is_cancelled() {
            return Err(closed());
        }
        let session = if resume.is_empty() {
            if state.sessions.len() >= RELAY_MAX_SESSIONS
                || state.session_count(client) >= RELAY_MAX_PER_IP
            {
                return Err(admission());
            }
            let session = Session::new(client, now)?;
            state.sessions.push(Arc::clone(&session));
            session
        } else {
            state
                .sessions
                .iter()
                .find(|session| {
                    session
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .admits(resume, now)
                })
                .cloned()
                .ok_or_else(resume_refused)?
        };
        let client_sessions = state.session_count(client);
        let mut account = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if account.client != client && client_sessions >= RELAY_MAX_PER_IP {
            return Err(admission());
        }
        let generation = account.generation.checked_add(1).ok_or_else(closed)?;
        account.rotate(resume, now)?;
        if resume.is_empty() {
            account.token_pending = false;
        }
        account.control.cancel();
        account.control = CancellationToken::new();
        account.generation = generation;
        account.last_join = 0;
        account.client = client;
        account.attached += 1;
        let cancel = account.control.clone();
        drop(account);
        Ok(Attachment {
            session,
            generation,
            cancel,
            controller: true,
        })
    }

    /// Binds a separate play stream to this exact original controller and an unexpired bearer.
    pub(super) fn open_play(
        &self,
        resume: &str,
        generation: u64,
        now: Instant,
    ) -> io::Result<Attachment> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.is_cancelled() {
            return Err(closed());
        }
        let session = state
            .sessions
            .iter()
            .find(|session| {
                session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .admits(resume, now)
            })
            .cloned()
            .ok_or_else(resume_refused)?;
        let mut account = session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        account.check(generation)?;
        account.attached += 1;
        let cancel = account.control.clone();
        drop(account);
        Ok(Attachment {
            session,
            generation,
            cancel,
            controller: false,
        })
    }

    /// Attaches a stream only to the current controller on this actual WebTransport connection.
    #[cfg(feature = "webtransport")]
    pub(super) fn bound_play(
        &self,
        controller: &Attachment,
        generation: u64,
    ) -> io::Result<Attachment> {
        let registry = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.is_cancelled()
            || !controller.controller
            || generation != controller.generation
            || !registry
                .sessions
                .iter()
                .any(|session| Arc::ptr_eq(session, &controller.session))
        {
            return Err(resume_refused());
        }
        let mut state = controller
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.check(controller.generation)?;
        state.attached += 1;
        Ok(Attachment {
            session: Arc::clone(&controller.session),
            generation,
            cancel: state.control.clone(),
            controller: false,
        })
    }

    /// Queues one real acquisition future; its capacity remains held through the final account lifetime.
    pub(super) fn sign_in(&self, attachment: &Attachment, now: Instant) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut account = attachment
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        account.check(attachment.generation)?;
        if account.play.is_some() {
            return Err(busy());
        }
        if !state.spend(account.client, now) {
            return Err(admission());
        }
        if account.sign_in.is_some() || account.account.is_some() {
            return Ok(());
        }
        let capacity = Arc::clone(&self.accounts)
            .try_acquire_owned()
            .map_err(|_| admission())?;
        let sequence = account.sign_in_sequence.checked_add(1).ok_or_else(closed)?;
        let deadline = now
            .checked_add(Duration::from_secs(u64::from(RELAY_SIGN_IN_SECONDS)))
            .ok_or_else(closed)?;
        let cancel = CancellationToken::new();
        account.sign_in_sequence = sequence;
        account.sign_in = Some((sequence, cancel.clone()));
        let session = Arc::downgrade(&attachment.session);
        let publish = session.clone();
        let cancelled = self.cancelled.clone();
        let publication = cancelled.clone();
        let future = async move {
            let acquiring = Account::sign_in(
                deadline,
                move |observation| {
                    if publication.is_cancelled() {
                        return Err(closed());
                    }
                    publish
                        .upgrade()
                        .ok_or_else(closed)?
                        .publish_sign_in(sequence, observation)
                },
                capacity,
            );
            tokio::pin!(acquiring);
            let result = tokio::select! { biased; () = cancelled.cancelled() => Err(closed()), () = cancel.cancelled() => Err(closed()), result = &mut acquiring => result };
            Completion {
                session,
                sequence,
                account: result,
            }
        };
        state.pending.push_back(Box::pin(future));
        drop(account);
        drop(state);
        self.changed.notify_one();
        Ok(())
    }

    /// Spends a play action only for an actual still-current control owner.
    pub(super) fn allow_play(&self, attachment: &Attachment, now: Instant) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let account = attachment
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        account.check(attachment.generation)?;
        if !state.spend(account.client, now) {
            return Err(admission());
        }
        Ok(())
    }

    /// Creates the actual cleanup owner, acquiring exclusive driving only on its first poll.
    /// Retain this future itself until physical connections retire, even after it completes.
    pub(super) fn drive(self: &Arc<Self>) -> RegistryDriver {
        let registry = Arc::clone(self);
        RegistryDriver {
            operation: Some(Box::pin(Arc::clone(&registry).drive_services())),
            lifetime: None,
            registry,
        }
    }

    /// Runs the original service work while its outer driver retains cleanup ownership.
    async fn drive_services(self: Arc<Self>) -> io::Result<()> {
        let mut work = FuturesUnordered::new();
        let mut maintenance =
            tokio::time::interval(Duration::from_secs(u64::from(RELAY_KEEPALIVE_SECONDS)));
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                work.extend(state.pending.drain(..));
            }
            tokio::select! {
                biased;
                () = self.cancelled.cancelled() => return Ok(()),
                Some(completion) = work.next(), if !work.is_empty() => {
                    if let Some(session) = completion.session.upgrade() { session.finish_sign_in(completion.sequence, completion.account); }
                }
                () = &mut changed => {},
                now = maintenance.tick() => self.expire(now.into_std()),
            }
        }
    }

    /// Retires only detached expired accounts and the original bounded per-peer action observations.
    fn expire(&self, now: Instant) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.limiters.retain(|_, limiter| {
            now.checked_duration_since(limiter.last)
                .is_none_or(|elapsed| elapsed <= Duration::from_secs(u64::from(RELAY_IDLE_SECONDS)))
        });
        state.sessions.retain(|session| {
            let expired = session
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .expired(now);
            if expired {
                session.close();
            }
            !expired
        });
    }

    /// Cancels every concrete controller and SDK acquisition, then drops unclaimed queued work.
    pub(super) fn close(&self) {
        self.cancelled.cancel();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for session in state.sessions.drain(..) {
            session.close();
        }
        state.pending.clear();
    }
}

impl State {
    /// Counts retained accounts independently of live connection admission.
    fn session_count(&self, client: IpAddr) -> usize {
        self.sessions
            .iter()
            .filter(|session| {
                session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .client
                    == client
            })
            .count()
    }

    /// Bounds action limiter retention before accepting a new peer address.
    fn spend(&mut self, client: IpAddr, now: Instant) -> bool {
        if !self.limiters.contains_key(&client) {
            if self.limiters.len() >= RELAY_MAX_ACTION_CLIENTS {
                return false;
            }
            self.limiters.insert(
                client,
                Limiter {
                    last: now,
                    budget: Budget::new(
                        RELAY_ACTION_BURST,
                        Duration::from_millis(u64::from(RELAY_ACTION_REFILL_MILLIS)),
                        now,
                    ),
                },
            );
        }
        let limiter = self.limiters.get_mut(&client).expect("admitted limiter");
        limiter.last = now;
        limiter.budget.spend(now)
    }
}

/// Capacity stays attached to the actual stream handler, including its final cleanup.
pub(super) struct ConnectionPermit {
    registry: Arc<Registry>,
    client: IpAddr,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut state = self
            .registry
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.active -= 1;
        let count = state
            .connections
            .get_mut(&self.client)
            .expect("admitted connection");
        *count -= 1;
        if *count == 0 {
            state.connections.remove(&self.client);
        }
    }
}

/// The actual connection owns its original controller generation and attachment lifetime.
pub(super) struct Attachment {
    pub(super) session: Arc<Session>,
    pub(super) generation: u64,
    pub(super) cancel: CancellationToken,
    controller: bool,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        if self.controller {
            // Cancel this exact token: a resumed controller has a different token.
            self.cancel.cancel();
        }
        let mut state = self
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.attached -= 1;
        if state.attached == 0 {
            state.detached_at = Instant::now();
        }
    }
}

impl Future for RegistryDriver {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let driver = self.get_mut();
        if driver.operation.is_none() {
            return Poll::Pending;
        }
        if driver.lifetime.is_none() {
            if driver.registry.driving.swap(true, Ordering::AcqRel) {
                driver.operation = None;
                return Poll::Ready(Err(busy()));
            }
            driver.lifetime = Some(Driver(Arc::clone(&driver.registry)));
        }
        let result = match driver.operation.as_mut() {
            Some(operation) => operation.as_mut().poll(cx),
            None => return Poll::Pending,
        };
        if result.is_ready() {
            // Release completed work without dropping the outer cleanup guard.
            driver.operation = None;
        }
        result
    }
}

struct Driver(Arc<Registry>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.close();
        self.0.driving.store(false, Ordering::Release);
    }
}

/// Keeps account, peer and capability details out of an admission refusal.
pub(super) fn admission() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        Denied(RefusalCode::AdmissionRefused),
    )
}

/// Keeps capability lookup details out of a resume refusal.
fn resume_refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        Denied(RefusalCode::ResumeRefused),
    )
}

#[derive(Debug)]
struct Denied(RefusalCode);

impl std::fmt::Display for Denied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("gateway request refused")
    }
}
impl std::error::Error for Denied {}

/// Maps only explicit application refusal classes; arbitrary causes never become browser text.
pub(super) fn refusal(error: &io::Error) -> RefusalCode {
    if let Some(denied) = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<Denied>())
    {
        return denied.0;
    }
    if error.kind() == io::ErrorKind::WouldBlock {
        RefusalCode::SessionBusy
    } else {
        RefusalCode::Refused
    }
}
