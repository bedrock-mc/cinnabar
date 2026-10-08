//! Client-authored classic appearance updates.

use sha2::{Digest, Sha256};
use valentine::bedrock::version::v1_26_51::{
    EnumsSharedTypespersonaArmSizeType, PlayerSkinPacket, SerializedSkinRef, SkinImage,
};

pub fn cape_content_id(cape: &crate::CapeImage) -> String {
    let mut hash = Sha256::new();
    hash.update(cape.width.to_le_bytes());
    hash.update(cape.height.to_le_bytes());
    hash.update(cape.rgba8.as_ref());
    format!("cape:{:x}", hash.finalize())
}

pub fn set_skin_packet_uuid(packet: &mut crate::Packet, uuid: [u8; 16]) {
    if let valentine::bedrock::version::v1_26_51::McpePacketData::PlayerSkinPacket(skin) =
        &mut packet.data
    {
        skin.uuid = uuid::Uuid::from_bytes(uuid);
    }
}

/// Sends the same pixels and geometry used by the local preview.
pub fn player_skin_packet(
    uuid: [u8; 16],
    skin: &crate::StandardSkin,
    arm_size: &str,
    id: &str,
    name: &str,
) -> crate::Packet {
    let geometry = skin.geometry.as_ref();
    let cape = skin.cape.as_ref().filter(|cape| cape.is_valid());
    let cape_id = cape.map_or_else(String::new, cape_content_id);
    PlayerSkinPacket {
        uuid: uuid::Uuid::from_bytes(uuid),
        serialized_skin: SerializedSkinRef {
            id: id.to_owned(),
            full_id: if cape_id.is_empty() {
                id.to_owned()
            } else {
                format!("{id}_{cape_id}")
            },
            cape_id,
            cape_image_data: cape.map_or_else(SkinImage::default, |cape| SkinImage {
                width: cape.width,
                height: cape.height,
                image_bytes: cape.rgba8.to_vec(),
            }),
            resource_patch: geometry.map_or_else(
                || {
                    serde_json::json!({"geometry":{"default":"geometry.humanoid.custom"}})
                        .to_string()
                },
                |source| source.resource_patch.to_string(),
            ),
            image_data: SkinImage {
                width: skin.width,
                height: skin.height,
                image_bytes: skin.rgba8.to_vec(),
            },
            geometry_data: geometry
                .map_or_else(String::new, |source| source.geometry_data.to_string()),
            arm_size: if arm_size == "slim" {
                EnumsSharedTypespersonaArmSizeType::Slim
            } else {
                EnumsSharedTypespersonaArmSizeType::Wide
            },
            is_primary_user: true,
            ..Default::default()
        },
        localized_new_skin_name: name.to_owned(),
        localized_old_skin_name: String::new(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn selected_skin_update_preserves_wire_pixels_geometry_and_model() {
        let skin = crate::StandardSkin {
            width: crate::CLASSIC_SKIN_SIDE as u32,
            height: crate::CLASSIC_SKIN_SIDE as u32,
            rgba8: vec![7; crate::CLASSIC_SKIN_SIDE * crate::CLASSIC_SKIN_SIDE * 4].into(),
            cape: Some(crate::CapeImage {
                width: crate::CAPE_DIMENSIONS[0].0,
                height: crate::CAPE_DIMENSIONS[0].1,
                rgba8: vec![
                    63;
                    (crate::CAPE_DIMENSIONS[0].0 * crate::CAPE_DIMENSIONS[0].1 * 4) as usize
                ]
                .into(),
            }),
            geometry: Some(Arc::new(crate::SkinGeometrySource {
                resource_patch: "{\"geometry\":{\"default\":\"geometry.humanoid.customSlim\"}}"
                    .into(),
                geometry_data: "{\"geometry.humanoid.customSlim\":{}}".into(),
                animations: Arc::from([]),
            })),
        };
        let packet = player_skin_packet([9; 16], &skin, "slim", "imported", "Fixture");
        let session = crate::BedrockSession { shield_item_id: 0 };
        let encoded = crate::encode(&packet, &session).unwrap();
        let packet = crate::decode_batch(encoded, &session)
            .unwrap()
            .pop()
            .unwrap();
        let valentine::bedrock::version::v1_26_51::McpePacketData::PlayerSkinPacket(wire) =
            packet.data
        else {
            panic!("skin packet")
        };
        assert_eq!(*wire.uuid.as_bytes(), [9; 16]);
        assert_eq!(
            wire.serialized_skin.image_data.image_bytes,
            skin.rgba8.as_ref()
        );
        assert_eq!(
            wire.serialized_skin.arm_size,
            EnumsSharedTypespersonaArmSizeType::Slim
        );
        assert_eq!(
            wire.serialized_skin.geometry_data,
            skin.geometry.as_ref().unwrap().geometry_data.as_ref()
        );
        assert_eq!(wire.localized_new_skin_name, "Fixture");
        let cape = skin.cape.as_ref().unwrap();
        assert_eq!(
            wire.serialized_skin.cape_image_data.image_bytes,
            cape.rgba8.as_ref()
        );
        assert_eq!(
            (
                wire.serialized_skin.cape_image_data.width,
                wire.serialized_skin.cape_image_data.height
            ),
            (cape.width, cape.height)
        );
        assert_eq!(wire.serialized_skin.cape_id, cape_content_id(cape));
        assert_eq!(
            wire.serialized_skin.full_id,
            format!("imported_{}", cape_content_id(cape))
        );
    }

    #[test]
    fn absent_cape_clears_the_wire_image_and_identity() {
        let skin = crate::StandardSkin {
            width: crate::CLASSIC_SKIN_SIDE as u32,
            height: crate::CLASSIC_SKIN_SIDE as u32,
            rgba8: vec![255; crate::CLASSIC_SKIN_SIDE * crate::CLASSIC_SKIN_SIDE * 4].into(),
            cape: None,
            geometry: None,
        };
        let mut packet = player_skin_packet([1; 16], &skin, "wide", "selected", "");
        set_skin_packet_uuid(&mut packet, [3; 16]);
        let valentine::bedrock::version::v1_26_51::McpePacketData::PlayerSkinPacket(wire) =
            packet.data
        else {
            panic!("skin packet")
        };
        assert_eq!(wire.uuid.as_bytes(), &[3; 16]);
        assert_eq!(wire.serialized_skin.cape_image_data.image_bytes.len(), 0);
        assert_eq!(wire.serialized_skin.cape_id, "");
        assert_eq!(wire.serialized_skin.full_id, "selected");
    }
}
