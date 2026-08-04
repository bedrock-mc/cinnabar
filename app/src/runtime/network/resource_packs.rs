use std::{collections::BTreeMap, sync::Arc};

use assets::{
    RuntimeTextureCatalog, ServerResourcePackCatalog, ServerResourcePackError, TextureRef,
};
use bevy::log::warn;
use render::ChunkTextureAssets;

use crate::{
    runtime::{shutdown::record_fatal_error, world::ClientWorld},
    ui_runtime::UiRuntime,
    ui_runtime::presentation::UiPresentationRuntime,
};

pub(super) fn apply_server_resource_packs(
    client_world: &mut ClientWorld,
    ui_runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    chunk_textures: &mut ChunkTextureAssets,
    resource_packs: &protocol::ResourcePackBundle,
    session_generation: u64,
) {
    let base_runtime_assets = Arc::clone(&client_world.base_runtime_assets);
    client_world.runtime_assets = Arc::clone(&base_runtime_assets);
    *chunk_textures =
        ChunkTextureAssets::with_revision(Arc::clone(&base_runtime_assets), session_generation);
    match presentation.install_resource_packs(resource_packs) {
        Ok(catalog) => {
            if catalog.ignored_encrypted_packs() != 0 {
                let message = format!(
                    "{} server resource pack(s) could not be decrypted",
                    catalog.ignored_encrypted_packs()
                );
                if resource_packs.must_accept() {
                    record_fatal_error(&mut client_world.fatal_error, message);
                } else {
                    warn!("{message}; retaining the base asset fallback");
                }
            }
            ui_runtime.install_translations(catalog.translations());
            match server_texture_overrides(client_world.texture_catalog.as_deref(), &catalog) {
                Ok(overrides) => match base_runtime_assets.with_texture_overrides(&overrides) {
                    Ok(runtime_assets) => {
                        let runtime_assets = Arc::new(runtime_assets);
                        client_world.runtime_assets = Arc::clone(&runtime_assets);
                        *chunk_textures =
                            ChunkTextureAssets::with_revision(runtime_assets, session_generation);
                    }
                    Err(error) => {
                        report_optional_or_required(
                            client_world,
                            resource_packs,
                            format!("failed to apply server block texture overrides: {error}"),
                        );
                    }
                },
                Err(error) => report_optional_or_required(
                    client_world,
                    resource_packs,
                    format!("failed to decode server block texture overrides: {error}"),
                ),
            }
        }
        Err(error) => {
            let reset_error = presentation.reset_resource_packs().err();
            report_optional_or_required(
                client_world,
                resource_packs,
                format!("failed to apply server resource packs: {error}"),
            );
            if let Some(reset_error) = reset_error {
                record_fatal_error(
                    &mut client_world.fatal_error,
                    format!("failed to restore base resource-pack assets: {reset_error}"),
                );
            }
        }
    }
}

fn report_optional_or_required(
    client_world: &mut ClientWorld,
    resource_packs: &protocol::ResourcePackBundle,
    message: String,
) {
    if resource_packs.must_accept() {
        record_fatal_error(&mut client_world.fatal_error, message);
    } else {
        warn!("{message}; retaining the base texture fallback");
    }
}

pub(super) fn server_texture_overrides(
    base_catalog: Option<&RuntimeTextureCatalog>,
    server_catalog: &ServerResourcePackCatalog,
) -> Result<BTreeMap<TextureRef, Box<[u8]>>, ServerResourcePackError> {
    let mut overrides = BTreeMap::new();
    let Some(base_catalog) = base_catalog else {
        return Ok(overrides);
    };
    for route in base_catalog.routes() {
        let path = server_catalog
            .block_texture_path(route.key(), route.variant())
            .unwrap_or(route.path());
        let Some(tile) = server_catalog.block_texture_tile(path)? else {
            continue;
        };
        for reference in route.references() {
            overrides.insert(*reference, tile.clone());
        }
    }
    Ok(overrides)
}
