//! Parallel decoded-raster staging and shared, budgeted actor texture pixels.

use super::{ActorTexture, AssetError, DecodedRaster, MAX_ACTOR_TEXTURES};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
    sync::Arc,
};

pub(super) struct SharedRaster {
    width: u16,
    height: u16,
    pixels: Arc<[u8]>,
}

impl From<DecodedRaster> for SharedRaster {
    fn from(raster: DecodedRaster) -> Self {
        Self {
            width: raster.width,
            height: raster.height,
            pixels: raster.pixels.into(),
        }
    }
}

#[derive(Default)]
struct RasterPlan {
    queue: BinaryHeap<(usize, Reverse<u32>, u16, u16)>,
    total: usize,
}

impl RasterPlan {
    /// Adds one usable raster, preserving the source-index tie break of global planning.
    fn insert(&mut self, source: u32, width: u16, height: u16) {
        let bytes = raster_bytes(width, height);
        self.total = self.total.saturating_add(bytes);
        self.queue.push((bytes, Reverse(source), width, height));
    }

    /// Plans one largest-first halving while the retained prefix exceeds `budget`.
    fn shrink(&mut self, budget: usize) -> Option<u32> {
        if self.total <= budget {
            return None;
        }
        let (bytes, Reverse(source), width, height) = self.queue.pop()?;
        if width.max(height) <= 1 {
            return None;
        }
        let (width, height) = (width.div_ceil(2), height.div_ceil(2));
        let halved = raster_bytes(width, height);
        self.total = self.total - bytes + halved;
        self.queue.push((halved, Reverse(source), width, height));
        Some(source)
    }
}

/// Stages unique sources up to `limit` usable rasters, shrinking between decodes to fit `budget`.
/// The budget must hold at least one RGBA pixel per admitted raster.
/// Failed reads remain absent for the owning rig to retry; unusable images remain cached.
/// Largest-first planning is prefix-monotone: added rasters cannot undo an earlier halving.
pub(super) fn prepare_rasters(
    sources: impl IntoIterator<Item = u32>,
    decode: impl Fn(u32) -> Result<Option<DecodedRaster>, AssetError> + Sync,
    budget: usize,
    limit: usize,
) -> BTreeMap<u32, Option<SharedRaster>> {
    let mut rasters = BTreeMap::<u32, Option<DecodedRaster>>::new();
    let mut plan = RasterPlan::default();
    let mut usable = 0;
    let mut sources = sources.into_iter();
    while usable < limit {
        let batch = sources
            .by_ref()
            .take((limit - usable).min(rayon::current_num_threads().max(1)))
            .collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        let decoded = batch
            .par_iter()
            .map(|&source| decode(source))
            .collect::<Vec<_>>();
        for (source, raster) in batch.into_iter().zip(decoded) {
            let Ok(raster) = raster else {
                continue;
            };
            if let Some(raster) = &raster {
                plan.insert(source, raster.width, raster.height);
                usable += 1;
            }
            rasters.insert(source, raster);
            while let Some(source) = plan.shrink(budget) {
                let raster = rasters.get_mut(&source).unwrap().as_mut().unwrap();
                raster.pixels = halve(&raster.pixels, raster.width, raster.height);
                (raster.width, raster.height) =
                    (raster.width.div_ceil(2), raster.height.div_ceil(2));
            }
        }
    }
    rasters
        .into_iter()
        .map(|(source, raster)| (source, raster.map(SharedRaster::from)))
        .collect()
}

/// Adds shared prepared pixels, shrinking further only when earlier admissions consumed space.
pub(super) fn admit(
    textures: &mut Vec<ActorTexture>,
    pixel_bytes: &mut usize,
    source: u32,
    raster: &mut SharedRaster,
    budget: usize,
) -> Option<usize> {
    if textures.len() == MAX_ACTOR_TEXTURES {
        return None;
    }
    while pixel_bytes.saturating_add(raster.pixels.len()) > budget {
        if raster.width.max(raster.height) <= 1 {
            return None;
        }
        raster.pixels = halve(&raster.pixels, raster.width, raster.height).into();
        (raster.width, raster.height) = (raster.width.div_ceil(2), raster.height.div_ceil(2));
    }
    *pixel_bytes += raster.pixels.len();
    textures.push(ActorTexture {
        source,
        width: raster.width,
        height: raster.height,
        pixel_sha256: Sha256::digest(&raster.pixels).into(),
        rgba8: Arc::clone(&raster.pixels),
    });
    Some(textures.len() - 1)
}

/// RGBA8 bytes of a `width` by `height` raster.
fn raster_bytes(width: u16, height: u16) -> usize {
    usize::from(width) * usize::from(height) * 4
}

/// 2x2 box filter of an RGBA8 raster; odd edges average the texels they have.
fn halve(pixels: &[u8], width: u16, height: u16) -> Vec<u8> {
    let (width, height) = (usize::from(width), usize::from(height));
    let (out_width, out_height) = (width.div_ceil(2), height.div_ceil(2));
    let mut output = Vec::with_capacity(out_width * out_height * 4);
    for y in 0..out_height {
        for x in 0..out_width {
            let mut sum = [0u32; 4];
            let mut count = 0;
            for source_y in 2 * y..(2 * y + 2).min(height) {
                for source_x in 2 * x..(2 * x + 2).min(width) {
                    let at = (source_y * width + source_x) * 4;
                    for (total, value) in sum.iter_mut().zip(&pixels[at..at + 4]) {
                        *total += u32::from(*value);
                    }
                    count += 1;
                }
            }
            output.extend(sum.map(|total| (total / count) as u8));
        }
    }
    output
}

#[cfg(test)]
mod tests;
