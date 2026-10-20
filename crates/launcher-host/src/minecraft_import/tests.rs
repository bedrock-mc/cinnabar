use super::*;
use launcher::menu::settings_options::SettingId;

/// Builds an isolated profile whose options and packs are fully synthetic.
fn service(root: &Path) -> Service {
    let data = root.join("minecraft");
    std::fs::create_dir_all(data.join("minecraftpe")).unwrap();
    std::fs::write(
        data.join("minecraftpe/options.txt"),
        b"gfx_field_of_view:80\naudio_music:0.25\nunknown:skip\nbad",
    )
    .unwrap();
    std::fs::write(
        data.join("minecraftpe/external_servers.txt"),
        b"1:Fixture:example.test:19132:0\nbad",
    )
    .unwrap();
    Service::new_with_data_root(root.join("cinnabar"), data)
}

/// Reviews the import confirmation before committing the retained preview.
fn apply(service: &mut Service, state: UserState, library: &mut GlobalPackLibrary) -> Outcome {
    service.act(
        Request {
            action: Action::Apply,
            state: state.clone(),
        },
        library,
    );
    service.act(
        Request {
            action: Action::ConfirmApply,
            state,
        },
        library,
    )
}

/// Supplies current preferences without account data or installed assets.
fn state() -> UserState {
    UserState {
        settings: SettingsOptions::default(),
        servers: Vec::new(),
        video: Default::default(),
        active: Vec::new(),
    }
}

/// Uses an empty, isolated pack library for the consent and restore contracts.
fn library(root: &Path) -> GlobalPackLibrary {
    GlobalPackLibrary::open(root.join("packs"), [1, 26, 50]).unwrap()
}

/// Writes a tiny original resource pack directory for the normal importer to validate.
fn pack(root: &Path, name: &str, id: &str) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let manifest = serde_json::json!({"format_version":2,"header":{"uuid":id,"name":name,"version":[1,0,0]},"modules":[{"type":"resources"}]});
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    directory
}

/// Writes a valid, uncompressed synthetic pack with the requested archive payload size.
fn sized_pack(root: &Path, name: &str, id: &str, payload_bytes: usize) -> PathBuf {
    use std::io::{Read, Write};
    let path = root.join(format!("{name}.mcpack"));
    std::fs::create_dir_all(root).unwrap();
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let manifest = serde_json::json!({"format_version":2,"header":{"uuid":id,"name":name,"version":[1,0,0]},"modules":[{"type":"resources"}]});
    writer.start_file("manifest.json", options).unwrap();
    writer
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    writer.start_file("payload.bin", options).unwrap();
    std::io::copy(
        &mut std::io::repeat(0).take(payload_bytes as u64),
        &mut writer,
    )
    .unwrap();
    writer.finish().unwrap();
    path
}

#[test]
fn combined_selected_archives_remain_available_during_import() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let size = resource_pack::MAX_STACK_ARCHIVE_BYTES / 3 + 1024;
    for (name, id) in [
        ("first", "11111111-1111-1111-1111-111111111111"),
        ("second", "22222222-2222-2222-2222-222222222222"),
    ] {
        let path = sized_pack(root.path(), name, id, size);
        let imported = library.import(&path).unwrap().imported.remove(0);
        library.activate(imported.id).unwrap();
    }
    library.apply().unwrap();
    let original = library.active().to_vec();
    let available = library.available().to_vec();
    let packs = service.locations[0].path.join("resource_packs");
    sized_pack(
        &packs,
        "large",
        "33333333-3333-3333-3333-333333333333",
        size,
    );
    pack(&packs, "small", "44444444-4444-4444-4444-444444444444");
    std::fs::write(
        service.locations[0]
            .path
            .join("minecraftpe/global_resource_packs.json"),
        br#"[
        {"pack_id":"33333333-3333-3333-3333-333333333333","version":[1,0,0]},
        {"pack_id":"44444444-4444-4444-4444-444444444444","version":[1,0,0]}
    ]"#,
    )
    .unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library,
    );
    let preview = service.preview.as_ref().unwrap();
    assert_eq!(preview.selection.len(), original.len() + 2);
    assert_eq!(
        preview.selection[0].id.to_string(),
        "33333333-3333-3333-3333-333333333333"
    );
    assert_eq!(
        preview.selection[1].id.to_string(),
        "44444444-4444-4444-4444-444444444444"
    );
    assert_eq!(&preview.selection[2..], original);
    let selected = preview.selection.clone();
    assert_eq!(library.available(), available);
    assert_eq!(library.active(), original);
    assert!(!storage::has_backup(&service.config_root));
    let applied = apply(&mut service, state(), &mut library);
    assert!(applied.stack.is_some());
    let imported = applied.state.unwrap();
    assert_eq!(imported.settings.value(SettingId::FieldOfView), 80);
    assert_eq!(imported.servers.len(), 1);
    assert_eq!(
        imported
            .active
            .iter()
            .map(|pack| (pack.id, &pack.subpack))
            .collect::<Vec<_>>(),
        selected
            .iter()
            .map(|pack| (pack.id, &pack.subpack))
            .collect::<Vec<_>>()
    );
    assert!(imported.active[..2].iter().all(|pack| pack.revision != 0));
    assert_eq!(&imported.active[2..], original);
}

#[test]
fn revert_restores_backup_revisions_after_pack_updates_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let source = pack(
        root.path(),
        "original",
        "11111111-1111-1111-1111-111111111111",
    );
    std::fs::write(source.join("marker.txt"), b"original").unwrap();
    let imported = library
        .import_prepared(resource_pack::prepare_import(&source).unwrap())
        .unwrap()
        .imported
        .remove(0);
    library.activate(imported.id).unwrap();
    library.apply().unwrap();
    let mut old = state();
    old.active = library.active().to_vec();
    service.act(
        Request {
            action: Action::Begin,
            state: old.clone(),
        },
        &mut library,
    );
    let imported = apply(&mut service, old.clone(), &mut library)
        .state
        .unwrap();
    library.commit_selection(&imported.active).unwrap();
    std::fs::write(source.join("marker.txt"), b"updated").unwrap();
    library
        .import_prepared(resource_pack::prepare_import(&source).unwrap())
        .unwrap();
    library.apply().unwrap();
    drop(library);
    let mut library = GlobalPackLibrary::open(root.path().join("packs"), [1, 26, 50]).unwrap();
    service.act(
        Request {
            action: Action::Revert,
            state: imported.clone(),
        },
        &mut library,
    );
    let restored = service.act(
        Request {
            action: Action::ConfirmRevert,
            state: imported,
        },
        &mut library,
    );
    let view = resource_pack::LayeredPackView::new(restored.stack.unwrap());
    assert_eq!(view.read("marker.txt").unwrap().as_ref(), b"original");
    let restored = restored.state.unwrap();
    assert_eq!(restored.active, old.active);
    assert_eq!(restored.settings, old.settings);
    assert_eq!(restored.servers, old.servers);
    assert_eq!(restored.video, old.video);
    library.commit_selection(&restored.active).unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: restored.clone(),
        },
        &mut library,
    );
    let imported_again = apply(&mut service, restored, &mut library).state.unwrap();
    assert_eq!(
        imported_again.active, old.active,
        "import must retain the previewed installed revision"
    );
}

#[test]
fn copied_packs_follow_imported_priority_and_revert_keeps_them_installed() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let original = pack(
        root.path(),
        "original",
        "11111111-1111-1111-1111-111111111111",
    );
    let original = library
        .import_prepared(resource_pack::prepare_import(&original).unwrap())
        .unwrap()
        .imported
        .remove(0);
    library.activate(original.id).unwrap();
    library.apply().unwrap();
    let old = library.active().to_vec();
    let packs = service.locations[0].path.join("resource_packs");
    let first = pack(&packs, "first", "22222222-2222-2222-2222-222222222222");
    let second = pack(&packs, "second", "33333333-3333-3333-3333-333333333333");
    std::fs::write(
        service.locations[0]
            .path
            .join("minecraftpe/global_resource_packs.json"),
        br#"[
        {"pack_id":"33333333-3333-3333-3333-333333333333","version":[1,0,0]},
        {"pack_id":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}
    ]"#,
    )
    .unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library,
    );
    assert_eq!(service.preview.as_ref().unwrap().packs.len(), 2);
    let imported = apply(&mut service, state(), &mut library);
    let imported = imported.state.unwrap();
    assert_eq!(
        imported
            .active
            .iter()
            .map(|active| active.id.to_string())
            .collect::<Vec<_>>(),
        vec![
            "33333333-3333-3333-3333-333333333333",
            "22222222-2222-2222-2222-222222222222",
            "11111111-1111-1111-1111-111111111111"
        ]
    );
    library.commit_selection(&imported.active).unwrap();
    assert!(first.join("manifest.json").is_file());
    assert!(second.join("manifest.json").is_file());
    service.act(
        Request {
            action: Action::Revert,
            state: imported.clone(),
        },
        &mut library,
    );
    let restored = service
        .act(
            Request {
                action: Action::ConfirmRevert,
                state: imported,
            },
            &mut library,
        )
        .state
        .unwrap();
    assert_eq!(restored.active, old);
    assert_eq!(library.available().len(), 3);
    assert!(library.preview().is_ok());
}

#[test]
fn changed_pack_catalog_requires_a_fresh_preview() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library,
    );
    let directory = pack(
        root.path(),
        "another",
        "44444444-4444-4444-4444-444444444444",
    );
    library
        .import_prepared(resource_pack::prepare_import(&directory).unwrap())
        .unwrap();
    let outcome = apply(&mut service, state(), &mut library);
    assert!(outcome.state.is_none());
    assert!(outcome.snapshot.body.contains("preview the import again"));
    assert!(!storage::has_backup(&service.config_root));
}

#[test]
fn merged_servers_preserve_large_valid_lists() {
    let incoming = (0..600)
        .map(|index| SavedServer {
            name: "n".repeat(launcher::menu::MAX_SERVER_NAME_BYTES),
            address: format!(
                "{index:03}.{}",
                "a".repeat(launcher::menu::MAX_SERVER_ADDRESS_BYTES - 4)
            ),
            favorite: false,
            last_joined_unix: 0,
        })
        .collect::<Vec<_>>();
    let (merged, skipped) = preview::merge_servers(&[], &incoming);
    assert_eq!(skipped, 0);
    assert_eq!(merged, incoming);
    crate::servers::validate(&merged).unwrap();
}

#[test]
fn either_answer_persists_and_detection_never_parses_options() {
    for import in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut service = service(root.path());
        let offered = startup_at(&service.config_root, &service.locations);
        assert_eq!(offered.phase, Phase::Offered);
        assert!(service.preview.is_none());
        let action = if import {
            Action::Begin
        } else {
            Action::NotNow
        };
        let result = service.act(
            Request {
                action,
                state: state(),
            },
            &mut library(root.path()),
        );
        assert!(result.state.is_none());
        assert_eq!(
            startup_at(&service.config_root, &service.locations).phase,
            Phase::Idle
        );
        if !import {
            assert!(service.sources.is_empty());
            assert!(service.preview.is_none());
        }
    }
}

#[test]
fn preview_is_read_only_and_revert_restores_every_replaced_authority() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let old = state();
    let original =
        std::fs::read(service.locations[0].path.join("minecraftpe/options.txt")).unwrap();
    let reviewed = service.act(
        Request {
            action: Action::Begin,
            state: old.clone(),
        },
        &mut library,
    );
    assert_eq!(reviewed.snapshot.phase, Phase::Preview);
    assert!(reviewed.snapshot.body.contains("Fixture"));
    assert!(reviewed.state.is_none());
    assert!(!storage::has_backup(&service.config_root));
    let applied = apply(&mut service, old.clone(), &mut library);
    let changed = applied.state.unwrap();
    assert!(applied.snapshot.can_revert);
    assert_eq!(changed.settings.value(SettingId::FieldOfView), 80);
    assert_eq!(changed.servers.len(), 1);
    assert_eq!(
        std::fs::read(service.locations[0].path.join("minecraftpe/options.txt")).unwrap(),
        original
    );
    service.act(
        Request {
            action: Action::Revert,
            state: changed.clone(),
        },
        &mut library,
    );
    let restored = service
        .act(
            Request {
                action: Action::ConfirmRevert,
                state: changed,
            },
            &mut library,
        )
        .state
        .unwrap();
    assert_eq!(restored.settings, old.settings);
    assert_eq!(restored.servers, old.servers);
    assert_eq!(restored.video, old.video);
    assert_eq!(restored.active, old.active);
}

#[test]
fn apply_requires_a_preview_and_cancelling_releases_it() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    assert!(apply(&mut service, state(), &mut library).state.is_none());
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library,
    );
    service.act(
        Request {
            action: Action::Close,
            state: state(),
        },
        &mut library,
    );
    assert!(service.preview.is_none());
    assert!(apply(&mut service, state(), &mut library).state.is_none());
}

#[test]
fn oversized_user_files_are_reported_without_losing_other_formats() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    std::fs::write(
        service.locations[0].path.join("minecraftpe/options.txt"),
        vec![b'x'; 65537],
    )
    .unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library(root.path()),
    );
    let preview = service.preview.as_ref().unwrap();
    assert!(preview.options.values.is_empty());
    assert_eq!(preview.servers.entries.len(), 1);
    assert!(preview.skipped >= 1);
}

#[cfg(unix)]
#[test]
fn links_cannot_redirect_options_to_private_files() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let path = service.locations[0].path.join("minecraftpe/options.txt");
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(root.path().join("unreadable-private-file"), path).unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library(root.path()),
    );
    assert!(service.preview.as_ref().unwrap().options.values.is_empty());
    assert!(service.preview.as_ref().unwrap().skipped >= 1);
}

/// Rewrites a synthetic directory pack's version and identifiable payload.
fn versioned_pack(root: &Path, name: &str, id: &str, version: u32) -> PathBuf {
    let directory = pack(root, name, id);
    let manifest_path = directory.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["header"]["version"] = serde_json::json!([version, 0, 0]);
    std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    std::fs::write(directory.join("marker.txt"), name.as_bytes()).unwrap();
    directory
}

#[test]
fn requested_source_version_wins_in_either_order_without_replacing_installed_packs() {
    let id = "55555555-5555-5555-5555-555555555555";
    for versions in [[1, 2], [2, 1]] {
        for installed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut service = service(root.path());
            let mut library = library(root.path());
            if installed {
                let original = versioned_pack(root.path(), "installed", id, 1);
                let metadata = library
                    .import_prepared(resource_pack::prepare_import(&original).unwrap())
                    .unwrap()
                    .imported
                    .remove(0);
                library.activate(metadata.id).unwrap();
                library.apply().unwrap();
            }
            let original = library.active().to_vec();
            let data = service.locations[0].path.clone();
            let packs = data.join("resource_packs");
            versioned_pack(&packs, "a", id, versions[0]);
            versioned_pack(&packs, "b", id, versions[1]);
            std::fs::write(
                data.join("minecraftpe/global_resource_packs.json"),
                format!(r#"[{{"pack_id":"{id}","version":[2,0,0]}}]"#),
            )
            .unwrap();
            service.act(
                Request {
                    action: Action::Begin,
                    state: state(),
                },
                &mut library,
            );
            assert_eq!(library.active(), original);
            let preview = service.preview.as_ref().unwrap();
            assert_eq!(preview.packs.len(), usize::from(!installed));
            if !installed {
                assert_eq!(preview.packs[0].report.imported[0].version, [2, 0, 0]);
            }
            let outcome = apply(&mut service, state(), &mut library);
            let applied = outcome.state.unwrap();
            assert_eq!(applied.active.len(), 1);
            let metadata = library.metadata(&applied.active[0]).unwrap();
            assert_eq!(metadata.version, [if installed { 1 } else { 2 }, 0, 0]);
            if installed {
                assert_eq!(applied.active, original);
            }
            let view = resource_pack::LayeredPackView::new(outcome.stack.unwrap());
            let expected = if installed {
                "installed"
            } else if versions[0] == 2 {
                "a"
            } else {
                "b"
            };
            assert_eq!(
                view.read("marker.txt").unwrap().as_ref(),
                expected.as_bytes()
            );
        }
    }
}

#[test]
fn bundle_duplicates_keep_requested_version_and_unrelated_resource_halves() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let data = service.locations[0].path.clone();
    let packs = data.join("resource_packs");
    let id = "55555555-5555-5555-5555-555555555555";
    let other = "66666666-6666-6666-6666-666666666666";
    versioned_pack(&packs.join("a_bundle"), "a_old", id, 1);
    versioned_pack(&packs.join("a_bundle"), "b_selected", id, 2);
    versioned_pack(&packs.join("a_bundle"), "c_other", other, 1);
    versioned_pack(&packs, "b_duplicate", id, 2);
    std::fs::write(
        data.join("minecraftpe/global_resource_packs.json"),
        format!(r#"[{{"pack_id":"{id}","version":[2,0,0]}}]"#),
    )
    .unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library,
    );
    let preview = service.preview.as_ref().unwrap();
    assert_eq!(preview.packs.len(), 2);
    assert_eq!(preview.packs[0].report.imported[0].version, [2, 0, 0]);
    assert_eq!(preview.selection.len(), 1);
    let applied = apply(&mut service, state(), &mut library);
    assert_eq!(library.available().len(), 2);
    let view = resource_pack::LayeredPackView::new(applied.stack.unwrap());
    assert_eq!(view.read("marker.txt").unwrap().as_ref(), b"b_selected");
}

#[test]
fn missing_requested_version_keeps_the_first_copy_available_but_inactive() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let data = service.locations[0].path.clone();
    let id = "55555555-5555-5555-5555-555555555555";
    versioned_pack(&data.join("resource_packs"), "a", id, 1);
    std::fs::write(
        data.join("minecraftpe/global_resource_packs.json"),
        format!(r#"[{{"pack_id":"{id}","version":[2,0,0]}}]"#),
    )
    .unwrap();
    service.act(
        Request {
            action: Action::Begin,
            state: state(),
        },
        &mut library,
    );
    let preview = service.preview.as_ref().unwrap();
    assert_eq!(preview.packs.len(), 1);
    assert!(preview.selection.is_empty());
    assert!(preview.skipped > 0);
}

#[test]
fn selected_shared_version_replaces_a_profile_copy_without_reading_shared_settings() {
    let root = tempfile::tempdir().unwrap();
    let service = service(root.path());
    let library = library(root.path());
    let data = service.locations[0].path.clone();
    let shared = root.path().join("shared");
    let id = "55555555-5555-5555-5555-555555555555";
    versioned_pack(&data.join("resource_packs"), "profile", id, 1);
    versioned_pack(&shared.join("resource_packs"), "shared", id, 2);
    std::fs::write(shared.join("options.txt"), b"gfx_field_of_view:100").unwrap();
    std::fs::write(
        data.join("minecraftpe/global_resource_packs.json"),
        format!(r#"[{{"pack_id":"{id}","version":[2,0,0]}}]"#),
    )
    .unwrap();
    let source = locations::Source {
        root: data,
        shared: Some(shared),
        label: "Fixture".into(),
    };
    let preview = preview::prepare(&source, &state(), &library);
    assert_eq!(preview.packs.len(), 1);
    assert_eq!(preview.packs[0].report.imported[0].version, [2, 0, 0]);
    assert_eq!(preview.selection.len(), 1);
    let mut settings = SettingsOptions::default();
    preview.options.apply(&mut settings);
    assert_eq!(settings.value(SettingId::FieldOfView), 80);
}

#[test]
fn import_requires_confirmation_and_cancel_retains_the_reviewed_preview() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path());
    let mut library = library(root.path());
    let original = state();
    let preview = service.act(
        Request {
            action: Action::Begin,
            state: original.clone(),
        },
        &mut library,
    );
    let bypass = service.act(
        Request {
            action: Action::ConfirmApply,
            state: original.clone(),
        },
        &mut library,
    );
    assert_eq!(bypass.snapshot.phase, Phase::Preview);
    assert!(bypass.state.is_none());
    let confirmation = service.act(
        Request {
            action: Action::Apply,
            state: original.clone(),
        },
        &mut library,
    );
    assert_eq!(confirmation.snapshot.phase, Phase::ConfirmImport);
    assert_eq!(
        confirmation.snapshot.actions(),
        [Action::ConfirmApply, Action::Close]
    );
    assert!(confirmation.state.is_none() && confirmation.stack.is_none());
    assert!(!storage::has_backup(&service.config_root));
    let cancelled = service.act(
        Request {
            action: Action::Close,
            state: original.clone(),
        },
        &mut library,
    );
    assert_eq!(cancelled.snapshot.phase, Phase::Preview);
    assert_eq!(cancelled.snapshot.body, preview.snapshot.body);
    assert!(service.preview.is_some());
    assert!(!storage::has_backup(&service.config_root));
    let stale_confirm = service.act(
        Request {
            action: Action::ConfirmApply,
            state: original.clone(),
        },
        &mut library,
    );
    assert!(stale_confirm.state.is_none());
    let imported = apply(&mut service, original.clone(), &mut library);
    assert_eq!(imported.snapshot.phase, Phase::Result);
    assert_eq!(
        imported
            .state
            .unwrap()
            .settings
            .value(SettingId::FieldOfView),
        80
    );
    assert!(storage::has_backup(&service.config_root));
    let repeated = service.act(
        Request {
            action: Action::ConfirmApply,
            state: original,
        },
        &mut library,
    );
    assert!(repeated.state.is_none());
}
