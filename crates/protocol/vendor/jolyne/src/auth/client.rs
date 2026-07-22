use crate::error::JolyneError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::SecretKey;
use p384::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Minecraft authentication endpoint for getting Mojang-signed chains
const MINECRAFT_AUTH_URL: &str = "https://multiplayer.minecraft.net/authentication";

/// Response from Minecraft authentication endpoint
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct MinecraftAuthResponse {
    chain: Vec<String>,
}

/// Requests a Mojang-signed authentication chain from Minecraft services.
///
/// This calls the Minecraft authentication endpoint with the XBL token to get
/// a properly signed JWT chain that servers can verify against Mojang's public key.
///
/// # Arguments
/// * `key` - The client's P-384 private key (used for encryption later)
/// * `xbl_token` - The XBL authorization token (from BEDROCK_MULTIPLAYER relying party)
/// * `user_hash` - The user hash for the XBL auth header
///
/// # Returns
/// The Mojang-signed chain as a JSON string: `{"chain": ["...", "...", "..."]}`
pub async fn request_minecraft_chain(
    key: &SecretKey,
    xbl_token: &str,
    user_hash: &str,
) -> Result<String, JolyneError> {
    // Marshal the public key to DER format
    let public_key_der = key
        .public_key()
        .to_public_key_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let public_key_b64 = STANDARD.encode(public_key_der.as_bytes());

    // Build the request body
    let body = serde_json::json!({
        "identityPublicKey": public_key_b64
    });

    // Build the authorization header
    let auth_header = format!("XBL3.0 x={};{}", user_hash, xbl_token);

    // Make the request
    let client = reqwest::Client::new();
    let response = client
        .post(MINECRAFT_AUTH_URL)
        .header("Authorization", auth_header)
        .header("User-Agent", "MCPE/Android")
        .header("Client-Version", crate::valentine::GAME_VERSION)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            JolyneError::Auth(crate::error::AuthError::BadSignature(format!(
                "HTTP error: {}",
                e
            )))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(JolyneError::Auth(crate::error::AuthError::BadSignature(
            format!("Minecraft auth failed ({}): {}", status, body),
        )));
    }

    // Return the raw response as the chain JSON
    let chain_data = response.text().await.map_err(|e| {
        JolyneError::Auth(crate::error::AuthError::BadSignature(format!(
            "Failed to read response: {}",
            e
        )))
    })?;

    Ok(chain_data)
}

#[derive(Serialize)]
struct IdentityClaims {
    #[serde(rename = "extraData")]
    extra_data: IdentityExtraData,
    #[serde(rename = "identityPublicKey")]
    identity_public_key: String,
    nbf: u64,
    exp: u64,
    iat: u64,
    iss: String,
}

#[derive(Serialize)]
struct IdentityExtraData {
    #[serde(rename = "displayName")]
    display_name: String,
    identity: String, // UUID
    #[serde(rename = "XUID")]
    xuid: String,
    #[serde(rename = "titleId")]
    title_id: String,
}

/// Complete ClientData payload with all fields required by BDS.
/// Based on gophertunnel's minecraft/protocol/login/data.go
#[derive(Serialize)]
struct ClientDataPayload {
    // Animation data (empty for simple skins)
    #[serde(rename = "AnimatedImageData")]
    animated_image_data: Vec<()>,
    #[serde(rename = "ArmSize")]
    arm_size: String,
    #[serde(rename = "CapeData")]
    cape_data: String,
    #[serde(rename = "CapeId")]
    cape_id: String,
    #[serde(rename = "CapeImageHeight")]
    cape_image_height: u32,
    #[serde(rename = "CapeImageWidth")]
    cape_image_width: u32,
    #[serde(rename = "CapeOnClassicSkin")]
    cape_on_classic_skin: bool,
    #[serde(rename = "ClientRandomId")]
    client_random_id: i64,
    #[serde(rename = "CompatibleWithClientSideChunkGen")]
    compatible_with_client_side_chunk_gen: bool,
    #[serde(rename = "CurrentInputMode")]
    current_input_mode: u32,
    #[serde(rename = "DefaultInputMode")]
    default_input_mode: u32,
    #[serde(rename = "DeviceId")]
    device_id: String,
    #[serde(rename = "DeviceModel")]
    device_model: String,
    #[serde(rename = "DeviceOS")]
    device_os: u32,
    #[serde(rename = "GameVersion")]
    game_version: String,
    #[serde(rename = "GraphicsMode")]
    graphics_mode: u32,
    #[serde(rename = "GuiScale")]
    gui_scale: i32,
    #[serde(rename = "IsEditorMode")]
    is_editor_mode: bool,
    #[serde(rename = "LanguageCode")]
    language_code: String,
    #[serde(rename = "MaxViewDistance")]
    max_view_distance: u32,
    #[serde(rename = "MemoryTier")]
    memory_tier: u32,
    #[serde(rename = "OverrideSkin")]
    override_skin: bool,
    #[serde(rename = "PersonaPieces")]
    persona_pieces: Vec<()>,
    #[serde(rename = "PersonaSkin")]
    persona_skin: bool,
    #[serde(rename = "PieceTintColors")]
    piece_tint_colors: Vec<()>,
    #[serde(rename = "PlatformOfflineId")]
    platform_offline_id: String,
    #[serde(rename = "PlatformOnlineId")]
    platform_online_id: String,
    #[serde(rename = "PlatformType")]
    platform_type: u32,
    #[serde(rename = "PlayFabId")]
    play_fab_id: String,
    #[serde(rename = "PremiumSkin")]
    premium_skin: bool,
    #[serde(rename = "SelfSignedId")]
    self_signed_id: String,
    #[serde(rename = "ServerAddress")]
    server_address: String,
    #[serde(rename = "SkinAnimationData")]
    skin_animation_data: String,
    #[serde(rename = "SkinColor")]
    skin_color: String,
    #[serde(rename = "SkinData")]
    skin_data: String,
    #[serde(rename = "SkinGeometryData")]
    skin_geometry_data: String,
    #[serde(rename = "SkinGeometryDataEngineVersion")]
    skin_geometry_data_engine_version: String,
    #[serde(rename = "SkinId")]
    skin_id: String,
    #[serde(rename = "SkinImageHeight")]
    skin_image_height: u32,
    #[serde(rename = "SkinImageWidth")]
    skin_image_width: u32,
    #[serde(rename = "SkinResourcePatch")]
    skin_resource_patch: String,
    #[serde(rename = "ThirdPartyName")]
    third_party_name: String,
    #[serde(rename = "ThirdPartyNameOnly")]
    third_party_name_only: bool,
    #[serde(rename = "TrustedSkin")]
    trusted_skin: bool,
    #[serde(rename = "UIProfile")]
    ui_profile: u32,
}

/// Generates a minimal valid skin resource patch JSON.
fn generate_skin_resource_patch() -> String {
    let json = serde_json::json!({
        "geometry": {
            "default": "geometry.humanoid.custom"
        }
    });
    json.to_string()
}

/// Bounded decoded appearance advertised by this client in ClientData.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvertisedSkin {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
    arm_size: String,
    resource_patch: String,
    geometry_data: String,
}

impl AdvertisedSkin {
    pub const fn width(&self) -> u32 {
        self.width
    }
    pub const fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba8(&self) -> &[u8] {
        &self.rgba8
    }
    pub fn arm_size(&self) -> &str {
        &self.arm_size
    }
    pub fn resource_patch(&self) -> &str {
        &self.resource_patch
    }
    pub fn geometry_data(&self) -> &str {
        &self.geometry_data
    }
}

#[must_use]
pub fn default_advertised_skin() -> AdvertisedSkin {
    AdvertisedSkin {
        width: 64,
        height: 64,
        rgba8: cinnabar_default_skin(),
        arm_size: "wide".to_owned(),
        resource_patch: generate_skin_resource_patch(),
        geometry_data: String::new(),
    }
}

/// Independently authored classic 64x64 skin used when no account appearance is available.
/// Only the six base-layer box-UV islands are opaque; Bedrock's optional hat, jacket,
/// sleeves, and trousers remain transparent instead of inflating into solid cuboids.
fn cinnabar_default_skin() -> Vec<u8> {
    const SIDE: usize = 64;
    const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
    const SKIN: [u8; 4] = [198, 126, 84, 255];
    const HAIR: [u8; 4] = [72, 43, 29, 255];
    const EYE: [u8; 4] = [42, 54, 72, 255];
    const SHIRT: [u8; 4] = [148, 45, 51, 255];
    const SHIRT_DARK: [u8; 4] = [104, 31, 38, 255];
    const TROUSERS: [u8; 4] = [45, 55, 76, 255];

    let mut rgba8 = TRANSPARENT.repeat(SIDE * SIDE);
    let mut fill = |x: usize, y: usize, width: usize, height: usize, colour: [u8; 4]| {
        for py in y..y + height {
            for px in x..x + width {
                let offset = (py * SIDE + px) * 4;
                rgba8[offset..offset + 4].copy_from_slice(&colour);
            }
        }
    };

    // Bedrock box UV islands for geometry.humanoid.custom.
    fill(0, 0, 32, 16, SKIN); // head
    fill(16, 16, 24, 16, SHIRT); // body
    fill(40, 16, 16, 16, SKIN); // right arm
    fill(0, 16, 16, 16, TROUSERS); // right leg
    fill(32, 48, 16, 16, SKIN); // left arm
    fill(16, 48, 16, 16, TROUSERS); // left leg

    // Hair framing, face, shirt seams, and shoes keep every visible side readable.
    fill(0, 0, 32, 8, HAIR);
    fill(0, 8, 8, 8, HAIR);
    fill(24, 8, 8, 8, HAIR);
    fill(8, 8, 8, 2, HAIR);
    fill(9, 11, 2, 1, EYE);
    fill(14, 11, 2, 1, EYE);
    fill(11, 14, 3, 1, [151, 72, 61, 255]);
    fill(16, 30, 24, 2, SHIRT_DARK);
    fill(40, 16, 16, 4, SHIRT);
    fill(32, 48, 16, 4, SHIRT);
    fill(0, 28, 16, 4, [30, 35, 50, 255]);
    fill(16, 60, 16, 4, [30, 35, 50, 255]);
    rgba8
}

fn validate_advertised_skin(skin: &AdvertisedSkin) -> Result<(), JolyneError> {
    let valid_dimensions = matches!(
        (skin.width, skin.height),
        (64, 32) | (64, 64) | (128, 128) | (256, 256)
    );
    let expected = usize::try_from(skin.width)
        .ok()
        .and_then(|width| {
            usize::try_from(skin.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    let expected_identifier = match skin.arm_size.as_str() {
        "wide" => "geometry.humanoid.custom",
        "slim" => "geometry.humanoid.customSlim",
        _ => return Err(crate::error::AuthError::InvalidJson.into()),
    };
    let patch: serde_json::Value = serde_json::from_str(&skin.resource_patch)
        .map_err(|_| crate::error::AuthError::InvalidJson)?;
    let identifier = patch
        .get("geometry")
        .and_then(|geometry| geometry.get("default"))
        .and_then(serde_json::Value::as_str);
    if !valid_dimensions
        || expected != Some(skin.rgba8.len())
        || identifier != Some(expected_identifier)
        || !skin.geometry_data.is_empty()
    {
        return Err(crate::error::AuthError::InvalidJson.into());
    }
    Ok(())
}

/// Generates a self-signed chain (for Offline Mode) and a ClientData JWT.
/// Returns (identity_chain_json_string, client_data_jwt).
pub fn generate_self_signed_chain(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
) -> Result<(String, String), JolyneError> {
    generate_self_signed_chain_with_skin(key, display_name, uuid, &default_advertised_skin())
}

pub fn generate_self_signed_chain_with_skin(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    skin: &AdvertisedSkin,
) -> Result<(String, String), JolyneError> {
    validate_advertised_skin(skin)?;
    generate_chain_internal(key, display_name, uuid, None, skin)
}

/// Response structure from Mojang authentication
#[derive(Debug, Deserialize)]
struct MojangChainResponse {
    chain: Vec<String>,
}

/// JWT header for extracting x5u
#[derive(Debug, Deserialize)]
struct JwtHeader {
    x5u: Option<String>,
}

/// Claims for the client's first token that links to Mojang chain
#[derive(Serialize)]
struct LinkingClaims {
    #[serde(rename = "identityPublicKey")]
    identity_public_key: String,
    #[serde(rename = "certificateAuthority")]
    certificate_authority: bool,
    nbf: u64,
    exp: u64,
}

/// Encodes a login request using a Mojang-signed chain.
///
/// This prepends the client's own token to the Mojang chain (like gophertunnel's login.Encode).
/// Returns (identity_chain_json_string, client_data_jwt).
pub fn encode_with_mojang_chain(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    mojang_chain_json: &str,
) -> Result<(String, String), JolyneError> {
    encode_with_mojang_chain_and_skin(
        key,
        display_name,
        uuid,
        mojang_chain_json,
        &default_advertised_skin(),
    )
}

pub fn encode_with_mojang_chain_and_skin(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    mojang_chain_json: &str,
    skin: &AdvertisedSkin,
) -> Result<(String, String), JolyneError> {
    validate_advertised_skin(skin)?;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + 6 * 60 * 60; // 6 hours

    // Parse the Mojang chain
    let mojang_response: MojangChainResponse =
        serde_json::from_str(mojang_chain_json).map_err(|e| {
            JolyneError::Auth(crate::error::AuthError::BadSignature(format!(
                "Failed to parse Mojang chain: {}",
                e
            )))
        })?;

    if mojang_response.chain.is_empty() {
        return Err(JolyneError::Auth(crate::error::AuthError::BadSignature(
            "Mojang chain is empty".to_string(),
        )));
    }

    // Get the x5u from the first token in the Mojang chain
    // The first token's header contains the public key we need to link to
    let first_token = &mojang_response.chain[0];
    let parts: Vec<&str> = first_token.split('.').collect();
    if parts.len() != 3 {
        return Err(JolyneError::Auth(crate::error::AuthError::BadSignature(
            "Invalid JWT format in Mojang chain".to_string(),
        )));
    }

    let header_json = URL_SAFE_NO_PAD.decode(parts[0]).map_err(|e| {
        JolyneError::Auth(crate::error::AuthError::BadSignature(format!(
            "Failed to decode JWT header: {}",
            e
        )))
    })?;
    let header: JwtHeader = serde_json::from_slice(&header_json).map_err(|e| {
        JolyneError::Auth(crate::error::AuthError::BadSignature(format!(
            "Failed to parse JWT header: {}",
            e
        )))
    })?;

    let mojang_public_key = header.x5u.ok_or_else(|| {
        JolyneError::Auth(crate::error::AuthError::BadSignature(
            "Missing x5u in Mojang chain header".to_string(),
        ))
    })?;

    // Prepare our keys
    let public_key_der = key
        .public_key()
        .to_public_key_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let public_key_b64 = STANDARD.encode(public_key_der.as_bytes());

    let private_der = key
        .to_pkcs8_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let encoding_key = EncodingKey::from_ec_der(private_der.as_bytes());

    // Create the linking token that connects our key to the Mojang chain
    let linking_claims = LinkingClaims {
        identity_public_key: mojang_public_key,
        certificate_authority: true,
        nbf: now - 6 * 60 * 60,
        exp,
    };

    let mut header = Header::new(Algorithm::ES384);
    header.x5u = Some(public_key_b64.clone());

    let linking_jwt = encode(&header, &linking_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    // Build the complete chain: our token + Mojang chain
    let mut complete_chain = vec![linking_jwt];
    complete_chain.extend(mojang_response.chain);

    // Inner chain JSON (just the chain array)
    let inner_chain = serde_json::json!({
        "chain": complete_chain
    })
    .to_string();

    // Outer request JSON (Certificate is a string containing the chain JSON)
    // This matches gophertunnel's format for Xbox Live authenticated logins
    let chain_json = serde_json::json!({
        "Certificate": inner_chain,
        "AuthenticationType": 2,
        "Token": ""
    })
    .to_string();

    // Generate client data token
    let client_token = generate_client_data_token(key, display_name, uuid, skin)?;

    Ok((chain_json, client_token))
}

/// Generates an Xbox Live authenticated chain and a ClientData JWT.
/// The XUID comes from the BEDROCK_MULTIPLAYER XBL token.
/// Returns (identity_chain_json_string, client_data_jwt).
#[allow(dead_code)]
pub fn generate_xbox_live_chain(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    xuid: &str,
) -> Result<(String, String), JolyneError> {
    generate_chain_internal(
        key,
        display_name,
        uuid,
        Some(xuid),
        &default_advertised_skin(),
    )
}

/// Internal chain generation that handles both self-signed and Xbox Live auth.
fn generate_chain_internal(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    xuid: Option<&str>,
    skin: &AdvertisedSkin,
) -> Result<(String, String), JolyneError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + 24 * 60 * 60; // 24h

    let public_key_der = key
        .public_key()
        .to_public_key_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let public_key_b64 = STANDARD.encode(public_key_der.as_bytes());

    let private_der = key
        .to_pkcs8_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let encoding_key = EncodingKey::from_ec_der(private_der.as_bytes());

    // Determine XUID - use provided XUID if available (Xbox Live auth), empty otherwise (self-signed)
    let xuid_value = xuid.unwrap_or("").to_string();

    // 1. Identity Token
    let identity_claims = IdentityClaims {
        extra_data: IdentityExtraData {
            display_name: display_name.to_string(),
            identity: uuid.to_string(),
            xuid: xuid_value,
            title_id: "896928775".to_string(), // Win10
        },
        identity_public_key: public_key_b64.clone(),
        nbf: now - 1,
        exp,
        iat: now,
        iss: "self".to_string(),
    };

    let mut header = Header::new(Algorithm::ES384);
    header.x5u = Some(public_key_b64); // Self-signed: x5u is self

    let identity_jwt = encode(&header, &identity_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    // Chain JSON
    let chain_json = serde_json::json!({
        "chain": [identity_jwt],
        "AuthenticationType": 2
    })
    .to_string();

    // 2. ClientData Token
    let skin_data_b64 = STANDARD.encode(&skin.rgba8);

    // Device ID is a UUID
    let device_id = Uuid::new_v4().to_string();

    let client_claims = ClientDataPayload {
        animated_image_data: vec![],
        arm_size: skin.arm_size.clone(),
        cape_data: "".into(),
        cape_id: "".into(),
        cape_image_height: 0,
        cape_image_width: 0,
        cape_on_classic_skin: false,
        client_random_id: (rand::random::<u64>() & 0x7FFFFFFFFFFFFFFF) as i64,
        compatible_with_client_side_chunk_gen: true,
        current_input_mode: 1, // Mouse/Keyboard
        default_input_mode: 1,
        device_id,
        device_model: "JolyneClient".into(),
        device_os: 7, // Win10
        game_version: crate::valentine::GAME_VERSION.into(),
        graphics_mode: 0,
        gui_scale: 0,
        is_editor_mode: false,
        language_code: "en_US".into(),
        max_view_distance: 32,
        memory_tier: 5, // Super High
        override_skin: false,
        persona_pieces: vec![],
        persona_skin: false,
        piece_tint_colors: vec![],
        platform_offline_id: "".into(),
        platform_online_id: "".into(),
        platform_type: 0,
        play_fab_id: "".into(),
        premium_skin: false,
        self_signed_id: uuid.to_string(),
        server_address: "".into(),
        skin_animation_data: "".into(),
        skin_color: "#b37b62".into(), // Default Steve skin color
        skin_data: skin_data_b64,
        skin_geometry_data: STANDARD.encode(&skin.geometry_data),
        skin_geometry_data_engine_version: "".into(),
        skin_id: format!("{}.Custom", uuid),
        skin_image_height: skin.height,
        skin_image_width: skin.width,
        skin_resource_patch: STANDARD.encode(&skin.resource_patch),
        third_party_name: display_name.into(),
        third_party_name_only: false,
        trusted_skin: false,
        ui_profile: 0,
    };

    // ClientData JWT - uses x5u to specify the signing key
    let mut client_header = Header::new(Algorithm::ES384);
    client_header.x5u = Some(
        STANDARD.encode(
            key.public_key()
                .to_public_key_der()
                .map_err(|e| {
                    JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string()))
                })?
                .as_bytes(),
        ),
    );

    let client_jwt = encode(&client_header, &client_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    Ok((chain_json, client_jwt))
}

/// Generates just the client data token (used with Mojang chain).
fn generate_client_data_token(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    skin: &AdvertisedSkin,
) -> Result<String, JolyneError> {
    let public_key_der = key
        .public_key()
        .to_public_key_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let public_key_b64 = STANDARD.encode(public_key_der.as_bytes());

    let private_der = key
        .to_pkcs8_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let encoding_key = EncodingKey::from_ec_der(private_der.as_bytes());

    let skin_data_b64 = STANDARD.encode(&skin.rgba8);
    let device_id = Uuid::new_v4().to_string();

    let client_claims = ClientDataPayload {
        animated_image_data: vec![],
        arm_size: skin.arm_size.clone(),
        cape_data: "".into(),
        cape_id: "".into(),
        cape_image_height: 0,
        cape_image_width: 0,
        cape_on_classic_skin: false,
        client_random_id: (rand::random::<u64>() & 0x7FFFFFFFFFFFFFFF) as i64,
        compatible_with_client_side_chunk_gen: true,
        current_input_mode: 1,
        default_input_mode: 1,
        device_id,
        device_model: "JolyneClient".into(),
        device_os: 1, // Android (for Xbox Live auth)
        game_version: crate::valentine::GAME_VERSION.into(),
        graphics_mode: 0,
        gui_scale: 0,
        is_editor_mode: false,
        language_code: "en_US".into(),
        max_view_distance: 32,
        memory_tier: 5,
        override_skin: false,
        persona_pieces: vec![],
        persona_skin: false,
        piece_tint_colors: vec![],
        platform_offline_id: "".into(),
        platform_online_id: "".into(),
        platform_type: 0,
        play_fab_id: "".into(),
        premium_skin: false,
        self_signed_id: uuid.to_string(),
        server_address: "".into(),
        skin_animation_data: "".into(),
        skin_color: "#b37b62".into(),
        skin_data: skin_data_b64,
        skin_geometry_data: STANDARD.encode(&skin.geometry_data),
        skin_geometry_data_engine_version: "".into(),
        skin_id: format!("{}.Custom", uuid),
        skin_image_height: skin.height,
        skin_image_width: skin.width,
        skin_resource_patch: STANDARD.encode(&skin.resource_patch),
        third_party_name: display_name.into(),
        third_party_name_only: false,
        trusted_skin: false,
        ui_profile: 0,
    };

    let mut client_header = Header::new(Algorithm::ES384);
    client_header.x5u = Some(public_key_b64);

    let client_jwt = encode(&client_header, &client_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    Ok(client_jwt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn assert_token_matches(token: &str, skin: &AdvertisedSkin) {
        let payload = token.split('.').nth(1).expect("JWT payload");
        let decoded = URL_SAFE_NO_PAD.decode(payload).expect("base64url payload");
        let claims: serde_json::Value = serde_json::from_slice(&decoded).expect("claims JSON");
        assert_eq!(claims["SkinImageWidth"], skin.width);
        assert_eq!(claims["SkinImageHeight"], skin.height);
        assert_eq!(claims["ArmSize"], skin.arm_size);
        assert_eq!(
            STANDARD
                .decode(claims["SkinData"].as_str().unwrap())
                .unwrap(),
            skin.rgba8
        );
        assert_eq!(
            STANDARD
                .decode(claims["SkinResourcePatch"].as_str().unwrap())
                .unwrap(),
            skin.resource_patch.as_bytes()
        );
        assert_eq!(
            STANDARD
                .decode(claims["SkinGeometryData"].as_str().unwrap())
                .unwrap(),
            skin.geometry_data.as_bytes()
        );
    }

    #[test]
    fn both_client_data_token_paths_encode_the_retained_decoded_skin_exactly() {
        let key = SecretKey::random(&mut rand::thread_rng());
        let uuid = Uuid::new_v4();
        let skin = default_advertised_skin();
        let (_, offline) =
            generate_self_signed_chain_with_skin(&key, "Cinnabar", uuid, &skin).unwrap();
        let authenticated = generate_client_data_token(&key, "Cinnabar", uuid, &skin).unwrap();
        assert_token_matches(&offline, &skin);
        assert_token_matches(&authenticated, &skin);
    }

    #[test]
    fn default_skin_is_a_recognizable_multicolour_wide_player() {
        let skin = default_advertised_skin();
        let pixel = |x: usize, y: usize| -> [u8; 4] {
            skin.rgba8[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
                .try_into()
                .unwrap()
        };

        assert_eq!(pixel(9, 11), [42, 54, 72, 255], "left eye");
        assert_eq!(pixel(14, 11), [42, 54, 72, 255], "right eye");
        assert_eq!(pixel(10, 12), [198, 126, 84, 255], "face skin tone");
        assert_eq!(pixel(20, 20), [148, 45, 51, 255], "shirt front");
        assert_eq!(pixel(4, 20), [45, 55, 76, 255], "right trouser leg");
        assert_eq!(pixel(20, 52), [45, 55, 76, 255], "left trouser leg");
        assert_eq!(
            pixel(32, 0),
            [0, 0, 0, 0],
            "unused overlay remains transparent"
        );
    }
}
