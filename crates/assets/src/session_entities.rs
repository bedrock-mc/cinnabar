//! Immutable server-pack actor artwork shared by compilation and presentation.
use crate::{
    ActorArtworkBinding, ActorPoseMode, ActorTexture, AssetError, RuntimeEntityAssets,
    RuntimeEquipmentCatalog,
};
use std::sync::Arc;
/// The pack's entity catalog with the artwork of its eligible rigs.
#[derive(Debug)]
pub struct SessionEntityPack {
    pub assets: Arc<RuntimeEntityAssets>,
    pub textures: Arc<[ActorTexture]>,
    pub bindings: Arc<[ActorArtworkBinding]>,
    /// The pack's attachable bindings and rasters for held and worn items.
    pub equipment: Option<Arc<crate::RuntimeEquipmentCatalog>>,
}

impl SessionEntityPack {
    /// Bytes [`Self::decode`] reads back; `entity_blob` is the encoding `assets` was built from.
    pub fn encode(&self, entity_blob: &[u8]) -> Result<Vec<u8>, AssetError> {
        let actor = encode_artwork(&self.textures, &self.bindings);
        let equipment = self
            .equipment
            .as_deref()
            .map(|catalog| {
                crate::encode_equipment_catalog_with_textures(
                    catalog.source_manifest_sha256(),
                    catalog.entity_blob_sha256(),
                    catalog.bindings(),
                    catalog.textures(),
                )
            })
            .transpose()?;
        let mut bytes = Vec::new();
        for section in [
            Some(entity_blob),
            Some(actor.as_slice()),
            equipment.as_deref(),
        ] {
            let Some(section) = section else { continue };
            bytes.extend_from_slice(&(section.len() as u64).to_le_bytes());
            bytes.extend_from_slice(section);
        }
        Ok(bytes)
    }

    /// Decodes each section; the entity and equipment sections pass their carriers' checks.
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        let mut rest = bytes;
        let mut section = || -> Result<Option<&[u8]>, AssetError> {
            if rest.is_empty() {
                return Ok(None);
            }
            let invalid = || AssetError::InvalidCompiledAssets {
                detail: "session entity pack section is truncated".into(),
            };
            let (length, tail) = rest.split_at_checked(8).ok_or_else(invalid)?;
            let length = usize::try_from(u64::from_le_bytes(length.try_into().unwrap()))
                .map_err(|_| invalid())?;
            let (body, tail) = tail.split_at_checked(length).ok_or_else(invalid)?;
            rest = tail;
            Ok(Some(body))
        };
        let missing = || AssetError::InvalidCompiledAssets {
            detail: "session entity pack section is missing".into(),
        };
        let entity_blob = section()?.ok_or_else(missing)?;
        let (textures, bindings) =
            decode_artwork(section()?.ok_or_else(missing)?).ok_or_else(|| {
                AssetError::InvalidCompiledAssets {
                    detail: "session entity artwork is malformed".into(),
                }
            })?;
        let equipment = section()?
            .map(RuntimeEquipmentCatalog::decode)
            .transpose()?
            .map(Arc::new);
        if section()?.is_some() {
            return Err(AssetError::InvalidCompiledAssets {
                detail: "session entity pack has trailing bytes".into(),
            });
        }
        Ok(Self {
            assets: Arc::new(RuntimeEntityAssets::decode(entity_blob)?),
            textures: textures.into(),
            bindings: bindings.into(),
            equipment,
        })
    }
}

/// Server artwork keeps bindings the vanilla actor carrier would reject, so it is stored as is.
fn encode_artwork(textures: &[ActorTexture], bindings: &[ActorArtworkBinding]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(textures.len() as u32).to_le_bytes());
    for texture in textures {
        bytes.extend_from_slice(&texture.source.to_le_bytes());
        bytes.extend_from_slice(&texture.width.to_le_bytes());
        bytes.extend_from_slice(&texture.height.to_le_bytes());
        bytes.extend_from_slice(&texture.pixel_sha256);
        bytes.extend_from_slice(&(texture.rgba8.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&texture.rgba8);
    }
    bytes.extend_from_slice(&(bindings.len() as u32).to_le_bytes());
    for binding in bindings {
        for value in [
            binding.rig,
            binding.geometry_candidate,
            binding.entity_symbol,
            binding.geometry,
            binding.render_controller,
            binding.texture,
            binding.pose_mode as u32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&(binding.material.len() as u32).to_le_bytes());
        bytes.extend_from_slice(binding.material.as_bytes());
    }
    bytes
}

fn decode_artwork(bytes: &[u8]) -> Option<(Vec<ActorTexture>, Vec<ActorArtworkBinding>)> {
    let mut rest = bytes;
    let mut take = |length: usize| -> Option<&[u8]> {
        let (head, tail) = rest.split_at_checked(length)?;
        rest = tail;
        Some(head)
    };
    let u32_of = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap());
    let u16_of = |bytes: &[u8]| u16::from_le_bytes(bytes.try_into().unwrap());
    let texture_count = u32_of(take(4)?) as usize;
    let mut textures = Vec::with_capacity(texture_count.min(crate::MAX_ACTOR_TEXTURES));
    for _ in 0..texture_count {
        let source = u32_of(take(4)?);
        let width = u16_of(take(2)?);
        let height = u16_of(take(2)?);
        let pixel_sha256 = take(32)?.try_into().ok()?;
        let length = usize::try_from(u64::from_le_bytes(take(8)?.try_into().ok()?)).ok()?;
        if length != usize::from(width) * usize::from(height) * 4 {
            return None;
        }
        textures.push(ActorTexture {
            source,
            width,
            height,
            pixel_sha256,
            rgba8: Arc::from(take(length)?),
        });
    }
    let binding_count = u32_of(take(4)?) as usize;
    let mut bindings = Vec::with_capacity(binding_count.min(crate::MAX_ACTOR_BINDINGS));
    for _ in 0..binding_count {
        let mut field = || Some(u32_of(take(4)?));
        let [
            rig,
            geometry_candidate,
            entity_symbol,
            geometry,
            render_controller,
            texture,
            pose,
        ] = [(); 7].map(|_| field());
        let pose_mode = match pose? {
            0 => ActorPoseMode::CompiledLiteral,
            1 => ActorPoseMode::RestPose,
            _ => return None,
        };
        let length = u32_of(take(4)?) as usize;
        let material = std::str::from_utf8(take(length)?).ok()?.into();
        if usize::try_from(texture?).ok()? >= textures.len() {
            return None;
        }
        bindings.push(ActorArtworkBinding {
            rig: rig?,
            geometry_candidate: geometry_candidate?,
            entity_symbol: entity_symbol?,
            geometry: geometry?,
            render_controller: render_controller?,
            texture: texture?,
            material,
            pose_mode,
        });
    }
    rest.is_empty().then_some((textures, bindings))
}
