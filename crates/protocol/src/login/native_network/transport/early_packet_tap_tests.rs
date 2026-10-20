use super::super::{LoginSequence, SessionLogin};
use super::*;
use crate::packet_tap::{PacketDirection, PacketOutcome};
use crate::session_transport::{
    Inbound,
    tests::{RecordingTap, TapEvent, packet_bytes},
};
use bedrock_net::{
    BatchCodec, BatchLimits, CompressionAlgorithm, CompressionSettings, DecodeOutcome,
};
use bedrock_protocol_1_26_51::{enums::PlayStatusType, packets, types::ActorRuntimeID, wire};
use nethernet_tokio::{AcceptedSession, NetherServer, ServerData};
use std::{sync::Arc, time::Duration};

/// Sends original generated entries through the public server's reliable channel.
async fn send<S: bedrock_net::BatchPhase>(
    peer: &AcceptedSession,
    codec: &BatchCodec<S>,
    packets: &[crate::Packet],
) {
    let entries = packets.iter().map(packet_bytes).collect::<Vec<_>>();
    for message in codec.encode(&entries).unwrap() {
        peer.session.send(message).await.unwrap();
    }
}

/// Checks a real native response without substituting a fake packet connection.
async fn receive<S: bedrock_net::BatchPhase>(
    peer: &mut AcceptedSession,
    codec: &BatchCodec<S>,
) -> Vec<packets::Packet> {
    let message = peer.reliable.recv().await.unwrap().unwrap();
    let DecodeOutcome::Packets { packets, .. } = codec.decode(message).unwrap() else {
        panic!("native response must be a valid batch");
    };
    packets
        .map(|bytes| {
            let mut reader = wire::Reader::from_shared(&bytes);
            let header = wire::PacketHeader::from_raw(reader.read_var_u32().unwrap());
            crate::codec::decode_body(
                packets::PacketId::from_raw(header.id).unwrap(),
                &bytes.slice(reader.position()..),
            )
            .unwrap()
        })
        .collect()
}

/// The expected observer payload comes from the real generated encoder after its header.
fn body(packet: &crate::Packet) -> Vec<u8> {
    let bytes = packet_bytes(packet);
    let mut reader = wire::Reader::from_shared(&bytes);
    reader.read_var_u32().unwrap();
    bytes[reader.position()..].to_vec()
}

#[tokio::test]
async fn native_early_packet_tap_records_startup_and_play_once_with_outcomes() {
    tokio::time::timeout(Duration::from_secs(20), tapped_native_join())
        .await
        .expect("finite actual native tap fixture stalled");
}

/// Exercises the retained raw prefix through the actual native join and play owners.
async fn tapped_native_join() {
    let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let signaling = LanSignaling::with_config(
        31,
        address,
        LanConfig {
            discovery_port: address.port(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut data = ServerData::new("Fixture".into(), "World".into());
    data.flag_b = true;
    data.session_id = "synthetic-nonce".into();
    data.connection_type = LAN_CONNECTION_TYPE;
    data.protocol_version = crate::PROTOCOL_VERSION as u32;
    signaling.set_server_data(data);
    let mut listener = NetherServer::bind_with(
        signaling,
        ConnectionConfig {
            allow_unsigned_answers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let tap = Arc::new(RecordingTap::default());
    let time = |value| {
        crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(value),
        })
    };
    let startup = [
        crate::Packet::new(packets::PlayStatus {
            status: PlayStatusType::Playerspawn,
        }),
        time(1),
        time(2),
        crate::Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        }),
        crate::Packet::new(packets::ItemRegistry::default()),
        crate::Packet::new(packets::ChunkRadiusUpdated {
            chunk_radius: wire::ZigZag32(8),
        }),
        time(3),
    ];
    let server = async {
        let mut peer = listener.accept().await.unwrap();
        let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
        assert!(matches!(
            &receive(&mut peer, &codec).await[0],
            packets::Packet::RequestNetworkSettings(_)
        ));
        send(
            &peer,
            &codec,
            &[crate::Packet::new(packets::NetworkSettings {
                compression_threshold: wire::U16LE(u16::MAX),
                ..Default::default()
            })],
        )
        .await;
        let codec = codec.negotiate(CompressionSettings {
            algorithm: CompressionAlgorithm::Deflate,
            threshold: u16::MAX,
        });
        assert!(matches!(
            &receive(&mut peer, &codec).await[0],
            packets::Packet::Login(_)
        ));
        send(
            &peer,
            &codec,
            &[
                crate::Packet::new(packets::PlayStatus {
                    status: PlayStatusType::Loginsuccess,
                }),
                crate::Packet::new(packets::ResourcePacksInfo::default()),
            ],
        )
        .await;
        assert!(matches!(
            &receive(&mut peer, &codec).await[0],
            packets::Packet::ClientCacheStatus(_)
        ));
        assert!(matches!(
            &receive(&mut peer, &codec).await[0],
            packets::Packet::ResourcePackClientResponse(_)
        ));
        let mut packets = startup[..3].to_vec();
        packets.push(crate::Packet::new(packets::ResourcePackStack::default()));
        packets.extend_from_slice(&startup[3..]);
        send(&peer, &codec, &packets).await;
        assert!(matches!(
            &receive(&mut peer, &codec).await[0],
            packets::Packet::ResourcePackClientResponse(_)
        ));
        assert!(matches!(
            &receive(&mut peer, &codec).await[0],
            packets::Packet::RequestChunkRadius(_)
        ));
        peer.session.closed().await;
    };
    let client = async {
        let settings = super::super::super::LoginSettings::default();
        let (mut play, ()) = LoginSequence::connect_native(
            &NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan),
            SessionLogin {
                display_name: "Fixture",
                client_random_id: crate::ClientRandomId::new(1).unwrap(),
                cache: None,
                skin: None,
                settings: &settings,
                tap: Some(tap.clone()),
                #[cfg(not(target_arch = "wasm32"))]
                relay: None,
                target: None,
            },
            |_, _| (),
        )
        .await
        .unwrap();
        let Inbound::Packet(first) = play.transport.recv().await.unwrap() else {
            panic!("early play prefix must survive startup");
        };
        assert_eq!(first.clone().decode().unwrap(), time(1));
        play.transport.reject_current(PacketOutcome::Skipped);
        play.transport.reject_current(PacketOutcome::Failed);
        for expected in [time(2), startup[5].clone(), time(3)] {
            assert_eq!(play.recv().await.unwrap(), expected);
        }
        let events = tap.events();
        let recorded = events
            .iter()
            .filter_map(|event| match event {
                TapEvent::Record(PacketDirection::Clientbound, id, body) => {
                    Some((*id, body.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let expected = startup
            .iter()
            .map(|packet| (packet.header.id as u32, body(packet)))
            .collect::<Vec<_>>();
        assert_eq!(
            recorded, expected,
            "every early and unread sibling must be observed once, in original order"
        );
        let record = first
            .record
            .expect("adopted early packet must own a real observer handle");
        let outcomes = events
            .iter()
            .filter(|event| matches!(event, TapEvent::Reject(..)))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes,
            [TapEvent::Reject(
                record.0,
                packets::PacketId::SetTime as u32,
                PacketOutcome::Skipped,
                body(&time(1))
            )]
        );
        assert!(
            matches!(&events[record.0 as usize], TapEvent::Record(PacketDirection::Clientbound, id, recorded_body)
                if *id == packets::PacketId::SetTime as u32 && *recorded_body == body(&time(1)))
        );
        play.close().await.unwrap();
    };
    tokio::join!(server, client);
    listener.close().await.unwrap();
}
