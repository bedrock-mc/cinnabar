//! Bounded projections of typed facts from network registry NBT.

const MAX_WALK_DEPTH: usize = 16;
const MAX_WALK_TAGS: usize = 4_096;
const MAX_COLLECTION_VALUES: usize = 4_096;

pub(super) const CANONICAL_EMPTY_COMPONENT_DATA: &[u8] = &[10, 0, 0];
pub(super) const CREATIVE_CATEGORY_PROPERTY: &str = "creative_category";

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
    creative_category_fields: usize,
    creative_category: u8,
    creative_category_invalid: bool,
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
    registry_properties(bytes)?.finish()
}

/// Reads the Int category's low byte, defaulting to zero within a valid properties compound.
/// Missing or ambiguous ancestors and duplicate category fields stay unresolved.
pub(super) fn negotiated_creative_category(bytes: &[u8]) -> Option<u8> {
    let evidence = registry_properties(bytes)?;
    (!evidence.creative_category_invalid
        && evidence.components == 1
        && evidence.item_properties == 1)
        .then_some(evidence.creative_category)
}

/// Walks one complete registry payload under the shared tag, collection and depth limits.
fn registry_properties(bytes: &[u8]) -> Option<Evidence> {
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
    Some(evidence)
}

/// Bounds on declared item tags retained per registry entry.
const MAX_DECLARED_TAGS: usize = 64;
const MAX_TAG_BYTES: usize = 256;

pub(super) const ITEM_TAGS_FIELD: &str = "item_tags";
pub(super) const TAGS_COMPONENT: &str = "minecraft:tags";
pub(super) const TAGS_LIST: &str = "tags";

/// Unions the supported legacy and authored component tag declarations.
/// Unusable shapes declare nothing rather than failing admission.
pub(super) fn declared_item_tags(bytes: &[u8]) -> Vec<std::sync::Arc<str>> {
    let mut reader = Reader::new(bytes);
    let mut tags = Vec::new();
    let mut evidence = Evidence::default();
    let found = (|| -> Result<(), ()> {
        if reader.read_u8()? != 10 {
            return Err(());
        }
        reader.read_string()?;
        find_child(
            &mut reader,
            &mut evidence,
            b"components",
            10,
            |reader, evidence| read_component_tags(reader, evidence, &mut tags),
        )
    })();
    if found.is_err() {
        tags.clear();
    }
    tags
}

/// Reads the first declaration of each tag encoding within one component compound.
fn read_component_tags(
    reader: &mut Reader<'_>,
    evidence: &mut Evidence,
    tags: &mut Vec<std::sync::Arc<str>>,
) -> Result<(), ()> {
    let mut legacy = false;
    let mut authored = false;
    loop {
        let child = reader.read_u8()?;
        if child == 0 {
            return Ok(());
        }
        evidence.visit_tag()?;
        let name = reader.read_string()?;
        if child == 9 && name == ITEM_TAGS_FIELD.as_bytes() && !legacy {
            legacy = true;
            read_tag_list(reader, tags)?;
        } else if child == 10 && name == TAGS_COMPONENT.as_bytes() && !authored {
            authored = true;
            find_child(reader, evidence, TAGS_LIST.as_bytes(), 9, |reader, _| {
                read_tag_list(reader, tags)
            })?;
        } else {
            scan_payload(child, reader, evidence, Scope::Other, 1)?;
        }
    }
}

/// Retains bounded namespaced strings, applying the entry limit across both encodings.
fn read_tag_list(reader: &mut Reader<'_>, tags: &mut Vec<std::sync::Arc<str>>) -> Result<(), ()> {
    if reader.read_u8()? != 8 {
        return Err(());
    }
    let length = reader.read_length()?;
    if length > MAX_DECLARED_TAGS - tags.len() {
        return Err(());
    }
    for _ in 0..length {
        let tag = std::str::from_utf8(reader.read_string()?).map_err(|_| ())?;
        if tag.len() > MAX_TAG_BYTES || !tag.contains(':') {
            return Err(());
        }
        tags.push(std::sync::Arc::from(tag));
    }
    Ok(())
}

/// Scans one compound body for the first child `name` of type `tag`, handing
/// its payload to `visit` and skipping every other child.
fn find_child(
    reader: &mut Reader<'_>,
    evidence: &mut Evidence,
    name: &[u8],
    tag: u8,
    mut visit: impl FnMut(&mut Reader<'_>, &mut Evidence) -> Result<(), ()>,
) -> Result<(), ()> {
    let mut visited = false;
    loop {
        let child = reader.read_u8()?;
        if child == 0 {
            return Ok(());
        }
        evidence.visit_tag()?;
        let child_name = reader.read_string()?;
        if !visited && child == tag && child_name == name {
            visited = true;
            visit(reader, evidence)?;
        } else {
            scan_payload(child, reader, evidence, Scope::Other, 1)?;
        }
    }
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
                    evidence.creative_category_invalid = true;
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
                    evidence.creative_category_invalid = true;
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
            Scope::ItemProperties if name == CREATIVE_CATEGORY_PROPERTY.as_bytes() => {
                evidence.creative_category_fields += 1;
                if evidence.creative_category_fields != 1 {
                    evidence.creative_category_invalid = true;
                }
                if tag == 3 {
                    evidence.creative_category = reader.read_zigzag_i32()? as u8;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn string(out: &mut Vec<u8>, value: &str) {
        out.push(value.len() as u8);
        out.extend_from_slice(value.as_bytes());
    }

    /// Writes a nonnegative list length through the production network-NBT codec.
    fn list_length(out: &mut Vec<u8>, count: usize) {
        bedrock_protocol_1_26_51::wire::Writer::new(out)
            .write_zigzag_i32(i32::try_from(count).unwrap())
            .unwrap();
    }

    fn component_data(tags: &[&str]) -> Vec<u8> {
        let mut out = vec![10, 0];
        out.push(10);
        string(&mut out, "components");
        out.push(1);
        string(&mut out, "other");
        out.push(1);
        out.push(9);
        string(&mut out, "item_tags");
        out.push(8);
        list_length(&mut out, tags.len());
        for tag in tags {
            string(&mut out, tag);
        }
        out.extend_from_slice(&[0, 0]);
        out
    }

    /// Declared tags come from `components.item_tags`; malformed data
    /// declares nothing.
    #[test]
    fn component_item_tags_are_extracted_or_empty() {
        let tags = declared_item_tags(&component_data(&["minecraft:planks", "custom:wood"]));
        assert_eq!(
            tags.iter().map(|tag| &**tag).collect::<Vec<_>>(),
            ["minecraft:planks", "custom:wood"]
        );
        assert!(declared_item_tags(&component_data(&["no_namespace"])).is_empty());
        assert!(declared_item_tags(CANONICAL_EMPTY_COMPONENT_DATA).is_empty());
        let mut truncated = component_data(&["minecraft:planks"]);
        truncated.truncate(truncated.len() - 3);
        assert!(declared_item_tags(&truncated).is_empty());
    }

    /// Adds authored tags beside the legacy list in an original network-NBT fixture.
    fn both_encodings(legacy: &[&str], authored: &[&str]) -> Vec<u8> {
        let mut bytes = component_data(legacy);
        bytes.truncate(bytes.len() - 2);
        bytes.push(10);
        string(&mut bytes, TAGS_COMPONENT);
        bytes.push(9);
        string(&mut bytes, TAGS_LIST);
        bytes.push(8);
        list_length(&mut bytes, authored.len());
        for tag in authored {
            string(&mut bytes, tag);
        }
        bytes.extend([0, 0, 0]);
        bytes
    }

    #[test]
    fn both_component_tag_encodings_share_admission_and_entry_bounds() {
        let bytes = both_encodings(&["minecraft:is_sword"], &["minecraft:is_spear"]);
        assert_eq!(
            declared_item_tags(&bytes)
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["minecraft:is_sword", "minecraft:is_spear"]
        );
        assert!(declared_item_tags(&both_encodings(&[], &["no_namespace"])).is_empty());
        assert!(
            declared_item_tags(&both_encodings(
                &["minecraft:is_sword"; MAX_DECLARED_TAGS],
                &["minecraft:is_spear"]
            ))
            .is_empty()
        );
        let mut truncated = bytes;
        truncated.truncate(truncated.len() - 3);
        assert!(declared_item_tags(&truncated).is_empty());
    }
}
