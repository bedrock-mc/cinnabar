//! Whole-card disclosure targets through the menu presentation adapter.

use std::sync::Arc;

use super::super::super::UiPresentationRuntime;
use super::focus::paint;
use launcher::menu::MenuAction;
use launcher::mods::{Action, ActiveMod, InstalledMod, Status};
use oreui::test_support::settings_view;
use ui_engine::test_support::fixture_font;

#[test]
fn mod_headers_share_pointer_and_navigation_activation_and_keep_focus_when_closed() {
    for gamepad in [false, true] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let mut view = settings_view(launcher::mods::SECTION);
        view.gamepad_input = gamepad;
        view.navigation_focus_visible = true;
        let snapshot = Arc::make_mut(&mut view.mods);
        snapshot.library.installed = (1..=2)
            .map(|id| InstalledMod {
                id,
                file: format!("mod-{id}.wasm"),
                bytes: 1,
                permissions: Vec::new(),
                metadata: Default::default(),
            })
            .collect();
        snapshot.library.active.push(ActiveMod {
            id: 1,
            status: Status::Running,
        });
        for id in [1, 2] {
            let header = MenuAction::Mods(Action::ReadMore(id));
            view.focused_action = Some(header);
            for expanded in [false, true, false] {
                if expanded || view.mods.details_expanded.is_some() {
                    Arc::make_mut(&mut view.mods).apply(Action::ReadMore(id));
                }
                // Observe both the initial animation frame and its settled state.
                for _ in 0..2 {
                    paint(&mut presentation, &view, [1280.0, 900.0]);
                    let (targets, _) = presentation.settings_focus_geometry();
                    let target = targets
                        .iter()
                        .find(|target| target.action == header)
                        .expect("the header remains the navigation target");
                    let bounds = presentation
                        .menu_hit_targets
                        .iter()
                        .find_map(|(action, bounds)| (*action == header).then_some(*bounds))
                        .expect("the entire header remains clickable");
                    assert_eq!(target.bounds, bounds);
                    for x in [
                        bounds.min().x() + 1.0,
                        (bounds.min().x() + bounds.max().x()) * 0.5,
                        bounds.max().x() - 1.0,
                    ] {
                        let y = (bounds.min().y() + bounds.max().y()) * 0.5;
                        assert_eq!(
                            presentation.hit_test_menu(ui::UiPoint::new(x, y).unwrap()),
                            Some(header)
                        );
                    }
                    if !expanded {
                        assert!(!targets.iter().any(|target| matches!(
                            target.action,
                            MenuAction::Mods(
                                Action::Enable(_)
                                    | Action::Disable(_)
                                    | Action::Reload(_)
                                    | Action::Remove(_)
                                    | Action::Customize(_)
                            )
                        )));
                        assert!(
                            !presentation
                                .menu_hit_targets
                                .iter()
                                .any(|(action, _)| matches!(
                                    action,
                                    MenuAction::Mods(
                                        Action::Enable(_)
                                            | Action::Disable(_)
                                            | Action::Reload(_)
                                            | Action::Remove(_)
                                            | Action::Customize(_)
                                    )
                                ))
                        );
                    }
                    presentation.menu_seconds += 1.0;
                }
                let remove = MenuAction::Mods(Action::Remove(id));
                assert_eq!(
                    presentation
                        .settings_focus_geometry()
                        .0
                        .iter()
                        .any(|target| target.action == remove),
                    expanded
                );
            }
        }
    }
}
