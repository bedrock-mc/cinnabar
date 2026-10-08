//! Lowercase hex, the protocol's only byte encoding.

use std::fmt;

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Why a string is not canonical lowercase hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexError {
    OddLength,
    InvalidDigit { index: usize },
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OddLength => f.write_str("hex has an odd length"),
            Self::InvalidDigit { index } => {
                write!(f, "byte {index} is not a lowercase hex digit")
            }
        }
    }
}

impl std::error::Error for HexError {}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Decodes lowercase hex; uppercase digits, odd lengths and other characters are errors.
pub fn decode(text: &str) -> Result<Vec<u8>, HexError> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(HexError::OddLength);
    }
    let digit = |index: usize| match bytes[index] {
        c @ b'0'..=b'9' => Ok(c - b'0'),
        c @ b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(HexError::InvalidDigit { index }),
    };
    (0..bytes.len())
        .step_by(2)
        .map(|index| Ok((digit(index)? << 4) | digit(index + 1)?))
        .collect()
}
