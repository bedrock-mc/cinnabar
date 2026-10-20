//! The pinned Go username predicate: a bounded UTF8 byte string containing space or category L.

use super::unicode_generated::LETTER_RANGES;

/// Preserves the original unanchored category predicate rather than imposing a character whitelist.
pub(super) fn valid(name: &str) -> bool {
    (1..=16).contains(&name.len())
        && name.chars().any(|character| {
            if character == ' ' {
                return true;
            }
            let code = u32::from(character);
            let index = LETTER_RANGES.partition_point(|range| range.1 < code);
            LETTER_RANGES
                .get(index)
                .is_some_and(|&(low, high, stride)| {
                    low <= code && code <= high && (code - low) % stride == 0
                })
        })
}
