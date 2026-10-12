//! Passive, bounded ability evidence. This module does not resolve permissions.
//! Wire fields follow protocol 2193 and the pinned gophertunnel
//! `minecraft/protocol/ability.go` at b725d82563e9.
use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::{CommandPermissionLevel, PlayerPermissionLevel};
use bedrock_protocol_1_26_51::types::SerializedAbilitiesData;
use bedrock_protocol_1_26_51::wire;

use crate::ProtocolError;

/// Client retention policy, not a protocol or vanilla layer limit.
pub const MAX_ABILITY_LAYERS: usize = 32;
const LAYER_WIRE_BYTES: usize = 22;

/// An uninterpreted layer. Float bits preserve non-finite values without using them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityLayerEvidence {
    pub layer_type: u16,
    pub abilities: u32,
    pub values: u32,
    pub fly_speed_bits: u32,
    pub vertical_fly_speed_bits: u32,
    pub walk_speed_bits: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbilityLayersEvidence {
    /// Packet order is significant evidence, not a precedence rule.
    Received(Arc<[AbilityLayerEvidence]>),
    /// A correctly framed update exceeds the client retention policy.
    Unavailable { declared_layers: u32 },
}

/// One received update, addressed by persistent actor identity, not runtime ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbilitiesUpdate {
    pub actor_unique_id: i64,
    pub player_permission: i8,
    pub command_permission: u8,
    pub layers: AbilityLayersEvidence,
}

/// Decode the complete UpdateAbilities body without materializing an unbounded collection.
/// Unknown discriminants, masks and float bits are inert evidence, not wire errors.
pub fn decode_abilities_update(body: &[u8]) -> Result<AbilitiesUpdate, ProtocolError> {
    if body.len() < 10 {
        return Err(wire::DecodeError::UnexpectedEof {
            needed: 10,
            remaining: body.len(),
        }
        .into());
    }
    let mut reader = wire::Reader::new(body);
    let actor_unique_id = i64::from_le_bytes(reader.read_bytes()?);
    let [player_permission, command_permission] = reader.read_bytes::<2>()?;
    let player_permission = player_permission as i8;
    let count = reader.read_var_u32()?;
    let required = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(LAYER_WIRE_BYTES))
        .ok_or(wire::DecodeError::VarIntTooLarge)?;
    if required > reader.remaining() {
        return Err(ProtocolError::TruncatedPacket {
            declared: required,
            available: reader.remaining(),
        });
    }
    if required < reader.remaining() {
        return Err(ProtocolError::TrailingPacketBytes {
            remaining: reader.remaining() - required,
        });
    }
    let layers = if count as usize > MAX_ABILITY_LAYERS {
        AbilityLayersEvidence::Unavailable {
            declared_layers: count,
        }
    } else {
        let mut layers = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let u32_le =
                |reader: &mut wire::Reader<'_>| reader.read_bytes().map(u32::from_le_bytes);
            layers.push(AbilityLayerEvidence {
                layer_type: u16::from_le_bytes(reader.read_bytes()?),
                abilities: u32_le(&mut reader)?,
                values: u32_le(&mut reader)?,
                fly_speed_bits: u32_le(&mut reader)?,
                vertical_fly_speed_bits: u32_le(&mut reader)?,
                walk_speed_bits: u32_le(&mut reader)?,
            });
        }
        AbilityLayersEvidence::Received(layers.into())
    };
    Ok(AbilitiesUpdate {
        actor_unique_id,
        player_permission,
        command_permission,
        layers,
    })
}

pub(crate) fn normalize_abilities(data: SerializedAbilitiesData) -> AbilitiesUpdate {
    let player_permission = PlayerPermissionLevel::to_raw(data.player_permissions);
    let command_permission = CommandPermissionLevel::to_raw(data.command_permissions);
    let layers = if data.layers.len() > MAX_ABILITY_LAYERS {
        AbilityLayersEvidence::Unavailable {
            declared_layers: u32::try_from(data.layers.len()).unwrap_or(u32::MAX),
        }
    } else {
        AbilityLayersEvidence::Received(
            data.layers
                .into_iter()
                .map(|layer| AbilityLayerEvidence {
                    layer_type: layer.serialized_layer.0,
                    abilities: layer.abilities_set.0,
                    values: layer.ability_values.0,
                    fly_speed_bits: layer.fly_speed.0.to_bits(),
                    vertical_fly_speed_bits: layer.vertical_fly_speed.0.to_bits(),
                    walk_speed_bits: layer.walk_speed.0.to_bits(),
                })
                .collect(),
        )
    };
    AbilitiesUpdate {
        actor_unique_id: data.target_player_raw_id.0,
        player_permission,
        command_permission,
        layers,
    }
}
