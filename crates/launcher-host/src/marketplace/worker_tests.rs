//! Exact, synthetic website/API handoff replay through the native public HTTP installer.

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{io::Read, net::TcpListener};

#[derive(Deserialize)]
struct Handoff {
    public_key: String,
    xuid: String,
    selected: CinnabarModEntry,
    exact_path: String,
    release_response: String,
    artifact: String,
    revocations: SignedRevocations,
    withdrawn: SignedRevocations,
    checked_at: u64,
}

/// Replays original response bytes, checks exact paths, and refuses credential-bearing requests.
fn serve(
    responses: Vec<(String, Vec<u8>, Option<String>)>,
) -> (Transport, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for (path, body, signature) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut buffer = [0; 512];
                let size = stream.read(&mut buffer).unwrap();
                assert_ne!(size, 0);
                request.extend_from_slice(&buffer[..size]);
                assert!(request.len() <= 8192);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
            let headers = request.to_ascii_lowercase();
            assert!(!headers.contains("authorization:") && !headers.contains("cookie:"));
            let signature = signature
                .map(|value| format!("X-Cinnabar-Signature: {value}\r\n"))
                .unwrap_or_default();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\n{signature}Content-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    let transport = Transport {
        origin: format!("http://{address}").parse().unwrap(),
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    };
    (transport, server)
}

#[test]
fn website_send_downloads_disabled_and_signed_withdrawal_holds_the_installed_component() {
    let fixture: Handoff =
        serde_json::from_str(include_str!("fixtures/website-send.json")).unwrap();
    let keys = Keys::parse(
        &serde_json::to_vec(&serde_json::json!({"ed25519":[fixture.public_key]})).unwrap(),
    )
    .unwrap();
    let shipped: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../data/marketplace-keys.json")).unwrap();
    assert!(
        !shipped["ed25519"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(fixture.public_key)),
        "throwaway test signer must never be a shipped trust root"
    );
    let bytes = STANDARD.decode(&fixture.artifact).unwrap();
    let (transport, server) = serve(vec![
        (
            "/v1/mods/revocations".into(),
            fixture.revocations.body.as_bytes().to_vec(),
            Some(fixture.revocations.signature.clone()),
        ),
        (
            fixture.exact_path,
            fixture.release_response.into_bytes(),
            None,
        ),
        (
            format!("/v1/mods/assets/{}", fixture.selected.sha256),
            bytes.clone(),
            None,
        ),
        (
            "/v1/mods/revocations".into(),
            fixture.withdrawn.body.as_bytes().to_vec(),
            Some(fixture.withdrawn.signature.clone()),
        ),
    ]);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        refresh(&transport, root.path(), &keys, &guard, || {
            fixture.checked_at
        })
        .await
        .unwrap();
        let prepared = download(
            &transport,
            root.path(),
            &keys,
            &fixture.xuid,
            &fixture.selected,
            bytes.len(),
            (Arc::new(AtomicU64::new(1)), 1, guard.clone()),
            || fixture.checked_at,
        )
        .await
        .unwrap();
        assert_eq!(prepared.bytes().unwrap(), bytes);
        let installed = prepared.install(fixture.checked_at).unwrap();
        assert!(
            !root.path().join("local-mod.json").exists(),
            "download cannot enable the component or approve permissions"
        );
        assert!(guard.allows(&installed, fixture.checked_at));
        refresh(&transport, root.path(), &keys, &guard, || {
            fixture.checked_at
        })
        .await
        .unwrap();
        assert!(!guard.allows(&installed, fixture.checked_at));
        assert!(
            guard
                .reason(&installed, fixture.checked_at)
                .contains("Synthetic integration withdrawal")
        );
    });
    server.join().unwrap();
}

#[test]
fn account_switch_retries_cancelled_delivery_but_keeps_durable_consumed_sends() {
    let fixture: Handoff =
        serde_json::from_str(include_str!("fixtures/website-send.json")).unwrap();
    let keys = Keys::parse(
        &serde_json::to_vec(&serde_json::json!({"ed25519":[fixture.public_key]})).unwrap(),
    )
    .unwrap();
    let bytes = STANDARD.decode(&fixture.artifact).unwrap();
    let mut responses = vec![(
        "/v1/mods/revocations".into(),
        fixture.revocations.body.as_bytes().to_vec(),
        Some(fixture.revocations.signature.clone()),
    )];
    for _ in 0..2 {
        responses.push((
            fixture.exact_path.clone(),
            fixture.release_response.as_bytes().to_vec(),
            None,
        ));
        responses.push((
            format!("/v1/mods/assets/{}", fixture.selected.sha256),
            bytes.clone(),
            None,
        ));
    }
    let (transport, server) = serve(responses);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    let generation = Arc::new(AtomicU64::new(1));
    let (outgoing, incoming) = crossbeam_channel::bounded(1);
    let key = request(&fixture.xuid, &fixture.selected);
    let mut deliveries = Deliveries::default();
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        refresh(&transport, root.path(), &keys, &guard, || {
            fixture.checked_at
        })
        .await
        .unwrap();
        for epoch in [1, 3] {
            generation.store(epoch, Ordering::SeqCst);
            guard.configure(epoch);
            guard.account(epoch, fixture.xuid.clone());
            deliveries.refresh(
                epoch,
                root.path(),
                Some(&keys),
                bytes.len(),
                &guard,
                fixture.checked_at,
            );
            assert!(!deliveries.requests.contains(&key));
            let prepared = download(
                &transport,
                root.path(),
                &keys,
                &fixture.xuid,
                &fixture.selected,
                bytes.len(),
                (generation.clone(), epoch, guard.clone()),
                || fixture.checked_at,
            )
            .await
            .unwrap();
            deliveries.queue(key.clone(), prepared, &outgoing);
            deliveries.refresh(
                epoch,
                root.path(),
                Some(&keys),
                bytes.len(),
                &guard,
                fixture.checked_at,
            );
            assert!(
                deliveries.requests.contains(&key),
                "pending delivery must remain deduplicated in the same epoch"
            );
            let prepared = incoming.recv().unwrap();
            if epoch == 1 {
                generation.store(2, Ordering::SeqCst);
                guard.configure(2);
                assert!(prepared.install(fixture.checked_at).is_err());
                let name = storage::filename(&fixture.xuid, &fixture.selected).unwrap();
                assert!(!root.path().join(name).exists());
            } else {
                let installed = prepared.install(fixture.checked_at).unwrap();
                generation.store(4, Ordering::SeqCst);
                guard.configure(4);
                guard.account(4, fixture.xuid.clone());
                deliveries.refresh(
                    4,
                    root.path(),
                    Some(&keys),
                    bytes.len(),
                    &guard,
                    fixture.checked_at,
                );
                assert!(
                    deliveries.requests.contains(&key),
                    "successful install must stay consumed after switching accounts"
                );
                assert!(guard.allows(&installed, fixture.checked_at));
                fs::remove_file(installed).unwrap();
                generation.store(5, Ordering::SeqCst);
                guard.configure(5);
                guard.account(5, fixture.xuid.clone());
                deliveries.refresh(
                    5,
                    root.path(),
                    Some(&keys),
                    bytes.len(),
                    &guard,
                    fixture.checked_at,
                );
                assert!(
                    deliveries.requests.contains(&key),
                    "local removal must stay consumed after switching accounts"
                );
                assert!(storage::removed(
                    root.path(),
                    &fixture.xuid,
                    &fixture.selected,
                    &keys,
                    bytes.len(),
                    fixture.checked_at
                ));
            }
        }
    });
    server.join().unwrap();
}
