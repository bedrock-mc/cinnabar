//! Established RakNet admission and retained finite local retirement.

use super::Inbox;
use bedrock_net::raknet::{ClosingRakNetConnection, RakNetConnection, RakNetError};
use bytes::Bytes;
use std::{io, time::Instant};

pub(in crate::login::native_network) struct RakNetTransport {
    connection: Option<Connection>,
    inbox: Inbox,
}

enum Connection {
    Established(RakNetConnection),
    Closing(ClosingRakNetConnection),
    Failed(Option<io::Error>),
}

impl RakNetTransport {
    /// Observes the same established transport without retaining its send capability.
    pub(super) fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        match self.connection.as_ref()? {
            Connection::Established(connection) => {
                Some(crate::RoundTripTime::raknet(connection.observe_rtt()))
            }
            _ => None,
        }
    }

    /// Retains only the actual successfully negotiated socket and its original queues.
    pub(super) fn new(connection: RakNetConnection) -> Self {
        Self {
            connection: Some(Connection::Established(connection)),
            inbox: Inbox::default(),
        }
    }

    /// Applies the original credential bound to capacity wait and immediate admission.
    pub(super) async fn send_until(
        &mut self,
        message: Bytes,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        let Connection::Established(connection) = self
            .connection
            .as_mut()
            .expect("synchronous transport transition")
        else {
            return Err(super::closing());
        };
        let capacity = connection.wait_send_capacity(message.len());
        match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline.into(), capacity)
                .await
                .map_err(|_| super::super::authentication::expired())?
                .map_err(io::Error::other)?,
            None => capacity.await.map_err(io::Error::other)?,
        }
        if let Some(deadline) = deadline {
            super::super::authentication::ensure_current(deadline, Instant::now())?;
        }
        connection.queue_send(message).map_err(io::Error::other)?;
        connection.flush().await.map_err(io::Error::other)
    }

    /// Receives the same accepted FIFO and terminal before or after consuming local send permission.
    pub(super) async fn recv(&mut self) -> io::Result<Bytes> {
        if let Some(message) = self.inbox.pop() {
            return Ok(message);
        }
        match self
            .connection
            .as_mut()
            .expect("synchronous transport transition")
        {
            Connection::Established(connection) => {
                connection.recv().await.map_err(io::Error::other)
            }
            Connection::Closing(connection) => connection.recv().await.map_err(io::Error::other),
            Connection::Failed(failure) => Err(failure
                .take()
                .unwrap_or_else(|| io::Error::other(RakNetError::Closed))),
        }
    }

    /// Services the same socket while retaining raw input before its first terminal.
    pub(super) async fn maintain(&mut self) {
        let received = match self
            .connection
            .as_mut()
            .expect("synchronous transport transition")
        {
            Connection::Established(connection) => connection.recv().await,
            Connection::Closing(connection) => connection.recv().await,
            Connection::Failed(_) => std::future::pending().await,
        }
        .map_err(io::Error::other);
        if let Err(error) = received.and_then(|message| self.inbox.push(message)) {
            // A rejected front seals the remainder; ordinary reads still drain its accepted prefix.
            self.connection = Some(Connection::Failed(Some(error)));
        }
    }

    /// Installs the closing owner before awaiting teardown, retaining it across canceled waits.
    pub(super) async fn close(&mut self) -> io::Result<()> {
        let current = self
            .connection
            .take()
            .expect("synchronous transport transition");
        let (connection, failure) = match current {
            Connection::Established(connection) => match connection.begin_close() {
                Ok(connection) => (connection, None),
                Err(error) => {
                    let (connection, cause) = error.into_parts();
                    (connection, Some(cause))
                }
            },
            Connection::Closing(connection) => (connection, None),
            Connection::Failed(failure) => {
                self.connection = Some(Connection::Failed(failure));
                return Ok(());
            }
        };
        self.connection = Some(Connection::Closing(connection));
        if let Some(failure) = failure {
            return Err(io::Error::other(failure));
        }
        let Some(Connection::Closing(connection)) = self.connection.as_mut() else {
            unreachable!("local close retained its actual closing owner")
        };
        connection.finish_close().await.map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests;
