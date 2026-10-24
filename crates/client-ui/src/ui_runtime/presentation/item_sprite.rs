//! Straight-alpha item icon pixels for world-space item meshes.

use render_model::dropped_item::ItemModelCost;
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
        self.resolved_item_sprite(icon, max_side)
    }

    /// Crops the selected artwork without resolving the item identity again.
    pub fn resolved_item_sprite(&self, icon: IconRef, max_side: u32) -> Option<ItemSpritePixels> {
        crop_icon(self, icon, max_side)
    }

    /// Measures the exact reduced sprite directly from its atlas, before cropping pixels.
    pub fn resolved_item_cost(
        &self,
        icon: IconRef,
        max_side: u32,
        native: bool,
    ) -> Option<ItemModelCost> {
        let source = IconPixels::new(self, icon, max_side)?;
        let vertices = render_model::sampled_sprite_vertex_count(
            source.width as usize,
            source.height as usize,
            |x, y| source.pixel(x as u32, y as u32)[3] >= if native { 2 } else { 1 },
        )?;
        ItemModelCost::new(
            source.width.max(source.height),
            1,
            (source.width * source.height * 4) as usize,
            vertices,
        )
    }

    /// Measures a session sheet and its authored geometry without copying either source.
    pub fn session_block_cost(
        &self,
        identifier: &str,
        metadata: u32,
        sheet: IconRef,
    ) -> Option<ItemModelCost> {
        let source = IconPixels::new(self, sheet, assets::MAX_TILE_SIZE)?;
        let grid = assets::BLOCK_ITEM_SHEET_GRID.map(u32::from);
        let [width, height] = [source.width / grid[0], source.height / grid[1]];
        let vertices =
            if let Some((_, vertices)) = self.session_icons.held_block(identifier, metadata) {
                if width == 0
                    || height == 0
                    || source.width != width * grid[0]
                    || source.height != height * grid[1]
                {
                    return None;
                }
                render_model::equipment::blocks::dropped_vertex_count(&vertices)?
            } else {
                if [source.width, source.height] != assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from) {
                    return None;
                }
                36
            };
        ItemModelCost::new(
            width.max(height),
            assets::BlockFace::ALL.len(),
            (width * height * 4) as usize * assets::BlockFace::ALL.len(),
            vertices,
        )
    }

    /// Measures a matching startup sheet without allocating its six face buffers.
    pub fn carried_block_cost(
        &self,
        visual: u32,
        source_manifest_sha256: [u8; 32],
    ) -> Option<ItemModelCost> {
        let sprite = self.carried_block_sheet(visual, source_manifest_sha256)?;
        if [sprite.width, sprite.height] != assets::BLOCK_ITEM_SHEET_SIZE
            || sprite.rgba8.len() != usize::from(sprite.width) * usize::from(sprite.height) * 4
        {
            return None;
        }
        let side = u32::from(assets::BLOCK_ITEM_FACE_SIDE);
        ItemModelCost::new(
            side,
            assets::BlockFace::ALL.len(),
            (side * side * 4) as usize * assets::BlockFace::ALL.len(),
            36,
        )
    }

    /// The admitted six-face sheet for this pack item variant, falling back to variant zero.
    pub fn session_block_sheet(&self, identifier: &str, metadata: u32) -> Option<IconRef> {
        self.session_icons
            .sheets
            .get(identifier)
            .and_then(|variants| variants.get(&metadata).or_else(|| variants.get(&0)))
            .copied()
    }

    /// Copies an admitted block sheet into six untinted faces for world-space item rendering.
    pub fn block_sheet_cube(&self, sheet: IconRef) -> Option<render_model::DroppedItemCube> {
        let pixels = crop_icon(self, sheet, assets::MAX_TILE_SIZE)?;
        carried_cube_pixels(pixels.width, pixels.height, &pixels.rgba8)
    }

    /// Predicts the exact texture-layer cost before a dropped session model is allocated.
    pub fn session_block_texture_layers(&self, identifier: &str, metadata: u32) -> Option<usize> {
        self.session_block_sheet(identifier, metadata)?;
        Some(assets::BlockFace::ALL.len())
    }

    /// Builds a session block's authored dropped model, retaining cube sheets for plain cubes.
    pub fn session_block_model(
        &self,
        identifier: &str,
        metadata: u32,
        sheet: IconRef,
    ) -> Option<render_model::DroppedItemModel> {
        if let Some((_, vertices)) = self.session_icons.held_block(identifier, metadata) {
            let pixels = crop_icon(self, sheet, assets::MAX_TILE_SIZE)?;
            let sprite = render_model::DroppedItemSprite {
                width: pixels.width,
                height: pixels.height,
                rgba8: pixels.rgba8.into(),
            };
            return render_model::equipment::blocks::dropped_model(sprite, &vertices)
                .map(render_model::DroppedItemModel::Block);
        }
        self.block_sheet_cube(sheet)
            .map(render_model::DroppedItemModel::Cube)
    }

    /// Native carried block faces, not the inventory's projected cube thumbnail or tinted
    /// world materials. Both catalogs must have been built from the same pinned pack.
    pub fn carried_block_cube(
        &self,
        visual: u32,
        source_manifest_sha256: [u8; 32],
    ) -> Option<render_model::DroppedItemCube> {
        carried_cube(self.carried_block_sheet(visual, source_manifest_sha256)?)
    }

    /// Borrows a startup face sheet only when its catalog matches the terrain source.
    fn carried_block_sheet(
        &self,
        visual: u32,
        source_manifest_sha256: [u8; 32],
    ) -> Option<&assets::IconSprite> {
        let catalog = self.icon_catalog.as_deref()?;
        if catalog.source_manifest_sha256() != source_manifest_sha256 {
            return None;
        }
        let index = catalog
            .block_sheets()
            .binary_search_by_key(&visual, |sheet| sheet.visual.0)
            .ok()?;
        catalog
            .sprites()
            .get(catalog.block_sheets()[index].sprite as usize)
    }
}

/// Decodes the canonical face sheet stored in the startup icon catalog.
fn carried_cube(sprite: &assets::IconSprite) -> Option<render_model::DroppedItemCube> {
    carried_cube_pixels(
        u32::from(sprite.width),
        u32::from(sprite.height),
        &sprite.rgba8,
    )
}

/// Splits a canonical carried sheet into six faces without projecting or tinting its pixels.
fn carried_cube_pixels(
    width: u32,
    height: u32,
    rgba8: &[u8],
) -> Option<render_model::DroppedItemCube> {
    use assets::{BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_SHEET_GRID, BLOCK_ITEM_SHEET_SIZE};
    if [width, height] != BLOCK_ITEM_SHEET_SIZE.map(u32::from)
        || rgba8.len() != width as usize * height as usize * 4
    {
        return None;
    }
    let side = usize::from(BLOCK_ITEM_FACE_SIDE);
    let columns = usize::from(BLOCK_ITEM_SHEET_GRID[0]);
    let faces = std::array::from_fn(|face| {
        let (x, y) = (face % columns * side, face / columns * side);
        let mut pixels = Vec::with_capacity(side * side * 4);
        for row in 0..side {
            let start = ((y + row) * width as usize + x) * 4;
            pixels.extend_from_slice(&rgba8[start..start + side * 4]);
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
    let source = IconPixels::new(runtime, icon, max_side)?;
    let mut rgba8 = Vec::with_capacity((source.width * source.height * 4) as usize);
    for y in 0..source.height {
        for x in 0..source.width {
            rgba8.extend_from_slice(source.pixel(x, y));
        }
    }
    Some(ItemSpritePixels {
        width: source.width,
        height: source.height,
        rgba8,
    })
}

/// Borrows the exact atlas samples used for reduced icon construction and cost measurement.
struct IconPixels<'a> {
    width: u32,
    height: u32,
    factor: u32,
    origin: [u32; 2],
    stride: u32,
    pixels: &'a [u8],
}

impl<'a> IconPixels<'a> {
    /// Validates the rectangle and reduction once before either reader visits its pixels.
    fn new(runtime: &'a UiPresentationRuntime, icon: IconRef, max_side: u32) -> Option<Self> {
        let width = u32::from(icon.uv[2].checked_sub(icon.uv[0])?);
        let height = u32::from(icon.uv[3].checked_sub(icon.uv[1])?);
        let page = runtime.textures.pages().get(usize::from(icon.page))?;
        let [stride, rows] = page.dimensions();
        if u32::from(icon.uv[2]) > stride || u32::from(icon.uv[3]) > rows {
            return None;
        }
        let factor = reduction_factor(width, height, max_side)?;
        let pixels = page.pixels();
        if pixels.len()
            != (stride as usize)
                .checked_mul(rows as usize)?
                .checked_mul(4)?
        {
            return None;
        }
        Some(Self {
            width: width / factor,
            height: height / factor,
            factor,
            origin: [u32::from(icon.uv[0]), u32::from(icon.uv[1])],
            stride,
            pixels,
        })
    }

    /// Samples the middle of the same reduced source block as the cropped icon.
    fn pixel(&self, x: u32, y: u32) -> &[u8] {
        let x = self.origin[0] + x * self.factor + self.factor / 2;
        let y = self.origin[1] + y * self.factor + self.factor / 2;
        let offset = ((y * self.stride + x) * 4) as usize;
        &self.pixels[offset..offset + 4]
    }
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
