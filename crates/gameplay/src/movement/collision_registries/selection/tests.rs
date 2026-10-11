use std::sync::OnceLock;

use assets::{ModelStateField, NetworkIdMode, RegistryRecord, TOP_SNOW_LAYER_COUNT};
use sim::{Aabb, PaletteWorld, Vec3};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

use super::{bounds, shape};
use crate::movement::PhysicsCollisionRegistries;

#[path = "gateway_tests.rs"]
mod gateway_tests;

struct Fixture {
    records: Box<[RegistryRecord]>,
    registries: PhysicsCollisionRegistries,
}

/// Binds the pinned block and physics carriers once for selection tests.
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let breg = include_bytes!("../../../../../assets/data/block-registry-v2193.bin");
        let preg = include_bytes!("../../../../../assets/data/block-physics-v2193.bin");
        let protocol = u32::try_from(
            serde_json::from_str::<serde_json::Value>(include_str!(
                "../../../../../../assets/bedrock-target.json"
            ))
            .unwrap()["wire_protocol"]
                .as_u64()
                .unwrap(),
        )
        .unwrap();
        let records = assets::read_registry_for_protocol(breg, protocol).unwrap();
        let registries = PhysicsCollisionRegistries::from_assets(breg, &records, preg, protocol)
            .expect("the pinned registries must bind");
        Fixture {
            records,
            registries,
        }
    })
}

/// Selects the runtime identifier used by the requested palette mode.
fn runtime_id(record: &RegistryRecord, mode: NetworkIdMode) -> u32 {
    match mode {
        NetworkIdMode::Sequential => record.sequential_id,
        NetworkIdMode::Hashed => record.network_hash,
    }
}

/// Finds a named vanilla block in the pinned registry.
fn record(name: &str) -> &'static RegistryRecord {
    fixture()
        .records
        .iter()
        .find(|record| record.name.as_ref() == name)
        .expect("named block must exist in the active registry")
}

/// Places a fixture block using the requested palette mode.
fn put(store: &mut ChunkStore, pos: [u8; 3], id: u32, mode: NetworkIdMode) {
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(pos[0], pos[1], pos[2], 0, id),
            runtime_id(record("minecraft:air"), mode),
        )
        .unwrap();
}

/// Loads a small fixture subchunk with support below the target block.
fn store(mode: NetworkIdMode, target: &RegistryRecord) -> ChunkStore {
    let mut store = ChunkStore::new();
    // The short test rays remain in this loaded subchunk, away from its halo.
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.apply_request_mode_air(key).unwrap();
    store.mark_sub_chunk_loaded(key).unwrap();
    put(
        &mut store,
        [8, 7, 8],
        runtime_id(record("minecraft:stone"), mode),
        mode,
    );
    put(&mut store, [8, 8, 8], runtime_id(target, mode), mode);
    store
}

#[test]
fn every_snow_state_has_native_selection_height_in_both_id_spaces() {
    let fixture = fixture();
    let records: Vec<_> = fixture
        .records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:snow_layer")
        .collect();
    assert_eq!(
        records.len(),
        usize::from(TOP_SNOW_LAYER_COUNT) * 2,
        "covered/uncovered heights must all bind"
    );
    for record in records {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let height = state["height"]["value"].as_u64().unwrap();
        let expected = Aabb::new(
            Vec3::ZERO,
            Vec3::new(
                1.0,
                (height + 1) as f64 / f64::from(TOP_SNOW_LAYER_COUNT),
                1.0,
            ),
        );
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture.registries.registry(mode);
            let id = runtime_id(record, mode);
            assert_eq!(registry.selection_shapes(id), Some([expected].as_slice()));
            let store = store(mode, record);
            let hit = PaletteWorld::new(&store, registry, 0)
                .block_interaction_ray_current(
                    Vec3::new(8.5, 10.0, 8.5),
                    Vec3::new(0.0, -1.0, 0.0),
                    4.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(hit.runtime_id, id, "pick must stop on snow, not support");
            assert_eq!(hit.block_pos, [8, 8, 8]);
            assert_eq!(hit.face, 1);
            assert_eq!(hit.hit_local.y, expected.max.y);
            let side = PaletteWorld::new(&store, registry, 0)
                .block_interaction_ray_current(
                    Vec3::new(8.5, 8.0 + expected.max.y / 2.0, 6.5),
                    Vec3::new(0.0, 0.0, 1.0),
                    3.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                side.runtime_id, id,
                "even the thinnest snow has a side pick"
            );
            assert_eq!(side.face, 2);
        }
    }
}

#[test]
fn flowers_grass_and_mushrooms_are_pickable_but_remain_passable() {
    let fixture = fixture();
    for (name, expected) in [
        (
            "minecraft:short_grass",
            bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        ),
        ("minecraft:fern", bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9])),
        (
            "minecraft:deadbush",
            bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        ),
        ("minecraft:bush", bounds([0.0; 3], [1.0, 0.8, 1.0])),
        (
            "minecraft:brown_mushroom",
            bounds([0.3, 0.0, 0.3], [0.7, 0.4, 0.7]),
        ),
        (
            "minecraft:red_mushroom",
            bounds([0.3, 0.0, 0.3], [0.7, 0.4, 0.7]),
        ),
        (
            "minecraft:poppy",
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ),
        (
            "minecraft:dandelion",
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ),
        (
            "minecraft:orange_tulip",
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ),
        (
            "minecraft:pink_tulip",
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ),
        (
            "minecraft:red_tulip",
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ),
        (
            "minecraft:white_tulip",
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ),
    ] {
        let record = record(name);
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture.registries.registry(mode);
            let id = runtime_id(record, mode);
            assert_eq!(registry.selection_shapes(id), Some([expected].as_slice()));
            assert!(
                registry.collision_shapes(id).unwrap().is_empty(),
                "{name} must not acquire a movement collider"
            );
            let store = store(mode, record);
            let world = PaletteWorld::new(&store, registry, 0);
            let hit = world
                .block_interaction_ray_current(
                    Vec3::new(8.5, 10.0, 8.5),
                    Vec3::new(0.0, -1.0, 0.0),
                    4.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(hit.runtime_id, id, "must select {name} above support");
            assert_eq!(hit.block_pos, [8, 8, 8]);
            assert_eq!(hit.face, 1);
            // Native world-space f32 translation rounds the local fraction;
            // compare against that translated face, not the pre-translation one.
            let expected_y = expected.translated(Vec3::new(8.0, 8.0, 8.0)).max.y - 8.0;
            assert_eq!(hit.hit_local.y, expected_y, "native top face for {name}");
            let edge = world
                .block_interaction_ray_current(
                    Vec3::new(8.01, 10.0, 8.01),
                    Vec3::new(0.0, -1.0, 0.0),
                    4.0,
                )
                .unwrap()
                .unwrap();
            let edge_is_inside = expected.min.x < 0.01
                && expected.min.z < 0.01
                && expected.max.x > 0.01
                && expected.max.z > 0.01;
            assert_eq!(
                edge.block_pos,
                if edge_is_inside { [8, 8, 8] } else { [8, 7, 8] },
                "{name}: only rays outside the native outline hit support"
            );
        }
    }
}

#[test]
fn cobweb_and_all_signs_are_pickable_independently_of_movement_colliders() {
    let fixture = fixture();
    let records: Vec<_> = fixture
        .records
        .iter()
        .filter(|record| {
            record.name.as_ref() == "minecraft:web"
                || record.name.ends_with("standing_sign")
                || record.name.ends_with("wall_sign")
                || record.name.ends_with("hanging_sign")
        })
        .collect();
    assert!(records.len() > 20);
    for record in records {
        let expected = shape(record).expect("the pinned selectable block has visual bounds");
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture.registries.registry(mode);
            let id = runtime_id(record, mode);
            if !record.name.ends_with("hanging_sign") {
                assert!(
                    registry.collision_shapes(id).unwrap().is_empty(),
                    "{} is selectable without a movement collider",
                    record.name
                );
            }
            let store = store(mode, record);
            let world = PaletteWorld::new(&store, registry, 0);
            let center = (expected.min + expected.max) * 0.5;
            let hit = world
                .block_interaction_ray_current(
                    Vec3::new(8.0 + center.x, 10.0, 8.0 + center.z),
                    Vec3::new(0.0, -1.0, 0.0),
                    4.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                hit.runtime_id, id,
                "{} must stop the interaction ray above its support",
                record.name
            );
            assert_eq!(hit.block_pos, [8, 8, 8]);
            assert_eq!(hit.face, 1);
            assert_eq!(hit.hit_local.y, expected.max.y);
        }
    }
}

#[test]
fn banners_are_pickable_without_movement_colliders() {
    let fixture = fixture();
    for (name, facing, expected) in [
        (
            "minecraft:standing_banner",
            None,
            bounds([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]),
        ),
        (
            "minecraft:wall_banner",
            Some(2),
            bounds([0.0, 0.0, 0.875], [1.0, 0.78125, 1.0]),
        ),
        (
            "minecraft:wall_banner",
            Some(3),
            bounds([0.0, 0.0, 0.0], [1.0, 0.78125, 0.125]),
        ),
        (
            "minecraft:wall_banner",
            Some(4),
            bounds([0.875, 0.0, 0.0], [1.0, 0.78125, 1.0]),
        ),
        (
            "minecraft:wall_banner",
            Some(5),
            bounds([0.0, 0.0, 0.0], [0.125, 0.78125, 1.0]),
        ),
    ] {
        let records: Vec<_> = fixture
            .records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .filter(|record| {
                facing.is_none_or(|facing| {
                    let state: serde_json::Value =
                        serde_json::from_str(&record.canonical_state).unwrap();
                    state["facing_direction"]["value"].as_u64() == Some(facing)
                })
            })
            .collect();
        assert!(!records.is_empty(), "{name} {facing:?} must exist");
        for record in records {
            for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
                let registry = fixture.registries.registry(mode);
                let id = runtime_id(record, mode);
                assert_eq!(registry.selection_shapes(id), Some([expected].as_slice()));
                assert!(
                    registry.collision_shapes(id).unwrap().is_empty(),
                    "{name} must not acquire a movement collider"
                );
                let store = store(mode, record);
                let center = (expected.min + expected.max) * 0.5;
                let hit = PaletteWorld::new(&store, registry, 0)
                    .block_interaction_ray_current(
                        Vec3::new(8.0 + center.x, 10.0, 8.0 + center.z),
                        Vec3::new(0.0, -1.0, 0.0),
                        4.0,
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(hit.runtime_id, id, "{name} must stop the interaction ray");
                assert_eq!(hit.block_pos, [8, 8, 8]);
                assert_eq!(hit.face, 1);
            }
        }
    }
}

#[test]
fn every_reviewed_foliage_route_binds_selection_without_movement_changes() {
    let fixture = fixture();
    for record in &fixture.records {
        let Some(expected) = shape(record) else {
            continue;
        };
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture.registries.registry(mode);
            assert_eq!(
                registry.selection_shapes(runtime_id(record, mode)),
                Some([expected].as_slice()),
                "{} {} {mode:?}",
                record.name,
                record.canonical_state
            );
        }
    }
    for name in ["minecraft:air", "minecraft:water", "minecraft:stone"] {
        assert!(
            shape(record(name)).is_none(),
            "no blanket passable-block picks"
        );
    }
}

#[test]
fn invalid_snow_state_does_not_create_unbounded_pick_geometry() {
    let mut snow = record("minecraft:snow_layer").clone();
    for height in [
        serde_json::Value::from(-1),
        serde_json::Value::from(TOP_SNOW_LAYER_COUNT),
        serde_json::Value::from(u64::MAX),
        serde_json::Value::Null,
    ] {
        snow.canonical_state = serde_json::json!({"height": {"type": "int", "value": height}})
            .to_string()
            .into();
        assert!(shape(&snow).is_none());
    }
}

#[test]
fn covered_snow_and_foliage_keep_separate_selection_bounds_in_both_layer_orders() {
    let fixture = fixture();
    let snow = fixture
        .records
        .iter()
        .find(|record| {
            if record.name.as_ref() != "minecraft:snow_layer" {
                return false;
            }
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            state["height"]["value"] == 0 && state["covered_bit"]["value"] == 1
        })
        .unwrap();
    let grass = record("minecraft:short_grass");
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let registry = fixture.registries.registry(mode);
        for (primary, secondary) in [(snow, grass), (grass, snow)] {
            let mut store = store(mode, primary);
            store
                .update_block(
                    SubChunkKey::new(0, 0, 0, 0),
                    BlockUpdate::new(8, 8, 8, 1, runtime_id(secondary, mode)),
                    runtime_id(record("minecraft:air"), mode),
                )
                .unwrap();
            let world = PaletteWorld::new(&store, registry, 0);
            for (x, z, target) in [(8.5, 8.5, grass), (8.01, 8.01, snow)] {
                let hit = world
                    .block_interaction_ray_current(
                        Vec3::new(x, 10.0, z),
                        Vec3::new(0.0, -1.0, 0.0),
                        4.0,
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(hit.runtime_id, runtime_id(target, mode));
                assert_eq!(hit.block_pos, [8, 8, 8]);
            }
        }
    }
}

#[test]
fn barrier_selection_is_creative_only_without_removing_collision_or_the_pick() {
    use protocol::PlayerGameMode;
    let fixture = fixture();
    let barrier = record("minecraft:barrier");
    let stone = record("minecraft:stone");
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let id = runtime_id(barrier, mode);
        let registry = fixture.registries.registry(mode);
        assert!(!registry.collision_shapes(id).unwrap().is_empty());
        let store = store(mode, barrier);
        let world = PaletteWorld::new(&store, registry, 0);
        let hit = world
            .block_interaction_ray_current(
                Vec3::new(8.5, 10.0, 8.5),
                Vec3::new(0.0, -1.0, 0.0),
                4.0,
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.runtime_id, id);
        for game_mode in [
            None,
            Some(PlayerGameMode::Unknown),
            Some(PlayerGameMode::Survival),
            Some(PlayerGameMode::Adventure),
            Some(PlayerGameMode::Creative),
        ] {
            assert_eq!(
                fixture
                    .registries
                    .selection_overlay_visible(mode, hit.runtime_id, game_mode),
                game_mode == Some(PlayerGameMode::Creative)
            );
            assert!(fixture.registries.selection_overlay_visible(
                mode,
                runtime_id(stone, mode),
                game_mode
            ));
        }
    }
}

fn wall_record(connections: u32) -> &'static RegistryRecord {
    fixture()
        .records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:cobblestone_wall"
                && record.model_state.get(ModelStateField::Connections) == Some(connections)
        })
        .expect("wall connection state must exist in the pinned registry")
}

#[test]
fn wall_and_fence_picks_stop_at_the_visible_post_without_shortening_collision() {
    let fixture = fixture();
    let fence = fixture
        .records
        .iter()
        .find(|record| {
            if record.name.as_ref() != "minecraft:oak_fence" {
                return false;
            }
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            ["north", "east", "south", "west"]
                .into_iter()
                .all(|direction| state[format!("minecraft:connection_{direction}")]["value"] == 0)
        })
        .expect("isolated fence must exist in the pinned registry");
    for (record, inset) in [(wall_record(0x100), 0.25), (fence, 0.375)] {
        let expected = bounds([inset, 0.0, inset], [1.0 - inset, 1.0, 1.0 - inset]);
        let collider = bounds([inset, 0.0, inset], [1.0 - inset, 1.5, 1.0 - inset]);
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture.registries.registry(mode);
            let id = runtime_id(record, mode);
            assert_eq!(registry.selection_shapes(id), Some([expected].as_slice()));
            assert_eq!(registry.collision_shapes(id), Some([collider].as_slice()));
            let store = store(mode, record);
            let world = PaletteWorld::new(&store, registry, 0);
            assert!(
                world
                    .block_interaction_ray_current(
                        Vec3::new(8.5, 9.25, 6.5),
                        Vec3::new(0.0, 0.0, 1.0),
                        3.0,
                    )
                    .unwrap()
                    .is_none(),
                "{} must not pick the collision extension above the model",
                record.name
            );
            let side = world
                .block_interaction_ray_current(
                    Vec3::new(8.5, 8.5, 6.5),
                    Vec3::new(0.0, 0.0, 1.0),
                    3.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(side.runtime_id, id);
            assert_eq!(side.block_pos, [8, 8, 8]);
            let top = world
                .block_interaction_ray_current(
                    Vec3::new(8.5, 10.0, 8.5),
                    Vec3::new(0.0, -1.0, 0.0),
                    3.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(top.runtime_id, id);
            assert_eq!(top.hit_local.y, expected.max.y);
        }
    }
}

#[test]
fn wall_straights_follow_the_visible_arm_width_and_height_in_both_id_spaces() {
    for (connections, min, max) in [
        (0x11, [0.3125, 0.0, 0.0], [0.6875, 0.875, 1.0]),
        (0x44, [0.0, 0.0, 0.3125], [1.0, 0.875, 0.6875]),
        (0x22, [0.3125, 0.0, 0.0], [0.6875, 1.0, 1.0]),
        (0x88, [0.0, 0.0, 0.3125], [1.0, 1.0, 0.6875]),
        (0x111, [0.25, 0.0, 0.0], [0.75, 1.0, 1.0]),
    ] {
        let record = wall_record(connections);
        let expected = bounds(min, max);
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture().registries.registry(mode);
            let id = runtime_id(record, mode);
            assert_eq!(registry.selection_shapes(id), Some([expected].as_slice()));
            assert!(
                registry
                    .collision_shapes(id)
                    .unwrap()
                    .iter()
                    .all(|collider| collider.max.y == 1.5)
            );
            let store = store(mode, record);
            let world = PaletteWorld::new(&store, registry, 0);
            let top = world
                .block_interaction_ray_current(
                    Vec3::new(8.5, 10.0, 8.5),
                    Vec3::new(0.0, -1.0, 0.0),
                    3.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(top.runtime_id, id);
            assert_eq!(top.hit_local.y, expected.max.y);
            assert!(
                world
                    .block_interaction_ray_current(
                        Vec3::new(8.5, 8.0 + expected.max.y + 0.0625, 6.5),
                        Vec3::new(0.0, 0.0, 1.0),
                        3.0,
                    )
                    .unwrap()
                    .is_none()
            );
        }
    }
}
