//! Bounded, vendor-independent named-audio ingress.

use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{self, LevelEvent, LevelSoundEvent, PlaySound, StopSound};
use bedrock_protocol_1_26_51::wire;

use crate::WorldPacketError;

/// Maximum UTF-8 bytes retained for one server-authored audio identifier.
///
/// This is an allocation-safety ceiling, not an identifier allowlist. Empty
/// and unknown identifiers remain valid and are preserved exactly.
pub const MAX_AUDIO_IDENTIFIER_BYTES: usize = 1_024;

#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    Play(PlayAudioEvent),
    Stop(StopAudioEvent),
    Level(LevelAudioEvent),
    /// A sound-only `LevelEventPacket` (the 1000-range ids).
    LevelEvent(LevelEventSound),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelEventSound {
    pub event_id: i32,
    pub position: [f32; 3],
    pub data: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayAudioEvent {
    pub name: Arc<str>,
    pub position: [i32; 3],
    pub volume: f32,
    pub pitch: f32,
    pub loop_count: i32,
    pub server_sound_handle: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopAudioEvent {
    pub name: Arc<str>,
    pub stop_all_sounds: bool,
    pub stop_music_legacy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LevelAudioEvent {
    pub sound_event: Arc<str>,
    pub position: [f32; 3],
    pub data: i32,
    pub actor_identifier: Arc<str>,
    pub is_baby: bool,
    pub is_global: bool,
    pub actor_unique_id: i64,
    pub fire_at_position: Option<[f32; 3]>,
}

/// Returns the sound event for `LevelEventPacket` ids in the sound range; others and
/// non-finite positions are skipped.
pub(crate) fn normalize_level_event_sound(packet: &LevelEvent) -> Option<AudioEvent> {
    if !(1000..2000).contains(&packet.event_id.0) {
        return None;
    }
    let position = packet.position.to_array();
    position
        .iter()
        .all(|value| value.is_finite())
        .then_some(())?;
    Some(AudioEvent::LevelEvent(LevelEventSound {
        event_id: packet.event_id.0,
        position,
        data: packet.data.0,
    }))
}

/// Rejects a decoded sound packet whose identifiers are oversized or not UTF-8, or whose
/// volume, pitch or positions are non-finite; other packets pass.
pub(crate) fn validate_audio_packet(packet: &packets::Packet) -> Result<(), WorldPacketError> {
    match packet {
        packets::Packet::PlaySound(packet) => {
            validate_wire_identifier(&packet.name, "PlaySound.name")?;
            validate_finite(packet.volume.0, "PlaySound.volume")?;
            validate_finite(packet.pitch.0, "PlaySound.pitch")
        }
        packets::Packet::StopSound(packet) => {
            validate_wire_identifier(&packet.sound_name, "StopSound.name")
        }
        packets::Packet::LevelSoundEvent(packet) => {
            validate_wire_identifier(&packet.sound_event, "LevelSoundEvent.sound_event")?;
            validate_wire_identifier(&packet.actor_identifier, "LevelSoundEvent.actor_identifier")?;
            validate_position(packet.position.to_array(), "LevelSoundEvent.position")?;
            if let Some(position) = &packet.fire_at_position {
                validate_position(position.to_array(), "LevelSoundEvent.fire_at_position")?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn normalize_play_sound(packet: PlaySound) -> Result<AudioEvent, WorldPacketError> {
    let name = packet.name.to_string_lossy();
    validate_identifier(&name, "PlaySound.name")?;
    validate_finite(packet.volume.0, "PlaySound.volume")?;
    validate_finite(packet.pitch.0, "PlaySound.pitch")?;
    let position = &packet.position;
    Ok(AudioEvent::Play(PlayAudioEvent {
        name: Arc::from(name),
        position: [position.x.0, position.y.0, position.z.0],
        volume: packet.volume.0,
        pitch: packet.pitch.0,
        loop_count: packet.loop_count.0,
        server_sound_handle: packet.server_sound_handle.map(|handle| handle.0),
    }))
}

pub(crate) fn normalize_stop_sound(packet: StopSound) -> Result<AudioEvent, WorldPacketError> {
    let name = packet.sound_name.to_string_lossy();
    validate_identifier(&name, "StopSound.name")?;
    Ok(AudioEvent::Stop(StopAudioEvent {
        name: Arc::from(name),
        stop_all_sounds: packet.stop_all_sounds,
        stop_music_legacy: packet.stop_music_legacy,
    }))
}

pub(crate) fn normalize_level_sound(
    packet: LevelSoundEvent,
) -> Result<AudioEvent, WorldPacketError> {
    let sound_event = packet.sound_event.to_string_lossy();
    let actor_identifier = packet.actor_identifier.to_string_lossy();
    validate_identifier(&sound_event, "LevelSoundEvent.sound_event")?;
    validate_identifier(&actor_identifier, "LevelSoundEvent.actor_identifier")?;
    let position = packet.position.to_array();
    validate_position(position, "LevelSoundEvent.position")?;
    let fire_at_position = packet.fire_at_position.map(|position| position.to_array());
    if let Some(position) = fire_at_position {
        validate_position(position, "LevelSoundEvent.fire_at_position")?;
    }
    Ok(AudioEvent::Level(LevelAudioEvent {
        sound_event: Arc::from(sound_event),
        position,
        data: packet.data.0,
        actor_identifier: Arc::from(actor_identifier),
        is_baby: packet.is_baby,
        is_global: packet.is_global,
        actor_unique_id: packet.actor_unique_id.0,
        fire_at_position,
    }))
}

fn validate_wire_identifier(
    value: &wire::Str,
    field: &'static str,
) -> Result<(), WorldPacketError> {
    validate_identifier_bytes(value.len(), field)?;
    value
        .to_str()
        .map(|_| ())
        .map_err(|_| WorldPacketError::InvalidAudioIdentifierUtf8 { field })
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), WorldPacketError> {
    validate_identifier_bytes(value.len(), field)
}

fn validate_identifier_bytes(bytes: usize, field: &'static str) -> Result<(), WorldPacketError> {
    if bytes > MAX_AUDIO_IDENTIFIER_BYTES {
        return Err(WorldPacketError::AudioIdentifierTooLong {
            field,
            bytes,
            max: MAX_AUDIO_IDENTIFIER_BYTES,
        });
    }
    Ok(())
}

fn validate_position(position: [f32; 3], field: &'static str) -> Result<(), WorldPacketError> {
    for value in position {
        validate_finite(value, field)?;
    }
    Ok(())
}

fn validate_finite(value: f32, field: &'static str) -> Result<(), WorldPacketError> {
    if !value.is_finite() {
        return Err(WorldPacketError::NonFiniteAudioField { field });
    }
    Ok(())
}

#[cfg(test)]
mod level_event_tests {
    use super::*;

    fn level(event_id: i32, x: f32) -> LevelEvent {
        LevelEvent {
            event_id: wire::ZigZag32(event_id),
            position: [x, 1.0, 2.0].into(),
            data: wire::ZigZag32(7),
        }
    }

    #[test]
    fn sound_range_level_events_normalize_and_others_are_skipped() {
        let Some(AudioEvent::LevelEvent(event)) = normalize_level_event_sound(&level(1003, 4.0))
        else {
            panic!("sound-range event");
        };
        assert_eq!(
            (event.event_id, event.data, event.position),
            (1003, 7, [4.0, 1.0, 2.0])
        );
        assert!(normalize_level_event_sound(&level(2001, 0.0)).is_none());
        assert!(normalize_level_event_sound(&level(1003, f32::NAN)).is_none());
    }
}
