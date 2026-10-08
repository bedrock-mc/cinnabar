//! Item sprites packed into shared atlas layers so equipment costs few texture pages.

use assets::IconSprite;
use render_model::equipment::EquipmentRaster;

pub const ATLAS_SIDE: u16 = 512;

/// Where one sprite sits in the atlas: layer index and pixel rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    pub layer: usize,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Placement {
    /// The sprite's `[u0, v0, u1, v1]` region of its atlas layer.
    pub fn uv_rect(self) -> [f32; 4] {
        let side = f32::from(ATLAS_SIDE);
        [
            f32::from(self.x) / side,
            f32::from(self.y) / side,
            f32::from(self.x + self.width) / side,
            f32::from(self.y + self.height) / side,
        ]
    }
}

pub struct SpriteAtlas {
    pub layers: Vec<EquipmentRaster>,
    /// Indexed like the icon catalog's sprites; `None` for one that could not be placed.
    pub placements: Vec<Option<Placement>>,
}

struct Layer {
    rgba8: Vec<u8>,
    shelf_y: u16,
    shelf_height: u16,
    cursor_x: u16,
}

impl Layer {
    fn new() -> Self {
        Self {
            rgba8: vec![0; usize::from(ATLAS_SIDE) * usize::from(ATLAS_SIDE) * 4],
            shelf_y: 0,
            shelf_height: 0,
            cursor_x: 0,
        }
    }

    /// Reserves a `width` x `height` slot on the current or next shelf, if it fits.
    fn reserve(&mut self, width: u16, height: u16) -> Option<(u16, u16)> {
        if self.cursor_x + width > ATLAS_SIDE {
            self.shelf_y += self.shelf_height;
            self.shelf_height = 0;
            self.cursor_x = 0;
        }
        if self.shelf_y + height > ATLAS_SIDE {
            return None;
        }
        let slot = (self.cursor_x, self.shelf_y);
        self.cursor_x += width;
        self.shelf_height = self.shelf_height.max(height);
        Some(slot)
    }
}

impl SpriteAtlas {
    /// Shelf-packs every valid sprite, tallest first.
    pub fn pack(sprites: &[IconSprite]) -> Self {
        let valid = |sprite: &IconSprite| {
            sprite.width != 0
                && sprite.height != 0
                && sprite.width <= ATLAS_SIDE
                && sprite.height <= ATLAS_SIDE
                && sprite.rgba8.len() == usize::from(sprite.width) * usize::from(sprite.height) * 4
        };
        let mut order = (0..sprites.len())
            .filter(|index| valid(&sprites[*index]))
            .collect::<Vec<_>>();
        order.sort_by_key(|index| {
            let sprite = &sprites[*index];
            (
                std::cmp::Reverse(sprite.height),
                std::cmp::Reverse(sprite.width),
                *index,
            )
        });
        let mut layers = vec![Layer::new()];
        let mut placements = vec![None; sprites.len()];
        for index in order {
            let sprite = &sprites[index];
            let mut slot = layers
                .last_mut()
                .and_then(|layer| layer.reserve(sprite.width, sprite.height));
            if slot.is_none() {
                layers.push(Layer::new());
                slot = layers
                    .last_mut()
                    .and_then(|layer| layer.reserve(sprite.width, sprite.height));
            }
            let Some((x, y)) = slot else {
                continue;
            };
            let layer_index = layers.len() - 1;
            let layer = &mut layers[layer_index];
            let row_bytes = usize::from(sprite.width) * 4;
            for row in 0..usize::from(sprite.height) {
                let target =
                    ((usize::from(y) + row) * usize::from(ATLAS_SIDE) + usize::from(x)) * 4;
                layer.rgba8[target..target + row_bytes]
                    .copy_from_slice(&sprite.rgba8[row * row_bytes..(row + 1) * row_bytes]);
            }
            placements[index] = Some(Placement {
                layer: layer_index,
                x,
                y,
                width: sprite.width,
                height: sprite.height,
            });
        }
        if layers
            .last()
            .is_some_and(|layer| layer.shelf_height == 0 && layer.cursor_x == 0)
        {
            layers.pop();
        }
        Self {
            layers: layers
                .into_iter()
                .map(|layer| EquipmentRaster {
                    width: ATLAS_SIDE,
                    height: ATLAS_SIDE,
                    rgba8: layer.rgba8.into(),
                })
                .collect(),
            placements,
        }
    }
}
