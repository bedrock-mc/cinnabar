use std::{fs, path::PathBuf};

#[test]
fn visual_dispatcher_only_orders_family_rules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/compiler/visuals");
    let dispatcher =
        fs::read_to_string(root.join("dispatcher.rs")).expect("read visual dispatcher source");

    for leaked in [
        "descriptor_for(",
        "sign_state(",
        "door_bounds(",
        "pane_quads(",
        "fence_arm_quads(",
        "wall_quads(",
        "pressure_plate_quads(",
        "button_quads(",
        "carpet_state(",
        "gate_quads(",
        "slab_quads(",
        "stair_quads(",
        "kelp_quads(",
        "crossed_quads(",
    ] {
        assert!(
            !dispatcher.contains(leaked),
            "family-specific operation `{leaked}` leaked into dispatcher"
        );
    }

    for family in [
        "signs.rs",
        "doors.rs",
        "panes.rs",
        "fences.rs",
        "walls.rs",
        "pressure_plates.rs",
        "button.rs",
        "carpets.rs",
        "gates.rs",
        "slabs.rs",
        "stairs.rs",
        "kelp.rs",
        "cross.rs",
        "cube.rs",
    ] {
        let source = fs::read_to_string(root.join(family)).expect("read family source");
        assert!(
            source.contains("compile_rule"),
            "visual family `{family}` does not own a compile_rule entrypoint"
        );
    }
}
