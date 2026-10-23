use super::*;
use bevy::prelude::{App, AppExit, IntoScheduleConfigs, Last, ResMut, Update};
use client_shell::menu::MenuRuntime;
use launcher::menu::{MenuAction, MenuScreen, settings_sections::SettingSection};

/// Holds disk work until the command channel closes, making teardown responsible for saving it.
fn queued_worker(root: PathBuf) -> Worker {
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (outgoing, events) = crossbeam_channel::unbounded();
    let disk = std::thread::spawn(move || {
        let (queued, batch) = crossbeam_channel::unbounded();
        for command in incoming {
            queued.send(command).unwrap();
        }
        drop(queued);
        run(
            root.clone(),
            root.join("config"),
            Vec::new(),
            batch,
            outgoing,
        );
    });
    Worker {
        commands: Some(Arc::new(commands)),
        disk: Some(disk),
        picker: crossbeam_channel::unbounded().0,
        picker_open: false,
        events,
        snapshot: Snapshot::default(),
        awaiting: None,
        minecraft_import_pending: false,
        minecraft_import_request: None,
    }
}

#[test]
fn quitting_waits_for_exit_frame_edits_to_reach_disk_without_runtime_acknowledgement() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        SettingSection::GlobalTexturePack.definition().index,
    ));
    let mut app = App::new();
    app.insert_resource(queued_worker(root.clone()))
        .insert_resource(menu)
        .init_resource::<crate::runtime::network::PackReload>()
        .add_message::<bevy::window::FileDragAndDrop>()
        .add_systems(Update, crate::global_resources::drive)
        .add_systems(
            Last,
            (|mut menu: ResMut<MenuRuntime>| {
                menu.activate(MenuAction::GlobalResources(Action::Activate(0)));
            })
            .before(crate::global_resources::enqueue_exit_actions),
        )
        .add_systems(Last, crate::global_resources::enqueue_exit_actions);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    drop(app);
    let persisted = GlobalPackLibrary::open(&root, engine_version())
        .unwrap()
        .active()
        .to_vec();
    assert_eq!(
        persisted.len(),
        1,
        "shutdown returned before the edit was saved"
    );
    assert_eq!(persisted[0].id, pack.id);
}

#[test]
fn shutdown_saves_queued_edits_while_the_import_picker_remains_open() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    let (opened, opening) = crossbeam_channel::bounded(1);
    let (close, closing) = crossbeam_channel::bounded::<()>(1);
    let mut worker = Worker::new_with_picker(
        root.clone(),
        fixture.path().join("config"),
        Vec::new(),
        move || {
            opened.send(()).unwrap();
            let _ = closing.recv();
            Ok(Vec::new())
        },
    );
    worker.send(Command::Action {
        action: Action::Import,
        revision: 0,
    });
    test_time::eventually("the open import picker", || opening.try_recv().is_ok());
    worker.send(Command::Action {
        action: Action::Activate(0),
        revision: 0,
    });
    let (done, completed) = crossbeam_channel::bounded(1);
    let drain = std::thread::spawn(move || {
        drop(worker);
        done.send(()).unwrap();
    });
    // Closing the gate on failure lets the worker finish without a stranded thread.
    let result = std::panic::catch_unwind(|| {
        test_time::eventually("disk writes while the picker is open", || {
            completed.try_recv().is_ok()
        });
        assert_eq!(
            GlobalPackLibrary::open(&root, engine_version())
                .unwrap()
                .active()[0]
                .id,
            pack.id
        );
    });
    drop(close);
    drain.join().unwrap();
    result.unwrap();
}

#[test]
fn teardown_without_app_exit_saves_queued_edits() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    let mut worker = queued_worker(root.clone());
    worker.send(Command::Action {
        action: Action::Activate(0),
        revision: 0,
    });
    let mut app = App::new();
    app.insert_resource(worker);
    app.world_mut().clear_all();
    let selection = GlobalPackLibrary::open(&root, engine_version())
        .unwrap()
        .active()
        .to_vec();
    assert_eq!(
        selection.len(),
        1,
        "teardown returned before saving the edit"
    );
    assert_eq!(selection[0].id, pack.id);
}
