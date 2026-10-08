/// Checks the complete carrier budget before appending a serialized field.
pub(crate) fn append_bounded(
    payload: &mut Vec<u8>,
    bytes: &[u8],
    limit: usize,
    overhead: usize,
) -> Option<()> {
    if payload
        .len()
        .checked_add(bytes.len())?
        .checked_add(overhead)?
        > limit
    {
        return None;
    }
    payload.extend_from_slice(bytes);
    Some(())
}

/// Whether the SHA-256 trailer at `payload_end` seals `bytes`; on success returns the SHA-256 of
/// all of `bytes`, so a carrier's file identity costs no second pass.
pub(crate) fn sealed_identity(bytes: &[u8], payload_end: usize) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let (payload, trailer) = bytes.split_at_checked(payload_end)?;
    let mut hasher = Sha256::new();
    hasher.update(payload);
    if hasher.clone().finalize().as_slice() != trailer {
        return None;
    }
    hasher.update(trailer);
    Some(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_identity_is_the_whole_file_digest_and_rejects_a_broken_seal() {
        use sha2::{Digest, Sha256};
        let mut bytes = b"payload".to_vec();
        let seal = Sha256::digest(&bytes);
        bytes.extend_from_slice(&seal);
        assert_eq!(
            sealed_identity(&bytes, 7),
            Some(<[u8; 32]>::from(Sha256::digest(&bytes)))
        );
        bytes[0] ^= 1;
        assert_eq!(sealed_identity(&bytes, 7), None);
        assert_eq!(sealed_identity(&bytes, bytes.len() + 1), None);
    }
    #[test]
    fn review_oversized_carrier_fields_are_refused_before_allocation() {
        let mut payload = Vec::new();
        assert!(append_bounded(&mut payload, &[0; 17], 16, 4).is_none());
        assert_eq!(payload.capacity(), 0);
        assert!(append_bounded(&mut payload, &[0; 12], 16, 4).is_some());
        let capacity = payload.capacity();
        assert!(append_bounded(&mut payload, &[0], 16, 4).is_none());
        assert_eq!(payload.len(), 12);
        assert_eq!(payload.capacity(), capacity);
    }
}
