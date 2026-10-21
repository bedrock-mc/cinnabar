//! Bounded catalog admission, immutable item handles and slot loadouts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

pub mod emotes;
mod ownership;
pub use ownership::{owns, set_ownership};
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// Number of supported slots, checked against the pinned wire schema.
pub const SLOT_COUNT: usize = 6;
pub const MAX_ITEMS: usize = 256;
pub const MAX_ASSET_BYTES: usize = 1024 * 1024;
pub type Slots = BTreeMap<String, Option<String>>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Catalog {
    pub version: u64,
    pub items: Vec<Descriptor>,
    #[serde(default)]
    pub emotes: Vec<emotes::Descriptor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
    pub bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Assets {
    pub geometry: Asset,
    pub texture: Asset,
    pub animation: Option<Asset>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RenderFlags {
    pub double_sided: bool,
    /// Draws every opaque texel fullbright in the world; previews keep the authored texture.
    #[serde(default, skip_serializing_if = "is_false")]
    pub emissive: bool,
}

/// Omits disabled optional flags so catalogs written before they existed round-trip unchanged.
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Descriptor {
    pub id: String,
    pub slot: String,
    pub display_name: String,
    pub tags: Vec<String>,
    pub free: bool,
    pub assets: Assets,
    pub geometry_id: String,
    pub attachment_bone: String,
    pub offset: [f32; 3],
    pub render_flags: RenderFlags,
}

/// An immutable admitted item; handles remain valid when a catalog refreshes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Item(u16);
static ITEMS: RwLock<Vec<&'static Descriptor>> = RwLock::new(Vec::new());
static ACTIVE: RwLock<Option<Vec<Item>>> = RwLock::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

impl Item {
    /// Installs metadata after its renderer has admitted every asset; bounds lifetime allocations.
    pub fn admit(descriptor: Descriptor) -> Option<Self> {
        let mut items = ITEMS.write().unwrap_or_else(|error| error.into_inner());
        if let Some(index) = items.iter().position(|item| **item == descriptor) {
            return Some(Self(index as u16));
        }
        if items.len() == MAX_ITEMS {
            return None;
        }
        let index = items.len();
        items.push(Box::leak(Box::new(descriptor)));
        GENERATION.fetch_add(1, Ordering::Release);
        Some(Self(index as u16))
    }

    /// Identifies catalog changes without copying its items on unchanged frames.
    pub fn generation() -> u64 {
        GENERATION.load(Ordering::Acquire)
    }

    /// Resolves the newest admitted revision of an identifier, ignoring unknown items.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().into_iter().find(|item| item.id() == id)
    }

    /// Lists the currently admitted metadata in catalog order, excluding superseded revisions.
    pub fn all() -> Vec<Self> {
        if let Some(active) = ACTIVE
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            return active.clone();
        }
        let items = ITEMS.read().unwrap_or_else(|error| error.into_inner());
        items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                (!items[index + 1..].iter().any(|newer| newer.id == item.id))
                    .then_some(Self(index as u16))
            })
            .collect()
    }

    /// Returns immutable metadata shared by the gallery, renderer and wire selection.
    pub fn descriptor(self) -> &'static Descriptor {
        ITEMS.read().unwrap_or_else(|error| error.into_inner())[usize::from(self.0)]
    }

    /// Returns the namespaced wire and persistence identifier.
    pub fn id(self) -> &'static str {
        &self.descriptor().id
    }
    /// Returns the catalog's player-facing name.
    pub fn name(self) -> &'static str {
        &self.descriptor().display_name
    }
    /// Returns the artwork cache key for the admitted texture revision.
    pub fn texture_key(self) -> String {
        format!(
            "cosmetic-texture:{}:{}",
            self.id(),
            self.descriptor().assets.texture.sha256
        )
    }
    /// Returns the artwork cache key for this geometry revision.
    pub fn thumbnail_key(self) -> String {
        format!("cosmetic-thumbnail:{}:{}", self.id(), self.0)
    }
    /// Returns a renderer-local stable geometry index.
    pub fn index(self) -> u16 {
        self.0
    }
}

/// Replaces the visible catalog after all supported assets have been admitted.
pub fn activate(ids: &[String]) {
    let items = ITEMS.read().unwrap_or_else(|error| error.into_inner());
    let active = ids
        .iter()
        .filter_map(|id| {
            items
                .iter()
                .rposition(|item| item.id == *id)
                .map(|index| Item(index as u16))
        })
        .collect();
    let mut current = ACTIVE.write().unwrap_or_else(|error| error.into_inner());
    if current.as_ref() != Some(&active) {
        *current = Some(active);
        GENERATION.fetch_add(1, Ordering::Release);
    }
}

impl Asset {
    /// Checks declared size and digest before decoding or reading an object from the disk cache.
    pub fn verifies(&self, bytes: &[u8]) -> bool {
        self.bytes <= MAX_ASSET_BYTES
            && self.bytes == bytes.len()
            && format!("{:x}", Sha256::digest(bytes)) == self.sha256
    }
}

/// Reads the supported slot vocabulary from the pinned server schema.
pub fn known_slot(slot: &str) -> bool {
    slot_index(slot).is_some()
}

/// Returns a stable slot index from the pinned schema for attachment layers.
pub fn slot_index(slot: &str) -> Option<usize> {
    known_slots().iter().position(|known| known == slot)
}

/// Lists the pinned schema's slot vocabulary in its stable attachment-layer order.
pub fn known_slots() -> &'static [String] {
    static SLOTS: OnceLock<Vec<String>> = OnceLock::new();
    SLOTS.get_or_init(|| {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../core/cinnabarapi/schema/v1.json"))
                .expect("pinned API schema");
        schema["$defs"]["CosmeticSlot"]["x-known-slots"]
            .as_array()
            .expect("slot vocabulary")
            .iter()
            .map(|slot| slot.as_str().expect("slot name").to_owned())
            .collect()
    })
}

/// Rejects unsafe metadata while ignoring item kinds this client cannot render.
pub fn supported(item: &Descriptor) -> bool {
    known_slot(&item.slot)
        && item.id.len() <= 128
        && item
            .id
            .split_once(':')
            .is_some_and(|(namespace, name)| !namespace.is_empty() && !name.is_empty())
        && item.display_name.len() <= 80
        && !item.display_name.is_empty()
        && item.attachment_bone.len() <= 64
        && !item.attachment_bone.is_empty()
        && item
            .offset
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 4.0)
        && [&item.assets.geometry, &item.assets.texture]
            .into_iter()
            .chain(item.assets.animation.iter())
            .all(valid_asset)
}

/// Checks common content-addressed asset metadata before any disk or network access.
pub fn valid_asset(asset: &Asset) -> bool {
    asset.bytes > 0
        && asset.bytes <= MAX_ASSET_BYTES
        && asset.sha256.len() == 64
        && asset
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Resolves only supported, free or explicitly entitled selections in their declared slot.
pub fn resolve(slots: &Slots, owns: impl Fn(&str) -> bool) -> Vec<Item> {
    slots
        .iter()
        .filter_map(|(slot, id)| {
            if !known_slot(slot) {
                return None;
            }
            let item = Item::from_id(id.as_deref()?)?;
            let descriptor = item.descriptor();
            (descriptor.slot == *slot && (descriptor.free || owns(item.id()))).then_some(item)
        })
        .collect()
}

#[cfg(test)]
mod tests;
