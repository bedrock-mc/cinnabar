//! The UI texture pages: the JSON-UI carrier's atlas pages beside the static UI pages, and the
//! layout of the bounded dynamic pages after them. Each carrier page is padded to the largest
//! page's size so all of them share one texture bucket; pixel UVs stay valid because padding
//! only extends right and down.

use std::sync::Arc;

use assets::RuntimeUiAssets;
use render_model::{UiRenderTextureArray, UiTexturePage};
use sha2::{Digest, Sha256};

use crate::engine::screen::PaintError;

/// Dynamic page offset holding the session's server item icons.
pub const SESSION_ICON_PAGE: usize = render_model::UI_SESSION_ICON_PAGE_OFFSET;
/// Dynamic pages after the general ten, holding the session's glyph-sheet atlas.
pub const GLYPH_PAGES: usize = 8;
pub const FIRST_GLYPH_PAGE: usize = SESSION_ICON_PAGE + 1;
/// Dynamic page offset of the server resource-pack UI textures, after the glyphs.
pub const SERVER_UI_PAGE: usize = FIRST_GLYPH_PAGE + GLYPH_PAGES;
/// Dynamic pages reserved for server resource-pack UI textures.
pub const SERVER_UI_PAGES: usize =
    render_model::UI_LOCAL_FONT_PAGE_OFFSET - SERVER_UI_PAGE - MODAL_UI_PAGES - MOD_UI_PAGES;
/// Dynamic page offset of a client part's modal screen textures, after the server pack's.
pub const MODAL_UI_PAGE: usize = SERVER_UI_PAGE + SERVER_UI_PAGES;
/// Dynamic pages reserved for the modal's bundle textures, apart from the server pack's.
pub const MODAL_UI_PAGES: usize = 4;
/// Dynamic page offset of a player mod's screen textures, after the modal's.
pub const MOD_UI_PAGE: usize = MODAL_UI_PAGE + MODAL_UI_PAGES;
/// Dynamic pages reserved for the player mod's package textures, before the local font page.
pub const MOD_UI_PAGES: usize = 4;

/// Empty texels kept around each image packed on an art page.
pub const ART_PAGE_GUTTER: u32 = 1;
/// The largest image side one art page holds inside its gutter.
pub const WHOLE_ART_PAGE: u32 = render_model::UI_ART_PAGE_SIDE - ART_PAGE_GUTTER * 2;

/// The texture array with the carrier pages inserted before the dynamic pages,
/// plus the texture page index of carrier page 0.
pub fn with_ui_pages(
    textures: &UiRenderTextureArray,
    assets: &RuntimeUiAssets,
) -> Result<(UiRenderTextureArray, u16), PaintError> {
    let atlas = assets.atlas_pages();
    let side = atlas.iter().fold([1u32, 1u32], |acc, page| {
        [acc[0].max(page.width), acc[1].max(page.height)]
    });
    let row_bytes = side[0] as usize * 4;
    let mut ui_pages = Vec::with_capacity(atlas.len());
    for page in atlas {
        // A full-size page shares the carrier's pixels; only a smaller one is padded into a copy.
        let pixels = if [page.width, page.height] == side {
            Arc::clone(&page.rgba8)
        } else {
            let mut pixels = vec![0u8; row_bytes * side[1] as usize];
            let source_row = page.width as usize * 4;
            for (row, source) in page.rgba8.chunks_exact(source_row).enumerate() {
                let start = row * row_bytes;
                pixels[start..start + source_row].copy_from_slice(source);
            }
            pixels.into()
        };
        ui_pages
            .push(UiTexturePage::owned(side, pixels).map_err(|_| PaintError::InvalidFontTexture)?);
    }
    let dynamic_start = textures.dynamic_start();
    let first = u16::try_from(dynamic_start).map_err(|_| PaintError::InvalidFontTexture)?;
    let mut pages = textures.pages()[..dynamic_start].to_vec();
    let added = ui_pages.len();
    pages.extend(ui_pages);
    pages.extend_from_slice(&textures.pages()[dynamic_start..]);
    let mut source = Sha256::new();
    source.update(b"ui-json-carrier-v1");
    source.update(textures.static_identity());
    source.update(assets.source_manifest_sha256());
    let textures = UiRenderTextureArray::with_source_identity(
        pages,
        dynamic_start + added,
        source.finalize().into(),
    )
    .map_err(|_| PaintError::InvalidFontTexture)?;
    Ok((textures, first))
}
