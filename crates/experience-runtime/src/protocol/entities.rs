//! Entity registration and transaction commands shared with the Dragonfly adapter.
use super::{ItemStack, Position};
use serde::{Deserialize, Serialize};

/// A process-local actor identity fenced by its world opening generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityHandle {
    pub id: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRotation {
    pub yaw: f64,
    pub pitch: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityBox {
    pub min: Position,
    pub max: Position,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySeat {
    pub position: Position,
    pub rotation_limit: Option<f64>,
    pub steers: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityArt {
    pub geometry: String,
    pub texture: String,
    pub render_controller: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityAppearance {
    Vanilla { id: String },
    Custom { art: EntityArt },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityPropertyDef {
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub initial: f32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityProperty {
    pub id: String,
    pub value: f32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDefinition {
    pub id: String,
    pub collision: EntityBox,
    pub appearance: EntityAppearance,
    pub seats: Vec<EntitySeat>,
    pub properties: Vec<EntityPropertyDef>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityState {
    pub handle: EntityHandle,
    pub kind: String,
    pub position: Position,
    pub rotation: EntityRotation,
    pub velocity: Position,
    pub name_tag: String,
    pub scale: f32,
    pub variant: i32,
    pub properties: Vec<EntityProperty>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySnapshot {
    pub state: EntityState,
    pub revision: u64,
    pub riders: Vec<EntityRider>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRider {
    pub player: String,
    pub seat: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityInput {
    pub rider: EntityRider,
    pub sideways: f32,
    pub forward: f32,
    pub yaw: f64,
    pub pitch: f64,
    pub jump: bool,
    pub sneak: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityEvent {
    Interact {
        player: String,
        hand: EntityHand,
        item: Option<ItemStack>,
    },
    Attacked {
        player: String,
        hand: EntityHand,
        item: Option<ItemStack>,
    },
    Mounted {
        rider: EntityRider,
    },
    Dismounted {
        rider: EntityRider,
    },
    Steering {
        input: EntityInput,
    },
    Despawned {},
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityHand {
    Main,
    Off,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityOp {
    Spawn { state: EntityState },
    Update { state: EntityState, teleport: bool },
    Despawn { handle: EntityHandle },
    Mount { handle: EntityHandle, seat: u32 },
    Dismount {},
}

/// Exercises every entity command and observation, including generation values above JSON's exact float range.
pub(super) fn fixtures() -> Vec<(&'static str, String)> {
    use super::*;
    let point = |n: f64| Position {
        x: n + 0.25,
        y: n + 0.5,
        z: n + 0.75,
    };
    let handle = EntityHandle {
        id: 9_007_199_254_740_993,
        generation: 9_007_199_254_740_995,
    };
    let world = WorldHandle {
        id: "probe:planet".into(),
        generation: handle.generation,
    };
    let state = EntityState {
        handle,
        kind: "probe:ship".into(),
        position: point(12.0),
        rotation: EntityRotation {
            yaw: 12.25,
            pitch: 3.5,
        },
        velocity: point(0.0),
        name_tag: "Ship".into(),
        scale: 1.25,
        variant: 3,
        properties: vec![EntityProperty {
            id: "probe:throttle".into(),
            value: 0.25,
        }],
    };
    let rider = EntityRider {
        player: "6f1c3c2e-5b7a-4d3e-9a51-0c8e2f4b7d10".into(),
        seat: 0,
    };
    let definition = EntityDefinition {
        id: state.kind.clone(),
        collision: EntityBox {
            min: point(-2.0),
            max: point(2.0),
        },
        appearance: EntityAppearance::Vanilla {
            id: "minecraft:boat".into(),
        },
        seats: vec![EntitySeat {
            position: point(0.0),
            rotation_limit: Some(45.5),
            steers: true,
        }],
        properties: vec![EntityPropertyDef {
            id: "probe:throttle".into(),
            min: -1.5,
            max: 1.5,
            initial: 0.25,
        }],
    };
    let mut custom = definition.clone();
    custom.id = "probe:custom_ship".into();
    custom.appearance = EntityAppearance::Custom {
        art: EntityArt {
            geometry: "/srv/probe/assets/ship.geo.json".into(),
            texture: "/srv/probe/assets/ship.png".into(),
            render_controller: "/srv/probe/assets/ship.render.json".into(),
        },
    };
    let mut out = vec![(
        "response_loaded_entities",
        serde_json::to_string_pretty(&Response::Loaded {
            entities: vec![definition, custom],
            protocol: PROTOCOL_VERSION,
            id: "probe".into(),
            version: "0.1.0".into(),
            blocks: Vec::new(),
            items: Vec::new(),
            focus: true,
            simulation: true,
            worlds: None,
        })
        .unwrap()
            + "\n",
    )];
    for event in [
        EntityEvent::Interact {
            player: rider.player.clone(),
            hand: EntityHand::Main,
            item: None,
        },
        EntityEvent::Attacked {
            player: rider.player.clone(),
            hand: EntityHand::Off,
            item: Some(ItemStack {
                id: "minecraft:stick".into(),
                metadata: 0,
                count: 1,
                max_count: 64,
                data: None,
                plain: true,
            }),
        },
        EntityEvent::Mounted {
            rider: rider.clone(),
        },
        EntityEvent::Dismounted {
            rider: rider.clone(),
        },
        EntityEvent::Steering {
            input: EntityInput {
                rider: rider.clone(),
                sideways: 0.25,
                forward: 0.5,
                yaw: 12.25,
                pitch: 3.5,
                jump: true,
                sneak: false,
            },
        },
        EntityEvent::Despawned {},
    ] {
        let (name, actor) = match &event {
            EntityEvent::Interact { .. } => (
                "request_callback_entity_interact",
                Some(rider.player.clone()),
            ),
            EntityEvent::Attacked { .. } => (
                "request_callback_entity_attacked",
                Some(rider.player.clone()),
            ),
            EntityEvent::Mounted { .. } => (
                "request_callback_entity_mounted",
                Some(rider.player.clone()),
            ),
            EntityEvent::Dismounted { .. } => (
                "request_callback_entity_dismounted",
                Some(rider.player.clone()),
            ),
            EntityEvent::Steering { .. } => (
                "request_callback_entity_steering",
                Some(rider.player.clone()),
            ),
            EntityEvent::Despawned {} => ("request_callback_entity_despawned", None),
        };
        let request = Request::Callback {
            seq: 7,
            info: Info {
                context: Some(WorldContext {
                    current: world.clone(),
                    player: None,
                    instances: vec![world.clone()],
                    stores: Vec::new(),
                    entities: vec![EntitySnapshot {
                        state: state.clone(),
                        revision: 4,
                        riders: vec![rider.clone()],
                    }],
                }),
                simulation: None,
                world_id: "world".into(),
                dimension_id: "overworld".into(),
                tick: 3,
                event_sequence: 7,
            },
            actor,
            world_min_y: -64,
            world_max_y: 319,
            data_budget: 1024,
            snapshot: Vec::new(),
            network: None,
            inventory: None,
            call: Call::Entity { handle, event },
        };
        out.push((name, serde_json::to_string_pretty(&request).unwrap() + "\n"));
    }
    let ops = vec![
        EntityOp::Spawn {
            state: state.clone(),
        },
        EntityOp::Update {
            state,
            teleport: false,
        },
        EntityOp::Mount { handle, seat: 0 },
        EntityOp::Dismount {},
        EntityOp::Despawn { handle },
    ]
    .into_iter()
    .map(|action| Op::Entity { action })
    .collect();
    out.push((
        "response_result_entities",
        serde_json::to_string_pretty(&Response::Result {
            seq: 7,
            outcome: Outcome::Committed { ops },
        })
        .unwrap()
            + "\n",
    ));
    out
}
