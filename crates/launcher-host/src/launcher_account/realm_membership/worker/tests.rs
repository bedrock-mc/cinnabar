use super::*;
use std::{
    future::Future,
    task::{Context, Waker},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// The observation owner survives actual cancellation/Stop until its original accepted write completes.
/// This qualifies retained I/O, not native Realms authentication or a live membership write.
#[tokio::test]
async fn realm_membership_observation_retains_actual_dispatched_io_through_stop() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let (wake, mut changed) = watch::channel(Wake::Account);
    let requests = Requests {
        requests: sender,
        wake: wake.clone(),
    };
    let (completed, mut completion) = tokio::sync::oneshot::channel();
    let deadline = tokio::time::Instant::now() + profile_worker::RESPONSE_TIMEOUT;
    let phase = tokio::time::timeout_at(deadline, async {
        let mut connection = TcpStream::connect(address).await.unwrap();
        connection.write_u8(1).await.unwrap();
        assert_eq!(connection.read_u8().await.unwrap(), 1);
        completed.send(()).unwrap();
    });
    let mut pending = None;
    let mut observing = Box::pin(observe(phase, &mut receiver, &mut pending, &mut changed));
    let mut stream = tokio::select! {
        request = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(stream.read_u8().await.unwrap(), 1);
            stream
        } => request,
        _ = &mut observing => panic!("observation ended before actual dispatch"),
    };
    requests.cancel(&Mutex::new(Snapshot::default()));
    drop(requests);
    retire(&wake);
    assert!(*wake.borrow() == Wake::Stop);
    assert!(
        observing
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert!(
        {
            let mut byte = [0];
            let mut read = std::pin::pin!(stream.read(&mut byte));
            read.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        },
        "retirement dropped the dispatched operation's actual socket"
    );
    stream.write_u8(1).await.unwrap();
    assert!(
        observing.await.is_none(),
        "retired intent must not publish its late response"
    );
    completion
        .try_recv()
        .expect("the actual original write response was observed");
    assert!(pending.is_none());
    assert!(
        std::pin::pin!(listener.accept())
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending(),
        "the original acceptance was resubmitted"
    );
}
