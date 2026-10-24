//! Fixed Legacy host bridge using the original caller driver and SDK-decoded envelopes.

use super::*;
use bedrock_services::signaling::{
    Event, RequestId,
    websocket::{MessagingSocket, SocketError},
};
use nethernet_tokio::{
    CallerServerSignaling, CallerServerSignalingDriver, NetherServer, SignalDelivery, SignalRequest,
};

pub(super) struct Listener {
    server: NetherServer,
    driver: CallerServerSignalingDriver,
    socket: MessagingSocket,
    pending: Option<(RequestId, SignalRequest)>,
    deadline: Instant,
}

impl Listener {
    /// Binds exactly this fixed host's original Legacy socket and bounded unsolicited route owner.
    pub(super) async fn bind(
        socket: MessagingSocket,
        local: u64,
        config: ConnectionConfig,
        deadline: Instant,
    ) -> io::Result<Self> {
        let (caller, driver) =
            CallerServerSignaling::new(local.to_string(), Default::default()).map_err(failed)?;
        let server = NetherServer::bind_with(caller, config)
            .await
            .map_err(failed)?;
        Ok(Self {
            server,
            driver,
            socket,
            pending: None,
            deadline,
        })
    }

    /// Drives actual Legacy service progress while retaining the listener's original accept future.
    pub(super) async fn accept(&mut self) -> io::Result<AcceptedSession> {
        let Self {
            server,
            driver,
            socket,
            pending,
            deadline,
        } = self;
        tokio::select! {
            result = server.accept() => result.map_err(failed),
            result = maintain(driver, socket, pending, *deadline) => Err(failed(result)),
        }
    }

    /// Keeps the same physical reader and one correlated transport send beside peer I/O.
    pub(super) async fn during<F: Future>(&mut self, operation: F) -> io::Result<F::Output> {
        tokio::select! {
            result = operation => Ok(result),
            terminal = maintain(&mut self.driver, &mut self.socket, &mut self.pending, self.deadline) => Err(failed(terminal)),
        }
    }

    /// Seals routes, drains the original socket Close and joins the original transport workers.
    pub(super) async fn close(mut self) -> io::Result<()> {
        self.driver.close();
        self.pending = None;
        self.socket.close();
        let (socket, server) = tokio::join!(
            async {
                loop {
                    match self.socket.drive().await {
                        Ok(_) => {}
                        Err(SocketError::Closed) => return Ok(()),
                        Err(error) => return Err(failed(error)),
                    }
                }
            },
            self.server.close()
        );
        socket.and(server.map_err(failed))
    }
}

/// Forwards the SDK's checked sender/signal through the same original transport delivery owner.
/// This fixed one-peer fixture retains one outgoing request until actual service completion.
async fn maintain(
    driver: &mut CallerServerSignalingDriver,
    socket: &mut MessagingSocket,
    pending: &mut Option<(RequestId, SignalRequest)>,
    original_deadline: Instant,
) -> io::Result<()> {
    loop {
        tokio::select! {
            request = driver.recv(), if pending.is_none() => {
                let request = request.ok_or_else(|| failed(()))?;
                let id = socket.send_legacy_signal(&request.signal().network_id, uuid::Uuid::new_v4(),
                    &request.signal().to_string(), false, request.deadline()).map_err(failed)?;
                *pending = Some((id, request));
            }
            event = socket.drive() => match event.map_err(failed)? {
                Event::LegacySignal(signal) => {
                    let deadline = original_deadline.min(Instant::now() + CallerServerSignalingConfig::default().signaling.send_timeout);
                    driver.prepare_delivery_before(signal.sender(), signal.signal(), deadline)
                        .map_err(failed)?.await.map_err(failed)?;
                }
                Event::Completed { request, result }
                    if pending.as_ref().is_some_and(|(id, _)| *id == request) => {
                    let (_, original) = pending.take().unwrap();
                    if result.is_err() { return Err(failed(())); }
                    original.finish(Ok(SignalDelivery::ServiceAccepted));
                }
                _ => {},
            }
        }
    }
}
