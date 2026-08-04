use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::RuntimeTextureCatalog;

use super::{AssetStartupError, TEXTURE_ASSETS_COMPILE_COMMAND, format_sha256, texture_asset_path};

const MAX_TEXTURE_ASSET_BLOB_BYTES: u64 = 16 * 1024 * 1024;

pub struct LoadedTextureAssets {
    runtime: Option<Arc<RuntimeTextureCatalog>>,
    selected_path: PathBuf,
}

impl LoadedTextureAssets {
    #[must_use]
    pub fn selected_path(&self) -> &Path {
        &self.selected_path
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        match &self.runtime {
            Some(runtime) => {
                let identity = runtime.identity();
                format!(
                    "loaded optional immutable texture route assets from {} (routes={} carrier_sha256={})",
                    self.selected_path.display(),
                    runtime.routes().len(),
                    format_sha256(identity.carrier_sha256)
                )
            }
            None => format!(
                "optional immutable texture route assets were not found at {}; using base texture routes",
                self.selected_path.display()
            ),
        }
    }

    pub fn into_runtime(self) -> Option<Arc<RuntimeTextureCatalog>> {
        self.runtime
    }
}

pub fn load_texture_assets(
    world_asset_path: &Path,
    expected_source_manifest_sha256: [u8; 32],
) -> Result<LoadedTextureAssets, AssetStartupError> {
    let path = texture_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedTextureAssets {
                runtime: None,
                selected_path: path,
            });
        }
        Err(source) => {
            return Err(AssetStartupError::TextureAssetsRead {
                path,
                source,
                rebuild_command: TEXTURE_ASSETS_COMPILE_COMMAND,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::TextureAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: TEXTURE_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_TEXTURE_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::TextureAssetsTooLarge {
            path,
            max_bytes: MAX_TEXTURE_ASSET_BLOB_BYTES,
            rebuild_command: TEXTURE_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_TEXTURE_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::TextureAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: TEXTURE_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_TEXTURE_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::TextureAssetsTooLarge {
            path,
            max_bytes: MAX_TEXTURE_ASSET_BLOB_BYTES,
            rebuild_command: TEXTURE_ASSETS_COMPILE_COMMAND,
        });
    }
    let runtime = RuntimeTextureCatalog::decode(&bytes, expected_source_manifest_sha256).map_err(
        |source| AssetStartupError::TextureAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: TEXTURE_ASSETS_COMPILE_COMMAND,
        },
    )?;
    Ok(LoadedTextureAssets {
        runtime: Some(Arc::new(runtime)),
        selected_path: path,
    })
}
