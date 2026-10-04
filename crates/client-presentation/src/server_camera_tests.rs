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

#[test]
fn overflow_drops_across_two_admit_calls_with_accounting() {
    let mut state = ServerCameraInstructions::default();
    let first: Vec<CommittedCameraEvent> = (1..=(MAX_SERVER_CAMERA_INSTRUCTIONS - 2))
        .map(|sequence| shake(sequence as u64, 0.5))
        .collect();
    state.admit(3, 0, first);
    let second: Vec<CommittedCameraEvent> = ((MAX_SERVER_CAMERA_INSTRUCTIONS - 1) as u64
        ..=(MAX_SERVER_CAMERA_INSTRUCTIONS + 6) as u64)
        .map(switch)
        .collect();
    state.admit(3, 0, second);

    assert_eq!(state.len(), MAX_SERVER_CAMERA_INSTRUCTIONS);
    assert_eq!(state.iter().next().expect("oldest").sequence, 7);
    assert_eq!(state.dropped_oldest_total(), 6);
    assert_eq!(
        state.admitted_total(),
        MAX_SERVER_CAMERA_INSTRUCTIONS as u64 + 6
    );
}

fn empty_camera_stream() -> WorldStream {
    WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn idle_drains_preserve_retained_entries_while_identity_is_stable() {
    let mut stream = empty_camera_stream();
    let mut state = ServerCameraInstructions::default();
    state.admit(3, 0, [shake(1, 0.25), switch(2)]);
    for _ in 0..16 {
        drain_committed_camera(&mut stream, 3, 0, &mut state);
    }

    assert_eq!(state.len(), 2);
    assert_eq!(state.resets(), 0);
}

#[test]
fn identity_change_clears_entries_on_the_first_zero_event_drain() {
    let mut stream = empty_camera_stream();
    let mut state = ServerCameraInstructions::default();
    state.admit(3, 0, [shake(1, 0.25), switch(2)]);

    drain_committed_camera(&mut stream, 3, 1, &mut state);
    assert!(state.is_empty());
    assert_eq!(state.resets(), 1);

    state.admit(3, 1, [shake(3, 0.75)]);
    drain_committed_camera(&mut stream, 9, 1, &mut state);
    assert!(state.is_empty());
    assert_eq!(state.resets(), 2);
    // Lifetime accounting survives the resets so overflow evidence is never lost.
    assert_eq!(state.admitted_total(), 3);
    assert_eq!(state.dropped_oldest_total(), 0);
}
