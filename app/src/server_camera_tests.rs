use super::*;

fn shake(sequence: u64, intensity: f32) -> CommittedCameraEvent {
    CommittedCameraEvent {
        sequence,
        event: protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
            intensity,
            duration_seconds: 1.0,
            shake_type: protocol::CameraShakeType::Positional,
            action: protocol::CameraShakeAction::Add,
        }),
    }
}

fn switch(sequence: u64) -> CommittedCameraEvent {
    CommittedCameraEvent {
        sequence,
        event: protocol::CameraEvent::Switch(protocol::CameraSwitchEvent {
            camera_unique_id: -1,
            target_player_unique_id: -2,
        }),
    }
}

#[test]
fn admission_preserves_fifo_order_across_interleaved_families() {
    let mut state = ServerCameraInstructions::default();
    state.admit(7, 0, [shake(1, 0.25), switch(2)]);
    state.admit(7, 0, [shake(3, 0.75)]);

    let retained: Vec<u64> = state.iter().map(|entry| entry.sequence).collect();
    assert_eq!(retained, vec![1, 2, 3]);
    assert_eq!(state.admitted_total(), 3);
    assert_eq!(state.dropped_oldest_total(), 0);
}

#[test]
fn overflow_drops_the_oldest_entry_with_accounting() {
    let mut state = ServerCameraInstructions::default();
    let events: Vec<CommittedCameraEvent> = (1..=(MAX_SERVER_CAMERA_INSTRUCTIONS as u64 + 4))
        .map(|sequence| shake(sequence, 0.5))
        .collect();
    state.admit(3, 0, events);

    assert_eq!(state.len(), MAX_SERVER_CAMERA_INSTRUCTIONS);
    assert_eq!(state.iter().next().expect("oldest").sequence, 5);
    assert_eq!(
        state.iter().last().expect("newest").sequence,
        MAX_SERVER_CAMERA_INSTRUCTIONS as u64 + 4
    );
    assert_eq!(state.dropped_oldest_total(), 4);
    assert_eq!(
        state.admitted_total(),
        MAX_SERVER_CAMERA_INSTRUCTIONS as u64 + 4
    );
}

#[test]
fn session_replacement_clears_retained_instructions() {
    let mut state = ServerCameraInstructions::default();
    state.admit(3, 0, [shake(1, 0.25)]);
    state.admit(4, 0, [switch(2)]);

    assert_eq!(state.len(), 1);
    assert_eq!(state.iter().next().expect("post-reset entry").sequence, 2);
    assert!(state.iter().all(|entry| entry.sequence != 1));
    assert_eq!(state.resets(), 1);
    // Lifetime accounting survives the reset so overflow evidence is never lost.
    assert_eq!(state.admitted_total(), 2);
    assert_eq!(state.dropped_oldest_total(), 0);
}

#[test]
fn dimension_change_clears_retained_instructions() {
    let mut state = ServerCameraInstructions::default();
    state.admit(3, 0, [shake(1, 0.25), switch(2)]);
    state.admit(3, 1, [shake(3, 0.75)]);

    assert_eq!(state.len(), 1);
    assert_eq!(state.iter().next().expect("post-reset entry").sequence, 3);
    assert_eq!(state.resets(), 1);
}

#[test]
fn unchanged_identity_keeps_history_and_counts_no_reset() {
    let mut state = ServerCameraInstructions::default();
    state.admit(3, 0, [shake(1, 0.25)]);
    state.admit(3, 0, [switch(2)]);

    assert_eq!(state.len(), 2);
    assert_eq!(state.resets(), 0);
}
