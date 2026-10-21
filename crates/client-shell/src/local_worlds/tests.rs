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
    worlds.client =
        Some(WorldsClient::spawn(manager, dir.path().join("no-core-here")).expect("worker"));
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

/// A catalog process stuck on a pipe cannot occupy the world worker or delay cancellation.
#[cfg(unix)]
#[test]
fn stalled_discovery_does_not_delay_saved_worlds() {
    use bridge::{Backend, WorldState};
    use launcher::local_worlds::world::NewWorld;
    use std::{fs, io::Write, os::unix::fs::PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("worlds");
    let store = launcher_host::local_worlds::open_store(root.clone()).unwrap();
    let world = store
        .create(&NewWorld {
            name: "Saved".into(),
            backend: Some(Backend::Dragonfly),
            ..NewWorld::default()
        })
        .unwrap();
    drop(store);
    let binary = dir.path().join("server");
    let fifo = binary.with_extension("gate");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    // Holding both ends keeps opening the test gate from waiting on discovery.
    let mut gate = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fifo)
        .unwrap();
    fs::write(&binary, b"#!/bin/sh\nif [ \"$1\" = -list-generators ]; then\n  echo called >> \"$0.calls\"\n  : > \"$0.discovering\"\n  IFS= read -r release < \"$0.gate\"\n  echo '[{\"id\":\"normal\",\"label\":\"Normal\"}]'\n  exit 0\nfi\n: > \"$0.started\"\nif [ -f \"$0.ready\" ]; then echo ready; fi\nwhile IFS= read -r line; do [ \"$line\" = stop ] && exit 0; done\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let manager =
        launcher_host::local_worlds::open_store_with_server(root, binary.clone()).unwrap();
    let mut worlds = LocalWorlds::default();
    worlds.manager = Some(manager.clone());
    worlds.client = Some(WorldsClient::spawn(manager.clone(), dir.path().join("no-core")).unwrap());
    test_time::eventually("discovery to stall", || {
        binary.with_extension("discovering").exists()
    });
    assert!(manager.generators().is_none());
    worlds.menu.apply(Event::Listed(vec![world]));
    worlds.input(Input::BeginCreate);
    worlds.pump();
    assert_eq!(
        worlds.menu.view().generator_message(),
        Some(launcher::local_worlds::generators::LOADING_GENERATORS)
    );
    assert!(!worlds.menu.view().can_create());
    worlds.input(Input::Back);

    fs::write(binary.with_extension("ready"), b"").unwrap();
    worlds.input(Input::Play);
    test_time::eventually("saved world to run during discovery", || {
        manager.status().state == WorldState::Running
    });
    worlds.input(Input::Back);
    test_time::eventually("saved world to close during discovery", || {
        manager.status().state == WorldState::Idle
    });
    fs::remove_file(binary.with_extension("ready")).unwrap();
    fs::remove_file(binary.with_extension("started")).unwrap();
    worlds.input(Input::Play);
    test_time::eventually("saved world to start during discovery", || {
        binary.with_extension("started").exists()
    });
    assert_eq!(manager.status().state, WorldState::Starting);
    worlds.input(Input::Back);
    test_time::eventually("saved world startup to cancel during discovery", || {
        manager.status().state == WorldState::Idle
    });
    assert!(manager.generators().is_none());
    worlds.detach();
    gate.write_all(b"release\n").unwrap();
    test_time::eventually("startup catalog to resolve", || {
        manager.generators().is_some()
    });
    worlds.pump();
    assert!(!worlds.menu.generators_pending());
    assert_eq!(
        fs::read_to_string(binary.with_extension("calls")).unwrap(),
        "called\n"
    );
}
