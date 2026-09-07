const MAX_WALK_DEPTH: usize = 16;
const MAX_WALK_TAGS: usize = 4_096;
const MAX_COLLECTION_VALUES: usize = 4_096;

pub(super) const CANONICAL_EMPTY_COMPONENT_DATA: &[u8] = &[10, 0, 0];

#[derive(Clone, Copy)]
enum Scope {
    Root,
    Components,
    ItemProperties,
    Other,
}

#[derive(Default)]
struct Evidence {
    components: usize,
    item_properties: usize,
    max_stack_size: usize,
    value: Option<u8>,
    invalid: bool,
    tags: usize,
}

impl Evidence {
    fn visit_tag(&mut self) -> Result<(), ()> {
        self.tags = self.tags.checked_add(1).ok_or(())?;
        if self.tags > MAX_WALK_TAGS {
            Err(())
        } else {
            Ok(())
        }
    }

    fn finish(self) -> Option<u8> {
        (!self.invalid && self.max_stack_size == 1)
            .then_some(self.value)
            .flatten()
    }
}

/// Extracts only the reviewed stack-size path from NetworkLittleEndian NBT.
///
/// The registry packet remains authoritative raw evidence. Unsupported depth,
/// excess work, duplicate path segments, or a semantically unusable value make
/// this optional projection unavailable without changing packet admission.
pub(super) fn negotiated_max_stack_size(bytes: &[u8]) -> Option<u8> {
    let mut reader = Reader::new(bytes);
    let root_tag = reader.read_u8().ok()?;
    let _root_name = reader.read_string().ok()?;
    if root_tag != 10 {
        return None;
    }

    let mut evidence = Evidence::default();
    evidence.visit_tag().ok()?;
    scan_compound(&mut reader, &mut evidence, Scope::Root, 1).ok()?;
    if !reader.is_finished() {
        return None;
    }
    evidence.finish()
}

fn scan_compound(
    reader: &mut Reader<'_>,
    evidence: &mut Evidence,
    scope: Scope,
    depth: usize,
) -> Result<(), ()> {
    if depth > MAX_WALK_DEPTH {
        return Err(());
    }
    loop {
        let tag = reader.read_u8()?;
        if tag == 0 {
            return Ok(());
        }
        evidence.visit_tag()?;
        let name = reader.read_string()?;

        let child_scope = match scope {
            Scope::Root if name == b"components" => {
                evidence.components += 1;
                if evidence.components != 1 || tag != 10 {
                    evidence.invalid = true;
                }
                if tag == 10 {
                    Scope::Components
                } else {
                    Scope::Other
                }
            }
            Scope::Components if name == b"item_properties" => {
                evidence.item_properties += 1;
                if evidence.item_properties != 1 || tag != 10 {
                    evidence.invalid = true;
                }
                if tag == 10 {
                    Scope::ItemProperties
                } else {
                    Scope::Other
                }
            }
            Scope::ItemProperties if name == b"max_stack_size" => {
                evidence.max_stack_size += 1;
                if evidence.max_stack_size != 1 || tag != 3 {
                    evidence.invalid = true;
                }
                if tag == 3 {
                    let value = reader.read_zigzag_i32()?;
                    evidence.value = u8::try_from(value).ok().filter(|value| *value != 0);
                    if evidence.value.is_none() {
                        evidence.invalid = true;
                    }
                    continue;
                }
                Scope::Other
            }
            _ => Scope::Other,
        };
        scan_payload(tag, reader, evidence, child_scope, depth)?;
    }
}

fn scan_payload(
    tag: u8,
    reader: &mut Reader<'_>,
    evidence: &mut Evidence,
    scope: Scope,
    depth: usize,
) -> Result<(), ()> {
    match tag {
        1 => reader.skip(1),
        2 => reader.skip(2),
        3 => reader.read_zigzag_i32().map(|_| ()),
        4 => reader.read_zigzag_i64().map(|_| ()),
        5 => reader.skip(4),
        6 => reader.skip(8),
        7 => {
            let length = reader.read_length()?;
            bounded_collection(length)?;
            reader.skip(length)
        }
        8 => reader.read_string().map(|_| ()),
        9 => {
            let nested_depth = depth.checked_add(1).ok_or(())?;
            if nested_depth > MAX_WALK_DEPTH {
                return Err(());
            }
            let element_tag = reader.read_u8()?;
            let length = reader.read_length()?;
            bounded_collection(length)?;
            if element_tag == 0 && length != 0 {
                return Err(());
            }
            for _ in 0..length {
                evidence.visit_tag()?;
                scan_payload(element_tag, reader, evidence, Scope::Other, nested_depth)?;
            }
            Ok(())
        }
        10 => scan_compound(reader, evidence, scope, depth.checked_add(1).ok_or(())?),
        11 => {
            let length = reader.read_length()?;
            bounded_collection(length)?;
            for _ in 0..length {
                reader.read_zigzag_i32()?;
            }
            Ok(())
        }
        12 => {
            let length = reader.read_length()?;
            bounded_collection(length)?;
            for _ in 0..length {
                reader.read_zigzag_i64()?;
            }
            Ok(())
        }
        _ => Err(()),
    }
}

fn bounded_collection(length: usize) -> Result<(), ()> {
    if length > MAX_COLLECTION_VALUES {
        Err(())
    } else {
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_u8(&mut self) -> Result<u8, ()> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_string(&mut self) -> Result<&'a [u8], ()> {
        let length = usize::try_from(self.read_var_u32()?).map_err(|_| ())?;
        self.read_exact(length)
    }

    fn read_length(&mut self) -> Result<usize, ()> {
        usize::try_from(self.read_zigzag_i32()?).map_err(|_| ())
    }

    fn read_zigzag_i32(&mut self) -> Result<i32, ()> {
        let value = self.read_var_u32()?;
        Ok(((value >> 1) as i32) ^ -((value & 1) as i32))
    }

    fn read_zigzag_i64(&mut self) -> Result<i64, ()> {
        let value = self.read_var_u64()?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }

    fn read_var_u32(&mut self) -> Result<u32, ()> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.read_u8()?;
            if shift == 28 && byte & 0xf0 != 0 {
                return Err(());
            }
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(())
    }

    fn read_var_u64(&mut self) -> Result<u64, ()> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = self.read_u8()?;
            if shift == 63 && byte & 0xfe != 0 {
                return Err(());
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(())
    }

    fn skip(&mut self, length: usize) -> Result<(), ()> {
        self.read_exact(length).map(|_| ())
    }

    fn read_exact(&mut self, length: usize) -> Result<&'a [u8], ()> {
        let end = self.offset.checked_add(length).ok_or(())?;
        let bytes = self.bytes.get(self.offset..end).ok_or(())?;
        self.offset = end;
        Ok(bytes)
    }
}
