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
    refresh: Cell<Refresh>,
    generation: Cell<u64>,
}

impl Intent {
    /// Invalidates old commands immediately when the page requests a fresh session.
    pub(crate) fn request(&self, refresh: Refresh) {
        if matches!(refresh, Refresh::Fresh) {
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
    type Transport: RelayTransport;
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
    mut changed: mpsc::Receiver<()>,
    paused: Rc<Cell<bool>>,
    intent: Rc<Intent>,
    cancelled: oneshot::Receiver<()>,
) {
    let mut session = Session::new();
    let mut cancelled = cancelled.fuse();
    let mut halted = false;
    let mut generation = 0;
    loop {
        let ready = matches!(session.state(), State::Ready(_) | State::Playing(_));
        let writing = ready && session.has_account_intent();
        let parked = paused.get()
            || (!ready
                && ((halted && !session.has_account_intent()) || session.next_retry().is_err()));
        enum Action {
            Dispose,
            Lifecycle,
            Account(Option<AccountCommand>),
            Step(Result<(), CloseReason>),
        }
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
                if ready {
                    let event = session.next_event(|| host.now()).await?;
                    host.account().borrow_mut().observe(&event);
                    match event {
                        ControlEvent::Account(account) => host.observe(
                            serde_json::json!({"state": account.state, "account": account}),
                        ),
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
                        if writing { std::future::pending().await } else { pending.next().await }
                    }.fuse() => Action::Account(command),
                    result = step.as_mut().fuse() => Action::Step(result),
                };
                match action {
                    Action::Account(Some(command)) if command.generation != generation => {}
                    // Keep the Hello/Welcome future alive until its bearer has been stored.
                    Action::Account(Some(operation)) if !ready && !parked => {
                        queued_account = Some(operation);
                    }
                    Action::Lifecycle
                        if !paused.get()
                            && matches!(intent.refresh.get(), Refresh::Reconnect)
                            && intent.generation.get() == generation
                            && (ready || handshaking.get()) =>
                    {
                        while changed.try_recv().is_ok() {}
                        intent.refresh.set(Refresh::None);
                        halted = false;
                    }
                    _ => break action,
                }
            }
        };
        match action {
            Action::Dispose | Action::Account(None) => break,
            Action::Lifecycle => {
                while changed.try_recv().is_ok() {}
                session.close(CloseReason::Transport);
                let refresh = intent.refresh.replace(Refresh::None);
                if generation != intent.generation.get() {
                    generation = intent.generation.get();
                    session.fresh_session();
                    host.account().borrow_mut().fresh_session();
                    halted = false;
                } else {
                    host.account().borrow_mut().disconnected();
                    match refresh {
                        Refresh::Reconnect => {
                            session.restart_retries();
                            halted = false;
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
                halted = command.operation == AccountOperation::SignOut;
                session.desire_account(command.operation);
                host.account()
                    .borrow_mut()
                    .restore_operation(command.operation);
            }
        }
    }
    session.close(CloseReason::Cancelled);
    host.account().borrow_mut().disconnected();
}

#[cfg(test)]
mod tests;
