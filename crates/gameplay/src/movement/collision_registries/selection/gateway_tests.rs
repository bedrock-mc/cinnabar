use super::*;

#[test]
fn gateway_faces_are_interaction_targets_without_movement_collision_or_outline() {
    let fixture = fixture();
    let gateway = record("minecraft:end_gateway");
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let registry = fixture.registries.registry(mode);
        let id = runtime_id(gateway, mode);
        assert!(registry.collision_shapes(id).unwrap().is_empty());
        let mut store = store(mode, gateway);
        put(
            &mut store,
            [8, 7, 8],
            runtime_id(record("minecraft:air"), mode),
            mode,
        );
        let world = PaletteWorld::new(&store, registry, 0);
        for (origin, direction, face) in [
            ([8.5, 10.0, 8.5], [0.0, -1.0, 0.0], 1),
            ([8.5, 6.0, 8.5], [0.0, 1.0, 0.0], 0),
            ([8.5, 8.5, 6.0], [0.0, 0.0, 1.0], 2),
            ([8.5, 8.5, 11.0], [0.0, 0.0, -1.0], 3),
            ([6.0, 8.5, 8.5], [1.0, 0.0, 0.0], 4),
            ([11.0, 8.5, 8.5], [-1.0, 0.0, 0.0], 5),
        ] {
            let hit = world
                .block_interaction_ray_current(
                    Vec3::new(origin[0], origin[1], origin[2]),
                    Vec3::new(direction[0], direction[1], direction[2]),
                    4.0,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                (hit.runtime_id, hit.block_pos, hit.face),
                (id, [8, 8, 8], face)
            );
        }
        for game_mode in [
            protocol::PlayerGameMode::Creative,
            protocol::PlayerGameMode::Survival,
        ] {
            assert!(
                !fixture
                    .registries
                    .selection_overlay_visible(mode, id, Some(game_mode))
            );
        }
    }
}
