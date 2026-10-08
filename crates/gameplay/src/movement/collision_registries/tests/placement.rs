use super::{BREG_V2193, active_content_registry_protocol, bind, synthetic_preg};
use crate::{
    block_use::{PlacementContext, UseSurroundings, predicted_placement},
    movement::PhysicsCollisionRegistries,
    placement_state::PlacementInput,
};
use assets::{NetworkIdMode, RegistryRecord};
use std::sync::OnceLock;
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// Shares the pinned palette and synthetic collision fixture across placement tests.
fn fixtures() -> &'static (Box<[RegistryRecord]>, PhysicsCollisionRegistries) {
    static FIXTURES: OnceLock<(Box<[RegistryRecord]>, PhysicsCollisionRegistries)> =
        OnceLock::new();
    FIXTURES.get_or_init(|| {
        let protocol = active_content_registry_protocol();
        let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
        let registries = bind(
            BREG_V2193,
            &synthetic_preg(protocol, BREG_V2193, &records),
            protocol,
        )
        .unwrap();
        (records, registries)
    })
}

/// Selects one real state by its observable values rather than palette ordering.
fn record(name: &str, values: &[(&str, serde_json::Value)]) -> &'static RegistryRecord {
    fixtures()
        .0
        .iter()
        .find(|record| {
            if record.name.as_ref() != name {
                return false;
            }
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            values
                .iter()
                .all(|(key, value)| state[*key]["value"] == *value)
        })
        .unwrap_or_else(|| panic!("missing pinned state for {name}: {values:?}"))
}

/// Selects the correct store identity for either palette encoding.
fn id(record: &RegistryRecord, mode: NetworkIdMode) -> u32 {
    match mode {
        NetworkIdMode::Sequential => record.sequential_id,
        NetworkIdMode::Hashed => record.network_hash,
    }
}

/// Builds loaded air around a solid clicked support.
fn store(mode: NetworkIdMode) -> ChunkStore {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(
            key,
            BlockUpdate::new(8, 8, 8, 0, id(record("minecraft:stone", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    store
}

/// Supplies collision-free player facts for a top-face placement.
fn surroundings() -> UseSurroundings {
    UseSurroundings {
        clicked_identifier: Some("minecraft:stone".to_owned()),
        clicked_canonical_state: Some("{}".to_owned()),
        held_block_identifier: None,
        neighbor_identifier: Some("minecraft:air".to_owned()),
        player_box: ([0.0; 3], [0.6, 1.8, 0.6]),
        actor_boxes: Vec::new(),
        sneaking: false,
        placed_boxes: None,
    }
}

/// Uses a bounded loaded dimension with a top-face click at its center.
fn context(around: &UseSurroundings) -> PlacementContext<'_> {
    PlacementContext {
        clicked: [8, 8, 8],
        input: PlacementInput {
            face: 1,
            click_position: [0.5, 1.0, 0.5],
            yaw: 0.0,
            pitch: 0.0,
        },
        surroundings: around,
        build_height: 0..16,
    }
}

/// Resolves the real palette and world without transport or worker polling.
fn predict(
    name: &str,
    store: &ChunkStore,
    mode: NetworkIdMode,
    context: &PlacementContext<'_>,
) -> Option<crate::block_use::PredictedPlacement> {
    let registry = &fixtures().1;
    let world = sim::PaletteWorld::new(store, registry.registry(mode), 0);
    predicted_placement(registry, mode, id(record(name, &[]), mode), &world, context)
}

#[test]
fn oriented_and_half_block_items_produce_local_predictions_immediately() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for name in [
            "minecraft:oak_log",
            "minecraft:oak_slab",
            "minecraft:trapdoor",
            "minecraft:torch",
            "minecraft:glass_pane",
            "minecraft:oak_fence",
        ] {
            let store = store(mode);
            let around = surroundings();
            let result = predict(name, &store, mode, &context(&around)).expect(name);
            assert_eq!(result.position, [8, 9, 8]);
            assert_eq!(
                fixtures().1.block_identifier(mode, result.block),
                Some(name)
            );
        }
    }
}

#[test]
fn a_matching_clicked_slab_predicts_its_double_in_the_same_cell() {
    use serde_json::json;
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut store = store(mode);
        let slab = record(
            "minecraft:oak_slab",
            &[("minecraft:vertical_half", json!("bottom"))],
        );
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(8, 8, 8, 0, id(slab, mode)),
                id(record("minecraft:air", &[]), mode),
            )
            .unwrap();
        let mut around = surroundings();
        around.clicked_identifier = Some(slab.name.to_string());
        around.clicked_canonical_state = Some(slab.canonical_state.to_string());
        let result = predict("minecraft:oak_slab", &store, mode, &context(&around)).unwrap();
        assert_eq!(result.position, [8, 8, 8]);
        assert_eq!(
            fixtures().1.block_identifier(mode, result.block),
            Some("minecraft:oak_double_slab")
        );
    }
}

#[test]
fn invalid_support_height_unloaded_data_and_actor_overlap_refuse_prediction() {
    let mode = NetworkIdMode::Sequential;
    let mut store = store(mode);
    let mut around = surroundings();
    let registry = &fixtures().1;
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 8, 8, 0, id(record("minecraft:air", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    around.clicked_identifier = Some("minecraft:air".to_owned());
    assert!(predict("minecraft:torch", &store, mode, &context(&around)).is_none());
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 8, 8, 0, id(record("minecraft:stone", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    around.clicked_identifier = Some("minecraft:stone".to_owned());
    let mut limited = context(&around);
    limited.build_height = 0..9;
    assert!(predict("minecraft:oak_log", &store, mode, &limited).is_none());
    limited.build_height = 0..32;
    limited.clicked = [8, 15, 8];
    assert!(predict("minecraft:oak_log", &store, mode, &limited).is_none());
    around.actor_boxes.push(([8.1, 9.1, 8.1], [8.9, 9.9, 8.9]));
    assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
    around.actor_boxes.clear();
    around.player_box = ([8.1, 9.1, 8.1], [8.9, 10.9, 8.9]);
    assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
    assert!(registry.block_is_full_cube(mode, id(record("minecraft:stone", &[]), mode)));
}

#[test]
fn collision_uses_the_resolved_slab_half() {
    let mode = NetworkIdMode::Sequential;
    let mut store = store(mode);
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 9, 9, 0, id(record("minecraft:stone", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    let mut around = surroundings();
    around.actor_boxes.push(([8.1, 9.6, 8.1], [8.9, 9.9, 8.9]));
    let mut click = context(&around);
    // A bottom slab clears this actor, whereas a top slab intersects it.
    assert!(predict("minecraft:oak_slab", &store, mode, &click).is_some());
    click.input.face = 2;
    click.input.click_position[1] = 0.9;
    click.clicked = [8, 9, 9];
    assert!(predict("minecraft:oak_slab", &store, mode, &click).is_none());
}

#[test]
fn connection_collision_includes_the_predicted_pane_arm() {
    let mode = NetworkIdMode::Sequential;
    let mut store = store(mode);
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(9, 9, 8, 0, id(record("minecraft:stone", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    let mut around = surroundings();
    let result = predict("minecraft:glass_pane", &store, mode, &context(&around)).unwrap();
    let state: serde_json::Value = serde_json::from_str(
        fixtures()
            .1
            .block_canonical_state(mode, result.block)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(state["minecraft:connection_east"]["value"], 1);
    around
        .actor_boxes
        .push(([8.7, 9.1, 8.46], [8.9, 9.9, 8.54]));
    assert!(predict("minecraft:glass_pane", &store, mode, &context(&around)).is_none());
}

#[test]
fn existing_snow_stacks_in_place_and_checks_actors_against_the_whole_cell() {
    use serde_json::json;
    let mode = NetworkIdMode::Sequential;
    let mut store = store(mode);
    let snow = record("minecraft:snow_layer", &[("height", json!(0))]);
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 8, 8, 0, id(snow, mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    let mut around = surroundings();
    around.clicked_identifier = Some(snow.name.to_string());
    around.clicked_canonical_state = Some(snow.canonical_state.to_string());
    let result = predict("minecraft:snow_layer", &store, mode, &context(&around)).unwrap();
    assert_eq!(result.position, [8, 8, 8]);
    let state: serde_json::Value = serde_json::from_str(
        fixtures()
            .1
            .block_canonical_state(mode, result.block)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(state["height"]["value"], 1);
    around.actor_boxes.push(([8.1, 8.8, 8.1], [8.9, 8.9, 8.9]));
    assert!(predict("minecraft:snow_layer", &store, mode, &context(&around)).is_none());
}

#[test]
fn stale_clicked_and_destination_observations_refuse_prediction() {
    let mode = NetworkIdMode::Sequential;
    let mut store = store(mode);
    let around = surroundings();
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 9, 8, 0, id(record("minecraft:stone", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 8, 8, 0, id(record("minecraft:air", &[]), mode)),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
    assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
}

#[test]
fn replacing_non_air_destinations_waits_for_verified_replacement_rules() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut store = store(mode);
        let mut around = surroundings();
        around.neighbor_identifier = Some("minecraft:short_grass".to_owned());
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(8, 9, 8, 0, id(record("minecraft:short_grass", &[]), mode)),
                id(record("minecraft:air", &[]), mode),
            )
            .unwrap();
        assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
        let mut store = self::store(mode);
        let mut around = surroundings();
        around.clicked_identifier = Some("minecraft:red_shrub".to_owned());
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(8, 8, 8, 0, id(record("minecraft:red_shrub", &[]), mode)),
                id(record("minecraft:air", &[]), mode),
            )
            .unwrap();
        assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
    }
}

#[test]
fn unresolved_families_and_occupied_destinations_stay_server_confirmed() {
    let mode = NetworkIdMode::Sequential;
    let store = store(mode);
    let mut around = surroundings();
    for name in [
        "minecraft:oak_stairs",
        "minecraft:observer",
        "minecraft:wooden_door",
        "minecraft:sea_pickle",
        "minecraft:cactus",
        "minecraft:red_shrub",
    ] {
        assert!(
            predict(name, &store, mode, &context(&around)).is_none(),
            "{name}"
        );
    }
    around.neighbor_identifier = Some("minecraft:stone".to_owned());
    assert!(predict("minecraft:oak_log", &store, mode, &context(&around)).is_none());
}

#[test]
fn indexed_state_lookup_preserves_typed_values_in_both_identity_spaces() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for record in fixtures().0.iter().filter(|record| {
            ["minecraft:oak_log", "minecraft:oak_slab", "minecraft:torch"]
                .contains(&record.name.as_ref())
        }) {
            let states = serde_json::from_str(&record.canonical_state).unwrap();
            assert_eq!(
                fixtures()
                    .1
                    .block_state_runtime_id(mode, &record.name, &states),
                Some(id(record, mode))
            );
        }
    }
}

#[test]
fn bamboo_placement_tests_the_destination_column_at_custom_build_heights() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for y in [-8_i32, 24] {
            let mut store = ChunkStore::new();
            let key = SubChunkKey::new(0, 0, y.div_euclid(16), 0);
            store.mark_sub_chunk_loaded(key).unwrap();
            let air = id(record("minecraft:air", &[]), mode);
            store
                .update_block(
                    key,
                    BlockUpdate::new(
                        0,
                        y.rem_euclid(16) as u8,
                        0,
                        0,
                        id(record("minecraft:stone", &[]), mode),
                    ),
                    air,
                )
                .unwrap();
            for actor in [false, true] {
                for (x, blocked) in [(1.78, true), (1.38, false)] {
                    let mut around = surroundings();
                    let bounds = (
                        [x - 0.02, f64::from(y), 0.49],
                        [x + 0.02, f64::from(y) + 0.8, 0.51],
                    );
                    if actor {
                        around.actor_boxes.push(bounds);
                    } else {
                        around.player_box = bounds;
                    }
                    let mut click = context(&around);
                    click.clicked = [0, y, 0];
                    click.input.face = 5;
                    click.build_height = y - 1..y + 2;
                    let bamboo = id(record("minecraft:bamboo", &[]), mode);
                    let registry = fixtures().1.registry(mode);
                    let world = sim::PaletteWorld::new(&store, registry, 0);
                    let destination = around.destination(click.clicked, click.input.face).0;
                    assert!(click.build_height.contains(&destination[1]));
                    assert_eq!(world.primary_runtime_id(destination).unwrap(), air);
                    let clicked = click.clicked;
                    let face = click.input.face;
                    around.set_placed_collision_shapes(registry, bamboo, clicked, face);
                    let mut stack = protocol::NetworkItemStack::empty();
                    stack.network_id = 2;
                    stack.count = 1;
                    stack.block_runtime_id = bamboo as i32;
                    let digest = stack.nbt_digest;
                    let item = protocol::VerifiedNetworkItemStack::try_new(stack, digest).unwrap();
                    let caps = client_world::game_mode_capabilities::GameModeCapabilities::for_mode(
                        protocol::PlayerGameMode::Survival,
                    );
                    let result =
                        crate::block_use::LocalUse::resolve(&item, clicked, face, &around, &caps);
                    assert_eq!(
                        result == crate::block_use::LocalUse::Nothing,
                        blocked,
                        "destination=(1,{y},0), x={x}, actor={actor}, mode={mode:?}"
                    );
                }
            }
        }
    }
}
