use super::budget::{MAX_STRING_BYTES, MAX_STRING_TOTAL_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    Wire,
    Policy,
}
pub(super) type ReadResult<T> = Result<T, Refusal>;

/// No allocation and no recursion. Every declared loop/copy has a policy charge.
pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    nodes: usize,
    strings: usize,
    opaque: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> ReadResult<Self> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(Refusal::Policy);
        }
        Ok(Self {
            bytes,
            offset: 0,
            nodes: 0,
            strings: 0,
            opaque: 0,
        })
    }
    pub(super) fn take(&mut self, count: usize) -> ReadResult<&'a [u8]> {
        let end = self.offset.checked_add(count).ok_or(Refusal::Wire)?;
        let value = self.bytes.get(self.offset..end).ok_or(Refusal::Wire)?;
        self.offset = end;
        Ok(value)
    }
    pub(super) fn byte(&mut self) -> ReadResult<u8> {
        Ok(self.take(1)?[0])
    }
    pub(super) fn uint(&mut self) -> ReadResult<u32> {
        let mut result = 0;
        for shift in (0..35).step_by(7) {
            let byte = self.byte()?;
            if shift == 28 && byte > 15 {
                return Err(Refusal::Wire);
            }
            result |= u32::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return Ok(result);
            }
        }
        Err(Refusal::Wire)
    }
    pub(super) fn int(&mut self) -> ReadResult<i32> {
        let value = self.uint()?;
        Ok(((value >> 1) as i32) ^ -((value & 1) as i32))
    }
    pub(super) fn count(&mut self, maximum: usize) -> ReadResult<usize> {
        let count = self.uint()? as usize;
        self.nodes = self.nodes.checked_add(count).ok_or(Refusal::Policy)?;
        if count > maximum || self.nodes > 262144 {
            return Err(Refusal::Policy);
        }
        Ok(count)
    }
    /// Reads exact UTF-8 text, preserving invalid text as an unsupported value.
    pub(super) fn text(&mut self) -> ReadResult<Option<&'a str>> {
        let count = self.uint()? as usize;
        if count > MAX_STRING_BYTES {
            return Err(Refusal::Policy);
        }
        self.strings = self.strings.checked_add(count).ok_or(Refusal::Policy)?;
        if self.strings > MAX_STRING_TOTAL_BYTES {
            return Err(Refusal::Policy);
        }
        Ok(std::str::from_utf8(self.take(count)?).ok())
    }
    /// Reads ingredient text with the existing unsupported-empty fallback.
    pub(super) fn string(&mut self) -> ReadResult<&'a str> {
        Ok(self.text()?.unwrap_or(""))
    }
    pub(super) fn opaque(&mut self) -> ReadResult<&'a [u8]> {
        let count = self.uint()? as usize;
        if count > 65536 {
            return Err(Refusal::Policy);
        }
        self.opaque = self.opaque.checked_add(count).ok_or(Refusal::Policy)?;
        if self.opaque > 1024 * 1024 {
            return Err(Refusal::Policy);
        }
        self.take(count)
    }
    pub(super) fn finish(&self) -> ReadResult<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(Refusal::Wire)
        }
    }
    pub(super) fn string_bytes(&self) -> usize {
        self.strings
    }
}
