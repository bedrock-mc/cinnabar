//! Typed actors share callback authority and maintain a consistent staged view.
use super::*;
use crate::callback::wit;
use crate::protocol as p;

/// Creates a callback with one registered ship and a live instance opening.
fn callback() -> CallbackRes {
    let mut res = Fixture::new().res();
    res.context = Some(p::WorldContext {
        current: p::WorldHandle {
            id: "probe:planet".into(),
            generation: 3,
        },
        player: None,
        instances: Vec::new(),
        stores: Vec::new(),
        entities: Vec::new(),
    });
    Arc::make_mut(&mut res.own).entities = vec![p::EntityDefinition {
        id: "probe:ship".into(),
        collision: p::EntityBox {
            min: p::Position {
                x: -0.5,
                y: 0.0,
                z: -0.5,
            },
            max: p::Position {
                x: 0.5,
                y: 1.0,
                z: 0.5,
            },
        },
        appearance: p::EntityAppearance::Vanilla {
            id: "minecraft:boat".into(),
        },
        seats: vec![p::EntitySeat {
            position: p::Position {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            rotation_limit: None,
            steers: true,
        }],
        properties: Vec::new(),
    }];
    res
}

/// Spawns an actor and returns the identity reserved by the callback.
fn spawn(res: &mut CallbackRes) -> wit::EntityHandle {
    res.spawn_entity(
        wit::EntityTypeId {
            value: "probe:ship".into(),
        },
        wit::Position {
            x: 0.0,
            y: 70.0,
            z: 0.0,
        },
        wit::EntityRotation {
            yaw: 0.0,
            pitch: 0.0,
        },
    )
    .unwrap()
    .unwrap()
}

#[test]
fn stale_handles_and_invalid_updates_leave_staged_state_intact() {
    let mut res = callback();
    let handle = spawn(&mut res);
    assert_eq!(handle.generation, 3);
    let mut state = res.entities().unwrap().pop().unwrap();
    state.handle.generation = 2;
    assert!(matches!(
        res.update_entity(state, false).unwrap(),
        Err(WorldError::Unavailable)
    ));
    let mut state = res.entities().unwrap().pop().unwrap();
    state.scale = f32::NAN;
    assert!(matches!(
        res.update_entity(state, false).unwrap(),
        Err(WorldError::OutOfBounds)
    ));
    assert_eq!(res.entities().unwrap()[0].scale, 1.0);
    assert_eq!(res.ops.len(), 1);
    res.despawn_entity(handle).unwrap().unwrap();
    assert!(res.entities().unwrap().is_empty());
    assert!(matches!(
        res.despawn_entity(handle).unwrap(),
        Err(WorldError::Unavailable)
    ));
}

#[test]
fn staged_mount_and_dismount_share_seat_occupancy() {
    let mut res = callback();
    let handle = spawn(&mut res);
    res.mount_entity(handle, 0).unwrap().unwrap();
    assert!(matches!(
        res.mount_entity(handle, 0).unwrap(),
        Err(WorldError::Unavailable)
    ));
    res.dismount_entity().unwrap().unwrap();
    assert!(matches!(
        res.dismount_entity().unwrap(),
        Err(WorldError::Unavailable)
    ));
    res.mount_entity(handle, 0).unwrap().unwrap();
    assert_eq!(res.context.as_ref().unwrap().entities[0].riders.len(), 1);
}

#[test]
fn actorless_callbacks_cannot_board_and_entity_calls_share_fuel_independent_limits() {
    let mut res = callback();
    let handle = spawn(&mut res);
    res.actor = None;
    assert!(matches!(
        res.mount_entity(handle, 0).unwrap(),
        Err(WorldError::PlayerUnavailable)
    ));
    res.host_calls = MAX_HOST_CALLS;
    assert!(res.entities().unwrap_err().is::<LimitExceeded>());
}

#[test]
fn spawn_handles_use_ordinals_when_block_writes_are_coalesced() {
    let mut res = callback();
    assert_eq!(res.set_block_data(ANCHOR, Some(vec![1])).unwrap(), Ok(()));
    let first = spawn(&mut res);
    assert_eq!(res.set_block(ANCHOR, COUNTER.to_owned()).unwrap(), Ok(()));
    let second = spawn(&mut res);
    res.despawn_entity(first).unwrap().unwrap();
    let third = spawn(&mut res);
    let base = res.info.event_sequence * MAX_STAGED_OPS as u64;
    assert_eq!(
        [first.id, second.id, third.id],
        [base + 1, base + 2, base + 3]
    );
    let staged: Vec<_> = res
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Entity {
                action: p::EntityOp::Spawn { state },
            } => Some(state.handle.id),
            _ => None,
        })
        .collect();
    assert_eq!(staged, [first.id, second.id, third.id]);
    assert_eq!(res.entities().unwrap().len(), 2);
}

#[test]
fn leave_cannot_spawn_entities_or_discard_private_state() {
    let mut res = callback();
    res.actor = None;
    res.player_store = true;
    res.store_write(
        wit::StoreKey {
            scope: wit::StoreScope::Player,
            schema: 1,
        },
        Some(vec![1]),
    )
    .unwrap()
    .unwrap();
    let result = res
        .spawn_entity(
            wit::EntityTypeId {
                value: "probe:ship".into(),
            },
            wit::Position {
                x: 0.0,
                y: 70.0,
                z: 0.0,
            },
            wit::EntityRotation {
                yaw: 0.0,
                pitch: 0.0,
            },
        )
        .unwrap();
    assert!(matches!(result, Err(WorldError::Unavailable)));
    assert!(res.ops.iter().all(|op| matches!(op, Op::StoreWrite { .. })));
    assert_eq!(res.ops.len(), 1);
    assert!(res.entities().unwrap().is_empty());
}
