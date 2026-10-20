//! Concrete application capabilities around the same bounded desktop connection pump.

use super::{NativeRelayConnection, packet_id, terminal};
use crate::session_transport::native::{self, DriverEnd, IncomingBody, NativeSender, PreparedPump};
use crate::session_wire::CoreMessage;
use bedrock_protocol_1_26_51::{packets::PacketId, wire};
use bytes::Bytes;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{mpsc, oneshot};

type Completion = Result<(), Arc<io::Error>>;
type Work = Pin<Box<dyn Future<Output = DriverEnd> + Send>>;
type PendingInput = Option<IncomingBody>;

/// Cloneable send admission minted only from an actual established relay connection.
/// Shared closure and remote failure revoke every clone at runtime.
#[derive(Clone)]
pub struct NativeRelaySender {
    sender: NativeSender,
}

/// One committed write; dropping its receipt does not cancel the accepted batch.
#[must_use]
pub struct NativeRelayWritten {
    receipt: Option<oneshot::Receiver<io::Result<()>>>,
    completion: Option<Completion>,
}

/// The unique accepted input receiver and its one active batch cursor.
#[must_use]
pub struct NativeRelayInbound {
    incoming: mpsc::Receiver<IncomingBody>,
    tail: Option<oneshot::Receiver<PendingInput>>,
    current: Option<wire::Frames>,
    ended: bool,
}

/// The unique caller-polled connection owner. Dropping it physically aborts the transport.
/// Completion is cleanup progress, not a replacement for input still held by the receiver.
#[must_use]
pub struct NativeRelayDriver {
    work: Option<Work>,
    sender: NativeSender,
    stop: Option<oneshot::Sender<()>>,
    tail: Option<oneshot::Sender<PendingInput>>,
    completion: Option<Completion>,
}

/// Consumed send admission and the same retained driver under one original close deadline.
#[must_use]
pub struct NativeRelayClosing {
    driver: NativeRelayDriver,
    deadline: tokio::time::Instant,
}

impl NativeRelayConnection {
    /// Moves this exact established connection into the original bounded pump without spawning.
    /// The caller must continuously poll the unique driver beside its application I/O.
    /// Input byte credit remains held through the active batch cursor and every returned
    /// shared packet view. A valid body larger than the queue budget occupies it alone
    /// until its last retained view is released.
    pub fn into_pump(self) -> (NativeRelaySender, NativeRelayInbound, NativeRelayDriver) {
        split(
            native::prepare_with_byte_budget(
                self.connection,
                Some(bridge::contract::RELAY_QUEUED_FRAME_BYTES),
            ),
            self.terminal,
        )
    }
}

/// Splits only prepared ownership, retaining the original one pending body as its completion tail.
fn split(
    pump: PreparedPump,
    ended: bool,
) -> (NativeRelaySender, NativeRelayInbound, NativeRelayDriver) {
    let PreparedPump {
        sender,
        incoming,
        stop,
        driver,
    } = pump;
    let (completed, tail) = oneshot::channel();
    let mut driver = NativeRelayDriver {
        work: Some(driver),
        sender: sender.clone(),
        stop: Some(stop),
        tail: Some(completed),
        completion: None,
    };
    if ended {
        driver.stop();
    }
    (
        NativeRelaySender { sender },
        NativeRelayInbound {
            incoming,
            tail: Some(tail),
            current: None,
            ended,
        },
        driver,
    )
}

impl NativeRelaySender {
    /// Checks complete packet framing before committing a batch to the original send queue.
    /// Canceling an uncommitted admission accepts nothing; accepted work belongs to the driver.
    pub async fn accept_body(&self, body: Bytes) -> io::Result<NativeRelayWritten> {
        crate::raw_packet::decode_batch_body(
            body.clone(),
            bedrock_net::BatchLimits::default().max_decoded_bytes,
        )
        .map_err(io::Error::other)?;
        Ok(NativeRelayWritten {
            receipt: Some(self.sender.accept(body).await?),
            completion: None,
        })
    }
}

impl Future for NativeRelayWritten {
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.completion.is_none() {
            let receipt = self.receipt.as_mut().expect("uncompleted write receipt");
            let result = match std::task::ready!(Pin::new(receipt).poll(cx)) {
                Ok(result) => result,
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "native writer stopped",
                )),
            };
            self.receipt = None;
            self.completion = Some(result.map_err(Arc::new));
        }
        Poll::Ready(completion(
            self.completion.as_ref().expect("completed write"),
        ))
    }
}

impl NativeRelayInbound {
    /// Drains actual accepted packet FIFO, preserving headers and the first decoded terminal.
    /// Driver completion never manufactures or replaces an input terminal.
    pub async fn recv(&mut self) -> io::Result<CoreMessage> {
        if self.ended {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let result = self.next().await;
        if result.is_err() {
            self.ended = true;
            self.current = None;
        }
        result
    }

    /// Advances only the active original frame cursor, without a second packet queue.
    async fn next(&mut self) -> io::Result<CoreMessage> {
        loop {
            let Some(entry) = self.current.as_mut().and_then(Iterator::next) else {
                self.current = None;
                let item = match self.incoming.recv().await {
                    Some(item) => item,
                    None => {
                        let Some(tail) = self.tail.as_mut() else {
                            return Err(io::ErrorKind::UnexpectedEof.into());
                        };
                        let result = tail.await;
                        self.tail = None;
                        result
                            .map_err(|_| io::ErrorKind::UnexpectedEof)?
                            .ok_or(io::ErrorKind::UnexpectedEof)?
                    }
                };
                self.current = Some(wire::Frames::new(
                    item.into_body()?,
                    crate::raw_packet::MAX_RAW_BATCH_PACKETS,
                ));
                continue;
            };
            let entry = entry.map_err(io::Error::other)?;
            let raw = {
                let mut reader = wire::Reader::from_shared(entry.frame());
                reader.read_var_u32().map_err(io::Error::other)?;
                entry.frame().slice(reader.position()..)
            };
            if let Some(message) = terminal(&raw)? {
                self.ended = true;
                drop(raw);
                drop(entry);
                self.current = None;
                return Ok(message);
            }
            if packet_id(&raw)? != PacketId::Transfer {
                return Ok(CoreMessage::Batch(vec![raw]));
            }
        }
    }
}

impl NativeRelayDriver {
    /// Polls the same in-flight connection work across cancellation and repeated completed waits.
    pub async fn drive(&mut self) -> io::Result<()> {
        if let Some(work) = &mut self.work {
            let end = work.await;
            self.work = None;
            if let Some(tail) = self.tail.take() {
                let _ = tail.send(end.pending);
            }
            self.completion = Some(end.close.map_err(Arc::new));
        }
        completion(self.completion.as_ref().expect("completed native driver"))
    }

    /// Seals all sender clones synchronously and retains accepted drain under one finite deadline.
    pub fn begin_close(mut self) -> NativeRelayClosing {
        self.stop();
        NativeRelayClosing {
            driver: self,
            deadline: tokio::time::Instant::now() + native::CLOSE_TIMEOUT,
        }
    }

    /// Revokes commitment before waking the retained driver's normal close branch.
    fn stop(&mut self) {
        self.sender.close_admission();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for NativeRelayDriver {
    fn drop(&mut self) {
        self.stop();
    }
}

impl NativeRelayClosing {
    /// A canceled wait retains both the actual work and the original deadline.
    /// Timeout aborts the physical connection; already queued receiver input remains drainable.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        if self.driver.completion.is_some() {
            return self.driver.drive().await;
        }
        match tokio::time::timeout_at(self.deadline, self.driver.drive()).await {
            Ok(result) => result,
            Err(_) => {
                self.driver.work = None;
                if let Some(tail) = self.driver.tail.take() {
                    let _ = tail.send(None);
                }
                self.driver.completion = Some(Err(Arc::new(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native close timed out",
                ))));
                self.driver.drive().await
            }
        }
    }
}

/// Reborrows retained completion without repolling a completed future or altering input causes.
fn completion(result: &Completion) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => Err(io::Error::new(error.kind(), Arc::clone(error))),
    }
}

#[cfg(test)]
mod tests;
