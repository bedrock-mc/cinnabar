//! Pump contract coverage; the real SDK socket heartbeat is covered in its owning crate.

use super::*;
use std::{future::pending, time::Duration};

pub(crate) struct ServiceRequest {
    deadline: tokio::time::Instant,
    end: bool,
    outcome: oneshot::Sender<io::Result<()>>,
}

pub(crate) struct ServiceConnection {
    incoming: mpsc::Receiver<Bytes>,
    service: mpsc::Receiver<ServiceRequest>,
    full: Option<oneshot::Sender<()>>,
    terminal: Option<io::Error>,
    reads: usize,
}

/// Creates the existing caller-driven service boundary with independent bounded payload input.
pub(crate) fn connection() -> (
    ServiceConnection,
    mpsc::Sender<Bytes>,
    mpsc::Sender<ServiceRequest>,
) {
    let (incoming, receive) = mpsc::channel(2);
    let (service, requests) = mpsc::channel(1);
    (
        ServiceConnection {
            incoming: receive,
            service: requests,
            full: None,
            terminal: None,
            reads: 0,
        },
        incoming,
        service,
    )
}

impl PacketConnection for ServiceConnection {
    async fn send_body(&mut self, _: Bytes) -> io::Result<()> {
        Ok(())
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        let body = match self.incoming.try_recv() {
            Ok(body) => body,
            Err(_) if self.terminal.is_some() => return Err(self.terminal.take().unwrap()),
            Err(_) => self.incoming.recv().await.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "synthetic payload source closed",
                )
            })?,
        };
        self.reads += 1;
        if self.reads == QUEUED_BATCHES + 1
            && let Some(full) = self.full.take()
        {
            let _ = full.send(());
        }
        Ok(body)
    }

    async fn maintain(&mut self) {
        if self.terminal.is_some() {
            pending::<()>().await;
        }
        let Some(request) = self.service.recv().await else {
            return pending().await;
        };
        let result = if tokio::time::Instant::now() >= request.deadline {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "synthetic service lease expired",
            ))
        } else {
            Ok(())
        };
        if request.end {
            self.terminal = Some(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "synthetic service ended",
            ));
        }
        let _ = request.outcome.send(result);
    }

    async fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Queues one finite service lease independently of bounded world payload input.
pub(crate) async fn request_service(
    service: &mpsc::Sender<ServiceRequest>,
    end: bool,
) -> (tokio::time::Instant, oneshot::Receiver<io::Result<()>>) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let (outcome, result) = oneshot::channel();
    service
        .send(ServiceRequest {
            deadline,
            end,
            outcome,
        })
        .await
        .unwrap();
    (deadline, result)
}

/// Receive capacity cannot suspend the service lease, and service termination preserves every accepted prefix.
#[tokio::test(start_paused = true)]
async fn service_lease_progress_survives_full_native_receive_capacity() {
    let (incoming, receive) = mpsc::channel(QUEUED_BATCHES + 2);
    let (service, requests) = mpsc::channel(1);
    let (full, held_full) = oneshot::channel();
    let mut connection = ServiceConnection {
        incoming: receive,
        service: requests,
        full: Some(full),
        terminal: None,
        reads: 0,
    };
    // Establish that the fixture's lease itself is healthy before moving it into the actual pump.
    let (_, healthy) = request_service(&service, false).await;
    connection.maintain().await;
    healthy.await.unwrap().unwrap();
    let mut transport = SessionTransport::from_native(connection);
    for index in 0..QUEUED_BATCHES + 2 {
        incoming.send(Bytes::from(vec![index as u8])).await.unwrap();
    }
    held_full.await.unwrap();
    for _ in 0..2 {
        tokio::time::advance(Duration::from_secs(2)).await;
        let (deadline, lease) = request_service(&service, false).await;
        tokio::time::timeout_at(deadline, lease)
            .await
            .expect("native receive backpressure expired a healthy service lease")
            .unwrap()
            .unwrap();
    }
    let (deadline, ended) = request_service(&service, true).await;
    tokio::time::timeout_at(deadline, ended)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Reader::Native(reader) = &mut transport.reader else {
        panic!("native fixture reader");
    };
    for index in 0..QUEUED_BATCHES + 2 {
        let body = std::future::poll_fn(|cx| reader.poll_next(cx))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(body, Bytes::from(vec![index as u8]));
    }
    let terminal = std::future::poll_fn(|cx| reader.poll_next(cx))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(terminal.kind(), io::ErrorKind::ConnectionAborted);
    assert!(
        std::future::poll_fn(|cx| reader.poll_next(cx))
            .await
            .is_none()
    );
    assert!(
        std::future::poll_fn(|cx| reader.poll_driver(cx))
            .await
            .unwrap()
            .is_none()
    );
}
