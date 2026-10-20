//! Appearance-owned indices shared by cape and cosmetic attachment sampling.
use assets::{EntityGeometryBone, RuntimeEntityAssets};
use cosmetics::Item;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, mem::size_of};

/// One cape bone's body driver and parent, matched with the original case-insensitive rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapeBoneBinding {
    pub(crate) bone: Option<usize>,
    pub(crate) parent: Option<usize>,
    pub(crate) needs_parent: bool,
}

/// Static attachment indices for one posed skeleton, independent of cosmetic catalog size.
#[derive(Debug)]
pub struct ActorAttachmentBindings {
    cape_key: [u8; 32],
    cape: Box<[CapeBoneBinding]>,
    anchors: Box<[(Box<str>, usize)]>,
}

impl ActorAttachmentBindings {
    /// Resolves cape metadata from the drawing catalog without building a second mesh.
    pub fn new(assets: &RuntimeEntityAssets, names: &[Box<str>]) -> Self {
        let bones = crate::find_geometry_index(assets, assets::CAPE_GEOMETRY_IDENTIFIER)
            .and_then(|index| crate::resolve_geometry_bones(assets, index as usize).ok())
            .unwrap_or_default();
        Self::from_cape(&bones, names)
    }

    /// Binds a cape and indexes each skeleton name, preserving its first ASCII-insensitive match.
    pub fn from_cape(bones: &[EntityGeometryBone], names: &[Box<str>]) -> Self {
        let mut anchors = BTreeMap::new();
        for (index, name) in names.iter().enumerate() {
            anchors
                .entry(name.to_ascii_lowercase().into_boxed_str())
                .or_insert(index);
        }
        let anchors: Box<[_]> = anchors.into_iter().collect();
        let cape = bones
            .iter()
            .map(|bone| CapeBoneBinding {
                bone: lookup(&anchors, &bone.name),
                parent: bone
                    .parent
                    .as_deref()
                    .and_then(|parent| lookup(&anchors, parent)),
                needs_parent: bone.parent.is_some(),
            })
            .collect();
        Self {
            cape,
            cape_key: cape_binding_key(bones),
            anchors,
        }
    }

    /// Returns a cape's ordered body indices without a frame-time name scan.
    pub fn cape(&self) -> &[CapeBoneBinding] {
        &self.cape
    }

    /// Uses prepared indices only for the cape metadata that produced them.
    pub fn cape_for(&self, cape: &crate::cape::CapeRig) -> Option<&[CapeBoneBinding]> {
        (self.cape_key == cape.binding_key).then_some(&self.cape)
    }

    /// Resolves any admitted revision against the skeleton, without retaining catalog items.
    pub fn anchor(&self, item: Item) -> Option<usize> {
        lookup(&self.anchors, &item.descriptor().attachment_bone)
    }

    /// Counts both boxed arrays and every owned name for shared plan admission.
    pub fn retained_bytes(&self) -> Option<usize> {
        let Self {
            cape_key: _,
            cape,
            anchors,
        } = self;
        let arrays = std::mem::size_of_val(cape.as_ref())
            .checked_add(std::mem::size_of_val(anchors.as_ref()))?;
        anchors.iter().try_fold(
            size_of::<Self>().checked_add(arrays)?,
            |bytes, (name, _)| bytes.checked_add(name.len()),
        )
    }
}

/// Searches lowercase skeleton keys without allocating or folding the query into a new string.
fn lookup(anchors: &[(Box<str>, usize)], name: &str) -> Option<usize> {
    anchors
        .binary_search_by(|(bone, _)| {
            bone.bytes()
                .cmp(name.bytes().map(|byte| byte.to_ascii_lowercase()))
        })
        .ok()
        .map(|slot| anchors[slot].1)
}

#[cfg(test)]
thread_local! {
    pub(crate) static ANCHOR_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Identifies only ordered names and parents, the metadata used by cape binding.
pub(crate) fn cape_binding_key(bones: &[EntityGeometryBone]) -> [u8; 32] {
    let mut key = Sha256::new();
    for bone in bones {
        key.update((bone.name.len() as u64).to_le_bytes());
        key.update(bone.name.as_bytes());
        key.update([u8::from(bone.parent.is_some())]);
        if let Some(parent) = &bone.parent {
            key.update((parent.len() as u64).to_le_bytes());
            key.update(parent.as_bytes());
        }
    }
    key.finalize().into()
}
