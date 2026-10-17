use std::{collections::HashMap, sync::Arc};

use super::super::review_tests::paint;
use crate::test_support::{add_capes, dressing_room_view as view};
use launcher::dressing_room::{
    CosmeticCategory, DressingRoomSection, STARTER_SKIN_NAMES, SkinEditor, SkinEditorMode,
    SkinEditorTarget,
};
use {super::*, launcher::menu::MenuAction};

#[test]
fn cosmetic_categories_filter_tiles_and_keep_original_thumbnail_indices() {
    let head = render_model::hats::fixture::bunny();
    let back = render_model::hats::fixture::dragon_wings();
    let pet = render_model::hats::fixture::shoulder_parrot();
    for category in CosmeticCategory::ALL {
        let mut view = view(STARTER_SKIN_NAMES.len());
        let wardrobe = Arc::make_mut(&mut view.dressing_room);
        wardrobe.section = DressingRoomSection::Cosmetics;
        wardrobe.cosmetic_category = category;
        let mut visible = Vec::new();
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            visible = draw(canvas, &view, [1280.0, 10_000.0])
                .unwrap()
                .visible_cosmetics;
        });
        for (item, included) in [
            (
                head,
                matches!(category, CosmeticCategory::All | CosmeticCategory::Head),
            ),
            (
                back,
                matches!(category, CosmeticCategory::All | CosmeticCategory::Back),
            ),
            (
                pet,
                matches!(category, CosmeticCategory::All | CosmeticCategory::Pet),
            ),
        ] {
            assert_eq!(
                hits.iter()
                    .any(|(action, _)| { *action == command(Action::SelectCosmetic(Some(item))) }),
                included,
                "{category:?} gallery: {}",
                item.name(),
            );
        }
        assert!(!hits.iter().any(|(action, _)| matches!(
            action,
            MenuAction::DressingRoom(Action::SetCosmeticCategory(_))
        )));
        if category == CosmeticCategory::Back {
            let items = cosmetics::Item::all();
            assert_eq!(
                visible
                    .iter()
                    .map(|index| items[*index])
                    .collect::<Vec<_>>(),
                vec![back],
                "filtered thumbnails retain their source catalog indices",
            );
        }
    }
}

#[test]
fn cosmetic_filter_menu_owns_input_and_fits_the_viewport() {
    let mut view = view(STARTER_SKIN_NAMES.len());
    let wardrobe = Arc::make_mut(&mut view.dressing_room);
    wardrobe.section = DressingRoomSection::Cosmetics;
    wardrobe.cosmetic_filter_open = true;
    for size in [[1280.0, 720.0], [320.0, 720.0], [640.0, 240.0]] {
        let (scrolls, hits, _) = paint(HashMap::new(), |canvas| {
            let preview = draw(canvas, &view, size).unwrap();
            draw_overlay(canvas, &view, size, preview.filter_anchor).unwrap();
        });
        assert!(scrolls.is_empty(), "the open menu owns wheel input");
        assert!(hits.iter().all(|(action, _)| matches!(
            action,
            MenuAction::DressingRoom(Action::ToggleCosmeticFilter | Action::SetCosmeticCategory(_))
        )));
        for category in CosmeticCategory::ALL {
            let bounds = hits
                .iter()
                .find_map(|(action, bounds)| {
                    (*action == command(Action::SetCosmeticCategory(category))).then_some(bounds)
                })
                .expect("every category remains selectable, including the current one");
            assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
            assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
        }
    }
}

#[test]
fn busy_cosmetics_block_category_changes() {
    let mut view = view(STARTER_SKIN_NAMES.len());
    let wardrobe = Arc::make_mut(&mut view.dressing_room);
    wardrobe.section = DressingRoomSection::Cosmetics;
    wardrobe.busy = true;
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0]).unwrap();
    });
    assert!(!hits.iter().any(|(action, _)| matches!(
        action,
        MenuAction::DressingRoom(Action::SetCosmeticCategory(_) | Action::ToggleCosmeticFilter)
    )));
}

#[test]
fn switching_cosmetic_categories_keeps_the_filtered_collection_visible() {
    render_model::hats::fixture::gallery();
    let back = render_model::hats::fixture::dragon_wings();
    let mut view = view(STARTER_SKIN_NAMES.len());
    Arc::make_mut(&mut view.dressing_room).section = DressingRoomSection::Cosmetics;
    for size in [[1280.0, 720.0], [480.0, 1280.0]] {
        Arc::make_mut(&mut view.dressing_room).cosmetic_category = CosmeticCategory::All;
        let (scrolls, _, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, size).unwrap();
        });
        let scroll = scrolls.iter().find(|scroll| scroll.max > 0.0).unwrap();
        Arc::make_mut(&mut view.dressing_room).cosmetic_category = CosmeticCategory::Back;
        let (_, hits, _) = paint(
            HashMap::from([(scroll.key.clone(), scroll.max)]),
            |canvas| {
                draw(canvas, &view, size).unwrap();
            },
        );
        assert!(
            hits.iter()
                .any(|(action, _)| { *action == command(Action::SelectCosmetic(Some(back))) })
        );
    }
}

#[test]
fn cosmetic_viewport_reports_only_intersecting_gallery_tiles() {
    let _ = render_model::hats::fixture::gallery();
    let mut view = view(STARTER_SKIN_NAMES.len());
    Arc::make_mut(&mut view.dressing_room).section = DressingRoomSection::Cosmetics;
    let mut visible = Vec::new();
    paint(HashMap::new(), |canvas| {
        let left = 40.0;
        let top = 40.0;
        let side = canvas.r(12.8);
        let right = left + side * 2.0 + canvas.r(1.2);
        let viewport = [left, top + canvas.r(6.0), right, top + canvas.r(6.0) + side];
        let mut preview =
            character::draw(canvas, &view, [left, top, right, 680.0], viewport).unwrap();
        hats::collection(
            canvas,
            &view,
            [left, top, right, 680.0],
            viewport,
            &mut preview,
        )
        .unwrap();
        visible = preview.visible_cosmetics;
    });
    assert_eq!(visible, vec![0, 1]);
}

fn text_bounds(nodes: Vec<ui::UiNode>, size: [f32; 2]) -> Vec<([u8; 32], ui::UiRect)> {
    let texts = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some((layout.key().content_sha256, node.id())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut tree = ui::UiTree::new(nodes).unwrap();
    let frame = tree
        .layout(
            ui_engine::engine::rect(0.0, 0.0, size[0], size[1]).unwrap(),
            ui::UiScale::default(),
            ui::SafeArea::default(),
        )
        .unwrap();
    texts
        .into_iter()
        .map(|(key, id)| (key, frame.bounds(id).unwrap()))
        .collect()
}

#[test]
fn character_stage_uses_one_baseline_above_the_rotation_hint() {
    let view = view(STARTER_SKIN_NAMES.len());
    let mut stage = [0.0; 4];
    let mut band = 0.0;
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        band = canvas.r(2.0);
        let bounds = [40.0, 40.0, 460.0, 680.0];
        stage = character::draw(canvas, &view, bounds, bounds).unwrap().clip;
    });
    let baselines = super::super::test_support::solids(&nodes)
        .into_iter()
        .filter(|(bounds, _)| {
            bounds[1] >= stage[3] - band
                && bounds[3] <= stage[3] + 0.01
                && bounds[2] - bounds[0] >= (stage[2] - stage[0]) * 0.5
                && bounds[3] - bounds[1] < band * 0.5
        })
        .count();
    assert_eq!(baselines, 1, "extra floor lines crowd the stage's baseline");
}

#[test]
fn wardrobe_description_fits_between_the_heading_and_tabs() {
    let view = view(STARTER_SKIN_NAMES.len());
    for width in [320.0, 600.0] {
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            library::toolbar(canvas, &view, [40.0, 40.0, 40.0 + width, 720.0]).unwrap();
        });
        let import = hits
            .iter()
            .find(|(action, _)| *action == command(Action::Import))
            .unwrap()
            .1;
        let tabs = hits
            .iter()
            .find(|(action, _)| *action == command(Action::SetSection(DressingRoomSection::Capes)))
            .unwrap()
            .1;
        let mut intro = text_bounds(nodes, [1280.0, 720.0])
            .into_iter()
            .filter(|(_, bounds)| {
                bounds.max().x() < import.min().x()
                    && bounds.min().y() >= import.min().y()
                    && bounds.max().y() < tabs.min().y()
            })
            .map(|(_, bounds)| bounds)
            .collect::<Vec<_>>();
        intro.sort_by(|left, right| left.min().y().total_cmp(&right.min().y()));
        assert_eq!(
            intro.len(),
            2,
            "the wardrobe needs a heading and description"
        );
        assert!(intro[0].max().y() < intro[1].min().y());
    }
}

#[test]
fn rotation_hint_centres_visible_glyphs_between_soft_dividers() {
    let view = view(STARTER_SKIN_NAMES.len());
    let mut stage = [0.0; 4];
    let mut band = 0.0;
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        band = canvas.r(4.0);
        let bounds = [40.0, 40.0, 460.0, 680.0];
        stage = character::draw(canvas, &view, bounds, bounds).unwrap().clip;
    });
    let (divider, color) = super::super::test_support::solids(&nodes)
        .into_iter()
        .find(|(bounds, _)| {
            bounds[0] >= stage[0]
                && bounds[2] <= stage[2]
                && bounds[1] > stage[3]
                && bounds[1] < stage[3] + band
                && bounds[2] - bounds[0] > (stage[2] - stage[0]) * 0.9
                && bounds[3] - bounds[1] < band * 0.1
        })
        .unwrap();
    let (at, layout) = nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. }
                if node.bounds().min().y() >= stage[3] && node.bounds().max().y() <= divider[1] =>
            {
                Some((node.bounds().min(), layout))
            }
            _ => None,
        })
        .unwrap();
    let ink = layout
        .glyphs()
        .iter()
        .filter(|glyph| !glyph.codepoint.is_whitespace())
        .fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |mut ink, glyph| {
                for axis in 0..2 {
                    ink[axis] = ink[axis].min(glyph.bounds_64[axis] as f32 / 64.0);
                    ink[axis + 2] = ink[axis + 2].max(glyph.bounds_64[axis + 2] as f32 / 64.0);
                }
                ink
            },
        );
    assert!(((ink[0] + ink[2]) * 0.5 + at.x() - (stage[0] + stage[2]) * 0.5).abs() < 0.1);
    assert!(((ink[1] + ink[3]) * 0.5 + at.y() - (stage[3] + divider[1]) * 0.5).abs() < 0.1);
    assert!(
        color[3] < 255,
        "the lower divider should blend gently into the panel"
    );
    assert!(divider[1] - at.y() - ink[3] >= band * 0.2);
}

#[test]
fn import_button_keeps_its_art_and_label_while_a_skin_change_is_pending() {
    let mut view = view(STARTER_SKIN_NAMES.len());
    let toolbar = [40.0, 40.0, 640.0, 720.0];
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        library::toolbar(canvas, &view, toolbar).unwrap();
    });
    let bounds = hits
        .iter()
        .find(|(action, _)| *action == command(Action::Import))
        .unwrap()
        .1;
    let contains = |b: [f32; 4]| {
        b[0] >= bounds.min().x() - 1.01
            && b[1] >= bounds.min().y()
            && b[2] <= bounds.max().x() + 1.01
            && b[3] <= bounds.max().y()
    };
    let mut previous = None;
    for busy in [false, true, false] {
        Arc::make_mut(&mut view.dressing_room).busy = busy;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            library::toolbar(canvas, &view, toolbar).unwrap();
        });
        assert_eq!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::Import)),
            !busy
        );
        let surfaces = super::super::test_support::solids(&nodes)
            .into_iter()
            .filter(|(b, _)| contains(*b))
            .collect::<Vec<_>>();
        let labels = nodes
            .iter()
            .filter_map(|node| match node.visual() {
                ui::UiVisual::Text { layout, color, .. }
                    if contains([
                        node.bounds().min().x(),
                        node.bounds().min().y(),
                        node.bounds().max().x(),
                        node.bounds().max().y(),
                    ]) =>
                {
                    Some((layout.key().content_sha256, *color))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let rendered = (surfaces, labels);
        if let Some(previous) = &previous {
            assert_eq!(
                &rendered, previous,
                "equipping a skin must not flash the import button"
            );
        }
        previous = Some(rendered);
    }
}

#[test]
fn signed_out_players_can_select_skins_and_import() {
    let view = view(STARTER_SKIN_NAMES.len());
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0]).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == command(Action::Import))
    );
    for index in 0..STARTER_SKIN_NAMES.len() {
        assert!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::Select(index)))
        );
    }
    assert!(!hits.iter().any(|(action, _)| matches!(
        action,
        MenuAction::DressingRoom(
            Action::SetModel(_) | Action::BeginRename(_) | Action::BeginDelete(_)
        )
    )));
    assert!(
        hits.iter()
            .all(|(_, bounds)| bounds.min().x() >= 0.0 && bounds.max().x() <= 1280.0)
    );
}

#[test]
fn scrolling_reaches_last_imported_skin_on_narrow_and_wide_screens() {
    let view = view(40);
    for size in [[1280.0, 720.0], [480.0, 720.0]] {
        let (scrolls, _, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, size).unwrap();
        });
        let area = scrolls
            .iter()
            .find(|area| area.max > 0.0)
            .expect("the library must scroll");
        let (_, hits, _) = paint(HashMap::from([(area.key.clone(), area.max)]), |canvas| {
            draw(canvas, &view, size).unwrap();
        });
        let (_, bounds) = hits
            .iter()
            .find(|(action, _)| *action == command(Action::Select(39)))
            .expect("the last imported skin remains reachable");
        assert!(bounds.min().y() >= area.viewport.min().y());
        assert!(bounds.max().y() <= area.viewport.max().y());
    }
}

#[test]
fn only_imported_skins_offer_a_model_choice_and_busy_state_blocks_changes() {
    let imported = STARTER_SKIN_NAMES.len();
    let mut view = view(imported + 1);
    for selected in [0, imported] {
        Arc::make_mut(&mut view.dressing_room).selected = Some(selected);
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        let model = hits
            .iter()
            .any(|(action, _)| matches!(action, MenuAction::DressingRoom(Action::SetModel(_))));
        assert_eq!(model, selected == imported);
        for action in [Action::BeginRename(selected), Action::BeginDelete(selected)] {
            assert_eq!(
                hits.iter()
                    .any(|(candidate, _)| *candidate == command(action)),
                selected == imported
            );
        }
    }
    Arc::make_mut(&mut view.dressing_room).busy = true;
    add_capes(&mut view);
    for section in [DressingRoomSection::Skins, DressingRoomSection::Capes] {
        Arc::make_mut(&mut view.dressing_room).section = section;
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        assert!(
            !hits
                .iter()
                .any(|(action, _)| matches!(action, MenuAction::DressingRoom(_)))
        );
    }
}

#[test]
fn character_stage_and_capture_end_above_details_and_management_controls() {
    for size in [[1280.0, 720.0], [480.0, 720.0]] {
        for (section, imported) in [
            (DressingRoomSection::Skins, false),
            (DressingRoomSection::Skins, true),
            (DressingRoomSection::Capes, true),
        ] {
            let mut view = view(STARTER_SKIN_NAMES.len() + 1);
            add_capes(&mut view);
            let wardrobe = Arc::make_mut(&mut view.dressing_room);
            wardrobe.section = section;
            wardrobe.selected = Some(if imported {
                STARTER_SKIN_NAMES.len()
            } else {
                0
            });
            wardrobe.selected_cape = Some(1);
            let name = if section == DressingRoomSection::Capes {
                wardrobe.selected_cape().unwrap().name.clone()
            } else {
                wardrobe.selected_skin().unwrap().name.clone()
            };
            let mut preview = None;
            let mut name_key = None;
            let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
                let request = canvas
                    .text_request(&name, 100_000, super::super::theme::BODY)
                    .unwrap();
                name_key = Some(canvas.layouts.layout(request).unwrap().key().content_sha256);
                preview = Some(draw(canvas, &view, size).unwrap());
            });
            let preview = preview.unwrap();
            assert!(preview.control[1] >= preview.clip[1]);
            assert!(preview.control[3] <= preview.clip[3]);
            let details = text_bounds(nodes, size)
                .into_iter()
                .filter(|(key, bounds)| {
                    *key == name_key.unwrap()
                        && bounds.min().x() >= preview.clip[0]
                        && bounds.min().x() < preview.clip[2]
                })
                .min_by(|(_, left), (_, right)| left.min().y().total_cmp(&right.min().y()))
                .expect("the selected character's name must be rendered in its details column")
                .1;
            assert!(
                preview.clip[3] < details.min().y(),
                "the character clip cannot enter its details"
            );
            for (action, bounds) in hits.iter().filter(|(action, _)| {
                matches!(
                    action,
                    MenuAction::DressingRoom(
                        Action::SetModel(_)
                            | Action::BeginRename(_)
                            | Action::BeginDelete(_)
                            | Action::BeginRenameCape(_)
                            | Action::BeginDeleteCape(_)
                    )
                )
            }) {
                assert!(
                    bounds.min().y() > preview.clip[3],
                    "{action:?} cannot overlap the character"
                );
            }
        }
    }
}

#[test]
fn skin_and_cape_card_captions_keep_padding_above_the_card_border() {
    for section in [DressingRoomSection::Skins, DressingRoomSection::Capes] {
        let mut view = view(STARTER_SKIN_NAMES.len());
        add_capes(&mut view);
        Arc::make_mut(&mut view.dressing_room).section = section;
        let mut minimum_padding = 0.0;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            minimum_padding = canvas.r(0.8);
            draw(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        let target = command(if section == DressingRoomSection::Skins {
            Action::Select(0)
        } else {
            Action::SelectCape(Some(0))
        });
        let card = hits
            .iter()
            .find(|(action, _)| *action == target)
            .expect("the first card must be visible")
            .1;
        let labels = text_bounds(nodes, [1280.0, 720.0])
            .into_iter()
            .filter(|(_, bounds)| {
                bounds.min().x() >= card.min().x()
                    && bounds.max().x() <= card.max().x()
                    && bounds.min().y() >= card.min().y()
                    && bounds.max().y() <= card.max().y()
            })
            .collect::<Vec<_>>();
        assert!(labels.len() >= 2, "a card retains its name and caption");
        let caption_bottom = labels
            .iter()
            .map(|(_, bounds)| bounds.max().y())
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            card.max().y() - caption_bottom >= minimum_padding - 0.01,
            "the {section:?} caption needs clear padding above its border"
        );
    }
}

#[test]
fn cape_tab_offers_no_cape_import_selection_and_imported_management() {
    let mut view = view(STARTER_SKIN_NAMES.len());
    add_capes(&mut view);
    for section in [DressingRoomSection::Skins, DressingRoomSection::Capes] {
        Arc::make_mut(&mut view.dressing_room).section = section;
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        for tab in [DressingRoomSection::Skins, DressingRoomSection::Capes] {
            assert_eq!(
                hits.iter()
                    .any(|(action, _)| *action == command(Action::SetSection(tab))),
                tab != section,
            );
        }
        assert!(hits.iter().any(|(action, _)| *action
            == command(if section == DressingRoomSection::Skins {
                Action::Import
            } else {
                Action::ImportCape
            })));
        assert_eq!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::SelectCape(None))),
            section == DressingRoomSection::Capes
        );
    }
    for selected in [Some(0), Some(1), None] {
        Arc::make_mut(&mut view.dressing_room).selected_cape = selected;
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            draw(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        for action in [Action::BeginRenameCape(1), Action::BeginDeleteCape(1)] {
            assert_eq!(
                hits.iter()
                    .any(|(candidate, _)| *candidate == command(action)),
                selected == Some(1)
            );
        }
        assert!(
            !hits
                .iter()
                .any(|(action, _)| matches!(action, MenuAction::DressingRoom(Action::SetModel(_))))
        );
    }
    let (scrolls, _, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0]).unwrap();
    });
    let offsets = scrolls
        .into_iter()
        .map(|area| (area.key, area.max))
        .collect();
    let (_, hits, _) = paint(offsets, |canvas| {
        draw(canvas, &view, [1280.0, 720.0]).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == command(Action::SelectCape(Some(1))))
    );
}

#[test]
fn editor_errors_remain_visible_inside_the_modal_without_covering_controls() {
    let message = "Could not save the wardrobe change. The collection file could not be written. Check that the destination is writable, then try again; your existing skin and cape are unchanged.";
    for target in [SkinEditorTarget::Skin, SkinEditorTarget::Cape] {
        for mode in [SkinEditorMode::Rename, SkinEditorMode::Delete] {
            for size in [[1280.0, 720.0], [480.0, 720.0]] {
                let mut view = view(STARTER_SKIN_NAMES.len() + 1);
                add_capes(&mut view);
                let wardrobe = Arc::make_mut(&mut view.dressing_room);
                wardrobe.editor = Some(SkinEditor {
                    index: if target == SkinEditorTarget::Cape {
                        1
                    } else {
                        STARTER_SKIN_NAMES.len()
                    },
                    mode,
                    draft: "My look".into(),
                    target,
                });
                wardrobe.message = Some(message.into());
                let mut error_key = None;
                let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
                    let request = canvas
                        .text_request(message, 100_000, super::super::theme::CAPTION)
                        .unwrap();
                    error_key = Some(canvas.layouts.layout(request).unwrap().key().content_sha256);
                    draw_overlay(canvas, &view, size, None).unwrap();
                });
                let surfaces = super::super::test_support::solids(&nodes);
                let texts = text_bounds(nodes, size);
                let error = texts
                    .iter()
                    .find(|(key, _)| *key == error_key.unwrap())
                    .expect("editor errors must be visible in the modal itself")
                    .1;
                assert!(error.min().x() >= 0.0 && error.min().y() >= 0.0);
                assert!(error.max().x() <= size[0] && error.max().y() <= size[1]);
                let panel = surfaces
                    .iter()
                    .find(|(bounds, _)| {
                        bounds[2] - bounds[0] < size[0]
                            && bounds[3] - bounds[1] < size[1]
                            && hits.iter().all(|(_, control)| {
                                bounds[0] <= control.min().x()
                                    && bounds[1] <= control.min().y()
                                    && bounds[2] >= control.max().x()
                                    && bounds[3] >= control.max().y()
                            })
                    })
                    .expect("the editor controls must have a visible modal panel")
                    .0;
                assert!(error.min().x() > panel[0] && error.min().y() > panel[1]);
                assert!(error.max().x() < panel[2] && error.max().y() < panel[3]);
                let overlaps = |other: ui::UiRect| {
                    error.min().x() < other.max().x()
                        && error.max().x() > other.min().x()
                        && error.min().y() < other.max().y()
                        && error.max().y() > other.min().y()
                };
                for (action, control) in hits {
                    assert!(
                        !overlaps(control),
                        "{target:?}/{mode:?} error cannot cover {action:?}"
                    );
                }
                for (key, other) in texts {
                    if key != error_key.unwrap() {
                        assert!(
                            !overlaps(other),
                            "the error must not cover the editor's other text"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cosmetics_offer_equip_remove_and_tabs_without_skin_management_actions() {
    for width in [360.0, 1280.0] {
        let mut view = view(STARTER_SKIN_NAMES.len());
        let wardrobe = Arc::make_mut(&mut view.dressing_room);
        wardrobe.section = DressingRoomSection::Cosmetics;
        wardrobe.selected_cosmetics.insert(
            "head".into(),
            Some(render_model::hats::fixture::bunny().id().into()),
        );
        let offsets = if width < 600.0 {
            HashMap::from([("dressing_room_hats".into(), 320.0)])
        } else {
            HashMap::new()
        };
        let (_, hits, _) = paint(offsets, |canvas| {
            super::draw(canvas, &view, [width, 720.0]).unwrap();
        });
        assert!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::SelectCosmetic(None)))
        );
        assert!(hits.iter().any(|(action, _)| *action
            == command(Action::SelectCosmetic(Some(
                render_model::hats::fixture::bunny()
            )))));
        assert!(
            hits.iter()
                .any(|(action, _)| *action
                    == command(Action::SetSection(DressingRoomSection::Skins)))
        );
        assert!(!hits.iter().any(|(action, _)| matches!(
            action,
            MenuAction::DressingRoom(
                Action::Import | Action::ImportCape | Action::SetModel(_) | Action::BeginRename(_)
            )
        )));
    }
}
