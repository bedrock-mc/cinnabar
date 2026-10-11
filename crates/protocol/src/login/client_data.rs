//! The Bedrock login client-data claims the core presents upstream for this client. The core claims
//! the device (`DeviceOS`, `DeviceModel`, `DeviceId`, `DefaultInputMode`) to match its sign-in.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::BytesMut;
use jolyne::auth::client::{NON_EDITOR_CONNECTION_INTENT, SUPER_HIGH_MEMORY_TIER};
use serde_json::{Value, json};
use std::num::NonZeroI64;
use uuid::Uuid;
use valentine::bedrock::codec::{BedrockCodec, VarUInt};
use valentine::bedrock::version::v1_26_51::EnumsInputMode;

use crate::{ClientSkin, GAME_VERSION, PlayerInputMode};

/// An opaque persisted login identifier, independent of account, device and transport identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientRandomId(NonZeroI64);

impl ClientRandomId {
    /// Accepts every signed 64-bit value except the uninitialized storage sentinel zero.
    pub const fn new(value: i64) -> Option<Self> {
        match NonZeroI64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the signed value serialized in the login claims without masking any bits.
    pub const fn get(self) -> i64 {
        self.0.get()
    }
}

/// The player's own settings a login reports, as vanilla's client takes them at join.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginSettings {
    /// The active UI language, in the `en_US` form of its lang file.
    pub language_code: String,
    /// The input the player is using now.
    pub input_mode: PlayerInputMode,
    /// The GUI scale offset from the video settings.
    pub gui_scale_offset: i8,
    /// Vanilla's device memory tier, from [`login_memory_tier`].
    pub memory_tier: u32,
    /// The graphics option: 0 simple, 1 fancy.
    pub graphics_mode: u8,
    /// Trusted local archive directory, never sent in the upstream login claims.
    pub resource_pack_cache_dir: Option<std::path::PathBuf>,
}

impl Default for LoginSettings {
    fn default() -> Self {
        Self {
            language_code: "en_US".to_owned(),
            input_mode: PlayerInputMode::Mouse,
            gui_scale_offset: 0,
            memory_tier: SUPER_HIGH_MEMORY_TIER,
            graphics_mode: 1,
            resource_pack_cache_dir: None,
        }
    }
}

/// Returns vanilla's login memory tier for physical RAM: 0 up to 1.5 GiB, then 2, 4 and 8 GiB
/// boundaries up to 4; unknown memory (`0`) reports the lowest tier.
pub fn login_memory_tier(physical_bytes: u64) -> u32 {
    const GIB: u64 = 1 << 30;
    [GIB * 3 / 2, 2 * GIB, 4 * GIB, 8 * GIB]
        .into_iter()
        .filter(|limit| physical_bytes > *limit)
        .count() as u32
}

/// Builds claims with the caller's stored ID and player settings; `None` uploads the
/// solid-white 64x64 placeholder skin.
pub(crate) fn login_client_data(
    display_name: &str,
    client_random_id: ClientRandomId,
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
        "ClientRandomId": client_random_id.get(),
        "ClientEditorConnectionIntent": NON_EDITOR_CONNECTION_INTENT,
        "ClientIsEditorCapable": false,
        "CompatibleWithClientSideChunkGen": false,
        "CurrentInputMode": input_mode_value(settings.input_mode),
        "GameVersion": GAME_VERSION,
        "GraphicsMode": settings.graphics_mode,
        "GuiScale": settings.gui_scale_offset,
        "LanguageCode": settings.language_code,
        "MaxViewDistance": render_api::MAX_VIEW_RADIUS_CHUNKS,
        "MemoryTier": settings.memory_tier,
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

    /// Repeated claims and bridge JSON retain all signed bits of the caller's ID.
    #[test]
    fn claims_reuse_the_supplied_signed_client_id() {
        assert!(ClientRandomId::new(0).is_none());
        for value in [i64::MIN, i64::MAX, -1, 1] {
            let id = ClientRandomId::new(value).unwrap();
            for name in ["First", "Second"] {
                let claims = login_client_data(name, id, None, &LoginSettings::default());
                let request = crate::session_wire::ConnectRequest {
                    protocol: crate::PROTOCOL_VERSION,
                    target: None,
                    client_cache: false,
                    client_data: claims,
                };
                let encoded = crate::session_wire::encode_connect(&request).unwrap();
                let decoded: serde_json::Value = serde_json::from_slice(&encoded[1..]).unwrap();
                assert_eq!(
                    decoded["client_data"]["ClientRandomId"].as_i64(),
                    Some(value)
                );
                assert_eq!(decoded["client_data"]["ThirdPartyName"], name);
            }
        }
    }

    fn decoded(claims: &Value, field: &str) -> Vec<u8> {
        STANDARD
            .decode(claims[field].as_str().expect("base64 string field"))
            .expect("valid base64")
    }

    /// Session serialization explicitly declines chunk generation and retains player data.
    #[test]
    fn session_claims_decline_chunk_generation_and_preserve_player_data() {
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
        let settings = LoginSettings {
            language_code: "de_DE".into(),
            input_mode: PlayerInputMode::GamePad,
            gui_scale_offset: -1,
            ..Default::default()
        };
        let request = crate::session_wire::ConnectRequest {
            protocol: crate::PROTOCOL_VERSION,
            target: None,
            client_cache: false,
            client_data: login_client_data(
                "Steve",
                ClientRandomId::new(-7).unwrap(),
                Some(&skin),
                &settings,
            ),
        };
        let encoded = crate::session_wire::encode_connect(&request).unwrap();
        let received = crate::session_wire::decode_connect(&encoded).unwrap();
        assert_eq!(received, request);
        let claims = received.client_data;
        assert_eq!(
            claims["CompatibleWithClientSideChunkGen"].as_bool(),
            Some(false)
        );
        assert_eq!(claims["ClientRandomId"].as_i64(), Some(-7));
        assert_eq!(claims["ThirdPartyName"], "Steve");
        assert_eq!(claims["GameVersion"], GAME_VERSION);
        assert_eq!(claims["LanguageCode"], settings.language_code);
        assert_eq!(claims["GuiScale"], settings.gui_scale_offset);
        assert_eq!(
            claims["CurrentInputMode"],
            input_mode_value(settings.input_mode)
        );
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
        let claims = login_client_data(
            "Alex",
            ClientRandomId::new(-7).unwrap(),
            None,
            &LoginSettings::default(),
        );
        assert_eq!(
            claims["CompatibleWithClientSideChunkGen"].as_bool(),
            Some(false)
        );
        assert_eq!(decoded(&claims, "SkinData").len(), 64 * 64 * 4);
        let patch: Value = serde_json::from_slice(&decoded(&claims, "SkinResourcePatch")).unwrap();
        assert_eq!(patch["geometry"]["default"], "geometry.humanoid.custom");
        assert_eq!(claims["CapeData"], "");
        assert_eq!(claims["CapeOnClassicSkin"], false);
    }

    /// Login validation requires a supported memory tier and explicit gameplay editor claims.
    #[test]
    fn session_and_self_signed_logins_report_supported_client_capabilities() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use jolyne::auth::client::generate_self_signed_chain;

        let settings = LoginSettings::default();
        let skin = ClientSkin {
            rgba8: vec![255; 64 * 64 * 4],
            width: 64,
            height: 64,
            arm_size: "wide".into(),
            cape: None,
            geometry: None,
        };
        let key = p384::SecretKey::random(&mut p384::elliptic_curve::rand_core::OsRng);
        for skin in [None, Some(&skin)] {
            let session_claims =
                login_client_data("Fixture", ClientRandomId::new(-7).unwrap(), skin, &settings);
            let (_, client_jwt) =
                generate_self_signed_chain(&key, "Fixture", Uuid::new_v4(), skin).unwrap();
            let jwt_claims: Value = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(client_jwt.split('.').nth(1).unwrap())
                    .unwrap(),
            )
            .unwrap();
            for claims in [session_claims, jwt_claims] {
                let tier = claims["MemoryTier"]
                    .as_u64()
                    .expect("an unsigned enum value");
                assert!(
                    tier <= u64::from(SUPER_HIGH_MEMORY_TIER),
                    "unsupported memory tier {tier} rejects otherwise valid login claims"
                );
                assert_eq!(claims["ClientIsEditorCapable"], false);
                assert_eq!(
                    claims["ClientEditorConnectionIntent"],
                    NON_EDITOR_CONNECTION_INTENT
                );
            }
        }
    }

    // Each tier boundary is exclusive: exactly 1.5, 2, 4 and 8 GiB stay in the lower tier.
    #[test]
    fn login_memory_tier_matches_vanilla_boundaries() {
        const GIB: u64 = 1 << 30;
        for (bytes, tier) in [
            (0, 0),
            (GIB * 3 / 2, 0),
            (GIB * 3 / 2 + 1, 1),
            (2 * GIB, 1),
            (2 * GIB + 1, 2),
            (4 * GIB, 2),
            (4 * GIB + 1, 3),
            (8 * GIB, 3),
            (8 * GIB + 1, 4),
            (64 * GIB, 4),
        ] {
            assert_eq!(login_memory_tier(bytes), tier, "{bytes} bytes");
        }
    }

    // The claims report the player's graphics option and device memory tier, not constants.
    #[test]
    fn claims_carry_the_graphics_option_and_memory_tier() {
        let settings = LoginSettings {
            memory_tier: 2,
            graphics_mode: 0,
            ..Default::default()
        };
        let claims = login_client_data("Steve", ClientRandomId::new(1).unwrap(), None, &settings);
        assert_eq!(claims["MemoryTier"], 2);
        assert_eq!(claims["GraphicsMode"], 0);
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
        let claims = login_client_data("Steve", ClientRandomId::new(-7).unwrap(), None, &settings);
        assert_eq!(claims["GameVersion"], GAME_VERSION);
        assert_eq!(claims["ThirdPartyName"], "Steve");
        assert_eq!(claims["LanguageCode"], "de_DE");
        assert_eq!(
            claims["CurrentInputMode"], 3,
            "vanilla's gamepad input mode"
        );
        assert_eq!(claims["GuiScale"], -1);
        assert_eq!(
            claims["MaxViewDistance"],
            render_api::MAX_VIEW_RADIUS_CHUNKS
        );
        assert_eq!(claims["MemoryTier"], SUPER_HIGH_MEMORY_TIER);
        assert_eq!(claims["GraphicsMode"], 1);
        for device_field in ["DeviceOS", "DeviceModel", "DeviceId", "DefaultInputMode"] {
            assert!(
                claims.get(device_field).is_none(),
                "{device_field} is the core's"
            );
        }
        assert!(claims["SkinId"].as_str().unwrap().ends_with(".Custom"));
        assert!(Uuid::parse_str(claims["SelfSignedId"].as_str().unwrap()).is_ok());
        let mouse = login_client_data(
            "Steve",
            ClientRandomId::new(-7).unwrap(),
            None,
            &LoginSettings::default(),
        );
        assert_eq!(
            (
                mouse["CurrentInputMode"].as_u64(),
                mouse["LanguageCode"].as_str()
            ),
            (Some(1), Some("en_US"))
        );
    }
}
