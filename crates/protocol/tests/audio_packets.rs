use std::sync::Arc;

use bytes::Bytes;
use protocol::{
    AudioEvent, BedrockSession, LevelAudioEvent, PlayAudioEvent, ProtocolError, StopAudioEvent,
    WorldEvent, WorldPacketError, decode_batch, encode, into_world_event,
};
use valentine::bedrock::version::v1_26_44::{McpePacketData, McpePacketName, PlaySoundPacket};

const PLAY_SOUND: &[u8] = include_bytes!("../fixtures/play_sound.bin");
const STOP_SOUND: &[u8] = include_bytes!("../fixtures/stop_sound.bin");
const LEVEL_SOUND_EVENT: &[u8] = include_bytes!("../fixtures/level_sound_event.bin");

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn decode_one(fixture: &'static [u8], id: McpePacketName) -> protocol::Packet {
    let mut packets =
        decode_batch(Bytes::from_static(fixture), &session()).expect("decode fixture");
    assert_eq!(packets.len(), 1);
    let packet = packets.pop().expect("one packet");
    assert_eq!(packet.header.id, id);
    assert_eq!(
        (packet.header.from_subclient, packet.header.to_subclient),
        (1, 2)
    );
    assert_eq!(encode(&packet, &session()).unwrap().as_ref(), fixture);
    packet
}

#[test]
fn public_protocol_2168_audio_fixtures_preserve_exact_generated_fields() {
    let play = decode_one(PLAY_SOUND, McpePacketName::PlaySoundPacket);
    let McpePacketData::PlaySoundPacket(play) = &play.data else {
        panic!("expected PlaySound")
    };
    assert_eq!(play.name, "custom:odd.sound");
    assert_eq!(
        [play.position.x, play.position.y, play.position.z],
        [10, -20, 31]
    );
    assert_eq!((play.volume, play.pitch, play.loop_count), (-0.25, 3.5, -7));
    assert_eq!(
        play.server_sound_handle
            .as_ref()
            .map(|handle| handle.server_sound_handle),
        Some(0x0123_4567_89ab_cdef)
    );

    let stop = decode_one(STOP_SOUND, McpePacketName::StopSoundPacket);
    let McpePacketData::StopSoundPacket(stop) = &stop.data else {
        panic!("expected StopSound")
    };
    assert!(stop.sound_name.is_empty());
    assert!(stop.stop_all_sounds);
    assert!(stop.stop_music_legacy);

    let level = decode_one(LEVEL_SOUND_EVENT, McpePacketName::LevelSoundEventPacket);
    let McpePacketData::LevelSoundEventPacket(level) = &level.data else {
        panic!("expected LevelSoundEvent")
    };
    assert_eq!(level.sound_event, "custom:unmapped.event");
    assert_eq!(
        [level.position.x, level.position.y, level.position.z],
        [-1.25, 64.5, 2.75]
    );
    assert_eq!(level.data, -12_345);
    assert!(level.actor_identifier.is_empty());
    assert!(level.is_baby);
    assert!(level.is_global);
    assert_eq!(level.actor_unique_id, -42);
    let fire = level
        .fire_at_position
        .as_ref()
        .expect("optional fire position");
    assert_eq!([fire.x, fire.y, fire.z], [9.25, -4.5, 0.125]);
}

#[test]
fn fixtures_normalize_without_interpreting_odd_or_empty_fields() {
    let play = into_world_event(decode_one(PLAY_SOUND, McpePacketName::PlaySoundPacket), 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        play,
        WorldEvent::Audio(AudioEvent::Play(PlayAudioEvent {
            name: Arc::from("custom:odd.sound"),
            position: [10, -20, 31],
            volume: -0.25,
            pitch: 3.5,
            loop_count: -7,
            server_sound_handle: Some(0x0123_4567_89ab_cdef),
        }))
    );

    let stop = into_world_event(decode_one(STOP_SOUND, McpePacketName::StopSoundPacket), 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        stop,
        WorldEvent::Audio(AudioEvent::Stop(StopAudioEvent {
            name: Arc::from(""),
            stop_all_sounds: true,
            stop_music_legacy: true,
        }))
    );

    let level = into_world_event(
        decode_one(LEVEL_SOUND_EVENT, McpePacketName::LevelSoundEventPacket),
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        level,
        WorldEvent::Audio(AudioEvent::Level(LevelAudioEvent {
            sound_event: Arc::from("custom:unmapped.event"),
            position: [-1.25, 64.5, 2.75],
            data: -12_345,
            actor_identifier: Arc::from(""),
            is_baby: true,
            is_global: true,
            actor_unique_id: -42,
            fire_at_position: Some([9.25, -4.5, 0.125]),
        }))
    );
}

#[test]
fn batch_decode_bounds_audio_before_allocation_and_keeps_truncation_fatal() {
    let packet: protocol::Packet = PlaySoundPacket {
        name: "x".repeat(protocol::MAX_AUDIO_IDENTIFIER_BYTES + 1),
        volume: 1.0,
        pitch: 1.0,
        ..Default::default()
    }
    .into();
    let overlong = encode(&packet, &session()).unwrap();
    assert!(matches!(
        decode_batch(overlong, &session()),
        Err(ProtocolError::World(
            WorldPacketError::AudioIdentifierTooLong { .. }
        ))
    ));

    let packet: protocol::Packet = PlaySoundPacket {
        volume: f32::INFINITY,
        pitch: 1.0,
        ..Default::default()
    }
    .into();
    let non_finite = encode(&packet, &session()).unwrap();
    assert!(matches!(
        decode_batch(non_finite, &session()),
        Err(ProtocolError::World(
            WorldPacketError::NonFiniteAudioField { .. }
        ))
    ));

    let truncated = Bytes::copy_from_slice(&PLAY_SOUND[..PLAY_SOUND.len() - 1]);
    assert!(matches!(
        decode_batch(truncated, &session()),
        Err(ProtocolError::TruncatedPacket { .. })
    ));
}
