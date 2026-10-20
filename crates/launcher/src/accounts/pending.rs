//! Short protected transactions for a native interactive sign-in attempt.

use super::*;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const ATTEMPT_FILE: &str = "native-signin.json";
const CHECKPOINT_FILE: &str = "native-pending.checkpoint";

/// A pending attempt owns no file lease or credential. A newer attempt invalidates this value.
pub struct PendingSignIn {
    store: AccountStore,
    attempt: Attempt,
}

impl std::fmt::Debug for PendingSignIn {
    /// Omits account paths and selection details from diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingSignIn")
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Attempt {
    generation: String,
    active: Option<String>,
    binding: Option<[u8; 32]>,
    expected_identity: Option<String>,
}

impl AccountStore {
    /// Discards only the cancelled native attempt; a newer pending sign-in remains untouched.
    pub fn try_discard_native_pending(&self, generation: &str) -> io::Result<()> {
        private_directory(&self.directory)?;
        let _store = try_lease(&self.directory.join("store.lock"))?;
        let _pending = cache_try_leases(&self.pending_cache())?;
        let Some(bytes) = read_optional(&self.directory.join(ATTEMPT_FILE))? else {
            return Ok(());
        };
        let attempt: Attempt =
            serde_json::from_slice(&bytes).map_err(|_| invalid("invalid sign-in attempt"))?;
        if attempt.generation != generation {
            return Ok(());
        }
        remove_optional(&self.pending_cache())?;
        remove_optional(&derived_path(&self.pending_cache()))?;
        remove_attempt(self)
    }

    /// Reserves a fresh attempt against the current selection/revision, releasing every lease before HTTP.
    /// A reauthentication must retain its selected identity; Add account passes no expected identity.
    pub fn try_begin_native_sign_in(
        &self,
        generation: &str,
        expected_identity: Option<&str>,
    ) -> io::Result<PendingSignIn> {
        if generation.is_empty()
            || generation.len() > super::native::MAX_GENERATION_BYTES
            || !generation
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(invalid("invalid sign-in generation"));
        }
        if let Some(id) = expected_identity {
            checked_id(id)?;
        }
        private_directory(&self.directory)?;
        let _store = try_lease(&self.directory.join("store.lock"))?;
        let _active = cache_try_leases(&self.active_cache)?;
        let _pending = cache_try_leases(&self.pending_cache())?;
        let active = self.index()?.active;
        if expected_identity.is_some() && active.as_deref() != expected_identity {
            return Err(stale());
        }
        let attempt = Attempt {
            generation: generation.to_owned(),
            active,
            binding: active_binding(self)?,
            expected_identity: expected_identity.map(str::to_owned),
        };
        // Retire the marker before removing an old attempt's protected files.
        remove_attempt(self)?;
        remove_optional(&self.pending_cache())?;
        remove_optional(&derived_path(&self.pending_cache()))?;
        let bytes =
            serde_json::to_vec(&attempt).map_err(|_| invalid("serialize sign-in attempt"))?;
        write_private(&self.directory.join(ATTEMPT_FILE), &bytes)?;
        Ok(PendingSignIn {
            store: self.clone(),
            attempt,
        })
    }
}

impl PendingSignIn {
    /// Publishes the newest OAuth rotation before writing proof, without modifying active or saved accounts.
    /// Retry WouldBlock off the frame with a bounded deadline; a stale attempt must never be retried as new.
    pub fn try_publish(&self, access: &str, refresh: &str, expiry: &str) -> io::Result<()> {
        let bytes = super::native::oauth_bytes(
            &self.attempt.generation,
            access,
            refresh,
            expiry,
            &BTreeMap::new(),
        )?;
        let _store = try_lease(&self.store.directory.join("store.lock"))?;
        let _active = cache_try_leases(&self.store.active_cache)?;
        let _pending = cache_try_leases(&self.store.pending_cache())?;
        self.recheck()?;
        write_private_bounded(
            &self.store.pending_cache(),
            &bytes,
            MAX_OAUTH_CACHE_BYTES as u64,
        )?;
        Ok(())
    }

    /// Persists proof only after the pending OAuth rotation is already durable.
    pub fn try_publish_checkpoint(&self, checkpoint: &[u8]) -> io::Result<()> {
        let _store = try_lease(&self.store.directory.join("store.lock"))?;
        let _active = cache_try_leases(&self.store.active_cache)?;
        let _pending = cache_try_leases(&self.store.pending_cache())?;
        self.recheck()?;
        let oauth = Zeroizing::new(read_private_bounded(
            &self.store.pending_cache(),
            MAX_OAUTH_CACHE_BYTES as u64,
        )?);
        let proof = super::native::checkpoint_bytes(
            &self.attempt.generation,
            self.attempt.expected_identity.as_deref(),
            &Sha256::digest(oauth.as_slice()).into(),
            checkpoint,
            true,
        )?;
        write_private(&self.store.directory.join(CHECKPOINT_FILE), &proof)
    }

    /// Commits only a verified profile belonging to this still-current pending attempt.
    /// Runs on the account mutation worker, after the previous account core has stopped.
    pub fn try_commit(&self, profile: &AccountProfile) -> io::Result<()> {
        let mut profile = checked_profile(
            &profile.id,
            &profile.gamertag,
            profile.picture_path.as_deref(),
        )?;
        if self
            .attempt
            .expected_identity
            .as_deref()
            .is_some_and(|id| id != profile.id)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "sign-in identity does not match selected account",
            ));
        }
        let _store = try_lease(&self.store.directory.join("store.lock"))?;
        let _active = cache_try_leases(&self.store.active_cache)?;
        let _pending = cache_try_leases(&self.store.pending_cache())?;
        self.recheck()?;
        let credentials = read_credentials(&self.store.pending_cache())?;
        #[derive(Deserialize)]
        struct Generation<'a> {
            cinnabar_sign_in_generation: &'a str,
        }
        let value: Generation<'_> = serde_json::from_str(&credentials.oauth)
            .map_err(|_| invalid("invalid pending credentials"))?;
        if value.cinnabar_sign_in_generation != self.attempt.generation {
            return Err(stale());
        }
        let proof = super::native::rebind_checkpoint(
            Zeroizing::new(read_private(&self.store.directory.join(CHECKPOINT_FILE))?),
            &self.attempt.generation,
            self.attempt.expected_identity.as_deref(),
            &profile.id,
            &Sha256::digest(credentials.oauth.as_bytes()).into(),
        )?;
        let mut index = self.store.index()?;
        let previous = index.profiles.get(&profile.id);
        self.store.save_picture(&mut profile, previous)?;
        self.store.write_credentials(&profile.id, &credentials)?;
        write_private(
            &super::native::checkpoint_path(&self.store, Some(&profile.id)),
            &proof,
        )?;
        index.profiles.insert(profile.id.clone(), profile.clone());
        self.store
            .activate_locked(&mut index, &profile.id, &credentials)?;
        // A cleanup failure is reported after the verified account has become authoritative.
        remove_optional(&self.store.pending_cache())?;
        remove_optional(&derived_path(&self.store.pending_cache()))?;
        remove_attempt(&self.store)
    }

    /// Checks both the attempt marker and the active account captured before browser interaction.
    fn recheck(&self) -> io::Result<()> {
        let bytes = read_private(&self.store.directory.join(ATTEMPT_FILE))?;
        let current: Attempt =
            serde_json::from_slice(&bytes).map_err(|_| invalid("invalid sign-in attempt"))?;
        if current != self.attempt
            || self.store.index()?.active != self.attempt.active
            || active_binding(&self.store)? != self.attempt.binding
        {
            return Err(stale());
        }
        Ok(())
    }
}

/// Hashes bounded protected bytes without copying OAuth fields into an ordinary deserialized value.
fn active_binding(store: &AccountStore) -> io::Result<Option<[u8; 32]>> {
    match read_private_bounded(&store.active_cache, MAX_OAUTH_CACHE_BYTES as u64) {
        Ok(bytes) => Ok(Some(
            Sha256::digest(Zeroizing::new(bytes).as_slice()).into(),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Retires the attempt marker and its fixed protected proof slot alongside pending OAuth cleanup.
pub(super) fn remove_attempt(store: &AccountStore) -> io::Result<()> {
    remove_optional(&store.directory.join(ATTEMPT_FILE))?;
    remove_optional(&store.directory.join(CHECKPOINT_FILE))
}

/// Reports a retired attempt without exposing identifiers or credentials.
fn stale() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "sign-in attempt or selected account changed",
    )
}

#[cfg(test)]
mod tests;
