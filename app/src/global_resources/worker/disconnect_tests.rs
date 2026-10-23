//! Worker loss must finish queued menu requests even when the send succeeded.

use super::*;
use bevy::prelude::{App, Update};
use client_shell::menu::MenuRuntime;
use launcher::menu::{MenuAction, MenuDialog};
use launcher::minecraft_import::{Action as ImportAction, Phase};

/// Connects a real menu driver to channels whose worker lifetime the test controls.
fn fixture() -> (
    App,
    crossbeam_channel::Receiver<Command>,
    crossbeam_channel::Sender<Event>,
) {
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (outgoing, events) = crossbeam_channel::unbounded();
    let worker = Worker {
        commands: Some(Arc::new(commands)),
        disk: None,
        picker: crossbeam_channel::unbounded().0,
        picker_open: false,
        events,
        snapshot: Snapshot::default(),
        awaiting: None,
        minecraft_import_pending: false,
        minecraft_import_request: None,
    };
    let mut app = App::new();
    app.insert_resource(worker)
        .insert_resource(MenuRuntime::new(true, 2, "Import fixture".into()))
        .init_resource::<crate::runtime::network::PackReload>()
        .add_message::<bevy::window::FileDragAndDrop>()
        .add_systems(Update, crate::global_resources::drive);
    (app, incoming, outgoing)
}

#[test]
fn an_open_import_picker_disables_import_and_rejects_duplicate_requests() {
    let (mut app, _incoming, outgoing) = fixture();
    let (picker, requests) = crossbeam_channel::unbounded();
    app.world_mut().resource_mut::<Worker>().picker = picker;
    for _ in 0..2 {
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::GlobalResources(Action::Import));
        app.update();
    }
    assert_eq!(requests.len(), 1, "only one picker may be outstanding");
    assert!(app.world().resource::<MenuRuntime>().global_resources.busy);
    outgoing.send(Event::Snapshot(Snapshot::default())).unwrap();
    app.update();
    assert!(
        app.world().resource::<MenuRuntime>().global_resources.busy,
        "disk completion must not clear picker activity"
    );
    outgoing.send(Event::PickerClosed).unwrap();
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().global_resources.busy);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::GlobalResources(Action::Import));
    app.update();
    assert_eq!(requests.len(), 2, "a later import can open another picker");
    drop(outgoing);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().global_resources.busy);
}

#[test]
fn queued_import_requests_finish_when_library_initialization_fails() {
    for action in [ImportAction::Begin, ImportAction::Close] {
        let (mut app, incoming, outgoing) = fixture();
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::MinecraftImport(action));
        app.update();
        assert_eq!(
            app.world()
                .resource::<MenuRuntime>()
                .view()
                .minecraft_import
                .phase,
            Phase::Busy
        );
        assert_eq!(
            incoming.len(),
            1,
            "request was accepted before initialization finished"
        );
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("global_packs.json"), b"malformed").unwrap();
        run(
            root.path().to_owned(),
            root.path().join("config"),
            Vec::new(),
            incoming,
            outgoing,
        );
        app.update();
        let menu = app.world().resource::<MenuRuntime>();
        assert_eq!(
            menu.view().minecraft_import.phase,
            if action == ImportAction::Close {
                Phase::Idle
            } else {
                Phase::Result
            }
        );
        assert!(!menu.global_resources.busy);
        if action == ImportAction::Begin {
            assert!(
                menu.view()
                    .minecraft_import
                    .body
                    .contains("worker is unavailable")
            );
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .activate(MenuAction::AddBack);
            app.update();
        }
        assert_eq!(app.world().resource::<MenuRuntime>().view().dialog, None);
    }
}

#[test]
fn delivered_completion_is_published_before_worker_disconnection() {
    let (mut app, incoming, outgoing) = fixture();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::MinecraftImport(ImportAction::Begin));
    app.update();
    assert!(incoming.try_recv().is_ok());
    outgoing
        .send(Event::MinecraftImport(
            launcher::minecraft_import::Snapshot {
                phase: Phase::Preview,
                body: "Prepared fixture".into(),
                ..Default::default()
            },
            None,
        ))
        .unwrap();
    drop(outgoing);
    drop(incoming);
    app.update();
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.minecraft_import.phase, Phase::Preview);
    assert_eq!(view.minecraft_import.body, "Prepared fixture");
    assert_eq!(view.dialog, Some(MenuDialog::MinecraftImport));
    assert!(
        app.world()
            .resource::<Worker>()
            .minecraft_import_request
            .is_none()
    );
}

#[test]
fn worker_loss_reports_pending_pack_save_failure_without_overwriting_preferences() {
    let (mut app, incoming, outgoing) = fixture();
    let state = launcher_host::minecraft_import::UserState {
        settings: Default::default(),
        servers: Vec::new(),
        video: Default::default(),
        active: Vec::new(),
    };
    outgoing
        .send(Event::MinecraftImport(
            launcher::minecraft_import::Snapshot {
                phase: Phase::Result,
                body: "Imported fixture".into(),
                ..Default::default()
            },
            Some(Box::new(state)),
        ))
        .unwrap();
    drop(outgoing);
    drop(incoming);
    app.update();
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.minecraft_import.phase, Phase::Result);
    assert_eq!(view.minecraft_import.body, "Imported fixture");
    assert!(
        view.minecraft_import
            .pack_status
            .contains("worker is unavailable")
    );
    assert!(!app.world().resource::<Worker>().minecraft_import_pending);
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .minecraft_import
            .pack_status
            .contains("worker is unavailable")
    );
}

#[test]
fn unchanged_snapshots_retain_their_identity_before_and_after_worker_loss() {
    let (mut app, _incoming, outgoing) = fixture();
    app.update();
    let initial = Arc::clone(&app.world().resource::<MenuRuntime>().global_resources);
    app.update();
    assert!(Arc::ptr_eq(
        &initial,
        &app.world().resource::<MenuRuntime>().global_resources
    ));

    outgoing
        .send(Event::Snapshot(Snapshot {
            selected: Some((false, 0)),
            busy: true,
            message: "Preparing fixture".into(),
            ..Default::default()
        }))
        .unwrap();
    app.update();
    let busy = Arc::clone(&app.world().resource::<MenuRuntime>().global_resources);
    assert_eq!(initial.revision, busy.revision);
    assert_ne!(initial.selected, busy.selected);
    assert!(!Arc::ptr_eq(&initial, &busy));
    drop(outgoing);
    app.update();
    let stopped = Arc::clone(&app.world().resource::<MenuRuntime>().global_resources);
    assert!(!stopped.busy);
    assert!(busy.busy, "existing readers keep their immutable snapshot");
    assert!(!Arc::ptr_eq(&busy, &stopped));
    app.update();
    assert!(Arc::ptr_eq(
        &stopped,
        &app.world().resource::<MenuRuntime>().global_resources
    ));
}
