use std::str;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::TextureRef;

pub const TEXTURE_CARRIER_MAGIC: [u8; 9] = *b"MCBETEX01";
pub const TEXTURE_CARRIER_SCHEMA: u32 = 2;
pub const MAX_TEXTURE_SOURCE_ROUTES: usize = 65_536;
pub const MAX_TEXTURE_ROUTE_KEY_BYTES: usize = 256;
pub const MAX_TEXTURE_ROUTE_PATH_BYTES: usize = 512;
pub const MAX_TEXTURE_ROUTE_REFERENCES: usize = 256;
pub const MAX_TEXTURE_ROUTE_ANIMATION_FRAMES: usize = 4_096;

const HEADER_BYTES: usize = 96;
const ROUTE_BYTES: usize = 64;
const HASH_BYTES: usize = 32;
const MAX_CARRIER_BYTES: usize = 16 * 1024 * 1024;

/// One logical terrain-texture route in the immutable base atlas.
///
/// A route is keyed by the terrain texture key and variant index used by the
/// Bedrock pack manifest. Every reference is replaced together when a server
/// pack supplies a compatible texture for that route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureSourceRoute {
    key: Box<str>,
    variant: u32,
    path: Box<str>,
    references: Box<[TextureRef]>,
    animation_frames: Box<[TextureRef]>,
}

impl TextureSourceRoute {
    #[must_use]
    pub fn new(
        key: impl Into<Box<str>>,
        variant: u32,
        path: impl Into<Box<str>>,
        references: impl Into<Box<[TextureRef]>>,
    ) -> Self {
        Self {
            key: key.into(),
            variant,
            path: path.into(),
            references: references.into(),
            animation_frames: Box::new([]),
        }
    }

    #[must_use]
    pub fn with_animation_frames(
        mut self,
        animation_frames: impl Into<Box<[TextureRef]>>,
    ) -> Self {
        self.animation_frames = animation_frames.into();
        self
    }

    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    #[must_use]
    pub const fn variant(&self) -> u32 {
        self.variant
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn references(&self) -> &[TextureRef] {
        &self.references
    }

    #[must_use]
    pub fn animation_frames(&self) -> &[TextureRef] {
        &self.animation_frames
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextureCatalogIdentity {
    pub schema: u32,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTextureCatalog {
    identity: TextureCatalogIdentity,
    routes: Box<[TextureSourceRoute]>,
}

impl RuntimeTextureCatalog {
    /// Decodes a route carrier only when it belongs to the exact source
    /// manifest selected by the caller.
    pub fn decode(
        bytes: &[u8],
        expected_source_manifest_sha256: [u8; 32],
    ) -> Result<Self, TextureCatalogError> {
        let envelope = validate_envelope(bytes, expected_source_manifest_sha256)?;
        let routes = decode_routes(bytes, envelope)?;
        Ok(Self {
            identity: TextureCatalogIdentity {
                schema: TEXTURE_CARRIER_SCHEMA,
                source_manifest_sha256: expected_source_manifest_sha256,
                carrier_sha256: array_at(bytes, envelope.hash_offset)?,
            },
            routes: routes.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn identity(&self) -> TextureCatalogIdentity {
        self.identity
    }

    #[must_use]
    pub fn routes(&self) -> &[TextureSourceRoute] {
        &self.routes
    }
}

#[derive(Debug, Error)]
pub enum TextureCatalogError {
    #[error("texture carrier source manifest does not match the required startup provenance")]
    SourceManifestMismatch,
    #[error("texture carrier SHA-256 does not match its payload")]
    CarrierHashMismatch,
    #[error("invalid MCBETEX01 carrier: {detail}")]
    InvalidCarrier { detail: Box<str> },
    #[error("invalid compiled texture route catalog: {detail}")]
    InvalidCatalog { detail: Box<str> },
}

pub fn encode_texture_catalog(
    source_manifest_sha256: [u8; 32],
    routes: &[TextureSourceRoute],
) -> Result<Box<[u8]>, TextureCatalogError> {
    validate_catalog(source_manifest_sha256, routes)?;
    let route_offset = HEADER_BYTES;
    let strings_offset = checked_add(route_offset, checked_mul(routes.len(), ROUTE_BYTES)?)?;
    let strings_bytes = routes.iter().try_fold(0usize, |total, route| {
        checked_add(total, checked_add(route.key.len(), route.path.len())?)
    })?;
    let refs_offset = checked_add(strings_offset, strings_bytes)?;
    let refs_count = routes.iter().try_fold(0usize, |total, route| {
        checked_add(total, route.references.len())
    })?;
    let animation_refs_count = routes.iter().try_fold(0usize, |total, route| {
        if route.animation_frames.len() > MAX_TEXTURE_ROUTE_ANIMATION_FRAMES {
            return Err(invalid_catalog(
                "route animation frame count is outside its bound",
            ));
        }
        checked_add(total, route.animation_frames.len())
    })?;
    let animation_refs_offset = checked_add(
        refs_offset,
        checked_mul(refs_count, 4)?,
    )?;
    let hash_offset = checked_add(
        animation_refs_offset,
        checked_mul(animation_refs_count, 4)?,
    )?;
    let total_bytes = checked_add(hash_offset, HASH_BYTES)?;
    if total_bytes > MAX_CARRIER_BYTES {
        return Err(invalid_catalog("texture carrier exceeds its byte bound"));
    }

    let mut bytes = Vec::with_capacity(total_bytes);
    bytes.extend_from_slice(&TEXTURE_CARRIER_MAGIC);
    push_u32(&mut bytes, TEXTURE_CARRIER_SCHEMA);
    push_u32(
        &mut bytes,
        u32::try_from(routes.len()).map_err(|_| invalid_catalog("route count overflows u32"))?,
    );
    push_u32(&mut bytes, 0);
    bytes.extend_from_slice(&source_manifest_sha256);
    for offset in [
        route_offset,
        strings_offset,
        refs_offset,
        animation_refs_offset,
        hash_offset,
    ] {
        push_u64(&mut bytes, offset)?;
    }
    bytes.resize(HEADER_BYTES, 0);

    let mut string_cursor = strings_offset;
    let mut refs_cursor = refs_offset;
    let mut animation_refs_cursor = animation_refs_offset;
    for route in routes {
        push_u64(&mut bytes, string_cursor)?;
        push_u32(
            &mut bytes,
            u32::try_from(route.key.len())
                .map_err(|_| invalid_catalog("route key length overflows u32"))?,
        );
        push_u64(&mut bytes, checked_add(string_cursor, route.key.len())?)?;
        push_u32(
            &mut bytes,
            u32::try_from(route.path.len())
                .map_err(|_| invalid_catalog("route path length overflows u32"))?,
        );
        push_u32(&mut bytes, route.variant);
        push_u64(&mut bytes, refs_cursor)?;
        push_u32(
            &mut bytes,
            u32::try_from(route.references.len())
                .map_err(|_| invalid_catalog("route reference count overflows u32"))?,
        );
        push_u64(&mut bytes, animation_refs_cursor)?;
        push_u32(
            &mut bytes,
            u32::try_from(route.animation_frames.len())
                .map_err(|_| invalid_catalog("route animation frame count overflows u32"))?,
        );
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        string_cursor = checked_add(string_cursor, route.key.len())?;
        string_cursor = checked_add(string_cursor, route.path.len())?;
        refs_cursor = checked_add(refs_cursor, checked_mul(route.references.len(), 4)?)?;
        animation_refs_cursor = checked_add(
            animation_refs_cursor,
            checked_mul(route.animation_frames.len(), 4)?,
        )?;
    }
    for route in routes {
        bytes.extend_from_slice(route.key.as_bytes());
        bytes.extend_from_slice(route.path.as_bytes());
    }
    for route in routes {
        for reference in &route.references {
            push_u32(&mut bytes, reference.raw());
        }
    }
    for route in routes {
        for reference in &route.animation_frames {
            push_u32(&mut bytes, reference.raw());
        }
    }
    if bytes.len() != hash_offset {
        return Err(invalid_catalog("texture carrier section layout drifted"));
    }
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    Ok(bytes.into_boxed_slice())
}

#[derive(Clone, Copy)]
struct Envelope {
    route_count: usize,
    route_offset: usize,
    strings_offset: usize,
    refs_offset: usize,
    animation_refs_offset: usize,
    hash_offset: usize,
}

fn validate_envelope(
    bytes: &[u8],
    expected_source_manifest_sha256: [u8; 32],
) -> Result<Envelope, TextureCatalogError> {
    if expected_source_manifest_sha256 == [0; 32] {
        return Err(TextureCatalogError::SourceManifestMismatch);
    }
    if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_CARRIER_BYTES {
        return Err(invalid_carrier("carrier byte length is outside its bound"));
    }
    if bytes.get(..TEXTURE_CARRIER_MAGIC.len()) != Some(TEXTURE_CARRIER_MAGIC.as_slice()) {
        return Err(invalid_carrier("invalid MCBETEX01 magic"));
    }
    if u32_at(bytes, 9)? != TEXTURE_CARRIER_SCHEMA {
        return Err(invalid_carrier("unsupported MCBETEX01 schema"));
    }
    let route_count = usize::try_from(u32_at(bytes, 13)?)
        .map_err(|_| invalid_carrier("route count exceeds platform"))?;
    if route_count == 0 || route_count > MAX_TEXTURE_SOURCE_ROUTES {
        return Err(invalid_carrier("route count is outside its bound"));
    }
    if u32_at(bytes, 17)? != 0 || array_at(bytes, 21)? != expected_source_manifest_sha256 {
        return Err(TextureCatalogError::SourceManifestMismatch);
    }
    if bytes[93..HEADER_BYTES] != [0; 3] {
        return Err(invalid_carrier("carrier header reserved bytes are nonzero"));
    }
    let envelope = Envelope {
        route_count,
        route_offset: usize_at(bytes, 53)?,
        strings_offset: usize_at(bytes, 61)?,
        refs_offset: usize_at(bytes, 69)?,
        animation_refs_offset: usize_at(bytes, 77)?,
        hash_offset: usize_at(bytes, 85)?,
    };
    let expected_strings = checked_add(
        envelope.route_offset,
        checked_mul(route_count, ROUTE_BYTES)?,
    )?;
    if envelope.route_offset != HEADER_BYTES
        || envelope.strings_offset != expected_strings
        || envelope.strings_offset > envelope.refs_offset
        || envelope.refs_offset > envelope.animation_refs_offset
        || envelope.animation_refs_offset > envelope.hash_offset
        || checked_add(envelope.hash_offset, HASH_BYTES)? != bytes.len()
    {
        return Err(invalid_carrier("carrier section offsets are noncanonical"));
    }
    let actual_digest = Sha256::digest(&bytes[..envelope.hash_offset]);
    if actual_digest.as_slice() != &bytes[envelope.hash_offset..] {
        return Err(TextureCatalogError::CarrierHashMismatch);
    }
    Ok(envelope)
}

fn decode_routes(
    bytes: &[u8],
    envelope: Envelope,
) -> Result<Vec<TextureSourceRoute>, TextureCatalogError> {
    let mut routes = Vec::with_capacity(envelope.route_count);
    let mut previous = None::<(Box<str>, u32, Box<str>)>;
    for index in 0..envelope.route_count {
        let offset = checked_add(envelope.route_offset, checked_mul(index, ROUTE_BYTES)?)?;
        let record = bytes
            .get(offset..checked_add(offset, ROUTE_BYTES)?)
            .ok_or_else(|| invalid_carrier("route record exceeds carrier"))?;
        let key_offset = usize_at(record, 0)?;
        let key_length = usize::try_from(u32_at(record, 8)?)
            .map_err(|_| invalid_carrier("route key length exceeds platform"))?;
        let path_offset = usize_at(record, 12)?;
        let path_length = usize::try_from(u32_at(record, 20)?)
            .map_err(|_| invalid_carrier("route path length exceeds platform"))?;
        let variant = u32_at(record, 24)?;
        let refs_offset = usize_at(record, 28)?;
        let refs_count = usize::try_from(u32_at(record, 36)?)
            .map_err(|_| invalid_carrier("route reference count exceeds platform"))?;
        let animation_refs_offset = usize_at(record, 40)?;
        let animation_refs_count = usize::try_from(u32_at(record, 48)?)
            .map_err(|_| invalid_carrier("route animation frame count exceeds platform"))?;
        if u32_at(record, 52)? != 0 || u32_at(record, 56)? != 0 || u32_at(record, 60)? != 0 {
            return Err(invalid_carrier("route record reserved bytes are nonzero"));
        }
        if refs_count == 0 || refs_count > MAX_TEXTURE_ROUTE_REFERENCES {
            return Err(invalid_carrier(
                "route reference count is outside its bound",
            ));
        }
        let key = boxed_text(bytes, key_offset, key_length, MAX_TEXTURE_ROUTE_KEY_BYTES)?;
        let path = boxed_text(
            bytes,
            path_offset,
            path_length,
            MAX_TEXTURE_ROUTE_PATH_BYTES,
        )?;
        validate_route_key(&key)?;
        validate_route_path(&path)?;
        let refs_end = checked_add(refs_offset, checked_mul(refs_count, 4)?)?;
        if refs_offset < envelope.refs_offset || refs_end > envelope.animation_refs_offset {
            return Err(invalid_carrier("route references exceed their carrier section"));
        }
        let refs_bytes = bytes
            .get(refs_offset..refs_end)
            .ok_or_else(|| invalid_carrier("route references exceed carrier"))?;
        let mut references = Vec::with_capacity(refs_count);
        let mut previous_reference = None;
        for raw in refs_bytes
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        {
            let reference = TextureRef::from_raw(raw)
                .map_err(|_| invalid_carrier("route contains an invalid texture reference"))?;
            if previous_reference.is_some_and(|previous| previous >= reference) {
                return Err(invalid_carrier("route references are not strictly ordered"));
            }
            previous_reference = Some(reference);
            references.push(reference);
        }
        if animation_refs_count > MAX_TEXTURE_ROUTE_ANIMATION_FRAMES {
            return Err(invalid_carrier(
                "route animation frame count is outside its bound",
            ));
        }
        let animation_refs_end = checked_add(
            animation_refs_offset,
            checked_mul(animation_refs_count, 4)?,
        )?;
        if animation_refs_offset < envelope.animation_refs_offset
            || animation_refs_end > envelope.hash_offset
        {
            return Err(invalid_carrier(
                "route animation references exceed their carrier section",
            ));
        }
        let animation_bytes = bytes
            .get(animation_refs_offset..animation_refs_end)
            .ok_or_else(|| invalid_carrier("route animation references exceed carrier"))?;
        let mut animation_frames = Vec::with_capacity(animation_refs_count);
        for raw in animation_bytes
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        {
            animation_frames.push(
                TextureRef::from_raw(raw).map_err(|_| {
                    invalid_carrier("route contains an invalid animation reference")
                })?,
            );
        }
        let sort_key = (key.clone(), variant, path.clone());
        if previous
            .as_ref()
            .is_some_and(|previous| previous >= &sort_key)
        {
            return Err(invalid_carrier("routes are not strictly ordered"));
        }
        previous = Some(sort_key);
        routes.push(
            TextureSourceRoute::new(key, variant, path, references)
                .with_animation_frames(animation_frames.into_boxed_slice()),
        );
    }
    Ok(routes)
}

fn validate_catalog(
    source_manifest_sha256: [u8; 32],
    routes: &[TextureSourceRoute],
) -> Result<(), TextureCatalogError> {
    if source_manifest_sha256 == [0; 32] {
        return Err(invalid_catalog("source manifest SHA-256 is zero"));
    }
    if routes.is_empty() || routes.len() > MAX_TEXTURE_SOURCE_ROUTES {
        return Err(invalid_catalog("route count is outside its bound"));
    }
    let mut previous = None::<(&str, u32, &str)>;
    for route in routes {
        validate_route_key(&route.key)?;
        validate_route_path(&route.path)?;
        if route.references.is_empty() || route.references.len() > MAX_TEXTURE_ROUTE_REFERENCES {
            return Err(invalid_catalog(
                "route reference count is outside its bound",
            ));
        }
        if route.animation_frames.len() > MAX_TEXTURE_ROUTE_ANIMATION_FRAMES {
            return Err(invalid_catalog(
                "route animation frame count is outside its bound",
            ));
        }
        for pair in route.references.windows(2) {
            if pair[0] >= pair[1] {
                return Err(invalid_catalog("route references are not strictly ordered"));
            }
        }
        let sort_key = (route.key.as_ref(), route.variant, route.path.as_ref());
        if previous.is_some_and(|previous| previous >= sort_key) {
            return Err(invalid_catalog("routes are not strictly ordered"));
        }
        previous = Some(sort_key);
    }
    Ok(())
}

fn validate_route_key(key: &str) -> Result<(), TextureCatalogError> {
    if key.is_empty()
        || key.len() > MAX_TEXTURE_ROUTE_KEY_BYTES
        || key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid_catalog("route key is outside its bound"));
    }
    Ok(())
}

fn validate_route_path(path: &str) -> Result<(), TextureCatalogError> {
    if path.is_empty()
        || path.len() > MAX_TEXTURE_ROUTE_PATH_BYTES
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid_catalog("route path is unsafe or outside its bound"));
    }
    Ok(())
}

fn boxed_text(
    bytes: &[u8],
    offset: usize,
    length: usize,
    maximum: usize,
) -> Result<Box<str>, TextureCatalogError> {
    if length == 0 || length > maximum {
        return Err(invalid_carrier("route string length is outside its bound"));
    }
    let end = checked_add(offset, length)?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| invalid_carrier("route string exceeds carrier"))?;
    let value = str::from_utf8(value).map_err(|_| invalid_carrier("route string is not UTF-8"))?;
    Ok(value.into())
}

fn checked_add(left: usize, right: usize) -> Result<usize, TextureCatalogError> {
    left.checked_add(right)
        .ok_or_else(|| invalid_carrier("carrier size overflows usize"))
}

fn checked_mul(left: usize, right: usize) -> Result<usize, TextureCatalogError> {
    left.checked_mul(right)
        .ok_or_else(|| invalid_carrier("carrier size overflows usize"))
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: usize) -> Result<(), TextureCatalogError> {
    bytes.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| invalid_catalog("carrier offset overflows u64"))?
            .to_le_bytes(),
    );
    Ok(())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, TextureCatalogError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| invalid_carrier("carrier integer offset overflows"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| invalid_carrier("carrier integer is truncated"))?;
    Ok(u32::from_le_bytes(
        value.try_into().expect("four-byte slice"),
    ))
}

fn usize_at(bytes: &[u8], offset: usize) -> Result<usize, TextureCatalogError> {
    usize::try_from(u64_at(bytes, offset)?)
        .map_err(|_| invalid_carrier("carrier offset exceeds platform"))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, TextureCatalogError> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| invalid_carrier("carrier offset integer overflows"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| invalid_carrier("carrier offset integer is truncated"))?;
    Ok(u64::from_le_bytes(
        value.try_into().expect("eight-byte slice"),
    ))
}

fn array_at(bytes: &[u8], offset: usize) -> Result<[u8; 32], TextureCatalogError> {
    let end = offset
        .checked_add(32)
        .ok_or_else(|| invalid_carrier("carrier hash offset overflows"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| invalid_carrier("carrier hash is truncated"))?;
    Ok(value.try_into().expect("32-byte slice"))
}

fn invalid_carrier(detail: impl Into<Box<str>>) -> TextureCatalogError {
    TextureCatalogError::InvalidCarrier {
        detail: detail.into(),
    }
}

fn invalid_catalog(detail: impl Into<Box<str>>) -> TextureCatalogError {
    TextureCatalogError::InvalidCatalog {
        detail: detail.into(),
    }
}
