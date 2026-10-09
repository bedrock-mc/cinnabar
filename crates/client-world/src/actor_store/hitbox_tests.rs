use std::sync::Arc;

use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue};
use world::{NbtCompound, NbtValue};

use super::{ActorStore, HITBOX_METADATA_KEY, tests::spawn};

/// One `Hitboxes` entry as `(min, max, pivot)`.
type Entry = ([f32; 3], [f32; 3], [f32; 3]);

fn hitbox_compound(entries: &[Entry]) -> ActorMetadata {
    let list = entries
        .iter()
        .map(|(min, max, pivot)| {
            let mut entry = NbtCompound::default();
            for (prefix, values) in [("Min", min), ("Max", max), ("Pivot", pivot)] {
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                    entry.insert(format!("{prefix}{axis}"), NbtValue::Float(*value));
                }
            }
            NbtValue::Compound(entry)
        })
        .collect();
    let mut root = NbtCompound::default();
    if !entries.is_empty() {
        root.insert("Hitboxes", NbtValue::List(list));
    }
    compound(root.encode_root().unwrap())
}

fn compound(bytes: Vec<u8>) -> ActorMetadata {
    ActorMetadata {
        key: HITBOX_METADATA_KEY,
        value: ActorMetadataValue::Compound(Arc::from(bytes)),
    }
}

fn store_with(yaw: f32, metadata: Vec<ActorMetadata>) -> ActorStore {
    let ActorEvent::Spawn(mut event) = spawn(8, 80) else {
        unreachable!();
    };
    event.position = [10.0, 64.0, 20.0];
    event.yaw = yaw;
    event.metadata = Arc::from(metadata);
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, ActorEvent::Spawn(event));
    store
}

fn boxes(store: &ActorStore) -> Vec<([f32; 3], [f32; 3])> {
    store.get(8).unwrap().hit_boxes().collect()
}

fn assert_boxes_near(actual: &[([f32; 3], [f32; 3])], expected: &[([f32; 3], [f32; 3])]) {
    assert_eq!(actual.len(), expected.len(), "{actual:?}");
    for ((min, max), (expected_min, expected_max)) in actual.iter().zip(expected) {
        for axis in 0..3 {
            assert!((min[axis] - expected_min[axis]).abs() < 1e-5, "{actual:?}");
            assert!((max[axis] - expected_max[axis]).abs() < 1e-5, "{actual:?}");
        }
    }
}

/// A 1×2×1 box raised 3 blocks and 1 block forward of the feet.
const RAISED: Entry = ([-0.5, 0.0, -0.5], [0.5, 2.0, 0.5], [0.0, 4.0, 1.0]);

#[test]
fn custom_hitbox_is_centred_on_its_pivot_instead_of_the_collision_box() {
    let store = store_with(0.0, vec![hitbox_compound(&[RAISED])]);
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
}

#[test]
fn hitbox_pivot_turns_with_the_actor_yaw_while_the_box_stays_axis_aligned() {
    // Yaw 90 faces west, so the forward pivot moves to -X.
    let store = store_with(90.0, vec![hitbox_compound(&[RAISED])]);
    assert_boxes_near(&boxes(&store), &[([8.5, 67.0, 19.5], [9.5, 69.0, 20.5])]);
}

#[test]
fn every_hitbox_entry_is_an_interaction_box() {
    let feet: Entry = ([-0.25, 0.0, -0.25], [0.25, 0.5, 0.25], [0.0, 0.25, 0.0]);
    let store = store_with(0.0, vec![hitbox_compound(&[feet, RAISED])]);
    assert_boxes_near(
        &boxes(&store),
        &[
            ([9.75, 64.0, 19.75], [10.25, 64.5, 20.25]),
            ([9.5, 67.0, 20.5], [10.5, 69.0, 21.5]),
        ],
    );
}

#[test]
fn empty_or_unreadable_hitbox_data_restores_the_collision_box() {
    let collision = vec![([9.7, 64.0, 19.7], [10.3, 65.8, 20.3])];
    let mut store = store_with(0.0, vec![hitbox_compound(&[RAISED])]);
    store.apply(
        1,
        2,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 8,
            metadata: Arc::from([hitbox_compound(&[])]),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
    assert_boxes_near(&boxes(&store), &collision);

    let store = store_with(0.0, vec![compound(vec![10, 0, 0xff])]);
    assert_boxes_near(&boxes(&store), &collision);
}

#[test]
fn malformed_hitbox_entries_are_skipped() {
    let inverted: Entry = ([0.5, 0.0, 0.5], [-0.5, 1.0, -0.5], [0.0; 3]);
    let infinite: Entry = ([0.0; 3], [f32::INFINITY, 1.0, 1.0], [0.0; 3]);
    let store = store_with(0.0, vec![hitbox_compound(&[inverted, infinite, RAISED])]);
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
}
