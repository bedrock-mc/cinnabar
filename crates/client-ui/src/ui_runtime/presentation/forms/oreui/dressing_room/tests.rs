use std::{collections::HashMap, sync::Arc};

use super::super::review_tests::paint;
use super::*;
use crate::ui_runtime::presentation::{TextMetrics, UiPresentationRuntime, tests::fixture_font};
use launcher::dressing_room::{
    DressingRoomCape, DressingRoomSection, DressingRoomSkin, DressingRoomView, STARTER_SKIN_NAMES,
    SkinEditor, SkinEditorMode, SkinEditorTarget, SkinModel,
};

fn view(count: usize) -> MenuView {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = crate::menu::MenuScreen::DressingRoom;
    view.dressing_room = Arc::new(DressingRoomView {
        skins: (0..count)
            .map(|index| DressingRoomSkin {
                engine_version: protocol::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
                id: index.to_string(),
                name: STARTER_SKIN_NAMES
                    .get(index)
                    .map_or_else(|| format!("Skin {index}"), |name| (*name).into()),
                path: String::new(),
                imported: index >= STARTER_SKIN_NAMES.len(),
                model: if index == 1 {
                    SkinModel::Slim
                } else {
                    SkinModel::Classic
                },
                skin: protocol::StandardSkin {
                    width: protocol::CLASSIC_SKIN_SIDE as u32,
                    height: protocol::CLASSIC_SKIN_SIDE as u32,
                    rgba8: vec![255; protocol::CLASSIC_SKIN_SIDE.pow(2) * 4].into(),
                    cape: None,
                    geometry: None,
                },
            })
            .collect::<Vec<_>>()
            .into(),
        selected: Some(0),
        ..Default::default()
    });
    view
}

fn add_capes(view: &mut MenuView) {
    let (width, height) = protocol::CAPE_DIMENSIONS[0];
    Arc::make_mut(&mut view.dressing_room).capes = (0..2)
        .map(|index| DressingRoomCape {
            id: format!("cape-{index}"),
            name: format!("Cape {index}"),
            path: String::new(),
            cape: protocol::CapeImage {
                width,
                height,
                rgba8: vec![255; (width * height * 4) as usize].into(),
            },
            imported: index > 0,
        })
        .collect::<Vec<_>>()
        .into();
}

fn append(
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
    size: [f32; 2],
) -> Vec<(MenuAction, ui::UiRect)> {
    let metrics = TextMetrics::for_viewport(
        [size[0] as u32, size[1] as u32],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    presentation
        .append_oreui_screen(view, &mut Vec::new(), &mut 1, metrics, size, None, &|_| {
            None
        })
        .unwrap()
        .unwrap()
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
            crate::ui_runtime::presentation::rect(0.0, 0.0, size[0], size[1]).unwrap(),
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
    let baselines = super::super::review_tests::solids(&nodes)
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
    let (divider, color) = super::super::review_tests::solids(&nodes)
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
        let surfaces = super::super::review_tests::solids(&nodes)
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
fn keyboard_focus_reveals_an_offscreen_skin_without_losing_the_library() {
    let mut view = view(40);
    view.navigation_focus_visible = true;
    view.focused_action = Some(command(Action::Select(39)));
    for size in [[1280.0, 720.0], [480.0, 720.0]] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let hits = append(&mut presentation, &view, size);
        assert!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::Select(39)))
        );
        assert!(
            presentation
                .menu_scrolls
                .offsets()
                .values()
                .any(|offset| *offset > 0.0)
        );
        assert!(
            presentation
                .form_presentation
                .menu_focus
                .contains(&command(Action::Select(0)))
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
fn management_modals_own_all_click_focus_and_character_input() {
    for target in [SkinEditorTarget::Skin, SkinEditorTarget::Cape] {
        for mode in [SkinEditorMode::Rename, SkinEditorMode::Delete] {
            let mut view = view(STARTER_SKIN_NAMES.len() + 1);
            add_capes(&mut view);
            let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
            presentation.set_player_preview_skin(None, Default::default());
            presentation.menu_preview.begin_frame(Some(view.screen));
            append(&mut presentation, &view, [1280.0, 720.0]);
            let stage = presentation
                .menu_player_preview_bounds()
                .expect("the character must initially accept drags");
            let point = ui::UiPoint::new(
                (stage.min().x() + stage.max().x()) * 0.5,
                (stage.min().y() + stage.max().y()) * 0.5,
            )
            .unwrap();
            assert!(presentation.menu_player_preview_pointer(
                Some(view.screen),
                Some(point),
                true,
                true
            ));
            Arc::make_mut(&mut view.dressing_room).editor = Some(SkinEditor {
                index: if target == SkinEditorTarget::Cape {
                    1
                } else {
                    STARTER_SKIN_NAMES.len()
                },
                mode,
                draft: "My look".into(),
                target,
            });
            let hits = append(&mut presentation, &view, [1280.0, 720.0]);
            let mut expected = vec![
                command(Action::Cancel),
                command(if mode == SkinEditorMode::Rename {
                    Action::SaveRename
                } else {
                    Action::ConfirmDelete
                }),
            ];
            if mode == SkinEditorMode::Rename {
                expected.push(MenuAction::EditSkinName);
            }
            for action in &expected {
                assert!(
                    hits.iter().any(|(hit, _)| hit == action),
                    "the modal exposes {action:?}"
                );
                assert!(presentation.form_presentation.menu_focus.contains(action));
            }
            assert!(
                hits.iter().all(|(action, _)| expected.contains(action)),
                "background cards and tabs cannot accept modal clicks"
            );
            assert!(
                presentation
                    .form_presentation
                    .menu_focus
                    .iter()
                    .all(|action| expected.contains(action)),
                "keyboard focus stays in the modal"
            );
            assert!(presentation.menu_player_preview_bounds().is_none());
            assert!(
                !presentation.menu_player_preview_pointer(
                    Some(view.screen),
                    Some(point),
                    true,
                    false
                ),
                "opening a modal releases the existing character drag"
            );
            assert!(
                !presentation.menu_player_preview_pointer(
                    Some(view.screen),
                    Some(point),
                    true,
                    true
                ),
                "the covered character cannot capture a new press"
            );
            assert!(!presentation.menu_scrolls.wheel(point, -1.0, false));
        }
    }
}

#[test]
fn empty_rename_and_busy_modals_preserve_cancel_without_committing() {
    for (mode, busy, draft) in [
        (SkinEditorMode::Rename, false, "  "),
        (SkinEditorMode::Rename, true, "My look"),
        (SkinEditorMode::Delete, true, "My look"),
    ] {
        let mut view = view(STARTER_SKIN_NAMES.len() + 1);
        let wardrobe = Arc::make_mut(&mut view.dressing_room);
        wardrobe.busy = busy;
        wardrobe.editor = Some(SkinEditor {
            index: STARTER_SKIN_NAMES.len(),
            mode,
            draft: draft.into(),
            target: SkinEditorTarget::Skin,
        });
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let hits = append(&mut presentation, &view, [1280.0, 720.0]);
        assert!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::Cancel))
        );
        assert!(!hits.iter().any(|(action, _)| matches!(
            action,
            MenuAction::DressingRoom(Action::SaveRename | Action::ConfirmDelete)
        )));
        if busy {
            assert!(
                hits.iter()
                    .all(|(action, _)| *action == command(Action::Cancel)),
                "a busy editor cannot accept text edits"
            );
            assert!(
                presentation
                    .form_presentation
                    .menu_focus
                    .iter()
                    .all(|action| *action == command(Action::Cancel))
            );
        }
    }
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
                    draw_editor(canvas, &view, size).unwrap();
                });
                let surfaces = super::super::review_tests::solids(&nodes);
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
fn custom_skin_models_keep_item_edits_without_classic_arm_actions() {
    let mut view = view(STARTER_SKIN_NAMES.len() + 1);
    let selected = STARTER_SKIN_NAMES.len();
    let wardrobe = Arc::make_mut(&mut view.dressing_room);
    wardrobe.selected = Some(selected);
    Arc::make_mut(&mut wardrobe.skins)[selected].model = SkinModel::Custom;
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let hits = append(&mut presentation, &view, [1920.0, 1080.0]);
    assert!(
        !hits
            .iter()
            .any(|(action, _)| matches!(action, MenuAction::DressingRoom(Action::SetModel(_))))
    );
    for action in [Action::BeginRename(selected), Action::BeginDelete(selected)] {
        assert!(
            hits.iter()
                .any(|(candidate, _)| *candidate == command(action))
        );
    }
}
