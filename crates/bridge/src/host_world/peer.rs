//! Actual backend attach and initialized phases, followed by bounded stop-and-wait play IO.

use super::{
    Frame, Reader, Writer,
    contract::*,
    invalid,
    process::{admit_json, bounded, check_deadline},
};
use bytes::Bytes;
use std::{io, time::Instant};

/// Original backend reservation; it can serve pack ranges but cannot send play data.
pub struct PendingBackendPeer {
    reader: Reader,
    writer: Writer,
    read: Option<PendingRead>,
}

struct PendingRead {
    archive: u32,
    offset: u64,
    length: u32,
    deadline: Instant,
}

/// One admitted Attach, retaining its original socket halves and finite deadline.
#[must_use = "finish this admitted Attach or drop it to retire its socket"]
pub struct AttachingBackendPeer {
    reader: Reader,
    writer: Writer,
    deadline: Instant,
}

/// Actual Go world output, still awaiting verified client runtime-ID initialization.
pub struct BackendWorldReady {
    reader: Reader,
    writer: Writer,
    world: Bytes,
}

/// One admitted Initialized, retaining its original socket halves and finite deadline.
#[must_use = "finish this admitted Initialized or drop it to retire its socket"]
pub struct InitializingBackendPeer {
    reader: Reader,
    writer: Writer,
    deadline: Instant,
}

/// Initialized backend peer; no constructor can bypass the preceding private-process phases.
pub struct ActiveBackendPeer {
    reader: Reader,
    writer: Writer,
    pending: Option<Bytes>,
    pending_id: u64,
    receive_id: u64,
    send_id: u64,
    send: Option<PendingSend>,
    receipt_admitted: bool,
    delivered: bool,
}

#[derive(Clone, Copy)]
enum SendPhase {
    Packet,
    Flush,
    Receipt,
}

struct PendingSend {
    phase: SendPhase,
    deadline: Instant,
    sequence: u64,
}
/// Local play admission ended; original accepted payload may still drain before terminal closure.
pub struct ClosingBackendPeer {
    active: ActiveBackendPeer,
    deadline: Instant,
    sent: bool,
    finished: bool,
}

impl PendingBackendPeer {
    pub(super) fn new(reader: Reader, writer: Writer) -> Self {
        Self {
            reader,
            writer,
            read: None,
        }
    }

    /// Reads exactly the original accepting continuation's one bounded archive range.
    /// Cancellation retains that range, partial output and its first deadline; a different read is refused.
    pub async fn read_pack(
        &mut self,
        archive: u32,
        offset: u64,
        length: u32,
        deadline: Instant,
    ) -> io::Result<Bytes> {
        if length == 0 || length > MAX_PACK_READ_BYTES {
            return Err(invalid());
        }
        if let Some(read) = &self.read {
            if (read.archive, read.offset, read.length) != (archive, offset, length) {
                return Err(invalid());
            }
        } else {
            let record = serde_json::to_vec(&Range {
                archive,
                offset,
                length,
            })
            .map_err(|_| invalid())?;
            self.writer.admit(Kind::ReadPack, &record)?;
            self.read = Some(PendingRead {
                archive,
                offset,
                length,
                deadline,
            });
        }
        let deadline = self
            .read
            .as_ref()
            .expect("original range retained")
            .deadline;
        let result = bounded(deadline, async {
            self.writer.flush().await?;
            let frame = self.reader.recv().await?.ok_or_else(invalid)?;
            if frame.kind != Kind::PackBytes || frame.body.len() != length as usize {
                return Err(invalid());
            }
            Ok(frame.body)
        })
        .await;
        if result.is_ok() {
            self.read = None;
        }
        result
    }

    /// Admits one Attach synchronously after the caller's current ServerWorldReady authority check.
    pub fn begin_attach(mut self, deadline: Instant) -> io::Result<AttachingBackendPeer> {
        if self.read.is_some() {
            return Err(invalid());
        }
        check_deadline(deadline)?;
        self.writer.admit(Kind::Attach, &[])?;
        Ok(AttachingBackendPeer {
            reader: self.reader,
            writer: self.writer,
            deadline,
        })
    }
}

impl AttachingBackendPeer {
    /// Finishes only the admitted Attach and original World response.
    /// Keep this future pinned; canceling the owning operation drops its actual socket.
    pub async fn finish(mut self) -> io::Result<BackendWorldReady> {
        bounded(self.deadline, async move {
            self.writer.flush().await?;
            let frame = self.reader.recv().await?.ok_or_else(invalid)?;
            if frame.kind != Kind::World {
                return Err(invalid());
            }
            Ok(BackendWorldReady {
                reader: self.reader,
                writer: self.writer,
                world: frame.body,
            })
        })
        .await
    }
}

impl BackendWorldReady {
    /// Borrows the original encoded Go packet roles, without inventing JSON GameData.
    pub fn world(&self) -> &[u8] {
        &self.world
    }

    /// Admits Initialized synchronously after actual ServerInitialized::finish and current authority checks.
    pub fn begin_initialized(
        mut self,
        cache_enabled: bool,
        radius: u32,
        deadline: Instant,
    ) -> io::Result<InitializingBackendPeer> {
        if radius == 0 {
            return Err(invalid());
        }
        check_deadline(deadline)?;
        admit_json(
            &mut self.writer,
            Kind::Initialized,
            &Initialized {
                cache_enabled,
                radius,
            },
        )?;
        Ok(InitializingBackendPeer {
            reader: self.reader,
            writer: self.writer,
            deadline,
        })
    }
}

impl InitializingBackendPeer {
    /// Flushes only the admitted Initialized before handing off the original play owner.
    /// Keep this future pinned; canceling the owning operation drops its actual socket.
    pub async fn finish(mut self) -> io::Result<ActiveBackendPeer> {
        bounded(self.deadline, async move {
            self.writer.flush().await?;
            Ok(ActiveBackendPeer {
                reader: self.reader,
                writer: self.writer,
                pending: None,
                pending_id: 0,
                receive_id: 0,
                send_id: 0,
                send: None,
                receipt_admitted: false,
                delivered: false,
            })
        })
        .await
    }
}

impl ActiveBackendPeer {
    /// Receives one original packet. A receipt is deliberately separate from external transport completion.
    pub async fn recv(&mut self) -> io::Result<Option<Bytes>> {
        loop {
            if self.pending_id != 0 {
                if self.delivered {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "native world bridge input awaits its receipt",
                    ));
                }
                self.delivered = true;
                return Ok(self.pending.clone());
            }
            let Some(frame) = self.reader.recv().await? else {
                return Ok(None);
            };
            if frame.kind == Kind::Closed || frame.kind == Kind::Close {
                return Ok(None);
            }
            self.accept(frame)?;
        }
    }

    /// Acknowledges only after the same packet's actual external send completed, not remote Minecraft processing.
    pub async fn received(&mut self, deadline: Instant) -> io::Result<()> {
        if self.pending_id == 0 {
            return Err(invalid());
        }
        bounded(deadline, async {
            if !self.receipt_admitted {
                if self
                    .send
                    .as_ref()
                    .is_some_and(|send| !matches!(send.phase, SendPhase::Receipt))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "native world bridge output is active",
                    ));
                }
                self.writer
                    .admit(Kind::Flushed, &self.pending_id.to_be_bytes())?;
                self.receipt_admitted = true;
            }
            self.writer.flush().await?;
            self.pending = None;
            self.pending_id = 0;
            self.receipt_admitted = false;
            self.delivered = false;
            Ok(())
        })
        .await
    }

    /// Admits one packet synchronously, retaining its original deadline until the receipt completes.
    pub fn queue_send(&mut self, packet: &[u8], deadline: Instant) -> io::Result<()> {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native world bridge write expired",
            ));
        }
        if self.send.is_some() || self.receipt_admitted {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "native world bridge output is active",
            ));
        }
        self.send_id = self.send_id.checked_add(1).ok_or_else(invalid)?;
        self.writer.admit(Kind::Packet, packet)?;
        self.send = Some(PendingSend {
            phase: SendPhase::Packet,
            deadline,
            sequence: self.send_id,
        });
        Ok(())
    }

    /// Resumes the same output/receipt after cancellation without resetting its deadline.
    /// WouldBlock means one opposite-direction packet must be dispatched and received first.
    pub async fn flush_send(&mut self) -> io::Result<()> {
        let Some(send) = &self.send else {
            return Ok(());
        };
        let (deadline, sequence) = (send.deadline, send.sequence);
        bounded(deadline, async {
            if self.pending_id != 0 {
                return Err(input_pending());
            }
            if matches!(
                self.send.as_ref().expect("retained send").phase,
                SendPhase::Packet
            ) {
                self.writer.flush().await?;
                self.writer.admit(Kind::Flush, &sequence.to_be_bytes())?;
                self.send.as_mut().expect("retained send").phase = SendPhase::Flush;
            }
            if matches!(
                self.send.as_ref().expect("retained send").phase,
                SendPhase::Flush
            ) {
                self.writer.flush().await?;
                self.send.as_mut().expect("retained send").phase = SendPhase::Receipt;
            }
            loop {
                let frame = self.reader.recv().await?.ok_or_else(invalid)?;
                if frame.kind == Kind::Flushed {
                    if frame.body.as_ref() != sequence.to_be_bytes() {
                        return Err(invalid());
                    }
                    self.send = None;
                    return Ok(());
                }
                self.accept(frame)?;
                if self.pending_id != 0 {
                    return Err(input_pending());
                }
            }
        })
        .await
    }

    /// Consumes local play admission and captures the one finite original cleanup deadline.
    pub fn begin_close(self, deadline: Instant) -> ClosingBackendPeer {
        ClosingBackendPeer {
            active: self,
            deadline,
            sent: false,
            finished: false,
        }
    }

    /// Retains only one outstanding application payload while continuing control parsing.
    fn accept(&mut self, frame: Frame) -> io::Result<()> {
        match frame.kind {
            Kind::Packet if self.pending.is_none() && !frame.body.is_empty() => {
                self.pending = Some(frame.body);
                Ok(())
            }
            Kind::Flush if self.pending.is_some() && self.pending_id == 0 => {
                let id = u64::from_be_bytes(frame.body.as_ref().try_into().map_err(|_| invalid())?);
                if self.receive_id.checked_add(1) != Some(id) {
                    return Err(invalid());
                }
                self.receive_id = id;
                self.pending_id = id;
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
}

/// Yields the same sole payload slot so control progress cannot deadlock opposite-direction output.
fn input_pending() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "native world bridge input awaits external dispatch",
    )
}

impl ClosingBackendPeer {
    /// Drains the same accepted packet before closure. No new play send capability exists.
    pub async fn recv(&mut self) -> io::Result<Option<Bytes>> {
        if self.finished {
            return Ok(None);
        }
        self.active.recv().await
    }

    /// Completes an accepted packet's actual external dispatch before its final receipt.
    pub async fn received(&mut self) -> io::Result<()> {
        self.active.received(self.deadline).await
    }

    /// Finishes once; canceled waits retain the original deadline and partial control output.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        bounded(self.deadline, async {
            self.active.flush_send().await?;
            if !self.sent {
                self.active.writer.admit(Kind::Close, &[])?;
                self.sent = true;
            }
            self.active.writer.flush().await?;
            loop {
                if self.active.pending_id != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "native world bridge has accepted input to drain",
                    ));
                }
                let frame = self.active.reader.recv().await?.ok_or_else(invalid)?;
                if frame.kind == Kind::Closed {
                    self.finished = true;
                    return Ok(());
                }
                self.active.accept(frame)?;
            }
        })
        .await
    }
}
