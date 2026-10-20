//! One prepared accepted-Nether effect, retained on the original codec/cipher owner.

use super::Connection;
use bytes::Bytes;
use futures::future::BoxFuture;
use nethernet_tokio::{SendPermit, session::Channel};
use std::{io, sync::Arc, time::Instant, vec::IntoIter};

/// The original effect's local admission result after new sends have been sealed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeAcceptingSendOutcome {
    /// No prepared effect existed.
    Idle,
    /// Every original message received its successful local core admission receipt.
    Complete,
    /// An uncommitted remainder was discarded; no complete-effect receipt is promised.
    Incomplete,
}

pub(super) struct PendingSend {
    deadline: Instant,
    messages: IntoIter<Bytes>,
    phase: Phase,
    failure: Option<Arc<io::Error>>,
    discarded: bool,
}

enum Phase {
    NeedCapacity,
    Reserving(BoxFuture<'static, io::Result<SendPermit>>),
    Reserved(SendPermit),
    Receipt(BoxFuture<'static, io::Result<()>>),
    Complete,
    Stopped,
}

pub(super) struct SendRetirement {
    deadline: Instant,
    outcome: Option<NativeAcceptingSendOutcome>,
}

impl Connection {
    /// Keeps ordinary sends/settings from replacing an unfinished or terminal effect.
    pub(super) fn ensure_send_idle(&self) -> io::Result<()> {
        if self.send_sealed {
            return Err(closed());
        }
        if self.pending_send.is_some() {
            return Err(invalid("native accepting send is still retained"));
        }
        Ok(())
    }

    /// Moves the original encoded messages into the sole retained send slot.
    pub(in crate::login::native_network) fn prepare_send(
        &mut self,
        body: Bytes,
        deadline: Instant,
    ) -> io::Result<()> {
        self.ensure_send_idle()?;
        check_deadline(deadline)?;
        drop(self.transport.accepted_sender()?);
        let messages = self.encode_body(body)?.into_iter();
        let phase = if messages.as_slice().is_empty() {
            Phase::Complete
        } else {
            Phase::NeedCapacity
        };
        self.pending_send = Some(PendingSend {
            deadline,
            messages,
            phase,
            failure: None,
            discarded: false,
        });
        Ok(())
    }

    /// Borrows the first absolute cutoff without replacing any operation state.
    pub(in crate::login::native_network) fn prepared_deadline(&self) -> io::Result<Instant> {
        Ok(self
            .pending_send
            .as_ref()
            .ok_or_else(|| invalid("no native accepting send is retained"))?
            .deadline)
    }

    /// Settles the exact previous receipt, then reserves one original actor command slot.
    pub(in crate::login::native_network) async fn next_send_capacity(
        &mut self,
    ) -> io::Result<bool> {
        loop {
            let pending = self
                .pending_send
                .as_mut()
                .ok_or_else(|| invalid("no native accepting send is retained"))?;
            if let Some(error) = &pending.failure {
                return Err(repeat(error));
            }
            if matches!(pending.phase, Phase::Complete) {
                return Ok(false);
            }
            if self.send_sealed {
                return Err(closed());
            }
            let deadline = pending.deadline;
            if let Err(error) = check_deadline(deadline) {
                return Err(self.fail_send(error));
            }
            match &mut pending.phase {
                Phase::NeedCapacity => {
                    if pending.messages.as_slice().is_empty() {
                        pending.phase = Phase::Complete;
                        continue;
                    }
                    let sender = self.transport.accepted_sender()?;
                    pending.phase = Phase::Reserving(Box::pin(async move {
                        sender
                            .reserve_send(Some(deadline))
                            .await
                            .map_err(io::Error::other)
                    }));
                }
                Phase::Reserving(reservation) => {
                    let result = tokio::time::timeout_at(
                        deadline.into(),
                        self.transport.progress_accepted(reservation.as_mut()),
                    )
                    .await;
                    match result {
                        Ok(Ok(Ok(permit))) => pending.phase = Phase::Reserved(permit),
                        Ok(Ok(Err(error))) => {
                            pending.phase = Phase::Stopped;
                            return Err(self.fail_send(error));
                        }
                        Ok(Err(error)) => return Err(self.fail_send(error)),
                        Err(_) => return Err(self.fail_send(expired())),
                    }
                }
                Phase::Reserved(_) => return Ok(true),
                Phase::Receipt(receipt) => {
                    let result = tokio::time::timeout_at(
                        deadline.into(),
                        self.transport.progress_accepted(receipt.as_mut()),
                    )
                    .await;
                    match result {
                        Ok(Ok(Ok(()))) => {
                            pending.phase = if pending.messages.as_slice().is_empty() {
                                Phase::Complete
                            } else {
                                Phase::NeedCapacity
                            };
                            if let Err(error) = check_deadline(deadline) {
                                return Err(self.fail_send(error));
                            }
                        }
                        Ok(Ok(Err(error))) => {
                            pending.phase = Phase::Stopped;
                            return Err(self.fail_send(error));
                        }
                        Ok(Err(error)) => return Err(self.fail_send(error)),
                        Err(_) => return Err(self.fail_send(expired())),
                    }
                }
                Phase::Complete => return Ok(false),
                Phase::Stopped => return Err(closed()),
            }
        }
    }

    /// Reborrows only a retained granted permit, preserving its original deadline and failure.
    pub(in crate::login::native_network) fn reserved_send_capacity(&mut self) -> io::Result<()> {
        if self.send_sealed {
            return Err(closed());
        }
        let pending = self
            .pending_send
            .as_mut()
            .ok_or_else(|| invalid("no native accepting send is retained"))?;
        if let Some(error) = &pending.failure {
            return Err(repeat(error));
        }
        if let Err(error) = check_deadline(pending.deadline) {
            return Err(self.fail_send(error));
        }
        if !matches!(pending.phase, Phase::Reserved(_)) {
            return Err(invalid("native accepting send has no capacity"));
        }
        Ok(())
    }

    /// Encrypts once and synchronously commits the retained permit; its receipt stays owned here.
    pub(in crate::login::native_network) fn commit_send_capacity(&mut self) -> io::Result<()> {
        self.reserved_send_capacity()?;
        let pending = self.pending_send.as_mut().expect("checked retained effect");
        let Phase::Reserved(permit) = std::mem::replace(&mut pending.phase, Phase::Stopped) else {
            unreachable!("checked retained permit")
        };
        let message = pending
            .messages
            .next()
            .expect("reserved capacity has one original message");
        let result = self.encrypt_message(message).and_then(|message| {
            permit
                .commit(Channel::Reliable, message)
                .map_err(io::Error::other)
        });
        match result {
            Ok(receipt) => {
                self.pending_send
                    .as_mut()
                    .expect("one retained effect")
                    .phase = Phase::Receipt(Box::pin(async move {
                    receipt.acknowledged().await.map_err(io::Error::other)
                }));
                Ok(())
            }
            Err(error) => Err(self.fail_send(error)),
        }
    }

    /// Releases only a fully acknowledged original effect, including after local sealing.
    pub(in crate::login::native_network) fn finish_send(&mut self) -> io::Result<()> {
        let pending = self
            .pending_send
            .as_ref()
            .ok_or_else(|| invalid("no native accepting send is retained"))?;
        if let Some(error) = &pending.failure {
            return Err(repeat(error));
        }
        if !matches!(pending.phase, Phase::Complete) {
            return Err(invalid("native accepting send is incomplete"));
        }
        self.pending_send = None;
        Ok(())
    }

    /// Seals local commits and discards only work that has not reached synchronous commit.
    pub(in crate::login::native_network) fn retire_send(&mut self, deadline: Instant) {
        self.send_sealed = true;
        if self.send_retirement.is_some() {
            return;
        }
        let deadline = self
            .pending_send
            .as_ref()
            .map_or(deadline, |pending| pending.deadline.min(deadline));
        let outcome = match self.pending_send.as_mut() {
            None => Some(NativeAcceptingSendOutcome::Idle),
            Some(pending) if matches!(pending.phase, Phase::Complete) => {
                Some(NativeAcceptingSendOutcome::Complete)
            }
            Some(pending) => {
                pending.discarded |= !pending.messages.as_slice().is_empty();
                pending.messages = Vec::new().into_iter();
                if matches!(pending.phase, Phase::Receipt(_)) {
                    None
                } else {
                    pending.discarded = true;
                    pending.phase = Phase::Stopped;
                    Some(NativeAcceptingSendOutcome::Incomplete)
                }
            }
        };
        self.send_retirement = Some(SendRetirement { deadline, outcome });
    }

    /// Finishes only the original committed receipt under the first cleanup cutoff.
    pub(in crate::login::native_network) async fn settle_send(
        &mut self,
    ) -> io::Result<NativeAcceptingSendOutcome> {
        loop {
            let retirement = self
                .send_retirement
                .as_ref()
                .ok_or_else(|| invalid("native accepting send is not retiring"))?;
            if let Some(outcome) = retirement.outcome {
                if let Some(error) = self
                    .pending_send
                    .as_ref()
                    .and_then(|pending| pending.failure.as_ref())
                {
                    return Err(repeat(error));
                }
                return Ok(outcome);
            }
            let deadline = retirement.deadline;
            if check_deadline(deadline).is_err() {
                self.pending_send
                    .as_mut()
                    .expect("retiring committed effect")
                    .phase = Phase::Stopped;
                self.send_retirement
                    .as_mut()
                    .expect("first retirement")
                    .outcome = Some(NativeAcceptingSendOutcome::Incomplete);
                return Err(self.fail_send(expired()));
            }
            let pending = self
                .pending_send
                .as_mut()
                .expect("retiring committed effect");
            let Phase::Receipt(receipt) = &mut pending.phase else {
                unreachable!("only an original receipt remains during settlement")
            };
            let result = tokio::time::timeout_at(
                deadline.into(),
                self.transport.settle_accepted(receipt.as_mut()),
            )
            .await;
            match result {
                Ok(Ok(Ok(()))) => {
                    pending.phase = Phase::Stopped;
                    if let Err(error) = check_deadline(deadline) {
                        self.send_retirement
                            .as_mut()
                            .expect("first retirement")
                            .outcome = Some(NativeAcceptingSendOutcome::Incomplete);
                        return Err(self.fail_send(error));
                    }
                    let outcome = if pending.discarded {
                        NativeAcceptingSendOutcome::Incomplete
                    } else {
                        NativeAcceptingSendOutcome::Complete
                    };
                    pending.phase = if pending.discarded {
                        Phase::Stopped
                    } else {
                        Phase::Complete
                    };
                    self.send_retirement
                        .as_mut()
                        .expect("first retirement")
                        .outcome = Some(outcome);
                }
                Ok(Ok(Err(error))) => {
                    pending.phase = Phase::Stopped;
                    self.send_retirement
                        .as_mut()
                        .expect("first retirement")
                        .outcome = Some(NativeAcceptingSendOutcome::Incomplete);
                    return Err(self.fail_send(error));
                }
                Ok(Err(error)) => {
                    drop(self.fail_send(error));
                }
                Err(_) => {
                    pending.phase = Phase::Stopped;
                    self.send_retirement
                        .as_mut()
                        .expect("first retirement")
                        .outcome = Some(NativeAcceptingSendOutcome::Incomplete);
                    return Err(self.fail_send(expired()));
                }
            }
        }
    }

    /// Caches the first cause without replacing a pending receipt or admitting new output.
    fn fail_send(&mut self, error: io::Error) -> io::Error {
        self.send_sealed = true;
        let pending = self
            .pending_send
            .as_mut()
            .expect("failure belongs to retained effect");
        repeat(pending.failure.get_or_insert_with(|| Arc::new(error)))
    }
}

/// Uses the same native adapter clock as the original Nether command admission.
fn check_deadline(deadline: Instant) -> io::Result<()> {
    if tokio::time::Instant::now().into_std() >= deadline {
        Err(expired())
    } else {
        Ok(())
    }
}

/// Repeats the original error class and cause without duplicating payload state.
fn repeat(error: &Arc<io::Error>) -> io::Error {
    io::Error::new(error.kind(), Arc::clone(error))
}

/// Reports an invalid local phase without exposing packet or identity data.
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Reports local send retirement without inspecting remote application state.
fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "native accepting send is sealed",
    )
}

/// Preserves the first finite operation cutoff.
fn expired() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "native accepting send deadline expired",
    )
}
