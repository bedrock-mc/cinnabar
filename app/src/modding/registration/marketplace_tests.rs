//! A held marketplace program cannot regain packet authority through pending loader results.

use super::*;

#[test]
fn pending_reload_cannot_reauthorize_a_held_marketplace_packet_program() {
    for prepared_reload in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut world, _messages, _retiring) = world();
        let entry = Entry {
            component: root.path().join("marketplace-packet.wasm"),
            enabled: true,
            grants: mod_host::ModGrants {
                packets: true,
                ..Default::default()
            },
            ..Default::default()
        };
        fs::write(&entry.component, packet_bytes()).unwrap();
        let candidate = Box::new(build_candidate(source_snapshot(entry.clone()).unwrap()).unwrap());
        let CandidateRuntime::Packet(program) = &candidate.runtime else {
            panic!("fixture must compile as a packet program");
        };
        // Seed a previously admitted program; its marketplace evidence is now unavailable.
        crate::modding::packets::publish(
            &mut world,
            entry.clone(),
            candidate.identity,
            program.clone(),
        );
        let relay = {
            let mut packets = world.resource_mut::<crate::modding::packets::PacketMods>();
            assert!(packets.authorize(std::slice::from_ref(&entry)).is_empty());
            packets.session_relay().unwrap()
        };
        let identity = protocol::session_wire::SessionIdentity {
            display_name: "Steve".into(),
            xuid: "1".into(),
            uuid: "u".into(),
        };
        assert_eq!(relay.start(&identity, "{}", &[1]).len(), 2);
        assert_eq!(relay.outbound(&[1]), Some(vec![0]));
        world
            .resource::<Watcher>()
            .desired
            .lock()
            .unwrap()
            .registration = Some(Registration {
            version: VERSION,
            request_id: "held".into(),
            mods: vec![entry.clone()],
        });
        install::sync_authority(&mut world);
        assert_eq!(relay.outbound(&[1]), None);

        let result = if prepared_reload {
            Ok(candidate)
        } else {
            Err("Source held while reloading.".into())
        };
        apply(
            &mut world,
            vec![entry.clone()],
            vec![Change {
                entry: entry.clone(),
                result,
            }],
        );
        let packets = world.resource::<crate::modding::packets::PacketMods>();
        assert!(!packets.admitted(&entry.component));
        assert!(packets.session_relay().is_none());
        assert_eq!(relay.outbound(&[1]), None);
    }
}
