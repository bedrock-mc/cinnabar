use std::sync::Arc;

use assets::{RuntimeFontCatalog, ServerResourcePackCatalog};

use super::{UiPresentationError, UiPresentationRuntime, font_texture_array_with_optional_hud};

pub(super) fn install_resource_packs(
    runtime: &mut UiPresentationRuntime,
    bundle: &protocol::ResourcePackBundle,
) -> Result<ServerResourcePackCatalog, UiPresentationError> {
    let inputs = bundle
        .packs()
        .iter()
        .map(|pack| assets::ServerResourcePackInput {
            uuid: &pack.uuid,
            version: &pack.version,
            name: &pack.name,
            content_key: &pack.content_key,
            bytes: &pack.bytes,
        })
        .collect::<Vec<_>>();
    let catalog = assets::compile_server_resource_packs(&runtime.base_font, &inputs)
        .map_err(UiPresentationError::ResourcePack)?;
    let font = catalog
        .font_overlay()
        .cloned()
        .map(Arc::new)
        .unwrap_or_else(|| Arc::clone(&runtime.base_font));
    install_font(runtime, font, Some(&catalog))?;
    Ok(catalog)
}

pub(super) fn reset_resource_packs(
    runtime: &mut UiPresentationRuntime,
) -> Result<(), UiPresentationError> {
    install_font(runtime, Arc::clone(&runtime.base_font), None)
}

fn install_font(
    runtime: &mut UiPresentationRuntime,
    font: Arc<RuntimeFontCatalog>,
    catalog: Option<&ServerResourcePackCatalog>,
) -> Result<(), UiPresentationError> {
    let (textures, solid_texture_page, hud_textures, item_textures) =
        font_texture_array_with_optional_hud(&font, runtime.hud_catalog.as_deref(), catalog)?;
    runtime.font = font;
    runtime.textures = Arc::new(textures);
    runtime.solid_texture_page = solid_texture_page;
    runtime.hud_textures = hud_textures;
    runtime.item_textures = item_textures;
    runtime.layouts =
        super::TextLayoutCache::new(super::TEXT_CACHE_ENTRIES, super::TEXT_CACHE_BYTES);
    runtime.revision = runtime.revision.saturating_add(1);
    Ok(())
}
