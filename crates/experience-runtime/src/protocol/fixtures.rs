//! The golden fixtures the Go adapter checks: one per request, response, outcome, op and call
//! variant, plus the limits and enums both sides must agree on.

use serde::Serialize;

use super::*;

/// Constants the Go adapter must agree with: the frame limit, the protocol version, the limits
/// its commit check enforces again, and the client message limits that must fit the client wire
/// protocol's.
#[derive(Serialize)]
struct Limits {
    max_frame_bytes: usize,
    protocol: u32,
    max_block_data_bytes: usize,
    max_staged_ops: usize,
    max_tells: usize,
    max_tell_bytes: usize,
    max_client_sends: usize,
    max_client_send_bytes: usize,
    max_value_depth: usize,
    max_name_bytes: usize,
    max_state_values: usize,
    max_state_combinations: usize,
    max_bones: usize,
    max_permutations: usize,
    max_materials: usize,
    max_condition_tests: usize,
    max_flipbook_frames: usize,
    max_geometry_bytes: usize,
    max_network_blocks: usize,
    max_network_data_bytes: usize,
    inventory_slots: usize,
    hotbar_slots: u8,
    max_items: usize,
    max_stack_size: u8,
    max_item_data_bytes: usize,
    max_server_items: usize,
    max_recipes: usize,
    max_recipe_input_items: usize,
    max_schedule_ticks: u32,
    max_scheduled_updates: usize,
}

/// A placement trait's state and its values, in the client's order.
#[derive(Serialize)]
struct PlacementValues {
    placement: PlacementState,
    state: &'static str,
    values: &'static [&'static str],
}

/// Every protocol enum string, so the Go adapter can check its sets against Rust.
#[derive(Serialize)]
struct Enums {
    faces: [Face; 6],
    causes: [Cause; 3],
    fail_kinds: [FailKind; 4],
    render_methods: [RenderMethod; 4],
    placement_states: [PlacementState; 4],
    placement_values: Vec<PlacementValues>,
}

/// Lists every variant of a fieldless enum. The same list feeds an exhaustive `match`, so adding
/// a variant fails to compile until it is listed here.
macro_rules! all_variants {
    ($ty:ident: $($variant:ident),+ $(,)?) => {{
        let _exhaustive = |value: $ty| match value {
            $($ty::$variant)|+ => (),
        };
        [$($ty::$variant),+]
    }};
}

/// The golden fixtures, as `(file stem, pretty JSON with a trailing newline)`: one per
/// request, response, outcome, op and call variant, plus `limits` and `enums`.
pub fn fixtures() -> Vec<(&'static str, String)> {
    fn pretty(msg: &impl Serialize) -> String {
        serde_json::to_string_pretty(msg).expect("fixtures serialize") + "\n"
    }
    let player = "6f1c3c2e-5b7a-4d3e-9a51-0c8e2f4b7d10";
    let controller = BlockPos {
        x: 12,
        y: 64,
        z: -7,
    };
    let neighbor = BlockPos {
        x: 13,
        y: 64,
        z: -7,
    };
    let callback = |seq: u64, actor: Option<&str>, call: Call| Request::Callback {
        seq,
        info: Info {
            world_id: "world".to_owned(),
            dimension_id: "overworld".to_owned(),
            tick: 48_213,
            event_sequence: seq + 900,
        },
        actor: actor.map(str::to_owned),
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 4096,
        snapshot: vec![
            Cell {
                pos: controller,
                loaded: true,
                id: "benergistics:controller".to_owned(),
                owned: true,
                data: Some(hex::encode(&[0x01, 0x00, 0xff])),
                states: vec![
                    state(
                        "benergistics:state",
                        StateValue::Choice("online".to_owned()),
                    ),
                    state("benergistics:powered", StateValue::Bool(true)),
                ],
            },
            Cell {
                pos: neighbor,
                loaded: true,
                id: "minecraft:stone".to_owned(),
                owned: false,
                data: None,
                states: Vec::new(),
            },
            Cell {
                pos: BlockPos {
                    x: 12,
                    y: 64,
                    z: 400,
                },
                loaded: false,
                id: String::new(),
                owned: false,
                data: None,
                states: Vec::new(),
            },
        ],
        network: None,
        inventory: None,
        call,
    };
    let result = |seq: u64, outcome: Outcome| Response::Result { seq, outcome };
    // A terminal right-click in a network: the anchor's network and the actor's inventory, a
    // stack of plain stone in hand and an Experience's own cell in the offhand.
    let mut slots = vec![None; 37];
    slots[0] = Some(ItemStack {
        id: "minecraft:stone".to_owned(),
        metadata: 0,
        count: 64,
        max_count: 64,
        data: None,
        plain: true,
    });
    slots[36] = Some(ItemStack {
        id: "benergistics:item_storage_cell_1k".to_owned(),
        metadata: 0,
        count: 1,
        max_count: 1,
        data: Some(hex::encode(&[0x01, 0x00, 0x00])),
        plain: false,
    });
    let mut networked = callback(
        7,
        Some(player),
        Call::Interact {
            player: player.to_owned(),
            pos: controller,
            face: Face::Up,
        },
    );
    if let Request::Callback {
        network, inventory, ..
    } = &mut networked
    {
        *network = Some(Network {
            blocks: vec![controller, neighbor],
            truncated: false,
        });
        *inventory = Some(Inventory { selected: 0, slots });
    }
    let new_stack = |id: &str, count: u8, data: Option<&[u8]>| NewStack {
        id: id.to_owned(),
        metadata: 0,
        count,
        data: data.map(hex::encode),
    };
    let pixel = |x: f32, y: f32, z: f32| Pixel { x, y, z };
    let north_on = vec![vec![StateTest {
        state: state("benergistics:north", StateValue::Bool(true)),
        equal: true,
    }]];
    // Every scalar type, the integer beyond what a JSON double holds exactly, and a list of
    // records, one of them empty, next to an empty list.
    let record = vec![
        Scalar::Bool(true),
        Scalar::Integer(-9_007_199_254_740_993),
        Scalar::Text("ME Controller \"linked\"".to_owned()),
        Scalar::Choice(2),
        Scalar::List(vec![
            Scalar::Record(vec![
                Scalar::Text("minecraft:iron_ingot".to_owned()),
                Scalar::Integer(64),
            ]),
            Scalar::Record(Vec::new()),
        ]),
        Scalar::List(Vec::new()),
    ];
    let without_snapshot = |mut request: Request| {
        if let Request::Callback { snapshot, .. } = &mut request {
            snapshot.clear();
        }
        request
    };
    let client_message = without_snapshot(callback(
        5,
        Some(player),
        Call::ClientMessage {
            player: player.to_owned(),
            channel: "benergistics.ack".to_owned(),
            schema: 1,
            payload: record.clone(),
            focus: None,
        },
    ));
    // An epoch with its player's focus has the snapshot of an interaction with that block.
    let epoch = callback(
        6,
        Some(player),
        Call::Epoch {
            player: player.to_owned(),
            focus: Some(controller),
        },
    );
    let asset = |file: &str| format!("/srv/experiences/benergistics/assets/{file}");
    let planks = || {
        ["minecraft:oak_planks", "minecraft:spruce_planks"]
            .map(|id| ItemMatch {
                id: id.to_owned(),
                metadata: None,
            })
            .to_vec()
    };
    let texture = |slot: &str, file: &str| Texture {
        slot: slot.to_owned(),
        path: format!("/srv/experiences/benergistics/assets/{file}"),
    };

    vec![
        (
            "request_load",
            pretty(&Request::Load {
                dir: "/srv/experiences/benergistics".to_owned(),
                items: vec![
                    ServerItem {
                        id: "minecraft:stone".to_owned(),
                        metadata: 0,
                        max_count: 64,
                        off_hand: false,
                        plain: true,
                    },
                    ServerItem {
                        id: "minecraft:ender_pearl".to_owned(),
                        metadata: 0,
                        max_count: 16,
                        off_hand: false,
                        plain: true,
                    },
                ],
                recipes: vec![
                    // Sticks: a 1×2 shape of planks, any of the tag's woods.
                    Recipe {
                        shape: Some([1, 2]),
                        inputs: vec![planks(), planks()],
                        output: Crafted {
                            id: "minecraft:stick".to_owned(),
                            metadata: 0,
                            count: 4,
                        },
                    },
                    // Red dye from a poppy, shapeless, of one metadata.
                    Recipe {
                        shape: None,
                        inputs: vec![vec![ItemMatch {
                            id: "minecraft:red_flower".to_owned(),
                            metadata: Some(0),
                        }]],
                        output: Crafted {
                            id: "minecraft:red_dye".to_owned(),
                            metadata: 0,
                            count: 1,
                        },
                    },
                ],
            }),
        ),
        (
            "request_callback_place",
            pretty(&callback(
                1,
                Some(player),
                Call::Place {
                    change: Change {
                        pos: controller,
                        actor: Some(player.to_owned()),
                        cause: Cause::Player,
                        before_id: "minecraft:air".to_owned(),
                        after_id: "benergistics:controller".to_owned(),
                        previous_data: None,
                    },
                },
            )),
        ),
        (
            "request_callback_break",
            pretty(&callback(
                2,
                None,
                Call::Break {
                    change: Change {
                        pos: controller,
                        actor: None,
                        cause: Cause::Environment,
                        before_id: "benergistics:controller".to_owned(),
                        after_id: "minecraft:air".to_owned(),
                        previous_data: Some(hex::encode(&[0x01, 0x00, 0xff])),
                    },
                },
            )),
        ),
        (
            "request_callback_interact",
            pretty(&callback(
                3,
                Some(player),
                Call::Interact {
                    player: player.to_owned(),
                    pos: controller,
                    face: Face::North,
                },
            )),
        ),
        (
            "request_callback_neighbor",
            pretty(&callback(
                4,
                None,
                Call::Neighbor {
                    pos: controller,
                    neighbor,
                },
            )),
        ),
        ("request_callback_client_message", pretty(&client_message)),
        ("request_callback_epoch", pretty(&epoch)),
        ("request_callback_network", pretty(&networked)),
        (
            "request_callback_scheduled_update",
            pretty(&callback(
                8,
                None,
                Call::ScheduledUpdate { pos: controller },
            )),
        ),
        ("request_shutdown", pretty(&Request::Shutdown {})),
        (
            "response_loaded",
            pretty(&Response::Loaded {
                protocol: PROTOCOL_VERSION,
                id: "benergistics".to_owned(),
                version: "0.1.0".to_owned(),
                blocks: vec![
                    BlockDef {
                        id: "benergistics:controller".to_owned(),
                        display_name: "ME Controller".to_owned(),
                        textures: vec![
                            texture("*", "controller.png"),
                            texture("up", "controller_powered.png"),
                        ],
                        mining: Mining::Breakable { hardness: 1.5 },
                        states: Vec::new(),
                        placement: Vec::new(),
                        visual: None,
                        permutations: Vec::new(),
                        network: true,
                    },
                    BlockDef {
                        id: "benergistics:creative_energy_cell".to_owned(),
                        display_name: "Creative Energy Cell".to_owned(),
                        textures: vec![texture("*", "creative_energy_cell.png")],
                        mining: Mining::Unbreakable {},
                        states: Vec::new(),
                        placement: Vec::new(),
                        visual: None,
                        permutations: Vec::new(),
                        network: true,
                    },
                    // A cable: six connection states shown by bone visibility, a flipbook, a
                    // placement trait and a permutation that turns it.
                    BlockDef {
                        id: "benergistics:glass_cable".to_owned(),
                        display_name: "ME Glass Cable".to_owned(),
                        textures: Vec::new(),
                        mining: Mining::Breakable { hardness: 0.5 },
                        states: vec![
                            StateDef {
                                name: "benergistics:north".to_owned(),
                                values: StateValues::Bool,
                            },
                            StateDef {
                                name: "benergistics:colour".to_owned(),
                                values: StateValues::Choices(vec![
                                    "fluix".to_owned(),
                                    "white".to_owned(),
                                ]),
                            },
                        ],
                        placement: vec![PlacementState::FacingDirection],
                        visual: Some(Visual {
                            geometry: Some(asset("glass_cable.geo.json")),
                            materials: vec![Material {
                                instance: "*".to_owned(),
                                path: asset("glass_cable.png"),
                                render_method: RenderMethod::AlphaTest,
                                flipbook: Some(Flipbook {
                                    ticks_per_frame: 25,
                                    frames: vec![0, 1, 2, 1],
                                    blend_frames: true,
                                }),
                            }],
                            bones: vec![BoneVisibility {
                                bone: "north".to_owned(),
                                visible: north_on.clone(),
                            }],
                            collision: Some(PixelBox {
                                min: pixel(6.5, 6.5, 6.5),
                                max: pixel(9.5, 9.5, 9.5),
                            }),
                            selection: None,
                            rotation: None,
                        }),
                        permutations: vec![Permutation {
                            when: vec![
                                north_on[0].clone(),
                                vec![StateTest {
                                    state: state(
                                        "benergistics:colour",
                                        StateValue::Choice("white".to_owned()),
                                    ),
                                    equal: false,
                                }],
                            ],
                            geometry: None,
                            materials: None,
                            bones: None,
                            collision: None,
                            selection: None,
                            rotation: Some(QuarterTurns { x: 0, y: 1, z: 0 }),
                        }],
                        network: true,
                    },
                ],
                items: vec![ItemDef {
                    id: "benergistics:item_storage_cell_1k".to_owned(),
                    display_name: "1k ME Item Storage Cell".to_owned(),
                    icon: asset("item_storage_cell_1k.png"),
                    max_stack: 1,
                }],
                focus: true,
            }),
        ),
        (
            "response_load_failed",
            pretty(&Response::LoadFailed {
                reason: "server.wasm: SHA-256 does not match experience.toml".to_owned(),
            }),
        ),
        (
            "response_result_committed",
            pretty(&result(
                1,
                Outcome::Committed {
                    ops: vec![
                        Op::SetBlock {
                            pos: neighbor,
                            id: "benergistics:creative_energy_cell".to_owned(),
                        },
                        Op::SetBlockData {
                            pos: controller,
                            data: Some(hex::encode(&[0x02, 0x10, 0xab])),
                        },
                        Op::SetBlockData {
                            pos: neighbor,
                            data: None,
                        },
                        Op::Tell {
                            player: player.to_owned(),
                            text: "Network online".to_owned(),
                        },
                        Op::SendClient {
                            player: player.to_owned(),
                            channel: "benergistics.controller".to_owned(),
                            schema: 1,
                            payload: record,
                        },
                        Op::SetBlockState {
                            pos: controller,
                            states: vec![state(
                                "benergistics:state",
                                StateValue::Choice("conflicted".to_owned()),
                            )],
                        },
                        Op::SetSlot {
                            slot: 0,
                            stack: Some(new_stack("minecraft:stone", 32, None)),
                        },
                        Op::SetSlot {
                            slot: 36,
                            stack: None,
                        },
                        Op::ScheduleUpdate {
                            pos: controller,
                            ticks: 3,
                        },
                        Op::DropItem {
                            pos: neighbor,
                            stack: new_stack(
                                "benergistics:item_storage_cell_1k",
                                1,
                                Some(&[0x01, 0x00, 0x00]),
                            ),
                        },
                    ],
                },
            )),
        ),
        (
            "response_result_rejected",
            pretty(&result(
                2,
                Outcome::Rejected {
                    reason: "a controller already powers this network".to_owned(),
                },
            )),
        ),
        (
            "response_result_failed",
            pretty(&result(
                3,
                Outcome::Failed {
                    kind: FailKind::Fuel,
                    reason: "callback exhausted its fuel".to_owned(),
                },
            )),
        ),
        (
            "limits",
            pretty(&Limits {
                max_frame_bytes: MAX_FRAME_BYTES,
                protocol: PROTOCOL_VERSION,
                max_block_data_bytes: MAX_BLOCK_DATA_BYTES,
                max_staged_ops: MAX_STAGED_OPS,
                max_tells: MAX_TELLS,
                max_tell_bytes: MAX_TELL_BYTES,
                max_client_sends: MAX_CLIENT_SENDS,
                max_client_send_bytes: MAX_CLIENT_SEND_BYTES,
                max_value_depth: MAX_VALUE_DEPTH,
                max_name_bytes: MAX_NAME_BYTES,
                max_state_values: MAX_STATE_VALUES,
                max_state_combinations: MAX_STATE_COMBINATIONS,
                max_bones: MAX_BONES,
                max_permutations: MAX_PERMUTATIONS,
                max_materials: MAX_MATERIALS,
                max_condition_tests: MAX_CONDITION_TESTS,
                max_flipbook_frames: MAX_FLIPBOOK_FRAMES,
                max_geometry_bytes: MAX_GEOMETRY_BYTES,
                max_network_blocks: MAX_NETWORK_BLOCKS,
                max_network_data_bytes: MAX_NETWORK_DATA_BYTES,
                inventory_slots: INVENTORY_SLOTS,
                hotbar_slots: HOTBAR_SLOTS,
                max_items: MAX_ITEMS,
                max_stack_size: MAX_STACK_SIZE,
                max_item_data_bytes: MAX_ITEM_DATA_BYTES,
                max_server_items: MAX_SERVER_ITEMS,
                max_recipes: MAX_RECIPES,
                max_recipe_input_items: MAX_RECIPE_INPUT_ITEMS,
                max_schedule_ticks: MAX_SCHEDULE_TICKS,
                max_scheduled_updates: MAX_SCHEDULED_UPDATES,
            }),
        ),
        (
            "enums",
            pretty(&Enums {
                faces: all_variants!(Face: Down, Up, North, South, West, East),
                causes: all_variants!(Cause: Player, Guest, Environment),
                fail_kinds: all_variants!(FailKind: Trap, Fuel, Deadline, Limit),
                render_methods: all_variants!(
                    RenderMethod: Opaque,
                    AlphaTest,
                    Blend,
                    DoubleSided
                ),
                placement_states: all_variants!(
                    PlacementState: CardinalDirection,
                    FacingDirection,
                    BlockFace,
                    VerticalHalf
                ),
                placement_values: PlacementState::ALL
                    .into_iter()
                    .map(|placement| {
                        let (state, values) = placement.state();
                        PlacementValues {
                            placement,
                            state,
                            values,
                        }
                    })
                    .collect(),
            }),
        ),
    ]
}

/// A state named `name` holding `value`.
fn state(name: &str, value: StateValue) -> BlockState {
    BlockState {
        name: name.to_owned(),
        value,
    }
}
