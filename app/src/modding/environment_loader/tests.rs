use super::super::registration::HostMetadata;
use super::*;

/// Writes a complete package with a verified component and no external resources.
fn package(directory: &std::path::Path) {
    let bytes = executable();
    let digest = Sha256::digest(&bytes);
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::fs::write(
        directory.join(experience_sdk::mod_manifest::COMPONENT),
        bytes,
    )
    .unwrap();
    std::fs::write(directory.join(mod_host::package::MANIFEST), format!(
        "id = \"worker_test\"\nversion = \"1.0.0\"\napi = \"{}\"\npermissions = []\n[files]\n\"{}\" = \"{hash}\"\n",
        experience_sdk::mod_manifest::API, experience_sdk::mod_manifest::COMPONENT
    )).unwrap();
}

#[test]
fn environment_packages_use_worker_admission() {
    let directory = tempfile::tempdir().unwrap();
    package(directory.path());
    let host = ModHost::load_package(directory.path(), ModGrants::default()).unwrap();
    let mut app = App::new();
    super::super::install(&mut app, vec![host]);
    assert!(app.world().contains_resource::<Loader>());
    app.insert_resource(UiPresentationResource(
        client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
            ui_engine::test_support::fixture_font(),
        )
        .unwrap(),
    ));
    let digest = app
        .world()
        .resource::<ModRuntime>()
        .hosts
        .host(0)
        .source_digest();
    for (connection, dimension) in [(0, 0), (1, 0), (1, 1), (1, 0), (0, 0)] {
        let session = mod_api::drawing::DrawingSession {
            connection,
            dimension,
            ..Default::default()
        };
        app.world_mut()
            .resource_mut::<ModRuntime>()
            .hosts
            .host_mut(0)
            .bind_drawing_session(session);
        test_time::eventually("package fence is admitted off-frame", || {
            app.world_mut().resource_mut::<Loader>().last_poll = Instant::now() - RELOAD_INTERVAL;
            poll(app.world_mut());
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .is_active()
        });
        let host = app.world().resource::<ModRuntime>().hosts.host(0);
        assert_eq!(host.drawing_session(), Some(session));
        assert_eq!(host.source_digest(), digest);
        assert!(host.package().is_some());
        assert!(!host.needs_drawing_restart());
    }
}

/// Adds an ordinary top-level static source fixture without any product resources.
fn section(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = vec![name.len() as u8];
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    bytes.push(0);
    let mut size = payload.len();
    loop {
        let byte = (size & 0x7f) as u8;
        size >>= 7;
        bytes.push(byte | if size == 0 { 0 } else { 0x80 });
        if size == 0 {
            break;
        }
    }
    bytes.extend(payload);
}

/// Builds a guest with no embedded presentation sections, also valid in directory packages.
fn executable() -> Vec<u8> {
    wat::parse_str(
        r#"(component
        (core module $code (func (export "init")) (func (export "frame")))
        (core instance $run (instantiate $code))
        (func (export "init") (canon lift (core func $run "init")))
        (func (export "frame") (canon lift (core func $run "frame"))))"#,
    )
    .unwrap()
}

/// A guest keeps its panel in the host; the bundle and font never enter linear memory.
fn component(title: &str, font: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = executable();
    let bundle = serde_json::json!({"panel":{"title":title,"toggle_key":"ShiftRight","dark":true,"controls":[]},"surfaces":{}}).to_string();
    section(&mut bytes, mod_api::PANEL_BUNDLE_SECTION, bundle.as_bytes());
    section(&mut bytes, mod_api::MENU_TITLE_SECTION, title.as_bytes());
    if let Some(font) = font {
        section(&mut bytes, mod_api::PANEL_FONT_SECTION, font);
    }
    bytes
}

/// Supplies immutable owner identity and the accepted predecessor's source fingerprint.
fn request(path: &std::path::Path, previous: [u8; 32]) -> Request {
    Request {
        source: Source {
            owner: mod_ui::PanelUiOwner::Environment(0),
            location: mod_host::Source::Component(path.to_owned()),
            grants: ModGrants {
                controls: true,
                environment: true,
                ..Default::default()
            },
            font: None,
            ui: None,
        },
        previous,
        settings: None,
        drawing: None,
    }
}

/// Publishes a real accepted guest and resources in a minimal ECS world without a worker clock.
fn world(prepared: admission::PreparedMod) -> World {
    let mut metadata = HostMetadata::environment(0);
    metadata.font = prepared.font;
    metadata.branding = prepared.branding;
    let mut world = World::new();
    world.insert_resource(UiPresentationResource(
        client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
            ui_engine::test_support::fixture_font(),
        )
        .unwrap(),
    ));
    world.insert_resource(ModRuntime::new(prepared.host, metadata));
    world
}

#[test]
fn changed_source_atomically_replaces_host_and_prepared_resources_without_duplicate_authority() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let first = component("First", None);
    std::fs::write(&path, &first).unwrap();
    let previous = admission::prepare(
        &path,
        &first,
        request(&path, [0; 32]).source.grants,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(previous.panel_ui.is_some());
    assert!(previous.branding.is_some());
    let digest = previous.host.source_digest();
    let mut world = world(previous);
    std::fs::write(&path, component("Second", None)).unwrap();
    let mut attempted = BTreeMap::new();
    let update = prepare(request(&path, digest), &mut attempted).unwrap();
    assert!(update.result.as_ref().unwrap().panel_ui.is_some());
    let retired = apply(&mut world, update).unwrap();
    let runtime = world.resource::<ModRuntime>();
    assert_eq!(runtime.hosts.host_count(), 1);
    assert_eq!(runtime.hosts.host(0).panel().unwrap().title, "Second");
    assert_eq!(runtime.hosts.host(0).menu_title(), Some("Second"));
    assert!(runtime.metadata[0].branding.is_some());
    assert_eq!(retired.panel().unwrap().title, "First");
    assert!(
        prepare(
            request(&path, runtime.hosts.host(0).source_digest()),
            &mut attempted
        )
        .is_none()
    );
}

#[test]
fn malformed_candidate_keeps_the_previous_guest_and_suppresses_identical_failed_retries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let first = component("First", None);
    std::fs::write(&path, &first).unwrap();
    let previous = admission::prepare(
        &path,
        &first,
        request(&path, [0; 32]).source.grants,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let digest = previous.host.source_digest();
    let mut world = world(previous);
    std::fs::write(&path, component("Invalid", Some(b"invalid outline font"))).unwrap();
    let mut attempted = BTreeMap::new();
    let update = prepare(request(&path, digest), &mut attempted).unwrap();
    assert!(update.result.is_err());
    assert!(apply(&mut world, update).is_none());
    assert_eq!(
        world
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .panel()
            .unwrap()
            .title,
        "First"
    );
    assert!(prepare(request(&path, digest), &mut attempted).is_none());
}

#[test]
fn repairing_an_explicit_ui_override_retries_the_same_changed_component() {
    for malformed in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("local.wasm");
        let ui = directory.path().join("panel.json");
        let first = component("First", None);
        let previous = admission::prepare(
            &path,
            &first,
            request(&path, [0; 32]).source.grants,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let digest = previous.host.source_digest();
        let mut world = world(previous);
        std::fs::write(&path, component("Second", None)).unwrap();
        if malformed {
            std::fs::write(&ui, b"malformed panel").unwrap();
        }
        let candidate = |previous| {
            let mut next = request(&path, previous);
            next.source.ui = Some(ui.clone());
            next
        };
        let mut attempted = BTreeMap::new();
        let rejected = prepare(candidate(digest), &mut attempted).unwrap();
        assert!(rejected.result.is_err());
        assert!(apply(&mut world, rejected).is_none());
        assert_eq!(
            world
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .panel()
                .unwrap()
                .title,
            "First"
        );
        assert!(prepare(candidate(digest), &mut attempted).is_none());

        let repaired = serde_json::json!({
            "panel":{"title":"Repaired","toggle_key":"ShiftRight","dark":true,"controls":[]},
            "surfaces":{}
        });
        std::fs::write(&ui, repaired.to_string()).unwrap();
        let update = prepare(candidate(digest), &mut attempted).expect("repaired override retries");
        assert!(update.result.as_ref().unwrap().panel_ui.is_some());
        drop(apply(&mut world, update).unwrap());
        let current = world.resource::<ModRuntime>().hosts.host(0);
        assert_eq!(current.panel().unwrap().title, "Repaired");
        let accepted = current.source_digest();
        std::fs::remove_file(&ui).unwrap();
        assert!(
            prepare(candidate(accepted), &mut attempted).is_none(),
            "accepted components retain their no-change fast path"
        );
    }
}

#[test]
fn changed_committed_settings_retry_a_rejected_component() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let mut granted = request(&path, [0; 32]).source.grants;
    granted.settings = true;
    let previous = admission::prepare(
        &path,
        &component("First", None),
        granted.clone(),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let digest = previous.host.source_digest();
    let destination = previous.host.settings_path().unwrap().to_owned();
    let committed = previous.host.settings_snapshot().unwrap().to_owned();
    let mut world = world(previous);
    std::fs::write(&path, component("Second", None)).unwrap();
    let candidate = |settings: String| {
        let mut next = request(&path, digest);
        next.source.grants = granted.clone();
        next.settings = Some(SettingsSnapshot {
            path: destination.clone(),
            json: settings,
        });
        next
    };
    let invalid = "x".repeat(mod_api::MAX_SETTINGS_BYTES + 1);
    let mut attempted = BTreeMap::new();
    let rejected = prepare(candidate(invalid.clone()), &mut attempted).unwrap();
    assert!(rejected.result.is_err());
    assert!(apply(&mut world, rejected).is_none());
    assert!(prepare(candidate(invalid), &mut attempted).is_none());
    let update = prepare(candidate(committed), &mut attempted).expect("new settings seed retries");
    drop(apply(&mut world, update).unwrap());
    assert_eq!(
        world
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .panel()
            .unwrap()
            .title,
        "Second"
    );
}

#[test]
fn repairing_settings_without_a_committed_seed_retries_the_changed_component() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let companion = mod_host::settings_companion(&path);
    let mut granted = request(&path, [0; 32]).source.grants;
    granted.settings = true;
    let previous = admission::prepare(
        &path,
        &component("First", None),
        granted.clone(),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let digest = previous.host.source_digest();
    let mut world = world(previous);
    std::fs::write(&path, component("Second", None)).unwrap();
    std::fs::write(&companion, [0xff]).unwrap();
    let candidate = || {
        let mut next = request(&path, digest);
        next.source.grants = granted.clone();
        next
    };
    let mut attempted = BTreeMap::new();
    let rejected = prepare(candidate(), &mut attempted).unwrap();
    assert!(rejected.result.is_err());
    assert!(apply(&mut world, rejected).is_none());
    assert!(prepare(candidate(), &mut attempted).is_none());
    std::fs::write(&companion, b"").unwrap();
    let update = prepare(candidate(), &mut attempted).expect("repaired settings source retries");
    drop(apply(&mut world, update).unwrap());
    assert_eq!(
        world
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .panel()
            .unwrap()
            .title,
        "Second"
    );
}

#[test]
fn two_owners_of_one_source_prepare_independently_including_failure_suppression() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shared.wasm");
    std::fs::write(&path, component("Changed", None)).unwrap();
    let mut attempted = BTreeMap::new();
    let first = request(&path, [0; 32]);
    let mut second = request(&path, [0; 32]);
    second.source.owner = mod_ui::PanelUiOwner::Environment(1);
    second.source.grants.controls = false;
    assert!(prepare(first, &mut attempted).unwrap().result.is_ok());
    let second = prepare(second, &mut attempted).unwrap().result.unwrap();
    assert!(second.panel_ui.is_none());
    assert_eq!(second.host.menu_title(), Some("Changed"));
    std::fs::write(&path, component("Invalid", Some(b"invalid font"))).unwrap();
    assert!(
        prepare(request(&path, [0; 32]), &mut attempted)
            .unwrap()
            .result
            .is_err()
    );
    let mut denied = request(&path, [0; 32]);
    denied.source.owner = mod_ui::PanelUiOwner::Environment(1);
    denied.source.grants.controls = false;
    assert!(prepare(denied, &mut attempted).unwrap().result.is_ok());
    assert!(prepare(request(&path, [0; 32]), &mut attempted).is_none());
}

#[test]
fn stale_settings_candidate_is_retired_and_same_source_retries_with_current_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let companion = mod_host::settings_companion(&path);
    let first = component("First", None);
    std::fs::write(&path, &first).unwrap();
    std::fs::write(&companion, "{\"value\":1}").unwrap();
    let mut current = request(&path, [0; 32]);
    current.source.grants.settings = true;
    let previous = admission::prepare(
        &path,
        &first,
        current.source.grants.clone(),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let companion = previous.host.settings_path().unwrap().to_owned();
    let digest = previous.host.source_digest();
    let mut world = world(previous);
    // Requests carry the accepted host's resolved writer destination, as the loader sends them.
    let settings_path = world
        .resource::<ModRuntime>()
        .hosts
        .host(0)
        .settings_path()
        .unwrap()
        .to_owned();
    std::fs::write(&path, component("Second", None)).unwrap();
    let mut attempted = BTreeMap::new();
    current.previous = digest;
    // The worker received these preferences before a newer value was committed.
    current.settings = Some(SettingsSnapshot {
        path: settings_path.clone(),
        json: "{\"value\":0}".into(),
    });
    let update = prepare(current, &mut attempted).unwrap();
    let rejected = apply(&mut world, update).unwrap();
    assert_eq!(rejected.panel().unwrap().title, "Second");
    let old = world.resource::<ModRuntime>().hosts.host(0);
    assert_eq!(old.panel().unwrap().title, "First");
    assert_eq!(old.settings_snapshot(), Some("{\"value\":1}"));
    let mut retry = request(&path, digest);
    retry.source.grants.settings = true;
    retry.settings = Some(SettingsSnapshot {
        path: settings_path,
        json: old.settings_snapshot().unwrap().into(),
    });
    let update = prepare(retry, &mut attempted).expect("valid source retries after stale settings");
    drop(apply(&mut world, update).unwrap());
    drop(rejected);
    let current = world.resource::<ModRuntime>().hosts.host(0);
    assert_eq!(current.panel().unwrap().title, "Second");
    assert_eq!(current.settings_snapshot(), Some("{\"value\":1}"));
    drop(world);
    assert_eq!(std::fs::read_to_string(companion).unwrap(), "{\"value\":1}");
}

#[test]
fn supplied_embedded_font_and_ui_reload_as_one_accepted_snapshot() {
    let Some(path) = std::env::var_os("CINNABAR_TEST_PANEL_FONT") else {
        eprintln!(
            "skipping supplied_embedded_font_and_ui_reload_as_one_accepted_snapshot: fixture unavailable; set CINNABAR_TEST_PANEL_FONT to a local outline font"
        );
        return;
    };
    let bytes =
        mod_host::local_files::read(std::path::Path::new(&path), font::MAX_SOURCE_BYTES).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let first = component("First", Some(&bytes));
    let previous = admission::prepare(
        &path,
        &first,
        request(&path, [0; 32]).source.grants,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let font = previous.font.as_ref().unwrap();
    assert!(font.linear_sampling());
    let digest = previous.host.source_digest();
    let mut world = world(previous);
    std::fs::write(&path, component("Second", Some(&bytes))).unwrap();
    let update = prepare(request(&path, digest), &mut BTreeMap::new()).unwrap();
    let accepted = update.result.as_ref().unwrap();
    assert!(accepted.font.as_ref().unwrap().glyph('A').is_some());
    assert!(accepted.panel_ui.is_some());
    assert!(apply(&mut world, update).is_some());
    assert_eq!(
        world
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .panel()
            .unwrap()
            .title,
        "Second"
    );
    assert!(world.resource::<ModRuntime>().metadata[0].font.is_some());
}

#[test]
fn drawing_restart_prepares_changed_guest_and_presentation_as_one_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let first = component("First", None);
    std::fs::write(&path, &first).unwrap();
    let previous = admission::prepare(
        &path,
        &first,
        request(&path, [0; 32]).source.grants,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let mut world = world(previous);
    let session = mod_api::drawing::DrawingSession {
        connection: 1,
        local: true,
        permissions: mod_api::drawing::DrawingPermissions::ALL,
        ..Default::default()
    };
    world
        .resource_mut::<ModRuntime>()
        .hosts
        .host_mut(0)
        .bind_drawing_session(session);
    let old = world.resource::<ModRuntime>().hosts.host(0);
    let mut restart = request(&path, old.source_digest());
    restart.drawing = admission::DrawingContext::capture(old, None);
    std::fs::write(&path, component("Second", None)).unwrap();
    let update = prepare(restart, &mut BTreeMap::new()).unwrap();
    let prepared = update.result.as_ref().unwrap();
    assert!(prepared.panel_ui.is_some() && prepared.branding.is_some());
    assert!(prepared.host.is_active());
    assert_eq!(prepared.host.drawing_session(), Some(session));
    apply(&mut world, update).unwrap();
    let runtime = world.resource::<ModRuntime>();
    assert_eq!(runtime.hosts.host(0).panel().unwrap().title, "Second");
    assert_eq!(runtime.hosts.host(0).menu_title(), Some("Second"));
    assert!(runtime.metadata[0].branding.is_some());
    assert!(!runtime.hosts.host(0).needs_drawing_restart());
}

#[test]
fn unchanged_drawing_restart_deduplicates_failures_and_rejects_stale_fences() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let bytes = component("First", None);
    std::fs::write(&path, &bytes).unwrap();
    let previous = admission::prepare(
        &path,
        &bytes,
        request(&path, [0; 32]).source.grants,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let mut world = world(previous);
    let mut attempted = BTreeMap::new();
    for connection in [1, 2] {
        let mut runtime = world.resource_mut::<ModRuntime>();
        let host = runtime.hosts.host_mut(0);
        host.bind_drawing_session(mod_api::drawing::DrawingSession {
            connection,
            ..Default::default()
        });
        let mut restart = request(&path, host.source_digest());
        restart.drawing = admission::DrawingContext::capture(host, None);
        restart.source.font = Some(directory.path().join("missing-font"));
        assert!(prepare(restart, &mut attempted).unwrap().result.is_err());
        let mut retry = request(&path, host.source_digest());
        retry.drawing = admission::DrawingContext::capture(host, None);
        retry.source.font = Some(directory.path().join("missing-font"));
        assert!(prepare(retry, &mut attempted).is_none());
    }
    let old = world.resource::<ModRuntime>().hosts.host(0);
    let mut restart = request(&path, old.source_digest());
    restart.drawing = admission::DrawingContext::capture(old, None);
    let update = prepare(restart, &mut attempted).unwrap();
    assert!(update.result.is_ok());
    {
        let mut runtime = world.resource_mut::<ModRuntime>();
        runtime
            .hosts
            .host_mut(0)
            .bind_drawing_session(mod_api::drawing::DrawingSession {
                connection: 3,
                ..Default::default()
            });
    }
    let retired = apply(&mut world, update).unwrap();
    assert!(retired.is_active());
    assert!(world.resource::<ModRuntime>().hosts.host(0).is_revoked());
}

#[test]
fn worker_sources_retain_loader_consent_before_a_component_requests_drawing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.wasm");
    let first = component("First", None);
    let previous = admission::prepare(
        &path,
        &first,
        ModGrants {
            world_draw: true,
            ..Default::default()
        },
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(!previous.host.grants().world_draw);
    let source = Source::from_host(mod_ui::PanelUiOwner::Environment(0), &previous.host);
    let mut changed = component("Second", None);
    section(
        &mut changed,
        mod_host::REQUESTED_GRANTS_SECTION,
        b"{\"world_draw\":true}",
    );
    std::fs::write(&path, changed).unwrap();
    let update = prepare(
        Request {
            source,
            previous: previous.host.source_digest(),
            settings: None,
            drawing: None,
        },
        &mut BTreeMap::new(),
    )
    .unwrap();
    assert!(update.result.unwrap().host.grants().world_draw);
}

/// Builds a guest that traps during init unless its post pass is authorized.
fn post_component() -> Vec<u8> {
    let package = include_str!("../../../../crates/mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let shader = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }";
    let data = format!("grade{shader}")
        .bytes()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let init = format!(
        "i32.const 1024 i32.const 5 i32.const 0 i32.const 1029 i32.const {} i32.const 0 i32.const 512 call $register i32.const 512 i32.load8_u if unreachable end",
        shader.len()
    );
    let source = include_str!("../../../../crates/mod-host/src/tests/render.wat")
        .replace("$RENDER", &format!("{name}/render@{version}"))
        .replace("$DATA", &data)
        .replace("$INIT", &init)
        .replace("$FRAME", "");
    let mut bytes = wat::parse_str(source).unwrap();
    section(
        &mut bytes,
        mod_host::REQUESTED_GRANTS_SECTION,
        b"{\"render_post\":true}",
    );
    bytes
}

/// Supplies trusted scope as if the negotiation controller had authenticated it.
fn post_grant(digests: &[[u8; 32]]) -> server_experience::negotiation::Grant {
    use server_experience::{
        manifest::Offer,
        negotiation::{Grant, VerifiedOffer},
    };
    let digests = digests
        .iter()
        .map(|digest| {
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let offer: Offer = serde_json::from_value(serde_json::json!({
        "version":server_experience::policy::WIRE_VERSION, "audience":"test.invalid",
        "server_key":"", "revision":1, "expires_unix":u64::MAX,
        "scope":{"permissions":["render_post"], "origins":[], "memory_bytes":0, "gpu_bytes":0, "personal_mods":digests},
        "packages":[], "fallback":"", "carrier":protocol::EXPERIENCE_CHANNEL,
    })).unwrap();
    Grant {
        offer: VerifiedOffer {
            offer,
            digest: String::new(),
        },
        session: "test".into(),
        connection: "test".into(),
        subclient: 0,
        expires_unix: u64::MAX,
        wire: server_experience::negotiation::Wire::v1(),
    }
}

/// Collects the production frame poll's request while leaving compilation to the test worker.
fn queued_request(world: &mut World, source: Source) -> Request {
    let (requests, incoming) = bounded(1);
    let (_results, results) = bounded(1);
    let (retired, _retiring) = bounded(1);
    world.insert_resource(Loader {
        sources: vec![source],
        requests: Some(requests),
        results,
        retired,
        pending_retired: Vec::new(),
        thread: None,
        pending: false,
        last_poll: Instant::now() - RELOAD_INTERVAL,
    });
    poll(world);
    incoming.try_recv().unwrap().pop().unwrap()
}

#[test]
fn replacement_init_checks_its_own_digest_against_the_authenticated_grant() {
    for scope in [
        "authorized",
        "unlisted",
        "expired",
        "revoked_before_publish",
        "expired_before_publish",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("local.wasm");
        let first = executable();
        let next = post_component();
        let old_digest: [u8; 32] = Sha256::digest(&first).into();
        let new_digest: [u8; 32] = Sha256::digest(&next).into();
        let digests = [old_digest, new_digest];
        let mut grant = post_grant(if scope == "unlisted" {
            &digests[..1]
        } else {
            &digests
        });
        if scope == "expired" {
            grant.expires_unix = 0;
        }
        let mut ui = client_ui::ui_runtime::UiRuntime::new(1);
        ui.experiences.session.state = server_experience::session::State::Granted(grant);
        let mut source = request(&path, old_digest).source;
        source.grants.render_post = true;
        let previous =
            admission::prepare(&path, &first, source.grants.clone(), None, None, None, None)
                .unwrap();
        let mut world = world(previous);
        world.insert_resource(UiRuntimeResource(ui));
        world
            .resource_mut::<ModRuntime>()
            .hosts
            .host_mut(0)
            .bind_drawing_session(mod_api::drawing::DrawingSession {
                connection: 1,
                permissions: mod_api::drawing::DrawingPermissions {
                    render_post: true,
                    ..Default::default()
                },
                ..Default::default()
            });
        std::fs::write(&path, next).unwrap();
        let update = prepare(queued_request(&mut world, source), &mut BTreeMap::new()).unwrap();
        if matches!(scope, "unlisted" | "expired") {
            assert!(
                update.result.is_err(),
                "{scope} cannot authorize post-pass init"
            );
            continue;
        }
        let prepared = update
            .result
            .as_ref()
            .expect("authorized replacement initializes");
        assert!(prepared.host.grants().render_post);
        assert_eq!(prepared.host.render().0.passes.len(), 1);
        if scope == "revoked_before_publish" {
            world
                .resource_mut::<UiRuntimeResource>()
                .experiences
                .session
                .state = server_experience::session::State::Disabled;
        } else if scope == "expired_before_publish"
            && let server_experience::session::State::Granted(grant) = &mut world
                .resource_mut::<UiRuntimeResource>()
                .experiences
                .session
                .state
        {
            grant.expires_unix = 0;
        }
        let retired = apply(&mut world, update).unwrap();
        let current = world.resource::<ModRuntime>().hosts.host(0);
        if matches!(scope, "revoked_before_publish" | "expired_before_publish") {
            assert_eq!(current.source_digest(), old_digest);
            assert!(current.is_revoked());
            assert_eq!(retired.source_digest(), new_digest);
        } else {
            assert_eq!(current.source_digest(), new_digest);
            assert!(current.is_active());
        }
    }
}
