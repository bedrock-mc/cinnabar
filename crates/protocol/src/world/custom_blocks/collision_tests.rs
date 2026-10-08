use super::{CustomBox, Nbt, parse_definition};

fn component(boxes: Vec<Nbt>) -> Nbt {
    Nbt::Compound(vec![
        ("enabled".into(), Nbt::Byte(1)),
        ("boxes".into(), Nbt::List(boxes)),
    ])
}

fn native_box(min: [f64; 3], max: [f64; 3]) -> Nbt {
    Nbt::Compound(
        ["minX", "minY", "minZ", "maxX", "maxY", "maxZ"]
            .into_iter()
            .zip(min.into_iter().chain(max))
            .map(|(name, value)| (name.into(), Nbt::Float(value)))
            .collect(),
    )
}

fn definition(collision: Nbt) -> super::Definition {
    parse_definition(&Nbt::Compound(vec![(
        "components".into(),
        Nbt::Compound(vec![("minecraft:collision_box".into(), collision)]),
    )]))
    .expect("definition")
}

#[test]
fn native_collision_boxes_retain_bench_and_slab_heights() {
    for (height, expected) in [(10.0, 0.625), (8.0, 0.5)] {
        let parsed = definition(component(vec![native_box([0.0; 3], [16.0, height, 16.0])]));
        assert!(parsed.collides);
        assert_eq!(
            parsed.collision_boxes.as_deref(),
            Some(
                [CustomBox {
                    min: [0.0; 3],
                    max: [1.0, expected, 1.0],
                }]
                .as_slice()
            ),
            "native corner coordinates must reach collision registration"
        );
    }
}

#[test]
fn native_collision_boxes_use_corner_coordinates_without_a_center_shift() {
    let parsed = definition(component(vec![native_box(
        [2.0, 2.0, 6.0],
        [6.0, 8.0, 16.0],
    )]));
    assert_eq!(
        parsed.collision_boxes.as_deref(),
        Some(
            [CustomBox {
                min: [0.625, 0.125, 0.375],
                max: [0.875, 0.5, 1.0],
            }]
            .as_slice()
        )
    );
}

#[test]
fn native_collision_boxes_clamp_and_order_endpoints_with_taller_y_bounds() {
    let parsed = definition(component(vec![native_box(
        [20.0, 40.0, 12.0],
        [-2.0, 8.0, 4.0],
    )]));
    assert_eq!(
        parsed.collision_boxes.as_deref(),
        Some(
            [CustomBox {
                min: [0.0, 0.5, 0.25],
                max: [1.0, 1.5, 0.75],
            }]
            .as_slice()
        )
    );
}

#[test]
fn native_collision_boxes_default_empty_lists_and_missing_coordinates() {
    let empty = definition(component(Vec::new()));
    assert!(empty.collides);
    assert!(empty.collision_boxes.is_none());
    let missing_enabled = definition(Nbt::Compound(vec![("boxes".into(), Nbt::List(Vec::new()))]));
    assert!(!missing_enabled.collides);
    let zero = definition(component(vec![Nbt::Compound(Vec::new())]));
    assert_eq!(
        zero.collision_boxes.as_deref(),
        Some(
            [CustomBox {
                min: [1.0, 0.0, 0.0],
                max: [1.0, 0.0, 0.0],
            }]
            .as_slice()
        )
    );
}

#[test]
fn native_collision_boxes_skip_invalid_and_excess_entries_without_losing_valid_shapes() {
    let mut boxes = (0..super::MAX_COLLISION_BOXES + 9)
        .map(|_| native_box([0.0; 3], [16.0, 8.0, 16.0]))
        .collect::<Vec<_>>();
    boxes[0] = Nbt::String("unexpected".into());
    boxes[1] = native_box([f64::NAN, 0.0, 0.0], [16.0; 3]);
    boxes[2] = native_box([0.0; 3], [1e100, 16.0, 16.0]);
    let parsed = definition(component(boxes));
    assert_eq!(parsed.skipped, 12);
    assert_eq!(
        parsed.collision_boxes.as_ref().unwrap().len(),
        super::MAX_COLLISION_BOXES - 3
    );
    let invalid = definition(component(vec![Nbt::String("unexpected".into())]));
    assert_eq!(invalid.skipped, 1);
    assert!(invalid.collision_boxes.is_none());
}

#[test]
fn native_collision_boxes_keep_origin_size_compatibility() {
    let list = |values: [f64; 3]| Nbt::List(values.into_iter().map(Nbt::Float).collect());
    let parsed = definition(Nbt::Compound(vec![
        ("origin".into(), list([-8.0, 0.0, -8.0])),
        ("size".into(), list([16.0, 8.0, 16.0])),
    ]));
    assert!(parsed.collides);
    assert_eq!(
        parsed.collision_boxes.as_deref(),
        Some(
            [CustomBox {
                min: [0.0; 3],
                max: [1.0, 0.5, 1.0],
            }]
            .as_slice()
        )
    );
}

#[test]
fn native_collision_boxes_default_wrong_typed_coordinates_to_zero() {
    let mut entry = native_box([2.0, 2.0, 6.0], [6.0, 8.0, 16.0]);
    let Nbt::Compound(fields) = &mut entry else {
        unreachable!()
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == "minX")
        .unwrap()
        .1 = Nbt::Int(2);
    let parsed = definition(component(vec![entry]));
    assert_eq!(
        parsed.collision_boxes.as_deref(),
        Some(
            [CustomBox {
                min: [0.625, 0.125, 0.375],
                max: [1.0, 0.5, 1.0],
            }]
            .as_slice()
        )
    );
}
