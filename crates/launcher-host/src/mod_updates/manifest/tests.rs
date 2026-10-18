use super::*;

/// Builds a versioned manifest whose payload remains on its publisher's origin.
fn document() -> serde_json::Value {
    serde_json::json!({
        "format": FORMAT, "version": "1.2.3", "size": 9,
        "url": "https://mods.example/releases/1.2.3/zoom.wasm",
        "sha256": "a".repeat(64),
    })
}

#[test]
fn https_manifest_selects_a_versioned_payload_without_a_github_repository() {
    let source = "https://mods.example/zoom/update.json".parse().unwrap();
    let release = parse(&serde_json::to_vec(&document()).unwrap(), &source).unwrap();
    assert_eq!(release.tag_name, "1.2.3");
    assert_eq!(release.manifest_url.as_deref(), Some(source.as_str()));
    let asset = super::super::download::select(&release.assets, source.as_str(), 9).unwrap();
    assert_eq!(asset.download_url.as_deref(), document()["url"].as_str());
    assert!(super::super::download::select(&release.assets, source.as_str(), 8).is_none());
}

#[test]
fn invalid_metadata_and_unrelated_payload_origins_cannot_select_downloads() {
    let source = "https://mods.example/zoom/update.json".parse().unwrap();
    for (field, invalid) in [
        ("url", serde_json::json!("http://mods.example/zoom.wasm")),
        ("url", serde_json::json!("https://other.example/zoom.wasm")),
        (
            "url",
            serde_json::json!("https://mods.example:8443/zoom.wasm"),
        ),
        (
            "url",
            serde_json::json!("https://user:password@mods.example/zoom.wasm"),
        ),
        ("url", serde_json::json!("https://mods.example/zoom.zip")),
        ("format", serde_json::json!("unknown")),
        ("version", serde_json::json!("nightly")),
        ("size", serde_json::json!(0)),
        ("size", serde_json::json!(-1)),
        ("sha256", serde_json::json!("bad")),
        ("sha256", serde_json::json!("z".repeat(64))),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut value = document();
        value[field] = invalid;
        assert!(
            parse(&serde_json::to_vec(&value).unwrap(), &source).is_err(),
            "{field}: {value}"
        );
    }
    assert!(parse(&vec![b' '; MAX_MANIFEST_BYTES + 1], &source).is_err());
    let duplicate = serde_json::to_string(&document()).unwrap().replacen(
        "\"version\":\"1.2.3\"",
        "\"version\":\"1.2.3\",\"version\":\"2.0.0\"",
        1,
    );
    assert!(parse(duplicate.as_bytes(), &source).is_err());
}
