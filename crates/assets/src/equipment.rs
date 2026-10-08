//! Bounded equipment (attachable) binding catalog.
//!
//! Maps an item identifier to the entity-catalog geometry, texture, material,
//! and render controller its `minecraft:attachable` selects, plus the display
//! transforms present as literal data in the pinned pack. This carries binding
//! data only; nothing here renders, and transforms flagged `NeedsMeasurement`
//! are Molang/engine-derived and must be measured natively before use.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::item::{ItemDisplayScalar, ItemDisplayTransform};
use crate::{AssetError, EntityDependencyResolution};

mod attack;
pub use attack::{
    CompiledItemAttackCooldown, CompiledItemAttackTiming, CompiledKineticWeaponTiming,
};

#[cfg(test)]
#[path = "equipment/texture_tests.rs"]
mod texture_tests;

pub const EQUIPMENT_CARRIER_MAGIC: [u8; 8] = *b"MCBEEQP1";
pub const EQUIPMENT_CARRIER_VERSION: u32 = 2;
pub const MAX_EQUIPMENT_BINDINGS: usize = 1024;
pub const MAX_EQUIPMENT_IDENTIFIER_BYTES: usize = 256;
pub const MAX_EQUIPMENT_TEXTURES: usize = 256;
pub const MAX_EQUIPMENT_TEXTURE_SIDE: u16 = crate::MAX_ACTOR_TEXTURE_SIDE;
pub const MAX_EQUIPMENT_PIXEL_BYTES: usize = crate::MAX_ACTOR_PIXEL_BYTES;
const MAX_EQUIPMENT_METADATA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_EQUIPMENT_CARRIER_BYTES: usize =
    MAX_EQUIPMENT_PIXEL_BYTES + MAX_EQUIPMENT_METADATA_BYTES;

const HEADER_BYTES: usize = 20;
const HASH_BYTES: usize = 32;

/// One item's attachable binding into the entity catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentBinding {
    /// Item identifier the attachable binds to (the catalog sort key).
    pub identifier: Box<str>,
    pub category: EquipmentCategory,
    pub geometry: EquipmentReference,
    pub texture: EquipmentReference,
    pub material: Box<str>,
    pub render_controller: Box<str>,
    pub first_person: EquipmentTransform,
    pub third_person: EquipmentTransform,
    pub dropped: EquipmentTransform,
    /// Literal-only named poses of the attachable's own bones (elytra states); empty otherwise.
    #[serde(default)]
    pub poses: Box<[AttachablePose]>,
}

/// One literal animation of an attachable: per-bone offsets in pixels and degrees.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachablePose {
    /// The attachable's local animation key (for example `default`, `sneaking`).
    pub key: Box<str>,
    pub bones: Box<[AttachablePoseBone]>,
}

/// A bone's literal channels; an absent channel keeps the rest value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachablePoseBone {
    pub bone: Box<str>,
    pub translation: Option<[ItemDisplayScalar; 3]>,
    pub rotation: Option<[ItemDisplayScalar; 3]>,
    pub scale: Option<[ItemDisplayScalar; 3]>,
}

impl EquipmentBinding {
    /// The literal pose stored under `key`.
    #[must_use]
    pub fn pose(&self, key: &str) -> Option<&AttachablePose> {
        self.poses.iter().find(|pose| pose.key.as_ref() == key)
    }
}

const MAX_POSES_PER_BINDING: usize = 16;
const MAX_ITEM_TIMINGS: usize = 2048;
const MAX_BONES_PER_POSE: usize = 32;

/// Where the attachment renders, which selects the biped bone a later tranche binds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EquipmentCategory {
    Held,
    Armor { slot: ArmorSlot },
    Shield,
    Elytra,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmorSlot {
    Helmet,
    Chestplate,
    Leggings,
    Boots,
}

/// A geometry or texture identifier plus whether the entity catalog retains it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentReference {
    pub identifier: Box<str>,
    pub resolution: EntityDependencyResolution,
}

/// A display transform, or a marker that the pack value is Molang/engine-derived.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum EquipmentTransform {
    Literal { transform: ItemDisplayTransform },
    NeedsMeasurement,
}

impl EquipmentTransform {
    /// The literal transform when present; `NeedsMeasurement` reads as `None`.
    #[must_use]
    pub const fn literal(self) -> Option<ItemDisplayTransform> {
        match self {
            Self::Literal { transform } => Some(transform),
            Self::NeedsMeasurement => None,
        }
    }
}

/// One attachable texture's decoded RGBA8 pixels, keyed by its pack identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquipmentTexture {
    pub identifier: Box<str>,
    pub width: u16,
    pub height: u16,
    pub rgba8: Arc<[u8]>,
}

/// The undyed leather armor tint, in RGB byte order.
pub const DEFAULT_LEATHER_RGB: u32 = 0x00a0_6540;

/// Resolves a leather material's dye mask while retaining its zero-alpha cutout.
pub fn color_mask_texel(mut texel: [u8; 4], tint: [u8; 3]) -> [u8; 4] {
    let mask = u32::from(texel[3]);
    for channel in 0..3 {
        let weight = 255 * (255 - mask) + u32::from(tint[channel]) * mask;
        texel[channel] = (u32::from(texel[channel]) * weight / (255 * 255)) as u8;
    }
    if texel[3] != 0 {
        texel[3] = 255;
    }
    texel
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EquipmentCatalogPayload {
    source_manifest_sha256: [u8; 32],
    entity_blob_sha256: [u8; 32],
    bindings: Box<[EquipmentBinding]>,
    #[serde(default)]
    item_use: Box<[ItemUseDuration]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    item_attack: Vec<CompiledItemAttackTiming>,
}

/// How long one item can be used (eaten, drunk, drawn) before it completes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemUseDuration {
    pub identifier: Box<str>,
    pub ticks: u32,
}

/// Decoded, validated equipment catalog pinned to one entity carrier build.
#[derive(Clone, Debug)]
pub struct RuntimeEquipmentCatalog {
    source_manifest_sha256: [u8; 32],
    entity_blob_sha256: [u8; 32],
    bindings: Arc<[EquipmentBinding]>,
    textures: Arc<[EquipmentTexture]>,
    item_use: Arc<[ItemUseDuration]>,
    item_attack: Arc<[CompiledItemAttackTiming]>,
}

impl RuntimeEquipmentCatalog {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_EQUIPMENT_CARRIER_BYTES {
            return Err(invalid("equipment carrier size is out of bounds"));
        }
        if bytes[..8] != EQUIPMENT_CARRIER_MAGIC
            || u32::from_le_bytes(field::<4>(bytes, 8)?) != EQUIPMENT_CARRIER_VERSION
        {
            return Err(invalid("unsupported equipment carrier header"));
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
        let textures = decode_textures(&bytes[payload_end..hash_start])?;
        Ok(Self {
            source_manifest_sha256: payload.source_manifest_sha256,
            entity_blob_sha256: payload.entity_blob_sha256,
            bindings: Arc::from(payload.bindings),
            item_use: Arc::from(payload.item_use),
            item_attack: Arc::from(payload.item_attack),
            textures: Arc::from(textures),
        })
    }

    /// Builds a catalog in memory from a server pack's attachables, bound to `identity` (a
    /// nonzero digest of the pack sources). Bindings and textures are sorted by identifier
    /// and the first of a repeated identifier is kept; nothing is carried for item use.
    pub fn from_parts(
        identity: [u8; 32],
        mut bindings: Vec<EquipmentBinding>,
        mut textures: Vec<EquipmentTexture>,
    ) -> Result<Self, AssetError> {
        bindings.sort_by(|left, right| left.identifier.cmp(&right.identifier));
        bindings.dedup_by(|later, earlier| later.identifier == earlier.identifier);
        textures.sort_by(|left, right| left.identifier.cmp(&right.identifier));
        textures.dedup_by(|later, earlier| later.identifier == earlier.identifier);
        validate(&identity, &identity, &bindings)?;
        validate_textures(&textures)?;
        Ok(Self {
            source_manifest_sha256: identity,
            entity_blob_sha256: identity,
            bindings: Arc::from(bindings),
            textures: Arc::from(textures),
            item_use: Arc::from(Vec::new()),
            item_attack: Arc::from(Vec::new()),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    /// SHA-256 the sibling entity carrier had at compile time; a consumer that
    /// loads both must confirm it before resolving `Catalog` references.
    #[must_use]
    pub const fn entity_blob_sha256(&self) -> [u8; 32] {
        self.entity_blob_sha256
    }

    #[must_use]
    pub fn bindings(&self) -> &[EquipmentBinding] {
        &self.bindings
    }

    /// Use durations sorted by item identifier.
    #[must_use]
    pub fn item_use(&self) -> &[ItemUseDuration] {
        &self.item_use
    }

    /// Optional attack facts sorted by item identifier.
    #[must_use]
    pub fn item_attack_timings(&self) -> &[CompiledItemAttackTiming] {
        &self.item_attack
    }

    /// Ticks the item can be used for, when the pack states it.
    #[must_use]
    pub fn item_use_ticks(&self, identifier: &str) -> Option<u32> {
        self.item_use
            .binary_search_by(|entry| entry.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| self.item_use[index].ticks)
    }

    /// Attachable textures sorted by identifier.
    #[must_use]
    pub fn textures(&self) -> &[EquipmentTexture] {
        &self.textures
    }

    #[must_use]
    pub fn texture(&self, identifier: &str) -> Option<&EquipmentTexture> {
        self.textures
            .binary_search_by(|texture| texture.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| &self.textures[index])
    }

    #[must_use]
    pub fn binding(&self, identifier: &str) -> Option<&EquipmentBinding> {
        self.bindings
            .binary_search_by(|binding| binding.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| &self.bindings[index])
    }
}

pub fn encode_equipment_catalog(
    source_manifest_sha256: [u8; 32],
    entity_blob_sha256: [u8; 32],
    bindings: &[EquipmentBinding],
) -> Result<Vec<u8>, AssetError> {
    encode_equipment_catalog_with_textures(
        source_manifest_sha256,
        entity_blob_sha256,
        bindings,
        &[],
    )
}

/// Encodes bindings plus a binary texture section (sorted, unique identifiers).
pub fn encode_equipment_catalog_with_textures(
    source_manifest_sha256: [u8; 32],
    entity_blob_sha256: [u8; 32],
    bindings: &[EquipmentBinding],
    textures: &[EquipmentTexture],
) -> Result<Vec<u8>, AssetError> {
    encode_equipment_catalog_full(
        source_manifest_sha256,
        entity_blob_sha256,
        bindings,
        textures,
        &[],
    )
}

/// Encodes bindings, textures, and item use durations (sorted, unique identifiers).
pub fn encode_equipment_catalog_full(
    source_manifest_sha256: [u8; 32],
    entity_blob_sha256: [u8; 32],
    bindings: &[EquipmentBinding],
    textures: &[EquipmentTexture],
    item_use: &[ItemUseDuration],
) -> Result<Vec<u8>, AssetError> {
    encode_equipment_catalog_with_attack_timings(
        source_manifest_sha256,
        entity_blob_sha256,
        bindings,
        textures,
        item_use,
        &[],
    )
}

/// Encodes the complete equipment catalog, including optional authored attack behavior.
pub fn encode_equipment_catalog_with_attack_timings(
    source_manifest_sha256: [u8; 32],
    entity_blob_sha256: [u8; 32],
    bindings: &[EquipmentBinding],
    textures: &[EquipmentTexture],
    item_use: &[ItemUseDuration],
    item_attack: &[CompiledItemAttackTiming],
) -> Result<Vec<u8>, AssetError> {
    validate(&source_manifest_sha256, &entity_blob_sha256, bindings)?;
    validate_textures(textures)?;
    validate_item_use(item_use)?;
    attack::validate(item_attack)?;
    let payload = EquipmentCatalogPayload {
        source_manifest_sha256,
        entity_blob_sha256,
        bindings: bindings.to_vec().into_boxed_slice(),
        item_use: item_use.to_vec().into_boxed_slice(),
        item_attack: item_attack.to_vec(),
    };
    let payload_bytes =
        serde_json::to_vec(&payload).map_err(|_| invalid("failed to encode equipment payload"))?;
    if payload_bytes.len() > MAX_EQUIPMENT_METADATA_BYTES {
        return Err(invalid("equipment metadata exceeds bound"));
    }
    let mut bytes = Vec::with_capacity(HEADER_BYTES + payload_bytes.len() + HASH_BYTES);
    bytes.extend_from_slice(&EQUIPMENT_CARRIER_MAGIC);
    bytes.extend_from_slice(&EQUIPMENT_CARRIER_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(payload_bytes.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&payload_bytes);
    bytes.extend_from_slice(&(textures.len() as u32).to_le_bytes());
    for texture in textures {
        bytes.extend_from_slice(&(texture.identifier.len() as u16).to_le_bytes());
        bytes.extend_from_slice(texture.identifier.as_bytes());
        bytes.extend_from_slice(&texture.width.to_le_bytes());
        bytes.extend_from_slice(&texture.height.to_le_bytes());
        bytes.extend_from_slice(&texture.rgba8);
    }
    let hash = Sha256::digest(&bytes);
    bytes.extend_from_slice(&hash);
    if bytes.len() > MAX_EQUIPMENT_CARRIER_BYTES {
        return Err(invalid("equipment carrier exceeds bound"));
    }
    Ok(bytes)
}

fn validate(
    source_manifest_sha256: &[u8; 32],
    entity_blob_sha256: &[u8; 32],
    bindings: &[EquipmentBinding],
) -> Result<(), AssetError> {
    if source_manifest_sha256 == &[0; 32]
        || entity_blob_sha256 == &[0; 32]
        || bindings.len() > MAX_EQUIPMENT_BINDINGS
    {
        return Err(invalid("equipment catalog provenance or count is invalid"));
    }
    let mut previous: Option<&str> = None;
    for binding in bindings {
        validate_identifier(&binding.identifier)?;
        validate_identifier(&binding.geometry.identifier)?;
        validate_identifier(&binding.texture.identifier)?;
        if previous.is_some_and(|previous| previous >= binding.identifier.as_ref())
            || binding.material.is_empty()
            || binding.material.len() > MAX_EQUIPMENT_IDENTIFIER_BYTES
            || binding.render_controller.is_empty()
            || binding.render_controller.len() > MAX_EQUIPMENT_IDENTIFIER_BYTES
            || !transform_is_canonical(binding.first_person)
            || !transform_is_canonical(binding.third_person)
            || !transform_is_canonical(binding.dropped)
            || !poses_are_valid(&binding.poses)
        {
            return Err(invalid("invalid or unordered equipment binding"));
        }
        previous = Some(&binding.identifier);
    }
    Ok(())
}

fn validate_item_use(item_use: &[ItemUseDuration]) -> Result<(), AssetError> {
    let mut previous: Option<&str> = None;
    for entry in item_use {
        validate_identifier(&entry.identifier)?;
        if previous.is_some_and(|previous| previous >= entry.identifier.as_ref())
            || entry.ticks == 0
        {
            return Err(invalid("invalid or unordered item use duration"));
        }
        previous = Some(&entry.identifier);
    }
    if item_use.len() > MAX_ITEM_TIMINGS {
        return Err(invalid("item use duration count exceeds bound"));
    }
    Ok(())
}

fn validate_textures(textures: &[EquipmentTexture]) -> Result<(), AssetError> {
    if textures.len() > MAX_EQUIPMENT_TEXTURES {
        return Err(invalid("equipment texture count exceeds bound"));
    }
    let mut previous: Option<&str> = None;
    let mut pixel_bytes = 0usize;
    for texture in textures {
        validate_identifier(&texture.identifier)?;
        let bytes = texture_pixel_bytes(texture.width, texture.height)
            .ok_or_else(|| invalid("equipment texture dimensions exceed bound"))?;
        pixel_bytes = pixel_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_EQUIPMENT_PIXEL_BYTES)
            .ok_or_else(|| invalid("equipment texture pixel budget exceeded"))?;
        if previous.is_some_and(|previous| previous >= texture.identifier.as_ref())
            || texture.rgba8.len() != bytes
        {
            return Err(invalid("invalid or unordered equipment texture"));
        }
        previous = Some(&texture.identifier);
    }
    Ok(())
}

fn texture_pixel_bytes(width: u16, height: u16) -> Option<usize> {
    let side_ok = |side: u16| (1..=MAX_EQUIPMENT_TEXTURE_SIDE).contains(&side);
    if !side_ok(width) || !side_ok(height) {
        return None;
    }
    usize::from(width)
        .checked_mul(usize::from(height))?
        .checked_mul(4)
}

fn decode_textures(section: &[u8]) -> Result<Vec<EquipmentTexture>, AssetError> {
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
        textures.push(EquipmentTexture {
            identifier,
            width,
            height,
            rgba8: Arc::from(take(&mut cursor, length)?),
        });
    }
    if !cursor.is_empty() {
        return Err(invalid("trailing bytes after equipment texture section"));
    }
    validate_textures(&textures)?;
    Ok(textures)
}

fn take<'a>(cursor: &mut &'a [u8], length: usize) -> Result<&'a [u8], AssetError> {
    let head = cursor
        .get(..length)
        .ok_or_else(|| invalid("truncated equipment texture section"))?;
    *cursor = &cursor[length..];
    Ok(head)
}

fn poses_are_valid(poses: &[AttachablePose]) -> bool {
    let canonical = |channel: &Option<[ItemDisplayScalar; 3]>| {
        channel.is_none_or(|values| values.iter().all(|value| scalar_is_canonical(*value)))
    };
    poses.len() <= MAX_POSES_PER_BINDING
        && poses.windows(2).all(|pair| pair[0].key < pair[1].key)
        && poses.iter().all(|pose| {
            validate_identifier(&pose.key).is_ok()
                && pose.bones.len() <= MAX_BONES_PER_POSE
                && pose.bones.iter().all(|bone| {
                    validate_identifier(&bone.bone).is_ok()
                        && canonical(&bone.translation)
                        && canonical(&bone.rotation)
                        && canonical(&bone.scale)
                })
        })
}

fn transform_is_canonical(transform: EquipmentTransform) -> bool {
    match transform.literal() {
        None => true,
        Some(transform) => transform
            .translation
            .iter()
            .chain(&transform.rotation)
            .chain(&transform.scale)
            .all(|scalar| scalar_is_canonical(*scalar)),
    }
}

fn scalar_is_canonical(scalar: ItemDisplayScalar) -> bool {
    ItemDisplayScalar::new(scalar.get()) == Some(scalar)
}

fn validate_identifier(identifier: &str) -> Result<(), AssetError> {
    if identifier.is_empty()
        || identifier.len() > MAX_EQUIPMENT_IDENTIFIER_BYTES
        || identifier.chars().any(char::is_control)
    {
        return Err(invalid(
            "equipment identifier is empty or exceeds its bound",
        ));
    }
    Ok(())
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(offset..offset + N)
        .ok_or_else(|| invalid("truncated equipment carrier field"))?
        .try_into()
        .map_err(|_| invalid("invalid equipment carrier field"))
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(translation: [f32; 3], rotation: [f32; 3]) -> ItemDisplayTransform {
        let scalar = |value: f32| ItemDisplayScalar::new(value).unwrap();
        ItemDisplayTransform {
            translation: translation.map(scalar),
            rotation: rotation.map(scalar),
            scale: [scalar(1.0); 3],
        }
    }

    fn sample() -> Vec<EquipmentBinding> {
        vec![
            EquipmentBinding {
                identifier: "minecraft:diamond_helmet".into(),
                category: EquipmentCategory::Armor {
                    slot: ArmorSlot::Helmet,
                },
                geometry: EquipmentReference {
                    identifier: "geometry.player.armor.helmet".into(),
                    resolution: EntityDependencyResolution::Catalog,
                },
                texture: EquipmentReference {
                    identifier: "textures/models/armor/diamond_1".into(),
                    resolution: EntityDependencyResolution::Catalog,
                },
                material: "armor".into(),
                render_controller: "controller.render.armor".into(),
                first_person: EquipmentTransform::NeedsMeasurement,
                third_person: EquipmentTransform::NeedsMeasurement,
                dropped: EquipmentTransform::NeedsMeasurement,
                poses: Box::new([]),
            },
            EquipmentBinding {
                identifier: "minecraft:trident".into(),
                category: EquipmentCategory::Held,
                geometry: EquipmentReference {
                    identifier: "geometry.trident".into(),
                    resolution: EntityDependencyResolution::Catalog,
                },
                texture: EquipmentReference {
                    identifier: "textures/entity/trident".into(),
                    resolution: EntityDependencyResolution::Catalog,
                },
                material: "entity_alphatest".into(),
                render_controller: "controller.render.item_default".into(),
                first_person: EquipmentTransform::Literal {
                    transform: transform([-7.0, -3.0, -2.0], [152.0, -9.0, 25.0]),
                },
                third_person: EquipmentTransform::Literal {
                    transform: transform([1.5, -2.5, -10.5], [97.0, -1.5, -49.0]),
                },
                dropped: EquipmentTransform::NeedsMeasurement,
                poses: Box::new([]),
            },
        ]
    }

    #[test]
    fn round_trips_and_looks_up_by_identifier() {
        let bytes = encode_equipment_catalog([1; 32], [2; 32], &sample()).unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        assert_eq!(catalog.source_manifest_sha256(), [1; 32]);
        assert_eq!(catalog.entity_blob_sha256(), [2; 32]);
        assert_eq!(catalog.bindings().len(), 2);
        let trident = catalog.binding("minecraft:trident").unwrap();
        assert_eq!(
            trident.first_person.literal().unwrap().translation[0].get(),
            -7.0
        );
        assert!(catalog.binding("minecraft:absent").is_none());
    }

    #[test]
    fn poses_round_trip_and_must_be_sorted_by_key() {
        let scalar = |value: f32| ItemDisplayScalar::new(value).unwrap();
        let pose = |key: &str| AttachablePose {
            key: key.into(),
            bones: Box::new([AttachablePoseBone {
                bone: "left_wing".into(),
                translation: Some([scalar(4.5), scalar(4.0), scalar(-2.0)]),
                rotation: None,
                scale: Some([scalar(1.0), scalar(1.0), scalar(2.0)]),
            }]),
        };
        let mut bindings = sample();
        bindings[0].poses = Box::new([pose("default"), pose("sneaking")]);
        let bytes = encode_equipment_catalog([1; 32], [2; 32], &bindings).unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        let helmet = catalog.binding("minecraft:diamond_helmet").unwrap();
        assert_eq!(helmet.pose("sneaking").unwrap().bones.len(), 1);
        assert!(helmet.pose("gliding").is_none());

        bindings[0].poses = Box::new([pose("sneaking"), pose("default")]);
        assert!(encode_equipment_catalog([1; 32], [2; 32], &bindings).is_err());
    }

    #[test]
    fn item_use_durations_round_trip_sorted_and_reject_zero_or_unordered() {
        let entry = |identifier: &str, ticks: u32| ItemUseDuration {
            identifier: identifier.into(),
            ticks,
        };
        let sorted = [
            entry("minecraft:apple", 32),
            entry("minecraft:honey_bottle", 40),
        ];
        let bytes =
            encode_equipment_catalog_full([1; 32], [2; 32], &sample(), &[], &sorted).unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        assert_eq!(catalog.item_use_ticks("minecraft:honey_bottle"), Some(40));
        assert_eq!(catalog.item_use_ticks("minecraft:bow"), None);
        let unordered = [entry("minecraft:b", 1), entry("minecraft:a", 1)];
        assert!(
            encode_equipment_catalog_full([1; 32], [2; 32], &sample(), &[], &unordered).is_err()
        );
        assert!(
            encode_equipment_catalog_full(
                [1; 32],
                [2; 32],
                &sample(),
                &[],
                &[entry("minecraft:a", 0)]
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_unordered_bindings_and_zero_provenance() {
        let mut reversed = sample();
        reversed.reverse();
        assert!(encode_equipment_catalog([1; 32], [2; 32], &reversed).is_err());
        assert!(encode_equipment_catalog([0; 32], [2; 32], &sample()).is_err());
    }

    #[test]
    fn rejects_tampered_envelope_and_payload() {
        let mut bytes = encode_equipment_catalog([1; 32], [2; 32], &sample()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert!(RuntimeEquipmentCatalog::decode(&bytes).is_err());

        let mut truncated = encode_equipment_catalog([1; 32], [2; 32], &sample()).unwrap();
        truncated[8] = 0x03; // unsupported version
        assert!(RuntimeEquipmentCatalog::decode(&truncated).is_err());
    }

    #[test]
    fn textures_round_trip_sorted_and_reject_bad_pixel_length() {
        let texture = |identifier: &str, width: u16, height: u16| EquipmentTexture {
            identifier: identifier.into(),
            width,
            height,
            rgba8: Arc::from(vec![7; usize::from(width) * usize::from(height) * 4]),
        };
        let textures = [texture("textures/a", 2, 1), texture("textures/b", 1, 2)];
        let bytes =
            encode_equipment_catalog_with_textures([1; 32], [2; 32], &sample(), &textures).unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        assert_eq!(catalog.textures().len(), 2);
        assert_eq!(catalog.texture("textures/b").unwrap().height, 2);
        assert!(catalog.texture("textures/c").is_none());

        let mut bad = texture("textures/a", 2, 1);
        bad.rgba8 = Arc::from(vec![0; 3]);
        assert!(
            encode_equipment_catalog_with_textures([1; 32], [2; 32], &sample(), &[bad]).is_err()
        );
        let unordered = [texture("textures/b", 1, 1), texture("textures/a", 1, 1)];
        assert!(
            encode_equipment_catalog_with_textures([1; 32], [2; 32], &sample(), &unordered)
                .is_err()
        );
    }
}

#[cfg(test)]
mod from_parts_tests {
    use super::*;

    fn binding(identifier: &str) -> EquipmentBinding {
        let reference = |identifier: &str| EquipmentReference {
            identifier: identifier.into(),
            resolution: EntityDependencyResolution::Catalog,
        };
        EquipmentBinding {
            identifier: identifier.into(),
            category: EquipmentCategory::Held,
            geometry: reference("geometry.a"),
            texture: reference("textures/entity/a"),
            material: "entity".into(),
            render_controller: "controller.render.a".into(),
            first_person: EquipmentTransform::NeedsMeasurement,
            third_person: EquipmentTransform::NeedsMeasurement,
            dropped: EquipmentTransform::NeedsMeasurement,
            poses: Box::new([]),
        }
    }

    // Bindings sort and dedupe by identifier; a zero identity is refused.
    #[test]
    fn from_parts_sorts_dedupes_and_requires_an_identity() {
        let catalog = RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            vec![binding("b:item"), binding("a:item"), binding("b:item")],
            Vec::new(),
        )
        .unwrap();
        let identifiers = catalog
            .bindings()
            .iter()
            .map(|binding| binding.identifier.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(identifiers, ["a:item", "b:item"]);
        assert!(catalog.binding("b:item").is_some());
        assert!(
            RuntimeEquipmentCatalog::from_parts([0; 32], vec![binding("a:item")], Vec::new())
                .is_err()
        );
    }
}
