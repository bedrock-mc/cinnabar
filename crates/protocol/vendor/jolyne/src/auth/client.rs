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
    STANDARD.encode(json.to_string().as_bytes())
}

/// Generates a self-signed chain (for Offline Mode) and a ClientData JWT.
/// Returns (identity_chain_json_string, client_data_jwt).
pub fn generate_self_signed_chain(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
) -> Result<(String, String), JolyneError> {
    generate_chain_internal(key, display_name, uuid, None)
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
    let client_token = generate_client_data_token(key, display_name, uuid)?;

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
    generate_chain_internal(key, display_name, uuid, Some(xuid))
}

/// Internal chain generation that handles both self-signed and Xbox Live auth.
fn generate_chain_internal(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    xuid: Option<&str>,
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
    // A minimal 64x64 RGBA skin is 16384 bytes (64*64*4).
    // Create a simple solid-color skin (all white/opaque)
    let skin_pixels = vec![255u8; 64 * 64 * 4];
    let skin_data_b64 = STANDARD.encode(&skin_pixels);

    // Device ID is a UUID
    let device_id = Uuid::new_v4().to_string();

    let client_claims = ClientDataPayload {
        animated_image_data: vec![],
        arm_size: "wide".into(), // Standard Steve arm size
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
        skin_geometry_data: STANDARD.encode(""), // Empty = use default
        skin_geometry_data_engine_version: "".into(),
        skin_id: format!("{}.Custom", uuid),
        skin_image_height: 64,
        skin_image_width: 64,
        skin_resource_patch: generate_skin_resource_patch(),
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

    // A minimal 64x64 RGBA skin is 16384 bytes (64*64*4).
    let skin_pixels = vec![255u8; 64 * 64 * 4];
    let skin_data_b64 = STANDARD.encode(&skin_pixels);
    let device_id = Uuid::new_v4().to_string();

    let client_claims = ClientDataPayload {
        animated_image_data: vec![],
        arm_size: "wide".into(),
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
        skin_geometry_data: STANDARD.encode(""),
        skin_geometry_data_engine_version: "".into(),
        skin_id: format!("{}.Custom", uuid),
        skin_image_height: 64,
        skin_image_width: 64,
        skin_resource_patch: generate_skin_resource_patch(),
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
