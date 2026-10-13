use std::sync::Arc;

use protocol::{ActorEvent, ActorMetadata, ActorMetadataValue};

use super::{ActorStore, PLAYER_FLAGS_METADATA_KEY, PLAYER_FLAGS_SLEEPING, tests::spawn};

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
        key: PLAYER_FLAGS_METADATA_KEY,
        value: ActorMetadataValue::Byte(PLAYER_FLAGS_SLEEPING as i8),
    }
}

/// The streamed bed position.
fn bed(block: [i32; 3]) -> ActorMetadata {
    ActorMetadata {
        key: super::placement::KEY_BED_POSITION,
        value: ActorMetadataValue::BlockPosition(block),
    }
}

#[test]
fn sleepers_lie_only_in_their_streamed_bed() {
    let mut store = sleeper(vec![sleeping(), bed([4, 64, -3])]);
    assert_eq!(store.bed_sample_points(), [(8, [4, 64, -3])]);
    store.set_bed_rotations(&[(8, 270.0)]);
    assert_eq!(store.get(8).unwrap().sleeping_bed(), Some(([4, 64, -3], 270.0)));

    let unplaced = sleeper(vec![sleeping()]);
    assert!(
        unplaced.bed_sample_points().is_empty(),
        "without a bed position there is no bed to read, as natively"
    );
    assert_eq!(unplaced.get(8).unwrap().sleeping_bed(), None);

    let awake = sleeper(vec![bed([4, 64, -3])]);
    assert_eq!(awake.get(8).unwrap().sleeping_bed(), None);
}
