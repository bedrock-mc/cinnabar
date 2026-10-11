use super::tests::bounds;
use super::tests::{fixture, record, runtime_id};
use assets::NetworkIdMode;

#[test]
fn passable_families_have_their_own_selection_bounds() {
    for (name, min, max) in [
        ("rail", [0.0; 3], [1.0, 0.0625, 1.0]),
        ("redstone_wire", [0.0; 3], [1.0, 0.0625, 1.0]),
        (
            "wooden_pressure_plate",
            [0.0625, 0.0, 0.0625],
            [0.9375, 0.0625, 0.9375],
        ),
        ("powder_snow", [0.0; 3], [1.0; 3]),
        ("structure_void", [0.0; 3], [1.0; 3]),
        ("frog_spawn", [0.0; 3], [1.0, 0.015625, 1.0]),
        ("hanging_roots", [0.15, 0.7, 0.15], [0.85, 1.0, 0.85]),
        ("spore_blossom", [0.15, 0.75, 0.15], [0.85, 1.0, 0.85]),
        ("kelp", [0.125, 0.0, 0.125], [0.875, 1.0, 0.875]),
        ("seagrass", [0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        ("sea_pickle", [0.0; 3], [1.0, 0.5, 1.0]),
        ("bamboo_sapling", [0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        ("small_dripleaf_block", [0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ("open_eyeblossom", [0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ("closed_eyeblossom", [0.15, 0.0, 0.15], [0.7, 0.6, 0.7]),
        ("cave_vines", [0.0; 3], [1.0; 3]),
        ("twisting_vines", [0.0; 3], [1.0; 3]),
        ("weeping_vines", [0.0; 3], [1.0; 3]),
        ("pale_hanging_moss", [0.0; 3], [1.0; 3]),
    ] {
        let record = record(&format!("minecraft:{name}"));
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let registry = fixture().registries.registry(mode);
            let id = runtime_id(record, mode);
            assert_eq!(
                registry.selection_shapes(id),
                Some([bounds(min, max)].as_slice()),
                "{name} {mode:?}"
            );
        }
    }
}
