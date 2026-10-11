//! Stable identities for block states carried by moving-block actors.

use sha2::{Digest, Sha256};

/// A typed value in a canonical block state.
pub enum StateValue<'a> {
    Byte(i8),
    Int(i32),
    String(&'a str),
}

/// Hashes a name and state values supplied in ascending key order.
pub fn state_identity<'a>(
    name: &str,
    states: impl IntoIterator<Item = (&'a str, StateValue<'a>)>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    write_text(&mut digest, name);
    for (key, value) in states {
        write_text(&mut digest, key);
        match value {
            StateValue::Byte(value) => digest.update([1, value as u8]),
            StateValue::Int(value) => {
                digest.update([3]);
                digest.update(value.to_le_bytes());
            }
            StateValue::String(value) => {
                digest.update([8]);
                write_text(&mut digest, value);
            }
        }
    }
    digest.finalize().into()
}

/// Encodes a length-prefixed string without allocating a temporary byte buffer.
fn write_text(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value.as_bytes());
}
