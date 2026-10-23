use {
    super::*,
    launcher::global_resources::{Action, Snapshot},
};

mod autoapply;
mod shutdown;

/// Runs queued commands with isolated worker storage and collects their events.
fn events_for(name: &str, queued: Vec<Command>) -> Vec<Event> {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-pack-worker-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let events = collect_events(root.clone(), queued);
    std::fs::remove_dir_all(root).unwrap();
    events
}

/// Runs the serial worker after queuing commands and closing their sender.
fn collect_events(root: PathBuf, queued: Vec<Command>) -> Vec<Event> {
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (outgoing, events) = crossbeam_channel::unbounded();
    for command in queued {
        commands.send(command).unwrap();
    }
    drop(commands);
    run(
        root.clone(),
        root.join("config"),
        Vec::new(),
        incoming,
        outgoing,
    );
    events.try_iter().collect()
}

/// Writes an original resource pack archive with two selectable subpacks.
fn write_fixture(root: &std::path::Path, id: u16) -> PathBuf {
    use std::io::Write;
    let path = root.join(format!("fixture-{id}.mcpack"));
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"00000000-0000-0000-0000-{id:012x}","name":"Fixture {id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}],"subpacks":[{{"folder_name":"low","name":"Low","memory_tier":0}},{{"folder_name":"high","name":"High","memory_tier":1}}]}}"#
    );
    archive.write_all(manifest.as_bytes()).unwrap();
    std::fs::write(&path, archive.finish().unwrap().into_inner()).unwrap();
    path
}

/// Installs a synthetic pack and returns its imported metadata.
fn import_fixture(
    library: &mut GlobalPackLibrary,
    root: &std::path::Path,
    id: u16,
) -> resource_pack::InstalledPack {
    library
        .import(&write_fixture(root, id))
        .unwrap()
        .imported
        .remove(0)
}

/// Sends a fixture request through the worker's import and artwork path.
fn import_request(
    service: &mut launcher_host::minecraft_import::Service,
    library: &mut GlobalPackLibrary,
    snapshot: &mut Snapshot,
    action: launcher::minecraft_import::Action,
    refresh_icons: impl FnOnce(&GlobalPackLibrary, &mut Snapshot),
) -> launcher_host::minecraft_import::Outcome {
    import_minecraft_data(
        service,
        launcher_host::minecraft_import::Request {
            action,
            state: launcher_host::minecraft_import::UserState {
                settings: Default::default(),
                servers: Vec::new(),
                video: Default::default(),
                active: library.active().to_vec(),
            },
        },
        library,
        snapshot,
        refresh_icons,
    )
    .0
}

/// Opens import confirmation without refreshing pack artwork or applying preferences.
fn confirm_import(
    service: &mut launcher_host::minecraft_import::Service,
    library: &mut GlobalPackLibrary,
    snapshot: &mut Snapshot,
) {
    let outcome = import_request(
        service,
        library,
        snapshot,
        launcher::minecraft_import::Action::Apply,
        |_, _| panic!("import confirmation refreshed artwork"),
    );
    assert_eq!(
        outcome.snapshot.phase,
        launcher::minecraft_import::Phase::ConfirmImport
    );
    assert!(outcome.state.is_none() && outcome.stack.is_none());
}

/// Supplies one user pack and its active reference without relying on installed Minecraft data.
fn minecraft_source(root: &std::path::Path, id: u16) -> PathBuf {
    let data = root.join("minecraft");
    let packs = data.join("resource_packs");
    std::fs::create_dir_all(&packs).unwrap();
    write_fixture(&packs, id);
    std::fs::create_dir_all(data.join("minecraftpe")).unwrap();
    let selection =
        format!(r#"[{{"pack_id":"00000000-0000-0000-0000-{id:012x}","version":[1,0,0]}}]"#);
    std::fs::write(
        data.join("minecraftpe/global_resource_packs.json"),
        selection,
    )
    .unwrap();
    data
}

#[test]
fn unchanged_minecraft_import_requests_reuse_icons_and_selected_cards() {
    use launcher::minecraft_import::Action as ImportAction;
    let fixture = tempfile::tempdir().unwrap();
    let mut library =
        GlobalPackLibrary::open(fixture.path().join("installed"), engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    library.activate(pack.id).unwrap();
    library.apply().unwrap();
    let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
        fixture.path().join("config"),
        fixture.path().join("missing-data"),
    );
    let mut snapshot = Snapshot {
        selected: Some((true, 0)),
        ..Default::default()
    };
    snapshot
        .icons
        .insert((pack.id.to_string(), pack.revision), "cached-icon".into());
    let icons = snapshot.icons.clone();
    for action in [
        ImportAction::NotNow,
        ImportAction::Close,
        ImportAction::Revert,
    ] {
        import_request(&mut service, &mut library, &mut snapshot, action, |_, _| {
            panic!("unchanged import request refreshed artwork")
        });
        assert_eq!(snapshot.icons, icons);
        assert_eq!(snapshot.selected, Some((true, 0)));
    }
}

#[test]
fn preview_reuses_icons_but_apply_and_revert_refresh_changed_pack_state() {
    use launcher::minecraft_import::{Action as ImportAction, Phase};
    let fixture = tempfile::tempdir().unwrap();
    let data = minecraft_source(fixture.path(), 1);
    let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
        fixture.path().join("config"),
        data,
    );
    let mut library =
        GlobalPackLibrary::open(fixture.path().join("installed"), engine_version()).unwrap();
    let mut snapshot = Snapshot::default();
    let preview = import_request(
        &mut service,
        &mut library,
        &mut snapshot,
        ImportAction::Begin,
        |_, _| panic!("preview refreshed artwork"),
    );
    assert_eq!(preview.snapshot.phase, Phase::Preview);
    let mut refreshes = 0;
    confirm_import(&mut service, &mut library, &mut snapshot);
    let applied = import_request(
        &mut service,
        &mut library,
        &mut snapshot,
        ImportAction::ConfirmApply,
        |library, _| {
            refreshes += 1;
            assert_eq!(library.available().len(), 1);
            assert_eq!(library.active().len(), 1);
        },
    );
    assert!(applied.state.is_some());
    assert_eq!(refreshes, 1);
    let confirmation = import_request(
        &mut service,
        &mut library,
        &mut snapshot,
        ImportAction::Revert,
        |_, _| panic!("revert confirmation refreshed artwork"),
    );
    assert_eq!(confirmation.snapshot.phase, Phase::Revert);
    let restored = import_request(
        &mut service,
        &mut library,
        &mut snapshot,
        ImportAction::ConfirmRevert,
        |library, _| {
            refreshes += 1;
            assert_eq!(library.available().len(), 1);
            assert!(library.active().is_empty());
        },
    );
    assert!(restored.state.is_some());
    assert_eq!(refreshes, 2);
}

#[test]
fn artwork_refreshes_after_an_import_changes_the_catalog_then_fails() {
    use launcher::minecraft_import::{Action as ImportAction, Phase};
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let original = import_fixture(&mut library, fixture.path(), 1);
    library.activate(original.id).unwrap();
    library.apply().unwrap();
    let data = minecraft_source(fixture.path(), 2);
    let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
        fixture.path().join("config"),
        data,
    );
    let mut snapshot = Snapshot::default();
    let preview = import_request(
        &mut service,
        &mut library,
        &mut snapshot,
        ImportAction::Begin,
        |_, _| panic!("preview refreshed artwork"),
    );
    assert_eq!(preview.snapshot.phase, Phase::Preview);
    for file in std::fs::read_dir(&root).unwrap() {
        let path = file.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "mcpack")
        {
            std::fs::remove_file(path).unwrap();
        }
    }
    let mut refreshed = false;
    confirm_import(&mut service, &mut library, &mut snapshot);
    let outcome = import_request(
        &mut service,
        &mut library,
        &mut snapshot,
        ImportAction::ConfirmApply,
        |library, _| {
            refreshed = true;
            assert_eq!(library.available().len(), 2);
        },
    );
    assert!(outcome.state.is_none());
    assert_eq!(outcome.snapshot.phase, Phase::Result);
    assert!(refreshed);
}

#[test]
fn batch_import_refreshes_icons_once_after_all_files_and_keeps_all_results() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let corrupt = fixture.path().join("corrupt.mcpack");
    std::fs::write(&corrupt, b"not a pack archive").unwrap();
    let missing = fixture.path().join("missing.mcpack");
    let errors = [&corrupt, &missing].map(|path| library.import(path).unwrap_err().to_string());
    let paths = vec![
        write_fixture(fixture.path(), 1),
        corrupt,
        write_fixture(fixture.path(), 2),
        missing,
        write_fixture(fixture.path(), 3),
    ];
    let names = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut snapshot = Snapshot {
        selected: Some((false, 0)),
        details_expanded: Some((false, 0)),
        settings: Some(0),
        ..Default::default()
    };

    let mut refreshes = 0;
    import_files(&mut library, paths, &mut snapshot, |library, snapshot| {
        refreshes += 1;
        assert_eq!(library.available().len(), 3);
        assert!(snapshot.message.contains("missing.mcpack"));
        assert!(snapshot.message.contains("fixture-3.mcpack"));
        super::super::icons::refresh(library, snapshot, &root.join("icons"));
    })
    .unwrap();
    refresh(&library, &mut snapshot);

    assert_eq!(refreshes, 1);
    assert_eq!(snapshot.available.len(), 3);
    assert!(snapshot.active.is_empty());
    assert_eq!(snapshot.selected, None);
    assert_eq!(snapshot.details_expanded, None);
    assert_eq!(snapshot.settings, None);
    for name in names {
        assert!(snapshot.message.contains(&name), "{}", snapshot.message);
    }
    for error in errors {
        assert!(snapshot.message.contains(&error), "{}", snapshot.message);
    }
    assert_eq!(
        snapshot
            .message
            .matches("Imported 1 resource packs")
            .count(),
        3
    );
}

#[test]
fn one_selected_file_preserves_single_import_status_and_errors() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let mut reference =
        GlobalPackLibrary::open(fixture.path().join("reference"), engine_version()).unwrap();
    let path = write_fixture(fixture.path(), 1);
    let mut expected = Snapshot::default();
    import(&mut reference, path.clone(), &mut expected).unwrap();
    let mut actual = Snapshot::default();
    let mut refreshes = 0;
    import_files(
        &mut library,
        vec![path],
        &mut actual,
        |library, snapshot| {
            refreshes += 1;
            assert_eq!(library.available().len(), 1);
            super::super::icons::refresh(library, snapshot, &root.join("icons"));
        },
    )
    .unwrap();
    assert_eq!(refreshes, 1);
    assert_eq!(actual.message, expected.message);

    let missing = fixture.path().join("missing.mcpack");
    let expected_error = library.import(&missing).unwrap_err().to_string();
    let error = import_files(&mut library, vec![missing], &mut actual, |_, _| {
        panic!("failed single import refreshed icons")
    })
    .unwrap_err();
    assert_eq!(error.to_string(), expected_error);
}

#[test]
fn cancelled_multi_selection_clears_status_without_changing_selection() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let mut snapshot = Snapshot {
        message: "Updating resource packs…".into(),
        selected: Some((false, 0)),
        ..Default::default()
    };
    import_files(&mut library, Vec::new(), &mut snapshot, |_, _| {
        panic!("cancelled import refreshed icons")
    })
    .unwrap();
    assert!(snapshot.message.is_empty());
    assert_eq!(snapshot.selected, Some((false, 0)));
    assert!(library.available().is_empty());
}

#[test]
fn entirely_failed_batch_preserves_icons_and_reports_each_error() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let mut snapshot = Snapshot::default();
    let icon_key = ("existing-pack".to_owned(), 1);
    snapshot
        .icons
        .insert(icon_key.clone(), "existing.png".into());
    import_files(
        &mut library,
        vec![
            fixture.path().join("missing-a.mcpack"),
            fixture.path().join("missing-b.mcpack"),
        ],
        &mut snapshot,
        |_, _| panic!("entirely failed batch refreshed icons"),
    )
    .unwrap();
    assert_eq!(snapshot.icons.get(&icon_key).unwrap(), "existing.png");
    assert!(snapshot.message.contains("missing-a.mcpack"));
    assert!(snapshot.message.contains("missing-b.mcpack"));
    assert!(library.available().is_empty());
}

#[test]
fn reordered_cards_and_variant_dialogs_follow_the_same_pack() {
    let fixture = tempfile::tempdir().unwrap();
    let mut library =
        GlobalPackLibrary::open(fixture.path().join("installed"), engine_version()).unwrap();
    let first = import_fixture(&mut library, fixture.path(), 1);
    let second = import_fixture(&mut library, fixture.path(), 2);
    library.activate(first.id).unwrap();
    library.activate(second.id).unwrap();
    let mut snapshot = Snapshot::default();
    refresh(&library, &mut snapshot);
    let (outgoing, _) = crossbeam_channel::unbounded();

    snapshot.details_expanded = Some((true, 1));
    snapshot.settings = Some(1);
    act(&mut library, &mut snapshot, Action::MoveUp(1), &outgoing).unwrap();
    refresh(&library, &mut snapshot);
    assert_eq!(snapshot.active[0].id, first.id);
    assert_eq!(snapshot.details_expanded, Some((true, 0)));
    assert_eq!(snapshot.settings, Some(0));
    assert_eq!(snapshot.selected, Some((true, 0)));

    snapshot.details_expanded = Some((true, 1));
    snapshot.settings = Some(1);
    act(&mut library, &mut snapshot, Action::MoveDown(0), &outgoing).unwrap();
    refresh(&library, &mut snapshot);
    assert_eq!(snapshot.active[0].id, second.id);
    assert_eq!(snapshot.details_expanded, Some((true, 0)));
    assert_eq!(snapshot.settings, Some(0));
    assert_eq!(snapshot.selected, Some((true, 1)));

    snapshot.details_expanded = Some((false, 0));
    act(&mut library, &mut snapshot, Action::MoveUp(1), &outgoing).unwrap();
    assert_eq!(snapshot.details_expanded, Some((false, 0)));
}

#[test]
fn changed_pack_lists_clear_indexed_card_and_dialog_state() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let first = import_fixture(&mut library, fixture.path(), 1);
    import_fixture(&mut library, fixture.path(), 2);
    library.activate(first.id).unwrap();
    let mut snapshot = Snapshot::default();
    refresh(&library, &mut snapshot);
    let (outgoing, _) = crossbeam_channel::unbounded();

    act(
        &mut library,
        &mut snapshot,
        Action::SelectAvailable(0),
        &outgoing,
    )
    .unwrap();
    assert_eq!(snapshot.selected, Some((false, 0)));
    snapshot.details_expanded = Some((false, 0));
    snapshot.settings = Some(0);
    act(&mut library, &mut snapshot, Action::Activate(0), &outgoing).unwrap();
    assert_eq!(snapshot.selected, None);
    assert_eq!(snapshot.details_expanded, None);
    assert_eq!(snapshot.settings, None);
    refresh(&library, &mut snapshot);

    act(
        &mut library,
        &mut snapshot,
        Action::SelectActive(0),
        &outgoing,
    )
    .unwrap();
    assert_eq!(snapshot.selected, Some((true, 0)));
    snapshot.details_expanded = Some((true, 0));
    snapshot.settings = Some(1);
    act(
        &mut library,
        &mut snapshot,
        Action::Deactivate(0),
        &outgoing,
    )
    .unwrap();
    assert_eq!(snapshot.selected, None);
    assert_eq!(snapshot.details_expanded, None);
    assert_eq!(snapshot.settings, None);
    refresh(&library, &mut snapshot);

    snapshot.selected = Some((false, 0));
    snapshot.details_expanded = Some((false, 0));
    snapshot.settings = Some(0);
    import(
        &mut library,
        write_fixture(fixture.path(), 3),
        &mut snapshot,
    )
    .unwrap();
    assert_eq!(snapshot.selected, None);
    assert_eq!(snapshot.details_expanded, None);
    assert_eq!(snapshot.settings, None);
}

#[test]
fn variant_dialog_closes_only_after_a_successful_selection() {
    let fixture = tempfile::tempdir().unwrap();
    let mut library =
        GlobalPackLibrary::open(fixture.path().join("installed"), engine_version()).unwrap();
    let pack = import_fixture(&mut library, fixture.path(), 1);
    library.activate(pack.id).unwrap();
    let mut snapshot = Snapshot::default();
    refresh(&library, &mut snapshot);
    let (outgoing, _) = crossbeam_channel::unbounded();
    let high = snapshot.active[0]
        .subpacks
        .iter()
        .position(|subpack| subpack.folder == "high")
        .unwrap();
    snapshot.settings = Some(0);
    snapshot.details_expanded = Some((true, 0));
    act(
        &mut library,
        &mut snapshot,
        Action::Subpack(high),
        &outgoing,
    )
    .unwrap();
    assert_eq!(snapshot.settings, None);
    assert_eq!(snapshot.details_expanded, Some((true, 0)));
    assert_eq!(library.active()[0].subpack, "high");

    snapshot.settings = Some(0);
    snapshot.active[0].subpacks[high].folder = "missing-variant".into();
    assert!(
        act(
            &mut library,
            &mut snapshot,
            Action::Subpack(high),
            &outgoing
        )
        .is_err()
    );
    assert_eq!(snapshot.settings, Some(0));
    assert_eq!(library.active()[0].subpack, "high");
}

#[test]
fn commit_completion_clears_the_temporary_worker_status() {
    let events = events_for("commit-status", vec![Command::Commit(Vec::new())]);
    let completed = events
        .iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .next_back()
        .unwrap();
    assert!(!completed.busy);
    assert!(
        completed.message.is_empty(),
        "successful acknowledgement clears its working status"
    );
    assert_eq!(completed.applied_selection, Some(Vec::new()));
    assert!(!completed.has_pending_changes());
    assert_eq!(
        completed.revision, 0,
        "acknowledgement does not replace staged lists"
    );
}

#[test]
fn presentation_only_actions_do_not_publish_a_pack_update() {
    let events = events_for(
        "presentation-status",
        [
            Action::ToggleAvailable,
            Action::ToggleActive,
            Action::CloseSettings,
        ]
        .into_iter()
        .map(|action| Command::Action {
            action,
            revision: 0,
        })
        .collect(),
    );
    for snapshot in events.iter().filter_map(|event| match event {
        Event::Snapshot(snapshot) => Some(snapshot),
        Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
    }) {
        assert!(
            !snapshot.busy,
            "presentation changes do not mark pack preparation busy"
        );
        assert!(
            snapshot.message.is_empty(),
            "presentation changes do not publish a working status"
        );
    }
}

#[test]
fn apply_keeps_its_pending_runtime_publication() {
    let events = events_for(
        "apply-status",
        vec![Command::Action {
            action: Action::Apply,
            revision: 0,
        }],
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Apply(..)))
            .count(),
        2
    );
    let completed = events
        .iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .next_back()
        .unwrap();
    assert!(!completed.busy);
    assert_eq!(completed.message, "Preparing resource packs…");
}

#[test]
fn failed_import_keeps_its_error_after_busy_finishes() {
    let events = events_for(
        "import-status",
        vec![Command::Import(PathBuf::from("unsupported.worker-fixture"))],
    );
    let completed = events
        .iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .next_back()
        .unwrap();
    assert!(!completed.busy);
    assert!(!completed.message.is_empty());
    assert_ne!(completed.message, "Updating resource packs…");
}

#[test]
fn presentation_actions_preserve_a_pending_apply_message() {
    let events = events_for(
        "pending-apply",
        vec![
            Command::Action {
                action: Action::Apply,
                revision: 0,
            },
            Command::Action {
                action: Action::ToggleAvailable,
                revision: 0,
            },
            Command::Action {
                action: Action::CloseSettings,
                revision: 0,
            },
        ],
    );
    let completed = events
        .iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .next_back()
        .unwrap();
    assert_eq!(completed.message, "Preparing resource packs…");
    assert!(completed.applied_selection.is_none());
    assert!(!completed.has_pending_changes());
}

#[test]
fn an_acknowledgement_preserves_newer_staging_and_queued_list_indices() {
    use std::io::Write;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let path = fixture.path().join("fixture.mcpack");
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(br#"{"format_version":2,"header":{"uuid":"00000000-0000-0000-0000-000000000001","name":"Fixture","version":[1,0,0]},"modules":[{"type":"resources"}]}"#).unwrap();
    std::fs::write(&path, archive.finish().unwrap().into_inner()).unwrap();
    let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
    let imported = library.import(&path).unwrap().imported.remove(0);
    let acknowledged = vec![resource_pack::ActivePack {
        id: imported.id,
        revision: imported.revision,
        subpack: String::new(),
    }];
    drop(library);
    let events = collect_events(
        root.clone(),
        vec![
            Command::Action {
                action: Action::Activate(0),
                revision: 0,
            },
            Command::Action {
                action: Action::Apply,
                revision: 1,
            },
            Command::Action {
                action: Action::Deactivate(0),
                revision: 1,
            },
            Command::Commit(acknowledged.clone()),
            Command::Action {
                action: Action::SelectAvailable(0),
                revision: 2,
            },
        ],
    );
    let completed = events
        .iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .next_back()
        .unwrap();
    assert_eq!(completed.applied_selection.as_ref(), Some(&acknowledged));
    assert!(
        completed.selection.is_empty(),
        "runtime acknowledgement preserves newer staged removal"
    );
    assert!(completed.active.is_empty());
    assert!(completed.has_pending_changes());
    assert_eq!(completed.selected, Some((false, 0)));
    assert_eq!(completed.revision, 2);
    assert!(completed.message.is_empty());
    assert_eq!(
        GlobalPackLibrary::open(root, engine_version())
            .unwrap()
            .active(),
        completed.selection,
        "restart preserves newer staged edits after an older runtime acknowledgement"
    );
}

#[test]
fn a_failed_save_still_records_the_runtime_acknowledgement() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (outgoing, events) = crossbeam_channel::unbounded();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        run(
            worker_root.clone(),
            worker_root.join("config"),
            Vec::new(),
            incoming,
            outgoing,
        )
    });
    while !matches!(
        events
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        Event::Snapshot(_)
    ) {}
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::write(&root, b"blocked storage").unwrap();
    commands.send(Command::Commit(Vec::new())).unwrap();
    drop(commands);
    worker.join().unwrap();
    let completed = events
        .try_iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .last()
        .unwrap();
    assert_eq!(completed.applied_selection, Some(Vec::new()));
    assert!(!completed.busy);
    assert!(!completed.message.is_empty());
    assert_ne!(completed.message, "Updating resource packs…");
}

#[test]
fn review_commands_from_one_snapshot_keep_both_panel_toggles() {
    let root = std::env::temp_dir().join(format!("cinnabar-pack-worker-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (outgoing, events) = crossbeam_channel::unbounded();
    for action in [Action::ToggleAvailable, Action::ToggleActive] {
        commands
            .send(Command::Action {
                action,
                revision: 0,
            })
            .unwrap();
    }
    drop(commands);
    run(
        root.clone(),
        root.join("config"),
        Vec::new(),
        incoming,
        outgoing,
    );
    let last = events
        .try_iter()
        .filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) | Event::MinecraftImport(..) | Event::MinecraftImportStatus(_) => None,
        })
        .last()
        .unwrap();
    assert!(!last.available_expanded);
    assert!(!last.active_expanded);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn minecraft_import_completion_tracks_submitted_revisions_after_replacement_and_save_failure() {
    use launcher::minecraft_import::Action as ImportAction;
    for (revert, fail_save) in [(false, false), (false, true), (true, false), (true, true)] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("installed");
        let data = minecraft_source(fixture.path(), 1);
        let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
            fixture.path().join("config"),
            data,
        );
        let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
        let original = import_fixture(&mut library, fixture.path(), 2);
        library.activate(original.id).unwrap();
        library.apply().unwrap();
        let mut snapshot = Snapshot::default();
        import_request(
            &mut service,
            &mut library,
            &mut snapshot,
            ImportAction::Begin,
            |_, _| {},
        );
        confirm_import(&mut service, &mut library, &mut snapshot);
        let mut outcome = import_request(
            &mut service,
            &mut library,
            &mut snapshot,
            ImportAction::ConfirmApply,
            |_, _| {},
        );
        if revert {
            library
                .commit_selection(&outcome.state.as_ref().unwrap().active)
                .unwrap();
            import_request(
                &mut service,
                &mut library,
                &mut snapshot,
                ImportAction::Revert,
                |_, _| {},
            );
            outcome = import_request(
                &mut service,
                &mut library,
                &mut snapshot,
                ImportAction::ConfirmRevert,
                |_, _| {},
            );
        }
        let submitted = outcome.state.unwrap().active;
        assert!(outcome.stack.is_some());
        let mut pending = Some(submitted.clone());
        let replacement = import_fixture(&mut library, fixture.path(), if revert { 2 } else { 1 });
        assert_ne!(replacement.revision, submitted[0].revision);
        let staged = library.active().to_vec();
        let (outgoing, events) = crossbeam_channel::unbounded();
        commit_selection(&mut library, &[], &mut pending, &outgoing).unwrap();
        assert_eq!(pending, Some(submitted.clone()));
        assert!(
            events.try_recv().is_err(),
            "unrelated acknowledgements do not complete the import"
        );
        if fail_save {
            std::fs::rename(&root, fixture.path().join("parked")).unwrap();
            std::fs::write(&root, b"blocked storage").unwrap();
        }
        let result = commit_selection(&mut library, &submitted, &mut pending, &outgoing);
        assert_eq!(result.is_err(), fail_save);
        assert!(pending.is_none());
        assert_eq!(
            library.active(),
            staged,
            "completion preserves later staged edits"
        );
        let Event::MinecraftImportStatus(status) = events.try_recv().unwrap() else {
            panic!("import completion must publish its save result");
        };
        assert!(status.contains(if fail_save {
            "could not be saved"
        } else {
            "applied and saved"
        }));
        assert!(events.try_recv().is_err());
        if !fail_save {
            let reopened = GlobalPackLibrary::open(&root, engine_version()).unwrap();
            assert_eq!(reopened.active(), submitted);
        }
    }
}
