//! Bounded, vendor-independent named-audio ingress.

use std::sync::Arc;

use valentine::bedrock::borrowed::BorrowedStr;
use valentine::bedrock::version::v1_26_44::{
    BorrowedMcpePacketData, LevelSoundEventPacket, PlaySoundPacket, StopSoundPacket,
};

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

pub(crate) fn validate_borrowed_audio_packet(
    packet: &BorrowedMcpePacketData,
) -> Result<(), WorldPacketError> {
    match packet {
        BorrowedMcpePacketData::PlaySoundPacket(packet) => {
            validate_borrowed_identifier(&packet.name, "PlaySound.name")?;
            validate_finite(packet.volume, "PlaySound.volume")?;
            validate_finite(packet.pitch, "PlaySound.pitch")
        }
        BorrowedMcpePacketData::StopSoundPacket(packet) => {
            validate_borrowed_identifier(&packet.sound_name, "StopSound.name")
        }
        BorrowedMcpePacketData::LevelSoundEventPacket(packet) => {
            validate_borrowed_identifier(&packet.sound_event, "LevelSoundEvent.sound_event")?;
            validate_borrowed_identifier(
                &packet.actor_identifier,
                "LevelSoundEvent.actor_identifier",
            )?;
            validate_position(
                [packet.position.x, packet.position.y, packet.position.z],
                "LevelSoundEvent.position",
            )?;
            if let Some(position) = &packet.fire_at_position {
                validate_position(
                    [position.x, position.y, position.z],
                    "LevelSoundEvent.fire_at_position",
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn normalize_play_sound(
    packet: PlaySoundPacket,
) -> Result<AudioEvent, WorldPacketError> {
    validate_identifier(&packet.name, "PlaySound.name")?;
    validate_finite(packet.volume, "PlaySound.volume")?;
    validate_finite(packet.pitch, "PlaySound.pitch")?;
    Ok(AudioEvent::Play(PlayAudioEvent {
        name: Arc::from(packet.name),
        position: [packet.position.x, packet.position.y, packet.position.z],
        volume: packet.volume,
        pitch: packet.pitch,
        loop_count: packet.loop_count,
        server_sound_handle: packet
            .server_sound_handle
            .map(|handle| handle.server_sound_handle),
    }))
}

pub(crate) fn normalize_stop_sound(
    packet: StopSoundPacket,
) -> Result<AudioEvent, WorldPacketError> {
    validate_identifier(&packet.sound_name, "StopSound.name")?;
    Ok(AudioEvent::Stop(StopAudioEvent {
        name: Arc::from(packet.sound_name),
        stop_all_sounds: packet.stop_all_sounds,
        stop_music_legacy: packet.stop_music_legacy,
    }))
}

pub(crate) fn normalize_level_sound(
    packet: LevelSoundEventPacket,
) -> Result<AudioEvent, WorldPacketError> {
    validate_identifier(&packet.sound_event, "LevelSoundEvent.sound_event")?;
    validate_identifier(&packet.actor_identifier, "LevelSoundEvent.actor_identifier")?;
    let position = [packet.position.x, packet.position.y, packet.position.z];
    validate_position(position, "LevelSoundEvent.position")?;
    let fire_at_position = packet
        .fire_at_position
        .map(|position| [position.x, position.y, position.z]);
    if let Some(position) = fire_at_position {
        validate_position(position, "LevelSoundEvent.fire_at_position")?;
    }
    Ok(AudioEvent::Level(LevelAudioEvent {
        sound_event: Arc::from(packet.sound_event),
        position,
        data: packet.data,
        actor_identifier: Arc::from(packet.actor_identifier),
        is_baby: packet.is_baby,
        is_global: packet.is_global,
        actor_unique_id: packet.actor_unique_id,
        fire_at_position,
    }))
}

fn validate_borrowed_identifier(
    value: &BorrowedStr,
    field: &'static str,
) -> Result<(), WorldPacketError> {
    validate_identifier_bytes(value.as_bytes().len(), field)?;
    value
        .as_str()
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
