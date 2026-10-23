use super::*;
use bevy::prelude::{App, AppExit, IntoScheduleConfigs, Last, ResMut, Update};
use client_shell::menu::MenuRuntime;
use launcher::menu::{MenuAction, MenuScreen, settings_sections::SettingSection};

#[test]
fn quitting_waits_for_exit_frame_edits_to_reach_disk_without_runtime_acknowledgement() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (outgoing, events) = crossbeam_channel::unbounded();
    let worker_root = root.clone();
    // Hold disk work until shutdown requests a drain or the app drops its sender.
    let disk = std::thread::spawn(move || {
        let (queued, batch) = crossbeam_channel::unbounded();
        while let Ok(command) = incoming.recv() {
            let flush = matches!(&command, Command::Flush(_));
            queued.send(command).unwrap();
            if flush {
                break;
            }
        }
        drop(queued);
        run(
            worker_root.clone(),
            worker_root.join("config"),
            Vec::new(),
            batch,
            outgoing,
        );
    });
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        SettingSection::GlobalTexturePack.definition().index,
    ));
    let mut app = App::new();
    app.insert_resource(Worker {
        commands,
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
    .add_systems(
        Last,
        (|mut menu: ResMut<MenuRuntime>| {
            menu.activate(MenuAction::GlobalResources(Action::Activate(0)));
        })
        .before(crate::global_resources::apply_on_leave),
    )
    .add_systems(Last, crate::global_resources::apply_on_leave);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    let persisted = GlobalPackLibrary::open(&root, engine_version())
        .unwrap()
        .active()
        .to_vec();
    drop(app);
    disk.join().unwrap();
    assert_eq!(
        persisted.len(),
        1,
        "shutdown returned before the edit was saved"
    );
    assert_eq!(persisted[0].id, pack.id);
}
