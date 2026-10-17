//! The page's single relay driver. Host observations own clocks, timers and presentation.
use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
};
use std::{cell::Cell, future::Future, rc::Rc, time::Duration};
use web_transport::{
    CloseReason, RelayTransport,
    control::refusal_message,
    session::{AccountOperation, ControlEvent, Session, State},
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
}

/// Runs selection, backoff and account control on one retained session until disposal.
pub(crate) async fn run<H: Host>(
    host: H,
    mut pending: mpsc::Receiver<AccountOperation>,
    mut changed: mpsc::Receiver<()>,
    paused: Rc<Cell<bool>>,
    refresh: Rc<Cell<Refresh>>,
    cancelled: oneshot::Receiver<()>,
) {
    let mut session = Session::new();
    let mut cancelled = cancelled.fuse();
    let mut halted = false;
    loop {
        let ready = matches!(session.state(), State::Ready(_) | State::Playing(_));
        let writing = ready && session.has_account_intent();
        let parked = paused.get()
            || (!ready
                && ((halted && !session.has_account_intent()) || session.next_retry().is_err()));
        enum Action {
            Dispose,
            Lifecycle,
            Account(Option<AccountOperation>),
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
                    match session.next_event(|| host.now()).await? {
                        ControlEvent::Account(account) => host.observe(
                            serde_json::json!({"state": account.state, "account": account}),
                        ),
                        ControlEvent::Connect(connect) => {
                            host.observe(serde_json::json!({"connect": connect}))
                        }
                        ControlEvent::Error(code) => host.observe(
                            serde_json::json!({"code": code, "error": refusal_message(code)}),
                        ),
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
                    // Keep the Hello/Welcome future alive until its bearer has been stored.
                    Action::Account(Some(operation)) if !ready && !parked => {
                        queued_account = Some(operation);
                    }
                    Action::Lifecycle
                        if !paused.get()
                            && matches!(refresh.get(), Refresh::Reconnect)
                            && (ready || handshaking.get()) =>
                    {
                        while changed.try_recv().is_ok() {}
                        refresh.set(Refresh::None);
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
                match refresh.replace(Refresh::None) {
                    Refresh::Fresh => {
                        session.fresh_session();
                        queued_account = None;
                        halted = false;
                    }
                    Refresh::Reconnect => {
                        session.restart_retries();
                        halted = false;
                    }
                    Refresh::None => {}
                }
            }
            Action::Account(Some(operation)) => queued_account = Some(operation),
            Action::Step(Ok(())) => {}
            Action::Step(Err(CloseReason::RetryAt(_))) => {}
            Action::Step(Err(reason)) => {
                session.close(reason);
                if let Some(code) = session.refusal() {
                    host.observe(serde_json::json!({"state": "refused", "code": code, "error": refusal_message(code)}));
                } else if session.next_retry() == Err(CloseReason::RetryExhausted) {
                    host.observe(serde_json::json!({"state": "retry_exhausted", "error": "Reconnect attempts exhausted. Connect relay to retry."}));
                } else {
                    host.observe(serde_json::json!({"state": "disconnected"}));
                }
            }
        }
        if let Some(mut operation) = queued_account {
            while let Ok(next) = pending.try_recv() {
                operation = next;
            }
            halted = operation == AccountOperation::SignOut;
            session.desire_account(operation);
        }
    }
    session.close(CloseReason::Cancelled);
}

#[cfg(test)]
mod tests;
