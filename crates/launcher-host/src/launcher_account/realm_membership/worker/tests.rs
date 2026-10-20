use super::*;
use std::{
    future::Future,
    task::{Context, Waker},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

/// The observation owner survives actual cancellation/Stop until its original accepted RPC completes.
/// This qualifies retained I/O, not native Realms authentication or a live membership write.
#[tokio::test]
async fn realm_membership_observation_retains_actual_dispatched_io_through_stop() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let (wake, mut changed) = watch::channel(Wake::Account);
    let requests = Requests {
        requests: sender,
        wake: wake.clone(),
    };
    let (completed, mut completion) = tokio::sync::oneshot::channel();
    let deadline = tokio::time::Instant::now() + profile_worker::RESPONSE_TIMEOUT;
    let phase = tokio::time::timeout_at(deadline, async {
        let reply = bridge::realm_membership(directory.path(), "fixture", true)
            .await
            .unwrap();
        assert_eq!(reply.code, "fixture");
        assert!(reply.realm.member);
        completed.send(()).unwrap();
    });
    let mut pending = None;
    let mut observing = Box::pin(observe(phase, &mut receiver, &mut pending, &mut changed));
    let (mut stream, request) = tokio::select! {
        request = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let size = stream.read_u32().await.unwrap();
            let mut body = vec![0; size as usize];
            stream.read_exact(&mut body).await.unwrap();
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            (stream, request)
        } => request,
        _ = &mut observing => panic!("observation ended before actual dispatch"),
    };
    assert_eq!(request["method"], bridge::contract::METHOD_REALM_MEMBERSHIP);
    assert_eq!(request["params"]["accept"], true);

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
    let response = serde_json::to_vec(&serde_json::json!({
        "jsonrpc":"2.0", "id":request["id"], "result":{
            "schema_version":bridge::contract::CONTROL_SCHEMA_VERSION,
            "code":"fixture", "realm":{"name":"Fixture", "state":"OPEN", "target":"realm_id/7", "member":true}
        }
    }))
    .unwrap();
    stream.write_u32(response.len() as u32).await.unwrap();
    stream.write_all(&response).await.unwrap();
    assert!(
        observing.await.is_none(),
        "retired intent must not publish its late response"
    );
    completion
        .try_recv()
        .expect("the actual original RPC response was observed");
    assert!(pending.is_none());
    assert!(
        std::pin::pin!(listener.accept())
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending(),
        "the original acceptance was resubmitted"
    );
}
