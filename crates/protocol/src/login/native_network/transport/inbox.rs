//! Bounded wire input retained while its original transport keeps progressing.

use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::{collections::VecDeque, future::Future, io};

#[derive(Default)]
pub(in crate::login::native_network) struct Inbox {
    messages: VecDeque<Bytes>,
    bytes: usize,
    pending_message: Option<Bytes>,
}

impl Inbox {
    /// Releases the charged FIFO prefix before its one unadmitted wire head.
    pub(super) fn pop(&mut self) -> Option<Bytes> {
        if let Some(message) = self.messages.pop_front() {
            self.bytes -= message.len();
            Some(message)
        } else {
            self.pending_message.take()
        }
    }

    /// Retains one bounded wire message for the ordinary decoder to consume later.
    #[cfg(test)]
    pub(super) fn push(&mut self, message: Bytes) -> io::Result<()> {
        match self.try_push(message)? {
            None => Ok(()),
            Some(_) => Err(Self::full_error()),
        }
    }

    /// Stops reads at the original FIFO bound or while its one legal refused head is retained.
    pub(super) fn is_full(&self) -> bool {
        let limits = bedrock_net::BatchLimits::default();
        self.pending_message.is_some()
            || self.bytes >= limits.max_encoded_bytes
            || self.messages.len() >= limits.max_packets
    }

    /// Returns the same unadmitted head on aggregate pressure without changing the FIFO.
    pub(super) fn try_push(&mut self, message: Bytes) -> io::Result<Option<Bytes>> {
        let limits = bedrock_net::BatchLimits::default();
        let bytes = self
            .bytes
            .checked_add(message.len())
            .ok_or_else(|| io::Error::other("native wire receive budget overflow"))?;
        if message.len() > limits.max_encoded_bytes {
            return Err(Self::full_error());
        }
        if self.pending_message.is_some()
            || bytes > limits.max_encoded_bytes
            || self.messages.len() >= limits.max_packets
        {
            return Ok(Some(message));
        }
        self.bytes = bytes;
        self.messages.push_back(message);
        Ok(None)
    }

    /// Keeps a just-read legal head outside the full FIFO without accepting a successor.
    /// Callers stop application reads until pop releases this same retained body.
    pub(super) fn retain(&mut self, message: Bytes) -> io::Result<()> {
        if self.pending_message.is_some() {
            return Err(Self::full_error());
        }
        self.pending_message = self.try_push(message)?;
        Ok(())
    }

    /// Shares the original write-side EOF diagnostic between the concrete input owners.
    pub(super) fn closed_during_write() -> io::Error {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "NetherNet peer closed during write",
        )
    }

    /// Preserves the strict admission failure diagnostic for all original callers.
    fn full_error() -> io::Error {
        io::Error::other("native wire receive queue is full")
    }

    /// Polls the same write while its independently driven actor retains paused input.
    pub(super) async fn send<T>(
        &mut self,
        sending: impl Future<Output = io::Result<T>> + Send,
        incoming: impl Stream<Item = io::Result<Bytes>> + Send,
    ) -> io::Result<T> {
        tokio::pin!(sending, incoming);
        loop {
            tokio::select! {
                biased;
                result = &mut sending => return result,
                message = incoming.next(), if !self.is_full() => {
                    self.retain(message.ok_or_else(Self::closed_during_write)??)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::{mpsc, oneshot};

    #[test]
    fn byte_pressure_preserves_the_unadmitted_head_and_strict_fifo_contract() {
        let limit = bedrock_net::BatchLimits::default().max_encoded_bytes;
        let first = Bytes::from(vec![0xfe; limit.checked_sub(1).unwrap()]);
        let head = Bytes::from_static(b"head");
        assert!(head.len() <= limit);
        let mut inbox = Inbox::default();
        inbox.push(first.clone()).unwrap();
        assert!(!inbox.is_full());
        assert!(
            inbox.push(head.clone()).is_err(),
            "strict insertion must still reject aggregate overflow"
        );
        let head = inbox
            .try_push(head)
            .unwrap()
            .expect("the original body must survive aggregate refusal");
        assert_eq!(head.as_ref(), b"head");
        assert_eq!(inbox.pop().unwrap(), first);
        assert!(inbox.pop().is_none(), "refusal changed the original FIFO");
        assert!(!inbox.is_full());
        assert!(inbox.try_push(head).unwrap().is_none());
        assert_eq!(inbox.pop().unwrap().as_ref(), b"head");
        assert!(inbox.pop().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn write_completes_when_the_driver_must_deliver_input_before_accepting_it() {
        let (send, mut receive) = mpsc::channel(1);
        let (accepted, completion) = oneshot::channel();
        let peer = async move {
            send.send(Bytes::from_static(b"first")).await.unwrap();
            send.send(Bytes::from_static(b"second")).await.unwrap();
            let _ = accepted.send(());
        };
        let incoming = futures::stream::unfold(&mut receive, |receive| async move {
            receive.recv().await.map(|message| (Ok(message), receive))
        });
        let mut inbox = Inbox::default();
        let write = inbox.send(
            async { completion.await.map_err(io::Error::other) },
            incoming,
        );
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(write, peer).0
        })
        .await;
        assert!(
            result.is_ok(),
            "write waited for input that only its owner can consume"
        );
        result.unwrap().unwrap();
        assert_eq!(inbox.pop().unwrap(), b"first"[..]);
        let second = match inbox.pop() {
            Some(message) => message,
            None => receive.recv().await.unwrap(),
        };
        assert_eq!(second, b"second"[..]);
    }
}

#[cfg(test)]
mod backpressure_tests;
