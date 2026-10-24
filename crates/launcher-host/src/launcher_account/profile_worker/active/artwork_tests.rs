use super::*;
use crate::catalog_art::ARTWORK_BUDGET;
use crate::remote_images::{LAUNCHER_ART, Surface};
use serde_json::json;
use std::{
    io,
    path::PathBuf,
    task::{Context, Waker},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
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

/// Controls the original artwork phase over acquired presentation, without an account service fixture.
async fn artwork_case(action: Action) -> (Result<(Profile, bool), Exit>, PathBuf) {
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
    let loaded: Profile = serde_json::from_value(json!({
        "gamertag":"Completed Profile","xuid":"123","friends":3,"followers":4,
        "gamerscore":42,"gamerpic":{"url":fast_url},
        "featured_screenshot":{"url":stall_url}
    }))
    .unwrap();
    let (sender, mut requests) = channel();
    requests.borrow_and_update();
    let shared = Mutex::new(Snapshot {
        profile_wake: Some(sender.clone()),
        ..Default::default()
    });
    let selection = Selection::capture(&shared);
    let mut profile = Box::pin(async {
        let mut loaded = loaded;
        let mut queued = false;
        wait(
            artwork::fill(&images, artwork::profile_slots(&mut loaded)),
            &shared,
            &selection,
            &mut requests,
            None,
            &mut queued,
        )
        .await?;
        Ok((loaded, queued))
    });
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
    let Ok((profile, queued)) = outcome else {
        panic!("optional artwork budget discarded the loaded profile");
    };
    assert!(queued, "same-account refresh must be retained");
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
    assert!(matches!(outcome, Err(Exit::Retired)));
}

/// A late account callback cannot revive a stopped real artwork phase.
#[tokio::test]
async fn stop_retires_actual_pending_artwork_despite_late_account_wake() {
    let (outcome, _) = tokio::time::timeout(RESPONSE_TIMEOUT, artwork_case(Action::Stop))
        .await
        .unwrap();
    assert!(matches!(outcome, Err(Exit::Stopped)));
}
