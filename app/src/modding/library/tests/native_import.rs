//! Native packet menus use the same explicit consent and next-join path as imported mods.

use super::*;
use crate::modding::{packets::PacketMods, registration::Watcher};
use protocol::PacketRelay;

/// Encodes an ordinary packet reactor whose command opens a menu after native negotiation.
fn native_packet_source() -> Vec<u8> {
    let output = |open| {
        serde_json::json!({
            "open": open,
            "panel": {
                "title": "Imported controls",
                "toggle_key": "ShiftRight",
                "dark": true,
                "controls": []
            }
        })
        .to_string()
    };
    let closed = output(false);
    let opened = output(true);
    let encode = |text: &str| {
        text.bytes()
            .map(|byte| format!("\\{byte:02x}"))
            .collect::<String>()
    };
    let source = include_str!("../../../../../crates/mod-host/src/packet_mod/tests/guest.wat")
        .replace("$IMPORTS", "")
        .replace("$START", "")
        .replace(
            "(local.set $op (i32.load8_u (local.get $batch)))",
            "(local.set $op (i32.load8_u (local.get $batch)))\n\
             (if (i32.eq (local.get $op) (i32.const 8))\n\
               (then (global.set $menu_open (i32.const 1))))",
        );
    let source = source.trim_end().strip_suffix(')').unwrap();
    wat::parse_str(format!(
        r#"{source}
        (global $native_flags (mut i32) (i32.const 0))
        (global $menu_open (mut i32) (i32.const 0))
        (data (i32.const 2048) "{}")
        (data (i32.const 4096) "{}")
        (func (export "native-init") (param i32)
            (global.set $native_flags (local.get 0)))
        (func (export "menu") (param i32 i32) (result i32)
            (if (i32.ne (global.get $native_flags) (i32.const 2)) (then unreachable))
            (i32.store (i32.const 128) (i32.const 0))
            (if (global.get $menu_open)
                (then
                    (i32.store (i32.const 132) (i32.const 4096))
                    (i32.store (i32.const 136) (i32.const {})))
                (else
                    (i32.store (i32.const 132) (i32.const 2048))
                    (i32.store (i32.const 136) (i32.const {}))))
            (i32.const 128)))"#,
        encode(&closed),
        encode(&opened),
        opened.len(),
        closed.len()
    ))
    .unwrap()
}

/// Drives the real registration worker until its latest packet candidate is ready for a join.
fn await_next_join(app: &mut App, mods: &Path, registration: &Path, timeout: std::time::Duration) {
    test_time::eventually_within(
        timeout,
        "imported packet mod is admitted for the next join",
        || {
            app.update();
            !app.world().resource::<Watcher>().preparing()
                && scan(mods, registration, String::new())
                    .active
                    .first()
                    .is_some_and(|active| active.status == launcher::mods::Status::NextJoin)
        },
    );
}

/// A serialized relay tick completes the queued menu call without timing assumptions.
fn menu_output(
    app: &App,
    relay: &dyn PacketRelay,
) -> Arc<client_extensions::modding::packet_relay::MenuOutput> {
    let packets = app.world().resource::<PacketMods>();
    let _ = packets.native_menu(mod_api::native_menu::Input::default());
    let _ = relay.tick();
    packets
        .native_menu(mod_api::native_menu::Input::default())
        .expect("the imported guest's menu callback produced a validated native panel")
}

/// Polls a stateful guest through a fixed number of serialized worker barriers.
fn exchange_external_menu(
    app: &App,
    relay: &dyn PacketRelay,
    input: &mod_api::native_menu::Input,
    retained: &mut Option<Arc<client_extensions::modding::packet_relay::MenuOutput>>,
) {
    let packets = app.world().resource::<PacketMods>();
    for _ in 0..3 {
        if let Some(output) = packets.native_menu(input.clone()) {
            *retained = Some(output);
        }
        let _ = relay.tick();
    }
    if let Some(output) = packets.native_menu(input.clone()) {
        *retained = Some(output);
    }
}

#[test]
fn imported_native_menu_requests_explicit_consent_and_starts_on_the_next_join() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let registration = root.path().join("config");
    let source = root.path().join("controls.wasm");
    fs::write(&source, native_packet_source()).unwrap();
    let mut requests = 0;
    let mut act = |action| {
        perform(
            action,
            &mods,
            &registration,
            &mut requests,
            &mut || Ok(Some(source.clone())),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap()
    };
    assert_eq!(act(Action::Import), "Imported controls.wasm.");
    assert!(registered(&registration).is_empty());
    let library = scan(&mods, &registration, String::new());
    let item = &library.installed[0];
    let id = item.id;
    let controls = item
        .permissions
        .iter()
        .find(|permission| permission.name == "Controls")
        .unwrap();
    assert_eq!(controls.requested, Some(true));
    assert!(!controls.enabled);
    assert_eq!(item.requested_counts(), (0, 2));
    act(Action::Enable(id));
    assert_eq!(
        registered(&registration)[0].entry.grants,
        mod_host::ModGrants::default(),
        "import and enable never grant authority without consent"
    );

    // A retained packet-only grant models a component installed before it gained native menus.
    let packet_capability = mod_host::ModGrants::CAPABILITIES
        .iter()
        .position(|capability| capability.name == "packets")
        .unwrap();
    act(Action::Permission(id, packet_capability, true));
    let mut app = App::new();
    app.insert_resource(Watcher::start(registration.clone()).unwrap());
    app.add_systems(
        Update,
        (crate::modding::registration::install_pending, ApplyDeferred).chain(),
    );
    await_next_join(&mut app, &mods, &registration, test_time::DEFAULT_TIMEOUT);
    let identity = protocol::session_wire::SessionIdentity {
        display_name: "Steve".into(),
        xuid: "1".into(),
        uuid: "fixture".into(),
    };
    let old = app
        .world()
        .resource::<PacketMods>()
        .session_relay()
        .unwrap();
    assert_eq!(old.start(&identity, "{}", &[1]).len(), 2);
    assert!(
        app.world()
            .resource::<PacketMods>()
            .native_menu_identity()
            .is_none(),
        "an existing packet grant does not silently acquire Controls"
    );

    act(Action::AllowRequested(id));
    assert_eq!(
        registered(&registration)[0].entry.grants,
        mod_host::ModGrants {
            packets: true,
            controls: true,
            ..Default::default()
        }
    );
    await_next_join(&mut app, &mods, &registration, test_time::DEFAULT_TIMEOUT);
    assert_eq!(
        old.outbound(&[1]),
        None,
        "changed grants revoke the old session"
    );
    let relay = app
        .world()
        .resource::<PacketMods>()
        .session_relay()
        .unwrap();
    assert_eq!(relay.start(&identity, "{}", &[1]).len(), 2);
    assert!(
        app.world()
            .resource::<PacketMods>()
            .native_menu_identity()
            .is_some()
    );
    assert!(!menu_output(&app, relay.as_ref()).output.open);
    assert_eq!(relay.outbound(&[8]), None);
    let output = menu_output(&app, relay.as_ref());
    assert!(
        output.output.open,
        "the packet command opens the native menu"
    );
    assert_eq!(
        output.output.panel.as_ref().unwrap().title,
        "Imported controls"
    );
    assert!(output.prepared.is_some());
    relay.stop();
    old.stop();
}

#[test]
fn imported_external_reactor_keeps_native_menu_alive_after_explicit_consent() {
    let (Some(source), Some(capture), Some(command)) = (
        std::env::var_os("CINNABAR_NATIVE_WASM"),
        std::env::var_os("CINNABAR_NATIVE_CAPTURE"),
        std::env::var_os("CINNABAR_NATIVE_OPEN_COMMAND"),
    ) else {
        eprintln!(
            "missing fixture: CINNABAR_NATIVE_WASM, CINNABAR_NATIVE_CAPTURE, CINNABAR_NATIVE_OPEN_COMMAND; skipping imported reactor regression"
        );
        return;
    };
    let expected_extensions = std::env::var("CINNABAR_NATIVE_INPUT_EXTENSIONS")
        .map(|json| serde_json::from_str::<mod_api::native_menu::InputExtensions>(&json).unwrap())
        .unwrap_or_default();
    expected_extensions.validate().unwrap();
    let source = PathBuf::from(source);
    let records = mod_host::packet_mod::capture::parse(&fs::read(capture).unwrap()).unwrap();
    let (identity, client_data, startup) = records
        .iter()
        .find_map(|record| match record {
            mod_host::packet_mod::capture::Record::Start {
                identity,
                client_data,
                startup,
            } => Some((identity, client_data, startup)),
            _ => None,
        })
        .expect("the synthetic capture needs a session start record");
    let command = fs::read(command).unwrap();
    assert!(
        !command.is_empty(),
        "the opening command needs encoded packet bytes"
    );
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let registration = root.path().join("config");
    let mut requests = 0;
    let mut act = |action| {
        perform(
            action,
            &mods,
            &registration,
            &mut requests,
            &mut || Ok(Some(source.clone())),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap()
    };
    act(Action::Import);
    assert!(registered(&registration).is_empty());
    let library = scan(&mods, &registration, String::new());
    let item = &library.installed[0];
    let id = item.id;
    assert_eq!(
        item.permissions
            .iter()
            .find(|permission| permission.name == "Controls")
            .unwrap()
            .requested,
        Some(true)
    );
    act(Action::Enable(id));
    assert_eq!(
        registered(&registration)[0].entry.grants,
        Default::default()
    );
    act(Action::AllowRequested(id));
    let granted = registered(&registration).pop().unwrap().entry.grants;
    assert!(granted.controls && granted.packets);
    let mut app = App::new();
    app.insert_resource(Watcher::start(registration.clone()).unwrap());
    app.add_systems(
        Update,
        (crate::modding::registration::install_pending, ApplyDeferred).chain(),
    );
    // A cold external reactor may carry a full language runtime. Use the
    // packet loader's readiness bound rather than the small WAT fixture wait.
    await_next_join(
        &mut app,
        &mods,
        &registration,
        client_extensions::modding::packet_relay::PACKET_MOD_READY_WAIT,
    );
    let relay = app
        .world()
        .resource::<PacketMods>()
        .session_relay()
        .unwrap();
    let _ = relay.start(
        &protocol::session_wire::SessionIdentity {
            display_name: identity.display_name.clone(),
            xuid: identity.xuid.clone(),
            uuid: identity.uuid.clone(),
        },
        client_data,
        startup,
    );
    let native_identity = app
        .world()
        .resource::<PacketMods>()
        .native_menu_identity()
        .expect("ordinary imported consent must activate the native menu after the next join");
    let mut input = mod_api::native_menu::Input {
        viewport: Some([640.0, 400.0]),
        pointer_held: true,
        pointer_cancelled: true,
        shift_held: true,
        hud_geometry: [(
            "fixture.clock".into(),
            mod_api::native_menu::HudGeometry {
                rect: [10.0, 20.0, 50.0, 15.0],
                size: [50.0, 15.0],
            },
        )]
        .into(),
        ..Default::default()
    };
    let mut retained = None;
    exchange_external_menu(&app, relay.as_ref(), &input, &mut retained);
    if let Some(initial) = &retained {
        assert!(!initial.output.open);
    }
    assert_eq!(
        app.world().resource::<PacketMods>().native_menu_identity(),
        Some(native_identity),
        "the initial closed poll must leave the imported guest active"
    );
    let _ = relay.outbound(&command);
    exchange_external_menu(&app, relay.as_ref(), &input, &mut retained);
    assert!(
        retained.as_ref().is_some_and(|output| output.output.open),
        "the encoded command must open the menu before local visibility acknowledges it"
    );
    input.open = true;
    for _ in 0..3 {
        exchange_external_menu(&app, relay.as_ref(), &input, &mut retained);
        let output = retained
            .as_ref()
            .expect("the encoded command must publish the imported guest's menu");
        assert!(
            output.output.open,
            "the encoded command opens the native menu"
        );
        assert_eq!(
            output.output.input_extensions.clone().unwrap_or_default(),
            expected_extensions,
            "the imported guest must retain only its explicitly accepted input groups"
        );
        assert!(output.output.panel.is_some() && output.prepared.is_some());
        assert_eq!(
            app.world().resource::<PacketMods>().native_menu_identity(),
            Some(native_identity),
            "negotiated input fields must not quarantine the imported guest"
        );
    }
    relay.stop();
}
