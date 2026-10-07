use chunk_pipeline::WorldStream;
use client_world::BlockEntityKind;

use crate::observations::CollisionLookup;

pub(super) fn block_entity_light_position(
    stream: &WorldStream,
    collisions: Option<&dyn CollisionLookup>,
    kind: &BlockEntityKind,
    center: [f32; 3],
) -> [f32; 3] {
    let (Some(collisions), BlockEntityKind::Falling { block_runtime_id }) = (collisions, kind)
    else {
        return center;
    };
    let mode = stream.network_id_mode();
    let id = stream.resolve_block_network_id(u32::from_ne_bytes(block_runtime_id.to_ne_bytes()));
    let Some(falling_type) = collisions.block_identifier(mode, id) else {
        return center;
    };
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
    );
    falling_light_position(center, falling_type, |cell| {
        let id = world.primary_runtime_id(cell).ok()?;
        collisions.block_identifier(mode, id)
    })
}

fn falling_light_position<'a>(
    center: [f32; 3],
    falling_type: &str,
    mut block_type: impl FnMut([i32; 3]) -> Option<&'a str>,
) -> [f32; 3] {
    if !center.into_iter().all(f32::is_finite) {
        return center;
    }
    let mut cell = center.map(|coordinate| coordinate.floor() as i32);
    let mut position = center;
    while block_type(cell) == Some(falling_type) {
        let Some(y) = cell[1].checked_add(1) else {
            break;
        };
        cell[1] = y;
        position[1] = y as f32;
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falling_light_skips_same_type_states_and_samples_the_first_clear_cell() {
        let center = [-1.25, 64.75, -3.125];
        let mut visited = Vec::new();
        let position = falling_light_position(center, "fixture:sand", |cell| {
            visited.push(cell);
            match cell {
                [-2, 64, -4] => Some("fixture:sand"),
                [-2, 65, -4] => Some("fixture:sand"),
                [-2, 66, -4] => Some("fixture:air"),
                _ => None,
            }
        });
        let light = if position[1].floor() == 66.0 {
            (0, 15)
        } else {
            (0, 0)
        };
        assert_eq!(
            light,
            (0, 15),
            "opaque source cells must not black out the actor"
        );
        assert_eq!(visited, [[-2, 64, -4], [-2, 65, -4], [-2, 66, -4]]);
        assert_eq!([position[0], position[2]], [center[0], center[2]]);
    }

    #[test]
    fn falling_light_stops_at_a_different_type_or_missing_data() {
        for final_type in [Some("fixture:gravel"), None] {
            let position = falling_light_position([1.5, 8.25, 2.5], "fixture:sand", |cell| {
                if cell[1] == 8 {
                    Some("fixture:sand")
                } else {
                    final_type
                }
            });
            assert_eq!(position[1].floor(), 9.0);
        }
    }

    #[test]
    fn falling_light_keeps_an_already_clear_or_invalid_probe() {
        let center = [1.5, 8.25, 2.5];
        assert_eq!(
            falling_light_position(center, "fixture:sand", |_| Some("fixture:air")),
            center
        );
        let position = falling_light_position([f32::NAN, 1.0, 2.0], "fixture:sand", |_| {
            panic!("a non-finite probe must not query terrain")
        });
        assert!(position[0].is_nan());
    }

    #[test]
    fn falling_light_cannot_overflow_the_terrain_cell_range() {
        let center = [0.0, i32::MAX as f32, 0.0];
        let mut queried = false;
        let position = falling_light_position(center, "fixture:sand", |_| {
            assert!(!queried, "the highest cell cannot advance to another cell");
            queried = true;
            Some("fixture:sand")
        });
        assert_eq!(position, center);
    }

    #[test]
    fn tnt_light_keeps_its_center_without_falling_type_queries() {
        struct UnusedCollisionLookup;
        impl CollisionLookup for UnusedCollisionLookup {
            fn registry(&self, _: assets::NetworkIdMode) -> &sim::CollisionRegistry {
                panic!("TNT does not use a falling-type terrain probe")
            }
            fn block_canonical_state(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
                panic!("TNT does not query a block state")
            }
            fn block_identifier(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
                panic!("TNT does not query a falling block type")
            }
        }
        let stream = WorldStream::new(protocol::WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        let kind = BlockEntityKind::PrimedTnt {
            visual: assets::ItemVisualRoute::BlockItem(assets::BlockVisualId(5)),
        };
        let center = [1.5, 8.25, 2.5];
        assert_eq!(
            block_entity_light_position(&stream, Some(&UnusedCollisionLookup), &kind, center),
            center
        );
    }
}
