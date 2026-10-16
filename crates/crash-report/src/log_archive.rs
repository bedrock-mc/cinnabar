//! Parses timestamped log archives so writers and readers use the same ordering.

/// Returns the UTC stamp and numeric collision suffix of an archive in this log family.
/// Unrelated files and malformed names are excluded.
pub fn key(name: &str, stem: &str, ext: &str) -> Option<(String, u32)> {
    let rest = name
        .strip_prefix(stem)?
        .strip_prefix('-')?
        .strip_suffix(ext)?
        .strip_suffix('.')?;
    let stamp_len = crate::utc::STAMP_LEN;
    let (stamp, suffix) = match rest.get(stamp_len..) {
        Some("") => (rest, 0),
        Some(tail) => (&rest[..stamp_len], tail.strip_prefix('-')?.parse().ok()?),
        None => return None,
    };
    let valid = stamp.as_bytes().iter().enumerate().all(|(index, byte)| {
        if index == 8 {
            *byte == b'-'
        } else {
            byte.is_ascii_digit()
        }
    });
    valid.then(|| (stamp.to_owned(), suffix))
}
