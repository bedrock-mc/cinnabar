use super::*;
use std::{io::Read, net::TcpListener};

/// Models the public asset API with a content-derived digest.
fn asset(name: &str, bytes: &[u8]) -> Asset {
    Asset {
        id: 42,
        name: name.into(),
        size: bytes.len() as u64,
        digest: Some(format!("sha256:{:x}", Sha256::digest(bytes))),
        download_url: None,
    }
}

#[test]
fn selects_only_one_exact_asset_with_a_bounded_size_and_sha256() {
    let assets = [asset("other.wasm", b"other"), asset("zoom.wasm", b"wasm")];
    assert_eq!(select(&assets, "zoom.wasm", 4).unwrap().id, 42);
    assert!(select(&assets, "Zoom.wasm", 4).is_none());
    assert!(select(&assets, "zoom.wasm", 3).is_none());
    let duplicate = [asset("zoom.wasm", b"a"), asset("zoom.wasm", b"b")];
    assert!(select(&duplicate, "zoom.wasm", 4).is_none());
    for digest in [
        None,
        Some("sha256:bad".into()),
        Some(format!("md5:{}", "a".repeat(64))),
        Some(format!("sha256:{}", "z".repeat(64))),
    ] {
        let mut missing = asset("zoom.wasm", b"a");
        missing.digest = digest;
        assert!(select(&[missing], "zoom.wasm", 4).is_none());
    }
}

#[test]
fn asset_redirects_require_credential_free_https_on_known_hosts() {
    for url in [
        "https://api.github.com/a",
        "https://release-assets.githubusercontent.com/a?sig=x",
        "https://objects.githubusercontent.com/a",
    ] {
        assert!(trusted(&url.parse().unwrap()));
    }
    for url in [
        "http://github.com/a",
        "https://github.com.evil.test/a",
        "https://github.com@evil.test/a",
        "https://u:p@github.com/a",
        "https://github.com:8443/a",
        "https://127.0.0.1/a",
    ] {
        assert!(!trusted(&url.parse().unwrap()));
    }
}

/// Sends a tiny real HTTP body through the same bounded staging path as release downloads.
fn stage(bytes: &[u8], asset: &Asset, limit: usize, cancelled: bool) -> Result<Downloaded> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let bytes = bytes.to_vec();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        // Consume the whole request head before replying.
        let (mut request, mut chunk) = (Vec::new(), [0; 512]);
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0, "request ended before its headers");
            request.extend_from_slice(&chunk[..read]);
        }
        // Close-delimited framing exercises the streaming bound without Content-Length.
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
            .unwrap();
        let _ = stream.write_all(&bytes);
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(async {
        let response = reqwest::Client::new()
            .get(format!("http://{address}/asset"))
            .send()
            .await?;
        let candidate = Candidate {
            id: 1,
            source: "example/zoom".into(),
            version: "1.0.0".into(),
            asset: Some("zoom.wasm".into()),
        };
        let fence = Fence {
            generation: Arc::new(AtomicU64::new(if cancelled { 2 } else { 1 })),
            revision: 1,
            permission: Arc::new(AtomicU64::new(1)),
            authorization: 1,
            progress: Progress::default(),
        };
        receive(response, &candidate, "1.1.0", asset, fence, limit).await
    });
    server.join().unwrap();
    result
}

#[test]
fn staged_download_verifies_bytes_length_digest_and_cancellation() {
    let expected = asset("zoom.wasm", b"component");
    let result = stage(b"component", &expected, 16, false).unwrap();
    assert_eq!(std::fs::read(result.path()).unwrap(), b"component");
    let path = result.path().to_owned();
    drop(result);
    assert!(!path.exists());
    for (body, limit, cancelled) in [
        (b"wronghash".as_slice(), 16, false),
        (b"short".as_slice(), 16, false),
        (b"component!".as_slice(), 16, false),
        (b"component".as_slice(), 4, false),
        (b"component".as_slice(), 16, true),
    ] {
        assert!(stage(body, &expected, limit, cancelled).is_err());
    }
}

#[test]
fn github_json_cannot_supply_a_manifest_download_override() {
    let asset: Asset = serde_json::from_value(serde_json::json!({
        "id":42, "name":"zoom.wasm", "size":9, "digest": format!("sha256:{}", "a".repeat(64)),
        "download_url":"https://other.example/zoom.wasm"
    }))
    .unwrap();
    assert!(asset.download_url.is_none());
}
