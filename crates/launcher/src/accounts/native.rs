//! Generation-fenced native credentials using the account store's existing private files and leases.

use super::*;
use serde::Deserializer;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(super) const MAX_GENERATION_BYTES: usize = 128;

mod checkpoint;
use checkpoint::Stored;
pub(super) use checkpoint::{checkpoint_bytes, rebind_checkpoint};

/// One selected sign-in, held against Go refresh and account-store replacement.
/// This owner has no Debug/Clone; callers must acquire nonblocking and bound their wait.
pub struct NativeLease {
    store: AccountStore,
    _store_lock: File,
    _cache_locks: (File, File),
    oauth: OAuth,
    selected_id: Option<String>,
    oauth_binding: [u8; 32],
    checkpoint: Stored,
}

#[derive(Deserialize)]
struct OAuth {
    #[serde(rename = "access_token", default, deserialize_with = "secret")]
    _access_token: Zeroizing<String>,
    #[serde(deserialize_with = "secret")]
    refresh_token: Zeroizing<String>,
    #[serde(rename = "expiry", default)]
    _expiry: Option<String>,
    cinnabar_sign_in_generation: String,
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}

/// Serializes a candidate without changing the lease's current protected credential owner.
#[derive(Serialize)]
struct OAuthPublication<'a> {
    access_token: &'a str,
    refresh_token: &'a str,
    expiry: &'a str,
    cinnabar_sign_in_generation: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_type: Option<&'static str>,
    #[serde(flatten)]
    other: &'a BTreeMap<String, serde_json::Value>,
}

impl AccountStore {
    /// Reads a consistent saved-account snapshot without waiting for an acquisition's store lease.
    /// Frame callers retain their previous view and retry on WouldBlock.
    pub fn try_snapshot(&self) -> io::Result<(Vec<AccountProfile>, Option<String>)> {
        private_directory(&self.directory)?;
        let _lease = try_lease(&self.directory.join("store.lock"))?;
        let index = self.index()?;
        Ok((sorted_profiles(index.profiles.into_values()), index.active))
    }

    /// Removes unfinished sign-in files only when their leases are immediately available.
    pub fn try_discard_pending(&self) -> io::Result<()> {
        private_directory(&self.directory)?;
        let _lease = try_lease(&self.directory.join("store.lock"))?;
        let pending = self.pending_cache();
        let _pending = cache_try_leases(&pending)?;
        remove_optional(&pending)?;
        remove_optional(&derived_path(&pending))?;
        super::pending::remove_attempt(self)
    }

    /// Tries all credential leases once, releasing acquired files if any later lease is busy.
    /// The caller schedules retries off the frame and cancels by dropping its attempt.
    pub fn try_native_lease(&self) -> io::Result<NativeLease> {
        private_directory(&self.directory)?;
        let store_lock = try_lease(&self.directory.join("store.lock"))?;
        let cache_locks = cache_try_leases(&self.active_cache)?;
        let bytes = Zeroizing::new(read_private_bounded(
            &self.active_cache,
            MAX_OAUTH_CACHE_BYTES as u64,
        )?);
        let oauth = parse_oauth(&bytes)?;
        let selected_id = self.index()?.active;
        let oauth_binding = Sha256::digest(&bytes).into();
        let checkpoint = Stored::read(
            self,
            &oauth.cinnabar_sign_in_generation,
            selected_id.as_deref(),
            &oauth_binding,
        )?;
        Ok(NativeLease {
            store: self.clone(),
            _store_lock: store_lock,
            _cache_locks: cache_locks,
            oauth,
            selected_id,
            oauth_binding,
            checkpoint,
        })
    }
}

impl NativeLease {
    /// Returns the immutable sign-in generation, which every later publication must still own.
    pub fn generation(&self) -> &str {
        &self.oauth.cinnabar_sign_in_generation
    }

    /// Returns the selected saved identity when the account picker has verified one.
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    /// Returns the configured OAuth client ID if an imported cache explicitly records one.
    pub fn client_id(&self) -> Option<&str> {
        self.oauth
            .other
            .get("client_id")
            .and_then(serde_json::Value::as_str)
    }

    /// Explicitly borrows the refresh credential while this owner excludes another refresh.
    pub fn refresh_token(&self) -> &str {
        &self.oauth.refresh_token
    }

    /// Borrows saved access material only for comparing a suspended SDK checkpoint.
    pub fn access_token(&self) -> &str {
        &self.oauth._access_token
    }

    /// Borrows the saved absolute OAuth expiry; an absent or malformed value requires refresh.
    pub fn oauth_expiry(&self) -> Option<&str> {
        self.oauth._expiry.as_deref()
    }

    /// Identifies the selected account's exact OAuth and checkpoint revision under this lease.
    pub fn binding(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(self.oauth_binding);
        hash.update(self.checkpoint.revision());
        hash.update(self.selected_id.as_deref().unwrap_or_default().as_bytes());
        hash.finalize().into()
    }

    /// Identifies the actual protected snapshot independently of an OAuth-only rotation.
    pub fn checkpoint_revision(&self) -> [u8; 32] {
        self.checkpoint.revision()
    }

    /// Reads a checkpoint only for this sign-in generation; SDK policy validation remains separate.
    /// A different OAuth revision is safe only after the SDK adopts this lease's current credentials.
    pub fn checkpoint(&self) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self
            .checkpoint
            .payload()
            .map(|bytes| Zeroizing::new(bytes.to_vec())))
    }

    /// Permits optional cache admission only for an unsuppressed exact OAuth/account envelope.
    /// The SDK must still validate policy/proof and obtain its own fresh NSAL response.
    pub fn checkpoint_cache_allowed(&self) -> bool {
        self.checkpoint.cache_allowed()
    }

    /// Suppresses persisted reuse before dispatching a request that can refuse cached entries.
    /// Opaque proof/refresh bytes survive; failure leaves dispatch to the caller unstarted.
    pub fn suppress_checkpoint(&mut self) -> io::Result<()> {
        if let Some(checkpoint) = self.checkpoint()? {
            self.write_checkpoint(&checkpoint, false)?;
            if let Some(id) = self.selected_id() {
                write_private(
                    &checkpoint_path(&self.store, Some(id)),
                    self.checkpoint.envelope(),
                )?;
            }
        }
        Ok(())
    }

    /// Rejects removal, replacement or account switching before returning any prepared credential.
    pub fn recheck(&self) -> io::Result<()> {
        let current = Zeroizing::new(read_private_bounded(
            &self.store.active_cache,
            MAX_OAUTH_CACHE_BYTES as u64,
        )?);
        let binding: [u8; 32] = Sha256::digest(&current).into();
        if parse_oauth(&current)?.cinnabar_sign_in_generation != self.generation()
            || binding != self.oauth_binding
            || self.store.index()?.active != self.selected_id
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "saved sign-in changed; reopen the account",
            ));
        }
        Ok(())
    }

    /// Publishes only the active generation's OAuth rotation, before identity is known.
    /// Saved accounts/checkpoints remain untouched; the complete JSON must fit the Go reader.
    pub fn publish_oauth(&mut self, access: &str, refresh: &str, expiry: &str) -> io::Result<()> {
        self.recheck()?;
        let token_type = (!self.oauth.other.contains_key("token_type")).then_some("Bearer");
        let oauth = oauth_bytes(
            self.generation(),
            access,
            refresh,
            expiry,
            &self.oauth.other,
        )?;
        write_private_bounded(
            &self.store.active_cache,
            &oauth,
            MAX_OAUTH_CACHE_BYTES as u64,
        )?;
        self.oauth._access_token = Zeroizing::new(access.to_owned());
        self.oauth.refresh_token = Zeroizing::new(refresh.to_owned());
        self.oauth._expiry = Some(expiry.to_owned());
        if let Some(token_type) = token_type {
            self.oauth.other.insert(
                "token_type".to_owned(),
                serde_json::Value::String(token_type.to_owned()),
            );
        }
        let binding = Sha256::digest(&oauth).into();
        if binding != self.oauth_binding {
            self.checkpoint.disallow_cache();
        }
        self.oauth_binding = binding;
        Ok(())
    }

    /// Saves a checkpoint and credential copy only after the caller verifies the selected XUID.
    /// A mismatched identity changes neither the saved credential nor its proof slot.
    pub fn publish_verified_identity(&mut self, xuid: &str, checkpoint: &[u8]) -> io::Result<()> {
        self.publish_identity_checkpoint(xuid, checkpoint, true)
    }

    /// Updates the authoritative slot before its saved backup, retaining explicit cache suppression.
    fn publish_identity_checkpoint(
        &mut self,
        xuid: &str,
        checkpoint: &[u8],
        cache_allowed: bool,
    ) -> io::Result<()> {
        self.recheck()?;
        checked_id(xuid)?;
        if self.selected_id().is_some_and(|selected| selected != xuid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native account identity does not match the selection",
            ));
        }
        if let Some(id) = self.selected_id.clone() {
            self.store
                .write_credentials(&id, &read_credentials(&self.store.active_cache)?)?;
            self.write_checkpoint(checkpoint, cache_allowed)?;
            write_private(
                &checkpoint_path(&self.store, Some(&id)),
                self.checkpoint.envelope(),
            )?;
        } else {
            self.write_checkpoint(checkpoint, cache_allowed)?;
        }
        Ok(())
    }

    /// Saves public profile metadata under the same selected-generation lease as its credentials.
    pub fn remember_verified_profile(
        &mut self,
        profile: &AccountProfile,
    ) -> io::Result<AccountProfile> {
        self.recheck()?;
        let mut profile = checked_profile(
            &profile.id,
            &profile.gamertag,
            profile.picture_path.as_deref(),
        )?;
        if self.selected_id().is_some_and(|id| id != profile.id) {
            return Err(invalid("verified profile does not match selected account"));
        }
        let mut index = self.store.index()?;
        let previous = index.profiles.get(&profile.id);
        self.store.save_picture(&mut profile, previous)?;
        self.store
            .write_credentials(&profile.id, &read_credentials(&self.store.active_cache)?)?;
        index.active = Some(profile.id.clone());
        index.profiles.insert(profile.id.clone(), profile.clone());
        self.store.write_index(&index)?;
        let checkpoint = self.checkpoint()?;
        let cache_allowed = self.checkpoint.cache_allowed();
        self.selected_id = Some(profile.id.clone());
        if let Some(checkpoint) = checkpoint {
            self.publish_identity_checkpoint(&profile.id, &checkpoint, cache_allowed)?;
        }
        Ok(profile)
    }

    /// Retains active proof ownership while identity is unknown, without touching a saved account.
    pub fn publish_active_checkpoint(&mut self, checkpoint: &[u8]) -> io::Result<()> {
        self.recheck()?;
        self.write_checkpoint(checkpoint, true)
    }

    /// Writes the authoritative active snapshot before it can become a saved backup.
    fn write_checkpoint(&mut self, checkpoint: &[u8], cache_allowed: bool) -> io::Result<()> {
        self.recheck()?;
        let bytes = checkpoint_bytes(
            self.generation(),
            self.selected_id(),
            &self.oauth_binding,
            checkpoint,
            cache_allowed,
        )?;
        let stored = Stored::decode(
            bytes,
            self.generation(),
            self.selected_id(),
            &self.oauth_binding,
        )?
        .ok_or_else(|| invalid("native checkpoint binding changed"))?;
        write_private(&checkpoint_path(&self.store, None), stored.envelope())?;
        self.checkpoint = stored;
        Ok(())
    }

    /// Reuses the Go-compatible install device file under its own nonblocking lease.
    /// `fresh` replaces a missing or unusable profile; validation belongs to its metadata owner.
    pub fn device_profile(
        &self,
        path: &Path,
        fresh: &[u8],
        valid: impl FnOnce(&[u8]) -> bool,
    ) -> io::Result<Zeroizing<Vec<u8>>> {
        let _lease = try_lease(&cache_lock_path(path))?;
        match read_optional(path)? {
            Some(bytes) if valid(&bytes) => Ok(Zeroizing::new(bytes)),
            _ => {
                write_private(path, fresh)?;
                Ok(Zeroizing::new(fresh.to_vec()))
            }
        }
    }
}

/// Encodes one Go-readable OAuth publication, enforcing the shared complete-record bound.
pub(super) fn oauth_bytes(
    generation: &str,
    access: &str,
    refresh: &str,
    expiry: &str,
    other: &BTreeMap<String, serde_json::Value>,
) -> io::Result<Zeroizing<Vec<u8>>> {
    if refresh.is_empty() {
        return Err(invalid("native OAuth requires a refresh credential"));
    }
    if access
        .len()
        .saturating_add(refresh.len())
        .saturating_add(expiry.len())
        > MAX_OAUTH_CACHE_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "OAuth cache exceeds the shared credential limit",
        ));
    }
    let oauth = Zeroizing::new(
        serde_json::to_vec(&OAuthPublication {
            access_token: access,
            refresh_token: refresh,
            expiry,
            cinnabar_sign_in_generation: generation,
            token_type: (!other.contains_key("token_type")).then_some("Bearer"),
            other,
        })
        .map_err(|_| invalid("serialize native credentials"))?,
    );
    if oauth.len() > MAX_OAUTH_CACHE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "OAuth cache exceeds the shared credential limit",
        ));
    }
    Ok(oauth)
}

/// Validates the authoritative OAuth generation without echoing credential JSON on failure.
fn parse_oauth(bytes: &[u8]) -> io::Result<OAuth> {
    let oauth: OAuth =
        serde_json::from_slice(bytes).map_err(|_| invalid("invalid native OAuth cache"))?;
    let generation = &oauth.cinnabar_sign_in_generation;
    if oauth.refresh_token.is_empty()
        || generation.is_empty()
        || generation.len() > MAX_GENERATION_BYTES
        || !generation
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid(
            "native account requires an existing valid sign-in generation",
        ));
    }
    Ok(oauth)
}

/// Keeps one slot per saved identity; its envelope rejects an older generation after reauthentication.
pub(super) fn checkpoint_path(store: &AccountStore, selected_id: Option<&str>) -> PathBuf {
    store.directory.join(format!(
        "native-{}.checkpoint",
        selected_id.unwrap_or("active")
    ))
}

/// Removes the current generation's native secret alongside the existing saved account.
pub(super) fn remove_current_checkpoint(store: &AccountStore) -> io::Result<()> {
    let selected = store.index()?.active;
    remove_optional(&checkpoint_path(store, selected.as_deref()))?;
    remove_optional(&checkpoint_path(store, None))
}

/// Takes a checked prefix without allocating or trusting a persisted length.
fn take<'a>(remaining: &mut &'a [u8], length: usize) -> io::Result<&'a [u8]> {
    let (value, tail) = remaining
        .split_at_checked(length)
        .ok_or_else(|| invalid("invalid native checkpoint"))?;
    *remaining = tail;
    Ok(value)
}

/// Deserializes one secret owner without adding serde implementations to public credentials.
fn secret<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(deserializer).map(Zeroizing::new)
}

#[cfg(test)]
mod tests;
