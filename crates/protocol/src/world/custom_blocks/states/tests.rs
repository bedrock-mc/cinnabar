use super::*;

fn compound(fields: Vec<(&str, Nbt)>) -> Nbt {
    Nbt::Compound(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn text(value: &str) -> Nbt {
    Nbt::String(value.to_owned())
}

fn flags(states: &[&str]) -> Nbt {
    compound(states.iter().map(|&state| (state, Nbt::Byte(1))).collect())
}

fn axes_of(root: &Nbt) -> StateAxes {
    let traits = root
        .list("traits")
        .iter()
        .map(|value| (None, value))
        .collect::<Vec<_>>();
    state_axes(root, &traits)
}

fn names(states: &StateAxes) -> Vec<&str> {
    states.axes.iter().map(|axis| axis.name.as_ref()).collect()
}

fn property(name: &str, values: usize) -> Nbt {
    compound(vec![
        ("name", text(name)),
        (
            "enum",
            Nbt::List((0..values as i64).map(Nbt::Int).collect()),
        ),
    ])
}

// Corner and sixteen-way rotation states must widen the palette run, or later ids shift.
#[test]
fn placement_direction_corner_and_rotation_states_are_counted_in_client_order() {
    let root = compound(vec![(
        "traits",
        Nbt::List(vec![compound(vec![
            ("name", text("minecraft:placement_direction")),
            (
                "enabled_states",
                flags(&["corner_and_cardinal_direction", "sixteen_way_rotation"]),
            ),
        ])]),
    )]);
    let states = axes_of(&root);
    assert_eq!(
        names(&states),
        [
            "minecraft:cardinal_direction",
            "minecraft:corner",
            "minecraft:sixteen_way_rotation"
        ]
    );
    assert_eq!(states.count, 4 * 5 * 16);
    assert_eq!(states.trait_count, 3);
    assert!(!states.identity_incomplete);
    assert_eq!(
        states.axes[1].values[4],
        CustomStateValue::String("outer_right".into())
    );
    assert_eq!(
        states.axes[2].values[9],
        CustomStateValue::String("down".into()),
        "rotations past the sixth serialize as the first facing name"
    );
}

#[test]
fn multi_block_trait_adds_one_state_per_part() {
    let root = compound(vec![(
        "traits",
        Nbt::List(vec![compound(vec![
            ("name", text("minecraft:multi_block")),
            ("enabled_states", flags(&["multi_block"])),
            ("part_count", Nbt::Byte(3)),
            ("direction", Nbt::Byte(1)),
        ])]),
    )]);
    let states = axes_of(&root);
    assert_eq!(names(&states), ["minecraft:multi_block_part"]);
    assert_eq!(
        states.axes[0].values.as_ref(),
        [0, 1, 2].map(CustomStateValue::Int)
    );
    assert_eq!(states.count, 3);
}

// Six six-value states need eighteen bits; the sixth falls outside sixteen and reads its first value.
#[test]
fn states_past_sixteen_bits_keep_their_first_value() {
    let root = compound(vec![(
        "properties",
        Nbt::List(
            (0..6)
                .map(|index| property(&format!("test:p{index}"), 6))
                .collect(),
        ),
    )]);
    let states = axes_of(&root);
    assert_eq!(states.count, 6 * 6 * 6 * 6 * 6);
    assert_eq!(states.axes[5].values.as_ref(), [CustomStateValue::Int(0)]);
    let wide = compound(vec![(
        "properties",
        Nbt::List(
            (0..17)
                .map(|index| property(&format!("test:b{index}"), 2))
                .collect(),
        ),
    )]);
    assert_eq!(axes_of(&wide).count as u64, MAX_STATES_PER_BLOCK);
}

#[test]
fn single_value_states_still_take_a_bit() {
    let root = compound(vec![(
        "properties",
        Nbt::List(
            (0..16)
                .map(|index| property(&format!("test:one{index}"), 1))
                .chain([property("test:last", 2)])
                .collect(),
        ),
    )]);
    let states = axes_of(&root);
    assert_eq!(states.count, 1);
    assert_eq!(states.axes[16].values.len(), 1);
}

#[test]
fn properties_without_enums_and_repeated_names_add_no_state() {
    let root = compound(vec![
        (
            "traits",
            Nbt::List(vec![compound(vec![
                ("name", text("minecraft:placement_position")),
                ("enabled_states", flags(&["vertical_half"])),
            ])]),
        ),
        (
            "properties",
            Nbt::List(vec![
                compound(vec![("name", text("test:no_enum"))]),
                property("minecraft:vertical_half", 3),
                property("test:kept", 3),
                property("test:kept", 4),
                compound(vec![("enum", Nbt::List(vec![Nbt::Int(0), Nbt::Int(1)]))]),
            ]),
        ),
    ]);
    let states = axes_of(&root);
    assert_eq!(
        names(&states),
        ["minecraft:vertical_half", "test:kept", ""],
        "a nameless property keeps the empty name"
    );
    assert_eq!(states.count, 2 * 3 * 2);
    assert!(!states.identity_incomplete);
}

#[test]
fn unknown_enabled_states_leave_the_identity_incomplete() {
    let root = compound(vec![(
        "traits",
        Nbt::List(vec![compound(vec![
            ("name", text("test:future_trait")),
            ("enabled_states", flags(&["test:future_state"])),
        ])]),
    )]);
    assert!(axes_of(&root).identity_incomplete);
}
