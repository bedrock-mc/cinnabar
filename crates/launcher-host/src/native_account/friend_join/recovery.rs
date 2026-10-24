//! Rebinds one existing member after physical RTA loss, without replacing its game or join.

use super::*;
use crate::native_account::directory::interrupted;

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

impl FriendJoin {
    /// Keeps the first refresh/recovery deadline on this owner across every canceled borrow.
    pub(super) async fn drive_membership(&mut self, until: Option<Instant>) -> io::Result<()> {
        if !self.admitted || self.closing || self.closed {
            return Err(invalid("friend membership is not active"));
        }
        if let Some(deadline) = until {
            check_deadline(deadline)?;
        }
        if self.recovery.is_none() && !self.changed {
            let result = if let Some(deadline) = until {
                self.notification(deadline).await
            } else {
                let result = self
                    .socket
                    .as_mut()
                    .ok_or_else(|| invalid("friend RTA is unavailable"))?
                    .drive()
                    .await;
                match result {
                    Ok(event) => event_changed(
                        event,
                        self.subscription
                            .ok_or_else(|| invalid("friend subscription is unavailable"))?,
                        &self.selection.reference,
                        &mut self.changed,
                    ),
                    Err(error) => Err(socket_error(error)),
                }
            };
            if let Err(error) = result {
                self.retain_interruption(error)?;
            }
        }
        if self.changed || self.recovery.is_some() {
            let deadline = self.active_refresh_deadline.get_or_insert_with(|| {
                until.unwrap_or_else(|| Instant::now() + super::super::ACQUISITION_TIMEOUT)
            });
            if let Some(until) = until {
                *deadline = (*deadline).min(until);
            }
            let deadline = *deadline;
            loop {
                check_deadline(deadline)?;
                let result = if self.recovery.is_some() {
                    self.recover_membership(deadline).await
                } else if self.changed {
                    self.refresh(deadline).await
                } else {
                    break;
                };
                if let Err(error) = result {
                    self.retain_interruption(error)?;
                }
            }
        }
        self.current_admission()?;
        self.active_refresh_deadline = None;
        Ok(())
    }

    /// Withdraws admission immediately; an actual terminal must still validate this provisional loss.
    fn retain_interruption(&mut self, error: io::Error) -> io::Result<()> {
        if !interrupted(&error) || self.closing || self.closed {
            return Err(error);
        }
        self.subscribed = false;
        self.changed = true;
        let recovery = self.recovery.get_or_insert(Recovery {
            phase: Phase::Drain,
            candidate: None,
            pending: false,
        });
        recovery.phase = Phase::Drain;
        Ok(())
    }

    /// Reuses the same account/socket/desired handle and reconciles each uncertain binding before reuse.
    async fn recover_membership(&mut self, deadline: Instant) -> io::Result<()> {
        loop {
            check_deadline(deadline)?;
            match self.recovery.as_ref().expect("retained recovery").phase {
                Phase::Drain => {
                    loop {
                        let event = bounded(
                            deadline,
                            self.socket.as_mut().expect("original RTA socket").drive(),
                        )
                        .await?;
                        match event {
                            Err(
                                SocketError::Closed
                                | SocketError::Transport
                                | SocketError::TimedOut,
                            ) => break,
                            Err(error) => return Err(socket_error(error)),
                            Ok(event) => {
                                if let Err(error) = event_changed(
                                    event,
                                    self.subscription.expect("original directory subscription"),
                                    &self.selection.reference,
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
                    // SDK cancellation aborts partial I/O. Re-enter through its retained terminal,
                    // never through an undrained completion or an overwritten candidate binding.
                    self.recovery.as_mut().unwrap().phase = Phase::Drain;
                    let events = bounded(
                        deadline,
                        operation.minecraft()?.recover_rta(
                            self.socket.as_mut().expect("original RTA socket"),
                            deadline,
                        ),
                    )
                    .await?
                    .map_err(|_| invalid("friend-world realtime recovery failed"))?;
                    self.scope
                        .as_ref()
                        .expect("original account scope")
                        .check(&operation)?;
                    if !operation
                        .minecraft()?
                        .owns_rta(self.socket.as_ref().unwrap().connection())
                    {
                        return Err(invalid("friend-world proof owner changed"));
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
                            &self.selection.reference,
                            &mut self.changed,
                        )?;
                    }
                    if !acknowledged {
                        return Err(invalid("friend directory was not resubscribed"));
                    }
                    let candidate = self
                        .socket
                        .as_ref()
                        .unwrap()
                        .connection()
                        .directory_binding(self.subscription.unwrap(), self.session_subscription)
                        .map_err(rta_error)?;
                    let recovery = self.recovery.as_mut().unwrap();
                    recovery.candidate = Some(candidate);
                    recovery.phase = Phase::Bind;
                    operation.persist()?;
                }
                Phase::Bind => {
                    let mut operation = self.operation(deadline).await?;
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.needs_sync())
                    {
                        self.refresh_owned(&mut operation, deadline).await?;
                    }
                    let session = self.session.as_ref().expect("original membership");
                    if !session.reference().same(&self.selection.reference)
                        || !self
                            .binding
                            .is_some_and(|binding| session.is_bound(binding))
                    {
                        return Err(cleanup_pending());
                    }
                    let recovery = self.recovery.as_mut().unwrap();
                    let candidate = recovery.candidate.expect("fresh acknowledged binding");
                    recovery.phase = Phase::Sync;
                    recovery.pending = true;
                    let result = service(
                        self.socket.as_mut().unwrap(),
                        self.subscription.unwrap(),
                        &self.selection.reference,
                        &mut self.changed,
                        deadline,
                        operation.minecraft()?.mpsd().bind(
                            self.session.as_mut().expect("original membership"),
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
                    self.refresh_owned(&mut operation, deadline).await?;
                    let session = self.session.as_ref().expect("original membership");
                    let candidate = self.recovery.as_ref().unwrap().candidate.unwrap();
                    if !session.reference().same(&self.selection.reference)
                        || !session.is_bound(candidate)
                    {
                        return Err(cleanup_pending());
                    }
                    // Commit cleanup ownership independently of title admission. A later host/nonce
                    // rejection must still leave the real rebound member removable.
                    self.binding = Some(candidate);
                    self.recovery = None;
                    self.subscribed = true;
                    self.current_admission()?;
                    operation.persist()?;
                    return Ok(());
                }
            }
        }
    }

    /// Resolves only the retained old/candidate binding; an old snapshot cannot disprove a pending PUT.
    pub(super) async fn settle_rebind(
        &mut self,
        operation: &mut Operation,
        deadline: Instant,
    ) -> io::Result<()> {
        let recovery = self.recovery.as_ref().expect("retained recovery");
        let candidate = recovery.candidate.expect("retained rebind candidate");
        let pending = recovery.pending;
        let session = self.session.as_mut().expect("original membership");
        if session.is_closed() && pending {
            let current = bounded(
                deadline,
                operation
                    .minecraft()?
                    .mpsd()
                    .read(&self.selection.reference),
            )
            .await?
            .map_err(|_| cleanup_pending())?;
            **session = current;
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
