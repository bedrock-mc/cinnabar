//! Real component crossings for opt-in filtering, grants and isolated execution.
use super::*;

/// Loads a typed return-value fixture with a configurable filter body and mask length.
fn host(body: &str, count: &str, controls: bool) -> (tempfile::TempDir, ModHost) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filter.wat");
    std::fs::write(
        &path,
        include_str!("action_filter.wat")
            .replace("$BODY", body)
            .replace("$COUNT", count),
    )
    .unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls,
            ..Default::default()
        },
    )
    .unwrap();
    (dir, host)
}
/// One resolved source-backed press.
fn frame() -> ActionFrame {
    ActionFrame {
        epoch: 1,
        time_us: 42,
        physical: ActionHeld {
            attack: false,
            item_use: false,
        },
        logical: ActionHeld {
            attack: false,
            item_use: false,
        },
        transitions: vec![ActionTransition {
            action: FilteredAction::Attack,
            kind: TransitionKind::Press,
            held_after: true,
        }],
    }
}
#[test]
fn filter_returns_a_verdict_even_when_ordinary_frame_fuel_is_empty() {
    let (_dir, mut host) = host("", "local.get $count", true);
    host.set_frame_authority(CallbackBudget::new(0), false, false, 0, Default::default());
    let verdict = host.filter_actions(frame()).unwrap();
    assert_eq!(verdict.accepted, [true]);
    assert!(host.is_active());
}
#[test]
fn only_explicit_granted_exports_claim_filter_ownership() {
    let (_dir, mut denied) = host("", "local.get $count", false);
    assert!(denied.action_filter_identity().is_none());
    assert!(denied.filter_actions(frame()).is_err());
    assert!(denied.is_active());
    let (_dir, ordinary) = load_fixture("");
    assert!(ordinary.action_filter_identity().is_none());
    let (_dir, enabled) = host("", "local.get $count", true);
    let (_dir, other) = host("", "local.get $count", true);
    assert_ne!(
        enabled.action_filter_identity(),
        other.action_filter_identity()
    );
}
#[test]
fn malformed_verdict_traps_and_import_calls_quarantine_the_owner() {
    for (body, count) in [
        ("", "i32.const 0"),
        ("unreachable", "local.get $count"),
        ("call $pressed drop", "local.get $count"),
        ("(loop $forever br $forever)", "local.get $count"),
    ] {
        let (_dir, mut host) = host(body, count, true);
        assert!(host.filter_actions(frame()).is_err());
        assert!(!host.is_active());
        assert!(host.action_filter_identity().is_none());
    }
}
#[test]
fn reloaded_filter_gets_a_new_identity_and_wrong_signatures_are_rejected() {
    let (dir, mut host) = host("", "local.get $count", true);
    let old = host.action_filter_identity();
    let path = dir.path().join("filter.wat");
    let source = include_str!("action_filter.wat")
        .replace("$BODY", "nop")
        .replace("$COUNT", "local.get $count");
    std::fs::write(&path, &source).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_ne!(old, host.action_filter_identity());
    std::fs::write(
        &path,
        source.replace("(param \"frame\" $frame)", "(param \"frame\" u32)"),
    )
    .unwrap();
    assert!(host.reload_if_changed().is_err());
    assert!(host.is_active());
}
