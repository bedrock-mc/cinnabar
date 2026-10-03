use super::*;
use protocol::AbilityLayersEvidence;

fn update(owner: i64, count: u32) -> AbilitiesUpdate {
    AbilitiesUpdate {
        actor_unique_id: owner,
        player_permission: -1,
        command_permission: 255,
        layers: if count == 0 {
            AbilityLayersEvidence::Received([].into())
        } else {
            AbilityLayersEvidence::Unavailable {
                declared_layers: count,
            }
        },
    }
}

#[test]
fn unknown_received_empty_and_unavailable_preserve_their_distinct_origins() {
    let mut runtime = UiRuntime::new(7);
    assert!(runtime.local_abilities().is_none());
    runtime.bind_local_abilities(7, 4, 0, true);
    assert!(
        runtime.local_abilities().is_none(),
        "binding is not a packet receipt"
    );
    runtime.apply_local_abilities(7, 4, 1, update(0, 0));
    assert_eq!(runtime.local_abilities(), Some(&update(0, 0)));
    runtime.apply_local_abilities(7, 4, 2, update(0, 33));
    assert_eq!(runtime.local_abilities(), Some(&update(0, 33)));
}

#[test]
fn wrong_identity_session_stream_and_duplicate_fifo_cannot_retire_latest_evidence() {
    let mut runtime = UiRuntime::new(7);
    runtime.bind_local_abilities(7, 4, 17, true);
    runtime.apply_local_abilities(7, 4, 5, update(17, 0));
    for (session, stream, sequence, owner) in [
        (6, 4, 6, 17),
        (7, 3, 6, 17),
        (7, 4, 6, 0),
        (7, 4, 5, 17),
        (7, 4, 4, 17),
    ] {
        runtime.apply_local_abilities(session, stream, sequence, update(owner, 33));
        assert_eq!(runtime.local_abilities(), Some(&update(17, 0)));
    }
}

#[test]
fn retired_and_failed_repeat_bindings_cannot_be_resurrected_by_drain_or_clone() {
    let mut runtime = UiRuntime::new(7);
    runtime.bind_local_abilities(7, 4, 17, true);
    runtime.apply_local_abilities(7, 4, 1, update(17, 0));
    runtime.begin_session(7);
    assert!(
        runtime.local_abilities().is_some(),
        "same-session begin is a no-op"
    );
    runtime.clear_local_abilities();
    runtime.bind_local_abilities(7, 4, 17, false);
    runtime.synchronize_local_abilities(7, Some(4));
    runtime.apply_local_abilities(7, 4, 2, update(17, 0));
    assert!(runtime.local_abilities().is_none());
    let mut clone = runtime.clone();
    clone.apply_local_abilities(7, 4, 3, update(17, 0));
    assert!(clone.local_abilities().is_none());
    runtime.bind_local_abilities(7, 5, 17, true);
    runtime.apply_local_abilities(7, 5, 1, update(17, 0));
    runtime.synchronize_local_abilities(7, Some(6));
    assert!(runtime.local_abilities().is_none());
    runtime.bind_local_abilities(7, 6, 17, true);
    runtime.apply_local_abilities(7, 6, 1, update(17, 0));
    runtime.begin_session(8);
    assert!(runtime.local_abilities().is_none());
}
