//! Cross-language fixtures for every world operation and player observation.
use super::*;

/// Covers handles, f64 positions, state scopes, world settings, and every new union variant.
pub(super) fn fixtures() -> Vec<(&'static str, String)> {
    let handle = WorldHandle {
        id: "probe:one".into(),
        generation: 7,
    };
    let before = PlayerLocation {
        world: handle.clone(),
        epoch: 11,
        position: Position {
            x: 1.25,
            y: 70.5,
            z: 3.75,
        },
    };
    let mut after = before.clone();
    after.epoch += 1;
    let registration = WorldRegistration {
        definitions: vec![WorldDefinition {
            id: "probe:plain".into(),
            settings: WorldSettings {
                generator: Generator::Flat,
                dimension: DimensionType::Overworld,
                persistent: true,
            },
        }],
        startup: vec![WorldInstance {
            id: handle.id.clone(),
            definition: "probe:plain".into(),
        }],
    };
    let stores = [StoreScope::Player, StoreScope::Instance, StoreScope::Mod]
        .into_iter()
        .map(|scope| StateRecord {
            key: StoreKey { scope, schema: 2 },
            revision: 5,
            data: Some("abcd".into()),
        })
        .collect::<Vec<_>>();
    let context = WorldContext {
        current: handle.clone(),
        player: Some(before.clone()),
        instances: vec![handle.clone()],
        stores: stores.clone(),
    };
    let mut out = Vec::new();
    for (name, event) in [
        (
            "request_callback_join",
            PlayerEvent::Join {
                location: before.clone(),
            },
        ),
        (
            "request_callback_leave",
            PlayerEvent::Leave {
                location: before.clone(),
            },
        ),
        (
            "request_callback_world_change",
            PlayerEvent::WorldChange {
                before: before.clone(),
                after: after.clone(),
            },
        ),
        (
            "request_callback_move",
            PlayerEvent::Move {
                before: before.clone(),
                after: after.clone(),
            },
        ),
    ] {
        let request = Request::Callback {
            seq: 1,
            info: Info {
                context: Some(context.clone()),
                world_id: "world".into(),
                dimension_id: "overworld".into(),
                tick: 3,
                event_sequence: 4,
            },
            actor: None,
            world_min_y: -64,
            world_max_y: 319,
            data_budget: 1024,
            snapshot: Vec::new(),
            network: None,
            inventory: None,
            call: Call::Player {
                player: "11111111-1111-1111-1111-111111111111".into(),
                event,
            },
        };
        out.push((name, serde_json::to_string_pretty(&request).unwrap() + "\n"));
    }
    let response = Response::Loaded {
        protocol: PROTOCOL_VERSION,
        id: "probe".into(),
        version: "0.1.0".into(),
        blocks: vec![],
        items: vec![],
        focus: true,
        worlds: Some(registration.clone()),
    };
    out.push((
        "response_loaded_worlds",
        serde_json::to_string_pretty(&response).unwrap() + "\n",
    ));
    let mut ops = vec![
        Op::World {
            action: WorldOp::Create {
                instance: registration.startup[0].clone(),
            },
        },
        Op::World {
            action: WorldOp::Unload {
                instance: handle.clone(),
            },
        },
        Op::World {
            action: WorldOp::Transfer {
                player: "11111111-1111-1111-1111-111111111111".into(),
                instance: handle,
                position: after.position,
            },
        },
    ];
    ops.extend(stores.into_iter().map(|record| Op::StoreWrite {
        key: record.key,
        data: record.data,
    }));
    let response = Response::Result {
        seq: 1,
        outcome: Outcome::Committed { ops },
    };
    out.push((
        "response_result_worlds",
        serde_json::to_string_pretty(&response).unwrap() + "\n",
    ));
    out
}
