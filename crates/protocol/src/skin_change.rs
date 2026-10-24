//! Client-authored classic appearance updates.

use render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION;

use bedrock_protocol_1_26_51::enums::PersonaArmSizeType;
use bedrock_protocol_1_26_51::packets::{self, PlayerSkin};
use bedrock_protocol_1_26_51::types::{SerializedSkinRef, SkinImage};
use bedrock_protocol_1_26_51::wire;
use sha2::{Digest, Sha256};

/// The built-in classic skin pack used by custom wide and slim models.
const CLASSIC_SKIN_PACK_ID: &str = "c18e65aa-7b21-4637-9b63-8ad63622ef01";

/// The pack used for images imported by the player.
const CUSTOM_SKIN_PACK_ID: &str = "aaaaaaaa-7b21-4637-9b63-8ad63622ef01";

/// Geometry data for a skin without its own model: the styled JSON `null` that Bedrock writes for
/// an empty geometry document. Servers parse this field as JSON, so it is never left empty.
const ABSENT_GEOMETRY_DATA: &str = "null\n";

/// Returns a built-in skin's pack identity without minting a per-login UUID.
pub fn builtin_skin_id(name: &str) -> String {
    format!("{CLASSIC_SKIN_PACK_ID}.{name}")
}

/// Returns an imported skin's pack identity from its persisted image name.
pub fn custom_skin_id(image_name: &str) -> String {
    format!("{CUSTOM_SKIN_PACK_ID}.{image_name}")
}

/// Returns the classic model's skin-pack identity, independent of login and player UUIDs.
pub(crate) fn classic_skin_id(arm_size: &str) -> String {
    let name = if arm_size == "slim" {
        "CustomSlim"
    } else {
        "Custom"
    };
    builtin_skin_id(name)
}

/// Selects the classic model when no skin-authored resource patch is available.
pub(crate) fn classic_skin_resource_patch(arm_size: &str) -> String {
    let model = if arm_size == "slim" {
        "geometry.humanoid.customSlim"
    } else {
        "geometry.humanoid.custom"
    };
    serde_json::json!({"geometry":{"default":model}}).to_string()
}

pub fn cape_content_id(cape: &render_api::CapeImage) -> String {
    let mut hash = Sha256::new();
    hash.update(cape.width.to_le_bytes());
    hash.update(cape.height.to_le_bytes());
    hash.update(cape.rgba8.as_ref());
    format!("cape:{:x}", hash.finalize())
}

pub fn set_skin_packet_uuid(packet: &mut crate::Packet, uuid: [u8; 16]) {
    if let packets::Packet::PlayerSkin(skin) = &mut *packet.data {
        skin.uuid = uuid::Uuid::from_bytes(uuid);
    }
}

/// Retains the selected model version on a client-authored appearance update.
pub fn set_skin_packet_engine_version(packet: &mut crate::Packet, version: &str) {
    if let packets::Packet::PlayerSkin(skin) = &mut *packet.data {
        skin.serialized_skin.geometry_data_min_engine_version = version.into();
    }
}

/// Sends the same pixels and geometry used by the local preview.
pub fn player_skin_packet(
    uuid: [u8; 16],
    skin: &render_api::StandardSkin,
    arm_size: &str,
    id: &str,
    name: &str,
) -> crate::Packet {
    let geometry = skin.geometry.as_ref();
    let cape = skin.cape.as_ref().filter(|cape| cape.is_valid());
    let cape_id = cape.map_or_else(String::new, cape_content_id);
    crate::Packet::new(PlayerSkin {
        uuid: uuid::Uuid::from_bytes(uuid),
        serialized_skin: SerializedSkinRef {
            id: id.into(),
            full_id: if cape_id.is_empty() {
                id.into()
            } else {
                format!("{id}_{cape_id}").into()
            },
            cape_image_data: cape.map_or_else(SkinImage::default, |cape| SkinImage {
                width: wire::U32LE(cape.width),
                height: wire::U32LE(cape.height),
                image_bytes: cape.rgba8.to_vec().into(),
            }),
            cape_id: cape_id.into(),
            resource_patch: geometry
                .map_or_else(
                    || classic_skin_resource_patch(arm_size),
                    |source| source.resource_patch.to_string(),
                )
                .into(),
            image_data: SkinImage {
                width: wire::U32LE(skin.width),
                height: wire::U32LE(skin.height),
                image_bytes: skin.rgba8.to_vec().into(),
            },
            geometry_data: geometry
                .map_or(ABSENT_GEOMETRY_DATA, |source| &*source.geometry_data)
                .into(),
            geometry_data_min_engine_version: DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
            arm_size: if arm_size == "slim" {
                PersonaArmSizeType::Slim
            } else {
                PersonaArmSizeType::Wide
            },
            is_primary_user: true,
            ..Default::default()
        },
        localized_new_skin_name: name.into(),
        localized_old_skin_name: wire::Str::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn selected_skin_update_preserves_wire_pixels_geometry_and_model() {
        let skin = render_api::StandardSkin {
            width: render_api::CLASSIC_SKIN_SIDE as u32,
            height: render_api::CLASSIC_SKIN_SIDE as u32,
            rgba8: vec![7; render_api::CLASSIC_SKIN_SIDE * render_api::CLASSIC_SKIN_SIDE * 4]
                .into(),
            cape: Some(render_api::CapeImage {
                width: render_api::CAPE_DIMENSIONS[0].0,
                height: render_api::CAPE_DIMENSIONS[0].1,
                rgba8: vec![
                    63;
                    (render_api::CAPE_DIMENSIONS[0].0 * render_api::CAPE_DIMENSIONS[0].1 * 4)
                        as usize
                ]
                .into(),
            }),
            geometry: Some(Arc::new(render_api::SkinGeometrySource {
                resource_patch: "{\"geometry\":{\"default\":\"geometry.humanoid.customSlim\"}}"
                    .into(),
                geometry_data: "{\"geometry.humanoid.customSlim\":{}}".into(),
                animations: Arc::from([]),
            })),
        };
        let id = custom_skin_id("fixture");
        let packet = player_skin_packet([9; 16], &skin, "slim", &id, "Fixture");
        let mut packet = packet;
        set_skin_packet_engine_version(&mut packet, "1.12.0");
        let encoded = crate::encode(&packet).unwrap();
        let packet = crate::decode_batch(encoded).unwrap().pop().unwrap();
        let packets::Packet::PlayerSkin(wire) = *packet.data else {
            panic!("skin packet")
        };
        assert_eq!(*wire.uuid.as_bytes(), [9; 16]);
        assert_eq!(
            wire.serialized_skin.image_data.image_bytes,
            skin.rgba8.as_ref()
        );
        assert_eq!(wire.serialized_skin.arm_size, PersonaArmSizeType::Slim);
        assert_eq!(
            wire.serialized_skin.geometry_data,
            &*skin.geometry.as_ref().unwrap().geometry_data
        );
        assert_eq!(
            wire.serialized_skin.geometry_data_min_engine_version,
            "1.12.0"
        );
        assert!(wire.serialized_skin.animation_data.is_empty());
        assert!(!wire.serialized_skin.is_persona);
        assert!(!wire.serialized_skin.is_premium);
        assert_eq!(wire.localized_new_skin_name, "Fixture");
        let cape = skin.cape.as_ref().unwrap();
        assert_eq!(
            wire.serialized_skin.cape_image_data.image_bytes,
            cape.rgba8.as_ref()
        );
        assert_eq!(
            (
                wire.serialized_skin.cape_image_data.width.0,
                wire.serialized_skin.cape_image_data.height.0
            ),
            (cape.width, cape.height)
        );
        assert_eq!(wire.serialized_skin.cape_id, cape_content_id(cape).as_str());
        assert_eq!(
            wire.serialized_skin.full_id,
            format!("{id}_{}", cape_content_id(cape)).as_str()
        );
    }

    #[test]
    fn absent_cape_clears_the_wire_image_and_identity() {
        let skin = render_api::StandardSkin {
            width: render_api::CLASSIC_SKIN_SIDE as u32,
            height: render_api::CLASSIC_SKIN_SIDE as u32,
            rgba8: vec![255; render_api::CLASSIC_SKIN_SIDE * render_api::CLASSIC_SKIN_SIDE * 4]
                .into(),
            cape: None,
            geometry: None,
        };
        let id = classic_skin_id("wide");
        let mut packet = player_skin_packet([1; 16], &skin, "wide", &id, "");
        set_skin_packet_uuid(&mut packet, [3; 16]);
        let packets::Packet::PlayerSkin(wire) = *packet.data else {
            panic!("skin packet")
        };
        assert_eq!(wire.uuid.as_bytes(), &[3; 16]);
        assert_eq!(wire.serialized_skin.cape_image_data.image_bytes.len(), 0);
        assert_eq!(wire.serialized_skin.cape_id, "");
        assert_eq!(wire.serialized_skin.id, classic_skin_id("wide").as_str());
        assert_eq!(
            wire.serialized_skin.full_id,
            classic_skin_id("wide").as_str()
        );
    }

    /// Servers such as Dragonfly disconnect a player whose appearance update has unparsable
    /// geometry, so a classic skin changing its cape must still send JSON geometry.
    #[test]
    fn classic_skin_cape_change_sends_json_geometry() {
        let cape = render_api::CapeImage {
            width: render_api::CAPE_DIMENSIONS[0].0,
            height: render_api::CAPE_DIMENSIONS[0].1,
            rgba8: vec![
                9;
                (render_api::CAPE_DIMENSIONS[0].0 * render_api::CAPE_DIMENSIONS[0].1 * 4)
                    as usize
            ]
            .into(),
        };
        for cape in [Some(cape), None] {
            let skin = render_api::StandardSkin {
                width: render_api::CLASSIC_SKIN_SIDE as u32,
                height: render_api::CLASSIC_SKIN_SIDE as u32,
                rgba8: vec![255; render_api::CLASSIC_SKIN_SIDE * render_api::CLASSIC_SKIN_SIDE * 4]
                    .into(),
                cape,
                geometry: None,
            };
            let packet = player_skin_packet([1; 16], &skin, "wide", &classic_skin_id("wide"), "");
            let encoded = crate::encode(&packet).unwrap();
            let packet = crate::decode_batch(encoded).unwrap().pop().unwrap();
            let packets::Packet::PlayerSkin(wire) = *packet.data else {
                panic!("skin packet")
            };
            let geometry: serde_json::Value =
                serde_json::from_slice(wire.serialized_skin.geometry_data.as_ref())
                    .expect("geometry data is JSON");
            assert_eq!(geometry, serde_json::Value::Null);
        }
    }
}
