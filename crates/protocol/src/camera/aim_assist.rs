//! Aim-assist packet normalization; semantic failures stay recoverable.

use std::sync::Arc;

use bedrock_protocol_1_26_51::{enums, packets, types, wire};

use super::{CameraEvent, MAX_CAMERA_PRESETS, bounded_identifier, validate_count, validate_finite};
use crate::WorldPacketError;

use super::aim_assist_types::{
    CameraAimAssistAction, CameraAimAssistActorPriority, CameraAimAssistCategory,
    CameraAimAssistExclusions, CameraAimAssistItemSetting, CameraAimAssistPreset,
    CameraAimAssistPresetSettings, CameraAimAssistPriorities, CameraAimAssistPriority,
    CameraAimAssistRegistry, CameraAimAssistSettings, CameraAimAssistTargetMode,
};

/// Bounds each collection in a preset's exclusion and priority tables.
pub const MAX_CAMERA_AIM_ASSIST_ENTRIES: usize = 4096;

/// Reports camera-preset activation so the server can send authoritative aim-assist settings.
#[must_use]
pub fn camera_aim_assist_activation_packet(
    preset_id: &str,
    allow: bool,
    clear: bool,
) -> crate::Packet {
    const MAX_NAME_BYTES: usize = 64;
    let mut end = preset_id.len().min(MAX_NAME_BYTES);
    while !preset_id.is_char_boundary(end) {
        end -= 1;
    }
    crate::Packet::new(packets::ClientCameraAimAssist {
        camera_preset_id: preset_id[..end].into(),
        action: if clear {
            enums::ClientCameraAimAssistAction::Clear
        } else {
            enums::ClientCameraAimAssistAction::Setfromcamerapreset
        },
        allow_aim_assist: allow,
    })
}

/// Accepts both explicit target ranking modes and rejects unknown selectors.
fn mode(value: u8) -> Result<CameraAimAssistTargetMode, WorldPacketError> {
    match value {
        0 => Ok(CameraAimAssistTargetMode::Angle),
        1 => Ok(CameraAimAssistTargetMode::Distance),
        _ => Err(WorldPacketError::InvalidCameraField {
            field: "aim_assist.target_mode",
        }),
    }
}

/// Keeps complete commands; the runtime decides how clear interacts with presets.
pub(crate) fn normalize_settings(
    packet: packets::CameraAimAssist,
) -> Result<CameraEvent, WorldPacketError> {
    validate_finite(packet.view_angle.x, "aim_assist.view_angle.x")?;
    validate_finite(packet.view_angle.y, "aim_assist.view_angle.y")?;
    validate_finite(packet.distance.0, "aim_assist.distance")?;
    let action = match packet.action {
        enums::CameraAimAssistAction::Set => CameraAimAssistAction::Set,
        enums::CameraAimAssistAction::Clear => CameraAimAssistAction::Clear,
        _ => {
            return Err(WorldPacketError::InvalidCameraField {
                field: "aim_assist.action",
            });
        }
    };
    let target_mode = match packet.target_mode {
        enums::TargetMode::Angle => mode(0)?,
        enums::TargetMode::Distance => mode(1)?,
        _ => {
            return Err(WorldPacketError::InvalidCameraField {
                field: "aim_assist.target_mode",
            });
        }
    };
    Ok(CameraEvent::AimAssist(CameraAimAssistSettings {
        preset_id: bounded_identifier(packet.preset_id, "aim_assist.preset")?,
        view_angle: [packet.view_angle.x, packet.view_angle.y],
        distance: packet.distance.0,
        target_mode,
        action,
        show_debug_render: packet.show_debug_render,
    }))
}

/// Optional camera-preset settings remain optional for inheritance.
pub(super) fn normalize_preset_settings(
    definition: types::CameraAimAssistCommandPresetDefinition,
) -> Result<CameraAimAssistPresetSettings, WorldPacketError> {
    let target_mode = definition
        .target_mode
        .map(|value| match value {
            enums::CameraAimAssistTargetMode::Angle => mode(0),
            enums::CameraAimAssistTargetMode::Distance => mode(1),
            _ => Err(WorldPacketError::InvalidCameraField {
                field: "aim_assist.target_mode",
            }),
        })
        .transpose()?;
    if let Some(angle) = &definition.view_angle {
        validate_finite(angle.x, "aim_assist.view_angle.x")?;
        validate_finite(angle.y, "aim_assist.view_angle.y")?;
    }
    let distance = definition.distance.map(|distance| distance.0);
    if let Some(distance) = distance {
        validate_finite(distance, "aim_assist.distance")?;
    }
    Ok(CameraAimAssistPresetSettings {
        preset_id: definition
            .preset_id
            .map(|id| bounded_identifier(id, "aim_assist.preset"))
            .transpose()?,
        target_mode,
        view_angle: definition.view_angle.map(|v| [v.x, v.y]),
        distance,
    })
}

/// Converts bounded identifier lists used by exclusions and liquid targeting.
fn identifiers(values: Vec<wire::Str>) -> Result<Arc<[Arc<str>]>, WorldPacketError> {
    validate_count(
        values.len(),
        MAX_CAMERA_AIM_ASSIST_ENTRIES,
        "aim_assist.identifiers",
    )?;
    values
        .into_iter()
        .map(|value| bounded_identifier(value, "aim_assist.identifier"))
        .collect()
}

/// Preserves declared block, entity and tag priority entries.
fn priorities(
    values: Vec<(wire::Str, wire::I32LE)>,
) -> Result<Arc<[CameraAimAssistPriority]>, WorldPacketError> {
    validate_count(
        values.len(),
        MAX_CAMERA_AIM_ASSIST_ENTRIES,
        "aim_assist.priorities",
    )?;
    values
        .into_iter()
        .map(|(key, value)| {
            Ok(CameraAimAssistPriority {
                identifier: bounded_identifier(key, "aim_assist.priority.identifier")?,
                priority: value.0,
            })
        })
        .collect()
}

/// Normalizes all four priority sources without resolving world actors or blocks.
fn category(
    value: types::CameraAimAssistCategoryDefinition,
) -> Result<CameraAimAssistCategory, WorldPacketError> {
    let p = value.priorities;
    Ok(CameraAimAssistCategory {
        name: bounded_identifier(value.name, "aim_assist.category")?,
        priorities: CameraAimAssistPriorities {
            entities: priorities(p.entities)?,
            blocks: priorities(p.blocks)?,
            block_tags: priorities(p.block_tags)?,
            entity_type_families: priorities(p.entity_type_families)?,
            entity_default: p.entity_default.map(|value| value.0),
            block_default: p.block_default.map(|value| value.0),
        },
    })
}

/// Retains item-category associations and all exclusion classes.
fn preset(
    value: types::CameraAimAssistPresetDefinition,
) -> Result<CameraAimAssistPreset, WorldPacketError> {
    validate_count(
        value.item_settings.len(),
        MAX_CAMERA_AIM_ASSIST_ENTRIES,
        "aim_assist.item_settings",
    )?;
    let e = value.exclusion_settings;
    Ok(CameraAimAssistPreset {
        identifier: bounded_identifier(value.identifier, "aim_assist.preset")?,
        exclusions: CameraAimAssistExclusions {
            blocks: identifiers(e.blocks)?,
            entities: identifiers(e.entities)?,
            block_tags: identifiers(e.block_tags)?,
            entity_type_families: identifiers(e.entity_type_families)?,
        },
        liquid_targeting_list: identifiers(value.liquid_targeting_list)?,
        item_settings: value
            .item_settings
            .into_iter()
            .map(|(item, category)| {
                Ok(CameraAimAssistItemSetting {
                    item: bounded_identifier(item, "aim_assist.item")?,
                    category: bounded_identifier(category, "aim_assist.item.category")?,
                })
            })
            .collect::<Result<Arc<[_]>, WorldPacketError>>()?,
        default_item_settings: value
            .default_item_settings
            .map(|v| bounded_identifier(v, "aim_assist.default_item_settings"))
            .transpose()?,
        hand_settings: value
            .hand_settings
            .map(|v| bounded_identifier(v, "aim_assist.hand_settings"))
            .transpose()?,
    })
}

/// The wire's first list contains categories and its second list contains presets.
pub(crate) fn normalize_aim_presets(
    packet: packets::CameraAimAssistPresets,
) -> Result<CameraEvent, WorldPacketError> {
    validate_count(
        packet.camera_aim_assist_presets.len(),
        MAX_CAMERA_PRESETS,
        "aim_assist.categories",
    )?;
    validate_count(
        packet.camera_aim_assist_categories.len(),
        MAX_CAMERA_PRESETS,
        "aim_assist.presets",
    )?;
    let replace = match packet.operation {
        enums::CameraAimAssistPresetOperation::Set => true,
        enums::CameraAimAssistPresetOperation::Addtoexisting => false,
        _ => {
            return Err(WorldPacketError::InvalidCameraField {
                field: "aim_assist.operation",
            });
        }
    };
    Ok(CameraEvent::AimAssistPresets(CameraAimAssistRegistry {
        categories: packet
            .camera_aim_assist_presets
            .into_iter()
            .map(category)
            .collect::<Result<Arc<[_]>, _>>()?,
        presets: packet
            .camera_aim_assist_categories
            .into_iter()
            .map(preset)
            .collect::<Result<Arc<[_]>, _>>()?,
        replace,
    }))
}

/// Metadata index keys are server-defined values, including negative sentinels.
pub(crate) fn normalize_actor_priorities(
    packet: packets::CameraAimAssistActorPriority,
) -> Result<CameraEvent, WorldPacketError> {
    validate_count(
        packet.camera_aim_assist_actor_priority_list.len(),
        MAX_CAMERA_AIM_ASSIST_ENTRIES,
        "aim_assist.actor_priorities",
    )?;
    Ok(CameraEvent::AimAssistActorPriority(
        packet
            .camera_aim_assist_actor_priority_list
            .into_iter()
            .map(|p| CameraAimAssistActorPriority {
                preset_index: p.preset_index.0,
                category_index: p.category_index.0,
                actor_index: p.actor_index.0,
                priority: p.priority_value.0,
            })
            .collect(),
    ))
}
