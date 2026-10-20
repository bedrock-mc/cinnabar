//! The actual ready server association, retaining its original receivers through local close.

use super::Inbox;
use bytes::Bytes;
use nethernet_tokio::{AcceptedSession, Retired, SessionSender};
use std::{future::Future, io, sync::Arc, time::Instant};

pub(in crate::login::native_network) struct AcceptedTransport {
    state: Option<State>,
    inbox: Inbox,
    input_failure: Option<Arc<io::Error>>,
    input_terminal_seen: bool,
    #[cfg(feature = "test-support")]
    fixture_nonce: Option<String>,
}
enum State {
    Established(AcceptedSession),
    Retired(AcceptedSession<Retired>),
}

impl AcceptedTransport {
    /// Observes the original ready association without extending its transport lifetime.
    pub(super) fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        self.sender()
            .ok()
            .map(|sender| crate::RoundTripTime::nethernet(sender.observe_rtt()))
    }

    /// Takes an existing Ready association; no new socket, dial or signaling route is created.
    pub(super) fn new(accepted: AcceptedSession) -> Self {
        Self {
            state: Some(State::Established(accepted)),
            inbox: Inbox::default(),
            input_failure: None,
            input_terminal_seen: false,
            #[cfg(feature = "test-support")]
            fixture_nonce: None,
        }
    }

    /// Retains only a fixture caller's original challenge on its actual already-established client.
    #[cfg(feature = "test-support")]
    pub(super) fn fixture_client(client: nethernet_tokio::NetherClient, nonce: String) -> Self {
        let mut transport = Self::new(client.into_accepted());
        transport.fixture_nonce = Some(nonce);
        transport
    }

    /// Normal server construction has no fixture challenge and retains its empty transport nonce.
    #[cfg(feature = "test-support")]
    pub(super) fn fixture_nonce(&self) -> &str {
        self.fixture_nonce.as_deref().unwrap_or_default()
    }

    /// Borrows capacity from this exact established association, without exposing its sender.
    pub(super) fn sender(&self) -> io::Result<SessionSender> {
        match self.state.as_ref().expect("synchronous phase transition") {
            State::Established(accepted) => Ok(accepted.session.sender()),
            State::Retired(_) => Err(super::closing()),
        }
    }

    /// Distinguishes a completed operation result from a retained-input failure.
    pub(super) async fn progress<T: Send>(
        &mut self,
        progress: impl Future<Output = T> + Send,
    ) -> io::Result<T> {
        if let Some(error) = &self.input_failure {
            return Err(io::Error::new(error.kind(), Arc::clone(error)));
        }
        let receiver = match self.state.as_mut().expect("synchronous phase transition") {
            State::Established(accepted) => &mut accepted.reliable,
            State::Retired(accepted) => &mut accepted.reliable,
        };
        let incoming = futures::stream::unfold(receiver, |receiver| async move {
            match receiver.recv().await {
                Ok(Some(data)) => Some((Ok(data), receiver)),
                Ok(None) => None,
                Err(error) => Some((Err(io::Error::other(error)), receiver)),
            }
        });
        match self
            .inbox
            .send(async { Ok(progress.await) }, incoming)
            .await
        {
            Ok(result) => Ok(result),
            Err(error) => {
                let error = Arc::new(error);
                let result = io::Error::new(error.kind(), Arc::clone(&error));
                self.input_failure = Some(error);
                Err(result)
            }
        }
    }

    /// Settles an already committed receipt without repolling failed or exhausted input.
    pub(super) async fn settle<T: Send>(
        &mut self,
        progress: impl Future<Output = T> + Send,
    ) -> io::Result<T> {
        if self.input_failure.is_some() {
            Ok(progress.await)
        } else {
            self.progress(progress).await
        }
    }

    /// Drives original accepted input beside the same bounded actor's send admission.
    pub(super) async fn send_until(
        &mut self,
        data: Bytes,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        let State::Established(accepted) =
            self.state.as_mut().expect("synchronous phase transition")
        else {
            return Err(super::closing());
        };
        let sender = accepted.session.sender();
        let send = async move {
            match deadline {
                Some(deadline) => sender.send_before(data, deadline).await,
                None => sender.send(data).await,
            }
            .map_err(io::Error::other)
        };
        let incoming = futures::stream::unfold(&mut accepted.reliable, |receiver| async move {
            match receiver.recv().await {
                Ok(Some(data)) => Some((Ok(data), receiver)),
                Ok(None) => None,
                Err(error) => Some((Err(io::Error::other(error)), receiver)),
            }
        });
        self.inbox.send(send, incoming).await
    }

    /// Drains the accepted write prefix before the original established/retired receive FIFO.
    pub(super) async fn recv(&mut self) -> io::Result<Bytes> {
        if let Some(data) = self.inbox.pop() {
            return Ok(data);
        }
        if let Some(error) = &self.input_failure {
            if !self.input_terminal_seen {
                self.input_terminal_seen = true;
                return Err(io::Error::new(error.kind(), Arc::clone(error)));
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "accepted NetherNet peer closed",
            ));
        }
        let result = match self.state.as_mut().expect("synchronous phase transition") {
            State::Established(accepted) => accepted.reliable.recv().await,
            State::Retired(accepted) => accepted.reliable.recv().await,
        };
        result.map_err(io::Error::other)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "accepted NetherNet peer closed",
            )
        })
    }

    /// Consumes established admission without replacing or dropping either accepted receiver.
    pub(super) fn close(&mut self) {
        self.state = Some(
            match self.state.take().expect("synchronous phase transition") {
                State::Established(accepted) => State::Retired(accepted.close()),
                retired @ State::Retired(_) => retired,
            },
        );
    }
}
