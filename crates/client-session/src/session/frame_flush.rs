//! Reserves each frame's delimiter before accepting its first command.

use std::time::Duration;
use tokio::sync::mpsc;

use super::{BatchSendError, NetworkCommand};

/// Creates a bounded command queue with one extra slot for the frame delimiter.
pub(super) fn command_channel(
    capacity: usize,
) -> (mpsc::Sender<NetworkCommand>, mpsc::Receiver<NetworkCommand>) {
    assert!(capacity > 0, "command capacity must be nonzero");
    mpsc::channel(capacity.checked_add(1).expect("command capacity overflow"))
}

/// Command admission and the reserved delimiter, protected by the handle's frame lock.
#[derive(Default)]
pub(super) struct FrameFlush {
    delimiter: Option<mpsc::OwnedPermit<NetworkCommand>>,
    dirty: bool,
    simulation_remainder: Duration,
    cache_opportunity: bool,
}

impl FrameFlush {
    /// Records simulation progress even when player physics cannot emit a packet.
    pub(super) fn advance_simulation(&mut self, elapsed: Duration) {
        let elapsed = self.simulation_remainder + elapsed.min(simulation_clock::MAX_FRAME_ELAPSED);
        self.cache_opportunity |= elapsed >= simulation_clock::TICK_DURATION;
        self.simulation_remainder = Duration::from_nanos(
            (elapsed.as_nanos() % simulation_clock::TICK_DURATION.as_nanos()) as u64,
        );
    }

    /// Reserves commands and their delimiter without waiting for queue capacity.
    /// The caller holds the frame lock until every returned permit has been sent.
    pub(super) fn reserve<'a>(
        &mut self,
        commands: &'a mpsc::Sender<NetworkCommand>,
        count: usize,
    ) -> Result<mpsc::PermitIterator<'a, NetworkCommand>, BatchSendError> {
        if self.delimiter.is_none() {
            self.delimiter = Some(commands.clone().try_reserve_owned().map_err(
                |error| match error {
                    mpsc::error::TrySendError::Full(_) => BatchSendError::Full,
                    mpsc::error::TrySendError::Closed(_) => BatchSendError::Closed,
                },
            )?);
        }
        let permits = commands
            .try_reserve_many(count)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(()) => BatchSendError::Full,
                mpsc::error::TrySendError::Closed(()) => BatchSendError::Closed,
            })?;
        self.dirty = true;
        Ok(permits)
    }

    /// Submits the delimiter after all accepted commands, even when the packet slots are full.
    pub(super) fn flush(&mut self, commands: &mpsc::Sender<NetworkCommand>) {
        if !self.dirty && !self.cache_opportunity {
            return;
        }
        if self.delimiter.is_none() {
            let Ok(permit) = commands.clone().try_reserve_owned() else {
                // Coalesce opportunities until a later frame can reserve the delimiter.
                return;
            };
            self.delimiter = Some(permit);
        }
        self.dirty = false;
        let _ = self
            .delimiter
            .take()
            .expect("accepted commands reserve their delimiter")
            .send(NetworkCommand::FlushFrame {
                cache_opportunity: std::mem::take(&mut self.cache_opportunity),
            });
    }

    /// Reserved capacity that is not an enqueued command.
    pub(super) fn reserved_slots(&self) -> usize {
        usize::from(self.delimiter.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_simulation_progress_submits_clean_frames_without_render_rate_opportunities() {
        let (commands, mut receiver) = command_channel(2);
        let mut frame = FrameFlush::default();
        let quarter = simulation_clock::TICK_DURATION / 4;
        for _ in 0..3 {
            frame.advance_simulation(quarter);
            frame.flush(&commands);
            assert!(receiver.try_recv().is_err());
        }
        frame.advance_simulation(quarter);
        frame.flush(&commands);
        assert!(matches!(
            receiver.try_recv(),
            Ok(NetworkCommand::FlushFrame {
                cache_opportunity: true
            })
        ));
        frame.flush(&commands);
        assert!(receiver.try_recv().is_err());
        frame.advance_simulation(simulation_clock::MAX_FRAME_ELAPSED);
        frame.flush(&commands);
        assert!(matches!(
            receiver.try_recv(),
            Ok(NetworkCommand::FlushFrame {
                cache_opportunity: true
            })
        ));
        assert!(
            receiver.try_recv().is_err(),
            "catch-up progress coalesces to one observed opportunity"
        );
    }

    #[test]
    fn a_full_queue_retains_and_coalesces_clean_frame_opportunities() {
        let (commands, mut receiver) = command_channel(1);
        commands.try_send(NetworkCommand::FinishLoading).unwrap();
        commands.try_send(NetworkCommand::FinishLoading).unwrap();
        let mut frame = FrameFlush::default();
        for _ in 0..3 {
            frame.advance_simulation(simulation_clock::TICK_DURATION);
            frame.flush(&commands);
        }
        receiver.try_recv().unwrap();
        frame.flush(&commands);
        receiver.try_recv().unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Ok(NetworkCommand::FlushFrame {
                cache_opportunity: true
            })
        ));
        frame.flush(&commands);
        assert!(receiver.try_recv().is_err());
    }
}
