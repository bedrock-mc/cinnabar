//! Independently expiring capabilities; the registry never retains a second plaintext copy.

use bridge::contract::{RELAY_RESUME_BYTES, RELAY_RESUME_SECONDS};
use rand::{TryRng, rngs::SysRng};
use std::{
    io,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub(super) struct Bearer {
    bytes: Zeroizing<[u8; RELAY_RESUME_BYTES]>,
    pub(super) expires: Instant,
}

impl Bearer {
    /// Issues a new capability without borrowing or extending a previous bearer.
    pub(super) fn new(now: Instant) -> io::Result<Self> {
        let mut bytes = Zeroizing::new([0; RELAY_RESUME_BYTES]);
        SysRng
            .try_fill_bytes(&mut *bytes)
            .map_err(|_| io::Error::other("gateway capability generation failed"))?;
        let expires = now
            .checked_add(Duration::from_secs(u64::from(RELAY_RESUME_SECONDS)))
            .ok_or_else(|| io::Error::other("gateway capability time overflow"))?;
        Ok(Self { bytes, expires })
    }

    /// Compares only canonical lowercase capabilities and their original expiry.
    pub(super) fn matches(&self, value: &str, now: Instant) -> bool {
        if now >= self.expires
            || value.len() != RELAY_RESUME_BYTES * 2
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return false;
        }
        let mut bytes = Zeroizing::new([0; RELAY_RESUME_BYTES]);
        hex::decode_to_slice(value, &mut *bytes).is_ok()
            && bool::from(self.bytes[..].ct_eq(&bytes[..]))
    }

    /// Copies the capability only into the bounded control frame intended for its controller.
    pub(super) fn wire(&self) -> String {
        hex::encode(self.bytes.as_slice())
    }
}
