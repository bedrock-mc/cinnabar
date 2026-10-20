//! Presentation retained only after the configured provider verified these claims.

use bedrock_net::login::{LoginError, TrustedIdentity};
use nethernet::identity::jwt::Claims;

/// Retains this joining player's signed identity without consulting client or host metadata.
pub(super) fn identity(claims: &Claims) -> Result<TrustedIdentity, LoginError> {
    TrustedIdentity::new(
        claims.xuid().ok_or(LoginError::Authentication)?.to_owned(),
        claims
            .client_public_key()
            .map_err(|_| LoginError::Authentication)?,
    )?
    .with_player_identity(
        claims
            .display_name()
            .ok_or(LoginError::Authentication)?
            .to_owned(),
        Some(optional(claims, "leguuid")?),
        optional(claims, "mid")?.to_owned(),
        optional(claims, "tid")?.to_owned(),
    )
}

/// Missing/null strings retain the provider's empty default; malformed present fields reject.
fn optional<'a>(claims: &'a Claims, name: &str) -> Result<&'a str, LoginError> {
    match claims.values().get(name).filter(|value| !value.is_null()) {
        None => Ok(""),
        Some(value) => value.as_str().ok_or(LoginError::Authentication),
    }
}
