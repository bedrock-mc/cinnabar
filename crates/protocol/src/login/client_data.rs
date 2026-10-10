//! The Bedrock login client-data claims the core presents upstream for this client. The core claims
//! the device (`DeviceOS`, `DeviceModel`, `DeviceId`, `DefaultInputMode`) to match its sign-in.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::BytesMut;
use serde_json::{Value, json};
use uuid::Uuid;
use valentine::bedrock::codec::{BedrockCodec, VarUInt};
use valentine::bedrock::version::v1_26_51::EnumsInputMode;

use crate::{ClientSkin, GAME_VERSION, PlayerInputMode};

/// The player's own settings a login reports, as vanilla's client takes them at join.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginSettings {
    /// The active UI language, in the `en_US` form of its lang file.
    pub language_code: String,
    /// The input the player is using now.
    pub input_mode: PlayerInputMode,
    /// The GUI scale offset from the video settings.
    pub gui_scale_offset: i8,
    /// Trusted local archive directory, never sent in the upstream login claims.
    pub resource_pack_cache_dir: Option<std::path::PathBuf>,
}

impl Default for LoginSettings {
    fn default() -> Self {
        Self {
            language_code: "en_US".to_owned(),
            input_mode: PlayerInputMode::Mouse,
            gui_scale_offset: 0,
            resource_pack_cache_dir: None,
        }
    }
}

/// Returns the client-data claims for `display_name`, `skin` and `settings`; `None` uploads the
/// solid-white 64x64 placeholder skin.
pub(crate) fn login_client_data(
    display_name: &str,
    skin: Option<&ClientSkin>,
    settings: &LoginSettings,
) -> Value {
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
        "CurrentInputMode": input_mode_value(settings.input_mode),
        "GameVersion": GAME_VERSION,
        "GraphicsMode": 0,
        "GuiScale": settings.gui_scale_offset,
        "IsEditorMode": false,
        "LanguageCode": settings.language_code,
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

/// The wire value of `mode`, as the generated codec encodes it.
fn input_mode_value(mode: PlayerInputMode) -> u32 {
    let mode = match mode {
        PlayerInputMode::Mouse => EnumsInputMode::Mouse,
        PlayerInputMode::Touch => EnumsInputMode::Touch,
        PlayerInputMode::GamePad => EnumsInputMode::Gamepad,
    };
    let mut encoded = BytesMut::new();
    mode.encode(&mut encoded)
        .expect("a buffer accepts an input mode");
    VarUInt::decode(&mut encoded.freeze(), ())
        .expect("an encoded input mode decodes")
        .0
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
        let claims = login_client_data("Steve", Some(&skin), &LoginSettings::default());
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
        let claims = login_client_data("Alex", None, &LoginSettings::default());
        assert_eq!(decoded(&claims, "SkinData").len(), 64 * 64 * 4);
        let patch: Value = serde_json::from_slice(&decoded(&claims, "SkinResourcePatch")).unwrap();
        assert_eq!(patch["geometry"]["default"], "geometry.humanoid.custom");
        assert_eq!(claims["CapeData"], "");
        assert_eq!(claims["CapeOnClassicSkin"], false);
    }

    /// The claims carry the version, offline name and the player's own settings, and leave the
    /// device to the core.
    #[test]
    fn claims_carry_the_players_settings_and_no_device() {
        let settings = LoginSettings {
            language_code: "de_DE".to_owned(),
            input_mode: PlayerInputMode::GamePad,
            gui_scale_offset: -1,
            ..Default::default()
        };
        let claims = login_client_data("Steve", None, &settings);
        assert_eq!(claims["GameVersion"], GAME_VERSION);
        assert_eq!(claims["ThirdPartyName"], "Steve");
        assert_eq!(claims["LanguageCode"], "de_DE");
        assert_eq!(
            claims["CurrentInputMode"], 3,
            "vanilla's gamepad input mode"
        );
        assert_eq!(claims["GuiScale"], -1);
        for device_field in ["DeviceOS", "DeviceModel", "DeviceId", "DefaultInputMode"] {
            assert!(
                claims.get(device_field).is_none(),
                "{device_field} is the core's"
            );
        }
        assert!(claims["SkinId"].as_str().unwrap().ends_with(".Custom"));
        assert!(Uuid::parse_str(claims["SelfSignedId"].as_str().unwrap()).is_ok());
        let mouse = login_client_data("Steve", None, &LoginSettings::default());
        assert_eq!(
            (
                mouse["CurrentInputMode"].as_u64(),
                mouse["LanguageCode"].as_str()
            ),
            (Some(1), Some("en_US"))
        );
    }
}
