use protocol_ids::ActorDataKey;
use std::sync::Arc;

use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue};
use world::{NbtCompound, NbtValue};

use super::{ActorApplyResult, ActorStore, tests::spawn};

mod budget;

#[test]
fn current_and_predicted_custom_boxes_keep_the_exact_network_origin() {
    let entry = ([-0.01, 0.0, -0.01], [0.01, 0.02, 0.01], [0.0; 3]);
    let mut store = store_with(0.0, vec![hitbox_compound(&[entry])]);
    store.actors.get_mut(&8).unwrap().kind = protocol::ActorKind::Player {
        uuid: [8; 16],
        username: "Remote".into(),
    };
    let height = 0.000_001_f32;
    store.apply(1, 2, super::tests::network_move(8, height, true));
    let bounds = boxes(&store)[0];
    assert_eq!(bounds.0[1].to_bits(), (height - 0.01).to_bits());
    assert_eq!(bounds.1[1].to_bits(), (height + 0.01).to_bits());

    let target = 0.000_002_f32;
    store.apply(1, 3, super::tests::network_move(8, target, false));
    store.predict_remote_motion(1);
    let pose = store.pick_pose(8).unwrap();
    let predicted: Vec<_> = store.get(8).unwrap().hit_boxes_at_pose(pose).collect();
    let expected = height + (target - height) * (1.0 / 3.0);
    assert_eq!(predicted[0].0[1].to_bits(), (expected - 0.01).to_bits());
    assert_eq!(predicted[0].1[1].to_bits(), (expected + 0.01).to_bits());
    store.advance_interpolation_frame(1);
    assert_eq!(boxes(&store), predicted);
}

/// One `Hitboxes` entry as `(min, max, pivot)`.
type Entry = ([f32; 3], [f32; 3], [f32; 3]);

/// Encodes ordered hitboxes as actor metadata.
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
    root.insert("Hitboxes", NbtValue::List(list));
    compound(root.encode_root().unwrap())
}

/// Wraps network NBT bytes in HITBOX actor metadata.
fn compound(bytes: Vec<u8>) -> ActorMetadata {
    ActorMetadata {
        key: ActorDataKey::HitBox.wire_value(),
        value: ActorMetadataValue::Compound(Arc::from(bytes)),
    }
}

/// Spawns one entity with a known position and the supplied metadata.
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

/// Collects the current interaction boxes for the fixture entity.
fn boxes(store: &ActorStore) -> Vec<([f32; 3], [f32; 3])> {
    store.get(8).unwrap().hit_boxes().collect()
}

/// Checks every bound within floating-point translation tolerance.
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
fn hitbox_pivot_is_independent_of_actor_yaw() {
    let store = store_with(90.0, vec![hitbox_compound(&[RAISED])]);
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
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
fn empty_or_unreadable_updates_preserve_existing_hitboxes() {
    let collision = boxes(&store_with(0.0, vec![]));
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
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);

    let store = store_with(0.0, vec![compound(vec![10, 0, 0xff])]);
    assert_boxes_near(&boxes(&store), &collision);
}

#[test]
fn malformed_hitbox_entries_are_skipped() {
    let infinite: Entry = ([0.0; 3], [f32::INFINITY, 1.0, 1.0], [0.0; 3]);
    let store = store_with(0.0, vec![hitbox_compound(&[infinite, RAISED])]);
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
}

/// Applies another metadata packet without replacing the actor.
fn update(store: &mut ActorStore, sequence: u64, metadata: ActorMetadata) -> ActorApplyResult {
    store.apply(
        1,
        sequence,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 8,
            metadata: Arc::from([metadata]),
            properties: Arc::from([]),
            tick: 0,
        }),
    )
}

#[test]
fn reversed_endpoints_are_normalized_on_each_axis() {
    for axis in 0..3 {
        let (mut min, mut max, pivot) = RAISED;
        std::mem::swap(&mut min[axis], &mut max[axis]);
        let store = store_with(0.0, vec![hitbox_compound(&[(min, max, pivot)])]);
        assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
    }
}

#[test]
fn hitbox_updates_append_and_empty_updates_do_not_remove_boxes() {
    let mut store = store_with(0.0, vec![hitbox_compound(&[RAISED])]);
    let snapshot = store.get(8).unwrap().clone();
    update(&mut store, 2, hitbox_compound(&[RAISED]));
    assert_eq!(boxes(&store).len(), 2);
    assert_eq!(
        snapshot.hit_boxes().count(),
        1,
        "published snapshots retain their own boxes"
    );
    for (index, value) in [
        hitbox_compound(&[]),
        compound(NbtCompound::default().encode_root().unwrap()),
        compound(vec![10, 0, 0xff]),
        ActorMetadata {
            key: ActorDataKey::HitBox.wire_value(),
            value: ActorMetadataValue::Int(0),
        },
    ]
    .into_iter()
    .enumerate()
    {
        update(&mut store, index as u64 + 3, value);
        assert_eq!(boxes(&store).len(), 2);
    }
}

#[test]
fn hitboxes_are_not_truncated_at_sixty_four_entries() {
    let store = store_with(0.0, vec![hitbox_compound(&vec![RAISED; 130])]);
    assert_eq!(boxes(&store).len(), 130);
}

#[test]
fn custom_hitboxes_ignore_render_scale_and_collision_dimensions() {
    let mut store = store_with(
        270.0,
        vec![
            hitbox_compound(&[RAISED]),
            ActorMetadata {
                key: ActorDataKey::Scale.wire_value(),
                value: ActorMetadataValue::Float(4.0),
            },
            ActorMetadata {
                key: ActorDataKey::Width.wire_value(),
                value: ActorMetadataValue::Float(0.0),
            },
            ActorMetadata {
                key: ActorDataKey::Height.wire_value(),
                value: ActorMetadataValue::Float(0.0),
            },
        ],
    );
    assert_ne!(
        boxes(&store),
        [store.get(8).unwrap().bounding_box().unwrap()]
    );
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
    update(
        &mut store,
        2,
        ActorMetadata {
            key: ActorDataKey::Scale.wire_value(),
            value: ActorMetadataValue::Float(0.0),
        },
    );
    assert_boxes_near(&boxes(&store), &[([9.5, 67.0, 20.5], [10.5, 69.0, 21.5])]);
}

#[test]
fn missing_or_non_float_fields_use_zero_without_numeric_coercion() {
    let mut entry = NbtCompound::default();
    entry.insert("MaxY", NbtValue::Float(2.0));
    entry.insert("PivotY", NbtValue::Float(4.0));
    entry.insert("PivotX", NbtValue::Int(10));
    let mut root = NbtCompound::default();
    root.insert("Hitboxes", NbtValue::List(vec![NbtValue::Compound(entry)]));
    let store = store_with(0.0, vec![compound(root.encode_root().unwrap())]);
    assert_boxes_near(&boxes(&store), &[([10.0, 67.0, 20.0], [10.0, 69.0, 20.0])]);
}

#[test]
fn player_hitboxes_use_the_native_origin_above_collision_feet() {
    let ActorEvent::Spawn(mut event) = spawn(8, 80) else {
        unreachable!()
    };
    event.kind = protocol::ActorKind::Player {
        uuid: [0; 16],
        username: "fixture".into(),
    };
    event.position = [10.0, 64.0, 20.0];
    event.metadata = Arc::from([hitbox_compound(&[RAISED])]);
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, ActorEvent::Spawn(event));
    let y = protocol::PLAYER_NETWORK_OFFSET;
    assert_boxes_near(
        &boxes(&store),
        &[([9.5, 67.0 + y, 20.5], [10.5, 69.0 + y, 21.5])],
    );
}

#[test]
fn predicted_custom_boxes_match_live_interpolation_without_rotating() {
    let mut store = store_with(0.0, vec![hitbox_compound(&[RAISED])]);
    store.apply(
        1,
        2,
        ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 8,
            position: [Some(13.0), Some(67.0), Some(23.0)],
            position_origin: protocol::ActorPositionOrigin::Feet,
            pitch: None,
            yaw: Some(90.0),
            head_yaw: None,
            on_ground: None,
            teleported: false,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: protocol::ActorInterpolation {
                ticks: 3,
                force_completion: false,
            },
        }),
    );
    for ticks in [1, 2, 3] {
        store.predict_remote_motion(ticks);
        let position = store.pick_pose(8).unwrap().position;
        let predicted: Vec<_> = store.get(8).unwrap().hit_boxes_at(position).collect();
        assert_boxes_near(
            &predicted,
            &[(
                [position[0] - 0.5, position[1] + 3.0, position[2] + 0.5],
                [position[0] + 0.5, position[1] + 5.0, position[2] + 1.5],
            )],
        );
        let old = store.get(8).unwrap().position;
        store.advance_interpolation_ticks(ticks);
        assert_boxes_near(&boxes(&store), &predicted);
        if ticks == 1 {
            assert_ne!(old, position);
        }
    }
}

#[test]
fn well_formed_non_compound_hitboxes_are_skipped() {
    let collision = boxes(&store_with(0.0, vec![]));
    let mut root = NbtCompound::default();
    root.insert("Hitboxes", NbtValue::List(vec![NbtValue::Int(7)]));
    let store = store_with(0.0, vec![compound(root.encode_root().unwrap())]);
    assert_boxes_near(&boxes(&store), &collision);
}
