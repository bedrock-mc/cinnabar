//! A bounded tree reader for network little-endian NBT, keeping only the
//! scalar kinds definitions use.

/// Nesting a definition may use before it is treated as malformed.
const MAX_NBT_DEPTH: usize = 32;

/// Reads a named root compound; `None` for truncation or excess nesting.
pub(crate) fn read_root(bytes: &[u8]) -> Option<Nbt> {
    let mut reader = NbtReader { bytes, position: 0 };
    if reader.u8()? != TAG_COMPOUND {
        return None;
    }
    reader.string()?;
    reader.payload(TAG_COMPOUND, 0)
}

const TAG_COMPOUND: u8 = 10;

#[derive(Debug)]
pub(crate) enum Nbt {
    Byte(i8),
    Int(i64),
    Float(f64),
    String(String),
    Other,
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
}

impl Nbt {
    pub(crate) fn field(&self, name: &str) -> Option<&Nbt> {
        match self {
            Self::Compound(fields) => fields
                .iter()
                .find_map(|(key, value)| (key == name).then_some(value)),
            _ => None,
        }
    }

    pub(crate) fn list(&self, name: &str) -> &[Nbt] {
        match self.field(name) {
            Some(Self::List(items)) => items,
            _ => &[],
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn number(&self) -> Option<f64> {
        match self {
            Self::Byte(value) => Some(f64::from(*value)),
            Self::Int(value) => Some(*value as f64),
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }
}

fn zigzag(raw: u64) -> i64 {
    ((raw >> 1) as i64) ^ -((raw & 1) as i64)
}

/// Network little-endian NBT: VarInt lengths and zigzag VarInt ints/longs.
struct NbtReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl NbtReader<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let bytes = self
            .bytes
            .get(self.position..self.position.checked_add(count)?)?;
        self.position += count;
        Some(bytes)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn var_u64(&mut self, max_bytes: usize) -> Option<u64> {
        let mut value = 0_u64;
        for index in 0..max_bytes {
            let byte = self.u8()?;
            let bits = if max_bytes == 5 { u32::BITS } else { u64::BITS };
            let shift = index * 7;
            let payload = u64::from(byte & 0x7f);
            if shift >= bits as usize || payload > (u64::MAX >> (u64::BITS - bits)) >> shift {
                return None;
            }
            value |= payload << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn length(&mut self) -> Option<usize> {
        let raw = self.var_u64(5)? as u32;
        let value = ((raw >> 1) as i32) ^ -((raw & 1) as i32);
        let length = usize::try_from(value).ok()?;
        (length <= self.bytes.len() - self.position).then_some(length)
    }

    fn string(&mut self) -> Option<String> {
        let length = usize::try_from(self.var_u64(5)?).ok()?;
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }

    fn payload(&mut self, tag: u8, depth: usize) -> Option<Nbt> {
        if depth > MAX_NBT_DEPTH {
            return None;
        }
        Some(match tag {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => {
                let bytes = self.take(2)?;
                Nbt::Int(i64::from(i16::from_le_bytes([bytes[0], bytes[1]])))
            }
            3 => Nbt::Int(zigzag(self.var_u64(5)?)),
            4 => Nbt::Int(zigzag(self.var_u64(10)?)),
            5 => Nbt::Float(f64::from(f32::from_le_bytes(
                self.take(4)?.try_into().ok()?,
            ))),
            6 => Nbt::Float(f64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            7 => {
                let length = self.length()?;
                self.take(length).map(|_| Nbt::Other)?
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let element = self.u8()?;
                let length = self.length()?;
                let items = (0..length)
                    .map(|_| self.payload(element, depth + 1))
                    .collect::<Option<Vec<_>>>()?;
                Nbt::List(items)
            }
            TAG_COMPOUND => {
                let mut fields = Vec::new();
                loop {
                    let child = self.u8()?;
                    if child == 0 {
                        break Nbt::Compound(fields);
                    }
                    let name = self.string()?;
                    fields.push((name, self.payload(child, depth + 1)?));
                }
            }
            11 => {
                let length = self.length()?;
                for _ in 0..length {
                    self.var_u64(5)?;
                }
                Nbt::Other
            }
            12 => {
                let length = self.length()?;
                for _ in 0..length {
                    self.var_u64(10)?;
                }
                Nbt::Other
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_nbt_varints_reject_unused_high_bits() {
        let bytes = [10, 0, 9, 1, 120, 1, 128, 128, 128, 128, 16, 0];
        assert!(read_root(&bytes).is_none());
        let mut reader = NbtReader {
            bytes: &[128, 128, 128, 128, 128, 128, 128, 128, 128, 2],
            position: 0,
        };
        assert!(reader.var_u64(10).is_none());
        let mut valid = NbtReader {
            bytes: &[255, 255, 255, 255, 255, 255, 255, 255, 255, 1],
            position: 0,
        };
        assert_eq!(valid.var_u64(10), Some(u64::MAX));
    }
}
