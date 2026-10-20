//! Real framed Core and missing-owner controls for the existing Home/report consumers.

use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

/// Reads the same Core Home RPC used by the default worker, without any service access.
#[tokio::test]
async fn default_home_consumer_keeps_the_original_core_rpc() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    let shared = Mutex::new(Snapshot::default());
    let (_wake, mut changed) = watch::channel(Wake::Account);
    let mut feed = Feed::default();
    let source = CoreFeeds {
        socket_dir: Some(directory.path()),
        art: None,
    };
    let read = feed.round(&source, &shared, &mut changed);
    let reply = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let size = stream.read_u32().await.unwrap();
        let mut bytes = vec![0; size as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(request["method"], bridge::contract::METHOD_HOME);
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0", "id":request["id"], "result":{
                "schema_version":bridge::contract::CONTROL_SCHEMA_VERSION,
                "home":{"realm_invites":3, "treatments":["original"]}
            }
        }))
        .unwrap();
        stream.write_u32(body.len() as u32).await.unwrap();
        stream.write_all(&body).await.unwrap();
    };
    let ((home, failed), ()) = tokio::time::timeout(profile_worker::RESPONSE_TIMEOUT, async {
        tokio::join!(read, reply)
    })
    .await
    .unwrap();
    assert!(!failed);
    assert_eq!(home.unwrap().realm_invites, 3);
    let mut snapshot = shared.lock().unwrap();
    assert_eq!(snapshot.home.take().unwrap().treatments, ["original"]);
    assert!(snapshot.pending_home_scope.marker().is_none());
}

/// Native mode cannot disclose a different Core account when its selected owner is missing.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn missing_native_home_owner_never_reads_or_reports_through_core() {
    let directory = tempfile::tempdir().unwrap();
    let listener =
        std::os::unix::net::UnixListener::bind(directory.path().join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let shared = Mutex::new(Snapshot {
        home_source: Source::Native(None),
        ..Default::default()
    });
    let (_wake, mut changed) = watch::channel(Wake::Account);
    let source = CoreFeeds {
        socket_dir: Some(directory.path()),
        art: None,
    };
    let (home, failed) = Feed::default().round(&source, &shared, &mut changed).await;
    assert!(home.is_none() && failed);
    let report = Report::capture(&shared.lock().unwrap(), MessageEvent::default());
    assert!(
        report
            .send(Some(directory.path()), &shared, &mut changed)
            .await
            == ReportOutcome::Retired
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
