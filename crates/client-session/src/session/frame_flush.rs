//! Reserves each frame's delimiter before accepting its first command.

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
}

impl FrameFlush {
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
    pub(super) fn flush(&mut self) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let _ = self
            .delimiter
            .take()
            .expect("accepted commands reserve their delimiter")
            .send(NetworkCommand::FlushFrame);
    }

    /// Reserved capacity that is not an enqueued command.
    pub(super) fn reserved_slots(&self) -> usize {
        usize::from(self.delimiter.is_some())
    }
}
