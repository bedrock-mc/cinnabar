use super::{compile, fixture, record};
use assets::block_shapes::ShapeBox;

#[test]
fn attachment_and_growth_variants_keep_their_class_visual_contracts() {
    let cases = [
        ("rail", r#"{"rail_direction":2}"#, [0.0; 3], [1.0, 0.5, 1.0]),
        (
            "stone_pressure_plate",
            r#"{"redstone_signal":1}"#,
            [0.0625, 0.0, 0.0625],
            [0.9375, 0.03125, 0.9375],
        ),
        (
            "stone_button",
            r#"{"facing_direction":2,"button_pressed_bit":0}"#,
            [0.3125, 0.375, 0.875],
            [0.6875, 0.625, 1.0],
        ),
        (
            "stone_button",
            r#"{"facing_direction":2,"button_pressed_bit":1}"#,
            [0.3125, 0.375, 0.9375],
            [0.6875, 0.625, 1.0],
        ),
        (
            "lever",
            r#"{"lever_direction":"east"}"#,
            [0.0, 0.25, 0.3125],
            [0.375, 0.75, 0.6875],
        ),
        (
            "trip_wire",
            r#"{"attached_bit":1,"suspended_bit":1}"#,
            [0.0, 0.0625, 0.0],
            [1.0, 0.15625, 1.0],
        ),
        (
            "tripwire_hook",
            r#"{"direction":0}"#,
            [0.375, 0.0625, 0.0],
            [0.625, 0.5625, 0.375],
        ),
        (
            "ladder",
            r#"{"facing_direction":2}"#,
            [0.0, 0.0, 0.8125],
            [1.0; 3],
        ),
        (
            "skeleton_skull",
            r#"{"facing_direction":4}"#,
            [0.5, 0.25, 0.25],
            [1.0, 0.75, 0.75],
        ),
        (
            "frame",
            r#"{"facing_direction":3,"item_frame_map_bit":1,"item_frame_photo_bit":0}"#,
            [0.0; 3],
            [1.0, 1.0, 0.0625],
        ),
        (
            "mangrove_propagule",
            r#"{"hanging":1,"propagule_stage":0}"#,
            [0.375, 0.75, 0.375],
            [0.625, 1.0, 0.625],
        ),
        (
            "mangrove_propagule",
            r#"{"hanging":1,"propagule_stage":4}"#,
            [0.375, 0.0, 0.375],
            [0.625, 1.0, 0.625],
        ),
        (
            "pumpkin_stem",
            r#"{"facing_direction":0,"growth":3}"#,
            [0.375, 0.0, 0.375],
            [0.625, 0.5, 0.625],
        ),
        (
            "pumpkin_stem",
            r#"{"facing_direction":4,"growth":7}"#,
            [0.0, 0.0, 0.375],
            [0.625, 0.25, 0.625],
        ),
        (
            "pitcher_crop",
            r#"{"growth":0,"upper_block_bit":0}"#,
            [0.3125, -0.0625, 0.3125],
            [0.6875, 0.1875, 0.6875],
        ),
        (
            "pitcher_crop",
            r#"{"growth":3,"upper_block_bit":1}"#,
            [0.1875, 0.0, 0.1875],
            [0.8125, 0.6875, 0.8125],
        ),
        (
            "portal",
            r#"{"portal_axis":"x"}"#,
            [0.0, 0.0, 0.375],
            [1.0, 1.0, 0.625],
        ),
    ];
    for (name, overrides, min, max) in cases {
        let mut record = record(&format!("minecraft:{name}")).clone();
        let mut state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let overrides: serde_json::Value = serde_json::from_str(overrides).unwrap();
        for (key, value) in overrides.as_object().unwrap() {
            state[key]["value"] = value.clone();
        }
        record.canonical_state = state.to_string().into();
        let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
        let output = compile(&record, input);
        assert_eq!(
            output.shapes.unwrap().selection.as_ref(),
            [ShapeBox::new(min.map(f64::from), max.map(f64::from))],
            "{name} {overrides}"
        );
        assert_eq!(
            output.boxes, input.boxes,
            "selection must not change movement for {name}"
        );
    }
}
