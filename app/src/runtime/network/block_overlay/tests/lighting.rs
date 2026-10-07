use super::{CustomBlockVisuals, CustomPermutation, CustomVisualComponents, block};
use crate::runtime::network::block_overlay::{
    OverlayGaps,
    condition::{BlockExpressions, state_visual},
};

#[test]
fn geometry_replacement_resets_absorption_but_other_permutations_preserve_it() {
    for (geometry, expected) in [(Some("geometry.path".into()), 0), (None, 15)] {
        let block = block(
            "test:path",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("geometry.path".into()),
                    geometry_use_block_type_light_absorption: true,
                    ..Default::default()
                },
                permutations: Box::new([CustomPermutation {
                    condition: "1".into(),
                    components: CustomVisualComponents {
                        geometry,
                        ..Default::default()
                    },
                }]),
                ..Default::default()
            },
        );
        let state = state_visual(
            &block,
            &BlockExpressions::new(&block),
            Some(&[]),
            &mut OverlayGaps::default(),
        );
        assert_eq!(state.components.effective_light_dampening(), expected);
    }
}

#[test]
fn geometry_replacement_keeps_explicit_absorption() {
    let block = block(
        "test:path",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("geometry.path".into()),
                geometry_use_block_type_light_absorption: true,
                light_dampening: Some(7),
                ..Default::default()
            },
            permutations: Box::new([CustomPermutation {
                condition: "1".into(),
                components: CustomVisualComponents {
                    geometry: Some("geometry.path".into()),
                    ..Default::default()
                },
            }]),
            ..Default::default()
        },
    );
    let state = state_visual(
        &block,
        &BlockExpressions::new(&block),
        Some(&[]),
        &mut OverlayGaps::default(),
    );
    assert_eq!(state.components.effective_light_dampening(), 7);
}
