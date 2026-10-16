use super::*;

#[test]
fn fence_gate_ray_targets_the_lower_gate_through_the_upper_cells_empty_space() {
    for record in fixture()
        .records
        .iter()
        .filter(|record| record.model_family == assets::ModelFamily::Gate)
    {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let id = runtime_id(record, mode);
            let mut store = store(mode, record);
            put(&mut store, [8, 9, 8], id, mode);
            let world = PaletteWorld::new(
                &store,
                fixture().registries.registry(mode),
                0,
                Default::default(),
            );
            let (origin, direction, face) = if orientation % 2 == 0 {
                (Vec3::new(8.5, 9.6, 7.5), Vec3::new(0.0, -1.0, 1.0), 2)
            } else {
                (Vec3::new(7.5, 9.6, 8.5), Vec3::new(1.0, -1.0, 0.0), 4)
            };
            let hit = world
                .block_interaction_ray_current(origin, direction, 4.0)
                .unwrap()
                .unwrap();
            assert_eq!(
                hit.block_pos,
                [8, 8, 8],
                "{} {} {mode:?}",
                record.name,
                record.canonical_state
            );
            assert_eq!(hit.runtime_id, id);
            assert_eq!(hit.face, face);
            let axis = if orientation % 2 == 0 { 2 } else { 0 };
            assert_eq!(hit.hit_local[axis], 0.375);
            let camera = world
                .camera_visibility_ray(origin, direction, 4.0)
                .unwrap()
                .unwrap();
            assert_eq!(camera.block_pos, hit.block_pos);
            assert_eq!(camera.hit_local, hit.hit_local);
            let outline = world.block_outline(hit.block_pos, id).unwrap().unwrap();
            assert_eq!(camera.selection_bounds, outline);
        }
    }
}
