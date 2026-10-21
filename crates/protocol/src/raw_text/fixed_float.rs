//! Stack storage for fixed-precision numeric substitutions.

// Sign + 309 integer digits + decimal point + precision 0..9 for finite f64.
pub(super) const MAX_FIXED_FLOAT_BYTES: usize = 320;

pub(super) struct FixedFloatText {
    bytes: [u8; MAX_FIXED_FLOAT_BYTES],
    len: usize,
}

impl Default for FixedFloatText {
    fn default() -> Self {
        Self {
            bytes: [0; MAX_FIXED_FLOAT_BYTES],
            len: 0,
        }
    }
}

impl FixedFloatText {
    /// Borrows the complete UTF-8 written by the formatter.
    pub(super) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("formatter writes valid UTF-8")
    }
}

impl std::fmt::Write for FixedFloatText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(std::fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        destination.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
