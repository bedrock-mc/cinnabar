use super::*;
use crate::menu::{MenuScreen, MenuView};
use crate::ui_runtime::UiRuntime;
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
        let shapes = presentation.layouts.len();
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
        assert_eq!(presentation.layouts.len(), shapes);
    }
}
