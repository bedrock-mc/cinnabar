//! Owned peer IO pauses before each application admission; payloads remain with their original owners.

use super::*;
use bytes::Bytes;

pub(super) enum Phase {
    Receive,
    BackendFlush,
    BackendReceive,
    BackendCapacity,
    Forward,
    Capacity,
    Acknowledge,
}

pub(in crate::native_account::hosted_world) enum Step {
    External(Bytes),
    Capacity,
    BackendCapacity,
    Progress,
    Ended,
}

impl NativeHostedPeer {
    /// Retains the first in-flight effect cutoff while another peer or account operation is driven.
    pub(in crate::native_account::hosted_world) fn deadline(&self) -> Option<Instant> {
        match (self.output.deadline(), self.backend_deadline) {
            (Some(output), Some(backend)) => Some(output.min(backend)),
            (output, backend) => output.or(backend),
        }
    }

    /// Drives only this original wire/backend until its next fresh-authority boundary.
    /// The caller retains this actual future across unrelated service or peer progress.
    pub(in crate::native_account::hosted_world) async fn next_step(&mut self) -> io::Result<Step> {
        match self.phase {
            Phase::Receive => {
                let packet = tokio::select! {
                    packet = self.wire.receive_play() => return packet.map(Step::External),
                    packet = self.backend.recv() => packet?,
                };
                self.stage_output(packet)
            }
            Phase::BackendFlush => match self.backend.flush_send().await {
                Ok(BackendProgress::Complete) => {
                    self.backend_deadline = None;
                    self.phase = Phase::Receive;
                    Ok(Step::Progress)
                }
                Ok(BackendProgress::Read) => {
                    self.phase = Phase::BackendReceive;
                    Ok(Step::Progress)
                }
                Ok(BackendProgress::Capacity) => {
                    self.phase = Phase::BackendCapacity;
                    Ok(Step::BackendCapacity)
                }
                Err(error) => Err(error),
            },
            Phase::BackendReceive => {
                let packet = self.backend.recv().await?;
                self.stage_output(packet)
            }
            Phase::Forward => {
                let available = self.wire.resume_send()?.next().await?.is_some();
                if available {
                    self.phase = Phase::Capacity;
                    Ok(Step::Capacity)
                } else {
                    self.output.externally_complete();
                    self.phase = Phase::Acknowledge;
                    Ok(Step::Progress)
                }
            }
            Phase::Capacity => Ok(Step::Capacity),
            Phase::BackendCapacity => Ok(Step::BackendCapacity),
            Phase::Acknowledge => {
                let deadline = self.output.deadline().expect("delivered output retained");
                self.backend.received(deadline).await?;
                self.output.acknowledged(&mut self.wire)?;
                self.phase = if self.backend_deadline.is_some() {
                    Phase::BackendFlush
                } else {
                    Phase::Receive
                };
                Ok(Step::Progress)
            }
        }
    }

    /// Records actual backend delivery before returning its ready result to a service postcheck.
    fn stage_output(&mut self, packet: Option<Bytes>) -> io::Result<Step> {
        let Some(packet) = packet else {
            return Ok(Step::Ended);
        };
        self.output
            .stage(&mut self.wire, packet, cleanup_deadline())?;
        self.phase = Phase::Forward;
        Ok(Step::Progress)
    }

    /// Applies only a ready original IO result, immediately after the caller's current authority check.
    /// True ends play; admitted writes and cipher progress remain with this same peer.
    pub(in crate::native_account::hosted_world) fn apply(
        &mut self,
        step: Step,
    ) -> io::Result<bool> {
        match step {
            Step::External(packet) => {
                let deadline = cleanup_deadline();
                self.backend.queue_send(&packet, deadline)?;
                self.backend_deadline = Some(deadline);
                self.phase = Phase::BackendFlush;
            }
            Step::Capacity => {
                self.wire.resume_send()?.reserved_capacity()?.commit()?;
                self.phase = Phase::Forward;
            }
            Step::BackendCapacity => {
                self.backend.commit_capacity()?;
                self.phase = Phase::BackendFlush;
            }
            Step::Progress => {}
            Step::Ended => return Ok(true),
        }
        Ok(false)
    }
}
