use super::*;
use bevy::prelude::{App, Last, Update};
use client_shell::menu::MenuRuntime;
use launcher::menu::{MenuAction, MenuScreen, settings_sections::SettingSection};

/// Drives menu transitions with queued disk work so no runtime reload can finish.
fn leave_events(actions: &[MenuAction]) -> (tempfile::TempDir, Vec<Event>) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    import_fixture(&mut library, fixture.path(), 1);
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (_outgoing, events) = crossbeam_channel::unbounded();
    commands.send(Command::Commit(Vec::new())).unwrap();
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        SettingSection::GlobalTexturePack.definition().index,
    ));
    let mut app = App::new();
    app.insert_resource(Worker {
        commands,
        picker: crossbeam_channel::unbounded().0,
        events,
        snapshot: Snapshot::default(),
        awaiting: None,
        minecraft_import_pending: false,
        minecraft_import_request: None,
    })
    .insert_resource(menu)
    .init_resource::<crate::runtime::network::PackReload>()
    .add_message::<bevy::window::FileDragAndDrop>()
    .add_systems(Update, crate::global_resources::drive)
    .add_systems(Last, crate::global_resources::apply_on_leave);
    app.update();
    for action in actions {
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(*action);
    }
    app.update();
    app.update();
    drop(app);
    let (outgoing, events) = crossbeam_channel::unbounded();
    run(
        root.clone(),
        root.join("config"),
        Vec::new(),
        incoming,
        outgoing,
    );
    (fixture, events.try_iter().collect())
}

/// Counts stack submissions after the worker's initial startup selection.
fn reloads(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::Apply(..)))
        .count()
        - 1
}

#[test]
fn leaving_with_pending_changes_applies_once() {
    for leave in [
        MenuAction::AddBack,
        MenuAction::SettingsSection(SettingSection::Video.definition().index),
        MenuAction::Navigate(MenuScreen::Home),
    ] {
        let (_fixture, events) =
            leave_events(&[MenuAction::GlobalResources(Action::Activate(0)), leave]);
        assert_eq!(reloads(&events), 1, "{leave:?}");
    }
}

#[test]
fn leaving_without_pending_changes_does_nothing() {
    let (_fixture, events) = leave_events(&[MenuAction::AddBack]);
    assert_eq!(reloads(&events), 0);
}

#[test]
fn apply_then_leave_submits_only_once() {
    let (_fixture, events) = leave_events(&[
        MenuAction::GlobalResources(Action::Activate(0)),
        MenuAction::GlobalResources(Action::Apply),
        MenuAction::AddBack,
    ]);
    assert_eq!(reloads(&events), 1);
}

#[test]
fn leaving_applies_deactivation_and_reordering_once() {
    for action in [Action::Deactivate(0), Action::MoveDown(0)] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("installed");
        let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
        for id in [1, 2] {
            let pack = import_fixture(&mut library, fixture.path(), id);
            library.activate(pack.id).unwrap();
        }
        library.apply().unwrap();
        let before = library.active().to_vec();
        let events = collect_events(
            root,
            vec![
                Command::Commit(before.clone()),
                Command::Action {
                    action,
                    revision: 0,
                },
                Command::Leave,
                Command::Leave,
            ],
        );
        assert_eq!(reloads(&events), 1);
        let after = events
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::Apply(_, selection) => Some(selection),
                _ => None,
            })
            .unwrap();
        let mut expected = before;
        match action {
            Action::Deactivate(_) => {
                expected.remove(0);
            }
            Action::MoveDown(_) => expected.swap(0, 1),
            _ => unreachable!(),
        }
        assert_eq!(*after, expected);
    }
}

#[test]
fn leaving_after_an_acknowledged_apply_does_not_reload() {
    let events = events_for(
        "leave-acknowledged",
        vec![
            Command::Action {
                action: Action::Apply,
                revision: 0,
            },
            Command::Commit(Vec::new()),
            Command::Leave,
        ],
    );
    assert_eq!(reloads(&events), 1);
}

#[test]
fn leaving_retries_a_failed_apply_of_pending_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    library.activate(pack.id).unwrap();
    let selection = library.active().to_vec();
    let events = collect_events(
        root,
        vec![
            Command::Commit(Vec::new()),
            Command::Action {
                action: Action::Activate(0),
                revision: 0,
            },
            Command::Action {
                action: Action::Apply,
                revision: 1,
            },
            Command::ApplyFailed(selection),
            Command::Leave,
        ],
    );
    assert_eq!(reloads(&events), 2);
}

#[test]
fn leaving_applies_corrected_selection_after_initial_preview_failure() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    library.activate(pack.id).unwrap();
    library.apply().unwrap();
    for file in std::fs::read_dir(&root).unwrap() {
        let path = file.unwrap().path();
        if resource_pack::is_pack_import_path(&path) {
            std::fs::remove_file(path).unwrap();
        }
    }
    assert!(library.preview().is_err());
    let events = collect_events(
        root,
        vec![
            Command::Action {
                action: Action::Deactivate(0),
                revision: 0,
            },
            Command::Leave,
            Command::Leave,
        ],
    );
    let submissions: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Apply(_, selection) => Some(selection.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(submissions, [Vec::<resource_pack::ActivePack>::new()]);
}

#[test]
fn leaving_retries_after_initial_runtime_failure_without_an_acknowledgement() {
    let events = events_for(
        "leave-initial-failure",
        vec![
            Command::ApplyFailed(Vec::new()),
            Command::Leave,
            Command::Leave,
        ],
    );
    assert_eq!(reloads(&events), 1);
}
