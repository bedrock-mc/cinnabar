//! An optional trusted relay between the session and its upstream connection. It sees every batch body, in the
//! session contract's batch form, before decoding and after encoding, and may inject its own.

use crate::ProtocolError;
use crate::packet_tap::PacketTap;
use crate::session_transport::{PacketSender, PacketWritten};
use crate::session_wire::SessionIdentity;
use bytes::{BufMut, Bytes, BytesMut};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, OwnedMutexGuard, mpsc};

/// How often a relay's timed work runs: one client tick.
pub const RELAY_TICK: Duration = Duration::from_millis(50);
/// Injected client-bound bodies waiting for the session to read them.
#[cfg(not(target_arch = "wasm32"))]
const INJECTED_CAPACITY: usize = 64;

/// A batch body is each packet as a varuint32 byte length followed by the encoded packet,
/// header included. Every call may block briefly and is serialized by the session, so the
/// relay observes client batches in the order they are written.
pub trait PacketRelay: Send + Sync {
    /// Begins the session from its identity, the Connect's client data JSON and the startup
    /// packets through StartGame as one body; returns resource packs to add. Runs off the async
    /// runtime, once. These packets subsequently pass through `inbound_startup` in wire order so startup
    /// state and any replacements use the same handler as later server traffic.
    fn start(
        &self,
        identity: &SessionIdentity,
        client_data: &str,
        startup: &[u8],
    ) -> Vec<RelayPack>;
    /// Server bodies through spawn may initialize registries; the host gives them its join budget.
    fn inbound_startup(&self, body: &[u8]) -> Option<Vec<u8>> {
        self.inbound(body)
    }
    /// A server batch body: `None` forwards it, a replacement takes its place and an empty one
    /// drops it.
    fn inbound(&self, body: &[u8]) -> Option<Vec<u8>>;
    /// A client batch body, with the same replacement rules as [`Self::inbound`].
    fn outbound(&self, body: &[u8]) -> Option<Vec<u8>>;
    /// Timed work, about once per [`RELAY_TICK`]; runs off the async runtime.
    fn tick(&self) -> RelayOutput;
    /// Ends the session without waiting for the relay.
    fn stop(&self);
}

use bedrock_packs::RelayPack;

/// Batch bodies a relay sends on its own; an empty body sends nothing.
#[derive(Default)]
pub struct RelayOutput {
    pub to_client: Vec<u8>,
    pub to_server: Vec<u8>,
}

/// The relay and the gate that keeps its view of client batches in write order.
struct Shared {
    relay: Arc<dyn PacketRelay>,
    gate: Arc<Mutex<()>>,
    active: AtomicBool,
}

impl Shared {
    /// Disables a faulty relay once; all later original batches bypass it.
    fn disable(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            tracing::warn!("packet relay produced malformed data; packets now pass through");
            self.relay.stop();
        }
    }

    /// Checks synthetic or replacement framing using the session's packet decoder.
    fn valid(&self, body: Bytes) -> bool {
        if crate::raw_packet::decode_batch_body(
            body,
            crate::session_transport::MAX_SESSION_BATCH_BYTES,
        )
        .is_err()
        {
            self.disable();
            return false;
        }
        self.active.load(Ordering::Acquire)
    }
}

/// A relay's final body, retaining its write-order gate until queue admission.
pub(crate) struct PreparedBatch {
    pub(crate) body: Bytes,
    _gate: Option<OwnedMutexGuard<()>>,
}

/// Writes whole client batch bodies to the original connection, through its optional relay.
#[derive(Clone)]
pub(crate) struct BatchWriter {
    sender: PacketSender,
    relay: Option<Arc<Shared>>,
    tap: Option<Arc<dyn PacketTap>>,
}

impl BatchWriter {
    /// A writer without a relay; bodies go straight to the original packet FIFO.
    pub(crate) fn direct(sender: PacketSender) -> Self {
        Self {
            sender,
            relay: None,
            tap: None,
        }
    }

    /// Reports every accepted server-bound batch after any relay replacement.
    pub(crate) fn with_tap(mut self, tap: Option<Arc<dyn PacketTap>>) -> Self {
        self.tap = tap;
        self
    }

    /// Records the final batch body, including batches injected by the relay.
    fn record(&self, body: &Bytes) {
        if let Some(tap) = &self.tap {
            crate::packet_tap::record_serverbound_body(tap.as_ref(), body);
        }
    }

    /// The original connection's packet FIFO, including its native admission policy.
    pub(crate) fn sender(&self) -> &PacketSender {
        &self.sender
    }

    /// Writes one body after every body accepted before it and resolves once flushed.
    /// A relay sees its body first; a batch it drops resolves at once.
    pub(crate) async fn send(&self, body: Bytes) -> Result<(), ProtocolError> {
        if let Some(written) = self.accept(body).await? {
            written.await?;
        }
        Ok(())
    }

    /// Accepts one final frame and records it once, retaining the flush ticket across cancellation.
    pub(crate) async fn accept(&self, body: Bytes) -> Result<Option<PacketWritten>, ProtocolError> {
        let Some(prepared) = self.prepare(body).await else {
            return Ok(None);
        };
        self.accept_prepared(prepared.body.clone()).await.map(Some)
    }

    /// Runs the relay once, keeping its replacement and FIFO gate separate from capacity waits.
    pub(crate) async fn prepare(&self, body: Bytes) -> Option<PreparedBatch> {
        let Some(shared) = &self.relay else {
            return Some(PreparedBatch { body, _gate: None });
        };
        let gate = Arc::clone(&shared.gate).lock_owned().await;
        if !shared.active.load(Ordering::Acquire) {
            return Some(PreparedBatch {
                body,
                _gate: Some(gate),
            });
        }
        let body = match shared.relay.outbound(&body) {
            None => body,
            Some(replacement) => {
                let replacement = Bytes::from(replacement);
                if !shared.valid(replacement.clone()) {
                    body
                } else if replacement.is_empty() {
                    return None;
                } else {
                    replacement
                }
            }
        };
        Some(PreparedBatch {
            body,
            _gate: Some(gate),
        })
    }

    /// Admits and records already relayed bytes without invoking the guest again.
    pub(crate) async fn accept_prepared(
        &self,
        body: Bytes,
    ) -> Result<PacketWritten, ProtocolError> {
        let written = self.sender.accept(body.clone()).await?;
        self.record(&body);
        Ok(written)
    }

    /// Accepts a relay's own server-bound body in write order, without relaying it again.
    #[cfg(not(target_arch = "wasm32"))]
    async fn inject(&self, shared: &Shared, body: &[u8]) -> Result<(), ProtocolError> {
        let _gate = shared.gate.lock().await;
        let body = Bytes::copy_from_slice(body);
        self.sender.accept(body.clone()).await?;
        self.record(&body);
        Ok(())
    }
}

/// The session side of a running relay: client-bound injections and the tick task.
pub(crate) struct RelayLink {
    shared: Arc<Shared>,
    pub(crate) injected: mpsc::Receiver<Bytes>,
    ticker: tokio::task::JoinHandle<()>,
}

impl RelayLink {
    /// Starts timed work on the current runtime and routes the original sender through the relay.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start(
        relay: Arc<dyn PacketRelay>,
        sender: PacketSender,
        tap: Option<Arc<dyn PacketTap>>,
    ) -> (Self, BatchWriter) {
        let shared = Arc::new(Shared {
            relay: Arc::clone(&relay),
            gate: Arc::new(Mutex::new(())),
            active: AtomicBool::new(true),
        });
        let writer = BatchWriter {
            sender,
            relay: Some(Arc::clone(&shared)),
            tap,
        };
        let (inject, injected) = mpsc::channel(INJECTED_CAPACITY);
        let ticker = tokio::spawn(run_ticks(Arc::clone(&shared), writer.clone(), inject));
        let link = Self {
            shared,
            injected,
            ticker,
        };
        (link, writer)
    }

    /// Lets the relay replace a server batch body; `None` keeps the original.
    pub(crate) fn inbound(&self, body: &[u8], startup: bool) -> Option<Bytes> {
        if !self.shared.active.load(Ordering::Acquire) {
            return None;
        }
        let replacement = if startup {
            self.shared.relay.inbound_startup(body)
        } else {
            self.shared.relay.inbound(body)
        };
        replacement
            .map(Bytes::from)
            .filter(|body| self.shared.valid(body.clone()))
    }
}

impl Drop for RelayLink {
    fn drop(&mut self) {
        self.ticker.abort();
        self.shared.relay.stop();
    }
}

/// Runs the relay's timed work until the session or the core connection ends.
#[cfg(not(target_arch = "wasm32"))]
async fn run_ticks(shared: Arc<Shared>, writer: BatchWriter, inject: mpsc::Sender<Bytes>) {
    let mut interval = tokio::time::interval(RELAY_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if !shared.active.load(Ordering::Acquire) {
            return;
        }
        let relay = Arc::clone(&shared.relay);
        let Ok(output) = tokio::task::spawn_blocking(move || relay.tick()).await else {
            return;
        };
        let to_client = Bytes::from(output.to_client);
        let to_server = Bytes::from(output.to_server);
        if !shared.valid(to_client.clone()) || !shared.valid(to_server.clone()) {
            return;
        }
        if !to_client.is_empty() && inject.send(to_client).await.is_err() {
            return;
        }
        if !to_server.is_empty() && writer.inject(&shared, &to_server).await.is_err() {
            return;
        }
    }
}

/// Starts a relay for a handed-off session off the async runtime and returns its packs.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn start(
    relay: Arc<dyn PacketRelay>,
    identity: SessionIdentity,
    client_data: String,
    startup: &[Bytes],
) -> Vec<RelayPack> {
    let startup = batch_body(startup);
    tokio::task::spawn_blocking(move || relay.start(&identity, &client_data, &startup))
        .await
        .unwrap_or_default()
}

/// Joins packets, each with its header, into one batch body.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn batch_body(packets: &[Bytes]) -> Bytes {
    let mut body = BytesMut::new();
    for packet in packets {
        put_entry(&mut body, packet);
    }
    body.freeze()
}

/// Appends one packet with its varuint32 length prefix, as a batch body carries it.
pub(crate) fn put_entry(body: &mut BytesMut, packet: &[u8]) {
    let mut length = u32::try_from(packet.len()).expect("a frame bounds its packets");
    while length >= 0x80 {
        body.put_u8(length as u8 | 0x80);
        length >>= 7;
    }
    body.put_u8(length as u8);
    body.extend_from_slice(packet);
}

#[cfg(all(test, unix))]
pub(crate) mod tests;
