//! Dressing room input ownership and focus through the menu adapter.

use std::sync::Arc;

use super::super::super::UiPresentationRuntime;
use launcher::dressing_room::{
    Action, STARTER_SKIN_NAMES, SkinEditor, SkinEditorMode, SkinEditorTarget, SkinModel,
};
use launcher::menu::{MenuAction, MenuView};
use oreui::test_support::{add_capes, dressing_room_view as view};
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

fn command(action: Action) -> MenuAction {
    MenuAction::DressingRoom(action)
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
                .projections
                .menu_focus
                .contains(&command(Action::Select(0)))
        );
    }
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
                assert!(presentation.projections.menu_focus.contains(action));
            }
            assert!(
                hits.iter().all(|(action, _)| expected.contains(action)),
                "background cards and tabs cannot accept modal clicks"
            );
            assert!(
                presentation
                    .projections
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
                    .projections
                    .menu_focus
                    .iter()
                    .all(|action| *action == command(Action::Cancel))
            );
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
