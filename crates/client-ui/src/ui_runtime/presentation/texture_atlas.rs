//! Bounded UI texture-array construction: font pages, the solid page, the
//! packed HUD atlas layer, and the packed item-icon layers. Split from the
//! presentation root to honor the production line budget.

use assets::{HudTextureRole, RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog};
use render_model::{
    MAX_UI_FIXED_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, UiRenderTextureArray, UiTextureFormat,
    UiTexturePage, UiTexturePlan,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::UiPresentationError;

use render_model::UI_DYNAMIC_PAGE_SIDE as VANILLA_HUD_ATLAS_SIDE;
const HUD_ATLAS_GUTTER: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct HudSprite {
    pub(super) uv: [u16; 4],
    pub(super) size: [u16; 2],
}

pub use ui::IconRef;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HudTexturePages {
    pub(super) page: u16,
    pub(super) sprites: [HudSprite; HudTextureRole::ALL.len()],
}

impl HudTexturePages {
    pub(super) fn sprite(&self, role: HudTextureRole) -> HudSprite {
        self.sprites[role as usize]
    }
}

/// One planned sprite placement in the icon layers.
struct IconPlacement {
    layer_offset: u32,
    cursor: [u32; 2],
    padded: [u32; 2],
}

/// Plans gutter-padded row placements for every icon sprite across as many
/// layers as needed, returning the placements and the layer count.
fn plan_icon_placements(
    icons: &RuntimeIconCatalog,
    width: u32,
    height: u32,
) -> Result<(Vec<IconPlacement>, u32), UiPresentationError> {
    let gutter_span = HUD_ATLAS_GUTTER * 2;
    let mut placements = Vec::with_capacity(icons.sprites().len());
    let mut layer_offset = 0u32;
    let mut cursor = [0u32, 0u32];
    let mut row_height = 0u32;
    for sprite in icons.sprites() {
        let padded = [
            u32::from(sprite.width) + gutter_span,
            u32::from(sprite.height) + gutter_span,
        ];
        if padded[0] > width || padded[1] > height {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        if cursor[0]
            .checked_add(padded[0])
            .is_none_or(|right| right > width)
        {
            cursor = [
                0,
                cursor[1]
                    .checked_add(row_height)
                    .ok_or(UiPresentationError::InvalidFontTexture)?,
            ];
            row_height = 0;
        }
        if cursor[1]
            .checked_add(padded[1])
            .is_none_or(|bottom| bottom > height)
        {
            layer_offset = layer_offset
                .checked_add(1)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            cursor = [0, 0];
            row_height = 0;
        }
        placements.push(IconPlacement {
            layer_offset,
            cursor,
            padded,
        });
        cursor[0] = cursor[0]
            .checked_add(padded[0])
            .ok_or(UiPresentationError::InvalidFontTexture)?;
        row_height = row_height.max(padded[1]);
    }
    let layer_count = layer_offset
        .checked_add(u32::from(!placements.is_empty()))
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    Ok((placements, layer_count))
}

pub(super) fn font_texture_array_with_optional_hud(
    font: &Arc<RuntimeFontCatalog>,
    hud: Option<&RuntimeHudCatalog>,
) -> Result<(UiRenderTextureArray, u16, Option<HudTexturePages>), UiPresentationError> {
    let (textures, solid_texture_page, hud_textures, _) =
        font_texture_array_with_hud_and_icons(font, hud, None)?;
    Ok((textures, solid_texture_page, hud_textures))
}

type TextureArrayWithIcons = (
    UiRenderTextureArray,
    u16,
    Option<HudTexturePages>,
    Option<Box<[IconRef]>>,
);

pub(super) fn font_texture_array_with_hud_and_icons(
    font: &Arc<RuntimeFontCatalog>,
    hud: Option<&RuntimeHudCatalog>,
    icons: Option<&RuntimeIconCatalog>,
) -> Result<TextureArrayWithIcons, UiPresentationError> {
    let width = VANILLA_HUD_ATLAS_SIDE;
    let height = VANILLA_HUD_ATLAS_SIDE;
    let font_layers =
        u32::try_from(font.pages().len()).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    if font_layers >= MAX_UI_TEXTURE_LAYERS {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    let solid_texture_page =
        u16::try_from(font_layers).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    let hud_layers = u32::from(hud.is_some());
    let icon_placements = icons
        .map(|icons| plan_icon_placements(icons, width, height))
        .transpose()?;
    let icon_layers = icon_placements.as_ref().map_or(0, |(_, layers)| *layers);
    // Dry-run every HUD placement before allocating even the solid page.
    if let Some(hud) = hud {
        let mut cursor = [0u32, 0u32];
        let mut row_height = 0;
        for texture in hud.textures() {
            let padded_width = texture
                .width
                .checked_add(2)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_height = texture
                .height
                .checked_add(2)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            if cursor[0]
                .checked_add(padded_width)
                .is_none_or(|right| right > width)
            {
                cursor[0] = 0;
                cursor[1] = cursor[1]
                    .checked_add(row_height)
                    .ok_or(UiPresentationError::InvalidFontTexture)?;
                row_height = 0;
            }
            if padded_width > width
                || cursor[1]
                    .checked_add(padded_height)
                    .is_none_or(|bottom| bottom > height)
            {
                return Err(UiPresentationError::InvalidFontTexture);
            }
            cursor[0] += padded_width;
            row_height = row_height.max(padded_height);
        }
    }
    let layers = font_layers
        .checked_add(1)
        .and_then(|layers| layers.checked_add(hud_layers))
        .and_then(|layers| layers.checked_add(icon_layers))
        .filter(|layers| *layers <= MAX_UI_TEXTURE_LAYERS)
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let layer_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let font_pages = (0..font.pages().len())
        .map(|index| {
            UiTexturePage::font(Arc::clone(font), index)
                .map_err(|_| UiPresentationError::InvalidFontTexture)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut planned = font_pages
        .iter()
        .map(|page| (page.dimensions(), page.format()))
        .collect::<Vec<_>>();
    planned.extend(std::iter::repeat_n(
        ([width, height], UiTextureFormat::Rgba8),
        (layers - font_layers) as usize,
    ));
    planned.extend((0..render_model::MAX_UI_DYNAMIC_PAGES).map(|offset| {
        if offset >= render_model::UI_FALLBACK_FONT_PAGE_OFFSET {
            return (
                [render_model::UI_FALLBACK_FONT_PAGE_SIDE; 2],
                UiTextureFormat::Coverage,
            );
        }
        let side = if offset == render_model::UI_LOCAL_FONT_PAGE_OFFSET {
            render_model::UI_LOCAL_FONT_PAGE_SIDE
        } else {
            width
        };
        ([side; 2], UiTextureFormat::Rgba8)
    }));
    planned.extend(std::iter::repeat_n(
        ([render_model::UI_ART_PAGE_SIDE; 2], UiTextureFormat::Rgba8),
        render_model::MAX_UI_ART_PAGES,
    ));
    let plan = UiTexturePlan::with_formats(&planned)
        .map_err(|_| UiPresentationError::InvalidFontTexture)?;
    if plan.bytes() > MAX_UI_FIXED_TEXTURE_BYTES {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    let total_bytes = layer_bytes * (layers - font_layers) as usize;
    let mut rgba8 = vec![0; total_bytes];
    let solid_start = 0;
    rgba8[solid_start..solid_start + layer_bytes].fill(255);
    let hud_textures = if let Some(hud) = hud {
        let page = solid_texture_page
            .checked_add(1)
            .ok_or(UiPresentationError::InvalidFontTexture)?;
        let layer_start = (usize::from(page) - font_layers as usize) * layer_bytes;
        let mut cursor = [0u32, 0u32];
        let mut row_height = 0u32;
        let mut sprites = [HudSprite::default(); HudTextureRole::ALL.len()];
        for texture in hud.textures() {
            let gutter_span = HUD_ATLAS_GUTTER
                .checked_mul(2)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_width = texture
                .width
                .checked_add(gutter_span)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_height = texture
                .height
                .checked_add(gutter_span)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let row_right = cursor[0]
                .checked_add(padded_width)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            if row_right > width {
                cursor[0] = 0;
                cursor[1] = cursor[1]
                    .checked_add(row_height)
                    .ok_or(UiPresentationError::InvalidFontTexture)?;
                row_height = 0;
            }
            let padded_right = cursor[0]
                .checked_add(padded_width)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_bottom = cursor[1]
                .checked_add(padded_height)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            if padded_right > width || padded_bottom > height {
                return Err(UiPresentationError::InvalidFontTexture);
            }
            let left = cursor[0]
                .checked_add(HUD_ATLAS_GUTTER)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let top = cursor[1]
                .checked_add(HUD_ATLAS_GUTTER)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let right = left
                .checked_add(texture.width)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let bottom = top
                .checked_add(texture.height)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            for padded_y in 0..padded_height {
                let source_y = padded_y
                    .saturating_sub(HUD_ATLAS_GUTTER)
                    .min(texture.height - 1);
                for padded_x in 0..padded_width {
                    let source_x = padded_x
                        .saturating_sub(HUD_ATLAS_GUTTER)
                        .min(texture.width - 1);
                    let source_start =
                        (source_y as usize * texture.width as usize + source_x as usize) * 4;
                    let target_start = layer_start
                        + ((cursor[1] + padded_y) as usize * width as usize
                            + (cursor[0] + padded_x) as usize)
                            * 4;
                    rgba8[target_start..target_start + 4]
                        .copy_from_slice(&texture.rgba8[source_start..source_start + 4]);
                }
            }
            sprites[texture.role as usize] = HudSprite {
                uv: [
                    u16::try_from(left).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(top).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(right).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(bottom).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                ],
                size: [
                    u16::try_from(texture.width)
                        .map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(texture.height)
                        .map_err(|_| UiPresentationError::InvalidFontTexture)?,
                ],
            };
            cursor[0] = padded_right;
            row_height = row_height.max(padded_height);
        }
        Some(HudTexturePages { page, sprites })
    } else {
        None
    };

    // Item icons fill their planned placements in the layers after the HUD
    // layer, with the same replicated 1px gutter so sampling never bleeds.
    let icon_refs = if let (Some(icons), Some((placements, _))) = (icons, &icon_placements) {
        let first_icon_layer = font_layers + 1 + hud_layers;
        let mut refs = Vec::with_capacity(placements.len());
        for (sprite, placement) in icons.sprites().iter().zip(placements.iter()) {
            let layer = first_icon_layer + placement.layer_offset;
            let layer_start = (layer - font_layers) as usize * layer_bytes;
            let sprite_width = u32::from(sprite.width);
            let sprite_height = u32::from(sprite.height);
            for padded_y in 0..placement.padded[1] {
                let source_y = padded_y
                    .saturating_sub(HUD_ATLAS_GUTTER)
                    .min(sprite_height - 1);
                for padded_x in 0..placement.padded[0] {
                    let source_x = padded_x
                        .saturating_sub(HUD_ATLAS_GUTTER)
                        .min(sprite_width - 1);
                    let source_start =
                        (source_y as usize * sprite_width as usize + source_x as usize) * 4;
                    let target_start = layer_start
                        + ((placement.cursor[1] + padded_y) as usize * width as usize
                            + (placement.cursor[0] + padded_x) as usize)
                            * 4;
                    rgba8[target_start..target_start + 4]
                        .copy_from_slice(&sprite.rgba8[source_start..source_start + 4]);
                }
            }
            let left = placement.cursor[0] + HUD_ATLAS_GUTTER;
            let top = placement.cursor[1] + HUD_ATLAS_GUTTER;
            refs.push(IconRef {
                page: u16::try_from(layer).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                uv: [
                    u16::try_from(left).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(top).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(left + sprite_width)
                        .map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(top + sprite_height)
                        .map_err(|_| UiPresentationError::InvalidFontTexture)?,
                ],
                glint: false,
            });
        }
        Some(refs.into_boxed_slice())
    } else {
        None
    };

    let mut pages = font_pages;
    for pixels in rgba8.chunks_exact(layer_bytes) {
        pages.push(
            UiTexturePage::owned([width, height], Arc::from(pixels))
                .map_err(|_| UiPresentationError::InvalidFontTexture)?,
        );
    }
    let dynamic_start = pages.len();
    let blank = UiTexturePage::owned([width, height], vec![0; layer_bytes].into())
        .map_err(|_| UiPresentationError::InvalidFontTexture)?;
    pages.extend((0..render_model::MAX_UI_DYNAMIC_PAGES).map(|offset| {
        if offset >= render_model::UI_FALLBACK_FONT_PAGE_OFFSET {
            return super::font_fallback::blank_page();
        }
        if offset == render_model::UI_LOCAL_FONT_PAGE_OFFSET {
            super::mod_panel_font::blank_page()
        } else {
            blank.clone()
        }
    }));
    let art_side = render_model::UI_ART_PAGE_SIDE;
    let blank_art = UiTexturePage::owned(
        [art_side, art_side],
        vec![0; art_side as usize * art_side as usize * 4].into(),
    )
    .map_err(|_| UiPresentationError::InvalidFontTexture)?;
    pages.extend(std::iter::repeat_n(
        blank_art,
        render_model::MAX_UI_ART_PAGES,
    ));
    let mut source = Sha256::new();
    source.update(b"ui-source-catalog-v1");
    source.update(font.identity().carrier_sha256);
    source.update([u8::from(hud.is_some()), u8::from(icons.is_some())]);
    if let Some(hud) = hud {
        source.update(hud.source_manifest_sha256());
    }
    if let Some(icons) = icons {
        source.update(icons.source_manifest_sha256());
    }
    let textures =
        UiRenderTextureArray::with_source_identity(pages, dynamic_start, source.finalize().into())
            .map_err(|_| UiPresentationError::InvalidFontTexture)?;
    Ok((textures, solid_texture_page, hud_textures, icon_refs))
}

pub(super) fn font_texture_array(
    font: &Arc<RuntimeFontCatalog>,
) -> Result<(UiRenderTextureArray, u16), UiPresentationError> {
    let (textures, solid, _, _) = font_texture_array_with_hud_and_icons(font, None, None)?;
    Ok((textures, solid))
}
