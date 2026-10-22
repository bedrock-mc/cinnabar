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
use image::{ColorType, ImageDecoder, ImageFormat, ImageReader, Limits};
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
    keeps_alpha: BTreeSet<u32>,
}

impl SourceDecoder {
    /// Decodes a source, rounding its alpha unless a drawing material reads it.
    fn decode(
        &self,
        entities: &CompiledEntityAssets,
        source: u32,
        bytes: &[u8],
    ) -> Option<DecodedRaster> {
        decode_raster(
            entities.sources[source as usize].path.as_ref(),
            bytes,
            !self.lenient && !self.keeps_alpha.contains(&source),
        )
    }
}

/// Creates the same bounded image reader for header inspection and full decoding.
fn raster_reader<'a>(path: &str, bytes: &'a [u8]) -> ImageReader<Cursor<&'a [u8]>> {
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
    reader
}

/// Bounds decoded pixels and any color-conversion buffer without allocating those pixels.
fn raster_decoded_bytes(path: &str, bytes: &[u8]) -> usize {
    let Ok(decoder) = raster_reader(path, bytes).into_decoder() else {
        return usize::MAX;
    };
    let (width, height) = decoder.dimensions();
    let rgba = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    let staged = if decoder.color_type() == ColorType::Rgba8 {
        rgba
    } else {
        rgba.saturating_add(decoder.total_bytes())
    };
    usize::try_from(staged).unwrap_or(usize::MAX)
}

/// Decodes actor art; `round_alpha` rounds every alpha byte at 0.5, which is how a material
/// with an unverified definition draws (see [`assets::actor_sources_keeping_alpha`]).
fn decode_raster(path: &str, bytes: &[u8], round_alpha: bool) -> Option<DecodedRaster> {
    let image = raster_reader(path, bytes).decode().ok()?;
    let (width, height) = (
        u16::try_from(image.width()).ok()?,
        u16::try_from(image.height()).ok()?,
    );
    let mut pixels = image.into_rgba8().into_raw();
    if round_alpha {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = if pixel[3] >= 128 { 255 } else { 0 };
        }
    }
    Some(DecodedRaster {
        width,
        height,
        pixels,
    })
}

/// Builds artwork with parallel source decoding and ordered texture admission.
fn build_artwork<B: AsRef<[u8]> + Send>(
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
        keeps_alpha: assets::actor_sources_keeping_alpha(render, &entities.sources),
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
        |source| {
            let bytes = read(source)?;
            let size =
                raster_decoded_bytes(&entities.sources[source as usize].path, bytes.as_ref());
            let decoder = &decoder;
            Ok((size, move || {
                decoder.decode(entities, source, bytes.as_ref())
            }))
        },
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
        // The body route is the base controller's first texture; a conditional overlay's (such
        // as the charged creeper's armor) must never stand in for it.
        let base_slot = layers
            .iter()
            .find(|layer| layer.condition.is_none())
            .or(layers.first())
            .filter(|layer| layer.slot_count > 0)
            .map(|layer| render.slots[layer.first_slot as usize]);
        let base_candidates = base_slot.map_or(&[][..], |slot| {
            &render.candidates[slot.first_candidate as usize..][..usize::from(slot.candidate_count)]
        });
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
            // The texture this geometry's own selection draws, else the default branch; never
            // another branch, whose layout can differ (a baby texture on the adult model).
            let route_source = candidate
                .condition
                .and_then(|condition| {
                    base_candidates
                        .iter()
                        .find(|texture| texture.condition == Some(condition))
                })
                .or_else(|| base_candidates.get(usize::from(base_slot?.default_candidate?)))
                .or(base_candidates.first())
                .map(|texture| texture.source);
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
                    let bytes = read(source)?;
                    slot.insert(
                        decoder
                            .decode(entities, source, bytes.as_ref())
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
                if route_source == Some(source) {
                    default_texture = Some(index);
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
                let bytes = read(source)?;
                slot.insert(
                    decoder
                        .decode(entities, source, bytes.as_ref())
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
