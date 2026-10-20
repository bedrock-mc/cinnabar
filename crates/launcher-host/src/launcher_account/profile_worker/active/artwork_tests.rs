use super::*;
use crate::remote_images::{LAUNCHER_ART, Surface};
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    task::{Context, Waker},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UnixListener},
    sync::oneshot,
};

enum Action {
    Budget,
    AccountReplacement,
    Stop,
}

/// Consumes a complete bounded original fixture request without depending on TCP segmentation.
async fn headers(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 4096, "fixture HTTP headers too large");
        bytes.push(stream.read_u8().await.unwrap());
    }
    String::from_utf8(bytes).unwrap()
}

/// Serves one cacheable image, then observes cancellation of the next actual pending request.
async fn serve_images(listener: TcpListener, admitted: oneshot::Sender<()>) {
    let (mut fast, _) = listener.accept().await.unwrap();
    assert!(headers(&mut fast).await.starts_with("GET /fast "));
    let body = b"\x89PNG\r\n\x1a\noriginal-fixture";
    fast.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    fast.write_all(body).await.unwrap();
    drop(fast);
    let (mut stalled, _) = listener.accept().await.unwrap();
    assert!(headers(&mut stalled).await.starts_with("GET /stall "));
    admitted.send(()).unwrap();
    let terminal = stalled.read(&mut [0]).await;
    assert!(
        matches!(terminal, Ok(0))
            || terminal.is_err_and(|error| matches!(
                error.kind(),
                io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted,
            )),
        "retired image request remained open"
    );
}

/// Loads actual Core profile data, then controls only the caller's already-admitted artwork phase.
async fn artwork_case(action: Action) -> (Outcome, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fast_url = format!("{base}/fast");
    let stall_url = format!("{base}/stall");
    let (admitted, admission) = oneshot::channel();
    let image_server = tokio::spawn(serve_images(listener, admitted));
    let images = ImageDirectory::new(
        directory.path().join("art"),
        Surface {
            https_only: false,
            public_hosts: false,
            timeout: RESPONSE_TIMEOUT + ARTWORK_BUDGET,
            ..LAUNCHER_ART
        },
    );
    let cached = images.fetch(&fast_url).await.unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    let core = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let size = stream.read_u32().await.unwrap();
        let mut bytes = vec![0; usize::try_from(size).unwrap()];
        stream.read_exact(&mut bytes).await.unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(request["method"], bridge::contract::METHOD_PROFILE);
        let reply = serde_json::to_vec(&json!({
            "jsonrpc":bridge::contract::JSON_RPC_VERSION,"id":request["id"],"result":{
                "schema_version":bridge::contract::CONTROL_SCHEMA_VERSION,"profile":{
                    "gamertag":"Completed Profile","xuid":"123","friends":3,"followers":4,
                    "gamerscore":42,"gamerpic":{"url":fast_url},
                    "featured_screenshot":{"url":stall_url}}}}))
        .unwrap();
        stream
            .write_u32(u32::try_from(reply.len()).unwrap())
            .await
            .unwrap();
        stream.write_all(&reply).await.unwrap();
    });
    let (sender, mut requests) = channel();
    requests.borrow_and_update();
    let shared = Mutex::new(Snapshot {
        profile_wake: Some(sender.clone()),
        ..Default::default()
    });
    let selection = Selection::capture(&shared);
    let mut profile = Box::pin(run(
        Some(directory.path()),
        &images,
        &shared,
        &selection,
        &mut requests,
        RESPONSE_TIMEOUT,
    ));
    tokio::select! {
        result = admission => result.unwrap(),
        _ = &mut profile => panic!("profile ended before actual artwork admission"),
    }
    assert!(refresh(&sender));
    tokio::task::yield_now().await;
    assert!(
        profile
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending(),
        "same-account refresh discarded the pending artwork phase"
    );
    match action {
        Action::Budget => {
            tokio::time::pause();
            tokio::time::advance(ARTWORK_BUDGET).await;
            tokio::time::resume();
        }
        Action::AccountReplacement => shared.lock().unwrap().retire_account_data(),
        Action::Stop => {
            stop(&sender);
            shared.lock().unwrap().retire_account_data();
        }
    }
    let outcome = profile.await;
    core.await.unwrap();
    image_server.await.unwrap();
    assert!(
        cached.is_file(),
        "completed image was lost from the actual cache"
    );
    (outcome, cached)
}

/// Optional artwork budget exhaustion keeps the loaded profile and its completed image.
#[tokio::test]
async fn artwork_budget_preserves_loaded_profile_and_completed_image() {
    let (outcome, cached) = tokio::time::timeout(RESPONSE_TIMEOUT, artwork_case(Action::Budget))
        .await
        .unwrap();
    let Outcome::Complete(Ok(profile), queued) = outcome else {
        panic!("optional artwork budget discarded the loaded profile");
    };
    assert!(queued, "same-account refresh must be retained");
    let profile = profile.into_current().await.unwrap();
    assert_eq!(profile.gamertag, "Completed Profile");
    assert_eq!(profile.xuid, "123");
    assert_eq!(
        (profile.friends, profile.followers, profile.gamerscore),
        (Some(3), Some(4), Some(42))
    );
    assert_eq!(PathBuf::from(profile.gamerpic.path), cached);
    assert!(profile.featured_screenshot.path.is_empty());
}

/// Account replacement still retires a real pending image request and withholds old presentation.
#[tokio::test]
async fn account_replacement_retires_actual_pending_artwork() {
    let (outcome, _) =
        tokio::time::timeout(RESPONSE_TIMEOUT, artwork_case(Action::AccountReplacement))
            .await
            .unwrap();
    assert!(matches!(outcome, Outcome::Retired));
}

/// A late account callback cannot revive a stopped real artwork phase.
#[tokio::test]
async fn stop_retires_actual_pending_artwork_despite_late_account_wake() {
    let (outcome, _) = tokio::time::timeout(RESPONSE_TIMEOUT, artwork_case(Action::Stop))
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Stopped));
}
