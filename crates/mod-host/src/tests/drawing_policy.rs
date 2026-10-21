use super::*;
use mod_api::drawing::{DrawingPermissions, DrawingSession};

/// Loads the same guest through either source kind to exercise shared restart bookkeeping.
fn drawing_fixture(package: bool) -> (tempfile::TempDir, ModHost) {
    if !package {
        return load_fixture("");
    }
    let directory = tempfile::tempdir().unwrap();
    let bytes = wat::parse_str(fixture("", "Hello")).unwrap();
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::fs::write(
        directory
            .path()
            .join(experience_sdk::mod_manifest::COMPONENT),
        bytes,
    )
    .unwrap();
    std::fs::write(directory.path().join(crate::package::MANIFEST), format!(
        "id = \"drawing_test\"\nversion = \"1.0.0\"\napi = \"{}\"\npermissions = []\n[files]\n\"{}\" = \"{hash}\"\n",
        experience_sdk::mod_manifest::API, experience_sdk::mod_manifest::COMPONENT,
    )).unwrap();
    let host = ModHost::load_package(directory.path(), ModGrants::default()).unwrap();
    (directory, host)
}

#[test]
fn returning_to_an_admitted_fence_still_restarts_the_revoked_guest() {
    for package in [false, true] {
        let (_directory, mut host) = drawing_fixture(package);
        let original = DrawingSession {
            connection: 1,
            local: true,
            permissions: DrawingPermissions::ALL,
            ..Default::default()
        };
        host.bind_drawing_session(original);
        assert!(host.reload_if_changed().unwrap());
        let digest = host.source_digest();
        host.bind_drawing_session(DrawingSession {
            dimension: 1,
            ..original
        });
        host.bind_drawing_session(original);
        assert!(host.is_revoked());
        assert!(host.reload_if_changed().unwrap());
        assert!(host.is_active());
        assert!(!host.needs_drawing_restart());
        assert_eq!(host.source_digest(), digest);
    }
}

#[test]
fn session_transfer_clears_output_and_rebuilds_without_a_source_edit() {
    let (_directory, mut host) = load_fixture("");
    let session = DrawingSession {
        connection: 1,
        local: true,
        permissions: DrawingPermissions::ALL,
        ..Default::default()
    };
    host.bind_drawing_session(session);
    assert!(host.needs_drawing_restart());
    assert!(host.reload_if_changed().unwrap());
    assert!(host.is_active());
    host.bind_drawing_session(DrawingSession {
        connection: 2,
        ..session
    });
    assert!(host.is_revoked());
    assert!(host.label().is_none());
    assert!(host.reload_if_changed().unwrap());
    assert!(host.is_active());
    host.bind_drawing_session(DrawingSession {
        connection: 2,
        ..session
    });
    assert!(host.is_active());
    host.bind_drawing_session(DrawingSession {
        dimension: 1,
        connection: 2,
        ..session
    });
    assert!(host.is_revoked());
    assert!(host.label().is_none());
}

#[test]
fn session_end_and_grant_change_stop_a_guest_with_disclosed_memory() {
    let (_directory, mut host) = load_fixture("");
    let session = DrawingSession {
        connection: 1,
        permissions: DrawingPermissions::ALL,
        ..Default::default()
    };
    host.bind_drawing_session(session);
    host.bind_drawing_session(DrawingSession {
        permissions: DrawingPermissions::default(),
        ..session
    });
    assert!(host.is_revoked());
    assert!(host.reload_if_changed().unwrap());
    host.bind_drawing_session(DrawingSession::default());
    assert!(host.is_revoked());
    assert!(host.render().0.passes.is_empty());
}

#[test]
fn traps_are_not_recovered_by_an_authority_transition() {
    let (_directory, mut host) = load_fixture("unreachable");
    host.frame(false).unwrap_err();
    host.bind_drawing_session(DrawingSession::default());
    assert!(!host.is_revoked());
    assert!(!host.reload_if_changed().unwrap());
}

#[test]
fn registration_revocation_survives_later_drawing_fences() {
    for pending_restart in [false, true] {
        let (_directory, mut host) = load_fixture("");
        if pending_restart {
            host.bind_drawing_session(DrawingSession::default());
        }
        host.revoke();
        host.bind_drawing_session(DrawingSession {
            connection: 2,
            local: true,
            permissions: DrawingPermissions::ALL,
            ..Default::default()
        });
        assert!(host.is_revoked());
        assert!(!host.needs_drawing_restart());
        assert!(!host.reload_if_changed().unwrap());
        host.frame(false).unwrap();
        assert!(host.label().is_none());
    }
}

#[test]
fn failed_drawing_restart_is_attempted_once_per_source_and_fence() {
    let (directory, mut host) = load_fixture("");
    let accepted = host.source_digest();
    host.bind_drawing_session(DrawingSession::default());
    std::fs::write(directory.path().join("mod.wat"), "broken").unwrap();
    assert!(host.reload_if_changed().is_err());
    assert_eq!(host.source_digest(), accepted);
    assert!(!host.reload_if_changed().unwrap());
    host.bind_drawing_session(DrawingSession {
        connection: 2,
        ..Default::default()
    });
    assert!(host.reload_if_changed().is_err());
    assert!(!host.reload_if_changed().unwrap());
    std::fs::write(directory.path().join("mod.wat"), fixture("", "Fresh")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.label(), Some("Fresh"));
}

/// Emits the same executable with a small, explicit drawing request.
fn requested_component(request: bool) -> Vec<u8> {
    let mut bytes = wat::parse_str(fixture("", "Hello")).unwrap();
    let name = crate::REQUESTED_GRANTS_SECTION;
    let mut payload = vec![name.len() as u8];
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(if request {
        b"{\"world_draw\":true}"
    } else {
        b"{}"
    });
    assert!(payload.len() < 128);
    bytes.extend_from_slice(&[0, payload.len() as u8]);
    bytes.extend(payload);
    bytes
}

#[test]
fn restored_component_requests_use_the_original_loader_consent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drawing.wasm");
    for initially_requested in [false, true] {
        for consent in [false, true] {
            std::fs::write(&path, requested_component(initially_requested)).unwrap();
            let mut host = ModHost::load_with_grants(
                &path,
                ModGrants {
                    world_draw: consent,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(host.grants().world_draw, consent && initially_requested);
            for request in [
                !initially_requested,
                initially_requested,
                !initially_requested,
            ] {
                std::fs::write(&path, requested_component(request)).unwrap();
                assert!(host.reload_if_changed().unwrap());
                assert_eq!(host.grants().world_draw, consent && request);
            }
        }
    }
}

#[test]
fn quarantine_cancels_a_pending_drawing_restart() {
    for failed_source in [false, true] {
        let (directory, mut host) = load_fixture("");
        if failed_source {
            std::fs::write(directory.path().join("mod.wat"), "broken").unwrap();
            assert!(host.reload_if_changed().is_err());
        }
        host.bind_drawing_session(DrawingSession::default());
        assert!(host.needs_drawing_restart());
        host.quarantine();
        host.bind_drawing_session(DrawingSession {
            connection: 2,
            ..Default::default()
        });
        assert!(!host.needs_drawing_restart());
        assert!(!host.reload_if_changed().unwrap());
        assert!(!host.is_active());
    }
}
