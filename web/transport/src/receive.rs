use tokio::io::ReadBuf;

/// Copies one chunk into spare output space, initializing only the bytes delivered.
pub(crate) fn copy_chunk(
    out: &mut ReadBuf<'_>,
    remaining: usize,
    copy: impl FnOnce(&mut [u8]),
) -> usize {
    let count = out.remaining().min(remaining);
    copy(out.initialize_unfilled_to(count));
    out.advance(count);
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::MaybeUninit;

    #[test]
    fn small_receive_initializes_only_delivered_bytes() {
        let mut storage = [MaybeUninit::uninit(); 4096];
        let mut out = ReadBuf::uninit(&mut storage);
        assert_eq!(
            copy_chunk(&mut out, 2, |bytes| bytes.copy_from_slice(&[1, 2])),
            2
        );
        assert_eq!(out.filled(), &[1, 2]);
        assert_eq!(out.initialized().len(), 2);
    }

    #[test]
    fn receive_keeps_existing_bytes_and_stops_at_capacity() {
        let mut storage = [MaybeUninit::uninit(); 3];
        let mut out = ReadBuf::uninit(&mut storage);
        out.put_slice(&[1]);
        assert_eq!(copy_chunk(&mut out, 8, |bytes| bytes.fill(2)), 2);
        assert_eq!(out.filled(), &[1, 2, 2]);
        assert_eq!(
            copy_chunk(&mut out, 8, |bytes| assert!(bytes.is_empty())),
            0
        );
    }
}
