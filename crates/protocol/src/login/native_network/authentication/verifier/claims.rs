//! Selected-account and temporal policy after issuer signature verification.

use super::rejected;
use nethernet::identity::jwt::Claims;
use p384::ecdsa::VerifyingKey;
use std::{
    io,
    time::{Duration, SystemTime},
};

const CLAIMS_LEEWAY: Duration = Duration::from_secs(60);

/// Binds an already verified signed identity to the selected account and minted connection proof.
pub(super) fn validate_binding(
    claims: &Claims,
    selected_xuid: &str,
    key: &VerifyingKey,
) -> io::Result<()> {
    if claims.xuid() != Some(selected_xuid)
        || selected_xuid.is_empty()
        || selected_xuid.parse::<i64>().is_err()
        || claims.client_public_key().map_err(|_| rejected())? != *key
    {
        return Err(rejected());
    }
    Ok(())
}

/// Validates each signed peer's own identity and finite time claims without local account binding.
pub(super) fn validate_identity(claims: &Claims, now: SystemTime) -> io::Result<()> {
    if claims.subject().is_none_or(str::is_empty)
        || claims
            .xuid()
            .is_none_or(|xuid| xuid.is_empty() || xuid.parse::<i64>().is_err())
        || claims.expiry().is_none_or(|expiry| now >= expiry)
    {
        return Err(rejected());
    }
    claims.client_public_key().map_err(|_| rejected())?;
    let latest = now.checked_add(CLAIMS_LEEWAY).ok_or_else(rejected)?;
    for name in ["nbf", "iat"] {
        if claims
            .numeric_date(name)
            .map_err(|_| rejected())?
            .is_some_and(|time| time > latest)
        {
            return Err(rejected());
        }
    }
    if let Some(value) = claims
        .values()
        .get("leguuid")
        .filter(|value| !value.is_null())
    {
        let value = value.as_str().ok_or_else(rejected)?;
        if !value.is_empty() && uuid::Uuid::parse_str(value).map_or(true, |id| id.is_nil()) {
            return Err(rejected());
        }
    }
    let name = claims.display_name().ok_or_else(rejected)?;
    if name.is_empty()
        || name.len() > 15
        || name.starts_with(' ')
        || name.ends_with(' ')
        || name.as_bytes()[0].is_ascii_digit()
        || name.contains("  ")
        || !name
            .bytes()
            .any(|byte| byte.is_ascii_alphanumeric() || byte == b' ')
    {
        return Err(rejected());
    }
    Ok(())
}
