use super::*;
use mod_api::drawing::{DrawingPermissions, DrawingSession};

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
