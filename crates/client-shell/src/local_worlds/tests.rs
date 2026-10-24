use {
    super::*,
    launcher::local_worlds::model::{Input, Screen},
};

#[test]
fn focus_only_pauses_while_playing_and_detached_is_inert() {
    let mut worlds = LocalWorlds::default();
    worlds.focus_changed(false);
    worlds.pump();
    assert!(!worlds.paused, "not in a world");
    worlds.set_playing(true);
    assert!(worlds.paused, "joined while unfocused");
    worlds.focus_changed(true);
    assert!(!worlds.paused);
    worlds.leave_world();
    assert!(!worlds.playing);
}

/// The pause menu pauses a single-player world and closing it resumes, as in vanilla.
#[test]
fn the_pause_menu_pauses_and_resumes_the_world() {
    let mut worlds = LocalWorlds::default();
    worlds.set_pause_menu(true);
    assert!(!worlds.paused, "online play never pauses");
    worlds.set_pause_menu(false);
    worlds.set_playing(true);
    worlds.set_pause_menu(true);
    assert!(worlds.paused);
    worlds.focus_changed(false);
    worlds.set_pause_menu(false);
    assert!(worlds.paused, "still unfocused");
    worlds.focus_changed(true);
    assert!(!worlds.paused);
}

#[test]
fn input_without_a_client_still_advances_the_model() {
    let mut worlds = LocalWorlds::default();
    worlds.input(Input::BeginCreate);
    assert_eq!(worlds.menu().screen(), Screen::Create);
    worlds.input(Input::Back);
    worlds.input(Input::Refresh);
    assert!(worlds.menu().busy());
}

/// An import asked for before the world worker is attached is refused, not left on its modal.
#[test]
fn an_import_before_attach_is_refused() {
    let mut worlds = LocalWorlds::default();
    worlds.input(Input::Import);
    assert_eq!(worlds.menu().screen(), Screen::Error);
    assert!(!worlds.menu().busy());
    worlds.input(Input::Back);
    assert_eq!(worlds.menu().screen(), Screen::List);
}

/// A detach while an import is pending ends its modal instead of leaving it up for good.
#[test]
fn a_detach_mid_import_ends_the_import_modal() {
    let dir = tempfile::tempdir().expect("worlds folder");
    let manager =
        launcher_host::local_worlds::open_store(dir.path().join("worlds")).expect("manager");
    let mut worlds = LocalWorlds::default();
    worlds.client = Some(WorldsClient::spawn(manager).expect("worker"));
    worlds.input(Input::ImportFiles(vec![dir.path().join("Home.mcworld")]));
    assert_eq!(worlds.menu().screen(), Screen::Importing);
    worlds.detach();
    assert_eq!(worlds.menu().screen(), Screen::Error);
    assert!(!worlds.menu().busy());
}

/// Disabling focus pause preserves explicit pause-menu control.
#[test]
fn focus_preference_does_not_disable_explicit_pause() {
    let mut worlds = LocalWorlds::default();
    worlds.set_playing(true);
    worlds.focus_changed(false);
    assert!(worlds.paused);
    worlds.pause_on_unfocus = false;
    worlds.sync_pause();
    assert!(!worlds.paused);
    worlds.set_pause_menu(true);
    assert!(worlds.paused);
}
