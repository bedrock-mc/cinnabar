use std::sync::{Arc, OnceLock};

use super::{ActorSkinPixels, MAX_RENDERED_PLAYERS, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE};

pub const MAX_ACTOR_TEXTURE_ATLAS_SIDE: usize = 4_096;
pub const MAX_ACTOR_TEXTURE_ATLAS_BYTES: usize =
    MAX_ACTOR_TEXTURE_ATLAS_SIDE * MAX_ACTOR_TEXTURE_ATLAS_SIDE * 4;

pub type ActorTexturePixels = ActorSkinPixels;

#[derive(Debug, Clone, PartialEq)]
pub struct ActorTextureAtlas {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
    pub regions: Arc<[[f32; 4]]>,
}

pub(super) fn normalize_skin(skin: Option<&ActorSkinPixels>) -> Vec<u8> {
    skin.and_then(normalize_actor_skin)
        .unwrap_or_else(default_actor_skin_rgba8)
        .to_vec()
}

#[must_use]
pub fn default_actor_skin_rgba8() -> Arc<[u8]> {
    static DEFAULT_SKIN: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(DEFAULT_SKIN.get_or_init(|| generated_default_skin().into()))
}

#[must_use]
pub fn normalize_actor_skin(skin: &ActorSkinPixels) -> Option<Arc<[u8]>> {
    if skin.width != skin.height || !matches!(skin.width, 64 | 128 | 256) {
        return None;
    }
    let side = usize::try_from(skin.width).expect("bounded standard skin side");
    if skin.rgba8.len() != side * side * 4 {
        return None;
    }
    if side == STANDARD_SKIN_SIDE {
        return Some(Arc::clone(&skin.rgba8));
    }
    let mut normalized = vec![0; STANDARD_SKIN_BYTES];
    for y in 0..STANDARD_SKIN_SIDE {
        for x in 0..STANDARD_SKIN_SIDE {
            let source_x = x * side / STANDARD_SKIN_SIDE;
            let source_y = y * side / STANDARD_SKIN_SIDE;
            let source = (source_y * side + source_x) * 4;
            let target = (y * STANDARD_SKIN_SIDE + x) * 4;
            normalized[target..target + 4].copy_from_slice(&skin.rgba8[source..source + 4]);
        }
    }
    Some(normalized.into())
}

fn generated_default_skin() -> Vec<u8> {
    let skin_tone = [198, 134, 91, 255];
    let mut rgba8 = skin_tone.repeat(STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE);
    fill_rect(&mut rgba8, 16, 16, 24, 16, [42, 91, 99, 255]);
    fill_rect(&mut rgba8, 0, 16, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 16, 48, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 8, 8, 8, 8, [112, 72, 48, 255]);
    rgba8
}

fn fill_rect(rgba8: &mut [u8], x: usize, y: usize, width: usize, height: usize, color: [u8; 4]) {
    for py in y..y + height {
        for px in x..x + width {
            let offset = (py * STANDARD_SKIN_SIDE + px) * 4;
            rgba8[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

/// Packs the active, already-culled actor textures into one bounded atlas.
/// Returned regions correspond one-for-one with the input textures. Exact
/// duplicate images share storage and a region.
#[must_use]
pub fn pack_actor_textures(textures: &[ActorTexturePixels]) -> Option<ActorTextureAtlas> {
    if textures.is_empty() || textures.len() > MAX_RENDERED_PLAYERS {
        return None;
    }
    let mut unique = Vec::<ActorTexturePixels>::new();
    let mut input_to_unique = Vec::with_capacity(textures.len());
    for texture in textures {
        let width = usize::try_from(texture.width).ok()?;
        let height = usize::try_from(texture.height).ok()?;
        let expected = width.checked_mul(height)?.checked_mul(4)?;
        if width == 0
            || height == 0
            || width > MAX_ACTOR_TEXTURE_ATLAS_SIDE
            || height > MAX_ACTOR_TEXTURE_ATLAS_SIDE
            || texture.rgba8.len() != expected
        {
            return None;
        }
        let index = unique
            .iter()
            .position(|candidate| {
                candidate.width == texture.width
                    && candidate.height == texture.height
                    && (Arc::ptr_eq(&candidate.rgba8, &texture.rgba8)
                        || candidate.rgba8 == texture.rgba8)
            })
            .unwrap_or_else(|| {
                unique.push(texture.clone());
                unique.len() - 1
            });
        input_to_unique.push(index);
    }

    // Height-first shelf packing is deterministic and gives small vanilla
    // textures useful compaction while retaining a hard 4096x4096/64 MiB cap.
    let mut order = (0..unique.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|&index| {
        (
            std::cmp::Reverse(unique[index].height),
            std::cmp::Reverse(unique[index].width),
            index,
        )
    });
    let mut placements = vec![[0usize; 4]; unique.len()];
    let mut x = 0usize;
    let mut y = 0usize;
    let mut row_height = 0usize;
    let mut used_width = 0usize;
    for index in order {
        let width = unique[index].width as usize;
        let height = unique[index].height as usize;
        if x.checked_add(width)? > MAX_ACTOR_TEXTURE_ATLAS_SIDE {
            y = y.checked_add(row_height)?;
            x = 0;
            row_height = 0;
        }
        if y.checked_add(height)? > MAX_ACTOR_TEXTURE_ATLAS_SIDE {
            return None;
        }
        placements[index] = [x, y, width, height];
        x = x.checked_add(width)?;
        row_height = row_height.max(height);
        used_width = used_width.max(x);
    }
    let used_height = y.checked_add(row_height)?;
    let byte_len = used_width.checked_mul(used_height)?.checked_mul(4)?;
    if used_width == 0 || used_height == 0 || byte_len > MAX_ACTOR_TEXTURE_ATLAS_BYTES {
        return None;
    }
    let mut rgba8 = vec![0; byte_len];
    for (texture, [left, top, width, height]) in unique.iter().zip(placements.iter().copied()) {
        for source_y in 0..height {
            let source = source_y * width * 4;
            let target = ((top + source_y) * used_width + left) * 4;
            rgba8[target..target + width * 4]
                .copy_from_slice(&texture.rgba8[source..source + width * 4]);
        }
    }
    let regions = input_to_unique
        .into_iter()
        .map(|index| {
            let [left, top, width, height] = placements[index];
            [
                (left as f32 + 0.5) / used_width as f32,
                (top as f32 + 0.5) / used_height as f32,
                width.saturating_sub(1) as f32 / used_width as f32,
                height.saturating_sub(1) as f32 / used_height as f32,
            ]
        })
        .collect::<Vec<_>>();
    Some(ActorTextureAtlas {
        width: used_width as u32,
        height: used_height as u32,
        rgba8: rgba8.into(),
        regions: regions.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varying_textures_pack_with_exact_deduplication_and_uv_regions() {
        let wide = ActorTexturePixels {
            width: 2,
            height: 1,
            rgba8: Arc::from([255, 0, 0, 255, 0, 255, 0, 255]),
        };
        let tall = ActorTexturePixels {
            width: 1,
            height: 2,
            rgba8: Arc::from([0, 0, 255, 255, 255, 255, 0, 255]),
        };
        let atlas = pack_actor_textures(&[wide.clone(), tall, wide])
            .expect("bounded varying textures pack");

        assert_eq!((atlas.width, atlas.height), (3, 2));
        assert_eq!(atlas.regions[0], atlas.regions[2]);
        assert_eq!(atlas.regions[0], [0.5, 0.25, 1.0 / 3.0, 0.0]);
        assert_eq!(atlas.regions[1], [1.0 / 6.0, 0.25, 0.0, 0.5]);
        assert_eq!(&atlas.rgba8[0..4], &[0, 0, 255, 255]);
        assert_eq!(&atlas.rgba8[4..8], &[255, 0, 0, 255]);
        assert_eq!(&atlas.rgba8[8..12], &[0, 255, 0, 255]);
        assert_eq!(&atlas.rgba8[12..16], &[255, 255, 0, 255]);
    }

    #[test]
    fn rejects_bad_dimensions_lengths_and_actor_budget() {
        let valid = ActorTexturePixels {
            width: 1,
            height: 1,
            rgba8: Arc::from([1, 2, 3, 4]),
        };
        assert!(
            pack_actor_textures(&[ActorTexturePixels {
                width: MAX_ACTOR_TEXTURE_ATLAS_SIDE as u32 + 1,
                ..valid.clone()
            }])
            .is_none()
        );
        assert!(
            pack_actor_textures(&[ActorTexturePixels {
                rgba8: Arc::from([1, 2, 3]),
                ..valid.clone()
            }])
            .is_none()
        );
        assert!(pack_actor_textures(&vec![valid; MAX_RENDERED_PLAYERS + 1]).is_none());
    }
}
