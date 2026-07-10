use crate::auth::types::{ClientDataClaims, OpenIdClaims, ValidatedIdentity};
use crate::auth::util::{decode_unverified_claims, key_from_base64_for_alg};
use crate::error::{AuthError, JolyneError};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::{debug, warn};

const KEY_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60); // 30 minutes, aligned with PMMP
const AUTHORIZATION_SERVICE_URI_FALLBACK: &str =
    "https://authorization.franchise.minecraft-services.net";
// Best-effort discovery target; if this fails we fall back to the static auth service.
const MINECRAFT_VERSION_NETWORK: &str = crate::valentine::GAME_VERSION;
const MINECRAFT_SERVICES_DISCOVERY_URL: &str =
    "https://client.discovery.minecraft-services.net/api/v1.0/discovery/MinecraftPE/builds/";
// Cache discovery responses to avoid repeated calls.
const DISCOVERY_TTL: Duration = Duration::from_secs(6 * 60 * 60); // 6h

#[derive(Debug, Deserialize)]
struct DiscoveryAuthService {
    #[serde(rename = "serviceUri")]
    service_uri: String,
}

#[derive(Debug, Deserialize)]
struct DiscoveryServiceEnvironments {
    auth: DiscoveryAuthEnvironmentVariants,
}

#[derive(Debug, Deserialize)]
struct DiscoveryAuthEnvironmentVariants {
    prod: DiscoveryAuthService,
}

#[derive(Debug, Deserialize)]
struct DiscoveryResult {
    #[serde(rename = "serviceEnvironments")]
    service_environments: DiscoveryServiceEnvironments,
}

#[derive(Debug, Deserialize)]
struct DiscoveryEnvelope {
    result: DiscoveryResult,
}

const MOJANG_AUDIENCE: &str = "api://auth-minecraft-services/multiplayer";
const CLOCK_DRIFT_MAX: u64 = 60;

#[derive(Debug, Deserialize)]
struct OpenIdConfiguration {
    issuer: String,
    #[serde(rename = "jwks_uri")]
    jwks_uri: String,
}

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kid: String,
    #[serde(rename = "use")]
    use_field: Option<String>,
    kty: String,
    n: String,
    e: String,
}

#[derive(Clone)]
struct AuthKeyring {
    #[allow(dead_code)]
    issuer: String,
    keys: HashMap<String, DecodingKey>,
}

impl AuthKeyring {
    fn get(&self, kid: &str) -> Option<DecodingKey> {
        self.keys.get(kid).cloned()
    }
}

struct AuthKeyProvider {
    keyring: Option<AuthKeyring>,
    last_fetch: Instant,
    refresh_interval: Duration,
}

impl AuthKeyProvider {
    fn new(refresh_interval: Duration) -> Self {
        Self {
            keyring: None,
            last_fetch: Instant::now() - refresh_interval, // force first fetch
            refresh_interval,
        }
    }

    async fn resolve_key(
        &mut self,
        kid: &str,
    ) -> Result<Option<(DecodingKey, String)>, JolyneError> {
        // Cache hit and still fresh.
        if let Some(kr) = &self.keyring
            && let Some(k) = kr.get(kid)
        {
            return Ok(Some((k, kr.issuer.clone())));
        }

        // Only refresh if we don't know the key or cache is stale.
        if self.last_fetch.elapsed() >= self.refresh_interval || self.keyring.is_none() {
            match fetch_auth_keys().await {
                Ok(kr) => {
                    self.last_fetch = Instant::now();
                    // Update cache even if key isn't there; we still tried.
                    self.keyring = Some(kr);
                }
                Err(err) => {
                    warn!(error = ?err, "failed to refresh auth keys; falling back to previous cache");
                }
            }
        }

        Ok(self
            .keyring
            .as_ref()
            .and_then(|kr| kr.get(kid).map(|k| (k, kr.issuer.clone()))))
    }
}

static AUTH_KEY_PROVIDER: Lazy<Mutex<AuthKeyProvider>> =
    Lazy::new(|| Mutex::new(AuthKeyProvider::new(KEY_REFRESH_INTERVAL)));

static DISCOVERY_CACHE: Lazy<Mutex<Option<(Instant, String)>>> = Lazy::new(|| Mutex::new(None));

fn key_from_header(header: &jsonwebtoken::Header, alg: Algorithm) -> Option<DecodingKey> {
    if let Some(x5u) = header.x5u.as_ref()
        && let Ok(k) = key_from_base64_for_alg(x5u, alg)
    {
        return Some(k);
    }
    if let Some(x5c) = header.x5c.as_ref().and_then(|v| v.first())
        && let Ok(k) = key_from_base64_for_alg(x5c, alg)
    {
        return Some(k);
    }
    None
}

fn decode_client_data_claims(client_data_jwt: &str) -> Option<ClientDataClaims> {
    decode_unverified_claims::<ClientDataClaims>(client_data_jwt)
}

pub(crate) fn fill_identity_from_client_data(
    mut identity: ValidatedIdentity,
    client_data_jwt: &str,
) -> ValidatedIdentity {
    if let Some(cd) = decode_client_data_claims(client_data_jwt) {
        if identity.identity_public_key.is_empty()
            && let Some(pk) = cd.identity_public_key
        {
            identity.identity_public_key = pk;
        }
        if identity.display_name.is_none() {
            identity.display_name = cd.display_name.or(cd.third_party_name);
        }
        if identity.xuid.is_none() {
            identity.xuid = cd.xuid;
        }
        if identity.uuid.is_none() {
            identity.uuid = cd.uuid;
        }
    }
    identity
}

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("jolyne-auth/0.1")
        .build()
        .expect("reqwest client")
});

async fn fetch_auth_keys() -> Result<AuthKeyring, JolyneError> {
    async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, JolyneError> {
        let resp = HTTP_CLIENT
            .get(url)
            .send()
            .await
            .map_err(|e| AuthError::BadSignature(format!("HTTP error fetching {url}: {e}")))?;
        let resp = resp
            .error_for_status()
            .map_err(|e| AuthError::BadSignature(format!("Unexpected HTTP fetching {url}: {e}")))?;
        resp.json::<T>()
            .await
            .map_err(|e| AuthError::BadSignature(format!("Invalid JSON from {url}: {e}")).into())
    }

    async fn discover_auth_service_uri() -> String {
        let mut cache = DISCOVERY_CACHE.lock().await;
        if let Some((ts, uri)) = cache.as_ref()
            && ts.elapsed() < DISCOVERY_TTL
        {
            return uri.clone();
        }
        let discovery_url =
            format!("{MINECRAFT_SERVICES_DISCOVERY_URL}{MINECRAFT_VERSION_NETWORK}");
        if let Ok(env) = get_json::<DiscoveryEnvelope>(&discovery_url).await {
            let uri = env.result.service_environments.auth.prod.service_uri;
            debug!(discovered_auth_service = %uri, "discovery succeeded");
            *cache = Some((Instant::now(), uri.clone()));
            return uri;
        }
        let uri = AUTHORIZATION_SERVICE_URI_FALLBACK.to_string();
        *cache = Some((Instant::now(), uri.clone()));
        uri
    }

    // Step 1: discover auth service URI (best-effort, cached)
    let auth_service_uri = discover_auth_service_uri().await;

    // Step 2: fetch OpenID configuration (or fallback to default keys path)
    let openid_config_url = format!("{auth_service_uri}/.well-known/openid-configuration");
    let (issuer, jwks_uri) = match get_json::<OpenIdConfiguration>(&openid_config_url).await {
        Ok(cfg) => {
            debug!(issuer = %cfg.issuer, jwks_uri = %cfg.jwks_uri, "openid configuration succeeded");
            (cfg.issuer, cfg.jwks_uri)
        }
        Err(err) => {
            warn!(error = ?err, "openid configuration failed; falling back to keys endpoint");
            (
                auth_service_uri.clone(),
                format!("{auth_service_uri}/.well-known/keys"),
            )
        }
    };

    // Step 3: fetch JWKS
    let jwks: Jwks = get_json(&jwks_uri).await?;

    let mut keys = HashMap::new();
    for key in jwks.keys {
        if key.use_field.as_deref() != Some("sig") || key.kty != "RSA" {
            debug!(
                kid = %key.kid,
                use_field = ?key.use_field,
                kty = %key.kty,
                "skipping non-signing or non-RSA key"
            );
            continue;
        }
        let decoding = DecodingKey::from_rsa_components(&key.n, &key.e).map_err(|e| {
            AuthError::BadSignature(format!("Invalid RSA components for {}: {e}", key.kid))
        })?;
        keys.insert(key.kid, decoding);
    }

    if keys.is_empty() {
        return Err(AuthError::BadSignature("No usable JWKS keys".to_string()).into());
    }

    debug!(keys = %keys.len(), issuer = %issuer, "loaded JWKS keys");

    Ok(AuthKeyring { issuer, keys })
}

pub async fn validate_open_id(
    token: &str,
    client_data_jwt: &str,
    online_mode: bool,
) -> Result<ValidatedIdentity, JolyneError> {
    let client_claims = decode_client_data_claims(client_data_jwt);
    let header = decode_header(token).map_err(|e| AuthError::InvalidHeader(e.to_string()))?;
    let alg = match header.alg {
        Algorithm::ES256 | Algorithm::ES384 | Algorithm::RS256 => header.alg,
        other => return Err(AuthError::UnsupportedAlg(format!("{other:?}")).into()),
    };
    let header_key_b64 = header
        .x5u
        .clone()
        .or_else(|| header.x5c.as_ref().and_then(|v| v.first().cloned()));
    let mut provider_key: Option<(DecodingKey, String)> = None;
    if online_mode
        && header.kid.is_some()
        && header_key_b64.is_none()
        && let Some(kid) = header.kid.as_deref()
    {
        let mut provider = AUTH_KEY_PROVIDER.lock().await;
        provider_key = provider.resolve_key(kid).await?;
    }

    let decoded = if online_mode {
        let issuer_for_validation = provider_key.as_ref().map(|(_, iss)| iss.clone());
        let key_opt =
            key_from_header(&header, alg).or_else(|| provider_key.as_ref().map(|(k, _)| k.clone()));
        if let Some(key) = key_opt {
            let mut validation = Validation::new(alg);
            validation.validate_exp = true;
            validation.validate_nbf = true;
            validation.leeway = CLOCK_DRIFT_MAX;
            validation.validate_aud = true;
            validation.set_audience(&[MOJANG_AUDIENCE]);
            if let Some(iss) = issuer_for_validation {
                validation.set_issuer(&[iss]);
            }
            match decode::<OpenIdClaims>(token, &key, &validation) {
                Ok(data) => data,
                Err(e) => {
                    let is_time = matches!(
                        e.kind(),
                        jsonwebtoken::errors::ErrorKind::ExpiredSignature
                            | jsonwebtoken::errors::ErrorKind::ImmatureSignature
                    );
                    let msg = if is_time {
                        AuthError::TemporalValidation
                    } else {
                        AuthError::BadSignature(e.to_string())
                    };
                    warn!(
                        error = %msg,
                        kid = ?header.kid,
                        alg = ?header.alg,
                        has_x5u = header.x5u.is_some(),
                        has_x5c = header.x5c.is_some(),
                        used_provider_key = provider_key.is_some(),
                        "openid decode failed"
                    );
                    return Err(msg.into());
                }
            }
        } else {
            return Err(AuthError::MissingIdentityKey.into());
        }
    } else {
        let claims = decode_unverified_claims::<OpenIdClaims>(token)
            .ok_or(AuthError::BadSignature("Invalid JWT format".to_string()))?;
        jsonwebtoken::TokenData { header, claims }
    };

    let claims = decoded.claims;
    let header_for_debug = decoded.header.clone();
    let token_identity_key = claims.identity_public_key.clone();
    let client_identity_key = client_claims
        .as_ref()
        .and_then(|cd| cd.identity_public_key.clone());

    // The key used for the Bedrock encryption handshake is the client's identity key (the one that signs the
    // client data JWT). Some OpenID tokens also repeat it as `identityPublicKey` / `cpk`.
    // Do NOT fall back to the OpenID JWT header key (x5u/x5c), as that is the provider signing key.
    if let (Some(token_key), Some(client_key)) = (&token_identity_key, &client_identity_key)
        && !token_key.is_empty()
        && !client_key.is_empty()
        && token_key != client_key
    {
        warn!(
            token_key_len = token_key.len(),
            client_key_len = client_key.len(),
            "openid identityPublicKey/cpk differs from clientData IdentityPublicKey"
        );
    }

    let pub_key_b64 = client_identity_key
        .clone()
        .filter(|k| !k.is_empty())
        .or_else(|| token_identity_key.clone().filter(|k| !k.is_empty()))
        .ok_or_else(|| {
            warn!(
                kid = ?decoded.header.kid,
                has_x5u = decoded.header.x5u.is_some(),
                has_x5c = decoded.header.x5c.is_some(),
                has_claim_identity_pk = claims.identity_public_key.is_some(),
                has_client_identity_pk = client_claims
                    .as_ref()
                    .and_then(|cd| cd.identity_public_key.clone())
                    .is_some(),
                provider_key_present = provider_key.is_some(),
                "Missing identityPublicKey; cannot derive session key"
            );
            AuthError::MissingIdentityKey
        })?;

    // Prefer values asserted by the XSTS/OpenID token; fall back to client data only when absent
    let mut xuid = claims.xuid.clone().or_else(|| {
        claims
            .extra_data
            .as_ref()
            .and_then(|extra| extra.xuid.clone())
    });
    let mut display_name = claims.xbox_username.clone().or_else(|| {
        claims
            .extra_data
            .as_ref()
            .and_then(|extra| extra.display_name.clone())
    });
    let mut uuid = claims
        .extra_data
        .as_ref()
        .and_then(|extra| extra.uuid.clone());

    if pub_key_b64.is_empty() {
        warn!(
            has_token_identity_key = token_identity_key.is_some(),
            has_header_key = header_for_debug.x5u.is_some() || header_for_debug.x5c.is_some(),
            has_client_identity_key = client_claims
                .as_ref()
                .and_then(|cd| cd.identity_public_key.clone())
                .is_some(),
            "Missing identity key after resolution"
        );
    }

    if let Some(cd) = client_claims.clone() {
        if display_name.is_none() {
            display_name = cd.display_name.or(cd.third_party_name);
        }
        if xuid.is_none() {
            xuid = cd.xuid;
        }
        if uuid.is_none() {
            uuid = cd.uuid;
        }
    }

    if xuid.is_none() || display_name.is_none() || pub_key_b64.is_empty() {
        warn!(
            got_claims_xuid = claims.xuid.is_some(),
            got_claims_xname = claims.xbox_username.is_some(),
            got_claims_identity_key = claims.identity_public_key.is_some(),
            got_extra_xuid = claims
                .extra_data
                .as_ref()
                .and_then(|e| e.xuid.clone())
                .is_some(),
            got_client_xuid = client_claims
                .as_ref()
                .and_then(|cd| cd.xuid.clone())
                .is_some(),
            got_client_identity_key = client_claims
                .as_ref()
                .and_then(|cd| cd.identity_public_key.clone())
                .is_some(),
            "identity fields missing after resolution"
        );
    }

    Ok(ValidatedIdentity {
        xuid,
        display_name,
        uuid,
        identity_public_key: pub_key_b64,
    })
}
