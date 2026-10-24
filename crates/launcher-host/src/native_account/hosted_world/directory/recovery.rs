//! Restores one host member without replacing publication, approval intent or peer IO.

use super::*;
use crate::native_account::directory::{cleanup_pending, interrupted};

pub(super) struct Recovery {
    phase: Phase,
    pub(super) candidate: Option<mpsd::Binding>,
    pending: bool,
}

#[derive(Clone, Copy)]
enum Phase {
    Drain,
    Restore,
    Bind,
    Sync,
}

impl Recovery {
    /// An uncertain dispatched rebind remains a cleanup obligation even after a closed snapshot.
    pub(super) fn pending(&self) -> bool {
        self.pending
    }
}

impl HostDirectory {
    /// Retains the first service cutoff across interruption and canceled borrows.
    pub(super) fn control_deadline(&mut self, until: Option<Instant>) -> Instant {
        let deadline = self
            .refresh_deadline
            .get_or_insert_with(|| Instant::now() + super::super::super::ACQUISITION_TIMEOUT);
        if let Some(until) = until {
            *deadline = (*deadline).min(until);
        }
        *deadline
    }

    /// Withdraws approval immediately; only the original physical terminal can authorize recovery.
    pub(super) fn retain_interruption(&mut self, error: io::Error) -> io::Result<()> {
        if !interrupted(&error) || self.closing || self.closed || self.session.is_none() {
            return Err(error);
        }
        self.subscribed = false;
        self.changed = true;
        if let Some(approvals) = &mut self.approvals {
            approvals.invalidate();
        }
        self.control_deadline(None);
        let recovery = self.recovery.get_or_insert(Recovery {
            phase: Phase::Drain,
            candidate: None,
            pending: false,
        });
        recovery.phase = Phase::Drain;
        Ok(())
    }

    /// Completes refresh/recovery under one cutoff without exposing an application result in between.
    pub(super) async fn refresh_control(&mut self, deadline: Instant) -> io::Result<()> {
        while self.changed || self.recovery.is_some() {
            check_deadline(deadline)?;
            let result = if self.recovery.is_some() {
                self.recover_membership(deadline).await
            } else {
                self.refresh(deadline).await
            };
            if let Err(error) = result {
                self.retain_interruption(error)?;
            }
        }
        Ok(())
    }

    /// Reuses the existing socket's desired subscription and the same account/member/proof owner.
    async fn recover_membership(&mut self, deadline: Instant) -> io::Result<()> {
        loop {
            self.active()?;
            check_deadline(deadline)?;
            match self
                .recovery
                .as_ref()
                .expect("retained host recovery")
                .phase
            {
                Phase::Drain => {
                    loop {
                        match bounded(deadline, self.socket.as_mut().expect("host socket").drive())
                            .await?
                        {
                            Err(
                                SocketError::Closed
                                | SocketError::Transport
                                | SocketError::TimedOut,
                            ) => break,
                            Err(error) => return Err(socket_error(error)),
                            Ok(event) => {
                                if let Err(error) = event_changed(
                                    event,
                                    self.subscription.expect("host subscription"),
                                    &self.reference,
                                    &mut self.changed,
                                ) && !interrupted(&error)
                                {
                                    return Err(error);
                                }
                            }
                        }
                    }
                    self.recovery.as_mut().unwrap().phase = Phase::Restore;
                }
                Phase::Restore => {
                    let mut operation = self.operation(deadline).await?;
                    if self.recovery.as_ref().unwrap().candidate.is_some() {
                        self.settle_rebind(&mut operation, deadline).await?;
                    }
                    // Cancellation aborts partial SDK IO; drain that retained terminal on re-entry.
                    self.recovery.as_mut().unwrap().phase = Phase::Drain;
                    let events = bounded(
                        deadline,
                        operation
                            .minecraft()?
                            .recover_rta(self.socket.as_mut().expect("host socket"), deadline),
                    )
                    .await?
                    .map_err(|_| invalid("host realtime recovery failed"))?;
                    self.scope
                        .as_ref()
                        .expect("host account scope")
                        .check(&operation)?;
                    if !operation
                        .minecraft()?
                        .owns_rta(self.socket.as_ref().unwrap().connection())
                    {
                        return Err(invalid("host RTA proof owner changed"));
                    }
                    let mut acknowledged = false;
                    for event in events {
                        if matches!(&event, rta::Event::Subscribed { subscription, .. }
                            if Some(*subscription) == self.subscription)
                        {
                            acknowledged = true;
                        }
                        event_changed(
                            event,
                            self.subscription.unwrap(),
                            &self.reference,
                            &mut self.changed,
                        )?;
                    }
                    if !acknowledged {
                        return Err(invalid("host directory was not resubscribed"));
                    }
                    let candidate = self.live_binding()?;
                    let recovery = self.recovery.as_mut().unwrap();
                    recovery.candidate = Some(candidate);
                    recovery.phase = Phase::Bind;
                    operation.persist()?;
                }
                Phase::Bind => {
                    let mut operation = self.operation(deadline).await?;
                    if self.session.as_ref().is_some_and(mpsd::Session::needs_sync) {
                        self.sync_recovery(&mut operation, deadline).await?;
                    }
                    let session = self.session.as_ref().expect("original host membership");
                    if !session.reference().same(&self.reference)
                        || !self
                            .binding
                            .is_some_and(|binding| session.is_bound(binding))
                    {
                        return Err(cleanup_pending());
                    }
                    let recovery = self.recovery.as_mut().unwrap();
                    let candidate = recovery.candidate.expect("acknowledged host binding");
                    recovery.phase = Phase::Sync;
                    recovery.pending = true;
                    let result = service(
                        self.socket.as_mut().unwrap(),
                        self.subscription.unwrap(),
                        &self.reference,
                        &mut self.changed,
                        deadline,
                        operation.minecraft()?.mpsd().bind(
                            self.session.as_mut().expect("original host membership"),
                            candidate,
                        ),
                    )
                    .await?
                    .map_err(directory_error)?;
                    self.recovery.as_mut().unwrap().pending = false;
                    if !matches!(result, mpsd::Update::Updated | mpsd::Update::Unchanged) {
                        return Err(cleanup_pending());
                    }
                    operation.persist()?;
                }
                Phase::Sync => {
                    let mut operation = self.operation(deadline).await?;
                    self.sync_recovery(&mut operation, deadline).await?;
                    let session = self.session.as_ref().expect("original host membership");
                    let candidate = self.recovery.as_ref().unwrap().candidate.unwrap();
                    if !session.reference().same(&self.reference) || !session.is_bound(candidate) {
                        return Err(cleanup_pending());
                    }
                    // Cleanup follows the confirmed member even if subsequent title approval fails.
                    self.binding = Some(candidate);
                    self.recovery = None;
                    self.subscribed = true;
                    self.validate_session()?;
                    self.observe()?;
                    if !self.changed {
                        if self.pending.is_some() {
                            let _ = self.confirm_pending();
                        } else {
                            let _ = self.confirm_retained();
                        }
                    }
                    operation.persist()?;
                    return Ok(());
                }
            }
        }
    }

    /// Refreshes the original member before admission; a live-binding check waits for rebind confirmation.
    async fn sync_recovery(
        &mut self,
        operation: &mut Operation,
        deadline: Instant,
    ) -> io::Result<()> {
        let mut changed = false;
        service(
            self.socket.as_mut().expect("host socket"),
            self.subscription.expect("host subscription"),
            &self.reference,
            &mut changed,
            deadline,
            operation
                .minecraft()?
                .mpsd()
                .sync(self.session.as_mut().expect("host membership")),
        )
        .await?
        .map_err(directory_error)?;
        self.changed = changed;
        Ok(())
    }

    /// Keeps old/candidate cleanup facts until a complete snapshot settles an uncertain rebind.
    pub(super) async fn settle_rebind(
        &mut self,
        operation: &mut Operation,
        deadline: Instant,
    ) -> io::Result<()> {
        let recovery = self.recovery.as_ref().expect("retained host recovery");
        let candidate = recovery.candidate.expect("retained host candidate");
        let pending = recovery.pending;
        let session = self.session.as_mut().expect("original host membership");
        if session.is_closed() && pending {
            *session = bounded(
                deadline,
                operation.minecraft()?.mpsd().read(&self.reference),
            )
            .await?
            .map_err(|_| cleanup_pending())?;
        } else if !session.is_closed() {
            bounded(deadline, operation.minecraft()?.mpsd().sync(session))
                .await?
                .map_err(directory_error)?;
        }
        if session.is_bound(candidate) {
            self.binding = Some(candidate);
        } else if pending
            || (!session.is_closed()
                && !self
                    .binding
                    .is_some_and(|binding| session.is_bound(binding)))
        {
            return Err(cleanup_pending());
        }
        let recovery = self.recovery.as_mut().unwrap();
        recovery.candidate = None;
        recovery.pending = false;
        Ok(())
    }
}
