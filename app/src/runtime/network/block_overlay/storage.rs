//! Shared texture layers and destination-specific animation bindings.

use assets::{Animation, NO_ANIMATION, TextureRef, texture::flipbook::FlipbookSource};
use pack_runtime::pack_textures::DecodedTexture;

use super::{
    Builder, Source, TextureRoute, TextureSlot,
    texture_admission::{AdmittedTexture, FrameFailure},
    textures::{FrameSequence, admit_static_rectangle, flipbook_frames},
};

/// A retained physical frame sequence, reusable by independently timed destinations.
#[derive(Clone, Copy)]
pub(super) struct FrameStorage {
    page: u32,
    layer: u32,
    frame_start: u32,
    frame_count: u32,
}

/// Physical failures are shared; timing failures belong only to the requested destination.
#[derive(Debug)]
enum AdmissionFailure {
    Source,
    Timing,
}

mod placement;
mod resolution;

#[cfg(test)]
mod tests;

impl Builder<'_> {
    /// Reuses source frames and binds their destination without allocating duplicate rasters.
    pub(super) fn load_texture(
        &mut self,
        route: &TextureRoute,
        carrier: Option<&assets::material_keys::MaterialKeys>,
    ) -> Option<AdmittedTexture> {
        let identity = self.catalog.identity(route)?;
        let source = identity.flipbook.as_ref();
        if let Some(source) = source
            && let Some((storage, pack)) = self.source_storage(route, Some(source), carrier)
            && let Some(slot) = self.bind_frames(storage, Some(source), identity.selector)
        {
            return Some(AdmittedTexture { slot, pack });
        }
        let (storage, pack) = self.source_storage(route, None, carrier)?;
        let slot = self.bind_frames(storage, None, identity.selector)?;
        Some(AdmittedTexture { slot, pack })
    }

    /// Caches source failures and resolved image frames independently of destination timing.
    fn source_storage(
        &mut self,
        route: &TextureRoute,
        source: Option<&FlipbookSource>,
        carrier: Option<&assets::material_keys::MaterialKeys>,
    ) -> Option<(FrameStorage, bool)> {
        let key = route.key();
        let storage_key = self.catalog.storage_key(route, source)?;
        let carrier_policy = carrier.is_some() && source.is_none();
        if let Some(storage) = self.slots.frames(&storage_key, false)
            && (storage.is_ok() || !carrier_policy)
        {
            return storage.ok().map(|storage| (storage, true));
        }
        if carrier_policy && let Some(storage) = self.slots.frames(&storage_key, true) {
            return storage.ok().map(|storage| (storage, false));
        }
        if let Some((path, pack)) = self
            .catalog
            .reusable_image_path(route, source, |path, tint| {
                super::textures::carrier_image_path(self.base_assets?, carrier?, path, tint)
            })
        {
            let resolved_key = storage_key.clone().with_image(path);
            // An indexed pack filename is reusable only from a successful pack decode.
            // A failed pack may fall back to a different carrier filename during decoding.
            if let Some(Ok(storage)) = self.slots.frames(&resolved_key, !pack) {
                self.slots.insert_frames(storage_key, Ok(storage), !pack);
                return Some((storage, pack));
            }
        }
        if self.remaining_layers() == 0 {
            // Missing pack pixels can still borrow a static base-page texture,
            // which needs no overlay layer. Present sources remain refusals.
            if source.is_none() && !carrier_policy && self.catalog.static_source_missing(route) {
                self.slots
                    .insert_frames(storage_key, Err(FrameFailure::Missing), false);
                return None;
            }
            if source.is_none() {
                self.remaining_capacity(key);
            }
            self.slots
                .insert_frames(storage_key, Err(FrameFailure::Rejected), carrier_policy);
            return None;
        }
        let Some(image) = self.catalog.decode_source(route, source, |path, tint| {
            super::textures::carrier_image(self.base_assets?, carrier?, path, tint)
        }) else {
            self.slots
                .insert_frames(storage_key, Err(FrameFailure::Missing), carrier_policy);
            return None;
        };
        let resolved_key = storage_key.clone().with_image(image.image_path);
        if let Some(storage) = self.slots.frames(&resolved_key, !image.pack) {
            self.slots.insert_frames(storage_key, storage, !image.pack);
            return storage.ok().map(|storage| (storage, image.pack));
        }
        let admission = self.store_frames(key, storage_key.grid, image.texture, source);
        let storage = match admission {
            Err(AdmissionFailure::Timing) => return None,
            Ok(storage) => Ok(storage),
            Err(AdmissionFailure::Source) => Err(FrameFailure::Rejected),
        };
        self.slots.insert_frames(resolved_key, storage, !image.pack);
        self.slots.insert_frames(storage_key, storage, !image.pack);
        storage.ok().map(|storage| (storage, image.pack))
    }

    /// Returns available layers, counting a full-array rejection after cached images are checked.
    fn remaining_capacity(&mut self, key: &str) -> Option<usize> {
        let available = self.remaining_layers();
        if available == 0 {
            self.gaps.saturated_textures += 1;
            self.catalog.report_full(key);
            return None;
        }
        Some(available)
    }

    /// Admits unique pixels and timeline references without retaining a failed timing binding.
    fn store_frames(
        &mut self,
        key: &str,
        grid: u8,
        mut texture: DecodedTexture,
        source: Option<&FlipbookSource>,
    ) -> Result<FrameStorage, AdmissionFailure> {
        let [width, height] = match source {
            Some(source) => {
                let side = (texture.width.min(texture.height) / source.replicate.max(1)).max(1);
                [side.min(self.texture_policy.max_side()); 2]
            }
            None => {
                admit_static_rectangle(&mut texture);
                [texture.width, texture.height]
            }
        };
        self.texture_policy
            .source_bytes(width, height, false)
            .ok_or(AdmissionFailure::Source)?;
        // Byte pressure reduces resolution; only layer exhaustion can shorten a timeline.
        let available = self.remaining_layers();
        if available == 0 {
            self.catalog.report_budget(key);
            return Err(AdmissionFailure::Source);
        }
        let mut frames = match source {
            Some(source) => {
                let frames =
                    flipbook_frames(&texture, source, available, self.texture_policy.max_side());
                drop(texture);
                frames
            }
            None => FrameSequence {
                frames: vec![texture],
                timeline: vec![0],
            },
        };
        if frames.frames.is_empty() {
            return Err(AdmissionFailure::Source);
        }
        if frames.frames.iter().any(|frame| {
            [frame.width, frame.height].into_iter().any(|axis| {
                axis.checked_shr(u32::from(grid))
                    .is_none_or(|size| size == 0)
            })
        }) {
            return Err(AdmissionFailure::Source);
        }
        let frame_count = frames.timeline.len() as u32;
        if frame_count > 1
            && self
                .overlay
                .animation_frames
                .len()
                .checked_add(frame_count as usize)
                .is_none_or(|total| total > assets::MAX_ANIMATION_FRAMES)
        {
            self.catalog.report_budget(key);
            return Err(AdmissionFailure::Source);
        }
        if source.is_some_and(|source| frame_count.checked_mul(source.ticks_per_frame).is_none()) {
            return Err(AdmissionFailure::Timing);
        }
        let Some((placements, tiles)) = self.fit_frames(&mut frames.frames, grid) else {
            self.catalog.report_budget(key);
            return Err(AdmissionFailure::Source);
        };
        self.resize_pages(&tiles);
        let admitted = frames.frames.len();
        let added_bytes: usize = frames.frames.iter().map(|frame| frame.rgba8.len()).sum();
        self.source_bytes += added_bytes;
        let mut references = Vec::with_capacity(admitted);
        let mut physical_frames = frames.frames.into_iter();
        for placement in placements {
            self.tiles
                .resize(self.tiles.len().max(placement.shard + 1), super::MIN_TILE);
            self.tiles[placement.shard] = placement.tile;
            self.upload_bytes += placement.upload_bytes;
            for (offset, frame) in physical_frames.by_ref().take(placement.count).enumerate() {
                references.push(
                    TextureRef::new(
                        placement.shard as u32 + self.overlay.base_texture_pages,
                        placement.layer + offset as u32,
                    )
                    .expect("bounded shard layer"),
                );
                self.sources.push((
                    if grid == 0 {
                        Source::Image(frame)
                    } else {
                        Source::GridImage(frame, grid)
                    },
                    placement.shard,
                ));
            }
        }
        let first = references[0];
        let frame_start = self.overlay.animation_frames.len() as u32;
        if frame_count > 1 {
            self.overlay.animation_frames.extend(
                frames
                    .timeline
                    .into_iter()
                    .map(|index| references[index as usize]),
            );
        }
        Ok(FrameStorage {
            page: first.page(),
            layer: first.layer(),
            frame_start,
            frame_count,
        })
    }

    /// Associates shared frames with this destination's timing, blending, and selectors.
    fn bind_frames(
        &mut self,
        storage: FrameStorage,
        source: Option<&FlipbookSource>,
        selector: (u32, u32),
    ) -> Option<TextureSlot> {
        let animation = match source {
            Some(source) if storage.frame_count > 1 => {
                storage.frame_count.checked_mul(source.ticks_per_frame)?;
                self.overlay.animations.push(Animation {
                    frame_start: storage.frame_start,
                    frame_count: storage.frame_count,
                    ticks_per_frame: source.ticks_per_frame,
                    atlas_index: selector.0,
                    atlas_tile_variant: selector.1,
                    replicate: source.replicate,
                    flags: u32::from(source.blend_frames) * assets::ANIMATION_FLAG_BLEND,
                });
                self.overlay.animations.len() as u32 - 1
            }
            _ => NO_ANIMATION,
        };
        Some(TextureSlot {
            page: storage.page,
            layer: storage.layer,
            animation,
        })
    }
}
