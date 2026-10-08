//! Native-sized immutable skin pixels avoid repeated expansion before actor selection.
use super::{
    ActorSkinPixels, CLASSIC_SKIN_SIDE, MAX_RENDERED_PLAYERS, STANDARD_SKIN_BYTES,
    STANDARD_SKIN_SIDE, validated_skin_shape,
};
use render_api::SkinRgba8;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
};

/// Class zero is the standard raster; remaining shader classes follow the classic powers of two.
pub const SKIN_CLASS_SIDES: [usize; 4] = [
    STANDARD_SKIN_SIDE,
    CLASSIC_SKIN_SIDE,
    CLASSIC_SKIN_SIDE * 2,
    CLASSIC_SKIN_SIDE * 4,
];
/// Pixel storage stays within the largest admitted player array.
pub const PLAYER_SKIN_BUDGET_BYTES: usize = MAX_RENDERED_PLAYERS * STANDARD_SKIN_BYTES;
/// Source keys and expanded pixels for the draw cap plus the local first-person hand.
const LEGACY_CACHE_BYTES: usize =
    (MAX_RENDERED_PLAYERS + 1) * (STANDARD_SKIN_BYTES + STANDARD_SKIN_BYTES / 2);

type Key = (SkinRgba8, u32);

#[derive(Default)]
struct LegacyCache {
    entries: HashMap<Key, SkinRgba8>,
    order: VecDeque<Key>,
    bytes: usize,
}

/// Square raster dimensions accepted by actor and hand publication.
pub fn actor_skin_side(skin: &SkinRgba8) -> Option<usize> {
    SKIN_CLASS_SIDES
        .into_iter()
        .find(|side| skin.len() == side * side * 4)
}

/// Square sources retain their pixels; half-height sources expand once at their native size.
pub fn prepare_actor_skin_cached(skin: &ActorSkinPixels) -> Option<SkinRgba8> {
    let (side, height) = validated_skin_shape(skin)?;
    if side == height {
        return Some(skin.rgba8.clone());
    }
    static CACHE: OnceLock<Mutex<LegacyCache>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(cache.prepare(skin, side))
}

impl LegacyCache {
    /// Keeps source keys and native expansions within one explicit working-set byte ceiling.
    fn prepare(&mut self, skin: &ActorSkinPixels, side: usize) -> SkinRgba8 {
        let key = (skin.rgba8.clone(), skin.width);
        if let Some(pixels) = self.entries.get(&key) {
            return pixels.clone();
        }
        let pixels: SkinRgba8 = render_api::expand_legacy_skin_rgba8(&skin.rgba8, side).into();
        let bytes = skin.rgba8.len() + pixels.len();
        while self.bytes + bytes > LEGACY_CACHE_BYTES {
            let oldest = self
                .order
                .pop_front()
                .expect("retained bytes own a cache entry");
            let previous = self.entries.remove(&oldest).expect("ordered legacy entry");
            self.bytes -= oldest.0.len() + previous.len();
        }
        self.bytes += bytes;
        self.order.push_back(key.clone());
        self.entries.insert(key, pixels.clone());
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largest_legacy_working_set_retains_source_keys_and_expanded_pixels() {
        let side = STANDARD_SKIN_SIDE;
        let sources: Vec<_> = (0..=MAX_RENDERED_PLAYERS)
            .map(|index| ActorSkinPixels {
                width: side as u32,
                height: side as u32 / 2,
                rgba8: vec![index as u8; side * side * 2].into(),
            })
            .collect();
        let mut cache = LegacyCache::default();
        let prepared: Vec<_> = sources
            .iter()
            .map(|skin| cache.prepare(skin, side))
            .collect();
        for (source, previous) in sources.iter().zip(&prepared) {
            let current = cache.prepare(source, side);
            assert!(std::sync::Arc::ptr_eq(previous.pixels(), current.pixels()));
        }
        assert_eq!(cache.entries.len(), sources.len());
        assert!(cache.bytes <= LEGACY_CACHE_BYTES);
    }

    #[test]
    fn native_skin_rasters_preserve_every_standard_normalized_texel() {
        for side in SKIN_CLASS_SIDES {
            for height in [side, side / 2] {
                let source = ActorSkinPixels {
                    width: side as u32,
                    height: height as u32,
                    rgba8: (0..side * height * 4)
                        .map(|index| index as u8)
                        .collect::<Vec<_>>()
                        .into(),
                };
                let expected = super::super::normalize_actor_skin(&source).unwrap();
                let native = prepare_actor_skin_cached(&source).unwrap();
                assert_eq!(actor_skin_side(&native), Some(side));
                let actual = super::super::normalize_actor_skin(&ActorSkinPixels {
                    width: source.width,
                    height: source.width,
                    rgba8: native.clone(),
                })
                .unwrap();
                assert_eq!(actual.as_ref(), expected.as_ref());
                if height == side {
                    assert!(std::sync::Arc::ptr_eq(
                        native.pixels(),
                        source.rgba8.pixels()
                    ));
                }
                let invalid = ActorSkinPixels {
                    rgba8: source.rgba8[..source.rgba8.len() - 1].to_vec().into(),
                    ..source
                };
                assert!(prepare_actor_skin_cached(&invalid).is_none());
            }
        }
    }
}
