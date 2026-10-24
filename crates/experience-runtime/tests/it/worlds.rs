//! The new guest imports and export run through the actual WASM component boundary.
use crate::common;
use experience_runtime::{
    callback::run,
    load::{engine, load},
    protocol::*,
};

/// An actor interaction with the world and private-state snapshots that the Go adapter supplies.
fn request(action: i32) -> Request {
    let mut request = common::interact(action);
    let Request::Callback { info, .. } = &mut request else {
        unreachable!()
    };
    let handle = WorldHandle {
        id: "probe:one".into(),
        generation: 7,
    };
    info.context = Some(WorldContext {
        current: handle.clone(),
        player: Some(PlayerLocation {
            world: handle.clone(),
            epoch: 11,
            position: Position {
                x: 1.25,
                y: 70.5,
                z: 3.75,
            },
        }),
        instances: vec![handle],
        stores: [StoreScope::Player, StoreScope::Instance, StoreScope::Mod]
            .into_iter()
            .map(|scope| StateRecord {
                key: StoreKey { scope, schema: 0 },
                revision: 0,
                data: None,
            })
            .collect(),
    });
    request
}

#[test]
fn guest_world_and_state_imports_stage_typed_operations() {
    let dir = common::probe_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    let registration = loaded.worlds.as_ref().unwrap();
    assert_eq!(registration.definitions[0].id, "probe:plain");
    for (action, kind) in [(30, "create"), (31, "transfer"), (32, "unload")] {
        let Outcome::Committed { ops } = run(&engine, &loaded, &request(action)) else {
            panic!("world guest action {action} failed")
        };
        assert_eq!(ops.len(), 1);
        let expected = match &ops[0] {
            Op::World {
                action: WorldOp::Create { instance },
            } => {
                assert_eq!(instance.definition, "probe:plain");
                "create"
            }
            Op::World {
                action:
                    WorldOp::Transfer {
                        instance, player, ..
                    },
            } => {
                assert_eq!(instance.generation, 7);
                assert_eq!(player, common::ACTOR);
                "transfer"
            }
            Op::World {
                action: WorldOp::Unload { instance },
            } => {
                assert_eq!(instance.generation, 7);
                "unload"
            }
            _ => panic!("unexpected operation"),
        };
        assert_eq!(expected, kind);
    }
    let Outcome::Committed { ops } = run(&engine, &loaded, &request(33)) else {
        panic!("state guest action failed")
    };
    assert_eq!(ops.len(), 3);
    for (op, scope) in ops
        .iter()
        .zip([StoreScope::Player, StoreScope::Instance, StoreScope::Mod])
    {
        assert_eq!(
            *op,
            Op::StoreWrite {
                key: StoreKey { scope, schema: 1 },
                data: Some("0102".into())
            }
        );
    }
}

#[test]
fn guest_player_export_receives_the_admitted_epoch() {
    let dir = common::probe_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    let mut request = request(30);
    let Request::Callback {
        info,
        call,
        snapshot,
        inventory,
        ..
    } = &mut request
    else {
        unreachable!()
    };
    snapshot.clear();
    *inventory = None;
    let location = info
        .context
        .as_ref()
        .unwrap()
        .player
        .as_ref()
        .unwrap()
        .clone();
    *call = Call::Player {
        player: common::ACTOR.into(),
        event: PlayerEvent::Join { location },
    };
    assert_eq!(
        run(&engine, &loaded, &request),
        Outcome::Committed {
            ops: vec![common::tell("player join 11")]
        }
    );
}
