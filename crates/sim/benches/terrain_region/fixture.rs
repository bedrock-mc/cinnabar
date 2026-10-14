//! Mixed terrain over a 3x3 column patch with one absent column: stone and ore, dirt, then a
//! surface of fences (some waterlogged), walls, stairs, water, lava, doors and offset plants.

use block_transform::random_offset::BAMBOO;
use sim::{
    Aabb, BlockPhysicsFlags, CollisionRegistry, DoorFacing, DoorState, SurfaceResponse, Vec3,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

pub const AIR: u32 = 0;
const STONE: u32 = 1;
const FENCE: u32 = 2;
const WALL: u32 = 3;
const STAIRS: u32 = 4;
const WATER: u32 = 5;
const LAVA: u32 = 6;
const DOOR_LOWER: u32 = 7;
const DOOR_UPPER: u32 = 8;
const GRASS: u32 = 9;
const ORE: u32 = 10;
const DIRT: u32 = 11;
/// Columns beside the origin, except this one, carry authoritative terrain.
pub const ABSENT_COLUMN: (i32, i32) = (1, 1);
const SURFACE: i32 = 64;

/// Identifier, canonical state and tags for one fixture runtime ID.
pub fn metadata(runtime_id: u32) -> (&'static str, &'static str, &'static [&'static str]) {
    match runtime_id {
        AIR => ("minecraft:air", "{}", &[]),
        STONE => (
            "minecraft:stone",
            "{}",
            &["minecraft:is_pickaxe_item_destructible", "stone"],
        ),
        FENCE => (
            "minecraft:oak_fence",
            "{}",
            &["minecraft:is_axe_item_destructible", "wood", "fence"],
        ),
        WALL => (
            "minecraft:cobblestone_wall",
            r#"{"wall_connection_type_east":{"type":"string","value":"short"},"wall_connection_type_north":{"type":"string","value":"none"},"wall_connection_type_south":{"type":"string","value":"none"},"wall_connection_type_west":{"type":"string","value":"short"},"wall_post_bit":{"type":"byte","value":1}}"#,
            &["minecraft:is_pickaxe_item_destructible", "stone", "wall"],
        ),
        STAIRS => (
            "minecraft:oak_stairs",
            r#"{"upside_down_bit":{"type":"byte","value":0},"weirdo_direction":{"type":"int","value":2}}"#,
            &["minecraft:is_axe_item_destructible", "wood", "stairs"],
        ),
        WATER => (
            "minecraft:water",
            r#"{"liquid_depth":{"type":"int","value":0}}"#,
            &["water"],
        ),
        LAVA => (
            "minecraft:lava",
            r#"{"liquid_depth":{"type":"int","value":0}}"#,
            &["lava"],
        ),
        DOOR_LOWER | DOOR_UPPER => (
            "minecraft:wooden_door",
            if runtime_id == DOOR_LOWER {
                r#"{"door_hinge_bit":{"type":"byte","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"},"open_bit":{"type":"byte","value":0},"upper_block_bit":{"type":"byte","value":0}}"#
            } else {
                r#"{"door_hinge_bit":{"type":"byte","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"},"open_bit":{"type":"byte","value":0},"upper_block_bit":{"type":"byte","value":1}}"#
            },
            &["minecraft:is_axe_item_destructible", "wood", "door"],
        ),
        GRASS => (
            "minecraft:short_grass",
            "{}",
            &["minecraft:is_shears_item_destructible", "plant"],
        ),
        ORE => (
            "minecraft:iron_ore",
            "{}",
            &["minecraft:is_pickaxe_item_destructible", "stone", "ore"],
        ),
        DIRT => (
            "minecraft:dirt",
            r#"{"dirt_type":{"type":"string","value":"normal"}}"#,
            &["minecraft:is_shovel_item_destructible", "dirt"],
        ),
        _ => unreachable!("fixture runtime ID"),
    }
}

fn shape(min: [f64; 3], max: [f64; 3]) -> Aabb {
    Aabb::new(
        Vec3::new(min[0], min[1], min[2]),
        Vec3::new(max[0], max[1], max[2]),
    )
}

/// Registers every fixture state with representative native shapes and physics.
pub fn registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::new();
    let cube = shape([0.0; 3], [1.0; 3]);
    let none = BlockPhysicsFlags::default();
    for (id, boxes) in [
        (AIR, vec![]),
        (STONE, vec![cube]),
        (ORE, vec![cube]),
        (DIRT, vec![cube]),
        (FENCE, vec![shape([0.375, 0.0, 0.375], [0.625, 1.5, 0.625])]),
        (WALL, vec![shape([0.25, 0.0, 0.25], [0.75, 1.5, 0.75])]),
        (
            STAIRS,
            vec![
                shape([0.0; 3], [1.0, 0.5, 1.0]),
                shape([0.0, 0.5, 0.5], [1.0, 1.0, 1.0]),
            ],
        ),
        (DOOR_LOWER, vec![cube]),
        (DOOR_UPPER, vec![cube]),
        (GRASS, vec![]),
    ] {
        registry
            .register_physics(id, boxes, 0.6, 1.0, 1.0, 0.0, none, SurfaceResponse::None)
            .unwrap();
    }
    for (id, flags) in [
        (WATER, BlockPhysicsFlags::WATER),
        (LAVA, BlockPhysicsFlags::LAVA),
    ] {
        registry
            .register_physics(
                id,
                [],
                0.6,
                1.0,
                1.0,
                0.888_888_9,
                flags,
                SurfaceResponse::None,
            )
            .unwrap();
    }
    for (id, upper) in [(DOOR_LOWER, false), (DOOR_UPPER, true)] {
        registry.set_door_state(
            id,
            DoorState {
                family: [7; 32],
                facing: DoorFacing::South,
                upper,
                open: false,
                hinge_right: false,
            },
        );
    }
    registry.set_pick_shapes(GRASS, [shape([0.1, 0.0, 0.1], [0.9, 0.8, 0.9])]);
    assert!(registry.set_random_offset(GRASS, BAMBOO));
    registry
}

fn mix(x: i32, y: i32, z: i32) -> u32 {
    let mut value = (x as u32).wrapping_mul(0x9e37_79b9)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ (z as u32).wrapping_mul(0xc2b2_ae35);
    value ^= value >> 15;
    value.wrapping_mul(0x2c1b_3c6d) >> 7
}

/// Primary and optional second layer at one block.
fn block(x: i32, y: i32, z: i32) -> (u32, Option<u32>) {
    let surface = mix(x, 0, z);
    match y {
        _ if y < SURFACE - 2 => (
            if mix(x, y, z).is_multiple_of(17) {
                ORE
            } else {
                STONE
            },
            None,
        ),
        _ if y < SURFACE => (DIRT, None),
        SURFACE => match surface % 10 {
            0 => (FENCE, surface.is_multiple_of(20).then_some(WATER)),
            1 => (WALL, None),
            2 => (STAIRS, None),
            3 => (WATER, None),
            4 => (LAVA, None),
            5 => (DOOR_LOWER, None),
            6 => (GRASS, None),
            _ => (AIR, None),
        },
        _ if y == SURFACE + 1 && surface % 10 == 5 => (DOOR_UPPER, None),
        _ => (AIR, None),
    }
}

/// Fills sections 2..=5 of every present column around the origin.
pub fn store() -> ChunkStore {
    let mut store = ChunkStore::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            if (cx, cz) == ABSENT_COLUMN {
                continue;
            }
            store.mark_chunk_loaded(ChunkKey::new(0, cx, cz)).unwrap();
            for cy in 2..=5 {
                let mut updates = Vec::new();
                for x in 0..16_u8 {
                    for y in 0..16_u8 {
                        for z in 0..16_u8 {
                            let world = [
                                cx * 16 + i32::from(x),
                                cy * 16 + i32::from(y),
                                cz * 16 + i32::from(z),
                            ];
                            let (primary, second) = block(world[0], world[1], world[2]);
                            if primary != AIR {
                                updates.push(BlockUpdate::new(x, y, z, 0, primary));
                            }
                            if let Some(second) = second {
                                updates.push(BlockUpdate::new(x, y, z, 1, second));
                            }
                        }
                    }
                }
                store
                    .update_sub_chunk_blocks(SubChunkKey::new(0, cx, cy, cz), &updates, AIR)
                    .unwrap();
            }
        }
    }
    store
}
