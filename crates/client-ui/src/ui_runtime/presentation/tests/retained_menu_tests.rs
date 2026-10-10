use super::*;
use crate::ui_runtime::UiRuntime;
use launcher::menu::{MenuScreen, MenuView};
use ui::DpiScale;

#[test]
fn settled_launcher_menus_do_not_allocate_rebuild_or_republish_unchanged_input() {
    for screen in [MenuScreen::Home, MenuScreen::Servers, MenuScreen::Settings] {
        let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation()
        else {
            eprintln!(
                "skipping settled_launcher_menus_do_not_allocate_rebuild_or_republish_unchanged_input: missing local UI carrier (make assets)"
            );
            return;
        };
        let runtime = UiRuntime::new(0);
        let player = player_state::PlayerState::new(0);
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = screen;
        presentation.set_menu_view(Some(view));
        let frame = |presentation: &mut UiPresentationRuntime, millis| {
            presentation
                .build(
                    &player,
                    &runtime,
                    millis,
                    [1280, 720],
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap()
        };
        for millis in [0, 500, 1000] {
            frame(&mut presentation, millis);
        }
        let before = frame(&mut presentation, 1500);
        let trees = presentation.tree_builds;
        let paints = presentation.oreui_paints;
        let shapes = presentation.layouts.built_layout_count();
        let (_, allocations) = crate::allocation_count::count(|| {
            for millis in 1600..1610 {
                let after = frame(&mut presentation, millis);
                assert_eq!(after.revision, before.revision);
                assert!(Arc::ptr_eq(&after.vertices, &before.vertices));
                assert!(Arc::ptr_eq(&after.textures, &before.textures));
            }
        });
        assert_eq!(allocations, 0, "{screen:?}");
        assert_eq!(presentation.tree_builds, trees);
        assert_eq!(presentation.oreui_paints, paints);
        assert_eq!(presentation.layouts.built_layout_count(), shapes);
    }
}

#[test]
fn retained_launcher_frames_update_for_focus_viewport_and_open_dialogs() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping retained_launcher_frames_update_for_focus_viewport_and_open_dialogs: missing local UI carrier (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(0);
    let player = player_state::PlayerState::new(0);
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Servers;
    presentation.set_menu_view(Some(view.clone()));
    let dpi = DpiScale::new(1.0).unwrap();
    for millis in [0, 500, 1000] {
        presentation
            .build(&player, &runtime, millis, [1280, 720], dpi)
            .unwrap();
    }
    let idle = presentation
        .build(&player, &runtime, 1500, [1280, 720], dpi)
        .unwrap();
    view.focused_action = Some(launcher::menu::MenuAction::PlayAddServer);
    view.navigation_focus_visible = true;
    presentation.set_menu_view(Some(view.clone()));
    let paints = presentation.oreui_paints;
    presentation
        .build(&player, &runtime, 2000, [1280, 720], dpi)
        .unwrap();
    assert!(
        presentation.oreui_paints > paints,
        "focus changes repaint immediately"
    );
    let focused = presentation
        .build(&player, &runtime, 2100, [1280, 720], dpi)
        .unwrap();
    assert!(focused.revision > idle.revision);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .any(|(action, _)| *action == launcher::menu::MenuAction::PlayAddServer)
    );
    let resized = presentation
        .build(&player, &runtime, 2500, [1600, 900], dpi)
        .unwrap();
    assert!(resized.revision > focused.revision);
    view.dialog = Some(launcher::menu::MenuDialog::Exit);
    presentation.set_menu_view(Some(view));
    let paints = presentation.oreui_paints;
    presentation
        .build(&player, &runtime, 3000, [1600, 900], dpi)
        .unwrap();
    assert!(
        presentation.oreui_paints > paints,
        "opening a dialog repaints immediately"
    );
    let dialog = presentation
        .build(&player, &runtime, 3500, [1600, 900], dpi)
        .unwrap();
    assert!(dialog.revision > resized.revision);
    assert!(
        !presentation
            .menu_hit_targets
            .iter()
            .any(|(action, _)| *action == launcher::menu::MenuAction::PlayAddServer)
    );
}

#[test]
fn remembering_an_owned_menu_snapshot_does_not_allocate() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping remembering_an_owned_menu_snapshot_does_not_allocate: missing local UI carrier (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(0);
    let player = player_state::PlayerState::new(0);
    let mut view = MenuView::new(true, "Owned snapshot".repeat(1024));
    view.screen = MenuScreen::Settings;
    presentation.set_menu_view(Some(view));
    let dpi = DpiScale::new(1.0).unwrap();
    for clock in [0, 500, 1000, 1500] {
        presentation
            .build(&player, &runtime, clock, [1280, 720], dpi)
            .unwrap();
    }
    presentation.retained_menu = None;
    presentation.menu_scrolls = Default::default();
    let frame = ([1280, 720], dpi.get(), presentation.safe_area);
    let (_, allocations) = crate::allocation_count::count(|| {
        presentation.remember_menu(&runtime, frame);
    });
    assert!(
        presentation.retained_menu.is_some(),
        "a completed settled frame is retained"
    );
    assert_eq!(
        allocations, 0,
        "retaining a published snapshot shares its owned data"
    );
}

#[test]
fn owned_menu_snapshot_caret_keeps_blinking_and_restarts_after_edit() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping owned_menu_snapshot_caret_keeps_blinking_and_restarts_after_edit: missing local UI carrier (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(0);
    let player = player_state::PlayerState::new(0);
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::AddServer;
    view.field = Some(launcher::menu::MenuField::Name);
    view.name = "Caret".into();
    presentation.set_menu_view(Some(view.clone()));
    let dpi = DpiScale::new(1.0).unwrap();
    let blink = (json_ui::CARET_BLINK_SECONDS * 1000.0) as u64 + 1;
    for (clock, shown) in [(0, true), (blink, false), (2 * blink, true)] {
        presentation
            .build(&player, &runtime, clock, [1280, 720], dpi)
            .unwrap();
        assert_eq!(presentation.menu_view.as_ref().unwrap().caret.shown, shown);
        assert!(
            presentation.retained_menu.is_none(),
            "focused text remains live"
        );
    }
    let mut edited = view;
    edited.caret.revision += 1;
    presentation.set_menu_view(Some(edited));
    presentation
        .build(&player, &runtime, 3 * blink, [1280, 720], dpi)
        .unwrap();
    assert!(
        presentation.menu_view.as_ref().unwrap().caret.shown,
        "an edit restarts the blink"
    );
}

#[test]
fn publication_updates_changed_menu_icons_without_copying_unchanged_snapshots() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping publication_updates_changed_menu_icons_without_copying_unchanged_snapshots: missing local UI carrier (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(0);
    let player = player_state::PlayerState::new(0);
    let mut view = MenuView::new(true, "Owned profile".repeat(128));
    view.screen = MenuScreen::Settings;
    view.profile_icon = Some(ui::IconRef {
        page: presentation.solid_texture_page,
        uv: [0, 0, 1, 1],
        glint: false,
    });
    presentation.set_menu_view(Some(view));
    let dpi = DpiScale::new(1.0).unwrap();
    for clock in [0, 500, 1000, 1500] {
        presentation
            .build(&player, &runtime, clock, [1280, 720], dpi)
            .unwrap();
    }
    let paints = presentation.oreui_paints;
    for clock in [2000, 2500, 3000] {
        let prepared = PendingUiPublication {
            inventory: runtime.capture_presentation_inventory(&player),
            preview: PreviewCapture {
                ready: true,
                skin: None,
                pose: Default::default(),
                shown: false,
                hands: false,
            },
            item_icons: (None, None),
            now_millis: clock,
            physical_size: [1280, 720],
            dpi_scale: dpi,
        };
        render_prepared_ui(&player, &mut runtime, &mut presentation, prepared).unwrap();
    }
    assert_eq!(
        presentation.menu_view.as_ref().unwrap().profile_icon,
        presentation.player_preview_icon()
    );
    assert_ne!(
        presentation.menu_view.as_ref().unwrap().profile_icon,
        Some(ui::IconRef {
            page: presentation.solid_texture_page,
            uv: [0, 0, 1, 1],
            glint: false,
        })
    );
    assert!(
        presentation.oreui_paints > paints,
        "a changed profile icon repaints the menu"
    );
    assert!(
        presentation.retained_menu.is_some(),
        "the changed frame settles"
    );
    let name = presentation
        .menu_view
        .as_ref()
        .unwrap()
        .display_name
        .as_ptr();
    let paints = presentation.oreui_paints;
    let prepared = PendingUiPublication {
        inventory: runtime.capture_presentation_inventory(&player),
        preview: PreviewCapture {
            ready: true,
            skin: None,
            pose: Default::default(),
            shown: false,
            hands: false,
        },
        item_icons: (None, None),
        now_millis: 3500,
        physical_size: [1280, 720],
        dpi_scale: dpi,
    };
    render_prepared_ui(&player, &mut runtime, &mut presentation, prepared).unwrap();
    assert_eq!(
        presentation
            .menu_view
            .as_ref()
            .unwrap()
            .display_name
            .as_ptr(),
        name,
        "an unchanged icon keeps the owned menu allocation"
    );
    assert_eq!(presentation.oreui_paints, paints);
}

#[test]
fn publishing_equal_owned_menu_snapshots_does_not_allocate() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let view = MenuView::new(true, "Published snapshot".repeat(256));
    presentation.set_menu_view(Some(view.clone()));
    let (_, allocations) = crate::allocation_count::count(|| {
        presentation.set_menu_view(Some(view));
    });
    assert_eq!(
        allocations, 0,
        "an equal incoming snapshot keeps its existing owned allocation"
    );
    let changed = MenuView::new(true, "Changed snapshot".into());
    presentation.set_menu_view(Some(changed));
    assert_eq!(
        presentation.menu_view.as_ref().unwrap().display_name,
        "Changed snapshot"
    );
    presentation.set_menu_view(None);
    assert!(
        presentation.menu_view.is_none(),
        "clearing a view still removes it"
    );
}

#[test]
fn retained_menu_is_invalidated_by_presentation_owned_experience_modal() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping retained_menu_is_invalidated_by_presentation_owned_experience_modal: missing local UI carrier (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(0);
    let player = player_state::PlayerState::new(0);
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Settings;
    presentation.set_menu_view(Some(view));
    let dpi = DpiScale::new(1.0).unwrap();
    for clock in [0, 500, 1000, 1500] {
        presentation
            .build(&player, &runtime, clock, [1280, 720], dpi)
            .unwrap();
    }
    assert!(presentation.retained_menu.is_some());
    let mut modal = server_experience::screen::Modal::default();
    modal.open(Some("ui/fixture.json".into()));
    let files = Arc::new(server_experience::screen::Files::default());
    presentation.set_experience_modal(Some(
        super::super::forms::experience_modal::ExperienceModal {
            bundle: "fixture",
            files: &files,
            modal: &modal,
        },
    ));
    let frame = ([1280, 720], dpi.get(), presentation.safe_area);
    assert!(
        presentation.retained_menu_input(&runtime, frame).is_none(),
        "an opening modal must be drawn"
    );
    presentation.remember_menu(&runtime, frame);
    assert!(
        presentation.retained_menu.is_none(),
        "modal frames remain live"
    );
    presentation.set_experience_modal(None);
    presentation.remember_menu(&runtime, frame);
    assert!(
        presentation.retained_menu.is_some(),
        "closing the modal restores retention"
    );
}
