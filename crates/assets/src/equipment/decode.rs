//! Carrier decoding and immutable texture-buffer sharing.

use super::*;

impl RuntimeEquipmentCatalog {
    /// Validates a carrier and retains its decoded texture pixels.
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        Self::decode_with_shared_pixels(bytes, |_| None)
    }

    /// Reuses equal immutable pixels while decoding, avoiding a second live allocation.
    pub(crate) fn decode_with_shared_pixels(
        bytes: &[u8],
        mut shared: impl FnMut(&[u8]) -> Option<Arc<[u8]>>,
    ) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_EQUIPMENT_CARRIER_BYTES {
            return Err(invalid("equipment carrier size is out of bounds"));
        }
        if bytes[..8] != EQUIPMENT_CARRIER_MAGIC
            || u32::from_le_bytes(field::<4>(bytes, 8)?) != EQUIPMENT_CARRIER_VERSION
        {
            return Err(invalid(
                "unsupported equipment carrier header; rebuild with make assets",
            ));
        }
        let payload_bytes = usize::try_from(u64::from_le_bytes(field::<8>(bytes, 12)?))
            .map_err(|_| invalid("equipment payload size exceeds platform"))?;
        if payload_bytes > MAX_EQUIPMENT_METADATA_BYTES {
            return Err(invalid("equipment metadata exceeds bound"));
        }
        let hash_start = bytes.len() - HASH_BYTES;
        let payload_end = HEADER_BYTES
            .checked_add(payload_bytes)
            .filter(|end| *end <= hash_start)
            .ok_or_else(|| invalid("noncanonical equipment carrier layout"))?;
        if Sha256::digest(&bytes[..hash_start]).as_slice() != &bytes[hash_start..] {
            return Err(invalid("equipment carrier envelope hash mismatch"));
        }
        let payload: EquipmentCatalogPayload =
            serde_json::from_slice(&bytes[HEADER_BYTES..payload_end])
                .map_err(|_| invalid("invalid equipment carrier payload"))?;
        let canonical = serde_json::to_vec(&payload)
            .map_err(|_| invalid("failed to canonicalize equipment payload"))?;
        if canonical.as_slice() != &bytes[HEADER_BYTES..payload_end] {
            return Err(invalid("noncanonical equipment payload encoding"));
        }
        validate(
            &payload.source_manifest_sha256,
            &payload.entity_blob_sha256,
            &payload.bindings,
        )?;
        validate_item_use(&payload.item_use)?;
        attack::validate(&payload.item_attack)?;
        use_profiles::validate(&payload.item_use_overlays)?;
        food::validate(&payload.item_food)?;
        let textures = decode_textures(&bytes[payload_end..hash_start], &mut shared)?;
        Ok(Self {
            source_manifest_sha256: payload.source_manifest_sha256,
            entity_blob_sha256: payload.entity_blob_sha256,
            bindings: Arc::from(payload.bindings),
            item_use_overlays: Arc::from(payload.item_use_overlays),
            item_use: Arc::from(payload.item_use),
            item_attack: Arc::from(payload.item_attack),
            item_food: Arc::from(payload.item_food),
            texture_paths_sha256: texture_paths_sha256(&textures),
            textures: Arc::from(textures),
        })
    }

    /// Shares equal texture buffers without changing catalog metadata or pixel values.
    pub(crate) fn share_texture_pixels(
        &mut self,
        mut shared: impl FnMut(&[u8]) -> Option<Arc<[u8]>>,
    ) -> bool {
        let replacements: Vec<_> = self
            .textures
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                shared(&texture.rgba8)
                    .filter(|pixels| !Arc::ptr_eq(pixels, &texture.rgba8))
                    .map(|pixels| (index, pixels))
            })
            .collect();
        if replacements.is_empty() {
            return false;
        }
        let textures = Arc::make_mut(&mut self.textures);
        for (index, pixels) in replacements {
            textures[index].rgba8 = pixels;
        }
        true
    }
}

/// Reads validated texture records, borrowing equal pixels from a live owner when available.
fn decode_textures(
    section: &[u8],
    shared: &mut impl FnMut(&[u8]) -> Option<Arc<[u8]>>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    let mut cursor = section;
    let count = u32::from_le_bytes(
        take(&mut cursor, 4)?
            .try_into()
            .map_err(|_| invalid("invalid equipment texture count"))?,
    ) as usize;
    if count > MAX_EQUIPMENT_TEXTURES {
        return Err(invalid("equipment texture count exceeds bound"));
    }
    let mut textures = Vec::with_capacity(count);
    let mut pixel_bytes = 0usize;
    for _ in 0..count {
        let name_len = usize::from(u16::from_le_bytes(
            take(&mut cursor, 2)?
                .try_into()
                .map_err(|_| invalid("invalid equipment texture name length"))?,
        ));
        let identifier: Box<str> = std::str::from_utf8(take(&mut cursor, name_len)?)
            .map_err(|_| invalid("equipment texture identifier is not UTF-8"))?
            .into();
        let width = u16::from_le_bytes(
            take(&mut cursor, 2)?
                .try_into()
                .map_err(|_| invalid("invalid equipment texture width"))?,
        );
        let height = u16::from_le_bytes(
            take(&mut cursor, 2)?
                .try_into()
                .map_err(|_| invalid("invalid equipment texture height"))?,
        );
        let length = texture_pixel_bytes(width, height)
            .ok_or_else(|| invalid("equipment texture dimensions exceed bound"))?;
        pixel_bytes = pixel_bytes
            .checked_add(length)
            .filter(|total| *total <= MAX_EQUIPMENT_PIXEL_BYTES)
            .ok_or_else(|| invalid("equipment texture pixel budget exceeded"))?;
        let pixels = take(&mut cursor, length)?;
        textures.push(EquipmentTexture {
            identifier,
            width,
            height,
            rgba8: shared(pixels).unwrap_or_else(|| Arc::from(pixels)),
        });
    }
    if !cursor.is_empty() {
        return Err(invalid("trailing bytes after equipment texture section"));
    }
    validate_textures(&textures)?;
    Ok(textures)
}

/// Advances the checked carrier cursor by exactly one field.
fn take<'a>(cursor: &mut &'a [u8], length: usize) -> Result<&'a [u8], AssetError> {
    let head = cursor
        .get(..length)
        .ok_or_else(|| invalid("truncated equipment texture section"))?;
    *cursor = &cursor[length..];
    Ok(head)
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(offset..offset + N)
        .ok_or_else(|| invalid("truncated equipment carrier field"))?
        .try_into()
        .map_err(|_| invalid("invalid equipment carrier field"))
}
