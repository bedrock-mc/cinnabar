//! Unconditional actor artwork for the explicitly incomplete neutral material profile.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    path::Path,
};

use assets::{
    ActorArtworkBinding, ActorTexture, AssetError, CompiledEntityAssets, MAX_ACTOR_PIXEL_BYTES,
    MAX_ACTOR_TEXTURE_SIDE, MAX_ACTOR_TEXTURES, encode_actor_catalog, encode_entity_blob,
    neutral_actor_geometry_uvs_are_supported,
};
use image::{ImageFormat, ImageReader, Limits};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::entity::compile_entity_assets;
use pack_compile_input::source::read_bounded_source;
mod pack;
mod raster;
pub use pack::{ActorPackCompilation, compile_actor_pack, compile_actor_pack_unless};
use raster::{SharedRaster, admit, prepare_rasters};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActorFallback {
    pub rig: u32,
    pub reason: Box<str>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActorCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub entity_carrier_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub textures: usize,
    pub bindings: usize,
    pub rest_pose_bindings: usize,
    pub pixel_bytes: usize,
    pub texture_evidence: Vec<ActorTextureEvidence>,
    pub fallbacks: Vec<ActorFallback>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActorTextureEvidence {
    pub source_path: Box<str>,
    pub source_sha256: [u8; 32],
    pub width: u16,
    pub height: u16,
    pub pixel_sha256: [u8; 32],
}
pub struct CompiledActorCarrier {
    pub bytes: Vec<u8>,
    pub report: ActorCompileReport,
}

/// Compiles neutral artwork against the resource-only entity carrier.
pub fn compile_actor_assets(
    root: &Path,
    manifest: &[u8],
) -> Result<CompiledActorCarrier, AssetError> {
    let entities = compile_entity_assets(root, manifest)?;
    compile_actor_assets_for_assets(root, &entities)
}

/// Compiles neutral artwork in the exact entity catalog's index space and pins its encoding.
/// Only resource textures are read from `root`, with their recorded size and hash checked.
pub fn compile_actor_assets_for_assets(
    root: &Path,
    entities: &CompiledEntityAssets,
) -> Result<CompiledActorCarrier, AssetError> {
    let entity_bytes = encode_entity_blob(entities)?;
    let runtime_entities = assets::RuntimeEntityAssets::decode(&entity_bytes)?;
    let read = |index: u32| -> Result<Vec<u8>, AssetError> {
        let source = &entities.sources[index as usize];
        let bytes = read_bounded_source(root, &root.join(source.path.as_ref()))?;
        if bytes.len() != source.source_bytes as usize
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
        {
            return Err(invalid("actor source changed after entity compilation"));
        }
        Ok(bytes)
    };
    let ArtworkBuild {
        textures,
        bindings,
        fallbacks,
        pixel_bytes,
    } = build_artwork(entities, &runtime_entities, &read, false)?;
    let bytes = encode_actor_catalog(&entity_bytes, &textures, &bindings)?;
    let report = ActorCompileReport {
        source_manifest_sha256: entities.source_manifest_sha256,
        entity_carrier_sha256: Sha256::digest(&entity_bytes).into(),
        carrier_sha256: Sha256::digest(&bytes).into(),
        textures: textures.len(),
        bindings: bindings.len(),
        rest_pose_bindings: bindings
            .iter()
            .filter(|binding| binding.pose_mode == assets::ActorPoseMode::RestPose)
            .count(),
        pixel_bytes,
        texture_evidence: textures
            .iter()
            .map(|texture| {
                let source = &entities.sources[texture.source as usize];
                ActorTextureEvidence {
                    source_path: source.path.clone(),
                    source_sha256: source.source_sha256,
                    width: texture.width,
                    height: texture.height,
                    pixel_sha256: texture.pixel_sha256,
                }
            })
            .collect(),
        fallbacks,
    };
    Ok(CompiledActorCarrier { bytes, report })
}

/// Artwork for every rig with a decodable texture; `read` returns a source's bytes by index.
struct ArtworkBuild {
    textures: Vec<ActorTexture>,
    bindings: Vec<ActorArtworkBinding>,
    fallbacks: Vec<ActorFallback>,
    pixel_bytes: usize,
}

struct DecodedRaster {
    width: u16,
    height: u16,
    pixels: Vec<u8>,
}

/// Finds rasters selected by the vanilla crystal's controllers without assuming a texture path.
fn native_crystal_sources(entities: &CompiledEntityAssets) -> BTreeSet<u32> {
    entities
        .render
        .layers
        .iter()
        .filter(|layer| {
            let rig = &entities.rig_bindings[layer.rig as usize];
            let symbol = &entities.symbols[rig.entity_symbol as usize];
            symbol.kind == assets::EntityAssetKind::Entity
                && assets::facts::entity::EntityFacts::pinned()
                    .get(&symbol.identifier)
                    .native_class
                    == assets::facts::entity::NativeEntityClass::EnderCrystal
        })
        .flat_map(|layer| {
            entities.render.slots[layer.first_slot as usize..][..usize::from(layer.slot_count)]
                .iter()
                .flat_map(|slot| {
                    entities.render.candidates[slot.first_candidate as usize..]
                        [..usize::from(slot.candidate_count)]
                        .iter()
                        .map(|candidate| candidate.source)
                })
        })
        .collect()
}

/// Geometries each texture source can be drawn with; `None` once a layer scrolls its UVs with
/// `uv_anim`, which can bring any texel on screen.
fn source_geometries(entities: &CompiledEntityAssets) -> BTreeMap<u32, Option<BTreeSet<u32>>> {
    let render = &entities.render;
    let mut drawn = BTreeMap::<u32, Option<BTreeSet<u32>>>::new();
    for layer in &render.layers {
        let rig = &entities.rig_bindings[layer.rig as usize];
        let geometries: BTreeSet<u32> = entities.rig_geometries[rig.first_geometry as usize..]
            [..usize::from(rig.geometry_count)]
            .iter()
            .map(|candidate| candidate.geometry)
            .chain(
                render.geometries[layer.first_geometry as usize..]
                    [..usize::from(layer.geometry_count)]
                    .iter()
                    .map(|choice| choice.geometry),
            )
            .collect();
        for source in layer_sources(render, layer) {
            let entry = drawn.entry(source).or_insert_with(|| Some(BTreeSet::new()));
            match entry {
                Some(set) if layer.uv_anim.is_none() => set.extend(&geometries),
                _ => *entry = None,
            }
        }
    }
    drawn
}

fn layer_sources<'a>(
    render: &'a assets::EntityRenderData,
    layer: &'a assets::EntityRenderLayer,
) -> impl Iterator<Item = u32> + 'a {
    render.slots[layer.first_slot as usize..][..usize::from(layer.slot_count)]
        .iter()
        .flat_map(|slot| {
            render.candidates[slot.first_candidate as usize..][..usize::from(slot.candidate_count)]
                .iter()
                .map(|candidate| candidate.source)
        })
}

/// Decodes each source once under the artwork build's alpha contract.
struct SourceDecoder {
    lenient: bool,
    crystal_sources: BTreeSet<u32>,
    dissolve_masks: BTreeSet<u32>,
    drawn: BTreeMap<u32, Option<BTreeSet<u32>>>,
}

impl SourceDecoder {
    /// Reads and decodes one source without retaining its encoded payload.
    fn decode<B: AsRef<[u8]>>(
        &self,
        entities: &CompiledEntityAssets,
        source: u32,
        read: &(impl Fn(u32) -> Result<B, AssetError> + Sync),
    ) -> Result<Option<DecodedRaster>, AssetError> {
        let asset = &entities.sources[source as usize];
        let binary_alpha = !self.lenient
            && !self.dissolve_masks.contains(&source)
            && !assets::native_actor_texture_preserves_fractional_alpha(asset);
        let sampled = |width: u16, height: u16| {
            let mut union = vec![false; usize::from(width) * usize::from(height)];
            for &geometry in self.drawn.get(&source)?.as_ref()? {
                let texels = assets::neutral_actor_geometry_sampled_texels(
                    &entities.geometries,
                    geometry as usize,
                    width,
                    height,
                )?;
                union
                    .iter_mut()
                    .zip(texels)
                    .for_each(|(union, texel)| *union |= texel);
            }
            Some(union)
        };
        Ok(decode_raster(
            asset.path.as_ref(),
            read(source)?.as_ref(),
            binary_alpha.then_some(&sampled as &dyn Fn(u16, u16) -> Option<Vec<bool>>),
            !self.lenient && self.crystal_sources.contains(&source),
        ))
    }
}

/// Decodes actor art, baking the crystal's native point-sampled alpha test when requested.
/// `binary_alpha` returns the texels a raster of the given size can be sampled at.
fn decode_raster(
    path: &str,
    bytes: &[u8],
    binary_alpha: Option<&dyn Fn(u16, u16) -> Option<Vec<bool>>>,
    crystal: bool,
) -> Option<DecodedRaster> {
    let format = if path.ends_with(".png") {
        ImageFormat::Png
    } else {
        ImageFormat::Tga
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_ACTOR_TEXTURE_SIDE.into());
    limits.max_image_height = Some(MAX_ACTOR_TEXTURE_SIDE.into());
    limits.max_alloc = Some(MAX_ACTOR_PIXEL_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let (width, height) = (
        u16::try_from(image.width()).ok()?,
        u16::try_from(image.height()).ok()?,
    );
    let mut pixels = image.into_rgba8().into_raw();
    // Vanilla ender_crystal inherits entity_alphatest (entity.material); entity.fragment
    // discards alpha below 0.5. Baking coverage keeps the binary carrier contract without
    // rejecting the whole model over the crystal raster's eight alpha-127 texels.
    if crystal {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = if pixel[3] >= 128 { 255 } else { 0 };
        }
    }
    // Neutral opacity rasters require binary alpha. A witnessed native material raster,
    // or a lenient server-pack build, retains every alpha byte instead of quantizing it.
    if let Some(sampled) = binary_alpha
        && !pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| matches!(pixel[3], 0 | 255))
    {
        let sampled = sampled(width, height)?;
        for (pixel, sampled) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(sampled) {
            if !matches!(pixel[3], 0 | 255) {
                if sampled {
                    return None;
                }
                // Point sampling never reads this texel, so no material can show its alpha.
                pixel[3] = 0;
            }
        }
    }
    Some(DecodedRaster {
        width,
        height,
        pixels,
    })
}

/// Builds artwork with parallel source decoding and ordered texture admission.
fn build_artwork<B: AsRef<[u8]>>(
    entities: &CompiledEntityAssets,
    _runtime_entities: &assets::RuntimeEntityAssets,
    read: &(impl Fn(u32) -> Result<B, AssetError> + Sync),
    lenient: bool,
) -> Result<ArtworkBuild, AssetError> {
    let mut textures = Vec::<ActorTexture>::new();
    let mut bindings = Vec::new();
    let mut fallbacks = Vec::new();
    let mut pixel_bytes = 0usize;
    let mut table = BTreeMap::<u32, usize>::new();
    let mut refused = BTreeSet::new();
    let render = &entities.render;
    let decoder = SourceDecoder {
        lenient,
        crystal_sources: native_crystal_sources(entities),
        dissolve_masks: assets::actor_dissolve_mask_sources(render),
        drawn: source_geometries(entities),
    };
    let layer_rasters = render
        .layers
        .iter()
        .flat_map(|layer| layer_sources(render, layer))
        .filter(|&source| {
            lenient
                || entities.sources[source as usize]
                    .path
                    .starts_with("textures/")
        })
        .collect::<BTreeSet<_>>();
    let mut decoded = prepare_rasters(
        layer_rasters,
        |source| decoder.decode(entities, source, read),
        MAX_ACTOR_PIXEL_BYTES,
        MAX_ACTOR_TEXTURES,
    );
    for (rig_index, rig) in entities.rig_bindings.iter().enumerate() {
        let reject = |fallbacks: &mut Vec<ActorFallback>, reason: &str| {
            fallbacks.push(ActorFallback {
                rig: rig_index as u32,
                reason: reason.into(),
            });
        };
        let first = render
            .layers
            .partition_point(|layer| (layer.rig as usize) < rig_index);
        let layers = &render.layers[first..];
        let layers = &layers[..layers.partition_point(|layer| layer.rig as usize == rig_index)];
        let sources: Vec<u32> = layers
            .iter()
            .flat_map(|layer| layer_sources(render, layer))
            .collect();
        if sources.is_empty() {
            reject(&mut fallbacks, "no_render_layer");
            continue;
        }
        // The body route is the base controller's art; a conditional overlay's (such as the
        // charged creeper's armor) must never stand in for it.
        let base_sources: Vec<u32> = layers
            .iter()
            .find(|layer| layer.condition.is_none())
            .or(layers.first())
            .map(|layer| layer_sources(render, layer).collect())
            .unwrap_or_default();
        for offset in 0..usize::from(rig.geometry_count) {
            let candidate_index = rig.first_geometry as usize + offset;
            let candidate = entities.rig_geometries[candidate_index];
            // Out-of-range UVs sample the clamped edge, so a lenient build still draws them.
            if !lenient
                && !neutral_actor_geometry_uvs_are_supported(
                    &entities.geometries,
                    candidate.geometry as usize,
                )
            {
                reject(&mut fallbacks, "uv_extents_or_inheritance");
                continue;
            }
            let mut default_texture = None;
            for &source in &sources {
                // Actor definitions may bind entity art or an item icon.
                if !lenient
                    && !entities.sources[source as usize]
                        .path
                        .starts_with("textures/")
                {
                    continue;
                }
                if refused.contains(&source) {
                    reject(&mut fallbacks, "texture_budget");
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(slot) = decoded.entry(source) {
                    slot.insert(
                        decoder
                            .decode(entities, source, read)?
                            .map(SharedRaster::from),
                    );
                }
                let Some(raster) = decoded.get_mut(&source).unwrap().as_mut() else {
                    continue;
                };
                // UVs use the declared size, independent of the raster resolution.
                let index = match table.get(&source) {
                    Some(&index) => index,
                    None => match admit(
                        &mut textures,
                        &mut pixel_bytes,
                        source,
                        raster,
                        MAX_ACTOR_PIXEL_BYTES,
                    ) {
                        Some(index) => {
                            table.insert(source, index);
                            index
                        }
                        None => {
                            decoded.remove(&source);
                            refused.insert(source);
                            reject(&mut fallbacks, "texture_budget");
                            continue;
                        }
                    },
                };
                if base_sources.contains(&source) {
                    default_texture.get_or_insert(index);
                }
            }
            let Some(texture) = default_texture else {
                reject(&mut fallbacks, "missing_or_ambiguous_texture");
                continue;
            };
            bindings.push(ActorArtworkBinding {
                rig: rig_index as u32,
                geometry_candidate: candidate_index as u32,
                entity_symbol: rig.entity_symbol,
                geometry: candidate.geometry,
                render_controller: rig
                    .render_controller
                    .ok_or_else(|| invalid("drawable rig lacks a render controller"))?,
                texture: texture as u32,
                material: "entity".into(),
                pose_mode: assets::ActorPoseMode::CompiledLiteral,
            });
        }
        // Controllers may draw another geometry with its own normalised UVs.
        let has_layer_geometry = layers
            .iter()
            .flat_map(|layer| {
                render.geometries[layer.first_geometry as usize..]
                    [..usize::from(layer.geometry_count)]
                    .iter()
            })
            .any(|choice| {
                lenient
                    || neutral_actor_geometry_uvs_are_supported(
                        &entities.geometries,
                        choice.geometry as usize,
                    )
            });
        if !has_layer_geometry {
            continue;
        }
        for &source in &sources {
            if table.contains_key(&source)
                || (!lenient
                    && !entities.sources[source as usize]
                        .path
                        .starts_with("textures/"))
            {
                continue;
            }
            if refused.contains(&source) {
                reject(&mut fallbacks, "texture_budget");
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(slot) = decoded.entry(source) {
                slot.insert(
                    decoder
                        .decode(entities, source, read)?
                        .map(SharedRaster::from),
                );
            }
            let Some(raster) = decoded.get_mut(&source).unwrap().as_mut() else {
                continue;
            };
            match admit(
                &mut textures,
                &mut pixel_bytes,
                source,
                raster,
                MAX_ACTOR_PIXEL_BYTES,
            ) {
                Some(index) => {
                    table.insert(source, index);
                }
                None => {
                    decoded.remove(&source);
                    refused.insert(source);
                    reject(&mut fallbacks, "texture_budget");
                }
            }
        }
    }
    Ok(ArtworkBuild {
        textures,
        bindings,
        fallbacks,
        pixel_bytes,
    })
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests;
