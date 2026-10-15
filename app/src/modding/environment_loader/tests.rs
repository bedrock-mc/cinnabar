use super::super::registration::HostMetadata;
use super::*;
use client_extensions::modding::multi::ModHosts;

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

/// A guest keeps its panel in the host; the bundle and font never enter linear memory.
fn component(title: &str, font: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = wat::parse_str(
        r#"(component
        (core module $code (func (export "init")) (func (export "frame")))
        (core instance $run (instantiate $code))
        (func (export "init") (canon lift (core func $run "init")))
        (func (export "frame") (canon lift (core func $run "frame"))))"#,
    )
    .unwrap();
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
            component: path.to_owned(),
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
    world.insert_resource(ModRuntime {
        hosts: ModHosts::new(prepared.host, Vec::new()),
        metadata: vec![metadata],
        render_sources: Vec::new(),
        render_merge: Default::default(),
        last_reload: Instant::now(),
        controls: mod_host::empty_controls(),
        suspended: false,
        hud_editor_owner: None,
        screens: Default::default(),
    });
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
    current.settings = Some((settings_path.clone(), "{\"value\":0}".into()));
    let update = prepare(current, &mut attempted).unwrap();
    let rejected = apply(&mut world, update).unwrap();
    assert_eq!(rejected.panel().unwrap().title, "Second");
    let old = world.resource::<ModRuntime>().hosts.host(0);
    assert_eq!(old.panel().unwrap().title, "First");
    assert_eq!(old.settings_snapshot(), Some("{\"value\":1}"));
    let mut retry = request(&path, digest);
    retry.source.grants.settings = true;
    retry.settings = Some((settings_path, old.settings_snapshot().unwrap().into()));
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
