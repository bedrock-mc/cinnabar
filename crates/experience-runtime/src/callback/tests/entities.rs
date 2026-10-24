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
