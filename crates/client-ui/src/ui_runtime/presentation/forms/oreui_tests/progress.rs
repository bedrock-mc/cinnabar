//! The loading screen through its adapter: backdrops, stillness and destination art.

use super::super::super::UiPresentationRuntime;
use oreui::test_support::solids;
use ui::LoadingStage;
use ui::oreui_theme as theme;
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

fn loading(stage: LoadingStage, seconds: f64) -> Vec<ui::UiNode> {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.menu_seconds = seconds;
    let mut nodes = Vec::new();
    presentation
        .append_oreui_loading(
            stage,
            ["Generating World", "Building terrain"],
            &mut nodes,
            &mut 1,
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2)),
            [1280.0, 720.0],
        )
        .unwrap();
    nodes
}

#[test]
fn world_loading_mutes_busy_backdrops_without_changing_the_connecting_screen() {
    for stage in [
        LoadingStage::BuildingTerrain,
        LoadingStage::ChangingDimension,
    ] {
        let nodes = loading(stage, 0.0);
        let overlay = nodes
            .iter()
            .find_map(|node| match node.visual() {
                ui::UiVisual::Gradient { colors, .. }
                    if node.bounds().min() == ui::UiPoint::new(0.0, 0.0).unwrap()
                        && node.bounds().max() == ui::UiPoint::new(1280.0, 720.0).unwrap() =>
                {
                    Some(colors)
                }
                _ => None,
            })
            .expect("world loading dims the full backdrop beneath the status card");
        assert!(
            overlay
                .iter()
                .all(|color| color[3] >= 200 && color[..3].iter().all(|channel| *channel < 50))
        );
        let card = solids(&nodes)
            .into_iter()
            .find(|(_, color)| *color == theme::NEUTRAL80.fill)
            .unwrap();
        assert!(
            overlay
                .iter()
                .all(|color| color[..3].iter().max().unwrap() < &card.1[0])
        );
    }
    assert!(
        !loading(LoadingStage::Connecting, 0.0)
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::Gradient { .. }))
    );
}

#[test]
fn dimension_loading_stays_static_without_an_animated_percentage() {
    let first = loading(LoadingStage::ChangingDimension, 0.0);
    let later = loading(LoadingStage::ChangingDimension, 0.5);
    assert_eq!(solids(&first), solids(&later));
    assert!(
        !first
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::RotatedSprite { .. }))
    );
    assert!(
        !solids(&first)
            .iter()
            .any(|(_, color)| *color == theme::PRIMARY_ROLE.fill)
    );
}

#[test]
fn dimension_loading_uses_a_neutral_backdrop() {
    let nodes = loading(LoadingStage::ChangingDimension, 0.0);
    let colors = nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Gradient { colors, .. } => Some(colors),
            _ => None,
        })
        .unwrap();
    for color in colors {
        assert!(
            color[..3].iter().max().unwrap() - color[..3].iter().min().unwrap() <= 10,
            "dimension loading avoids a purple wash: {color:?}"
        );
    }
}

#[test]
fn overworld_loading_keeps_the_installed_grass_block_square_and_static() {
    use oreui::images::{OVERWORLD_BLOCK_IMAGE, OreUiImages, OreUiPage, OreUiSprite};
    let sizes = [[112, 112]];
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation
        .enable_oreui_originals(OreUiImages {
            pages: sizes
                .map(|dimensions| OreUiPage {
                    dimensions,
                    pixels: vec![255; (dimensions[0] * dimensions[1] * 4) as usize].into(),
                })
                .to_vec(),
            sprites: std::sync::Arc::new(
                [OVERWORLD_BLOCK_IMAGE]
                    .iter()
                    .enumerate()
                    .map(|(index, key)| {
                        (
                            (*key).to_owned(),
                            OreUiSprite {
                                page: index as u16,
                                bounds: [0, 0, sizes[index][0] as u16, sizes[index][1] as u16],
                            },
                        )
                    })
                    .collect(),
            ),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        })
        .unwrap();
    let page = presentation
        .screens
        .oreui
        .originals
        .as_ref()
        .unwrap()
        .page();
    for dimension in [0, 37, -1] {
        presentation.hud_frame_mut().dimension = dimension;
        let mut frame = |seconds| {
            presentation.menu_seconds = seconds;
            let mut nodes = Vec::new();
            presentation
                .append_oreui_loading(
                    LoadingStage::ChangingDimension,
                    ["Entering a dimension", "Building terrain"],
                    &mut nodes,
                    &mut 1,
                    TextMetrics::for_viewport(
                        [1280, 720],
                        ui::DpiScale::new(1.0).unwrap(),
                        Some(2),
                    ),
                    [1280.0, 720.0],
                )
                .unwrap();
            nodes
        };
        let nodes = frame(0.0);
        let later = frame(0.5);
        let sprites = |nodes: &[ui::UiNode]| {
            nodes
                .iter()
                .filter_map(|node| match node.visual() {
                    ui::UiVisual::Sprite {
                        texture_page,
                        uv,
                        color,
                    } => Some((node.bounds(), *texture_page, *uv, *color)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sprites(&nodes),
            sprites(&later),
            "destination art stays static"
        );
        let art: Vec<_> = nodes.iter().filter(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, .. } if *texture_page == page)).collect();
        if dimension != 0 {
            assert!(
                art.is_empty(),
                "unknown dimensions must not be labelled as the Nether"
            );
        } else {
            assert_eq!(art.len(), 1, "one destination image is displayed");
            let ui::UiVisual::Sprite {
                texture_page, uv, ..
            } = art[0].visual()
            else {
                unreachable!()
            };
            assert_eq!(*texture_page, page + dimension as u16);
            let bounds = art[0].bounds();
            let ratio =
                (bounds.max().x() - bounds.min().x()) / (bounds.max().y() - bounds.min().y());
            assert!((ratio - f32::from(uv[2] - uv[0]) / f32::from(uv[3] - uv[1])).abs() < 0.001);
        }
    }
}
