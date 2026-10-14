//! Connection tasks prepared for the host's executor. Native sockets keep Tokio offloading;
//! browser hosts can spawn the returned writer locally and supply their own timer future.

use std::future::Future;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{FrameQueue, FramedReader};

/// Prepares one bounded writer and reader without spawning or executing either inline.
/// The host must spawn the writer; dropping it closes the queue and wakes waiting senders.
/// No `Send` bound is imposed here on browser-owned stream handles. Native Tokio spawning
/// still requires `Send` on the resulting task.
pub fn prepare_connection<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: R,
    write: W,
    maximum: usize,
) -> (FramedReader<R>, FrameQueue, impl Future<Output = ()>) {
    crate::framed::prepare(read, write, maximum)
}

/// A caller-supplied deadline expired before the operation completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

/// Waits for an operation or a host timer, dropping the loser. Ready work wins a timer race,
/// preserving Tokio's native timeout behavior.
/// Native hosts pass Tokio timers; a browser host owns timer registration and cancellation.
pub async fn until<F: Future, T: Future<Output = ()>>(
    work: F,
    expires: T,
) -> Result<F::Output, Elapsed> {
    tokio::select! {
        biased;
        result = work => Ok(result),
        _ = expires => Err(Elapsed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::StreamExt;

    #[tokio::test]
    async fn host_driven_writer_preserves_order_and_completion() {
        let (local, peer) = tokio::io::duplex(32);
        let (read, write) = tokio::io::split(local);
        let (mut received, frames, writer) = prepare_connection(read, write, 8);
        let (peer_read, peer_write) = tokio::io::split(peer);
        let (mut peer_received, peer_frames, peer_writer) =
            prepare_connection(peer_read, peer_write, 8);
        let first = frames.accept(Bytes::from_static(b"one")).await.unwrap();
        let second = frames.accept(Bytes::from_static(b"two")).await.unwrap();
        drop(frames);
        peer_frames
            .accept(Bytes::from_static(b"reply"))
            .await
            .unwrap();
        drop(peer_frames);
        let observe = async {
            first.await.unwrap();
            second.await.unwrap();
            assert_eq!(peer_received.next().await.unwrap().unwrap(), b"one"[..]);
            assert_eq!(peer_received.next().await.unwrap().unwrap(), b"two"[..]);
            assert_eq!(received.next().await.unwrap().unwrap(), b"reply"[..]);
        };
        tokio::join!(writer, peer_writer, observe);
    }

    #[tokio::test]
    async fn dropping_unspawned_writer_closes_queue() {
        let (local, _peer) = tokio::io::duplex(8);
        let (read, write) = tokio::io::split(local);
        let (_, frames, writer) = prepare_connection(read, write, 8);
        let written = frames.accept(Bytes::from_static(b"one")).await.unwrap();
        drop(writer);
        assert!(written.await.is_err());
        assert!(frames.accept(Bytes::from_static(b"two")).await.is_err());
    }

    #[tokio::test]
    async fn caller_timer_cancels_work_without_platform_time() {
        struct Dropped(std::rc::Rc<std::cell::Cell<bool>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
        let guard = Dropped(dropped.clone());
        let (expire, expired) = tokio::sync::oneshot::channel();
        let work = async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        };
        let mut task = std::pin::pin!(until(work, async {
            expired.await.unwrap();
        }));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(task.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        expire.send(()).unwrap();
        assert_eq!(task.await, Err(Elapsed));
        assert!(
            dropped.get(),
            "expiry drops local work without requiring Send"
        );
        assert_eq!(
            until(std::future::ready(7), std::future::pending()).await,
            Ok(7)
        );
        assert_eq!(
            until(std::future::ready(7), std::future::ready(())).await,
            Ok(7)
        );
    }
}
