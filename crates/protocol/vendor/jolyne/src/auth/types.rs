use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationType {
    Full = 0,
    Guest = 1,
    SelfSigned = 2,
}

impl TryFrom<u32> for AuthenticationType {
    type Error = crate::error::AuthError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(AuthenticationType::Full),
            1 => Ok(AuthenticationType::Guest),
            2 => Ok(AuthenticationType::SelfSigned),
            other => Err(crate::error::AuthError::UnsupportedAuthType(other)),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthInfo {
    #[serde(rename = "AuthenticationType")]
    pub authentication_type: Option<u32>,
    #[serde(rename = "Token")]
    pub token: Option<String>,
    #[serde(rename = "Certificate")]
    pub certificate: Option<serde_json::Value>,
    // Some clients may still send legacy "chain" shape in root.
    pub chain: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExtraDataClaims {
    #[serde(rename = "XUID")]
    pub xuid: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "identity")]
    pub uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OpenIdClaims {
    // Newer Bedrock OpenID tokens carry the authenticated XUID and gamertag directly.
    #[serde(rename = "xid")]
    pub xuid: Option<String>,
    #[serde(rename = "xname")]
    pub xbox_username: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "uhs")]
    pub user_hash: Option<String>,
    // Some recent clients send the client public key as "cpk" instead of identityPublicKey.
    #[serde(rename = "identityPublicKey", alias = "cpk")]
    pub identity_public_key: Option<String>,
    #[serde(rename = "extraData")]
    pub extra_data: Option<ExtraDataClaims>,
    #[allow(dead_code)]
    pub iss: Option<String>,
    #[allow(dead_code)]
    pub aud: Option<String>,
    #[allow(dead_code)]
    pub exp: Option<u64>,
    #[allow(dead_code)]
    pub nbf: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClientDataClaims {
    // Some client builds send lower-case identityPublicKey; accept both.
    #[serde(rename = "IdentityPublicKey", alias = "identityPublicKey")]
    pub identity_public_key: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "ThirdPartyName")]
    pub third_party_name: Option<String>,
    #[serde(rename = "XUID")]
    pub xuid: Option<String>,
    #[serde(rename = "identity")]
    pub uuid: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ChainClaims {
    #[serde(rename = "identityPublicKey")]
    pub identity_public_key: Option<String>,
    #[serde(rename = "extraData")]
    pub extra_data: Option<ExtraDataClaims>,
    // Standard JWT fields checked by jsonwebtoken Validation
    #[allow(dead_code)]
    pub iss: Option<String>,
    #[allow(dead_code)]
    pub aud: Option<String>,
    #[allow(dead_code)]
    pub exp: Option<u64>,
    #[allow(dead_code)]
    pub nbf: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ValidatedIdentity {
    pub xuid: Option<String>,
    pub display_name: Option<String>,
    pub identity_public_key: String, // The client's public key for encryption
    pub uuid: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct ChainOnly {
    pub chain: Option<Vec<String>>,
    pub certificate: Option<serde_json::Value>,
}
