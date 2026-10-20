//! Borrowed send capabilities over the original accepted connection's retained effect.

use super::NativeAcceptingIo;
use crate::login::native_network::Connection;
use bedrock_protocol_1_26_51::packets;
use bytes::Bytes;
use std::{io, time::Instant};

pub use super::super::connection::send::NativeAcceptingSendOutcome;

/// Borrowed progress for one encoded effect. Dropping this borrow retains its exact progress.
#[must_use = "drive or explicitly retire the original retained send"]
pub struct NativeAcceptingSend<'a> {
    pub(in crate::login::native_network) connection: &'a mut Connection,
}

/// A borrowed original queue permit. Application authority must be current at commit.
/// Dropping this borrow retains the actual permit on its original connection.
#[must_use = "commit after checking current authority, or retain the original effect"]
pub struct NativeAcceptingCapacity<'a> {
    connection: &'a mut Connection,
}

/// Local commits are sealed; only an already committed original receipt can finish.
/// Dropping this borrow retains that receipt and the first cleanup cutoff.
#[must_use = "settle the original receipt before normal global transport retirement"]
pub struct NativeAcceptingSendRetirement<'a> {
    pub(in crate::login::native_network) connection: &'a mut Connection,
}

impl NativeAcceptingIo {
    /// Encodes typed effects once without changing the cipher or admitting a command.
    /// Rejects another retained effect or a locally retired send direction.
    pub fn prepare_packets(
        &mut self,
        packets: Vec<packets::Packet>,
        deadline: Instant,
    ) -> io::Result<NativeAcceptingSend<'_>> {
        let packets: Vec<_> = packets.into_iter().map(crate::Packet::new).collect();
        let body = if packets.is_empty() {
            Bytes::new()
        } else {
            crate::session_transport::batch_body(&packets).ok_or_else(super::invalid)?
        };
        self.connection.prepare_send(body, deadline)?;
        Ok(NativeAcceptingSend {
            connection: &mut self.connection,
        })
    }

    /// Encodes one original complete raw packet without changing its bytes or cipher.
    pub fn prepare_raw(
        &mut self,
        packet: Bytes,
        deadline: Instant,
    ) -> io::Result<NativeAcceptingSend<'_>> {
        let body = self
            .connection
            .raw_body(packet)
            .map_err(|_| super::invalid())?;
        self.connection.prepare_send(body, deadline)?;
        Ok(NativeAcceptingSend {
            connection: &mut self.connection,
        })
    }

    /// Resumes the exact cursor, permit or receipt retained by an earlier borrowed operation.
    pub fn resume_send(&mut self) -> io::Result<NativeAcceptingSend<'_>> {
        self.connection.prepared_deadline()?;
        Ok(NativeAcceptingSend {
            connection: &mut self.connection,
        })
    }

    /// Seals new local commits and caps settlement by the first cleanup and original send cutoffs.
    /// This does not close the association. Settle its receipt before calling begin_close.
    pub fn begin_send_retirement(
        &mut self,
        deadline: Instant,
    ) -> NativeAcceptingSendRetirement<'_> {
        self.connection.retire_send(deadline);
        NativeAcceptingSendRetirement {
            connection: &mut self.connection,
        }
    }
}

impl NativeAcceptingSend<'_> {
    /// The first original capacity/admission/edge-wait cutoff, unchanged by cancellation.
    pub fn deadline(&self) -> Instant {
        self.connection
            .prepared_deadline()
            .expect("borrowed send retains its effect")
    }

    /// Settles the previous receipt and borrows the next original queue permit.
    /// Keep the application grant check and synchronous commit adjacent, without awaiting.
    /// None proves all original local core receipts succeeded; it does not prove peer processing.
    pub async fn next(&mut self) -> io::Result<Option<NativeAcceptingCapacity<'_>>> {
        if self.connection.next_send_capacity().await? {
            Ok(Some(NativeAcceptingCapacity {
                connection: self.connection,
            }))
        } else {
            Ok(None)
        }
    }

    /// Reborrows the original permit already granted by `next` after an owned IO handoff.
    /// This neither reserves capacity nor changes the cipher; a pending or retired send is rejected.
    pub fn reserved_capacity(&mut self) -> io::Result<NativeAcceptingCapacity<'_>> {
        self.connection.reserved_send_capacity()?;
        Ok(NativeAcceptingCapacity {
            connection: self.connection,
        })
    }

    /// Releases only a completed effect after its caller's original continuation/receipt obligation.
    pub fn finish(self) -> io::Result<()> {
        self.connection.finish_send()
    }
}

impl NativeAcceptingCapacity<'_> {
    /// Advances the original cipher once and commits this original permit without yielding.
    /// Failure seals the direction after cipher progress; retrying plaintext is not permitted.
    pub fn commit(self) -> io::Result<()> {
        self.connection.commit_send_capacity()
    }
}

impl NativeAcceptingSendRetirement<'_> {
    /// Settles only already committed output under the retained first cutoff.
    /// Complete means local core admission for the whole original effect, not peer delivery.
    pub async fn finish(&mut self) -> io::Result<NativeAcceptingSendOutcome> {
        self.connection.settle_send().await
    }
}

#[cfg(test)]
pub(in crate::login::native_network) mod tests;
