use super::*;

/// Writes the host's packet-mod test guest, without WASI, as a core module source.
fn guest(directory: &std::path::Path) -> PathBuf {
    let path = directory.join("packet.wat");
    let source = include_str!("../../../../mod-host/src/packet_mod/tests/guest.wat")
        .replace("$IMPORTS", "")
        .replace("$START", "");
    std::fs::write(&path, source).unwrap();
    path
}

fn selection(path: PathBuf) -> PacketModSelection {
    PacketModSelection {
        path,
        grants: ModGrants {
            packets: true,
            ..Default::default()
        },
    }
}

fn identity() -> protocol::session_wire::SessionIdentity {
    protocol::session_wire::SessionIdentity {
        display_name: "Steve".into(),
        xuid: "1".into(),
        uuid: "u".into(),
    }
}

#[test]
fn a_selected_packet_mod_relays_each_session() {
    let directory = tempfile::tempdir().unwrap();
    let loader = PacketModLoader::default();
    assert!(loader.session_relay().is_none(), "nothing is selected");
    drop(loader.select(Some(selection(guest(directory.path())))));
    for _ in 0..2 {
        let relay = loader
            .session_relay()
            .expect("a selected packet mod relays");
        let packs = relay.start(&identity(), "{}", &[1]);
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].archive, b"Steve");
        assert_eq!(relay.inbound_startup(&[1]), Some(vec![1]));
        assert_eq!(relay.inbound(&[0]), None);
        assert_eq!(relay.inbound(&[1]), Some(vec![1]));
        assert_eq!(relay.outbound(&[1]), Some(vec![0]));
        assert_eq!(relay.outbound(&[2]), Some(Vec::new()));
        let output = relay.tick();
        assert_eq!(
            (output.to_client, output.to_server),
            (b"c".to_vec(), b"s".to_vec())
        );
        relay.stop();
        assert_eq!(relay.inbound(&[1]), None, "a stopped relay passes through");
    }
}

#[test]
fn a_failing_packet_mod_passes_everything_through() {
    let directory = tempfile::tempdir().unwrap();
    let loader = PacketModLoader::default();
    drop(loader.select(Some(selection(directory.path().join("missing.wasm")))));
    let relay = loader.session_relay().expect("the selection still relays");
    assert!(relay.start(&identity(), "{}", &[]).is_empty());
    assert_eq!(relay.inbound(&[1]), None);
    let output = relay.tick();
    assert!(output.to_client.is_empty() && output.to_server.is_empty());

    let path = guest(directory.path());
    drop(loader.select(Some(selection(path))));
    let relay = loader.session_relay().unwrap();
    relay.start(&identity(), "{}", &[]);
    assert_eq!(relay.inbound(&[3]), None, "a trap passes its batch through");
    assert_eq!(relay.inbound(&[1]), None, "and quarantines the instance");
    drop(loader.select(None));
    assert!(loader.session_relay().is_none());
}

#[test]
fn clearing_or_replacing_the_selection_revokes_existing_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let path = guest(directory.path());
    let selected = selection(path.clone());
    let program = Arc::new(CompiledPacketMod::load(&path, &selected.grants).unwrap());
    let loader = PacketModLoader::default();
    drop(loader.select_prepared(selected.clone(), Arc::clone(&program)));
    let old = loader.session_relay().unwrap();
    old.start(&identity(), "{}", &[1]);
    assert_eq!(old.outbound(&[1]), Some(vec![0]));
    drop(loader.select_prepared(selected, program));
    assert_eq!(
        old.outbound(&[1]),
        None,
        "a replaced source cannot keep acting in its previous session"
    );
    let fresh = loader.session_relay().unwrap();
    fresh.start(&identity(), "{}", &[1]);
    assert_eq!(fresh.outbound(&[1]), Some(vec![0]));
    drop(loader.select(None));
    assert_eq!(fresh.outbound(&[1]), None);
    assert!(fresh.tick().to_client.is_empty());
    assert!(loader.session_relay().is_none());
}

#[test]
fn a_session_created_before_reselection_never_starts_the_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = guest(directory.path());
    let selected = selection(path.clone());
    let program = Arc::new(CompiledPacketMod::load(&path, &selected.grants).unwrap());
    let loader = PacketModLoader::default();
    drop(loader.select_prepared(selected.clone(), Arc::clone(&program)));
    let stale = loader.session_relay().unwrap();
    drop(loader.select_prepared(selected, program));
    assert!(stale.start(&identity(), "{}", &[1]).is_empty());
    assert_eq!(stale.outbound(&[1]), None);
}

/// Selection must transfer its executable owner to the caller's disposal worker.
#[test]
fn selection_returns_the_old_executable_for_off_thread_retirement() {
    let directory = tempfile::tempdir().unwrap();
    let path = guest(directory.path());
    let selected = selection(path.clone());
    let program = Arc::new(CompiledPacketMod::load(&path, &selected.grants).unwrap());
    let weak = Arc::downgrade(&program);
    let loader = PacketModLoader::default();
    assert!(loader.select_prepared(selected.clone(), program).is_none());
    let retired = loader.select(None).expect("clearing transfers ownership");
    assert!(
        weak.upgrade().is_some(),
        "select must not destroy its executable"
    );
    let thread = std::thread::spawn(move || drop(retired));
    thread.join().unwrap();
    assert!(
        weak.upgrade().is_none(),
        "the disposal worker released the executable"
    );
    let first = Arc::new(CompiledPacketMod::load(&path, &selected.grants).unwrap());
    let replacement = Arc::new(CompiledPacketMod::load(&path, &selected.grants).unwrap());
    assert!(
        loader
            .select_prepared(selected.clone(), first.clone())
            .is_none()
    );
    let retired = loader.select_prepared(selected, replacement).unwrap();
    assert!(Arc::ptr_eq(&first, &retired));
}
