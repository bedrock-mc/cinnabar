use std::io::Write;

use super::*;

/// A stored archive with the given entry names and an optional comment.
fn archive(names: &[&str], comment: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for name in names {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
    }
    writer.set_raw_comment(comment.into());
    writer.finish().unwrap().into_inner()
}

#[test]
fn prepare_strips_comment_and_returns_entry_count() {
    let mut bytes = archive(&["a", "b"], b"comment");
    assert_eq!(prepare_zip_bytes(&mut bytes, 2), Ok(2));
    assert_eq!(find_eocd(&bytes, 2), Ok((bytes.len() - EOCD_MIN_BYTES, 2)));
}

#[test]
fn comment_signature_never_replaces_the_real_footer() {
    let mut fake = [0u8; EOCD_MIN_BYTES];
    fake[..4].copy_from_slice(b"PK\x05\x06");
    fake[8..10].copy_from_slice(&9u16.to_le_bytes());
    fake[10..12].copy_from_slice(&9u16.to_le_bytes());
    let bytes = archive(&["a"], &fake);
    assert_eq!(find_eocd(&bytes, 1).map(|(_, entries)| entries), Ok(1));
}

#[test]
fn rejects_entry_counts_over_the_limit_and_zip64_sentinels() {
    let mut bytes = archive(&["a", "b"], b"");
    assert_eq!(find_eocd(&bytes, 1), Err(FooterError::TooManyEntries));
    let footer = bytes.len() - EOCD_MIN_BYTES;
    bytes[footer + 10..footer + 12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(find_eocd(&bytes, 1), Err(FooterError::Zip64));
    assert_eq!(find_eocd(b"PK\x05\x06", 1), Err(FooterError::Invalid));
}
