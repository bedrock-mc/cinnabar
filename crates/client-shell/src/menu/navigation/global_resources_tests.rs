use super::super::MenuRuntime;
use launcher::{
    global_resources::{Action, Snapshot},
    menu::{MenuAction, MenuScreen, settings_sections::SettingSection},
};
use std::sync::Arc;

/// Opens Global Resources with an acknowledged selection that has been cleared.
fn pending_menu() -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        SettingSection::GlobalTexturePack.definition().index,
    ));
    menu.global_resources = Arc::new(Snapshot {
        applied_selection: Some(vec![resource_pack::ActivePack {
            id: "11111111-1111-1111-1111-111111111111".parse().unwrap(),
            subpack: String::new(),
            revision: 0,
        }]),
        ..Snapshot::default()
    });
    menu
}

#[test]
fn leaving_global_resources_sends_the_apply_button_command_once_when_available() {
    let mut button = pending_menu();
    button.activate(MenuAction::GlobalResources(Action::Apply));
    assert_eq!(button.global_resource_actions, [Action::Apply]);
    let routes: &[fn(&mut MenuRuntime)] = &[
        |menu| menu.activate(MenuAction::AddBack),
        |menu| menu.go_back(),
        |menu| menu.activate(MenuAction::Navigate(MenuScreen::Home)),
        |menu| {
            menu.activate(MenuAction::SettingsSection(
                SettingSection::Video.definition().index,
            ))
        },
        |menu| {
            menu.activate(MenuAction::SettingsSearchSection(
                SettingSection::Video.definition().index,
            ))
        },
        |menu| menu.activate(MenuAction::Mods(launcher::mods::Action::ViewUpdates)),
        |menu| menu.set_visible(false),
        |menu| menu.show_home(),
        |menu| menu.show_world(),
        |menu| menu.show_connecting(),
    ];
    for (route, leave) in routes.iter().enumerate() {
        for (state, enabled) in [(0, true), (1, false), (2, false), (3, false)] {
            let mut menu = pending_menu();
            let snapshot = Arc::make_mut(&mut menu.global_resources);
            match state {
                1 => snapshot.busy = true,
                2 => snapshot.applied_selection = Some(snapshot.selection.clone()),
                3 => snapshot.applied_selection = None,
                _ => {}
            }
            leave(&mut menu);
            menu.activate(MenuAction::Navigate(MenuScreen::Home));
            assert_eq!(
                menu.global_resource_actions,
                if enabled {
                    button.global_resource_actions.clone()
                } else {
                    Vec::new()
                },
                "route {route}, state {state}",
            );
        }
    }
}

#[test]
fn staying_in_global_resources_does_not_apply() {
    let mut menu = pending_menu();
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        SettingSection::GlobalTexturePack.definition().index,
    ));
    Arc::make_mut(&mut menu.global_resources).settings = Some(0);
    menu.activate(MenuAction::AddBack);
    assert_eq!(menu.scene(), Some(MenuScreen::Settings));
    assert_eq!(menu.global_resource_actions, [Action::CloseSettings]);
}
