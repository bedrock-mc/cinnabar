//! Settings import, explicit consent, registration admission and a real protocol join.

use super::*;
use crate::modding::{
    packets::PacketMods,
    registration::{self, Watcher},
};
use bevy::prelude::{App, ApplyDeferred, IntoScheduleConfigs, Update};
use protocol::session_wire::{
    CoreMessage, SessionHandoff, SessionIdentity, batch_frame_from_bedrock, decode_core_message,
    encode_core_message,
};
use std::io::{Cursor, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// An original minimal pack makes successful reactor startup observable to preparation.
fn fixture_pack() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"format_version":2,"header":{"name":"Session fixture","description":"Original test data","uuid":"9db65801-ddd9-4eaf-b624-7fb558041158","version":[1,0,0]},"modules":[{"type":"resources","uuid":"2cf4c16a-34e0-4df2-bdd5-59294479e954","version":[1,0,0]}]}"#).unwrap();
    zip.finish().unwrap().into_inner()
}

/// A canonical reactor forwards every batch and returns one identifiable session pack.
fn forwarding_reactor(pack: &[u8]) -> Vec<u8> {
    let escaped: String = pack.iter().map(|byte| format!("\\{byte:02x}")).collect();
    let source = r#"(module
        (memory (export "memory") 1)
        (global $next (mut i32) (i32.const 4096))
        (data (i32.const 1024) "$PACK")
        (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
            (local $old i32)
            (local.set $old (global.get $next))
            (global.set $next (i32.add (global.get $next) (local.get 3)))
            (local.get $old))
        (func (export "start") (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
            (memory.fill (i32.const 128) (i32.const 0) (i32.const 20))
            (i32.store (i32.const 132) (i32.const 256))
            (i32.store (i32.const 136) (i32.const 1))
            (i32.store (i32.const 256) (i32.const 1024))
            (i32.store (i32.const 260) (i32.const $PACKLEN))
            (i32.const 128))
        (func (export "on-batch") (param i32 i32 i32) (result i32)
            (memory.fill (i32.const 128) (i32.const 0) (i32.const 20)) (i32.const 128))
        (func (export "tick") (result i32)
            (memory.fill (i32.const 128) (i32.const 0) (i32.const 20)) (i32.const 128))
        (func (export "stop")))"#;
    wat::parse_str(
        source
            .replace("$PACKLEN", &pack.len().to_string())
            .replace("$PACK", &escaped),
    )
    .unwrap()
}

/// Uses the production core schema; the bridge owns packet and frame encodings.
async fn send_core(peer: &mut tokio::io::DuplexStream, message: CoreMessage) {
    let frame = encode_core_message(&message).unwrap();
    peer.write_u32(frame.len().try_into().unwrap())
        .await
        .unwrap();
    peer.write_all(&frame).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_import_and_consent_reach_a_new_session_join() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let config = root.path().join("config");
    let source = root.path().join("forward.wasm");
    let pack = fixture_pack();
    fs::write(&source, forwarding_reactor(&pack)).unwrap();
    let mut requests = 0;
    perform(
        Action::Import,
        &mods,
        &config,
        &mut requests,
        &mut || Ok(Some(source.clone())),
        &mut |_| panic!("import cannot request retirement"),
    )
    .unwrap();
    let shown = scan(&mods, &config, String::new());
    let id = shown.installed[0].id;
    assert!(
        shown.installed[0]
            .permissions
            .iter()
            .all(|permission| !permission.enabled)
    );
    let mut action = |action| {
        perform(
            action,
            &mods,
            &config,
            &mut requests,
            &mut || panic!("selection cannot open a picker"),
            &mut |_| panic!("selection cannot request retirement"),
        )
        .unwrap()
    };
    action(Action::Enable(id));
    assert!(
        !registered(&config)[0].entry.grants.packets,
        "enabling cannot grant authority"
    );
    action(Action::AllowRequested(id));
    assert!(registered(&config)[0].entry.grants.packets);

    let mut app = App::new();
    app.insert_resource(Watcher::start(config.clone()).unwrap());
    app.add_systems(
        Update,
        (registration::install_pending, ApplyDeferred).chain(),
    );
    let mut relay = None;
    test_time::eventually("Settings source reaches the session loader", || {
        app.update();
        relay = app
            .world()
            .get_resource::<PacketMods>()
            .and_then(PacketMods::session_relay);
        relay.is_some()
    });
    let relay = relay.unwrap();
    let (client, mut peer) = tokio::io::duplex(8192);
    let (read, write) = tokio::io::split(client);
    let (reader, frames, writer) =
        bridge::tasks::prepare_connection(read, write, bridge::MAX_FRAME_LEN);
    let writer = tokio::spawn(writer);
    let core = async {
        let length = peer.read_u32().await.unwrap();
        let mut connect = vec![0; length as usize];
        peer.read_exact(&mut connect).await.unwrap();
        protocol::session_wire::decode_connect(&connect).unwrap();
        let fixture = include_bytes!("../../../../../crates/protocol/fixtures/start_game.bin");
        let CoreMessage::Batch(startup) =
            decode_core_message(batch_frame_from_bedrock(fixture).unwrap()).unwrap()
        else {
            panic!("StartGame fixture is one batch")
        };
        send_core(
            &mut peer,
            CoreMessage::Handoff(SessionHandoff {
                identity: SessionIdentity {
                    display_name: "Fixture".into(),
                    ..Default::default()
                },
                startup,
                ..Default::default()
            }),
        )
        .await;
        for packet in [
            protocol::test_support::packets::item_registry_packet(),
            protocol::test_support::packets::chunk_radius_updated_packet(8),
            protocol::test_support::packets::player_spawn_packet(),
        ] {
            let message = decode_core_message(
                batch_frame_from_bedrock(&protocol::encode(&packet).unwrap()).unwrap(),
            )
            .unwrap();
            send_core(&mut peer, message).await;
        }
        peer
    };
    let settings = protocol::LoginSettings::default();
    let join = protocol::LoginSequence::connect_prepared(
        protocol::SessionIo {
            reader,
            frames,
            timers: bridge::tasks::Timers::tokio(),
        },
        protocol::SessionLogin {
            display_name: "Fixture",
            client_random_id: protocol::ClientRandomId::new(1).unwrap(),
            cache: None,
            skin: None,
            settings: &settings,
            tap: None,
            relay: Some(relay),
            target: None,
        },
        &protocol::NativeJoinPreparation,
        move |mut packs, data| {
            let added = packs.take_relay_packs();
            assert_eq!(added.len(), 1, "the imported reactor ran start");
            assert_eq!(
                added[0].archive, pack,
                "its session output reaches preparation"
            );
            data
        },
    );
    let (joined, _peer) = tokio::join!(join, core);
    let (mut session, _) = joined.expect("the normally imported reactor permits a complete spawn");
    session.finish_loading().await.unwrap();
    action(Action::Disable(id));
    test_time::eventually("Settings disable removes the session selection", || {
        app.update();
        app.world()
            .resource::<PacketMods>()
            .identities()
            .next()
            .is_none()
    });
    assert!(
        app.world()
            .resource::<PacketMods>()
            .session_relay()
            .is_none()
    );
    drop(session);
    writer.await.unwrap();
}
