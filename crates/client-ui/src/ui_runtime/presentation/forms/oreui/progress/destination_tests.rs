use super::*;
use crate::ui_runtime::{
    oreui_assets::{OreUiImages, OreUiPage, OreUiSprite, dimensions::DIMENSIONS},
    presentation::{TextMetrics, UiPresentationRuntime},
};

#[test]
fn dimension_backgrounds_cover_each_window_without_stretching_or_bleeding_into_terrain() {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    let source_size = [160, 90];
    presentation
        .enable_oreui_originals(OreUiImages {
            pages: (0..3)
                .map(|_| OreUiPage {
                    dimensions: source_size,
                    pixels: vec![255; (source_size[0] * source_size[1] * 4) as usize].into(),
                })
                .collect(),
            sprites: std::sync::Arc::new(
                DIMENSIONS
                    .iter()
                    .enumerate()
                    .map(|(index, art)| {
                        (
                            art.background.to_owned(),
                            OreUiSprite {
                                page: index as u16,
                                bounds: [0, 0, 160, 90],
                            },
                        )
                    })
                    .collect(),
            ),
            loading_frames: Default::default(),
            animations: Default::default(),
        })
        .unwrap();
    let first_page = presentation
        .form_presentation
        .oreui_originals
        .as_ref()
        .unwrap()
        .page;
    for physical in [[1280, 720], [720, 1280], [1800, 720]] {
        let size = physical.map(|value| value as f32);
        let metrics = TextMetrics::for_viewport(physical, ui::DpiScale::new(1.0).unwrap(), Some(2));
        for dimension in [0, 1, 2, 37, -1] {
            presentation.hud_frame_mut().dimension = dimension;
            for stage in [
                LoadingStage::ChangingDimension,
                LoadingStage::BuildingTerrain,
            ] {
                let mut nodes = Vec::new();
                presentation
                    .append_oreui_loading(
                        stage,
                        ["Entering a dimension", "Building terrain"],
                        &mut nodes,
                        &mut 1,
                        metrics,
                        size,
                    )
                    .unwrap();
                let backgrounds: Vec<_> = nodes
                    .iter()
                    .filter(|node| {
                        matches!(node.visual(), ui::UiVisual::Sprite {texture_page, ..}
                        if *texture_page >= first_page && *texture_page < first_page + 3)
                    })
                    .collect();
                if stage != LoadingStage::ChangingDimension || !(0..3).contains(&dimension) {
                    assert!(backgrounds.is_empty());
                    continue;
                }
                assert_eq!(
                    backgrounds.len(),
                    1,
                    "destination background must be displayed"
                );
                let node = backgrounds[0];
                assert_eq!(node.bounds().min(), ui::UiPoint::new(0.0, 0.0).unwrap());
                assert_eq!(
                    node.bounds().max(),
                    ui::UiPoint::new(size[0], size[1]).unwrap()
                );
                let ui::UiVisual::Sprite {
                    texture_page, uv, ..
                } = node.visual()
                else {
                    unreachable!()
                };
                assert_eq!(*texture_page, first_page + dimension as u16);
                assert!(uv[2] <= 160 && uv[3] <= 90);
                let sampled_ratio = f32::from(uv[2] - uv[0]) / f32::from(uv[3] - uv[1]);
                assert!((sampled_ratio / (size[0] / size[1]) - 1.0).abs() < 0.03);
                let tint = nodes
                    .iter()
                    .find_map(|node| match node.visual() {
                        ui::UiVisual::Gradient { colors, .. } => Some(colors),
                        _ => None,
                    })
                    .unwrap();
                assert!(
                    tint.iter().all(|color| color[3] > 0 && color[3] < 255),
                    "the scenery remains visible"
                );
            }
        }
    }
}

#[test]
fn nether_and_end_use_the_existing_block_icon_pages() {
    use crate::test_support::{fixture_font, fixture_hud};
    use assets::{IconEntry, IconSprite, RuntimeIconCatalog, encode_icon_catalog};
    use std::sync::Arc;
    let sprites = (0..3)
        .map(|index| IconSprite {
            width: 32,
            height: 32,
            rgba8: [index, 2, 3, 255].repeat(32 * 32).into(),
        })
        .collect::<Vec<_>>();
    let mut entries = DIMENSIONS
        .iter()
        .enumerate()
        .map(|(index, art)| IconEntry {
            identifier: art.block.into(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&encode_icon_catalog([0; 32], &sprites, &entries).unwrap())
            .unwrap(),
    );
    let mut presentation =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons).unwrap();
    for dimension in [1, 2] {
        presentation.hud_frame_mut().dimension = dimension;
        let icon = presentation
            .item_icon(DIMENSIONS[dimension as usize].block, 0)
            .unwrap();
        let mut nodes = Vec::new();
        presentation
            .append_oreui_loading(
                LoadingStage::ChangingDimension,
                ["Entering a dimension", "Building terrain"],
                &mut nodes,
                &mut 1,
                TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2)),
                [1280.0, 720.0],
            )
            .unwrap();
        let drawn: Vec<_> = nodes
            .iter()
            .filter(|node| {
                matches!(node.visual(), ui::UiVisual::Sprite {texture_page, uv, ..}
            if *texture_page == icon.page && *uv == icon.uv)
            })
            .collect();
        assert_eq!(
            drawn.len(),
            1,
            "the destination uses its existing block icon"
        );
        assert_eq!(
            drawn[0].bounds().max().x() - drawn[0].bounds().min().x(),
            drawn[0].bounds().max().y() - drawn[0].bounds().min().y()
        );
    }
}
