use protocol_ids::{ActorDataKey, ActorFlag, PlayerFlag};
use std::sync::Arc;

use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, ActorRemoveEvent,
};

use super::{ActorStore, tests::spawn};

/// A store holding player 8 with `metadata`.
fn sleeper(metadata: Vec<ActorMetadata>) -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = spawn(8, 80) else {
        unreachable!();
    };
    event.metadata = Arc::from(metadata);
    store.apply(1, 1, ActorEvent::Spawn(event));
    store
}

/// The sleeping player flag.
fn sleeping() -> ActorMetadata {
    ActorMetadata {
        key: ActorDataKey::PlayerFlags.wire_value(),
        value: ActorMetadataValue::Byte(PlayerFlag::Sleeping.wire_value() as i8),
    }
}

/// The streamed bed position.
fn bed(block: [i32; 3]) -> ActorMetadata {
    ActorMetadata {
        key: ActorDataKey::BedPosition.wire_value(),
        value: ActorMetadataValue::BlockPosition(block),
    }
}

#[test]
fn sleepers_lie_only_in_their_streamed_bed() {
    let mut store = sleeper(vec![sleeping(), bed([4, 64, -3])]);
    assert_eq!(store.bed_sample_points(), [(8, [4, 64, -3])]);
    store.set_bed_rotations(&[(8, 270.0)]);
    assert_eq!(
        store.get(8).unwrap().sleeping_bed(),
        Some(([4, 64, -3], 270.0))
    );

    let unplaced = sleeper(vec![sleeping()]);
    assert!(
        unplaced.bed_sample_points().is_empty(),
        "without a bed position there is no bed to read, as natively"
    );
    assert_eq!(unplaced.get(8).unwrap().sleeping_bed(), None);

    let awake = sleeper(vec![bed([4, 64, -3])]);
    assert_eq!(awake.get(8).unwrap().sleeping_bed(), None);
}

/// Delivers a partial metadata update through the ordinary actor ingress path.
fn update(store: &mut ActorStore, sequence: u64, runtime_id: u64, metadata: Vec<ActorMetadata>) {
    store.apply(
        1,
        sequence,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id,
            metadata: metadata.into(),
            properties: Arc::from([]),
            tick: sequence,
        }),
    );
}

/// Writes the extended actor sleep flag independently of the player sleep flag.
fn actor_sleeping(on: bool) -> ActorMetadata {
    ActorMetadata {
        key: ActorDataKey::FlagsTwo.wire_value(),
        value: ActorMetadataValue::FlagsExtended(if on {
            1 << (ActorFlag::Sleeping.wire_value() - 64)
        } else {
            0
        }),
    }
}

/// Clears the player-specific sleep flag.
fn awake() -> ActorMetadata {
    ActorMetadata {
        key: ActorDataKey::PlayerFlags.wire_value(),
        value: ActorMetadataValue::Byte(0),
    }
}

#[test]
fn membership_tracks_partial_metadata_and_clears_departed_or_changed_beds() {
    let mut store = sleeper(vec![sleeping()]);
    assert!(store.bed_sample_points().is_empty());
    update(&mut store, 2, 8, vec![bed([4, 64, -3])]);
    store.set_bed_rotations(&[(8, 270.0)]);
    update(&mut store, 3, 8, vec![sleeping()]);
    assert_eq!(store.get(8).unwrap().status.sleep_rotation, Some(270.0));
    update(&mut store, 4, 8, vec![bed([5, 64, -3])]);
    assert_eq!(store.bed_sample_points(), [(8, [5, 64, -3])]);
    assert_eq!(
        store.get(8).unwrap().sleeping_bed(),
        Some(([5, 64, -3], 0.0))
    );
    store.set_bed_rotations(&[(8, 180.0)]);
    update(&mut store, 5, 8, vec![awake()]);
    assert!(store.bed_sample_points().is_empty());
    assert_eq!(store.get(8).unwrap().status.sleep_rotation, None);
    store.set_bed_rotations(&[(8, 90.0)]);
    assert_eq!(store.get(8).unwrap().status.sleep_rotation, None);
    update(&mut store, 6, 8, vec![actor_sleeping(true)]);
    assert_eq!(store.bed_sample_points(), [(8, [5, 64, -3])]);
    store.set_bed_rotations(&[(8, 90.0)]);
    update(&mut store, 7, 8, vec![sleeping(), actor_sleeping(false)]);
    assert_eq!(store.get(8).unwrap().status.sleep_rotation, Some(90.0));
    update(
        &mut store,
        8,
        8,
        vec![ActorMetadata {
            key: ActorDataKey::BedPosition.wire_value(),
            value: ActorMetadataValue::Int(0),
        }],
    );
    assert!(store.bed_sample_points().is_empty());
    assert_eq!(store.get(8).unwrap().status.sleep_rotation, None);
    update(&mut store, 9, 8, vec![bed([6, 64, -3])]);
    assert_eq!(store.bed_sample_points(), [(8, [6, 64, -3])]);
}

#[test]
fn missing_samples_clear_old_headings_and_recovered_samples_replace_them() {
    let mut store = sleeper(vec![sleeping(), bed([4, 64, -3])]);
    for samples in [&[(8, 270.0)][..], &[], &[(8, 90.0)][..]] {
        store.set_bed_rotations(samples);
        assert_eq!(
            store.get(8).unwrap().status.sleep_rotation,
            samples.first().map(|sample| sample.1)
        );
        assert_eq!(store.bed_sample_points(), [(8, [4, 64, -3])]);
    }
}

#[test]
fn sleeper_membership_expires_with_actor_lifetimes_dimensions_and_sessions() {
    let mut store = sleeper(vec![sleeping(), bed([4, 64, -3])]);
    store.apply(1, 2, spawn(8, 81));
    assert!(store.bed_sample_points().is_empty());
    update(&mut store, 3, 8, vec![sleeping(), bed([4, 64, -3])]);
    store.apply(1, 4, spawn(9, 81));
    assert!(store.bed_sample_points().is_empty());
    update(&mut store, 5, 9, vec![sleeping(), bed([4, 64, -3])]);
    store.apply(
        1,
        6,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 81,
        }),
    );
    assert!(store.bed_sample_points().is_empty());
    store.apply(1, 7, spawn(8, 80));
    update(&mut store, 8, 8, vec![sleeping(), bed([4, 64, -3])]);
    store.reset_dimension(1, 9, 1);
    assert!(store.bed_sample_points().is_empty());
    store.begin_session(2, 0);
    assert!(store.bed_sample_points().is_empty());
    let mut store = sleeper(vec![sleeping(), bed([4, 64, -3])]);
    store.begin_session(2, 0);
    assert!(store.bed_sample_points().is_empty());
}

#[test]
fn sampling_tracks_only_sleepers_in_a_populated_world() {
    let mut store = ActorStore::new(1, 0);
    for runtime in 1..=512 {
        store.apply(1, runtime, spawn(runtime, runtime as i64));
    }
    assert!(store.bed_sample_points().is_empty());
    for (index, runtime) in [500, 4, 257].into_iter().enumerate() {
        update(
            &mut store,
            513 + index as u64,
            runtime,
            vec![sleeping(), bed([runtime as i32, 64, 0])],
        );
        assert_eq!(store.bed_sample_points().len(), index + 1);
    }
    assert_eq!(
        store.bed_sample_points(),
        [(4, [4, 64, 0]), (257, [257, 64, 0]), (500, [500, 64, 0])]
    );
    store.set_bed_rotations(&[(4, 90.0), (257, 180.0), (500, 270.0)]);
    for runtime in 1..=512 {
        let expected = match runtime {
            4 => Some(90.0),
            257 => Some(180.0),
            500 => Some(270.0),
            _ => None,
        };
        assert_eq!(store.get(runtime).unwrap().status.sleep_rotation, expected);
    }
}
