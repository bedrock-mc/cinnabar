use super::*;

#[tokio::test]
async fn prepare_connect_warms_one_target_and_cancels_without_selecting_a_join() {
    let directory = tempfile::tempdir().unwrap();
    let listener =
        tokio::net::UnixListener::bind(crate::control_endpoint_path(directory.path())).unwrap();
    let fixture =
        tokio::spawn(async move {
            for expected in [
                serde_json::json!({"kind":"raknet", "value":"selected.test:19132"}),
                serde_json::json!({"kind":"", "value":""}),
            ] {
                let (stream, _) = listener.accept().await.unwrap();
                let mut framed = FramedStream::with_max(
                    crate::endpoint::PlatformStream::Unix(stream),
                    CONTROL_MAX_FRAME_LEN,
                );
                let request: serde_json::Value =
                    serde_json::from_slice(&framed.next().await.unwrap().unwrap()).unwrap();
                assert_eq!(request["method"], "prepare_connect.v1");
                assert_eq!(request["params"], expected);
                framed.send(Bytes::from(serde_json::to_vec(&serde_json::json!({
                "jsonrpc":"2.0", "id":REQUEST_ID, "result":{"schema_version":SCHEMA_VERSION}
            })).unwrap())).await.unwrap();
            }
        });
    let target = ConnectTarget::RakNet("selected.test:19132".into());
    prepare_connect_target(directory.path(), Some(&target))
        .await
        .unwrap();
    prepare_connect_target(directory.path(), None)
        .await
        .unwrap();
    fixture.await.unwrap();
}
