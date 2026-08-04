use std::collections::BTreeMap;

use assets::ServerResourcePackCatalog;

use super::HudSprite;

pub(super) const ITEM_ATLAS_SIDE: u32 = 1_024;
const ITEM_ATLAS_GUTTER: u32 = 1;
const MAX_PRESENTED_ITEM_ICONS: usize = 4_096;
type ItemSpriteMap = BTreeMap<(Box<str>, u32), HudSprite>;
type PackedItemIcons = (Vec<u8>, ItemSpriteMap);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ItemTexturePages {
    pub(super) page: u16,
    pub(super) sprites: BTreeMap<(Box<str>, u32), HudSprite>,
}

impl ItemTexturePages {
    pub(super) fn sprite(&self, identifier: &str, metadata: u32) -> Option<HudSprite> {
        let key = (identifier.into(), metadata);
        self.sprites
            .get(&key)
            .copied()
            .or_else(|| self.sprites.get(&(identifier.into(), 0)).copied())
    }
}

pub(super) fn pack_item_icons(catalog: &ServerResourcePackCatalog) -> Option<PackedItemIcons> {
    if catalog.item_icons().is_empty() {
        return None;
    }
    let side = usize::try_from(ITEM_ATLAS_SIDE).ok()?;
    let pixels = side.checked_mul(side)?.checked_mul(4)?;
    let mut rgba8 = vec![0; pixels];
    let mut sprites = BTreeMap::new();
    let mut cursor = [0u32, 0u32];
    let mut row_height = 0u32;
    for icon in catalog.item_icons().iter().take(MAX_PRESENTED_ITEM_ICONS) {
        let width = u32::from(icon.width());
        let height = u32::from(icon.height());
        let gutter_span = ITEM_ATLAS_GUTTER.checked_mul(2)?;
        let padded_width = width.checked_add(gutter_span)?;
        let padded_height = height.checked_add(gutter_span)?;
        if padded_width > ITEM_ATLAS_SIDE || padded_height > ITEM_ATLAS_SIDE {
            continue;
        }
        if cursor[0].checked_add(padded_width)? > ITEM_ATLAS_SIDE {
            cursor[0] = 0;
            cursor[1] = cursor[1].checked_add(row_height)?;
            row_height = 0;
        }
        let padded_right = cursor[0].checked_add(padded_width)?;
        let padded_bottom = cursor[1].checked_add(padded_height)?;
        if padded_right > ITEM_ATLAS_SIDE || padded_bottom > ITEM_ATLAS_SIDE {
            break;
        }
        let left = cursor[0].checked_add(ITEM_ATLAS_GUTTER)?;
        let top = cursor[1].checked_add(ITEM_ATLAS_GUTTER)?;
        let right = left.checked_add(width)?;
        let bottom = top.checked_add(height)?;
        let source_width = usize::from(icon.width());
        let source_height = usize::from(icon.height());
        if icon.rgba8().len() != source_width.checked_mul(source_height)?.checked_mul(4)? {
            continue;
        }
        for padded_y in 0..padded_height {
            let source_y = padded_y.saturating_sub(ITEM_ATLAS_GUTTER).min(height - 1);
            for padded_x in 0..padded_width {
                let source_x = padded_x.saturating_sub(ITEM_ATLAS_GUTTER).min(width - 1);
                let source_start = (source_y as usize * source_width + source_x as usize) * 4;
                let target_start =
                    ((cursor[1] + padded_y) as usize * side + (cursor[0] + padded_x) as usize) * 4;
                rgba8[target_start..target_start + 4]
                    .copy_from_slice(&icon.rgba8()[source_start..source_start + 4]);
            }
        }
        let Ok(uv) = [left, top, right, bottom]
            .into_iter()
            .map(u16::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map(|values| [values[0], values[1], values[2], values[3]])
        else {
            continue;
        };
        sprites.insert(
            (icon.identifier().into(), icon.metadata()),
            HudSprite {
                uv,
                size: [icon.width(), icon.height()],
            },
        );
        cursor[0] = padded_right;
        row_height = row_height.max(padded_height);
    }
    (!sprites.is_empty()).then_some((rgba8, sprites))
}
