use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

use super::startup::read_verified_physics_registry;
use super::*;

fn temporary_path(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rust-mcbe-{label}-{}-{nonce}.bin",
        std::process::id()
    ))
}

#[test]
fn verified_physics_registry_accepts_exact_digest() {
    let path = temporary_path("preg-valid");
    fs::write(&path, b"PREG test carrier").expect("write fixture");
    let expected = format!("{:x}", Sha256::digest(b"PREG test carrier"));

    let result = read_verified_physics_registry(
        &path,
        &format!("{expected}\n"),
        crate::asset_startup::active_content_registry_protocol(),
    );
    fs::remove_file(path).expect("remove fixture");

    assert_eq!(result.expect("valid digest"), b"PREG test carrier");
}

#[test]
fn verified_physics_registry_rejects_stale_carrier_with_guidance() {
    let path = temporary_path("preg-stale");
    fs::write(&path, b"stale PREG test carrier").expect("write fixture");

    let error = read_verified_physics_registry(
        &path,
        &"0".repeat(64),
        crate::asset_startup::active_content_registry_protocol(),
    )
    .expect_err("stale digest must fail");
    fs::remove_file(path).expect("remove fixture");
    let message = format!("{error:#}");

    assert!(message.contains("stale or corrupt"));
    assert!(message.contains("make physics-assets"));
    assert!(message.contains("make client"));
}

#[test]
fn missing_physics_registry_reports_acquisition_guidance() {
    let path = temporary_path("preg-missing");
    let error = read_verified_physics_registry(
        &path,
        &"0".repeat(64),
        crate::asset_startup::active_content_registry_protocol(),
    )
    .expect_err("missing carrier must fail");
    let message = format!("{error:#}");

    assert!(message.contains("read required protocol-2193 physics registry"));
    assert!(message.contains("make physics-assets"));
    assert!(message.contains("make client"));
}
