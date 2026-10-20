//! Bounded duplex packet queues around one native connection owner.

use bytes::Bytes;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

use super::{PacketSender, Reader, SessionTransport};
use crate::login::native_network::{Connection, PacketConnection};

mod budget;
pub(crate) mod delay;
mod position;
use budget::ByteBudget;
pub(crate) use budget::ByteCredit;
use delay::{Deadline, NativePacketDelay};

const QUEUED_BATCHES: usize = 4;
pub(crate) const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

type PendingInput = Option<IncomingBody>;

/// The original body and its optional incoming byte credit.
#[derive(Debug)]
pub(crate) struct IncomingBody {
    pub(crate) body: io::Result<Bytes>,
    pub(crate) permit: Option<ByteCredit>,
}

impl IncomingBody {
    /// Keeps optional credit with every shared view of the original body.
    pub(crate) fn into_body(self) -> io::Result<Bytes> {
        match (self.body, self.permit) {
            (Ok(body), Some(credit)) => Ok(Bytes::from_owner(CreditedBody {
                body,
                _credit: credit,
            })),
            (body, _) => body,
        }
    }
}

/// Drops the original bytes before returning their byte credit.
struct CreditedBody {
    body: Bytes,
    _credit: ByteCredit,
}

impl AsRef<[u8]> for CreditedBody {
    fn as_ref(&self) -> &[u8] {
        &self.body
    }
}

/// Transfers the same uncommitted incoming tail after the actual transport close attempt.
pub(crate) struct DriverEnd {
    pub(crate) pending: PendingInput,
    pub(crate) close: io::Result<()>,
}

/// The original bounded channels and one caller-owned connection driver, without spawning.
pub(crate) struct PreparedPump {
    pub(crate) sender: NativeSender,
    pub(crate) incoming: mpsc::Receiver<IncomingBody>,
    pub(crate) stop: oneshot::Sender<()>,
    pub(crate) driver: Pin<Box<dyn Future<Output = DriverEnd> + Send>>,
}

#[cfg(test)]
pub(crate) mod tests;

pub(crate) struct NativeReader {
    incoming: mpsc::Receiver<IncomingBody>,
    pub(super) rtt: Option<crate::RoundTripTime>,
    task: Option<tokio::task::JoinHandle<io::Result<PendingInput>>>,
    tail: PendingInput,
    failure: Option<io::Error>,
    stop: Option<oneshot::Sender<tokio::time::Instant>>,
    closed: bool,
}

impl Drop for NativeReader {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl NativeReader {
    /// Requests finite teardown once; the original task keeps its cutoff across canceled waits.
    pub(super) async fn close(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(tokio::time::Instant::now() + CLOSE_TIMEOUT);
        }
        let result = std::future::poll_fn(|cx| self.poll_driver(cx)).await;
        self.closed = true;
        match result {
            Ok(pending) => {
                if let Some(item) = pending {
                    self.tail = Some(item);
                }
                self.failure.take().map_or(Ok(()), Err)
            }
            Err(error) => Err(error),
        }
    }

    /// Takes completed driver ownership once, retaining the handle across pending polls.
    fn poll_driver(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<PendingInput>> {
        let Some(task) = &mut self.task else {
            return Poll::Ready(Ok(None));
        };
        let result = std::task::ready!(Pin::new(task).poll(cx));
        self.task = None;
        Poll::Ready(
            result
                .map_err(io::Error::other)
                .and_then(std::convert::identity),
        )
    }

    /// Drains queued bodies before the driver's retained input, without losing either on cancellation.
    pub(super) fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>> {
        match self.incoming.poll_recv(cx) {
            Poll::Ready(None) => {}
            other => return other.map(|item| item.map(IncomingBody::into_body)),
        }
        match std::task::ready!(self.poll_driver(cx)) {
            Ok(Some(item)) => self.tail = Some(item),
            Ok(None) => {}
            Err(error) if !self.closed => self.failure = Some(error),
            Err(_) => {}
        }
        Poll::Ready(self.tail.take().map(IncomingBody::into_body))
    }
}

#[derive(Clone)]
pub(crate) struct NativeSender {
    pub(crate) delay: NativePacketDelay,
    send: mpsc::Sender<SendBatch>,
    closed: Arc<Mutex<bool>>,
}

struct SendBatch {
    body: Bytes,
    delay: Option<Deadline>,
    written: oneshot::Sender<io::Result<()>>,
}

/// Serializes queue commitment with receiver closure and every receiver-drop path.
struct Writes {
    delay: NativePacketDelay,
    receive: mpsc::Receiver<SendBatch>,
    closed: Arc<Mutex<bool>>,
    failed: bool,
}

impl Drop for Writes {
    fn drop(&mut self) {
        self.fail();
    }
}

impl Writes {
    /// Prevents every later commitment, including a permit reserved before closure.
    fn close_admission(&mut self) {
        let mut closed = self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *closed = true;
        self.receive.close();
        self.delay.retire();
    }

    /// Resolves queued receipts once no producer can append a late batch.
    fn fail(&mut self) {
        self.failed = true;
        self.close_admission();
        while self.receive.try_recv().is_ok() {}
    }

    /// Whether this queue still accepts new transport work.
    fn is_active(&self) -> bool {
        !*self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Creates one bounded queue whose admission gate outlives its receiver.
fn write_channel(capacity: usize) -> (NativeSender, Writes) {
    let (send, receive) = mpsc::channel(capacity);
    let closed = Arc::new(Mutex::new(false));
    let delay = NativePacketDelay::new();
    (
        NativeSender {
            delay: delay.clone(),
            send,
            closed: Arc::clone(&closed),
        },
        Writes {
            delay,
            receive,
            closed,
            failed: false,
        },
    )
}

impl NativeSender {
    /// Revokes every sender clone without discarding accepted writes.
    pub(crate) fn close_admission(&self) {
        *self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.delay.retire();
    }

    /// Commits one bounded batch after capacity is reserved, returning its write receipt.
    pub(crate) async fn accept(
        &self,
        body: Bytes,
    ) -> io::Result<oneshot::Receiver<io::Result<()>>> {
        Connection::check_body_size(&body)?;
        if *self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "native connection closed",
            ));
        }
        let permit =
            self.send.reserve().await.map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "native connection closed")
            })?;
        self.commit(permit, body)
    }

    /// Publishes an already size-checked batch atomically with respect to receiver teardown.
    fn commit(
        &self,
        permit: mpsc::Permit<'_, SendBatch>,
        body: Bytes,
    ) -> io::Result<oneshot::Receiver<io::Result<()>>> {
        let (written, receipt) = oneshot::channel();
        let closed = self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "native connection closed",
            ));
        }
        let delay = self.delay.capture(delay::now());
        permit.send(SendBatch {
            body,
            delay,
            written,
        });
        drop(closed);
        Ok(receipt)
    }
}

/// Prepares the original count-only incoming owner without polling or spawning it.
#[cfg(test)]
pub(crate) fn prepare(connection: impl PacketConnection) -> PreparedPump {
    prepare_with_byte_budget(connection, None)
}

/// Prepares the same incoming owner; byte credit must be attached before FIFO commitment.
pub(crate) fn prepare_with_byte_budget(
    mut connection: impl PacketConnection,
    byte_budget: Option<usize>,
) -> PreparedPump {
    let mut budget = byte_budget.map(ByteBudget::new);
    let (sender, mut writes) = write_channel(QUEUED_BATCHES);
    let (incoming, receive) = mpsc::channel(QUEUED_BATCHES);
    let (stop, mut stopping) = oneshot::channel();
    let driver = Box::pin(async move {
        let mut pending: PendingInput = None;
        let mut reservation: Option<budget::Reservation> = None;
        let mut front: Option<SendBatch> = None;
        let delay = writes.delay.clone();
        loop {
            let allows_read =
                pending.is_none() && budget.as_ref().is_none_or(ByteBudget::allows_read);
            let ready_input = pending.is_some() && reservation.is_none();
            tokio::select! {
                _ = &mut stopping => break,
                _ = incoming.closed() => break,
                permit = incoming.reserve(), if ready_input => {
                    let Ok(permit) = permit else { break };
                    let item = pending.take().expect("pending input");
                    let ended = item.body.is_err();
                    permit.send(item);
                    if ended { break; }
                }
                credit = async { reservation.as_mut().expect("pending byte reservation").await }, if reservation.is_some() => {
                    reservation = None;
                    pending.as_mut().expect("reserved incoming body").permit = Some(credit);
                }
                _ = async { budget.as_mut().expect("read-gated byte budget").changed().await }, if pending.is_none() && !allows_read => {}
                result = async {
                    if allows_read {
                        Some(connection.recv_body().await)
                    } else {
                        connection.maintain().await;
                        None
                    }
                } => {
                    if let Some(result) = result {
                        if result.is_err() {
                            writes.fail();
                            front = None;
                        }
                        reservation = result.as_ref().ok().and_then(|body| {
                            budget.as_ref().map(|budget| budget.reserve(body.len()))
                        });
                        pending = Some(IncomingBody { body: result, permit: None });
                    }
                }
                batch = writes.receive.recv(), if front.is_none() && writes.is_active() => {
                    let Some(batch) = batch else { break };
                    if batch.delay.is_some_and(|deadline| delay.remaining(deadline, delay::now()).is_some()) {
                        front = Some(batch);
                    } else {
                        write_batch(&mut connection, &mut writes, batch).await;
                    }
                }
                _ = delay.wait(front.as_ref().and_then(|batch| batch.delay)), if front.is_some() && writes.is_active() => {
                    let batch = front.take().expect("retained original outgoing batch");
                    write_batch(&mut connection, &mut writes, batch).await;
                }
            }
        }
        let drain = !writes.failed;
        writes.close_admission();
        if drain {
            if let Some(batch) = front.take() {
                write_batch(&mut connection, &mut writes, batch).await;
            }
            while !writes.failed {
                let Some(batch) = writes.receive.recv().await else {
                    break;
                };
                write_batch(&mut connection, &mut writes, batch).await;
            }
        }
        drop(front);
        // Move the same pending input through task completion so close never
        // waits for capacity held by its own caller's receive borrow.
        let close = connection.close().await;
        DriverEnd { pending, close }
    });
    PreparedPump {
        sender,
        incoming: receive,
        stop,
        driver,
    }
}

/// Completes one original accepted write without letting a policy wake cancel cipher progress.
async fn write_batch(
    connection: &mut impl PacketConnection,
    writes: &mut Writes,
    batch: SendBatch,
) {
    let tracking = writes.delay.tracking(delay::now());
    let position =
        tracking.and_then(|tracking| position::own_position(&batch.body, tracking.actor));
    let result = connection.send_body(batch.body).await;
    if result.is_err() {
        writes.fail();
    } else if let (Some(tracking), Some(position)) = (tracking, position) {
        writes.delay.forwarded(tracking, position, delay::now());
    }
    let _ = batch.written.send(result);
}

impl SessionTransport {
    /// Moves login's connection and unread packets into the same play FIFO.
    pub(crate) fn from_native(connection: impl PacketConnection) -> Self {
        let rtt = connection.observe_rtt();
        let PreparedPump {
            sender,
            incoming,
            stop,
            driver,
        } = prepare_with_byte_budget(connection, Some(bridge::contract::RELAY_QUEUED_FRAME_BYTES));
        let (closing, requested) = oneshot::channel();
        // The sole desktop task owns enforcement even when its caller stops awaiting close.
        // Successful teardown keeps the original desktop close-result and pending-input policy.
        let task = tokio::spawn(async move {
            let mut driver = driver;
            let deadline = tokio::select! {
                result = &mut driver => return Ok(result.pending),
                deadline = requested => deadline.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?,
            };
            let _ = stop.send(());
            tokio::time::timeout_at(deadline, driver)
                .await
                .map(|end| end.pending)
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "native close timed out"))
        });
        Self {
            reader: Reader::Native(NativeReader {
                incoming,
                rtt,
                task: Some(task),
                tail: None,
                failure: None,
                stop: Some(closing),
                closed: false,
            }),
            writer: crate::packet_relay::BatchWriter::direct(PacketSender::Native(sender)),
            relay: None,
            prefer_injected: false,
            joining: true,
            timers: bridge::tasks::Timers::tokio(),
            startup: Default::default(),
            received: Default::default(),
            ahead: Default::default(),
            ahead_bytes: 0,
            ended: false,
            sending: Default::default(),
            tap: None,
            current: None,
            split_frames: 0,
            delivered: (0, 0),
        }
    }
}
