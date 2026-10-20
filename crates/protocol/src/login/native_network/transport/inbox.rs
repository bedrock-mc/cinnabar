//! Bounded wire input retained while its original transport keeps progressing.

use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::{collections::VecDeque, future::Future, io};

#[derive(Default)]
pub(in crate::login::native_network) struct Inbox {
    messages: VecDeque<Bytes>,
    bytes: usize,
}

impl Inbox {
    /// Removes the next wire message while releasing its byte charge.
    pub(super) fn pop(&mut self) -> Option<Bytes> {
        let message = self.messages.pop_front()?;
        self.bytes -= message.len();
        Some(message)
    }

    /// Retains one bounded wire message for the ordinary decoder to consume later.
    pub(super) fn push(&mut self, message: Bytes) -> io::Result<()> {
        let limits = bedrock_net::BatchLimits::default();
        let bytes = self
            .bytes
            .checked_add(message.len())
            .ok_or_else(|| io::Error::other("native wire receive budget overflow"))?;
        if bytes > limits.max_encoded_bytes || self.messages.len() >= limits.max_packets {
            return Err(io::Error::other("native wire receive queue is full"));
        }
        self.bytes = bytes;
        self.messages.push_back(message);
        Ok(())
    }

    /// Keeps driver input progressing until this write completes, with bounded retention.
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
                message = incoming.next() => {
                    let message = message.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "NetherNet peer closed during write"))??;
                    self.push(message)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::{mpsc, oneshot};

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
