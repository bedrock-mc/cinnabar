//! The original child process connects back to one private, capability-bound parent endpoint.

use super::{Reader, Writer, contract::*, invalid, split};
use serde::{Serialize, de::DeserializeOwned};
use std::{io, net::Ipv4Addr, time::Instant};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;

/// A bound parent endpoint awaiting its exact child, without a world/admission claim.
pub struct PendingHostProcess {
    listener: TcpListener,
    bootstrap: Bootstrap,
}

/// Original authenticated process control and final immutable resource metadata.
pub struct HostProcess {
    listener: TcpListener,
    bootstrap: Bootstrap,
    reader: Reader,
    writer: Writer,
    snapshot: Snapshot,
    next_peer: u64,
    control_deadline: Option<Instant>,
    closing: bool,
    close_admitted: bool,
    closed: bool,
    close_deadline: Option<Instant>,
}

/// One reserved Open after prior control output drained, before new admission.
#[must_use = "check current application authority before admitting this Open"]
pub struct PreparedBackendOpen<'a> {
    process: &'a mut HostProcess,
    open: Open,
    deadline: Instant,
}

/// One admitted Open, retaining the original process borrow, peer ID and deadline.
#[must_use = "finish this admitted Open or retain the process for bounded retirement"]
pub struct OpeningBackendPeer<'a> {
    process: &'a mut HostProcess,
    peer: u64,
    deadline: Instant,
}

impl PendingHostProcess {
    /// Takes edge-generated random entropy; this is a private IPC capability, never a player proof.
    pub async fn bind(
        capability: &[u8; CAPABILITY_BYTES as usize],
        opening: &[u8; OPENING_BYTES as usize],
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        Ok(Self {
            bootstrap: Bootstrap {
                address: listener.local_addr()?.to_string(),
                opening: hex(opening),
                capability: hex(capability),
            },
            listener,
        })
    }

    /// Send this exact record through the actual original child stdin owner, off the frame thread.
    pub fn bootstrap(&self) -> &Bootstrap {
        &self.bootstrap
    }

    /// Consumes the pending endpoint. Cancellation drops its actual sockets rather than a detached accept task.
    pub async fn establish(self, deadline: Instant) -> io::Result<HostProcess> {
        bounded(deadline, async {
            let (mut reader, writer) = accept(&self.listener, &self.bootstrap, 0, deadline).await?;
            let frame = reader.recv().await?.ok_or_else(invalid)?;
            if frame.kind != Kind::Snapshot {
                return Err(invalid());
            }
            let snapshot: Snapshot = parse(&frame.body)?;
            if snapshot.max_players == 0
                || snapshot.max_players > MAX_PLAYERS
                || snapshot.max_chunk_radius == 0
            {
                return Err(invalid());
            }
            Ok(HostProcess {
                listener: self.listener,
                bootstrap: self.bootstrap,
                reader,
                writer,
                snapshot,
                next_peer: 1,
                control_deadline: None,
                closing: false,
                close_admitted: false,
                closed: false,
                close_deadline: None,
            })
        })
        .await
    }
}

impl HostProcess {
    /// Borrows only the final original Config snapshot, without archives or a mutable replacement.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Reserves the original Open and drains only previously admitted control output.
    /// Recheck current application authority after this wait, before consuming begin_open.
    /// Cancellation retains the original partial output/deadline and consumes the reserved peer ID.
    pub async fn prepare_open(
        &mut self,
        identity: Identity,
        client_data: Box<serde_json::value::RawValue>,
        deadline: Instant,
    ) -> io::Result<PreparedBackendOpen<'_>> {
        if self.closing {
            return Err(invalid());
        }
        let peer = self.next_peer;
        self.next_peer = peer.checked_add(1).ok_or_else(invalid)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(expired)?;
        let remaining_millis = u64::try_from(remaining.as_millis())
            .map_err(|_| invalid())?
            .min(u64::from(MAX_OPEN_MILLIS));
        if remaining_millis == 0 {
            return Err(expired());
        }
        let open = Open {
            peer,
            remaining_millis,
            identity,
            client_data,
        };
        bounded(deadline, async {
            if let Some(original) = self.control_deadline {
                bounded(original, self.writer.flush()).await?;
                self.control_deadline = None;
            }
            Ok(())
        })
        .await?;
        Ok(PreparedBackendOpen {
            process: self,
            open,
            deadline,
        })
    }

    /// Retires this exact child bridge and peers, independent of a later account/proof replacement.
    /// Canceled waits retain the first deadline and the actual partial output on this owner.
    pub async fn close(&mut self, deadline: Instant) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closing = true;
        let deadline = *self.close_deadline.get_or_insert(deadline);
        bounded(deadline, async {
            if let Some(original) = self.control_deadline {
                bounded(original.min(deadline), self.writer.flush()).await?;
                self.control_deadline = None;
            }
            if !self.close_admitted {
                self.writer.admit(Kind::Close, &[])?;
                self.close_admitted = true;
            }
            self.writer.flush().await?;
            loop {
                let frame = self.reader.recv().await?.ok_or_else(invalid)?;
                match frame.kind {
                    Kind::Closed if frame.body.is_empty() => {
                        self.closed = true;
                        return Ok(());
                    }
                    Kind::Opened | Kind::Rejected => {
                        let _: PeerResult = parse(&frame.body)?;
                    }
                    _ => return Err(invalid()),
                }
            }
        })
        .await
    }
}

impl<'a> PreparedBackendOpen<'a> {
    /// Admits the original Open synchronously after the caller's current authority check.
    pub fn begin_open(self) -> io::Result<OpeningBackendPeer<'a>> {
        check_deadline(self.deadline)?;
        self.process.control_deadline = Some(self.deadline);
        admit_json(&mut self.process.writer, Kind::Open, &self.open)?;
        Ok(OpeningBackendPeer {
            process: self.process,
            peer: self.open.peer,
            deadline: self.deadline,
        })
    }
}

impl OpeningBackendPeer<'_> {
    /// Finishes the admitted Open and original capability-bound peer exchange.
    /// Keep this future pinned. Cancellation drops an accepted peer socket; an unaccepted child
    /// dial retains its original finite budget, and later calls drain its stale reply.
    pub async fn finish(self) -> io::Result<super::PendingBackendPeer> {
        let Self {
            process,
            peer,
            deadline,
        } = self;
        bounded(deadline, async {
            process.writer.flush().await?;
            process.control_deadline = None;
            // The child waits for our mutual Hello before reporting Opened.
            let incoming = accept(&process.listener, &process.bootstrap, peer, deadline);
            tokio::pin!(incoming);
            let mut socket = None;
            let mut opened = false;
            loop {
                if opened && let Some((reader, writer)) = socket.take() {
                    return Ok(super::PendingBackendPeer::new(reader, writer));
                }
                tokio::select! {
                    result = &mut incoming, if socket.is_none() => { socket = Some(result?); }
                    frame = process.reader.recv() => {
                        let frame = frame?.ok_or_else(invalid)?;
                        if !matches!(frame.kind, Kind::Opened | Kind::Rejected) { return Err(invalid()); }
                        let result: PeerResult = parse(&frame.body)?;
                        if result.peer < peer { continue; }
                        if result.peer != peer { return Err(invalid()); }
                        if frame.kind == Kind::Rejected || result.code != 0 {
                            return Err(io::Error::new(io::ErrorKind::WouldBlock, "native world bridge admission refused"));
                        }
                        opened = true;
                    }
                }
            }
        })
        .await
    }
}

/// Authenticates the first bounded frame before handing either stream half to a consumer.
async fn accept(
    listener: &TcpListener,
    bootstrap: &Bootstrap,
    peer: u64,
    deadline: Instant,
) -> io::Result<(Reader, Writer)> {
    loop {
        let (stream, address) = listener.accept().await?;
        if !address.ip().is_loopback() {
            continue;
        }
        stream.set_nodelay(true)?;
        let (mut reader, mut writer) = split(stream);
        let hello = bounded(deadline, async {
            let frame = reader.recv().await?.ok_or_else(invalid)?;
            if frame.kind != Kind::Hello {
                return Err(invalid());
            }
            parse::<Hello>(&frame.body)
        })
        .await?;
        if hello.version != VERSION
            || hello.opening != bootstrap.opening
            || !bool::from(
                hello
                    .capability
                    .as_bytes()
                    .ct_eq(bootstrap.capability.as_bytes()),
            )
            || hello.peer > peer
        {
            continue;
        }
        if hello.peer != peer {
            continue;
        } // Canceled older reservation: dropping its real socket retires it.
        send_json(&mut writer, Kind::Hello, &hello).await?;
        return Ok((reader, writer));
    }
}

/// Encodes and admits one bounded generated record; failures never include its contents.
pub(super) fn admit_json(
    writer: &mut Writer,
    kind: Kind,
    record: &impl Serialize,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(record).map_err(|_| invalid())?;
    writer.admit(kind, &bytes)
}

/// Admits and flushes bounded generated metadata through the same output slot.
pub(super) async fn send_json(
    writer: &mut Writer,
    kind: Kind,
    record: &impl Serialize,
) -> io::Result<()> {
    admit_json(writer, kind, record)?;
    writer.flush().await
}

/// Decodes bounded process metadata without exposing raw values in diagnostics.
pub(super) fn parse<T: DeserializeOwned>(body: &[u8]) -> io::Result<T> {
    serde_json::from_slice(body).map_err(|_| invalid())
}

/// Bounds actual IO by the caller's original monotonic cutoff, never by progress-relative retries.
pub(super) async fn bounded<T>(
    deadline: Instant,
    io: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    check_deadline(deadline)?;
    let result = tokio::time::timeout_at(deadline.into(), io)
        .await
        .map_err(|_| expired())??;
    check_deadline(deadline)?;
    Ok(result)
}

/// Rejects an expired original operation before synchronous admission or after progress.
pub(super) fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(expired());
    }
    Ok(())
}

/// Hex encodes only fixed-size IPC entropy using the existing standard formatter.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(result, "{byte:02x}").expect("String write");
    }
    result
}

/// Contains no endpoint, world locator or private record.
fn expired() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "native world bridge deadline expired",
    )
}
