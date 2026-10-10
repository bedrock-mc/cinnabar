//! The Bedrock login client-data claims the core presents upstream for this client.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ClientSkin, GAME_VERSION};

/// Returns the client-data claims for `display_name` and `skin`; `None` uploads the solid-white
/// 64x64 placeholder skin. Like the GDK Windows client this reports Win32 with a lowercase-hex
/// device ID, since BDS 1.26.5x drops logins that claim the retired Win10 platform.
pub(crate) fn login_client_data(display_name: &str, skin: Option<&ClientSkin>) -> Value {
    let identity = Uuid::new_v4();
    let (skin_data, skin_width, skin_height, arm_size) = match skin {
        Some(skin) => (
            STANDARD.encode(&skin.rgba8),
            skin.width,
            skin.height,
            skin.arm_size.clone(),
        ),
        None => (
            STANDARD.encode(vec![255u8; 64 * 64 * 4]),
            64,
            64,
            "wide".to_owned(),
        ),
    };
    let cape = skin.and_then(|skin| skin.cape.as_ref());
    let geometry = skin.and_then(|skin| skin.geometry.as_ref());
    let resource_patch = geometry.map_or_else(
        || {
            let model = if arm_size == "slim" {
                "geometry.humanoid.customSlim"
            } else {
                "geometry.humanoid.custom"
            };
            STANDARD.encode(json!({"geometry": {"default": model}}).to_string())
        },
        |geometry| STANDARD.encode(&geometry.resource_patch),
    );
    // Two halves keep each `json!` expansion within the default recursion limit.
    let mut claims = json!({
        "ClientRandomId": (Uuid::new_v4().as_u64_pair().0 & 0x7fff_ffff_ffff_ffff) as i64,
        "CompatibleWithClientSideChunkGen": true,
        "CurrentInputMode": 1,
        "DefaultInputMode": 1,
        "DeviceId": Uuid::new_v4().simple().to_string(),
        "DeviceModel": "JolyneClient",
        "DeviceOS": 8,
        "GameVersion": GAME_VERSION,
        "GraphicsMode": 0,
        "GuiScale": 0,
        "IsEditorMode": false,
        "LanguageCode": "en_US",
        "MaxViewDistance": 32,
        "MemoryTier": 5,
        "PlatformOfflineId": "",
        "PlatformOnlineId": "",
        "PlatformType": 0,
        "PlayFabId": "",
        "SelfSignedId": identity.to_string(),
        "ServerAddress": "",
        "ThirdPartyName": display_name,
        "ThirdPartyNameOnly": false,
        "UIProfile": 0,
    });
    let skin_claims = json!({
        "AnimatedImageData": [],
        "ArmSize": arm_size,
        "CapeData": cape.map_or_else(String::new, |cape| STANDARD.encode(&cape.rgba8)),
        "CapeId": cape.map_or_else(String::new, |cape| cape.id.clone()),
        "CapeImageHeight": cape.map_or(0, |cape| cape.height),
        "CapeImageWidth": cape.map_or(0, |cape| cape.width),
        "CapeOnClassicSkin": cape.is_some(),
        "OverrideSkin": false,
        "PersonaPieces": [],
        "PersonaSkin": false,
        "PieceTintColors": [],
        "PremiumSkin": false,
        "SkinAnimationData": "",
        "SkinColor": "#b37b62",
        "SkinData": skin_data,
        "SkinGeometryData": geometry.map_or_else(String::new, |geometry| STANDARD.encode(&geometry.geometry_data)),
        "SkinGeometryDataEngineVersion": geometry.map_or_else(String::new, |geometry| STANDARD.encode(&geometry.engine_version)),
        "SkinId": format!("{identity}.Custom"),
        "SkinImageHeight": skin_height,
        "SkinImageWidth": skin_width,
        "SkinResourcePatch": resource_patch,
        "TrustedSkin": false,
    });
    if let (Value::Object(claims), Value::Object(skin_claims)) = (&mut claims, skin_claims) {
        claims.extend(skin_claims);
    }
    claims
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientCape, ClientSkinGeometry};

    fn decoded(claims: &Value, field: &str) -> Vec<u8> {
        STANDARD
            .decode(claims[field].as_str().expect("base64 string field"))
            .expect("valid base64")
    }

    /// The uploaded skin, cape and geometry reach the claims byte for byte.
    #[test]
    fn claims_carry_the_supplied_skin() {
        let skin = ClientSkin {
            rgba8: vec![7; 64 * 32 * 4],
            width: 64,
            height: 32,
            arm_size: "slim".into(),
            cape: Some(ClientCape {
                rgba8: vec![9; 64 * 32 * 4],
                width: 64,
                height: 32,
                id: "cape-id".into(),
            }),
            geometry: Some(ClientSkinGeometry {
                resource_patch: "{\"patch\":1}".into(),
                geometry_data: "{\"geometry\":2}".into(),
                engine_version: "1.26.50".into(),
            }),
        };
        let claims = login_client_data("Steve", Some(&skin));
        assert_eq!(decoded(&claims, "SkinData"), skin.rgba8);
        assert_eq!(
            (
                claims["SkinImageWidth"].as_u64(),
                claims["SkinImageHeight"].as_u64()
            ),
            (Some(64), Some(32))
        );
        assert_eq!(claims["ArmSize"], "slim");
        assert_eq!(decoded(&claims, "CapeData"), vec![9; 64 * 32 * 4]);
        assert_eq!(claims["CapeId"], "cape-id");
        assert_eq!(claims["CapeOnClassicSkin"], true);
        assert_eq!(decoded(&claims, "SkinResourcePatch"), b"{\"patch\":1}");
        assert_eq!(decoded(&claims, "SkinGeometryData"), b"{\"geometry\":2}");
        assert_eq!(
            decoded(&claims, "SkinGeometryDataEngineVersion"),
            b"1.26.50"
        );
        assert_eq!(claims["PremiumSkin"], false);
        assert_eq!(claims["PersonaSkin"], false);
        assert_eq!(claims["SkinAnimationData"], "");
        assert_eq!(claims["AnimatedImageData"], json!([]));
    }

    /// Without a skin the placeholder satisfies the core's size check and names the wide model.
    #[test]
    fn claims_without_a_skin_upload_the_placeholder() {
        let claims = login_client_data("Alex", None);
        assert_eq!(decoded(&claims, "SkinData").len(), 64 * 64 * 4);
        let patch: Value = serde_json::from_slice(&decoded(&claims, "SkinResourcePatch")).unwrap();
        assert_eq!(patch["geometry"]["default"], "geometry.humanoid.custom");
        assert_eq!(claims["CapeData"], "");
        assert_eq!(claims["CapeOnClassicSkin"], false);
    }

    /// The fields the core validates name this client's version, platform and offline name.
    #[test]
    fn claims_identify_the_client_as_the_core_requires() {
        let claims = login_client_data("Steve", None);
        assert_eq!(claims["GameVersion"], GAME_VERSION);
        assert_eq!(claims["ThirdPartyName"], "Steve");
        assert_eq!(claims["DeviceOS"], 8);
        assert_eq!(claims["LanguageCode"], "en_US");
        let device = claims["DeviceId"].as_str().unwrap();
        assert!(
            device.len() == 32
                && device
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert!(claims["SkinId"].as_str().unwrap().ends_with(".Custom"));
        assert!(Uuid::parse_str(claims["SelfSignedId"].as_str().unwrap()).is_ok());
    }
}
