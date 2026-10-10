use super::super::super::loading_screen::LoadingStage;
use super::super::review_tests::{paint, solids};
use super::*;

fn loading(stage: LoadingStage, seconds: f64) -> Vec<ui::UiNode> {
    use crate::ui_runtime::presentation::{TextMetrics, UiPresentationRuntime};
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
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
    use crate::ui_runtime::{
        oreui_assets::{OVERWORLD_BLOCK_IMAGE, OreUiImages, OreUiPage, OreUiSprite},
        presentation::{TextMetrics, UiPresentationRuntime},
    };
    let sizes = [[112, 112]];
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
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
        .form_presentation
        .oreui_originals
        .as_ref()
        .unwrap()
        .page;
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

fn fill(seconds: f64, fraction: Option<f32>) -> Option<[f32; 4]> {
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        canvas.seconds = seconds;
        track(canvas, [10.0, 20.0, 210.0, 32.0], fraction).unwrap();
    });
    solids(&nodes)
        .into_iter()
        .find(|(_, color)| *color == theme::PRIMARY_ROLE.fill)
        .map(|(bounds, _)| bounds)
}

#[test]
fn unknown_progress_animates_the_loader_without_claiming_a_percentage() {
    let mut previous = None;
    for seconds in [0.0, 0.25, 0.5, 0.75, 1.0, 1.25] {
        let (_, _, nodes) = paint(Default::default(), |canvas| {
            canvas.seconds = seconds;
            loader(canvas, [10.0, 20.0, 58.0, 68.0]).unwrap();
        });
        let frame = nodes
            .iter()
            .filter_map(|node| {
                if let ui::UiVisual::RotatedSprite { color, .. } = node.visual() {
                    Some(*color)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(frame.len(), 3);
        assert_eq!(fill(seconds, None), None);
        if let Some(previous) = previous {
            assert_ne!(frame, previous);
        }
        previous = Some(frame);
    }
}

#[test]
fn download_progress_is_determinate_and_clamps_invalid_input() {
    let half = fill(0.0, Some(0.5)).unwrap();
    assert_eq!(Some(half), fill(10.0, Some(0.5)));
    assert!((half[2] - 110.0).abs() < 1e-5);
    assert_eq!(fill(0.0, Some(2.0)).unwrap()[2], 208.0);
    assert_eq!(fill(0.5, Some(f32::NAN)), None);
}

#[test]
fn loading_logo_and_status_form_one_compact_centered_group() {
    use crate::ui_runtime::presentation::{IconRef, menu_artwork::TITLE_KEY};
    static ART: std::sync::OnceLock<std::collections::HashMap<String, IconRef>> =
        std::sync::OnceLock::new();
    let art = ART.get_or_init(|| {
        std::collections::HashMap::from([(
            TITLE_KEY.to_owned(),
            IconRef {
                page: 1,
                uv: [0, 0, 300, 100],
                glint: false,
            },
        )])
    });
    let mut view = MenuView::new(true, "Fixture".into());
    view.connecting = true;
    let mut rem = 0.0;
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        rem = canvas.rem;
        canvas.title_artwork = art.get(TITLE_KEY).copied();
        join(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let logo = nodes
        .iter()
        .find(|node| {
            matches!(
                node.visual(),
                ui::UiVisual::Sprite {
                    texture_page: 1,
                    ..
                }
            )
        })
        .unwrap()
        .bounds();
    let panel = solids(&nodes)
        .into_iter()
        .find(|(bounds, color)| {
            *color == theme::NEUTRAL80.fill && bounds[2] - bounds[0] > rem * 20.0
        })
        .unwrap()
        .0;
    let gap = panel[1] - logo.max().y();
    assert!(
        gap >= 0.0 && gap <= rem * 2.4 + 0.01,
        "logo-to-status gap: {gap}"
    );
    assert!(((logo.min().y() + panel[3]) * 0.5 - 360.0).abs() <= rem);
}

#[test]
fn long_loading_details_scroll_without_moving_cancel_off_screen() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.progress = Some(launcher::local_worlds::Progress {
        stage: launcher::local_worlds::Stage::DownloadingServer,
        fraction: Some(0.5),
        detail: "A long world name and progress description ".repeat(40),
    });
    for size in [[1280.0, 720.0], [640.0, 320.0], [320.0, 560.0]] {
        let (scrolls, hits, _) = paint(Default::default(), |canvas| {
            join(canvas, &view, size, &|_| None).unwrap()
        });
        let bounds = hits[0].1;
        assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
        assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
        let scroll = scrolls.iter().find(|scroll| scroll.max > 0.0).unwrap();
        assert!(scroll.viewport.min().y() < scroll.viewport.max().y());
        assert!(scroll.viewport.max().y() < bounds.min().y());
    }
}
