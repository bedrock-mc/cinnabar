//! Borrowed network-NBT fields and lossless item-tag conversion for trade offers.

use bedrock_protocol_1_26_51::wire::{NbtVariant, Reader};

#[derive(Clone, Copy)]
pub(super) struct Value<'a> {
    tag: u8,
    payload: &'a [u8],
}

pub(super) struct Compound<'a>(Vec<(&'a [u8], Value<'a>)>);

impl<'a> Compound<'a> {
    /// Whether this compound contains no fields at all.
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Borrows the fields of one validated network compound, excluding its name.
    fn read(reader: &mut Reader<'a>) -> Option<Self> {
        let mut fields = Vec::new();
        while !reader.is_empty() {
            let bytes = reader.read_nbt(NbtVariant::Network).ok()?;
            let mut field = Reader::new(bytes);
            let tag = field.read_u8().ok()?;
            if tag == 0 {
                return Some(Self(fields));
            }
            let name = text(&mut field)?;
            let payload = field.take(field.remaining()).ok()?;
            fields.push((name, Value { tag, payload }));
        }
        None
    }

    /// Reads a named root compound without imposing a second NBT size or depth budget.
    pub(super) fn root(bytes: &'a [u8]) -> Option<Self> {
        let mut reader = Reader::new(bytes);
        if reader.read_u8().ok()? != 10 {
            return None;
        }
        text(&mut reader)?;
        let root = Self::read(&mut reader)?;
        reader.is_empty().then_some(root)
    }

    /// Returns a uniquely named field; ambiguous duplicate fields remain unsupported.
    pub(super) fn field(&self, name: &str) -> Option<Value<'a>> {
        let mut fields = self.0.iter().filter(|(key, _)| *key == name.as_bytes());
        let value = fields.next()?.1;
        fields.next().is_none().then_some(value)
    }

    /// Reads a numeric field, using the caller's default when it is absent.
    pub(super) fn number(&self, name: &str, default: f64) -> f64 {
        self.field(name).and_then(Value::number).unwrap_or(default)
    }

    /// Builds item user data, retaining saved placement and destruction restrictions.
    pub(super) fn item_data(&self) -> Option<Vec<u8>> {
        let mut output = self
            .field("tag")
            .map(Value::item_data)
            .unwrap_or_else(|| Some(vec![0; 10]))?;
        output.truncate(output.len().checked_sub(8)?);
        for name in ["CanPlaceOn", "CanDestroy"] {
            let strings = self
                .field(name)
                .map(Value::strings)
                .unwrap_or_else(|| Some(Vec::new()))?;
            output.extend_from_slice(&i32::try_from(strings.len()).ok()?.to_le_bytes());
            for text in strings {
                output.extend_from_slice(&u16::try_from(text.len()).ok()?.to_le_bytes());
                output.extend_from_slice(text);
            }
        }
        Some(output)
    }
}

impl<'a> Value<'a> {
    /// Retains a validated compound's complete network payload without rewriting its fields.
    pub(super) fn compound_bytes(self) -> Option<&'a [u8]> {
        (self.tag == 10).then_some(self.payload)
    }

    /// Borrows a compound payload while retaining all of its unknown fields.
    pub(super) fn compound(self) -> Option<Compound<'a>> {
        (self.tag == 10).then_some(())?;
        Compound::read(&mut Reader::new(self.payload))
    }

    /// Visits every compound in a list in its original order.
    pub(super) fn compounds(self) -> Option<Vec<Compound<'a>>> {
        (self.tag == 9).then_some(())?;
        let mut reader = Reader::new(self.payload);
        let tag = reader.read_u8().ok()?;
        let count = length(&mut reader)?;
        if count != 0 && tag != 10 {
            return None;
        }
        (0..count).map(|_| Compound::read(&mut reader)).collect()
    }

    /// Reads the numeric kinds used by offer counts, prices, tiers and network IDs.
    pub(super) fn number(self) -> Option<f64> {
        let mut reader = Reader::new(self.payload);
        Some(match self.tag {
            1 => f64::from(reader.read_u8().ok()? as i8),
            2 => f64::from(i16::from_le_bytes(reader.read_bytes().ok()?)),
            3 => f64::from(reader.read_zigzag_i32().ok()?),
            4 => reader.read_zigzag_i64().ok()? as f64,
            5 => f64::from(f32::from_le_bytes(reader.read_bytes().ok()?)),
            6 => f64::from_le_bytes(reader.read_bytes().ok()?),
            _ => return None,
        })
    }

    /// Reads an Int tag without losing the signed representation of a 32-bit network ID.
    pub(super) fn int32(self) -> Option<i32> {
        (self.tag == 3).then_some(())?;
        Reader::new(self.payload).read_zigzag_i32().ok()
    }

    /// Reads the saved item's unsigned byte count, also accepting integral wider counts.
    pub(super) fn count(self) -> Option<u16> {
        if self.tag == 1 {
            return Some(u16::from(*self.payload.first()?));
        }
        let count = self.number()?;
        (count.is_finite() && count.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&count))
            .then_some(count as u16)
    }

    /// Borrows a UTF-8 string field.
    pub(super) fn string(self) -> Option<&'a str> {
        (self.tag == 8).then_some(())?;
        std::str::from_utf8(text(&mut Reader::new(self.payload))?).ok()
    }

    /// Borrows a saved item's string-list payload without discarding restrictions.
    fn strings(self) -> Option<Vec<&'a [u8]>> {
        (self.tag == 9).then_some(())?;
        let mut reader = Reader::new(self.payload);
        let tag = reader.read_u8().ok()?;
        let count = length(&mut reader)?;
        if count != 0 && tag != 8 {
            return None;
        }
        (0..count).map(|_| text(&mut reader)).collect()
    }

    /// Converts a retained compound into the fixed little-endian item-data envelope.
    pub(super) fn item_data(self) -> Option<Vec<u8>> {
        (self.tag == 10).then_some(())?;
        let mut output = vec![255, 255, 1, 10, 0, 0];
        transcode(&mut Reader::new(self.payload), 10, &mut output)?;
        output.extend_from_slice(&[0; 8]);
        Some(output)
    }
}

/// Borrows a network length-prefixed string without UTF-8 coercion.
fn text<'a>(reader: &mut Reader<'a>) -> Option<&'a [u8]> {
    let length = reader.read_var_u32().ok()? as usize;
    reader.take(length).ok()
}

/// Reads a nonnegative list or array length bounded by the remaining packet bytes.
fn length(reader: &mut Reader<'_>) -> Option<usize> {
    let count = usize::try_from(reader.read_zigzag_i32().ok()?).ok()?;
    (count <= reader.remaining()).then_some(count)
}

/// Writes a fixed little-endian string, preserving its original bytes.
fn write_text(reader: &mut Reader<'_>, output: &mut Vec<u8>) -> Option<()> {
    let bytes = text(reader)?;
    output.extend_from_slice(&u16::try_from(bytes.len()).ok()?.to_le_bytes());
    output.extend_from_slice(bytes);
    Some(())
}

/// Transcodes every NBT payload kind, including unknown item tags and typed empty lists.
fn transcode(reader: &mut Reader<'_>, tag: u8, output: &mut Vec<u8>) -> Option<()> {
    match tag {
        1 | 2 | 5 | 6 => {
            let size = match tag {
                1 => 1,
                2 => 2,
                5 => 4,
                _ => 8,
            };
            output.extend_from_slice(reader.take(size).ok()?);
        }
        3 => output.extend_from_slice(&reader.read_zigzag_i32().ok()?.to_le_bytes()),
        4 => output.extend_from_slice(&reader.read_zigzag_i64().ok()?.to_le_bytes()),
        8 => write_text(reader, output)?,
        7 | 9 | 11 | 12 => {
            let element = if tag == 9 {
                reader.read_u8().ok()?
            } else {
                match tag {
                    7 => 1,
                    11 => 3,
                    _ => 4,
                }
            };
            if tag == 9 {
                output.push(element);
            }
            let count = length(reader)?;
            output.extend_from_slice(&i32::try_from(count).ok()?.to_le_bytes());
            for _ in 0..count {
                transcode(reader, element, output)?;
            }
        }
        10 => loop {
            let child = reader.read_u8().ok()?;
            output.push(child);
            if child == 0 {
                break;
            }
            write_text(reader, output)?;
            transcode(reader, child, output)?;
        },
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn network_identity_keeps_the_high_bit_of_its_signed_nbt_field() {
        let value = super::Compound::root(&[10, 0, 3, 1, b'n', 1, 0]).unwrap();
        assert_eq!(value.field("n").unwrap().int32().unwrap() as u32, u32::MAX);
    }

    #[test]
    fn saved_item_byte_counts_are_unsigned() {
        let value = super::Compound::root(&[10, 0, 1, 1, b'n', 128, 0]).unwrap();
        assert_eq!(value.field("n").unwrap().count(), Some(128));
    }
}
