//! Protected checkpoint identity and conservative migration of generation-only records.

use super::*;

const LEGACY_PREFIX: &[u8] = b"CNAC\x01";
const PREFIX: &[u8] = b"CNAC\x02";

/// One opaque SDK snapshot and the exact protected envelope revision that owns it.
pub(super) struct Stored {
    bytes: Zeroizing<Vec<u8>>,
    offset: usize,
    cache_allowed: bool,
    revision: [u8; 32],
}

impl Stored {
    /// Reads the authoritative active slot before a selected account's saved backup.
    pub(super) fn read(
        store: &AccountStore,
        generation: &str,
        identity: Option<&str>,
        oauth: &[u8; 32],
    ) -> io::Result<Self> {
        for slot in [None, identity]
            .into_iter()
            .take(if identity.is_some() { 2 } else { 1 })
        {
            if let Some(bytes) = read_optional(&checkpoint_path(store, slot))?
                && let Some(stored) =
                    Self::decode(Zeroizing::new(bytes), generation, identity, oauth)?
            {
                return Ok(stored);
            }
        }
        Ok(Self {
            bytes: Zeroizing::new(Vec::new()),
            offset: 0,
            cache_allowed: false,
            revision: Sha256::digest(b"").into(),
        })
    }

    /// Validates application binding while leaving all SDK framing and key policy to the SDK.
    pub(super) fn decode(
        bytes: Zeroizing<Vec<u8>>,
        generation: &str,
        identity: Option<&str>,
        oauth: &[u8; 32],
    ) -> io::Result<Option<Self>> {
        let mut remaining = bytes.as_slice();
        let prefix = take(&mut remaining, PREFIX.len())?;
        if prefix != PREFIX && prefix != LEGACY_PREFIX {
            return Err(invalid("unsupported native checkpoint"));
        }
        if field(&mut remaining)? != generation.as_bytes() {
            return Ok(None);
        }
        let cache_allowed = if prefix == PREFIX {
            if field(&mut remaining)? != identity.unwrap_or_default().as_bytes() {
                return Ok(None);
            }
            let bound = take(&mut remaining, 32)? == oauth;
            let allowed = match take(&mut remaining, 1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(invalid("invalid native checkpoint admission")),
            };
            bound && allowed
        } else {
            // Older records retain proof ownership but cannot qualify cached credentials.
            false
        };
        let offset = bytes.len() - remaining.len();
        let revision = Sha256::digest(bytes.as_slice()).into();
        Ok(Some(Self {
            bytes,
            offset,
            cache_allowed,
            revision,
        }))
    }

    /// Borrows only opaque SDK bytes, including after optional-cache suppression.
    pub(super) fn payload(&self) -> Option<&[u8]> {
        let bytes = &self.bytes[self.offset..];
        (!bytes.is_empty()).then_some(bytes)
    }

    /// Borrows the original protected bytes for an atomic saved-account backup.
    pub(super) fn envelope(&self) -> &[u8] {
        &self.bytes
    }

    /// Includes cache admission and opaque payload in the cross-process revision.
    pub(super) fn revision(&self) -> [u8; 32] {
        self.revision
    }

    /// Reports application admission only; fresh SDK policy/time admission remains mandatory.
    pub(super) fn cache_allowed(&self) -> bool {
        self.cache_allowed
    }

    /// A changed protected OAuth record cannot qualify this snapshot in the current lease.
    pub(super) fn disallow_cache(&mut self) {
        self.cache_allowed = false;
    }
}

/// Encodes an exact OAuth/account fence without interpreting or copying the SDK schema.
pub(in crate::accounts) fn checkpoint_bytes(
    generation: &str,
    identity: Option<&str>,
    oauth: &[u8; 32],
    checkpoint: &[u8],
    cache_allowed: bool,
) -> io::Result<Zeroizing<Vec<u8>>> {
    let identity = identity.unwrap_or_default();
    if !identity.is_empty() {
        checked_id(identity)?;
    }
    let length = PREFIX.len() + 2 + generation.len() + 2 + identity.len() + oauth.len() + 1;
    if checkpoint.len().saturating_add(length) > MAX_FILE_BYTES as usize {
        return Err(invalid("invalid native checkpoint size"));
    }
    let generation_length =
        u16::try_from(generation.len()).map_err(|_| invalid("invalid sign-in generation"))?;
    let identity_length =
        u16::try_from(identity.len()).map_err(|_| invalid("invalid selected identity"))?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(length + checkpoint.len()));
    bytes.extend_from_slice(PREFIX);
    bytes.extend_from_slice(&generation_length.to_le_bytes());
    bytes.extend_from_slice(generation.as_bytes());
    bytes.extend_from_slice(&identity_length.to_le_bytes());
    bytes.extend_from_slice(identity.as_bytes());
    bytes.extend_from_slice(oauth);
    bytes.push(u8::from(cache_allowed));
    bytes.extend_from_slice(checkpoint);
    Ok(bytes)
}

/// Rebinds an already verified pending identity without admitting a mismatched OAuth snapshot.
pub(in crate::accounts) fn rebind_checkpoint(
    bytes: Zeroizing<Vec<u8>>,
    generation: &str,
    previous: Option<&str>,
    identity: &str,
    oauth: &[u8; 32],
) -> io::Result<Zeroizing<Vec<u8>>> {
    let stored = Stored::decode(bytes, generation, previous, oauth)?
        .ok_or_else(|| invalid("pending native checkpoint binding changed"))?;
    checkpoint_bytes(
        generation,
        Some(identity),
        oauth,
        stored.payload().unwrap_or_default(),
        stored.cache_allowed(),
    )
}

/// Takes a bounded application string field without allocating secret storage.
fn field<'a>(remaining: &mut &'a [u8]) -> io::Result<&'a [u8]> {
    let length = u16::from_le_bytes(
        take(remaining, 2)?
            .try_into()
            .map_err(|_| invalid("invalid native checkpoint"))?,
    );
    take(remaining, usize::from(length))
}
