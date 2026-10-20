//! Exact protected OAuth comparison without admitting saved access credentials.

use super::*;

/// Preserves suspended cache material only when the protected store binds every OAuth field.
pub(super) fn restore_account(
    lease: &NativeLease,
    policy: &xbox::Config,
) -> io::Result<xbox::Account> {
    let auth = MicrosoftAuth::new(policy.clone()).map_err(service_error)?;
    let Some(bytes) = lease.checkpoint()? else {
        return auth
            .into_account(import_credentials(lease, policy)?)
            .map_err(service_error);
    };
    let mut account = auth
        .restore_account(xbox::Checkpoint::from_bytes(bytes).map_err(service_error)?)
        .map_err(service_error)?;
    if !lease.checkpoint_cache_allowed() || !matches_oauth(account.credentials(), lease) {
        // Changed or unqualified material keeps proof ownership and forces actual OAuth refresh.
        account
            .replace_credentials(import_credentials(lease, policy)?)
            .map_err(service_error)?;
    }
    Ok(account)
}

/// Compares original material; this function never constructs an active saved-access owner.
fn matches_oauth(credentials: &Credentials, lease: &NativeLease) -> bool {
    let Some(expiry) = lease.oauth_expiry() else {
        return false;
    };
    let Ok(expiry) = OffsetDateTime::parse(expiry, &Rfc3339) else {
        return false;
    };
    let Ok(nanos) = u128::try_from(expiry.unix_timestamp_nanos()) else {
        return false;
    };
    let Ok(seconds) = u64::try_from(nanos / 1_000_000_000) else {
        return false;
    };
    let Ok(subsecond) = u32::try_from(nanos % 1_000_000_000) else {
        return false;
    };
    let Some(expiry) = UNIX_EPOCH.checked_add(Duration::new(seconds, subsecond)) else {
        return false;
    };
    credentials.access_token() == lease.access_token()
        && credentials.refresh_token() == lease.refresh_token()
        && credentials.expires_at() == expiry
}
