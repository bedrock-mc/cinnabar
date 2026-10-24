//! Authority and staging rules for world and typed-state callbacks.
use super::*;
use crate::host::cinnabar::experience_server::types as wit;
use crate::protocol::{self as p, StoreScope};

/// A callback with one authorized instance and three private scopes.
fn world_callback() -> CallbackRes {
    let mut res = Fixture::new().res();
    let handle = p::WorldHandle {
        id: "probe:one".into(),
        generation: 7,
    };
    res.context = Some(p::WorldContext {
        current: handle.clone(),
        player: Some(p::PlayerLocation {
            world: handle.clone(),
            epoch: 11,
            position: p::Position {
                x: 1.,
                y: 70.,
                z: 2.,
            },
        }),
        instances: vec![handle],
        stores: [StoreScope::Player, StoreScope::Instance, StoreScope::Mod]
            .into_iter()
            .map(|scope| p::StateRecord {
                key: p::StoreKey { scope, schema: 0 },
                revision: 0,
                data: None,
            })
            .collect(),
    });
    Arc::make_mut(&mut res.own)
        .worlds
        .definitions
        .push(p::WorldDefinition {
            id: "probe:plain".into(),
            settings: p::WorldSettings {
                generator: p::Generator::Empty,
                dimension: p::DimensionType::Overworld,
                persistent: false,
            },
        });
    res
}

#[test]
fn player_reads_require_the_callback_actor() {
    let mut res = world_callback();
    let location = res.player_location(ACTOR.into()).unwrap().unwrap();
    assert_eq!(location.epoch, 11);
    assert_eq!(location.world.generation, 7);
    assert!(matches!(
        res.player_location("00000000-0000-0000-0000-000000000000".into())
            .unwrap(),
        Err(WorldError::Denied)
    ));
    res.actor = None;
    res.player_store = false;
    assert!(matches!(
        res.store_read(wit::StoreKey {
            scope: wit::StoreScope::Player,
            schema: 1
        })
        .unwrap(),
        Err(WorldError::Denied)
    ));
}

#[test]
fn lifecycle_operations_require_owned_current_handles_and_exclusive_results() {
    let mut res = world_callback();
    let handle = res
        .resolve_world(wit::WorldInstanceId {
            value: "probe:one".into(),
        })
        .unwrap()
        .unwrap();
    let mut stale = handle.clone();
    stale.generation -= 1;
    assert!(matches!(
        res.unload_world(stale).unwrap(),
        Err(WorldError::Unavailable)
    ));
    assert!(matches!(
        res.create_world(wit::WorldInstance {
            id: wit::WorldInstanceId {
                value: "other:one".into()
            },
            definition: wit::WorldDefinitionId {
                value: "probe:plain".into()
            }
        })
        .unwrap(),
        Err(WorldError::Denied)
    ));
    res.unload_world(handle).unwrap().unwrap();
    assert!(res.tell(ACTOR.into(), "after lifecycle".into()).is_err());
    let mut res = world_callback();
    res.tell(ACTOR.into(), "before lifecycle".into())
        .unwrap()
        .unwrap();
    let handle = res.world().unwrap();
    assert!(matches!(
        res.unload_world(handle).unwrap(),
        Err(WorldError::Denied)
    ));
}

#[test]
fn state_records_preserve_schema_presence_and_share_block_quota() {
    let mut res = world_callback();
    res.budget = 2;
    let key = || wit::StoreKey {
        scope: wit::StoreScope::Instance,
        schema: 3,
    };
    res.store_write(key(), Some(vec![1, 2])).unwrap().unwrap();
    assert_eq!(res.store_read(key()).unwrap().unwrap(), Some(vec![1, 2]));
    assert!(matches!(
        res.store_read(wit::StoreKey {
            scope: wit::StoreScope::Instance,
            schema: 4
        })
        .unwrap(),
        Err(WorldError::Unavailable)
    ));
    assert!(matches!(
        res.store_write(
            wit::StoreKey {
                scope: wit::StoreScope::Mod,
                schema: 1
            },
            Some(vec![3])
        )
        .unwrap(),
        Err(WorldError::QuotaExceeded)
    ));
    res.store_write(key(), Some(vec![])).unwrap().unwrap();
    assert_eq!(res.store_read(key()).unwrap().unwrap(), Some(vec![]));
    res.store_write(key(), None).unwrap().unwrap();
    assert_eq!(res.store_read(key()).unwrap().unwrap(), None);
    assert_eq!(res.ops.len(), 1);
    assert_eq!(res.added, 0);
}

#[test]
fn leave_keeps_private_player_state_without_live_player_authority() {
    let mut res = world_callback();
    res.actor = None;
    res.player_store = true;
    let key = wit::StoreKey {
        scope: wit::StoreScope::Player,
        schema: 1,
    };
    res.store_write(key, Some(vec![1])).unwrap().unwrap();
    assert!(matches!(
        res.player_location(ACTOR.into()).unwrap(),
        Err(WorldError::Denied)
    ));
    assert!(matches!(
        res.tell(ACTOR.into(), "left".into()).unwrap(),
        Err(WorldError::PlayerUnavailable)
    ));
}

/// Both bounds are inside the world; the cells past them are outside even though the
/// snapshot holds them.
#[test]
fn world_height_bounds_reads_and_writes() {
    let mut res = Fixture {
        min_y: ANCHOR.y,
        max_y: ANCHOR.y,
        ..Fixture::new()
    }
    .res();
    assert_eq!(res.get_block(ANCHOR).unwrap(), Ok(COUNTER.to_owned()));
    assert_eq!(res.get_block(UP).unwrap(), Err(WorldError::OutOfBounds));
    assert_eq!(res.block_data(DOWN).unwrap(), Err(WorldError::OutOfBounds));
    assert_eq!(
        res.set_block(UP, AIR.to_owned()).unwrap(),
        Err(WorldError::OutOfBounds)
    );
    assert_eq!(
        res.set_block_data(DOWN, None).unwrap(),
        Err(WorldError::OutOfBounds)
    );
}
