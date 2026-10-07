use super::*;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-registration-{}-{}",
            std::process::id(),
            WRITE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let resolved = self.0.canonicalize().unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert!(resolved.starts_with(&root) && resolved != root);
        fs::remove_dir_all(resolved).unwrap();
    }
}

fn registration(directory: &Scratch, request_id: &str) -> Registration {
    Registration {
        version: 1,
        request_id: request_id.into(),
        enabled: true,
        component: directory.0.join("selected.component.wat"),
        font: None,
        grants: ModGrants {
            controls: true,
            ..Default::default()
        },
    }
}

fn write_registration(directory: &Scratch, registration: &Registration) {
    let mut json = serde_json::to_value(registration).unwrap();
    json["request_id"] = serde_json::Value::String(registration.request_id.clone());
    fs::write(
        directory.0.join(REGISTRATION_FILE),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();
}

#[test]
fn packet_delay_grant_is_opt_in_and_survives_registration_decode() {
    let old: ModGrants = serde_json::from_str(r#"{"controls":true}"#).unwrap();
    assert!(!old.packet_delay);
    let selected: ModGrants =
        serde_json::from_str(r#"{"controls":true,"packet_delay":true}"#).unwrap();
    assert!(selected.packet_delay);
}

fn fixture(enabled: bool, frame: &str, text: &str) -> String {
    let package = include_str!("../../../../crates/mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let panel = format!(
        r#"{{"title":"Local controls","toggle_key":"ShiftRight","dark":true,"controls":[{{"kind":"toggle","id":"enabled","label":"Enabled","value":{enabled}}}]}}"#
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

fn candidate(directory: &Scratch, enabled: bool, text: &str) -> Box<Candidate> {
    let registration = registration(directory, "first");
    fs::write(
        &registration.component,
        fixture(
            enabled,
            "call $pressed if i32.const 1 i32.const 4 i32.const 128 call $label end",
            text,
        ),
    )
    .unwrap();
    Box::new(build_candidate(source_snapshot(registration).unwrap()).unwrap())
}

fn rendered_output() -> mod_host::mod_render::RenderOutput {
    use mod_host::mod_render::{Billboard, BillboardPattern, Primitives, RenderOutput};
    RenderOutput {
        passes: Vec::new(),
        primitives: Arc::new(Primitives {
            billboards: vec![Billboard {
                position: [0.0; 3],
                width: 1.0,
                height: 1.0,
                color: [1.0; 4],
                pattern: BillboardPattern::Spark,
                upright: false,
            }],
            ..Default::default()
        }),
    }
}

fn world() -> (World, Receiver<Message>, Receiver<ModHost>) {
    let (_, updates) = bounded(1);
    let (messages, receive_messages) = bounded(8);
    let (retired, receive_retired) = bounded(2);
    let mut world = World::new();
    world.insert_resource(Watcher {
        updates,
        messages,
        retired,
        latest: Arc::new(AtomicU64::new(1)),
        thread: None,
        pending: Mutex::new(None),
        pending_ack: Mutex::new(None),
        desired: Arc::new(Mutex::new(Desired::default())),
        alive: Arc::new(AtomicBool::new(true)),
        last_ack: AtomicU64::new(0),
        fallback_retired: Mutex::new(Vec::with_capacity(2)),
        settings: Arc::new(Mutex::new(None)),
    });
    world.insert_resource(
        UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap(),
    );
    (world, receive_messages, receive_retired)
}

#[test]
fn registration_is_bounded_absolute_and_optional_grants_default_denied() {
    let directory = Scratch::new();
    let mut registration = registration(&directory, "first");
    registration.grants = ModGrants::default();
    write_registration(&directory, &registration);
    let observed = observe(&directory.0.join(REGISTRATION_FILE));
    let registration = observed.result.unwrap().unwrap();
    assert_eq!(observed.request_id.as_deref(), Some("first"));
    let grants = registration.grants;
    assert!(
        !grants.players
            && !grants.camera
            && !grants.controls
            && !grants.interaction
            && !grants.settings
            && !grants.environment
    );
    for value in [
        serde_json::json!({"version":2,"request_id":"first","enabled":true,"component":directory.0.join("module.wasm")}),
        serde_json::json!({"version":1,"request_id":"first","enabled":true,"component":"relative.wasm"}),
    ] {
        fs::write(
            directory.0.join(REGISTRATION_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(
            observe(&directory.0.join(REGISTRATION_FILE))
                .result
                .is_err()
        );
    }
    fs::write(
        directory.0.join(REGISTRATION_FILE),
        vec![b' '; MAX_BYTES + 1],
    )
    .unwrap();
    assert!(
        observe(&directory.0.join(REGISTRATION_FILE))
            .result
            .is_err()
    );
}

#[test]
fn source_identity_ignores_request_id_but_tracks_component_and_grants() {
    let directory = Scratch::new();
    let mut registration = registration(&directory, "first");
    fs::write(&registration.component, b"one").unwrap();
    let first = source_snapshot(registration.clone()).unwrap().identity;
    registration.request_id = "second".into();
    assert_eq!(
        source_snapshot(registration.clone()).unwrap().identity,
        first
    );
    registration.grants.interaction = true;
    assert_ne!(
        source_snapshot(registration.clone()).unwrap().identity,
        first
    );
    registration.grants.interaction = false;
    fs::write(&registration.component, b"two").unwrap();
    assert_ne!(source_snapshot(registration).unwrap().identity, first);
}

#[test]
fn candidate_identity_and_compilation_use_one_snapshot_after_file_replacement() {
    let directory = Scratch::new();
    let registration = registration(&directory, "first");
    fs::write(&registration.component, fixture(false, "", "Hello")).unwrap();
    let snapshot = source_snapshot(registration.clone()).unwrap();
    let identity = snapshot.identity;
    fs::write(&registration.component, fixture(true, "", "Other")).unwrap();
    let candidate = build_candidate(snapshot).unwrap();
    assert_eq!(candidate.identity, identity);
    assert_eq!(candidate.host.label(), Some("Hello"));
    assert_ne!(source_snapshot(registration).unwrap().identity, identity);
}

#[test]
fn actual_install_acknowledges_only_after_host_and_panel_are_published() {
    let directory = Scratch::new();
    let (mut world, messages, _) = world();
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "first".into(),
            result: Ok(Action::Replace(candidate(&directory, false, "Hello"))),
        },
    );
    assert!(world.resource::<ModRuntime>().host.is_active());
    assert_eq!(world.resource::<ModRuntime>().host.label(), Some("Hello"));
    assert!(world.resource::<ModRuntime>().host.panel().is_some());
    let Message::Acknowledge {
        generation,
        status,
        identity,
    } = messages.try_recv().unwrap()
    else {
        panic!("expected installation acknowledgment")
    };
    assert_eq!(generation, 1);
    assert_eq!(status.state, "loaded");
    assert_eq!(status.request_id, "first");
    assert_eq!(
        identity,
        world.resource::<ModRuntime>().registration_identity
    );
}

#[test]
fn id_only_reattachment_preserves_guest_state_panel_input_and_interaction() {
    let directory = Scratch::new();
    let (mut world, messages, _) = world();
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "first".into(),
            result: Ok(Action::Replace(candidate(&directory, true, "Hello"))),
        },
    );
    messages.try_recv().unwrap();
    world.resource_mut::<ModRuntime>().host.frame(true).unwrap();
    world.resource_mut::<ModRuntime>().host.set_panel_open(true);
    world
        .resource_mut::<UiPresentationRuntime>()
        .set_mod_panel_open(true);
    world.resource_mut::<ModInteraction>().attack_reach = Some(5.0);
    let identity = world
        .resource::<ModRuntime>()
        .registration_identity
        .unwrap();
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "second".into(),
            result: Ok(Action::Reuse {
                identity,
                registration: registration(&directory, "second"),
            }),
        },
    );
    let runtime = world.resource::<ModRuntime>();
    assert_eq!(runtime.host.label(), Some("ello"));
    assert!(runtime.host.panel_open());
    assert!(matches!(
        &runtime.host.panel().unwrap().controls[0],
        ui::mod_panel::Control::Toggle { value: true, .. }
    ));
    assert!(world.resource::<UiPresentationRuntime>().mod_panel_open());
    assert_eq!(world.resource::<ModInteraction>().attack_reach, Some(5.0));
    let Message::Acknowledge { status, .. } = messages.try_recv().unwrap() else {
        panic!("expected reuse acknowledgment")
    };
    assert_eq!(status.request_id, "second");
    assert_eq!(status.state, "loaded");
}

#[test]
fn new_component_resets_state_and_retires_old_host_outside_world() {
    let directory = Scratch::new();
    let (mut world, messages, retired) = world();
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "first".into(),
            result: Ok(Action::Replace(candidate(&directory, true, "Hello"))),
        },
    );
    messages.try_recv().unwrap();
    world.resource_mut::<ModRuntime>().host.set_panel_open(true);
    world.insert_resource(super::super::ModCueFeed(vec![mod_host::ModCue {
        name: "stale".into(),
        values: Vec::new(),
    }]));
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "second".into(),
            result: Ok(Action::Replace(candidate(&directory, false, "Other"))),
        },
    );
    assert!(world.resource::<super::super::ModCueFeed>().0.is_empty());
    assert_eq!(world.resource::<ModRuntime>().host.label(), Some("Other"));
    assert!(!world.resource::<ModRuntime>().host.panel_open());
    assert!(matches!(
        &world
            .resource::<ModRuntime>()
            .host
            .panel()
            .unwrap()
            .controls[0],
        ui::mod_panel::Control::Toggle { value: false, .. }
    ));
    assert_eq!(retired.try_recv().unwrap().label(), Some("Hello"));
}

#[test]
fn disable_and_invalid_registration_revoke_all_owned_outputs() {
    for result in [Ok(Action::Disabled), Err("invalid registration".into())] {
        let directory = Scratch::new();
        let (mut world, messages, retired) = world();
        install(
            &mut world,
            Update {
                generation: 1,
                request_id: "first".into(),
                result: Ok(Action::Replace(candidate(&directory, true, "Hello"))),
            },
        );
        messages.try_recv().unwrap();
        world.resource_mut::<ModInteraction>().attack_reach = Some(6.0);
        world.resource_mut::<VisualTimeOverride>().0 = Some(18_000);
        world
            .resource_mut::<UiPresentationRuntime>()
            .set_mod_panel_open(true);
        let mut scene = render::ModRenderScene::default();
        scene.apply(&rendered_output(), 7);
        world.insert_resource(scene);
        world.insert_resource(super::super::ModCueFeed(vec![mod_host::ModCue {
            name: "stale".into(),
            values: Vec::new(),
        }]));
        install(
            &mut world,
            Update {
                generation: 1,
                request_id: "second".into(),
                result,
            },
        );
        assert_eq!(world.resource::<render::ModRenderScene>().vertex_count(), 0);
        assert!(world.resource::<super::super::ModCueFeed>().0.is_empty());
        assert!(!world.contains_resource::<ModRuntime>());
        assert!(!world.contains_resource::<ModInteraction>());
        assert!(!world.contains_resource::<VisualTimeOverride>());
        assert!(!world.resource::<UiPresentationRuntime>().mod_panel_open());
        assert_eq!(retired.try_recv().unwrap().label(), Some("Hello"));
    }
}

#[test]
fn revoked_generation_cannot_install_a_completed_candidate() {
    let directory = Scratch::new();
    let (mut world, messages, retired) = world();
    world
        .resource::<Watcher>()
        .latest
        .store(2, Ordering::Release);
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "stale".into(),
            result: Ok(Action::Replace(candidate(&directory, true, "Hello"))),
        },
    );
    assert!(!world.contains_resource::<ModRuntime>());
    assert!(messages.try_recv().is_err());
    assert_eq!(retired.try_recv().unwrap().label(), Some("Hello"));
}

#[test]
fn inactive_guest_is_reinitialized_instead_of_falsely_acknowledged_as_loaded() {
    let directory = Scratch::new();
    let (mut world, messages, _) = world();
    let registration = registration(&directory, "first");
    fs::write(
        &registration.component,
        fixture(false, "unreachable", "Hello"),
    )
    .unwrap();
    let candidate = build_candidate(source_snapshot(registration.clone()).unwrap()).unwrap();
    let identity = candidate.identity;
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "first".into(),
            result: Ok(Action::Replace(Box::new(candidate))),
        },
    );
    messages.try_recv().unwrap();
    assert!(
        world
            .resource_mut::<ModRuntime>()
            .host
            .frame(false)
            .is_err()
    );
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "retry".into(),
            result: Ok(Action::Reuse {
                identity,
                registration,
            }),
        },
    );
    assert!(matches!(
        messages.try_recv().unwrap(),
        Message::Reload { .. }
    ));
}

#[test]
fn saturated_retirement_queue_releases_authority_before_waiting_for_disposal() {
    let directory = Scratch::new();
    let (mut world, messages, retired) = world();
    install(
        &mut world,
        Update {
            generation: 1,
            request_id: "first".into(),
            result: Ok(Action::Replace(candidate(&directory, true, "Hello"))),
        },
    );
    messages.try_recv().unwrap();
    world.resource_mut::<ModRuntime>().host.set_panel_open(true);
    world
        .resource_mut::<UiPresentationRuntime>()
        .set_mod_panel_open(true);
    world.resource_mut::<ModInteraction>().attack_reach = Some(6.0);
    world.resource_mut::<VisualTimeOverride>().0 = Some(18_000);
    for _ in 0..2 {
        world
            .resource::<Watcher>()
            .retire(candidate(&directory, false, "Queue").host);
    }
    assert_eq!(retired.len(), 2);
    let watcher = world.resource::<Watcher>();
    watcher.latest.store(2, Ordering::Release);
    *watcher.desired.lock().unwrap() = Desired {
        generation: 2,
        identity: None,
        request_id: "disabled".into(),
        status: Some(("disabled", None)),
    };
    sync_authority(&mut world);
    install(
        &mut world,
        Update {
            generation: 2,
            request_id: "disabled".into(),
            result: Ok(Action::Disabled),
        },
    );
    let runtime = world.resource::<ModRuntime>();
    assert!(runtime.suspended);
    assert!(!runtime.host.panel_open());
    assert!(!runtime.controls.focused);
    assert!(!world.resource::<UiPresentationRuntime>().mod_panel_open());
    assert_eq!(world.resource::<ModInteraction>().attack_reach, None);
    assert_eq!(world.resource::<VisualTimeOverride>().0, None);
    let Message::Acknowledge { status, .. } = messages.try_recv().unwrap() else {
        panic!("expected revocation acknowledgment")
    };
    assert_eq!(status.state, "disabled");
    assert!(
        world
            .resource::<Watcher>()
            .pending
            .lock()
            .unwrap()
            .is_some()
    );
}

#[test]
fn acknowledgment_status_is_bounded_and_replaces_only_fixed_companion() {
    let directory = Scratch::new();
    let component = directory.0.join("module.wasm");
    fs::write(&component, b"unchanged").unwrap();
    let status = Status::new(
        "first".into(),
        "error",
        Some(format!("{}\n", "é".repeat(MAX_MESSAGE_BYTES))),
    );
    assert!(status.message.as_ref().unwrap().len() <= MAX_MESSAGE_BYTES);
    assert!(!status.message.as_ref().unwrap().contains('\n'));
    let path = directory.0.join(STATUS_FILE);
    write_status(&path, &status).unwrap();
    write_status(&path, &Status::new("second".into(), "disabled", None)).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(json["request_id"], "second");
    assert_eq!(json["state"], "disabled");
    assert_eq!(fs::read(component).unwrap(), b"unchanged");
}

#[test]
fn reattachment_waits_for_queued_retirement_before_reading_companion_settings() {
    use std::sync::mpsc;

    #[derive(Debug)]
    struct PendingFlush {
        path: PathBuf,
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Drop for PendingFlush {
        fn drop(&mut self) {
            self.started.send(()).unwrap();
            self.release.recv().unwrap();
            fs::write(&self.path, "{\"cps\":30}").unwrap();
        }
    }

    let directory = Scratch::new();
    let mut registration = registration(&directory, "reattach");
    registration.grants.settings = true;
    fs::write(&registration.component, fixture(false, "", "Hello")).unwrap();
    let companion = registration.component.with_extension("settings.json");
    fs::write(&companion, "{\"cps\":12}").unwrap();
    let snapshot = source_snapshot(registration).unwrap();
    let (retired, retirement) = bounded(1);
    let (started, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (building, built) = mpsc::channel();
    retired
        .send(PendingFlush {
            path: companion.clone(),
            started,
            release: released,
        })
        .unwrap();
    let loader = thread::spawn(move || {
        after_retirement(&retirement, || {
            building.send(()).unwrap();
            build_candidate(snapshot).unwrap()
        })
    });
    waiting
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(
        built.try_recv().is_err(),
        "candidate read settings before retirement finished"
    );
    release.send(()).unwrap();
    let candidate = loader.join().unwrap();
    assert_eq!(candidate.host.settings_seed(), Some("{\"cps\":30}"));
    drop(candidate);
    assert_eq!(fs::read_to_string(companion).unwrap(), "{\"cps\":30}");
}

#[test]
fn block_highlights_grant_is_opt_in_and_survives_registration_decode() {
    let old: ModGrants = serde_json::from_str(r#"{"controls":true}"#).unwrap();
    assert!(!old.block_highlights);
    let selected: ModGrants = serde_json::from_str(r#"{"block_highlights":true}"#).unwrap();
    assert!(selected.block_highlights);
}

#[test]
fn fullbright_grant_is_opt_in_and_survives_registration_decode() {
    let old: ModGrants = serde_json::from_str(r#"{"environment":true}"#).unwrap();
    assert!(!old.fullbright);
    let selected: ModGrants = serde_json::from_str(r#"{"fullbright":true}"#).unwrap();
    assert!(selected.fullbright);
}
