//! Final process authority retires the original driver, independently of graceful close.

use super::*;
use bridge::tasks::NativeExecution;
use tokio_util::task::TaskTracker;

/// The socket is destroyed before the original HeldClose field sends its destruction ACK.
struct SocketClose {
    _socket: tokio::net::UdpSocket,
    held: HeldClose,
}

impl PacketIo for SocketClose {
    async fn send_body(&mut self, body: Bytes) -> io::Result<()> {
        self.held.send_body(body).await
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        self.held.recv_body().await
    }

    async fn close(&mut self) -> io::Result<()> {
        self.held.close().await
    }
}

/// Uses the real retained task, destruction ACK and socket, without a wall-clock shutdown race.
#[tokio::test(start_paused = true)]
async fn process_stop_destroys_the_original_driver_before_parent_completion() {
    let mut observations = Vec::new();
    for stopped_before_spawn in [false, true] {
        let tasks = TaskTracker::new();
        let (execution, stop) =
            NativeExecution::for_process(tokio::runtime::Handle::current(), tasks.clone());
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let (entered, mut closing) = oneshot::channel();
        let (release, held) = oneshot::channel();
        let (dropped, retired) = oneshot::channel();
        let pump = prepare(SocketClose {
            _socket: socket,
            held: HeldClose {
                entered: Some(entered),
                release: held,
                dropped: Some(dropped),
            },
        });
        let pending_receipt = if stopped_before_spawn {
            Some(
                pump.sender
                    .accept(Bytes::from_static(b"accepted-before-process-stop"))
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        if stopped_before_spawn {
            stop.stop();
        }
        let (sender, mut reader) = NativeReader::spawn(pump, None, Some(execution));
        let result = {
            let mut close = std::pin::pin!(reader.close());
            if !stopped_before_spawn {
                let _pending = futures::poll!(close.as_mut());
                (&mut closing).await.unwrap();
                stop.stop();
            }
            close.await
        };
        let error_kind = result.err().map(|error| error.kind());
        let task_joined = reader.task.is_none();
        let failed_receipt = match pending_receipt {
            Some(receipt) => !matches!(receipt.await, Ok(Ok(()))),
            None => true,
        };
        let close_not_entered = !stopped_before_spawn || closing.try_recv().is_err();
        let _ = release.send(());
        retired.await.unwrap();
        drop(reader);
        drop(sender);
        tasks.close();
        tasks.wait().await;
        let socket_released = std::net::UdpSocket::bind(address).is_ok();
        observations.push((
            error_kind,
            task_joined && tasks.is_empty() && socket_released,
            failed_receipt && close_not_entered,
        ));
    }
    assert!(observations.iter().all(|(_, retired, _)| *retired));
    assert!(
        observations
            .iter()
            .all(|(cause, _, _)| *cause == Some(io::ErrorKind::Interrupted)),
        "process stop did not retire the original driver: {observations:?}"
    );
    assert!(observations.iter().all(|(_, _, no_effect)| *no_effect));
}
