//! The signed service signaling edge of the existing native connection owner.

use super::Inbox;
use crate::login::native_network::authentication::{
    MessagingAdmission, MessagingTarget, ensure_current, expired,
};
use bedrock_net::messaging::{ClosingMessagingConnection, MessagingConnection};
use bytes::Bytes;
use nethernet_tokio::{ConnectionConfig, NetherError};
use std::{io, time::Instant};

pub(in crate::login::native_network) struct MessagingTransport {
    connection: Option<Connection>,
    target: MessagingTarget,
    inbox: Inbox,
}

enum Connection {
    Established(MessagingConnection),
    Closing(ClosingMessagingConnection),
}

impl MessagingTransport {
    /// Observes the same established transport without retaining its send capability.
    pub(super) fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        match self.connection.as_ref()? {
            Connection::Established(connection) => Some(crate::RoundTripTime::nethernet(
                connection.sender().observe_rtt(),
            )),
            _ => None,
        }
    }

    /// Checks the exact joined-session route before using the protected socket.
    pub(super) async fn connect(
        endpoint: &crate::NativeConnectionTarget,
        admission: Option<MessagingAdmission>,
    ) -> io::Result<Self> {
        let admission = admission.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native messaging requires selected-account admission",
            )
        })?;
        let target = admission.target.connection_target();
        if endpoint.address != target.address || endpoint.transport != target.transport {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native messaging target mismatch",
            ));
        }
        ensure_current(admission.expires, Instant::now())?;
        let expires = admission.expires;
        let connection = admission
            .prepared
            .connect(
                ConnectionConfig {
                    identity: Some(admission.identity),
                    server_trust: Some(admission.trust),
                    allow_anonymous_answers: admission.target.allow_anonymous_answers(),
                    attempts: 1,
                    ..ConnectionConfig::default()
                },
                admission.expires,
            )
            .await
            .map_err(|error| {
                if Instant::now() >= expires {
                    expired()
                } else {
                    io::Error::other(error)
                }
            })?;
        Ok(Self {
            connection: Some(Connection::Established(connection)),
            target: admission.target,
            inbox: Inbox::default(),
        })
    }

    /// Borrows only the explicitly admitted session challenge.
    pub(super) fn nonce(&self) -> &str {
        self.target.nonce()
    }

    /// Drives service and incoming data while waiting for actual SCTP output admission.
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
        let sender = connection.sender();
        let sending = async {
            let result = match deadline {
                Some(deadline) => sender.send_before(message, deadline).await,
                None => sender.send(message).await,
            };
            match result {
                Err(NetherError::Timeout) if deadline.is_some() => Err(expired()),
                result => result.map_err(io::Error::other),
            }
        };
        tokio::pin!(sending);
        loop {
            if self.inbox.is_full() {
                tokio::select! {
                    biased;
                    result = &mut sending => return result,
                    _ = connection.maintain() => {},
                }
            } else {
                tokio::select! {
                    biased;
                    result = &mut sending => return result,
                    message = connection.recv() => {
                        let message = message.map_err(io::Error::other)?
                            .ok_or_else(Inbox::closed_during_write)?;
                        self.inbox.retain(message)?;
                    }
                }
            }
        }
    }

    /// Preserves messages accepted while a write was pending, ahead of the next transport read.
    pub(super) async fn recv(&mut self) -> io::Result<Bytes> {
        if let Some(message) = self.inbox.pop() {
            return Ok(message);
        }
        let result = match self
            .connection
            .as_mut()
            .expect("synchronous transport transition")
        {
            Connection::Established(connection) => connection.recv().await,
            Connection::Closing(connection) => connection.recv().await,
        };
        result
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "NetherNet peer closed"))
    }

    /// Drives only service/control/signaling; terminal reporting waits behind accepted input.
    pub(super) async fn maintain(&mut self) {
        match self
            .connection
            .as_mut()
            .expect("synchronous transport transition")
        {
            Connection::Established(connection) => connection.maintain().await,
            Connection::Closing(_) => std::future::pending().await,
        }
    }

    /// Consumes local sending synchronously while retaining the exact service/receiver drain owner.
    pub(super) fn close(&mut self) {
        let current = self
            .connection
            .take()
            .expect("synchronous transport transition");
        self.connection = Some(match current {
            Connection::Established(connection) => Connection::Closing(connection.begin_close()),
            closing @ Connection::Closing(_) => closing,
        });
    }
}
