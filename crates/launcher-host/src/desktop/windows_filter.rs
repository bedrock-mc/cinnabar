//! Owns and checks the Win32 chooser's paired, double-NUL-terminated filter buffer.

/// Encodes nonempty display/pattern pairs; an empty input means no filter.
/// Rejects missing terminators, empty components and trailing data before native code runs.
pub(super) fn encode(filter: &str) -> Result<Option<Vec<u16>>, ()> {
    if filter.is_empty() {
        return Ok(None);
    }
    let pairs = filter.strip_suffix("\0\0").ok_or(())?;
    let mut parts = pairs.split('\0');
    while let Some(display) = parts.next() {
        let pattern = parts.next().ok_or(())?;
        if display.is_empty() || pattern.is_empty() {
            return Err(());
        }
    }
    Ok(Some(filter.encode_utf16().collect()))
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn absent_filter_has_no_native_buffer() {
        assert_eq!(encode(""), Ok(None));
    }

    #[test]
    fn complete_unicode_pairs_keep_their_native_terminators() {
        let filter = "图片\0*.png;*.jpg\0All files\0*.*\0\0";
        let encoded = encode(filter).unwrap().unwrap();
        assert_eq!(String::from_utf16(&encoded).unwrap(), filter);
        assert!(encoded.ends_with(&[0, 0]));
    }

    #[test]
    fn malformed_filters_are_rejected_without_a_dialog() {
        for filter in [
            "Images",
            "Images\0*.png",
            "Images\0*.png\0",
            "Images\0\0",
            "\0*.png\0\0",
            "Images\0*.png\0Orphan\0\0",
            "Images\0*.png\0\0trailing",
            "Images\0*.png\0\0\0",
            "\0\0",
        ] {
            assert_eq!(encode(filter), Err(()), "{filter:?}");
        }
    }
}
