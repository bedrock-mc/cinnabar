use super::*;
use candidate::build_candidate;
use std::fs;

#[path = "recovery_tests.rs"]
mod recovery_tests;

fn fixture(enabled: bool, frame: &str, text: &str) -> String {
    fixture_with_capture(enabled, frame, text, false)
}

/// Builds a panel fixture with optional key capture for rebinding tests.
fn fixture_with_capture(enabled: bool, frame: &str, text: &str, capture_key: bool) -> String {
    let package = include_str!("../../../../crates/mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let panel = format!(
        r#"{{"title":"Local controls","toggle_key":"ShiftRight","dark":true,"capture_key":{capture_key},"controls":[{{"kind":"toggle","id":"enabled","label":"Enabled","value":{enabled}}}]}}"#
    );
    let template = include_str!("../../../../crates/mod-host/src/tests/guest.wat");
    template
        .replace("(component", &format!("(component (import \"{name}/panel@{version}\" (instance $panel (export \"set-content\" (func (param \"json\" string) (result (result (error string))))))) (alias export $panel \"set-content\" (func $set-panel))"))
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
        .replace("$INPUT", &format!("{name}/input@{version}"))
        .replace("$TEXT", text)
        .replace("$LENGTH", &text.len().to_string())
        .replace("$FRAME", frame)
        .replace("(core func $lower-label", "(core func $lower-panel (canon lower (func $set-panel) (memory $memory) (realloc $realloc))) (core func $lower-label")
        .replace("(import \"host\" \"time\"", "(import \"host\" \"panel\" (func $panel (param i32 i32 i32))) (import \"host\" \"time\"")
        .replace("(export \"time\" (func $lower-time))", "(export \"panel\" (func $lower-panel)) (export \"time\" (func $lower-time))")
        .replace("(data (i32.const 0)", &format!("(data (i32.const 1024) \"{}\") (data (i32.const 0)", panel.replace('"', "\\22")))
        .replacen("(func (export \"init\")", &format!("(func (export \"init\") i32.const 1024 i32.const {} i32.const 512 call $panel", panel.len()), 1)
}

/// Prepares a real guest whose retained label reveals whether its state was restarted.
fn candidate(root: &std::path::Path, name: &str, text: &str) -> Box<Candidate> {
    let entry = Entry {
        component: root.join(name),
        enabled: true,
        grants: mod_host::ModGrants {
            controls: true,
            ..Default::default()
        },
        ..Default::default()
    };
    fs::write(
        &entry.component,
        fixture(
            true,
            "call $pressed if i32.const 1 i32.const 4 i32.const 128 call $label end",
            text,
        ),
    )
    .unwrap();
    Box::new(build_candidate(source::source_snapshot(entry).unwrap()).unwrap())
}

/// Provides frame publication with real bounded channels and no coordinator thread.
fn world() -> (World, Receiver<Message>, Receiver<Retired>) {
    let (_, updates) = bounded(1);
    let (messages, incoming) = bounded(8);
    let (retired, retiring) = bounded(mod_api::MAX_LOADED_MODS * 2);
    let mut world = World::new();
    world.insert_resource(Watcher {
        updates,
        messages,
        retired,
        latest: Arc::new(AtomicU64::new(0)),
        applied: AtomicU64::new(0),
        desired: Arc::new(Mutex::new(Desired::default())),
        alive: Arc::new(AtomicBool::new(true)),
        thread: None,
        pending: Mutex::new(None),
        acknowledgments: Mutex::new(Vec::new()),
        fallback_retired: Mutex::new(Vec::new()),
        settings: Arc::new(Mutex::new(BTreeMap::new())),
        drawing: Arc::new(Mutex::new(BTreeMap::new())),
        companions: Arc::new(Mutex::new(Vec::new())),
    });
    (world, incoming, retiring)
}

/// Publishes the desired list before applying its already prepared changes.
fn apply(world: &mut World, entries: Vec<Entry>, changes: Vec<Change>) {
    let watcher = world.resource::<Watcher>();
    let generation = watcher.latest.fetch_add(1, Ordering::AcqRel) + 1;
    let registration = Registration {
        version: super::VERSION,
        request_id: format!("request-{generation}"),
        mods: entries,
    };
    *watcher.desired.lock().unwrap() = Desired {
        generation,
        registration: Some(registration.clone()),
    };
    install::apply(
        world,
        Update {
            generation,
            registration,
            changes,
        },
    );
}

/// Wraps a prepared candidate as a single identity-scoped successful admission.
fn change(candidate: Box<Candidate>) -> Change {
    Change {
        entry: candidate.registration.clone(),
        result: Ok(candidate),
    }
}

#[test]
fn enabling_disabling_and_reordering_preserve_the_other_instances_state() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, _messages, retiring) = world();
    let first = candidate(root.path(), "a.wasm", "Hello");
    let a = first.registration.clone();
    apply(&mut world, vec![a.clone()], vec![change(first)]);
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .frame(true)
        .unwrap();
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    let second = candidate(root.path(), "b.wasm", "World");
    let b = second.registration.clone();
    apply(&mut world, vec![a.clone(), b.clone()], vec![change(second)]);
    assert_eq!(world.resource::<ModRuntime>().hosts.host_count(), 2);
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    let first_owner = world.resource::<ModRuntime>().metadata[0].owner;
    apply(&mut world, vec![b.clone(), a.clone()], Vec::new());
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(1).label(),
        Some("ello")
    );
    assert_eq!(
        world.resource::<ModRuntime>().metadata[1].owner,
        first_owner
    );
    apply(
        &mut world,
        vec![
            Entry {
                enabled: false,
                ..b
            },
            a,
        ],
        Vec::new(),
    );
    assert_eq!(world.resource::<ModRuntime>().hosts.host_count(), 1);
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    assert_eq!(retiring.len(), 1);
}

#[test]
fn failed_reload_keeps_old_instance_and_successful_reload_resets_only_its_guest() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, messages, _retiring) = world();
    let first = candidate(root.path(), "a.wasm", "Hello");
    let mut a = first.registration.clone();
    let second = candidate(root.path(), "b.wasm", "World");
    let b = second.registration.clone();
    apply(
        &mut world,
        vec![a.clone(), b.clone()],
        vec![change(first), change(second)],
    );
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .frame(true)
        .unwrap();
    messages.try_iter().for_each(drop);
    a.reload += 1;
    fs::write(&a.component, "invalid component").unwrap();
    let failed = source::source_snapshot(a.clone())
        .and_then(build_candidate)
        .map(Box::new);
    assert!(failed.is_err());
    apply(
        &mut world,
        vec![a.clone(), b.clone()],
        vec![Change {
            entry: a.clone(),
            result: failed,
        }],
    );
    let runtime = world.resource::<ModRuntime>();
    assert!(runtime.hosts.host(0).is_active());
    assert_eq!(runtime.hosts.host(0).label(), Some("ello"));
    assert_eq!(runtime.hosts.host(1).label(), Some("World"));
    let Message::Acknowledge { statuses, .. } = messages.try_recv().unwrap() else {
        panic!("reload acknowledgment")
    };
    assert_eq!(statuses[0].state, "reload-error");
    fs::write(&a.component, fixture(true, "", "Fresh")).unwrap();
    let next = Box::new(build_candidate(source::source_snapshot(a.clone()).unwrap()).unwrap());
    apply(&mut world, vec![a, b], vec![change(next)]);
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("Fresh")
    );
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(1).label(),
        Some("World")
    );
}

#[test]
fn revoking_one_mod_releases_only_its_authority_before_loader_completion() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let first = candidate(root.path(), "a.wasm", "Hello");
    let mut a = first.registration.clone();
    let second = candidate(root.path(), "b.wasm", "World");
    let b = second.registration.clone();
    apply(
        &mut world,
        vec![a.clone(), b.clone()],
        vec![change(first), change(second)],
    );
    a.enabled = false;
    world
        .resource::<Watcher>()
        .desired
        .lock()
        .unwrap()
        .registration = Some(Registration {
        version: super::VERSION,
        request_id: "off".into(),
        mods: vec![a, b],
    });
    install::sync_authority(&mut world);
    let runtime = world.resource::<ModRuntime>();
    assert!(!runtime.hosts.host(0).is_active());
    assert!(runtime.hosts.host(1).is_active());
    assert_eq!(runtime.hosts.host(1).label(), Some("World"));
}

#[test]
fn an_obsolete_completed_candidate_cannot_replace_the_current_host() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, _messages, retiring) = world();
    let first = candidate(root.path(), "a.wasm", "Hello");
    let a = first.registration.clone();
    apply(&mut world, vec![a.clone()], vec![change(first)]);
    let stale = candidate(root.path(), "a.wasm", "Stale");
    world
        .resource::<Watcher>()
        .desired
        .lock()
        .unwrap()
        .generation = 2;
    world
        .resource::<Watcher>()
        .latest
        .store(2, Ordering::Release);
    install::apply(
        &mut world,
        Update {
            generation: 1,
            registration: Registration {
                version: super::VERSION,
                request_id: "old".into(),
                mods: vec![a],
            },
            changes: vec![change(stale)],
        },
    );
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("Hello")
    );
    assert_eq!(retiring.len(), 1);
}

#[test]
fn status_messages_are_bounded_and_unloading_preserves_saved_camera_settings() {
    let status = Status::new(PathBuf::from("a"), "error", Some("é\n".repeat(1000)), None);
    let message = status.message.unwrap();
    assert!(message.len() <= status::MAX_MESSAGE_BYTES && !message.contains('\n'));
    let mut world = World::new();
    let mut camera = client_presentation::camera::CameraSettingsAuthority::default();
    let fov = camera.horizontal_fov_degrees();
    let hurt_style = camera.hurt_camera_style();
    camera.set_view_scale(0.25, 0.25);
    camera.set_hurt_camera_style(Some(render_api::HurtCameraStyle::new(2.0, true).unwrap()));
    world.insert_resource(camera);
    presentation::clear_presentation(&mut world);
    let camera = world.resource::<client_presentation::camera::CameraSettingsAuthority>();
    assert_eq!((camera.fov_scale(), camera.look_scale()), (1.0, 1.0));
    assert_eq!(camera.horizontal_fov_degrees(), fov);
    assert_eq!(camera.hurt_camera_style(), hurt_style);
}

#[test]
fn the_real_watcher_adds_and_removes_one_guest_without_restarting_its_neighbor() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let a = root.join("a.wasm");
    let b = root.join("b.wasm");
    fs::write(
        &a,
        fixture(
            false,
            "call $pressed if i32.const 1 i32.const 4 i32.const 128 call $label end",
            "Hello",
        ),
    )
    .unwrap();
    fs::write(&b, fixture(false, "", "World")).unwrap();
    selection::select(root, &a, true, "a".into()).unwrap();
    let mut app = App::new();
    app.insert_resource(Watcher::start(root.into()).unwrap());
    app.add_systems(
        bevy::prelude::Update,
        (install_pending, ApplyDeferred).chain(),
    );
    test_time::eventually("first registered guest is installed", || {
        app.update();
        app.world()
            .get_resource::<ModRuntime>()
            .is_some_and(|runtime| runtime.hosts.host_count() == 1)
    });
    app.world_mut()
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .frame(true)
        .unwrap();
    selection::select(root, &b, true, "b".into()).unwrap();
    test_time::eventually("second guest is independently installed", || {
        app.update();
        app.world()
            .get_resource::<ModRuntime>()
            .is_some_and(|runtime| runtime.hosts.host_count() == 2)
    });
    assert_eq!(
        app.world().resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    selection::disable(root, &b, "off".into()).unwrap();
    test_time::eventually("second guest is independently retired", || {
        app.update();
        app.world()
            .get_resource::<ModRuntime>()
            .is_some_and(|runtime| runtime.hosts.host_count() == 1)
    });
    assert_eq!(
        app.world().resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
}

#[cfg(unix)]
#[test]
fn normalized_registration_still_rejects_a_component_leaf_symlink() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let source = root.join("real.wasm");
    let link = root.join("linked.wasm");
    fs::write(&source, fixture(false, "", "Hello")).unwrap();
    std::os::unix::fs::symlink(&source, &link).unwrap();
    selection::select(root, &link, true, "linked".into()).unwrap();
    let registration = source::observe(&root.join(REGISTRATION_FILE))
        .result
        .unwrap()
        .unwrap();
    // Registration resolves the directory but keeps the leaf, so the link is still rejected.
    assert_eq!(
        registration.mods[0].component,
        root.canonicalize().unwrap().join("linked.wasm")
    );
    assert!(source::source_snapshot(registration.mods[0].clone()).is_err());
}

#[test]
fn environment_hosts_keep_their_state_and_panel_identity_when_registered_mods_move_ahead() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let environment = candidate(directory.path(), "environment.wasm", "Hello");
    let env = environment.registration.clone();
    apply(&mut world, vec![env], vec![change(environment)]);
    {
        let mut runtime = world.resource_mut::<ModRuntime>();
        runtime.metadata[0].registration = None;
        runtime.metadata[0].owner = mod_ui::PanelUiOwner::Environment(4);
        runtime.hosts.host_mut(0).frame(true).unwrap();
    }

    let registered = candidate(directory.path(), "registered.wasm", "World");
    let entry = registered.registration.clone();
    apply(&mut world, vec![entry], vec![change(registered)]);
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(1).label(),
        Some("ello")
    );
    assert_eq!(
        world.resource::<ModRuntime>().metadata[1].owner,
        mod_ui::PanelUiOwner::Environment(4)
    );
    apply(&mut world, Vec::new(), Vec::new());
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    assert_eq!(
        world.resource::<ModRuntime>().metadata[0].owner,
        mod_ui::PanelUiOwner::Environment(4)
    );
}

#[test]
fn restoring_grants_rebuilds_an_instance_revoked_during_pending_admission() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, messages, _retiring) = world();
    let first = candidate(directory.path(), "a.wasm", "Hello");
    let original = first.registration.clone();
    apply(&mut world, vec![original.clone()], vec![change(first)]);
    messages.try_iter().for_each(drop);
    let mut revoked = original.clone();
    revoked.grants.controls = false;
    world
        .resource::<Watcher>()
        .desired
        .lock()
        .unwrap()
        .registration = Some(Registration {
        version: super::VERSION,
        request_id: "revoke".into(),
        mods: vec![revoked],
    });
    install::sync_authority(&mut world);
    assert!(!world.resource::<ModRuntime>().hosts.host(0).is_active());
    assert!(world.resource::<ModRuntime>().hosts.host(0).is_revoked());
    // The original bytes/grants match the coordinator cache, but its guest is no longer reusable.
    apply(&mut world, vec![original.clone()], Vec::new());
    let Message::Retry { inactive, .. } = messages.try_recv().unwrap() else {
        panic!("inactive guest falsely acknowledged as running")
    };
    assert_eq!(inactive, std::slice::from_ref(&original.component));
    assert!(messages.try_recv().is_err());
    let fresh =
        Box::new(build_candidate(source::source_snapshot(original.clone()).unwrap()).unwrap());
    apply(&mut world, vec![original], vec![change(fresh)]);
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .frame(true)
        .unwrap();
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
}

#[test]
fn registered_prefix_changes_preserve_every_environment_hosts_relative_order() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let e0 = candidate(directory.path(), "e0.wasm", "First");
    let e1 = candidate(directory.path(), "e1.wasm", "Other");
    apply(
        &mut world,
        vec![e0.registration.clone(), e1.registration.clone()],
        vec![change(e0), change(e1)],
    );
    for (index, metadata) in world
        .resource_mut::<ModRuntime>()
        .metadata
        .iter_mut()
        .enumerate()
    {
        metadata.registration = None;
        metadata.owner = mod_ui::PanelUiOwner::Environment(index);
    }
    let r = candidate(directory.path(), "r.wasm", "Local");
    let entry = r.registration.clone();
    apply(&mut world, vec![entry], vec![change(r)]);
    let labels = |world: &World| {
        (0..world.resource::<ModRuntime>().hosts.host_count())
            .map(|index| {
                world
                    .resource::<ModRuntime>()
                    .hosts
                    .host(index)
                    .label()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&world), ["Local", "First", "Other"]);
    apply(&mut world, Vec::new(), Vec::new());
    assert_eq!(labels(&world), ["First", "Other"]);
}

#[test]
fn settings_owner_publication_includes_environment_loaded_hosts() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let prepared = candidate(directory.path(), "environment.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.grants.settings = true;
    let prepared =
        Box::new(build_candidate(source::source_snapshot(entry.clone()).unwrap()).unwrap());
    apply(&mut world, vec![entry.clone()], vec![change(prepared)]);
    world.resource_mut::<ModRuntime>().metadata[0].registration = None;
    let watcher = world.resource::<Watcher>();
    watcher.remember_settings(Some(world.resource::<ModRuntime>()));
    let permit = removal::Permit::new(
        watcher.latest.load(Ordering::Acquire),
        watcher.latest.clone(),
        watcher.companions.clone(),
    );
    assert!(permit.shared_companion(&mod_host::settings_companion(&entry.component)));
}

#[test]
fn unrelated_registration_changes_preserve_trapped_identities_until_explicit_reload() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, messages, _retiring) = world();
    let prepared = candidate(directory.path(), "trapped.wasm", "Hello");
    let a = prepared.registration.clone();
    fs::write(&a.component, fixture(true, "unreachable", "Hello")).unwrap();
    let trapped = Box::new(build_candidate(source::source_snapshot(a.clone()).unwrap()).unwrap());
    let identity = trapped.identity;
    apply(&mut world, vec![a.clone()], vec![change(trapped)]);
    messages.try_iter().for_each(drop);
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .frame(false)
        .unwrap_err();
    assert!(!world.resource::<ModRuntime>().hosts.host(0).is_revoked());
    let other = candidate(directory.path(), "other.wasm", "World");
    let b = other.registration.clone();
    apply(&mut world, vec![b.clone(), a.clone()], vec![change(other)]);
    let Message::Acknowledge { installed, .. } = messages.try_recv().unwrap() else {
        panic!("unrelated change attempted to retry a trapped guest")
    };
    assert_eq!(
        installed.get(&a.component),
        Some(&Admitted {
            source: identity,
            drawing: None
        })
    );
    assert!(world.resource::<ModRuntime>().hosts.host(0).is_active());
    assert!(!world.resource::<ModRuntime>().hosts.host(1).is_active());
    apply(&mut world, vec![a.clone(), b.clone()], Vec::new());
    assert!(matches!(
        messages.try_recv().unwrap(),
        Message::Acknowledge { .. }
    ));
    assert!(!world.resource::<ModRuntime>().hosts.host(0).is_active());
    // Revoking a guest that already trapped does not make an unrelated restoration retry it.
    let mut revoked = a.clone();
    revoked.grants.controls = false;
    world
        .resource::<Watcher>()
        .desired
        .lock()
        .unwrap()
        .registration
        .as_mut()
        .unwrap()
        .mods[0] = revoked;
    install::sync_authority(&mut world);
    apply(&mut world, vec![a.clone(), b.clone()], Vec::new());
    assert!(matches!(
        messages.try_recv().unwrap(),
        Message::Acknowledge { .. }
    ));
    assert!(!world.resource::<ModRuntime>().hosts.host(0).is_revoked());
    let mut reload = a;
    reload.reload += 1;
    fs::write(&reload.component, fixture(true, "", "Fresh")).unwrap();
    let fresh =
        Box::new(build_candidate(source::source_snapshot(reload.clone()).unwrap()).unwrap());
    apply(&mut world, vec![reload, b], vec![change(fresh)]);
    assert!(world.resource::<ModRuntime>().hosts.host(0).is_active());
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("Fresh")
    );
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(1).label(),
        Some("World")
    );
}

#[test]
fn reenable_before_disable_acknowledgment_retries_missing_hosts() {
    for keep_other in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (mut world, messages, _retiring) = world();
        let first = candidate(directory.path(), "a.wasm", "Hello");
        let a = first.registration.clone();
        let mut entries = vec![a.clone()];
        let mut changes = vec![change(first)];
        if keep_other {
            let other = candidate(directory.path(), "b.wasm", "World");
            entries.push(other.registration.clone());
            changes.push(change(other));
        }
        apply(&mut world, entries.clone(), changes);
        messages.try_iter().for_each(drop);
        let mut disabled = entries.clone();
        disabled[0].enabled = false;
        apply(&mut world, disabled, Vec::new());
        assert_eq!(world.contains_resource::<ModRuntime>(), keep_other);
        // Reuse arrives while the coordinator still has the old installed identity.
        apply(&mut world, entries.clone(), Vec::new());
        assert!(matches!(
            messages.try_recv().unwrap(),
            Message::Acknowledge { .. }
        ));
        let Message::Retry { inactive, .. } = messages.try_recv().unwrap() else {
            panic!("missing guest falsely acknowledged as installed")
        };
        assert_eq!(inactive, std::slice::from_ref(&a.component));
        assert!(messages.try_recv().is_err());
        let fresh = Box::new(build_candidate(source::source_snapshot(a).unwrap()).unwrap());
        apply(&mut world, entries, vec![change(fresh)]);
        let runtime = world.resource::<ModRuntime>();
        assert!(runtime.hosts.host(0).is_active());
        assert_eq!(runtime.hosts.host(0).label(), Some("Hello"));
        if keep_other {
            assert_eq!(runtime.hosts.host(1).label(), Some("World"));
        }
    }
}

#[test]
fn failed_admission_acknowledges_an_absent_host_without_retrying() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, messages, _retiring) = world();
    let entry = Entry {
        component: directory.path().join("missing.wasm"),
        enabled: true,
        ..Default::default()
    };
    let result = source::source_snapshot(entry.clone())
        .and_then(build_candidate)
        .map(Box::new);
    assert!(result.is_err());
    apply(
        &mut world,
        vec![entry.clone()],
        vec![Change { entry, result }],
    );
    let Message::Acknowledge {
        installed,
        statuses,
        ..
    } = messages.try_recv().unwrap()
    else {
        panic!("failed admission retried without a source change")
    };
    assert!(installed.is_empty());
    assert_eq!(statuses[0].state, "error");
    assert!(messages.try_recv().is_err());
}

/// A binary reactor exercises the same source kind and canonical exports as a Go build.
fn packet_bytes() -> Vec<u8> {
    let fixture = include_str!("../../../../crates/mod-host/src/packet_mod/tests/guest.wat")
        .replace("$IMPORTS", "")
        .replace("$START", "");
    wat::parse_str(fixture).unwrap()
}

#[test]
fn packet_candidates_survive_source_removal_and_revoke_on_disable() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, messages, _) = world();
    let path = root.path().join("packet.wasm");
    fs::write(&path, packet_bytes()).unwrap();
    let entry = Entry {
        component: path.clone(),
        enabled: true,
        grants: mod_host::ModGrants {
            packets: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let candidate =
        Box::new(build_candidate(source::source_snapshot(entry.clone()).unwrap()).unwrap());
    fs::remove_file(&path).unwrap();
    apply(&mut world, vec![entry.clone()], vec![change(candidate)]);
    assert!(
        !world.contains_resource::<ModRuntime>(),
        "packet guests do not enter the frame host"
    );
    let relay = world
        .resource::<crate::modding::packets::PacketMods>()
        .session_relay()
        .unwrap();
    let identity = protocol::session_wire::SessionIdentity {
        display_name: "Steve".into(),
        xuid: "1".into(),
        uuid: "u".into(),
    };
    assert_eq!(relay.start(&identity, "{}", &[1]).len(), 2);
    assert_eq!(relay.outbound(&[1]), Some(vec![0]));
    assert!(
        matches!(messages.try_recv().unwrap(), Message::Acknowledge { statuses, .. } if statuses[0].state == "next-join")
    );
    let mut disabled = entry;
    disabled.enabled = false;
    apply(&mut world, vec![disabled], Vec::new());
    assert_eq!(relay.outbound(&[1]), None);
    assert!(
        world
            .resource::<crate::modding::packets::PacketMods>()
            .session_relay()
            .is_none()
    );
}

#[test]
fn joins_wait_for_initial_observation_and_finish_after_admission() {
    let (mut world, _, _) = world();
    assert!(world.resource::<Watcher>().preparing());
    apply(&mut world, Vec::new(), Vec::new());
    assert!(!world.resource::<Watcher>().preparing());

    let root = tempfile::tempdir().unwrap();
    let entry = Entry {
        component: root.path().join("packet.wasm"),
        enabled: true,
        grants: mod_host::ModGrants {
            packets: true,
            ..Default::default()
        },
        ..Default::default()
    };
    fs::write(&entry.component, packet_bytes()).unwrap();
    let candidate =
        Box::new(build_candidate(source::source_snapshot(entry.clone()).unwrap()).unwrap());
    let watcher = world.resource::<Watcher>();
    watcher.desired.lock().unwrap().generation += 1;
    assert!(
        watcher.preparing(),
        "a newer selection is still being prepared"
    );
    apply(&mut world, vec![entry.clone()], vec![change(candidate)]);
    assert!(!world.resource::<Watcher>().preparing());
    assert!(
        world
            .resource::<crate::modding::packets::PacketMods>()
            .session_relay()
            .is_some()
    );

    apply(
        &mut world,
        vec![entry.clone()],
        vec![Change {
            entry,
            result: Err("bad source".into()),
        }],
    );
    assert!(
        !world.resource::<Watcher>().preparing(),
        "an admission error ends the wait too"
    );
}

#[test]
fn drawing_fence_changes_cannot_restore_withdrawn_registration_grants() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let first = candidate(root.path(), "a.wasm", "Hello");
    let mut entry = first.registration.clone();
    apply(&mut world, vec![entry.clone()], vec![change(first)]);
    entry.grants.controls = false;
    world
        .resource::<Watcher>()
        .desired
        .lock()
        .unwrap()
        .registration
        .as_mut()
        .unwrap()
        .mods[0] = entry;
    install::sync_authority(&mut world);
    let mut runtime = world.resource_mut::<ModRuntime>();
    let host = runtime.hosts.host_mut(0);
    host.bind_drawing_session(mod_api::drawing::DrawingSession {
        connection: 2,
        local: true,
        permissions: mod_api::drawing::DrawingPermissions::ALL,
        ..Default::default()
    });
    assert!(host.is_revoked());
    assert!(!host.needs_drawing_restart());
    assert!(!host.reload_if_changed().unwrap());
    host.frame(false).unwrap();
    assert!(host.label().is_none());
}

#[test]
fn registered_drawing_restart_uses_worker_admission_and_rejects_stale_results() {
    let root = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let first = candidate(root.path(), "a.wasm", "Hello");
    let entry = first.registration.clone();
    apply(&mut world, vec![entry.clone()], vec![change(first)]);
    let session = mod_api::drawing::DrawingSession {
        connection: 1,
        ..Default::default()
    };
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .bind_drawing_session(session);
    let context = super::super::admission::DrawingContext::capture(
        world.resource::<ModRuntime>().hosts.host(0),
    );
    let mut snapshot = source::source_snapshot(entry.clone()).unwrap();
    snapshot.drawing = context;
    let prepared = Box::new(build_candidate(snapshot).unwrap());
    assert_eq!(
        prepared.frame_host().unwrap().drawing_session(),
        Some(session)
    );
    apply(&mut world, vec![entry.clone()], vec![change(prepared)]);
    assert!(world.resource::<ModRuntime>().hosts.host(0).is_active());
    assert!(
        !world
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .needs_drawing_restart()
    );
    let mut snapshot = source::source_snapshot(entry.clone()).unwrap();
    snapshot.drawing = super::super::admission::DrawingContext::capture(
        world.resource::<ModRuntime>().hosts.host(0),
    );
    let stale = Box::new(build_candidate(snapshot).unwrap());
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .bind_drawing_session(mod_api::drawing::DrawingSession {
            connection: 2,
            ..session
        });
    apply(&mut world, vec![entry], vec![change(stale)]);
    assert!(world.resource::<ModRuntime>().hosts.host(0).is_revoked());
}

#[test]
fn the_real_watcher_restarts_unchanged_bytes_under_each_drawing_fence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("a.wasm");
    fs::write(&path, fixture(false, "", "Hello")).unwrap();
    selection::select(directory.path(), &path, true, "selected".into()).unwrap();
    let mut app = App::new();
    app.insert_resource(Watcher::start(directory.path().into()).unwrap());
    app.add_systems(
        bevy::prelude::Update,
        (install_pending, ApplyDeferred).chain(),
    );
    test_time::eventually("registered guest is admitted", || {
        app.update();
        app.world()
            .get_resource::<ModRuntime>()
            .is_some_and(|runtime| runtime.hosts.host(0).is_active())
    });
    for connection in [1, 2] {
        let session = mod_api::drawing::DrawingSession {
            connection,
            ..Default::default()
        };
        app.world_mut()
            .resource_mut::<ModRuntime>()
            .hosts
            .host_mut(0)
            .bind_drawing_session(session);
        test_time::eventually("registered drawing fence is admitted by the worker", || {
            app.update();
            let runtime = app.world().resource::<ModRuntime>();
            runtime.hosts.host(0).is_active()
                && runtime.hosts.host(0).drawing_session() == Some(session)
        });
        let runtime = app.world().resource::<ModRuntime>();
        assert_eq!(runtime.hosts.host(0).label(), Some("Hello"));
        assert!(!runtime.hosts.host(0).needs_drawing_restart());
    }
}
