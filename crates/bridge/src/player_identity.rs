//! Portable player identifier validation shared by roster and account services.
/// Accepts decimal Xbox identifiers within the API's wire bound.
pub fn valid_cinnabar_xuid(xuid: &str) -> bool {
    !xuid.is_empty() && xuid.len() <= 20 && xuid.bytes().all(|b| b.is_ascii_digit())
}

/// Accepts only the API's lowercase, hyphenated UUID representation.
pub fn valid_cinnabar_uuid(uuid: &str) -> bool {
    uuid.len() == 36
        && uuid.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
