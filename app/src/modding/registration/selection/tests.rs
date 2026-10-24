use super::*;

/// Reads the exact canonical list consumed by the watcher.
fn read(directory: &Path) -> Registration {
    Observer::default()
        .observe(&directory.join(REGISTRATION_FILE))
        .result
        .unwrap()
        .unwrap()
}

#[test]
fn single_selection_migrates_without_losing_grants_or_panel_sources() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let old = serde_json::json!({"version":1,"request_id":"legacy","enabled":true,"component":root.join("a.wasm"),"font":root.join("font.ttf"),"ui":root.join("ui.json"),"grants":{"camera":true,"settings":true,"commands":["ability"]}});
    write_json(&root.join(REGISTRATION_FILE), &old).unwrap();
    let migrated = read(root);
    assert_eq!(migrated.version, super::super::VERSION);
    assert!(migrated.mods[0].grants.camera && migrated.mods[0].grants.settings);
    assert_eq!(migrated.mods[0].grants.commands, ["ability"]);
    select(root, &root.join("b.wasm"), true, "enable-b".into()).unwrap();
    let kept = read(root);
    assert_eq!(kept.mods.len(), 2);
    assert_eq!(kept.mods[0], migrated.mods[0]);
    assert_eq!(kept.mods[1].grants, ModGrants::default());
}

#[test]
fn per_mod_permissions_survive_toggle_and_reorder_but_not_removal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (a, b) = (root.join("a.wasm"), root.join("b.wasm"));
    std::fs::write(&a, b"text fixture").unwrap();
    std::fs::write(&b, b"text fixture").unwrap();
    select(root, &a, true, "a".into()).unwrap();
    select(root, &b, true, "b".into()).unwrap();
    let camera = ModGrants::CAPABILITIES
        .iter()
        .position(|capability| capability.name == "camera")
        .unwrap();
    permission(root, &a, camera, true, "grant".into()).unwrap();
    let prior = read(root).mods[0].clone();
    disable(root, &a, "disable".into()).unwrap();
    select(root, &a, true, "enable".into()).unwrap();
    move_entry(root, &a, false, "down".into()).unwrap();
    let reordered = read(root);
    assert_eq!(reordered.mods[1], prior);
    assert!(!reordered.mods[0].grants.camera);
    forget(root, &a, "remove".into()).unwrap();
    select(root, &a, true, "replacement".into()).unwrap();
    assert_eq!(read(root).mods[1].grants, ModGrants::default());
}

#[test]
fn stale_card_commands_preserve_the_other_active_mod() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let a = root.join("a.wasm");
    let b = root.join("b.wasm");
    select(root, &a, true, "a".into()).unwrap();
    select(root, &b, true, "b".into()).unwrap();
    disable(root, &a, "off".into()).unwrap();
    let before = std::fs::read(root.join(REGISTRATION_FILE)).unwrap();
    for result in [
        disable(root, &a, "stale".into()),
        reload(root, &a, "stale".into()),
        move_entry(root, &a, true, "stale".into()),
    ] {
        assert!(result.is_err());
    }
    assert_eq!(std::fs::read(root.join(REGISTRATION_FILE)).unwrap(), before);
    assert!(read(root).mods[1].enabled);
}

#[test]
fn reload_requests_are_scoped_and_removal_receipts_cover_all_fields() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let a = root.join("a.wasm");
    let b = root.join("b.wasm");
    select(root, &a, true, "a".into()).unwrap();
    select(root, &b, true, "b".into()).unwrap();
    reload(root, &a, "reload-a".into()).unwrap();
    let selected = read(root);
    assert_eq!(selected.mods[0].reload, 1);
    assert_eq!(selected.mods[1].reload, 0);
    reload_all(root, "all".into()).unwrap();
    let selected = read(root);
    assert_eq!(selected.mods[0].reload, 2);
    assert_eq!(selected.mods[1].reload, 1);
    let receipt = forget(root, &a, "remove".into()).unwrap();
    receipt.confirm(root).unwrap();
    select(root, &a, true, "remove".into()).unwrap();
    assert!(receipt.confirm(root).is_err());
}

#[test]
fn registration_rejects_duplicate_identity_and_excess_entries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut selected = Registration::empty();
    selected.mods = vec![
        Entry {
            component: root.join("a.wasm"),
            enabled: true,
            ..Default::default()
        };
        2
    ];
    assert!(selected.validate().is_err());
    selected.mods = (0..=mod_api::MAX_LOADED_MODS)
        .map(|index| Entry {
            component: root.join(format!("{index}.wasm")),
            enabled: true,
            ..Default::default()
        })
        .collect();
    assert!(selected.validate().is_err());
}

#[test]
fn permission_revocation_survives_malformed_or_missing_component_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let component = resolved_component(&root.join("mod.wasm"));
    let requested = ModGrants {
        commands: vec!["ability".into()],
        ..Default::default()
    };
    let mut grants = ModGrants::default();
    for capability in ModGrants::CAPABILITIES {
        (capability.set)(&mut grants, true, Some(&requested));
    }
    for missing in [false, true] {
        std::fs::write(&component, b"\0asm").unwrap();
        if missing {
            std::fs::remove_file(&component).unwrap();
        }
        assert!(mod_host::declarations(&component).is_err());
        for (index, capability) in ModGrants::CAPABILITIES.iter().enumerate() {
            let entry = Entry {
                component: component.clone(),
                enabled: true,
                grants: grants.clone(),
                ..Default::default()
            };
            let mut registration = Registration::empty();
            registration.mods.push(entry.clone());
            write_selection(&root.join(REGISTRATION_FILE), registration).unwrap();
            permission(root, &component, index, false, "revoke".into()).unwrap();
            let mut expected = entry;
            (capability.set)(&mut expected.grants, false, None);
            assert_eq!(read(root).mods, [expected]);
            let revoked = std::fs::read(root.join(REGISTRATION_FILE)).unwrap();
            assert!(permission(root, &component, index, true, "grant".into()).is_err());
            assert_eq!(
                std::fs::read(root.join(REGISTRATION_FILE)).unwrap(),
                revoked
            );
        }
    }
}
