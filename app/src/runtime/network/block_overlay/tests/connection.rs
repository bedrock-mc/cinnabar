//! Wiki-style custom fences resolve their rail bones per connection state.
//! The `minecraft:connection` trait expands to four boolean axes, so each of
//! the sixteen states hides a different arm subset instead of staying
//! unevaluated with every arm visible.

use std::sync::Arc;

use protocol::{
    CustomBlock, CustomBlockVisuals, CustomBlocks, CustomBox, CustomCollision,
    CustomPhysicalComponents, CustomSelection, CustomStateAxis, CustomStateValue,
    CustomVisualComponents,
};

use super::{OverlayGaps, block, compile_block_overlay, materials, view_with_geometry};

const FENCE_GEOMETRY: &str = r#"{"format_version": "1.12.0", "minecraft:geometry": [{
    "description": {"identifier": "geometry.fence", "texture_width": 16, "texture_height": 16},
    "bones": [
        {"name": "post", "cubes": [{"origin": [-2, 0, -2], "size": [4, 16, 4], "uv": [0, 0]}]},
        {"name": "north_rails", "cubes": [{"origin": [-2, 6, -8], "size": [4, 4, 6], "uv": [0, 0]}]},
        {"name": "east_rails", "cubes": [{"origin": [2, 6, -2], "size": [6, 4, 4], "uv": [0, 0]}]},
        {"name": "south_rails", "cubes": [{"origin": [-2, 6, 2], "size": [4, 4, 6], "uv": [0, 0]}]},
        {"name": "west_rails", "cubes": [{"origin": [-8, 6, -2], "size": [6, 4, 4], "uv": [0, 0]}]}
    ]}]}"#;

const DIRECTIONS: [(&str, &str); 4] = [
    ("north_rails", "minecraft:connection_north"),
    ("east_rails", "minecraft:connection_east"),
    ("south_rails", "minecraft:connection_south"),
    ("west_rails", "minecraft:connection_west"),
];

/// Builds one boolean axis for each rail direction in the fixture.
fn connection_axes() -> Box<[CustomStateAxis]> {
    DIRECTIONS
        .iter()
        .map(|(_, state)| CustomStateAxis {
            name: (*state).into(),
            values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
        })
        .collect()
}

/// Selects the fixture's north and south collision boxes from their connection states.
fn collision_permuts() -> Box<[protocol::CustomPermutation]> {
    let shift = |direction: &str, boxes: CustomBox| protocol::CustomPermutation {
        physical: CustomPhysicalComponents {
            collision: Some(CustomCollision {
                enabled: true,
                boxes: Some(Arc::from([boxes])),
            }),
            selection: Some(CustomSelection::Box(boxes)),
            ..CustomPhysicalComponents::default()
        },
        condition: format!("q.block_state('minecraft:connection_{direction}')").into(),
        components: CustomVisualComponents::default(),
    };
    Box::new([
        shift(
            "north",
            CustomBox {
                min: [0.375, 0.0, 0.0],
                max: [0.625, 1.5, 0.625],
            },
        ),
        shift(
            "south",
            CustomBox {
                min: [0.375, 0.0, 0.375],
                max: [0.625, 1.5, 1.0],
            },
        ),
    ])
}

/// Builds a fence whose rail visibility and collision depend on its connection states.
fn fence() -> CustomBlock {
    block(
        "test:fence",
        16,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("geometry.fence".into()),
                materials: materials("gen"),
                bone_visibility: DIRECTIONS
                    .iter()
                    .map(|(bone, state)| {
                        (
                            Arc::from(*bone),
                            Arc::from(format!("q.block_state('{state}')")),
                        )
                    })
                    .collect(),
                ..CustomVisualComponents::default()
            },
            permutations: collision_permuts(),
            state_axes: connection_axes(),
            ..CustomBlockVisuals::default()
        },
    )
}

/// Lists the rails hidden by a fixture state whose north connection varies fastest.
fn hidden_for_state(index: u32) -> Vec<String> {
    DIRECTIONS
        .iter()
        .enumerate()
        .filter(|(bit, _)| index & (1 << bit) == 0)
        .map(|(_, (bone, _))| (*bone).to_owned())
        .collect()
}

#[test]
fn connection_states_hide_only_their_disconnected_rails() {
    let fence = fence();
    let expressions = super::super::condition::BlockExpressions::new(&fence);
    for index in 0..16 {
        let mut gaps = OverlayGaps::default();
        let values = fence.state_values(index).expect("decoded connection state");
        let resolved =
            super::super::condition::state_visual(&fence, &expressions, Some(&values), &mut gaps);
        let mut hidden: Vec<String> = resolved
            .hidden_bones
            .iter()
            .map(|bone| bone.to_string())
            .collect();
        hidden.sort();
        let mut expected = hidden_for_state(index);
        expected.sort();
        assert_eq!(hidden, expected, "state {index}");
        assert_eq!(gaps.unevaluated_permutations, 0, "state {index}");
    }
}

#[test]
fn connected_fence_compiles_sixteen_textured_states() {
    let view = view_with_geometry(FENCE_GEOMETRY.as_bytes());
    let blocks = CustomBlocks {
        blocks: vec![fence()].into(),
        ..CustomBlocks::default()
    };
    let compiled = compile_block_overlay(&view, &blocks, false, None).expect("overlay");
    assert_eq!(compiled.gaps.unevaluated_permutations, 0);
    assert_eq!(compiled.gaps.missing_geometry, 0);
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(compiled.gaps.incomplete_state_identities, 0);
    assert_eq!(compiled.overlay.visuals.len(), 16);
    let quads = |state: usize| {
        let template = compiled.overlay.visuals[state].model_template as usize;
        compiled.overlay.model_templates[template].quad_count
    };
    assert_eq!(quads(0), 6, "the isolated post keeps one cube");
    assert_eq!(
        quads(15),
        30,
        "the fully connected fence keeps all five cubes"
    );
    assert_eq!(quads(1), 12, "one connected arm adds one cube");
}
