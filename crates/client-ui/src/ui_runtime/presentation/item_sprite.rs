//! Straight-alpha item icon pixels for world-space item meshes.

use {super::UiPresentationRuntime, ui::IconRef};

/// One item icon cropped from a UI atlas page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpritePixels {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

impl UiPresentationRuntime {
    /// The icon for `identifier`/`metadata` reduced by an integer factor to fit `max_side`.
    pub fn item_sprite(
        &self,
        identifier: &str,
        metadata: u32,
        max_side: u32,
    ) -> Option<ItemSpritePixels> {
        let icon = self.item_icon(identifier, metadata)?;
        crop_icon(self, icon, max_side)
    }

    /// Native carried block faces, not the inventory's projected cube thumbnail or tinted
    /// world materials. Both catalogs must have been built from the same pinned pack.
    pub fn carried_block_cube(
        &self,
        visual: u32,
        source_manifest_sha256: [u8; 32],
    ) -> Option<render_model::DroppedItemCube> {
        let catalog = self.icon_catalog.as_deref()?;
        if catalog.source_manifest_sha256() != source_manifest_sha256 {
            return None;
        }
        let index = catalog
            .block_sheets()
            .binary_search_by_key(&visual, |sheet| sheet.visual.0)
            .ok()?;
        let sprite = catalog
            .sprites()
            .get(catalog.block_sheets()[index].sprite as usize)?;
        carried_cube(sprite)
    }
}

fn carried_cube(sprite: &assets::IconSprite) -> Option<render_model::DroppedItemCube> {
    use assets::{BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_SHEET_GRID, BLOCK_ITEM_SHEET_SIZE};
    if [sprite.width, sprite.height] != BLOCK_ITEM_SHEET_SIZE
        || sprite.rgba8.len() != usize::from(sprite.width) * usize::from(sprite.height) * 4
    {
        return None;
    }
    let side = usize::from(BLOCK_ITEM_FACE_SIDE);
    let columns = usize::from(BLOCK_ITEM_SHEET_GRID[0]);
    let faces = std::array::from_fn(|face| {
        let (x, y) = (face % columns * side, face / columns * side);
        let mut pixels = Vec::with_capacity(side * side * 4);
        for row in 0..side {
            let start = ((y + row) * usize::from(sprite.width) + x) * 4;
            pixels.extend_from_slice(&sprite.rgba8[start..start + side * 4]);
        }
        std::sync::Arc::from(pixels)
    });
    Some(render_model::DroppedItemCube {
        tile: u32::from(BLOCK_ITEM_FACE_SIDE),
        faces,
        tints: [render_model::OPAQUE_WHITE; 6],
    })
}

fn crop_icon(
    runtime: &UiPresentationRuntime,
    icon: IconRef,
    max_side: u32,
) -> Option<ItemSpritePixels> {
    let width = u32::from(icon.uv[2].checked_sub(icon.uv[0])?);
    let height = u32::from(icon.uv[3].checked_sub(icon.uv[1])?);
    if width == 0 || height == 0 {
        return None;
    }
    let page = runtime.textures.pages().get(usize::from(icon.page))?;
    let [page_width, page_height] = page.dimensions();
    if u32::from(icon.uv[2]) > page_width || u32::from(icon.uv[3]) > page_height {
        return None;
    }
    let factor = reduction_factor(width, height, max_side)?;
    let (out_width, out_height) = (width / factor, height / factor);
    let pixels = page.pixels();
    let mut rgba8 = Vec::with_capacity((out_width * out_height * 4) as usize);
    for y in 0..out_height {
        for x in 0..out_width {
            // Nearest sample from the middle of each reduced block.
            let source_x = u32::from(icon.uv[0]) + x * factor + factor / 2;
            let source_y = u32::from(icon.uv[1]) + y * factor + factor / 2;
            let offset = ((source_y * page_width + source_x) * 4) as usize;
            rgba8.extend_from_slice(pixels.get(offset..offset + 4)?);
        }
    }
    Some(ItemSpritePixels {
        width: out_width,
        height: out_height,
        rgba8,
    })
}

/// Selects an integer sampling factor that preserves both icon dimensions.
fn reduction_factor(width: u32, height: u32, max_side: u32) -> Option<u32> {
    if width == 0 || height == 0 {
        return None;
    }
    let minimum = width.max(height).div_ceil(max_side.max(1));
    let (mut a, mut b) = (width, height);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    (minimum..=a).find(|factor| a.is_multiple_of(*factor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carried_faces_are_not_biome_tinted_or_reprojected() {
        let tiles = std::array::from_fn(|face| assets::IconSprite {
            width: assets::BLOCK_ITEM_FACE_SIDE,
            height: assets::BLOCK_ITEM_FACE_SIDE,
            rgba8: [face as u8, 50, 80, 255]
                .repeat(usize::from(assets::BLOCK_ITEM_FACE_SIDE).pow(2))
                .into(),
        });
        let sprite = assets::compose_block_item_sheet(&tiles).unwrap();
        let cube = carried_cube(&sprite).unwrap();
        for (face, pixels) in cube.faces.iter().enumerate() {
            assert_eq!(pixels.as_ref(), tiles[face].rgba8.as_ref());
            assert_eq!(cube.tints[face], render_model::OPAQUE_WHITE);
        }
        assert!(carried_cube(&tiles[0]).is_none());
    }
    #[test]
    fn review_icon_reduction_finds_a_larger_common_divisor() {
        assert_eq!(reduction_factor(16, 16, 6), Some(4));
        assert_eq!(reduction_factor(70, 70, 20), Some(5));
        assert_eq!(reduction_factor(70, 35, 20), Some(5));
    }
}
