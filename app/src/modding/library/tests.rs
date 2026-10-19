use super::*;
use mod_host::ModHost;

/// Writes a binary component with the host's declared-request custom section.
fn requested_component(path: &Path, request: &mod_host::ModGrants) {
    /// Appends a section's unsigned LEB128 byte length.
    fn length(mut value: usize, bytes: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            bytes.push(byte | if value == 0 { 0 } else { 0x80 });
            if value == 0 {
                break;
            }
        }
    }
    let name = mod_host::REQUESTED_GRANTS_SECTION;
    let mut section = Vec::new();
    length(name.len(), &mut section);
    section.extend_from_slice(name.as_bytes());
    section.extend_from_slice(&serde_json::to_vec(request).unwrap());
    let mut bytes = wat::parse_str("(component)").unwrap();
    bytes.push(0);
    length(section.len(), &mut bytes);
    bytes.extend(section);
    fs::write(path, bytes).unwrap();
}

/// Writes requested permissions and a version into a binary component's declarations.
fn versioned_component(path: &Path, request: &mod_host::ModGrants, version: &str) {
    requested_component(path, request);
    let mut bytes = fs::read(path).unwrap();
    let section = [&[7u8][..], b"version", version.as_bytes()].concat();
    bytes.extend([0, u8::try_from(section.len()).unwrap()]);
    bytes.extend(section);
    fs::write(path, bytes).unwrap();
}

#[test]
fn source_revisions_refresh_cached_declarations() {
    for external in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mods = directory.path().join("mods");
        let registration = directory.path().join("config");
        fs::create_dir_all(&mods).unwrap();
        let path = if external {
            directory.path().join("zoom.wasm")
        } else {
            mods.join("zoom.wasm")
        };
        let mut requested = mod_host::ModGrants {
            camera: true,
            ..Default::default()
        };
        versioned_component(&path, &requested, "1.0");
        select(&registration, &path, true, "enable".into()).unwrap();
        let mut cache = BTreeMap::new();
        for version in ["1.0", "2.0-rebuilt", "3.0-replaced"] {
            if version != "1.0" {
                requested.settings = true;
                if version == "3.0-replaced" {
                    let replacement = directory.path().join("replacement.wasm");
                    versioned_component(&replacement, &requested, version);
                    fs::remove_file(&path).unwrap();
                    fs::rename(replacement, &path).unwrap();
                } else {
                    versioned_component(&path, &requested, version);
                }
            }
            let library = scan_cached(&mods, &registration, String::new(), &mut cache);
            let [item] = library.installed.as_slice() else {
                panic!("one registered component");
            };
            assert_eq!(
                item.metadata.version.as_deref(),
                Some(version),
                "external: {external}"
            );
            assert_eq!(library.active[0].id, item.id);
            for (permission, capability) in item
                .permissions
                .iter()
                .zip(mod_host::ModGrants::CAPABILITIES)
            {
                assert_eq!(permission.requested, Some((capability.enabled)(&requested)));
                assert!(
                    !permission.enabled,
                    "refreshing declarations cannot grant permission"
                );
            }
            let unchanged = scan_cached(&mods, &registration, String::new(), &mut cache);
            assert_eq!(unchanged, library);
            assert_eq!(cache.len(), 1);
        }
        fs::remove_file(&path).unwrap();
        let missing = scan_cached(&mods, &registration, String::new(), &mut cache);
        assert_eq!(missing.installed[0].metadata, Default::default());
        versioned_component(&path, &requested, "4.0");
        let restored = scan_cached(&mods, &registration, String::new(), &mut cache);
        assert_eq!(
            restored.installed[0].metadata.version.as_deref(),
            Some("4.0")
        );
        assert_eq!(cache.len(), 1);
    }
}

#[test]
fn batch_permission_actions_grant_exact_requests_and_revoke_all() {
    let directory = tempfile::tempdir().unwrap();
    let (mods, registration) = (
        directory.path().join("mods"),
        directory.path().join("config"),
    );
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("zoom.wasm");
    let requested = mod_host::ModGrants {
        camera: true,
        settings: true,
        commands: vec!["ability".into()],
        ..Default::default()
    };
    requested_component(&component, &requested);
    let other = mods.join("other.wasm");
    fs::write(&other, b"text fixture").unwrap();
    select(&registration, &component, true, "enable".into()).unwrap();
    select(&registration, &other, true, "other".into()).unwrap();
    selection::permission(&registration, &component, 0, true, "extra".into()).unwrap();
    let before = registered(&registration);
    let shown = scan(&mods, &registration, String::new());
    let id = shown
        .installed
        .iter()
        .find(|item| item.file == "zoom.wasm")
        .unwrap()
        .id;
    let mut requests = 0;
    perform(
        Action::AllowRequested(id),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    let allowed = registered(&registration);
    assert_eq!(allowed[0].entry.grants, requested);
    assert_eq!(allowed[1].entry, before[1].entry);
    assert_eq!(requests, 1);
    let scanned = scan(&mods, &registration, String::new());
    let item = scanned.installed.iter().find(|item| item.id == id).unwrap();
    for (permission, capability) in item
        .permissions
        .iter()
        .zip(mod_host::ModGrants::CAPABILITIES)
    {
        assert_eq!(permission.name, capability.label);
        assert_eq!(permission.sensitive, capability.sensitive);
        assert_eq!(permission.requested, Some((capability.enabled)(&requested)));
    }
    let updated = mod_host::ModGrants {
        commands: vec!["ability".into(), "say".into()],
        ..requested.clone()
    };
    requested_component(&component, &updated);
    let updated_card = scan(&mods, &registration, String::new());
    let updated_card = updated_card
        .installed
        .iter()
        .find(|item| item.file == "zoom.wasm")
        .unwrap();
    assert_eq!(updated_card.requested_counts(), (2, 3));
    assert!(
        updated_card
            .permissions
            .iter()
            .find(|permission| permission.name == "Commands")
            .unwrap()
            .enabled
    );
    fs::remove_file(&component).unwrap();
    let missing = scan(&mods, &registration, String::new()).active[0].id;
    perform(
        Action::RevokeAll(missing),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    let revoked = registered(&registration);
    assert_eq!(revoked[0].entry.grants, mod_host::ModGrants::default());
    assert!(revoked[0].entry.enabled);
    assert_eq!(revoked[1].entry, before[1].entry);
    assert_eq!(requests, 2);
}

#[test]
fn undeclared_and_malformed_requests_cannot_get_batch_grants() {
    let directory = tempfile::tempdir().unwrap();
    let (mods, registration) = (
        directory.path().join("mods"),
        directory.path().join("config"),
    );
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("mod.wasm");
    select(&registration, &component, true, "enable".into()).unwrap();
    let record = registration.join(super::super::registration::REGISTRATION_FILE);
    let before = fs::read(&record).unwrap();
    for bytes in [b"undeclared".as_slice(), b"\0asm".as_slice()] {
        fs::write(&component, bytes).unwrap();
        let id = scan(&mods, &registration, String::new()).active[0].id;
        assert!(
            perform(
                Action::AllowRequested(id),
                &mods,
                &registration,
                &mut 0,
                &mut || Ok(None),
                &mut |_| Ok(retirement_permit())
            )
            .is_err()
        );
        assert_eq!(fs::read(&record).unwrap(), before);
    }
}

/// Models a completed retirement whose watcher generation remains current.
fn retirement_permit() -> removal::Permit {
    removal::Permit::new(
        1,
        Arc::new(std::sync::atomic::AtomicU64::new(1)),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    )
}

/// Creates a directory alias, or reports the missing Windows symlink privilege fixture.
#[cfg(any(unix, windows))]
fn directory_alias(target: &Path, alias: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, alias);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, alias);
    match result {
        Ok(()) => true,
        #[cfg(windows)]
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("Missing fixture: Windows directory symlink privilege is unavailable");
            false
        }
        Err(error) => panic!("create directory alias: {error}"),
    }
}

/// A stale Disable command cannot revoke the component which replaced its displayed card.
#[test]
fn disabling_a_stale_card_preserves_the_new_selection() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let first = mods.join("first.wasm");
    let second = mods.join("second.wasm");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();
    select(&registration, &first, true, "first".into()).unwrap();
    let shown = scan(&mods, &registration, String::new());
    let displayed_id = shown.active[0].id;
    disable(&registration, &first, "disabled".into()).unwrap();
    select(&registration, &second, true, "second".into()).unwrap();
    let record = registration.join(super::super::registration::REGISTRATION_FILE);
    let before = fs::read(&record).unwrap();
    let error = perform(
        Action::Disable(displayed_id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap_err();
    assert!(error.contains("no longer enabled"), "{error}");
    assert_eq!(fs::read(&record).unwrap(), before);
    let current = registered(&registration)
        .into_iter()
        .find(|current| current.entry.enabled)
        .unwrap();
    assert!(current.entry.enabled && same_path(&current.entry.component, &second));
    let id = scan(&mods, &registration, String::new()).active[0].id;
    perform(
        Action::Disable(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    assert!(
        !registered(&registration)
            .iter()
            .any(|current| current.entry.enabled)
    );
}

/// Re-enabling after a successful retirement reply must still prevent file deletion.
#[test]
fn removal_rechecks_selection_after_retirement_returns() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("zoom.wasm");
    let companion = settings_companion(&component);
    fs::write(&component, b"component").unwrap();
    fs::write(&companion, b"preferences").unwrap();
    let id = installed(&mods)[0].0.id;
    let error = perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_: &Receipt| {
            select(&registration, &component, true, "replacement".into()).unwrap();
            Ok(retirement_permit())
        },
    )
    .unwrap_err();
    assert!(error.contains("selection changed"), "{error}");
    assert_eq!(fs::read(component).unwrap(), b"component");
    assert_eq!(fs::read(companion).unwrap(), b"preferences");
    assert!(
        registered(&registration)
            .iter()
            .any(|current| current.entry.enabled)
    );
}

/// A newer applied generation blocks deletion even if the registration bytes stay identical.
#[test]
fn removal_rechecks_generation_after_retirement_returns() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("zoom.wasm");
    let companion = settings_companion(&component);
    fs::write(&component, b"component").unwrap();
    fs::write(&companion, b"preferences").unwrap();
    let id = installed(&mods)[0].0.id;
    let error = perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| {
            Ok(removal::Permit::new(
                1,
                Arc::new(std::sync::atomic::AtomicU64::new(2)),
                Arc::new(std::sync::Mutex::new(Vec::new())),
            ))
        },
    )
    .unwrap_err();
    assert!(error.contains("selection changed"), "{error}");
    assert_eq!(fs::read(component).unwrap(), b"component");
    assert_eq!(fs::read(companion).unwrap(), b"preferences");
}

/// Real and linked directory spellings select, disable and remove the same component.
#[cfg(any(unix, windows))]
#[test]
fn removal_resolves_the_selected_components_directory_alias() {
    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("real");
    let mods = directory.path().join("linked");
    let registration = directory.path().join("config");
    fs::create_dir_all(&real).unwrap();
    if !directory_alias(&real, &mods) {
        return;
    }
    let component = real.join("zoom.wasm");
    let companion = settings_companion(&component);
    fs::write(&component, b"component").unwrap();
    fs::write(&companion, b"preferences").unwrap();
    select(&registration, &component, true, "selected".into()).unwrap();
    let record = registration.join(super::super::registration::REGISTRATION_FILE);
    let mut granted: serde_json::Value =
        serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    granted["mods"][0]["grants"]["settings"] = true.into();
    fs::write(&record, serde_json::to_vec(&granted).unwrap()).unwrap();
    let id = scan(&mods, &registration, String::new()).active[0].id;
    perform(
        Action::Disable(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    select(
        &registration,
        &mods.join("zoom.wasm"),
        true,
        "reenabled".into(),
    )
    .unwrap();
    let kept: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    assert_eq!(kept["mods"][0]["grants"], granted["mods"][0]["grants"]);
    perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_: &Receipt| {
            assert!(
                !registered(&registration)
                    .iter()
                    .any(|current| current.entry.enabled)
            );
            Ok(retirement_permit())
        },
    )
    .unwrap();
    let cleared: serde_json::Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(cleared["mods"], serde_json::json!([]));
    assert!(!component.exists() && !companion.exists());
}

/// Companion ownership also follows directory aliases, including an absent companion entry.
#[cfg(any(unix, windows))]
#[test]
fn removal_preserves_shared_preferences_through_a_directory_alias() {
    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("real");
    let mods = directory.path().join("linked");
    let registration = directory.path().join("config");
    fs::create_dir_all(&real).unwrap();
    if !directory_alias(&real, &mods) {
        return;
    }
    let component = real.join("zoom.wasm");
    let selected = real.join("zoom.wat");
    let companion = settings_companion(&component);
    fs::write(&component, b"component").unwrap();
    fs::write(&selected, b"selected component").unwrap();
    assert!(same_path(
        &companion,
        &settings_companion(&mods.join("zoom.wat"))
    ));
    fs::write(&companion, b"selected preferences").unwrap();
    select(&registration, &selected, true, "selected".into()).unwrap();
    let id = installed(&mods)[0].0.id;
    perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    assert_eq!(fs::read(companion).unwrap(), b"selected preferences");
    assert!(!component.exists() && selected.exists());
}

/// Import reads share the host's source ceiling and require regular files.
#[test]
fn import_rejects_nonregular_and_oversized_sources() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let source = directory.path().join("large.wasm");
    fs::File::create(&source)
        .unwrap()
        .set_len(mod_host::MAX_COMPONENT_BYTES as u64 + 1)
        .unwrap();
    assert!(import(&mods, &source).unwrap_err().contains("byte limit"));
    fs::remove_file(&source).unwrap();
    fs::create_dir(&source).unwrap();
    assert!(
        import(&mods, &source)
            .unwrap_err()
            .contains("Could not read")
    );
    assert!(!mods.exists());
}

/// Linked source components cannot bypass the host's admission rules through Import.
#[cfg(unix)]
#[test]
fn import_rejects_a_linked_source_component() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let source = directory.path().join("source.wasm");
    let link = directory.path().join("linked.wasm");
    fs::write(&source, component("")).unwrap();
    std::os::unix::fs::symlink(&source, &link).unwrap();
    assert!(import(&mods, &link).unwrap_err().contains("Could not read"));
    assert!(!mods.exists());
}

/// Hidden filenames are rejected before any component is installed or reported as imported.
#[test]
fn import_rejects_hidden_component_names() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let source = directory.path().join(".zoom.wasm");
    fs::write(&source, component("")).unwrap();
    let error = import(&mods, &source).unwrap_err();
    assert!(error.contains("must not start with a dot"), "{error}");
    assert!(!mods.exists());
    assert!(installed(&mods).is_empty());
}

/// A final retirement save happens before deletion, so it cannot recreate the companion.
#[test]
fn removal_waits_for_pending_settings_before_deleting_files() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let path = mods.join("zoom.wasm");
    let companion = settings_companion(&path);
    fs::write(&path, b"component").unwrap();
    fs::write(&companion, b"old preferences").unwrap();
    select(&registration, &path, true, "selected".into()).unwrap();
    let id = installed(&mods)[0].0.id;
    let retire = |_: &Receipt| {
        assert!(
            !registered(&registration)
                .iter()
                .any(|current| current.entry.enabled)
        );
        assert!(path.exists() && companion.exists());
        fs::write(&companion, b"final preferences").unwrap();
        Ok(retirement_permit())
    };
    let (commands, incoming) = unbounded();
    let (outgoing, published) = unbounded();
    commands.send(Command::Action(Action::Remove(id))).unwrap();
    drop(commands);
    run(
        &mods,
        &registration,
        &incoming,
        &outgoing,
        || Ok(None),
        retire,
    );
    let updates: Vec<_> = published.try_iter().collect();
    assert_eq!(
        updates
            .iter()
            .map(|library| library.busy)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    assert!(updates[2].message.is_empty() && updates[2].installed.is_empty());
    assert!(!path.exists() && !companion.exists());
}

/// Failed retirement keeps both files available for a later removal attempt.
#[test]
fn removal_keeps_files_when_retirement_cannot_complete() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let path = mods.join("zoom.wasm");
    let companion = settings_companion(&path);
    fs::write(&path, b"component").unwrap();
    fs::write(&companion, b"preferences").unwrap();
    let id = installed(&mods)[0].0.id;
    let error = perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| Err("host stopped".into()),
    )
    .unwrap_err();
    assert_eq!(error, "host stopped");
    assert_eq!(fs::read(&path).unwrap(), b"component");
    assert_eq!(fs::read(&companion).unwrap(), b"preferences");
}

/// Components sharing a settings anchor cannot delete the selected mod's preferences.
#[test]
fn removal_preserves_a_companion_shared_with_the_selected_component() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let removed = mods.join("zoom.wasm");
    let selected = mods.join("zoom.wat");
    let companion = settings_companion(&removed);
    fs::write(&removed, b"removed component").unwrap();
    fs::write(&selected, b"selected component").unwrap();
    fs::write(&companion, b"selected preferences").unwrap();
    select(&registration, &selected, true, "selected".into()).unwrap();
    let id = installed(&mods)[0].0.id;
    perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    assert!(!removed.exists());
    assert_eq!(fs::read(&selected).unwrap(), b"selected component");
    assert_eq!(fs::read(&companion).unwrap(), b"selected preferences");
    assert!(
        registered(&registration)
            .iter()
            .any(|current| current.entry.enabled)
    );
}

/// Builds a minimal mod with a distinct frame body for import tests.
fn component(frame: &str) -> String {
    format!(
        r#"(component
                (core module $m
                    (func (export "init"))
                    (func (export "frame") {frame}))
                (core instance $i (instantiate $m))
                (func (export "init") (canon lift (core func $i "init")))
                (func (export "frame") (canon lift (core func $i "frame"))))"#
    )
}

/// A filename collision leaves the enabled component, grants and registration untouched.
#[test]
fn importing_the_same_name_never_replaces_the_enabled_mod() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&registration).unwrap();
    let source = directory.path().join("zoom.wasm");
    let original = component("");
    fs::write(&source, &original).unwrap();
    assert_eq!(import(&mods, &source).unwrap(), "zoom.wasm");
    let installed = mods.join("zoom.wasm");
    select(&registration, &installed, true, "enabled".into()).unwrap();
    let record = registration.join(super::super::registration::REGISTRATION_FILE);
    let mut granted: serde_json::Value =
        serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    granted["mods"][0]["grants"]["hud"] = true.into();
    fs::write(&record, serde_json::to_vec(&granted).unwrap()).unwrap();
    let previous = fs::read(&record).unwrap();

    fs::write(&source, component("unreachable")).unwrap();
    let error = import(&mods, &source).unwrap_err();
    assert!(error.contains("already installed"), "{error}");
    assert_eq!(fs::read(&installed).unwrap(), original.as_bytes());
    assert_eq!(fs::read(&record).unwrap(), previous);
    assert!(
        registered(&registration)
            .iter()
            .any(|current| current.entry.enabled)
    );
    assert_eq!(fs::read_dir(&mods).unwrap().count(), 1);
}

/// Cancelled imports always finish even when the idle library and message are unchanged.
#[test]
fn cancelling_import_publishes_idle_and_allows_more_commands() {
    let directory = tempfile::tempdir().unwrap();
    let (commands, incoming) = unbounded();
    let (outgoing, published) = unbounded();
    commands.send(Command::Action(Action::Import)).unwrap();
    commands.send(Command::Action(Action::Import)).unwrap();
    drop(commands);
    run(
        &directory.path().join("mods"),
        &directory.path().join("config"),
        &incoming,
        &outgoing,
        || Ok(None),
        |_| Ok(retirement_permit()),
    );
    let updates: Vec<_> = published.try_iter().collect();
    assert_eq!(
        updates
            .iter()
            .map(|library| library.busy)
            .collect::<Vec<_>>(),
        [false, true, false, true, false]
    );
    let mut snapshot = launcher::mods::Snapshot::default();
    snapshot.publish(updates[4].clone());
    assert_eq!(snapshot.library, updates[0]);
    for action in [
        Action::Import,
        Action::Enable(1),
        Action::Disable(1),
        Action::Remove(1),
    ] {
        assert_eq!(snapshot.apply(action), Some(action));
    }
}

/// Existing links cannot redirect staging writes or be replaced at publication.
#[cfg(unix)]
#[test]
fn publishing_never_overwrites_destination_or_staging_links() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    fs::create_dir_all(&mods).unwrap();
    let target = directory.path().join("outside.wasm");
    let destination = mods.join("zoom.wasm");
    symlink(&target, &destination).unwrap();
    assert!(publish_component(&mods, "zoom.wasm", b"replacement").is_err());
    assert_eq!(fs::read_link(&destination).unwrap(), target);
    assert!(!target.exists());

    fs::remove_file(&destination).unwrap();
    fs::write(&target, b"original").unwrap();
    let staging = mods.join(format!(".zoom.wasm.{}.importing", std::process::id()));
    symlink(&target, &staging).unwrap();
    assert!(publish_component(&mods, "zoom.wasm", b"replacement").is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert_eq!(fs::read_link(&staging).unwrap(), target);
    assert!(!destination.exists());
}

/// A bad package is rejected before anything lands in the mods directory.
#[test]
fn import_rejects_a_bad_package_and_installs_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let source = directory.path().join("broken.wasm");
    fs::write(&source, b"not a component").unwrap();
    let error = import(&mods, &source).unwrap_err();
    assert!(error.contains("broken.wasm is not a valid mod"), "{error}");
    fs::write(&source, b"(component)").unwrap();
    let error = import(&mods, &source).unwrap_err();
    assert!(error.contains("component lacks"), "{error}");
    let text = directory.path().join("notes.txt");
    fs::write(&text, b"(component)").unwrap();
    assert!(import(&mods, &text).is_err());
    assert!(installed(&mods).is_empty());
    assert!(!mods.exists());
}

/// Removal clears only the removed entry and its retained authorization.
#[test]
fn removing_the_enabled_mod_disables_its_registration() {
    let directory = tempfile::tempdir().unwrap();
    let (mods, registration) = (
        directory.path().join("mods"),
        directory.path().join("config"),
    );
    fs::create_dir_all(&mods).unwrap();
    fs::create_dir_all(&registration).unwrap();
    fs::write(mods.join("zoom.wasm"), b"component").unwrap();
    fs::write(mods.join("zoom.settings.json"), b"{}").unwrap();
    let library = scan(&mods, &registration, String::new());
    let id = library.installed[0].id;
    let mut requests = 0;
    perform(
        Action::Enable(id),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    assert_eq!(scan(&mods, &registration, String::new()).active[0].id, id);
    let record = registration.join(super::super::registration::REGISTRATION_FILE);
    let mut granted: serde_json::Value =
        serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    granted["mods"][0]["grants"]["settings"] = true.into();
    granted["mods"][0]["font"] = serde_json::to_value(directory.path().join("font.ttf")).unwrap();
    granted["mods"][0]["ui"] = serde_json::to_value(directory.path().join("ui.json")).unwrap();
    fs::write(&record, serde_json::to_vec(&granted).unwrap()).unwrap();
    perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    let after = scan(&mods, &registration, String::new());
    assert!(after.installed.is_empty() && after.active.is_empty());
    assert!(
        !registered(&registration)
            .iter()
            .any(|current| current.entry.enabled)
    );
    assert!(!mods.join("zoom.settings.json").exists());
    let source = directory.path().join("zoom.wasm");
    fs::write(&source, component("unreachable")).unwrap();
    import(&mods, &source).unwrap();
    let id = installed(&mods)[0].0.id;
    perform(
        Action::Enable(id),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    let replacement: serde_json::Value =
        serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(
        replacement["mods"][0]["grants"],
        serde_json::to_value(mod_host::ModGrants::default()).unwrap()
    );
    assert!(replacement["mods"][0]["font"].is_null() && replacement["mods"][0]["ui"].is_null());
}

#[test]
fn a_replaced_file_rejects_its_stale_card_without_granting_or_enabling_the_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let path = mods.join("a.wasm");
    fs::write(&path, b"old file").unwrap();
    let old_id = installed(&mods)[0].0.id;
    fs::write(&path, b"replacement with different bytes").unwrap();
    for action in [
        Action::Enable(old_id),
        Action::Disable(old_id),
        Action::Reload(old_id),
        Action::MoveUp(old_id),
        Action::Permission(old_id, 0, true),
        Action::Remove(old_id),
    ] {
        let error = perform(
            action,
            &mods,
            &registration,
            &mut 0,
            &mut || Ok(None),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap_err();
        assert!(error.contains("no longer installed"));
    }
    assert!(registered(&registration).is_empty());
    assert_eq!(fs::read(path).unwrap(), b"replacement with different bytes");
}

#[test]
fn replacement_during_retirement_does_not_delete_the_new_component_or_its_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let path = mods.join("a.wasm");
    let companion = settings_companion(&path);
    fs::write(&path, b"old file").unwrap();
    fs::write(&companion, b"preferences").unwrap();
    let id = installed(&mods)[0].0.id;
    let error = perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| {
            fs::write(&path, b"new replacement file").unwrap();
            Ok(retirement_permit())
        },
    )
    .unwrap_err();
    assert!(error.contains("changed while removal"));
    assert_eq!(fs::read(path).unwrap(), b"new replacement file");
    assert_eq!(fs::read(companion).unwrap(), b"preferences");
}

#[test]
fn removing_an_import_preserves_a_live_environment_hosts_shared_companion() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let imported = mods.join("zoom.wasm");
    let environment = mods.join("zoom.wat");
    let companion = settings_companion(&imported);
    fs::write(&imported, b"imported").unwrap();
    fs::write(&environment, component("")).unwrap();
    fs::write(&companion, b"{\"saved\":true}").unwrap();
    let host = ModHost::load_with_grants(
        &environment,
        mod_host::ModGrants {
            settings: true,
            ..Default::default()
        },
    )
    .unwrap();
    let owners = Arc::new(std::sync::Mutex::new(vec![
        host.settings_path().unwrap().to_owned(),
    ]));
    let id = installed(&mods)
        .into_iter()
        .find(|(_, path)| path == &imported)
        .unwrap()
        .0
        .id;
    perform(
        Action::Remove(id),
        &mods,
        &registration,
        &mut 0,
        &mut || Ok(None),
        &mut |_| {
            Ok(removal::Permit::new(
                1,
                Arc::new(std::sync::atomic::AtomicU64::new(1)),
                owners.clone(),
            ))
        },
    )
    .unwrap();
    assert!(!imported.exists());
    assert!(host.is_active());
    assert_eq!(fs::read(&companion).unwrap(), b"{\"saved\":true}");
    drop(host);
    assert!(companion.exists());
}

#[test]
fn active_cards_remain_actionable_after_their_source_disappears() {
    for rename_directory in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mods = directory.path().join("mods");
        let registration = directory.path().join("config");
        fs::create_dir_all(&mods).unwrap();
        let component = mods.join("zoom.wasm");
        fs::write(&component, b"component").unwrap();
        select(&registration, &component, true, "enabled".into()).unwrap();
        let camera = mod_host::ModGrants::CAPABILITIES
            .iter()
            .position(|capability| capability.name == "camera")
            .unwrap();
        selection::permission(&registration, &component, camera, true, "enabled".into()).unwrap();
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(registration.join(super::super::registration::REGISTRATION_FILE)).unwrap(),
        )
        .unwrap();
        fs::write(registration.join(super::super::registration::STATUS_FILE),
            serde_json::to_vec(&serde_json::json!({
                "version": record["version"], "request_id": record["request_id"],
                "independent_registration": true, "client_pid": std::process::id(),
                "mods": [{"component": record["mods"][0]["component"], "state": "reload-error", "message": "source missing", "reload": record["mods"][0]["reload"].as_u64().unwrap_or_default()}]
            })).unwrap()).unwrap();
        let original_id = scan(&mods, &registration, String::new()).active[0].id;
        if rename_directory {
            fs::rename(&mods, directory.path().join("moved")).unwrap();
        } else {
            fs::remove_file(&component).unwrap();
        }
        let library = scan(&mods, &registration, String::new());
        assert_eq!(library.active.len(), 1);
        assert_eq!(
            library.active[0].status,
            launcher::mods::Status::ReloadFailed("source missing".into())
        );
        let (active, available) = library.groups();
        assert_eq!(active.len(), 1);
        assert!(available.is_empty());
        assert_eq!(active[0].name(), "zoom");
        assert_eq!(active[0].file, "zoom.wasm");
        let id = active[0].id;
        assert_ne!(id, original_id);
        assert!(active[0].permissions[camera].enabled);
        assert!(
            perform(
                Action::Remove(id),
                &mods,
                &registration,
                &mut 0,
                &mut || Ok(None),
                &mut |_| Ok(retirement_permit())
            )
            .is_err()
        );
        assert!(registered(&registration)[0].entry.enabled);
        perform(
            Action::Permission(id, camera, false),
            &mods,
            &registration,
            &mut 0,
            &mut || Ok(None),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap();
        assert!(!registered(&registration)[0].entry.grants.camera);
        perform(
            Action::Reload(id),
            &mods,
            &registration,
            &mut 0,
            &mut || Ok(None),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap();
        assert_eq!(registered(&registration)[0].entry.reload, 1);
        perform(
            Action::Disable(id),
            &mods,
            &registration,
            &mut 0,
            &mut || Ok(None),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap();
        let library = scan(&mods, &registration, String::new());
        assert!(library.active.is_empty());
        assert!(library.installed.is_empty());
        assert!(!registered(&registration)[0].entry.enabled);
    }
}

#[test]
fn a_missing_sources_card_cannot_retarget_a_replacement_file() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("zoom.wasm");
    select(&registration, &component, true, "enabled".into()).unwrap();
    let id = scan(&mods, &registration, String::new()).active[0].id;
    fs::write(&component, b"replacement").unwrap();
    for action in [
        Action::Disable(id),
        Action::Reload(id),
        Action::Remove(id),
        Action::Permission(id, 0, true),
    ] {
        assert!(
            perform(
                action,
                &mods,
                &registration,
                &mut 0,
                &mut || Ok(None),
                &mut |_| Ok(retirement_permit())
            )
            .is_err()
        );
    }
    assert!(registered(&registration)[0].entry.enabled);
    assert_eq!(fs::read(component).unwrap(), b"replacement");
}

/// Cards carry declared metadata and fall back to the file stem when it is absent.
#[test]
fn scanned_cards_show_declared_metadata_or_the_file_stem() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let mut declared = wat::parse_str("(component)").unwrap();
    let section = [&[7u8][..], b"version", b"1.2.0"].concat();
    declared.extend([0, section.len() as u8]);
    declared.extend(section);
    fs::write(
        mods.join("plain.wasm"),
        wat::parse_str("(component)").unwrap(),
    )
    .unwrap();
    fs::write(mods.join("zoom.wasm"), declared).unwrap();
    let library = scan(&mods, &registration, String::new());
    let [plain, zoom] = library.installed.as_slice() else {
        panic!("two installed cards");
    };
    assert_eq!(plain.name(), "plain");
    assert_eq!(plain.metadata, Default::default());
    assert_eq!(zoom.name(), "zoom");
    assert_eq!(zoom.metadata.version.as_deref(), Some("1.2.0"));
}

#[test]
fn importing_a_large_packet_source_keeps_bytes_and_permissions_default_off() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let path = root.path().join("large.wasm");
    let fixture = include_str!("../../../../crates/mod-host/src/packet_mod/tests/guest.wat")
        .replace("$IMPORTS", "")
        .replace("$START", "");
    let mut bytes = wat::parse_str(fixture).unwrap();
    // A large inert custom section models a runtime's size without changing its guest behavior.
    let mut section = vec![0; mod_host::MAX_COMPONENT_SOURCE_BYTES + 1];
    section[0] = 0; // Empty custom-section name.
    bytes.push(0);
    let mut length = section.len();
    loop {
        let next = (length & 127) as u8;
        length >>= 7;
        bytes.push(next | if length != 0 { 128 } else { 0 });
        if length == 0 {
            break;
        }
    }
    bytes.extend(section);
    fs::write(&path, &bytes).unwrap();
    import(&mods, &path).unwrap();
    assert_eq!(fs::read(mods.join("large.wasm")).unwrap(), bytes);
    let declared = mod_host::declarations(&mods.join("large.wasm"))
        .unwrap()
        .requested
        .unwrap()
        .unwrap();
    assert!(declared.packets);
    let selected = root.path().join("config");
    selection::select(&selected, &mods.join("large.wasm"), true, "enable".into()).unwrap();
    let entry = selection::registered(&selected).pop().unwrap().entry;
    assert!(
        !entry.grants.packets,
        "importing and enabling never silently grant packet authority"
    );
}

#[path = "tests/packet_flow.rs"]
mod packet_flow;
