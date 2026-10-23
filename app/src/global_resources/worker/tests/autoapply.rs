use super::*;
use bevy::prelude::{App, Update};
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
        commands: Some(Arc::new(commands)),
        disk: None,
        picker: crossbeam_channel::unbounded().0,
        picker_open: false,
        events,
        snapshot: Snapshot::default(),
        awaiting: None,
        minecraft_import_pending: false,
        minecraft_import_request: None,
    })
    .insert_resource(menu)
    .init_resource::<crate::runtime::network::PackReload>()
    .add_message::<bevy::window::FileDragAndDrop>()
    .add_systems(Update, crate::global_resources::drive);
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
fn leaving_after_apply_keeps_the_in_flight_selection() {
    let (_fixture, events) = leave_events(&[
        MenuAction::GlobalResources(Action::Activate(0)),
        MenuAction::GlobalResources(Action::Apply),
        MenuAction::AddBack,
    ]);
    assert_eq!(reloads(&events), 1);
}

#[test]
fn older_acknowledgements_preserve_the_newer_in_flight_selection() {
    for failed in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("installed");
        let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
        import_fixture(&mut library, fixture.path(), 1);
        let events = collect_events(
            root,
            vec![
                Command::Commit(Vec::new()),
                Command::Action {
                    action: Action::Activate(0),
                    revision: 0,
                },
                Command::Leave,
                if failed {
                    Command::ApplyFailed(Vec::new())
                } else {
                    Command::Commit(Vec::new())
                },
                Command::Leave,
            ],
        );
        assert_eq!(reloads(&events), 1, "older failure: {failed}");
    }
}

#[test]
fn leaving_without_pending_changes_does_nothing() {
    let (_fixture, events) = leave_events(&[MenuAction::AddBack]);
    assert_eq!(reloads(&events), 0);
}

#[test]
fn late_picker_result_stays_pending_until_the_next_leave() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    library.activate(pack.id).unwrap();
    library.apply().unwrap();
    let before = library.active().to_vec();
    let events = collect_events(
        root,
        vec![
            Command::Commit(before.clone()),
            Command::Leave,
            Command::ImportFiles(Ok(vec![write_fixture(fixture.path(), 1)])),
            Command::Leave,
        ],
    );
    let imported = events
        .iter()
        .position(|event| matches!(event, Event::Snapshot(snapshot) if snapshot.revision == 1 && !snapshot.busy))
        .unwrap();
    assert_eq!(reloads(&events[..=imported]), 0);
    let Event::Snapshot(snapshot) = &events[imported] else {
        unreachable!();
    };
    assert_ne!(
        snapshot.selection, before,
        "reimport advances the active revision"
    );
    assert!(snapshot.has_pending_changes());
    let applied: Vec<_> = events[imported + 1..]
        .iter()
        .filter_map(|event| match event {
            Event::Apply(_, selection) => Some(selection),
            _ => None,
        })
        .collect();
    assert_eq!(applied, [&snapshot.selection]);
}
