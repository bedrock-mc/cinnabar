use super::*;
use crate::menu::MenuScreen;
use crate::ui_runtime::{
    oreui_assets::{
        OreUiImages, OreUiPage, OreUiSprite, SETTINGS_ICON_HIGHLIGHT_IMAGE,
        load_optional_oreui_images,
    },
    presentation::{TextMetrics, UiPresentationRuntime, tests::fixture_font},
};
use std::{collections::HashMap, sync::Arc};

#[test]
fn installed_settings_categories_use_their_full_native_icon_cells() {
    let Some(images) = load_optional_oreui_images() else {
        eprintln!(
            "skipping installed_settings_categories_use_their_full_native_icon_cells: installed OreUI bundle unavailable"
        );
        return;
    };
    for key in SETTINGS_ICONS {
        let sprite = images
            .sprites
            .get(key)
            .expect("native Settings icon exists");
        let [left, top, right, bottom] = sprite.bounds;
        assert_eq!([right - left, bottom - top], [24; 2], "{key}");
    }
}

fn presentation() -> (UiPresentationRuntime, u16) {
    let mut sprites: HashMap<_, _> = SETTINGS_ICONS
        .iter()
        .map(|key| {
            (
                (*key).into(),
                OreUiSprite {
                    page: 0,
                    bounds: [0, 0, 24, 24],
                },
            )
        })
        .collect();
    sprites.insert(
        SETTINGS_ICON_HIGHLIGHT_IMAGE.into(),
        OreUiSprite {
            page: 1,
            bounds: [0, 0, 216, 24],
        },
    );
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let page = runtime.textures.dynamic_start() as u16 + 1;
    runtime
        .enable_oreui_originals(OreUiImages {
            pages: vec![
                OreUiPage {
                    dimensions: [24; 2],
                    pixels: vec![255; 24 * 24 * 4].into(),
                },
                OreUiPage {
                    dimensions: [216, 24],
                    pixels: vec![255; 216 * 24 * 4].into(),
                },
            ],
            sprites: Arc::new(sprites),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        })
        .unwrap();
    (runtime, page)
}

fn frame(
    runtime: &mut UiPresentationRuntime,
    section: u8,
    seconds: f64,
    page: u16,
) -> Vec<[u16; 4]> {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = section;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    runtime.menu_seconds = seconds;
    runtime
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    runtime.end_animation_frame();
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Sprite {
                texture_page, uv, ..
            } if *texture_page == page => Some(*uv),
            _ => None,
        })
        .collect()
}

#[test]
fn selected_settings_icon_sweeps_once_and_restarts_on_reselection() {
    let (mut runtime, page) = presentation();
    let controller = section_index(TABS[2].0);
    let audio = section_index(TABS[7].0);
    assert_eq!(frame(&mut runtime, controller, 2.0, page), [[0, 0, 24, 24]]);
    assert_eq!(
        frame(&mut runtime, controller, 2.051, page),
        [[48, 0, 72, 24]]
    );
    assert_eq!(
        frame(&mut runtime, controller, 2.5, page),
        [[192, 0, 216, 24]]
    );
    assert_eq!(
        frame(&mut runtime, controller, 12.0, page),
        [[192, 0, 216, 24]]
    );
    assert_eq!(frame(&mut runtime, audio, 12.1, page), [[0, 0, 24, 24]]);
    assert_eq!(
        frame(&mut runtime, controller, 12.2, page),
        [[0, 0, 24, 24]]
    );
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Home;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    runtime
        .append_oreui_screen(
            &view,
            &mut Vec::new(),
            &mut 1,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    runtime.end_animation_frame();
    assert_eq!(
        frame(&mut runtime, controller, 13.0, page),
        [[0, 0, 24, 24]]
    );
}

#[test]
fn category_group_divider_has_two_full_width_edges_and_its_own_flow_slot() {
    use super::super::super::review_tests::{paint, solids};
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        assert_eq!(
            group_label(canvas, "Controls", [20.0, 300.0], 100.0).unwrap(),
            150.0
        );
    });
    assert_eq!(
        solids(&nodes),
        [
            ([20.0, 146.0, 300.0, 148.0], [0, 0, 0, 102]),
            ([20.0, 148.0, 300.0, 150.0], [255, 255, 255, 26]),
        ]
    );
}

#[test]
fn category_images_keep_native_pixel_steps_at_odd_gui_scales() {
    let (runtime, _) = presentation();
    let originals = runtime.form_presentation.oreui_originals.as_deref();
    for (gui, dpi, expected) in [
        (1, 1.0, 12.0),
        (2, 1.0, 24.0),
        (3, 1.0, 24.0),
        (4, 1.0, 48.0),
        (3, 2.0, 12.0),
    ] {
        let metrics =
            TextMetrics::for_viewport([3840, 2160], ui::DpiScale::new(dpi).unwrap(), Some(gui));
        let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 4096));
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &runtime.font,
            metrics,
            0,
            originals,
        );
        icon(&mut canvas, 2, [100.0, 100.0]).unwrap();
        let cell = nodes.last().unwrap().bounds();
        assert_eq!(
            [
                cell.max().x() - cell.min().x(),
                cell.max().y() - cell.min().y()
            ],
            [expected; 2]
        );
    }
}

#[test]
fn category_bevels_follow_selection_hover_and_press() {
    use super::super::super::review_tests::{paint, solids};

    let action = MenuAction::SettingsSection(section_index(TABS[7].0));
    for (selected, hovered, pressed, fill, edges) in [
        (false, false, false, NEUTRAL80.fill, None),
        (false, false, true, NEUTRAL80.fill, None),
        (
            false,
            true,
            false,
            NEUTRAL80.hovered,
            Some(([255, 255, 255, 26], [0, 0, 0, 102])),
        ),
        (
            false,
            true,
            true,
            NEUTRAL80.hovered,
            Some(([0, 0, 0, 204], [255, 255, 255, 26])),
        ),
        (
            true,
            false,
            false,
            NEUTRAL80.hovered,
            Some(([0, 0, 0, 102], [255, 255, 255, 26])),
        ),
        (
            true,
            true,
            true,
            NEUTRAL80.hovered,
            Some(([0, 0, 0, 102], [255, 255, 255, 26])),
        ),
    ] {
        let mut view = MenuView::new(true, "Player".into());
        view.settings_section = section_index(TABS[if selected { 7 } else { 5 }].0);
        view.hovered = hovered.then_some(action);
        view.pressed = pressed.then_some(action);
        let mut edge = 0.0;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            edge = canvas.r(EDGE);
            draw(canvas, &view, [20.0, 20.0, 420.0, 720.0], &|_| None, None).unwrap();
        });
        let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
        let [left, top, right, bottom] =
            [row.min().x(), row.min().y(), row.max().x(), row.max().y()];
        let fills = solids(&nodes);
        assert!(
            fills.contains(&([left, top, right, bottom], fill)),
            "selected={selected}, hovered={hovered}, pressed={pressed}"
        );
        let strips: Vec<_> = fills
            .into_iter()
            .filter(|(bounds, _)| {
                bounds[0] == left
                    && bounds[2] == right
                    && bounds[1] >= top
                    && bounds[3] <= bottom
                    && bounds[3] - bounds[1] <= edge
            })
            .collect();
        if let Some((upper, lower)) = edges {
            assert_eq!(
                strips,
                [
                    ([left, top, right, top + edge], upper),
                    ([left, bottom - edge, right, bottom], lower)
                ]
            );
        } else {
            assert!(strips.is_empty(), "an idle category has no divider");
        }
    }
}

#[test]
fn category_press_and_selection_keep_the_same_highlight_brightness() {
    use super::super::super::{
        motion::Feedback,
        review_tests::{paint, solids},
    };
    let color = |motion| {
        let (_, _, nodes) = paint(HashMap::new(), |canvas| {
            row_background(canvas, [20.0, 20.0, 420.0, 80.0], motion).unwrap();
        });
        solids(&nodes)[0].1
    };
    assert_eq!(
        color(Feedback {
            press: 1.0,
            ..Default::default()
        }),
        color(Feedback {
            selected: 1.0,
            ..Default::default()
        }),
    );
}

fn visible_color(draw: &ui::UiDrawList, point: [f32; 2]) -> Option<[u8; 4]> {
    let hit = ui::UiPoint::new(point[0], point[1]).unwrap();
    draw.batches
        .iter()
        .filter(|batch| batch.clip.contains(hit))
        .flat_map(|batch| {
            draw.indices[batch.index_range.start as usize..batch.index_range.end as usize]
                .as_chunks::<6>()
                .0
                .iter()
        })
        .rfind(|quad| {
            let min = std::array::from_fn::<_, 2, _>(|axis| {
                quad.iter()
                    .map(|index| draw.vertices[*index as usize].position[axis])
                    .fold(f32::INFINITY, f32::min)
            });
            let max = std::array::from_fn::<_, 2, _>(|axis| {
                quad.iter()
                    .map(|index| draw.vertices[*index as usize].position[axis])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            point[0] >= min[0] && point[0] < max[0] && point[1] >= min[1] && point[1] < max[1]
        })
        .map(|quad| draw.vertices[quad[0] as usize].color)
}

fn draw_list(nodes: Vec<ui::UiNode>) -> ui::UiDrawList {
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        ui::UiRect::new(
            ui::UiPoint::new(0.0, 0.0).unwrap(),
            ui::UiPoint::new(1280.0, 720.0).unwrap(),
        )
        .unwrap(),
        ui::UiScale::default(),
        ui::SafeArea::default(),
    )
    .unwrap();
    tree.build_draw_list().unwrap()
}

#[test]
fn focused_category_outline_remains_visible_over_the_following_row() {
    use super::super::super::review_tests::paint;

    let action = MenuAction::SettingsSection(section_index("global_texture_pack_forced_index"));
    for (selected, hovered) in [(true, false), (false, false), (false, true)] {
        let mut view = MenuView::new(true, "Player".into());
        view.settings_section = if selected {
            section_index("global_texture_pack_forced_index")
        } else {
            section_index(TABS[0].0)
        };
        view.focused_action = Some(action);
        view.hovered = hovered.then_some(action);
        let mut edge = 0.0;
        let (_, hits, nodes) = paint(
            HashMap::from([("oreui_settings_sidebar".into(), 120.0)]),
            |canvas| {
                edge = canvas.r(EDGE);
                draw(canvas, &view, [20.0, 20.0, 420.0, 720.0], &|_| None, None).unwrap();
            },
        );
        let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
        let overlap = edge * if selected || hovered { 2.0 } else { 1.0 };
        let centre = [
            (row.min().x() + row.max().x()) * 0.5,
            (row.min().y() + row.max().y()) * 0.5,
        ];
        let samples = [
            [centre[0], row.min().y() - overlap + edge * 0.5],
            [centre[0], row.max().y() + overlap - edge * 0.5],
            [row.min().x() + edge * 0.5, centre[1]],
            [row.max().x() - edge * 0.5, centre[1]],
        ];
        let draw = draw_list(nodes);
        for point in samples {
            assert_eq!(
                visible_color(&draw, point),
                Some(theme::OUTLINE),
                "all focus edges remain visible: selected={selected}, hovered={hovered}, point={point:?}"
            );
        }
    }
}

#[test]
fn focused_category_outline_stays_clipped_to_its_sidebar_viewport() {
    use super::super::super::review_tests::paint;

    let action = MenuAction::SettingsSection(section_index(TABS[2].0));
    let mut view = MenuView::new(true, "Player".into());
    view.settings_section = section_index(TABS[2].0);
    view.focused_action = Some(action);
    let bounds = [20.0, 20.0, 420.0, 300.0];
    let (scrolls, hits, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, bounds, &|_| None, None).unwrap();
    });
    let viewport = scrolls[0].viewport;
    let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let offset = row.min().y() - viewport.min().y();
    let mut edge = 0.0;
    let (_, hits, nodes) = paint(
        HashMap::from([("oreui_settings_sidebar".into(), offset)]),
        |canvas| {
            edge = canvas.r(EDGE);
            draw(canvas, &view, bounds, &|_| None, None).unwrap();
        },
    );
    let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let draw = draw_list(nodes);
    let x = row.min().x() + edge * 0.5;
    assert_eq!(
        visible_color(&draw, [x, viewport.min().y() + edge]),
        Some(theme::OUTLINE)
    );
    assert_ne!(
        visible_color(&draw, [x, viewport.min().y() - edge * 0.5]),
        Some(theme::OUTLINE),
        "the focused row's vertical border remains inside its scroll clip"
    );
}
