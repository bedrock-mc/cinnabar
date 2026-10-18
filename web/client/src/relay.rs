//! The page's single relay driver. Host observations own clocks, timers and presentation.
use crate::launcher::RelayAccount;
use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    time::Duration,
};
use web_transport::{
    CloseReason, RelayTransport,
    control::connection_refusal_message,
    session::{AccountOperation, ControlEvent, RequestRefusal, Session, State},
};

/// Bounds control opening and account writes with the same host-owned deadline.
const CONTROL_DEADLINE: Duration = Duration::from_secs(10);

/// Lets the relay replenish a control request between busy sign-out refusals.
const SIGN_OUT_RETRY_DELAY: Duration =
    Duration::from_millis(bridge::contract::RELAY_CONTROL_REFILL_MILLIS as u64);

use crate::play;

/// Keeps play admission and reconnect policy in the same sign-out transition.
#[derive(Clone, Copy)]
enum Admission {
    Open,
    SigningOut {
        deadline: Duration,
        retry_at: Option<Duration>,
    },
    Halted,
}

impl Admission {
    /// Returns the bounded sign-out exchange while new play sessions are blocked.
    fn sign_out(self) -> Option<(Duration, Option<Duration>)> {
        match self {
            Self::SigningOut { deadline, retry_at } => Some((deadline, retry_at)),
            _ => None,
        }
    }

    /// Ends a pending exchange while keeping automatic reconnects halted.
    fn finish_sign_out(self) -> Self {
        match self {
            Self::SigningOut { .. } => Self::Halted,
            _ => self,
        }
    }

    /// Resumes reconnects without interrupting a pending sign-out exchange.
    fn reconnect(self) -> Self {
        match self {
            Self::Halted => Self::Open,
            _ => self,
        }
    }
}

/// Explicit UI intent; restoring a page retains its account capability.
#[derive(Clone, Copy, Default)]
pub(crate) enum Refresh {
    #[default]
    None,
    Reconnect,
    Fresh,
}

/// Tags account intent at issuance, before the driver can observe a lifecycle change.
#[derive(Default)]
pub(crate) struct Intent {
    pub(crate) ready: Rc<Cell<bool>>,
    refresh: Cell<Refresh>,
    generation: Cell<u64>,
}

impl Intent {
    /// Invalidates old commands immediately when the page requests a fresh session.
    pub(crate) fn request(&self, refresh: Refresh) {
        if matches!(refresh, Refresh::Fresh) {
            self.ready.set(false);
            self.generation.set(
                self.generation
                    .get()
                    .checked_add(1)
                    .expect("session generation exhausted"),
            );
        }
        self.refresh.set(refresh);
    }

    /// Associates one account command with the session selected by the page.
    pub(crate) fn account(&self, operation: AccountOperation) -> AccountCommand {
        AccountCommand {
            generation: self.generation.get(),
            operation,
        }
    }
}

/// Account commands stay bound to their issuing session through every queue.
#[derive(Debug)]
pub(crate) struct AccountCommand {
    generation: u64,
    operation: AccountOperation,
}

/// Browser services supplied only at composition, keeping the driver testable without JS.
pub(crate) trait Host {
    type Transport: RelayTransport<Stream: 'static> + 'static;
    /// Samples the host monotonic clock.
    fn now(&self) -> Duration;
    /// Supplies one bounded retry offset.
    fn jitter(&self) -> Duration;
    /// Registers a timer cancelled when its future is dropped.
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()>;
    /// Creates candidates without performing a second account or gameplay implementation.
    fn transports(&self) -> Result<(Option<Self::Transport>, Self::Transport), CloseReason>;
    /// Emits only typed, sanitized page observations.
    fn observe(&self, event: serde_json::Value);
    /// Shares the menu's account presentation with the retained session driver.
    fn account(&self) -> &RefCell<RelayAccount>;
}

/// Runs selection, backoff and account control on one retained session until disposal.
pub(crate) async fn run<H: Host>(
    host: H,
    mut pending: mpsc::Receiver<AccountCommand>,
    mut joins: mpsc::Receiver<play::Request<<H::Transport as RelayTransport>::Stream>>,
    mut changed: mpsc::Receiver<()>,
    paused: Rc<Cell<bool>>,
    intent: Rc<Intent>,
    cancelled: oneshot::Receiver<()>,
) {
    let mut session = Session::new();
    let mut cancelled = cancelled.fuse();
    let admission = Cell::new(Admission::Open);
    let mut requested: Option<play::Request<<H::Transport as RelayTransport>::Stream>> = None;
    let mut playing: Option<play::Active> = None;
    let mut generation = 0;
    loop {
        intent.ready.set(
            matches!(session.state(), State::Ready(_))
                && requested.is_none()
                && playing.is_none()
                && !paused.get()
                && admission.get().sign_out().is_none()
                && generation == intent.generation.get(),
        );
        let ready = matches!(session.state(), State::Ready(_) | State::Playing(_));
        let writing = ready && session.has_account_intent();
        let opening = requested.is_some();
        let parked = paused.get()
            || (!ready
                && ((!matches!(admission.get(), Admission::Open)
                    && !session.has_account_intent())
                    || session.next_retry().is_err()));
        enum Action<S> {
            Dispose,
            Lifecycle,
            Account(Option<AccountCommand>),
            Join(Option<play::Request<S>>),
            PlayEnded,
            Step(Result<(), CloseReason>),
        }
        let mut started = None;
        let mut queued_account = None;
        let action = {
            let handshaking = Cell::new(false);
            let step = async {
                if parked {
                    return std::future::pending::<Result<(), CloseReason>>().await;
                }
                if writing {
                    bridge::tasks::until(session.flush_account(), host.sleep(CONTROL_DEADLINE))
                        .await
                        .unwrap_or(Err(CloseReason::Transport))?;
                    return Ok(());
                }
                if let Some(request) = requested.take() {
                    if request.reply.is_canceled() {
                        return Ok(());
                    }
                    if !matches!(session.state(), State::Ready(_)) {
                        let _ = request.reply.send(Err(CloseReason::Protocol));
                        return Ok(());
                    }
                    match session
                        .open_play(|| host.now(), host.sleep(CONTROL_DEADLINE))
                        .await
                    {
                        Ok((reader, frames, writer)) => {
                            let (ended, finished) = oneshot::channel();
                            let (cancel, cancelled) = oneshot::channel();
                            started = Some(play::Active {
                                finished,
                                cancel: Some(cancel),
                            });
                            let _ = request.reply.send(Ok(play::Lease {
                                io: play::Io {
                                    join: session.play_join().expect("opened play has a join"),
                                    reader,
                                    frames,
                                    writer,
                                    packs: Box::pin(session.pack_stream()),
                                },
                                ended,
                                cancelled,
                            }));
                        }
                        Err(reason) => {
                            let _ = request.reply.send(Err(reason));
                            return Err(reason);
                        }
                    }
                    return Ok(());
                }
                if ready {
                    let event = session.next_event(|| host.now());
                    let event = if let Some((deadline, retry_at)) = admission.get().sign_out() {
                        let wake = retry_at.map_or(deadline, |retry| retry.min(deadline));
                        match bridge::tasks::until(
                            event,
                            host.sleep(wake.saturating_sub(host.now())),
                        )
                        .await
                        {
                            Ok(event) => event?,
                            Err(_) if host.now() < deadline => {
                                admission.set(Admission::SigningOut {
                                    deadline,
                                    retry_at: None,
                                });
                                session.desire_account(AccountOperation::SignOut);
                                return Ok(());
                            }
                            Err(_) => return Err(CloseReason::Transport),
                        }
                    } else {
                        event.await?
                    };
                    if let Some((deadline, _)) = admission.get().sign_out()
                        && matches!(
                            event,
                            ControlEvent::RequestRefused(RequestRefusal::Refused(
                                bridge::contract::RefusalCode::SessionBusy
                            ))
                        )
                    {
                        admission.set(Admission::SigningOut {
                            deadline,
                            retry_at: Some(host.now().saturating_add(SIGN_OUT_RETRY_DELAY)),
                        });
                        return Ok(());
                    }
                    host.account().borrow_mut().observe(&event);
                    match event {
                        ControlEvent::Account(account) => {
                            if matches!(
                                account.state,
                                bridge::contract::AuthState::SignedOut
                                    | bridge::contract::AuthState::Failed
                            ) {
                                admission.set(admission.get().finish_sign_out());
                            }
                            host.observe(
                                serde_json::json!({"state": account.state, "account": account}),
                            );
                        }
                        ControlEvent::Connect(connect) => {
                            host.observe(serde_json::json!({"connect": connect}))
                        }
                        ControlEvent::RequestRefused(refusal) => {
                            let code = match refusal {
                                RequestRefusal::RetrySignIn => {
                                    bridge::contract::RefusalCode::AdmissionRefused
                                }
                                RequestRefusal::Refused(code) => code,
                            };
                            host.observe(
                                serde_json::json!({"code": code, "error": refusal.message()}),
                            );
                        }
                        ControlEvent::Keepalive => {}
                    }
                } else {
                    let due = session.next_retry()?;
                    if due > host.now() {
                        host.sleep(due.saturating_sub(host.now())).await;
                    }
                    if host.now() < due {
                        return Err(CloseReason::RetryAt(due));
                    }
                    let (quic, socket) = host.transports()?;
                    handshaking.set(true);
                    web_transport::select(
                        &mut session,
                        quic,
                        || socket,
                        || host.now(),
                        || host.sleep(CONTROL_DEADLINE),
                        || host.jitter(),
                    )
                    .await?;
                    if !session.resumed_account() {
                        let mut account = host.account().borrow_mut();
                        account.fresh_session();
                        if let Some(operation) = session.pending_account() {
                            account.restore_operation(operation);
                        }
                    }
                    host.observe(serde_json::json!({"state": "connected"}));
                }
                Ok(())
            };
            futures::pin_mut!(step);
            loop {
                let action = futures::select_biased! {
                    _ = cancelled => Action::Dispose,
                    signal = changed.next().fuse() => if signal.is_none() { Action::Dispose } else { Action::Lifecycle },
                    command = async {
                        if writing || opening { std::future::pending().await } else { pending.next().await }
                    }.fuse() => Action::Account(command),
                    request = async {
                        if writing || opening { std::future::pending().await } else { joins.next().await }
                    }.fuse() => Action::Join(request),
                    _ = async {
                        if writing || admission.get().sign_out().is_some() { return std::future::pending().await; }
                        match playing.as_mut() {
                            Some(active) => { let _ = (&mut active.finished).await; }
                            None => std::future::pending().await,
                        }
                    }.fuse() => Action::PlayEnded,
                    result = step.as_mut().fuse() => Action::Step(result),
                };
                match action {
                    Action::Account(Some(command)) if command.generation != generation => {}
                    // Keep the Hello/Welcome future alive until its bearer has been stored.
                    Action::Account(Some(operation)) if !ready && !parked => {
                        queued_account = Some(operation);
                    }
                    Action::Join(Some(request)) if !ready => {
                        let _ = request.reply.send(Err(CloseReason::Protocol));
                    }
                    Action::Lifecycle
                        if !paused.get()
                            && matches!(intent.refresh.get(), Refresh::Reconnect)
                            && intent.generation.get() == generation
                            && (ready || handshaking.get()) =>
                    {
                        while changed.try_recv().is_ok() {}
                        intent.refresh.set(Refresh::None);
                        admission.set(admission.get().reconnect());
                    }
                    _ => break action,
                }
            }
        };
        if let Some(finished) = started {
            playing = Some(finished);
        }
        match action {
            Action::Dispose | Action::Account(None) => break,
            Action::Join(None) => break,
            Action::Join(Some(request)) => {
                if matches!(session.state(), State::Ready(_))
                    && playing.is_none()
                    && admission.get().sign_out().is_none()
                {
                    admission.set(Admission::Open);
                    requested = Some(request);
                } else {
                    let _ = request.reply.send(Err(CloseReason::Protocol));
                }
            }
            Action::PlayEnded => {
                playing = None;
                session.close(CloseReason::Transport);
            }
            Action::Lifecycle => {
                while changed.try_recv().is_ok() {}
                session.close(CloseReason::Transport);
                playing = None;
                admission.set(admission.get().finish_sign_out());
                let refresh = intent.refresh.replace(Refresh::None);
                if generation != intent.generation.get() {
                    generation = intent.generation.get();
                    session.fresh_session();
                    host.account().borrow_mut().fresh_session();
                    admission.set(Admission::Open);
                } else {
                    host.account().borrow_mut().disconnected();
                    match refresh {
                        Refresh::Reconnect => {
                            session.restart_retries();
                            admission.set(admission.get().reconnect());
                        }
                        Refresh::None | Refresh::Fresh => {}
                    }
                }
            }
            Action::Account(Some(operation)) => queued_account = Some(operation),
            Action::Step(Ok(())) => {}
            Action::Step(Err(CloseReason::RetryAt(_))) => {}
            Action::Step(Err(reason)) => {
                session.close(reason);
                playing = None;
                admission.set(admission.get().finish_sign_out());
                host.account().borrow_mut().disconnected();
                if let Some(code) = session.refusal() {
                    host.observe(serde_json::json!({"state": "refused", "code": code, "error": connection_refusal_message(code)}));
                } else if session.next_retry() == Err(CloseReason::RetryExhausted) {
                    host.observe(serde_json::json!({"state": "retry_exhausted", "error": "Reconnect attempts exhausted. Connect relay to retry."}));
                } else {
                    host.observe(serde_json::json!({"state": "disconnected"}));
                }
            }
        }
        if let Some(command) = queued_account {
            let latest = std::iter::once(command)
                .chain(std::iter::from_fn(|| pending.try_recv().ok()))
                .filter(|command| command.generation == generation)
                .last();
            if let Some(command) = latest {
                match command.operation {
                    AccountOperation::SignOut => {
                        admission.set(Admission::SigningOut {
                            deadline: host.now().saturating_add(CONTROL_DEADLINE),
                            retry_at: None,
                        });
                        for request in requested
                            .take()
                            .into_iter()
                            .chain(std::iter::from_fn(|| joins.try_recv().ok()))
                        {
                            let _ = request.reply.send(Err(CloseReason::Protocol));
                        }
                        if let Some(active) = playing.as_mut() {
                            active.cancel.take();
                        }
                    }
                    AccountOperation::SignIn => admission.set(Admission::Open),
                }
                session.desire_account(command.operation);
                host.account()
                    .borrow_mut()
                    .restore_operation(command.operation);
            }
        }
    }
    intent.ready.set(false);
    session.close(CloseReason::Cancelled);
    host.account().borrow_mut().disconnected();
}

#[cfg(test)]
mod tests;
