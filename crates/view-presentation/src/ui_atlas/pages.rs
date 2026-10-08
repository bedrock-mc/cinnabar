//! Uploads the JSON-UI carrier's atlas pages beside the static UI pages. Each
//! page is padded to the largest page's size so all of them share one texture
//! bucket; pixel UVs stay valid because padding only extends right and down.

use assets::RuntimeUiAssets;
use render_model::{UiRenderTextureArray, UiTexturePage};
use sha2::{Digest, Sha256};

use crate::UiAtlasError;

/// The texture array with the carrier pages inserted before the dynamic pages,
/// plus the texture page index of carrier page 0.
pub fn with_ui_pages(
    textures: &UiRenderTextureArray,
    assets: &RuntimeUiAssets,
) -> Result<(UiRenderTextureArray, u16), UiAtlasError> {
    let atlas = assets.atlas_pages();
    let side = atlas.iter().fold([1u32, 1u32], |acc, page| {
        [acc[0].max(page.width), acc[1].max(page.height)]
    });
    let row_bytes = side[0] as usize * 4;
    let mut ui_pages = Vec::with_capacity(atlas.len());
    for page in atlas {
        let mut pixels = vec![0u8; row_bytes * side[1] as usize];
        let source_row = page.width as usize * 4;
        for (row, source) in page.rgba8.chunks_exact(source_row).enumerate() {
            let start = row * row_bytes;
            pixels[start..start + source_row].copy_from_slice(source);
        }
        ui_pages.push(
            UiTexturePage::owned(side, pixels.into())
                .map_err(|_| UiAtlasError::InvalidFontTexture)?,
        );
    }
    let dynamic_start = textures.dynamic_start();
    let first = u16::try_from(dynamic_start).map_err(|_| UiAtlasError::InvalidFontTexture)?;
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
    .map_err(|_| UiAtlasError::InvalidFontTexture)?;
    Ok((textures, first))
}
