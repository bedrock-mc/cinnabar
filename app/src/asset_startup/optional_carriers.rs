//! Loaders for the optional runtime carriers that sit beside the world carrier.

use super::*;

/// Reads the texture key sidecar beside the world carrier; absence or mismatch
/// only disables vanilla block retexturing from server packs.
pub(super) fn load_material_keys(
    world_asset_path: &Path,
    material_count: usize,
) -> Option<assets::MaterialKeys> {
    let path = world_asset_path.with_extension("matkeys.json");
    let file = File::open(&path).ok()?;
    let mut bytes = Vec::new();
    file.take(assets::MAX_MATERIAL_KEYS_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > assets::MAX_MATERIAL_KEYS_BYTES {
        return None;
    }
    let keys = assets::MaterialKeys::from_json(&bytes, material_count);
    if keys.is_none() {
        bevy::log::warn!(
            "material key sidecar does not match the world carrier; rebuild with make assets"
        );
    }
    keys
}

/// Reads the vanilla entity-definition sidecar beside the entity carrier; absence only limits
/// server-pack entities that reference vanilla render controllers, animations or geometry.
pub(super) fn load_vanilla_entity_refs(
    world_asset_path: &Path,
) -> Option<assets::VanillaEntityRefs> {
    let path = entity_asset_path(world_asset_path).with_extension("vanillarefs.json");
    let file = File::open(&path).ok()?;
    let mut bytes = Vec::new();
    file.take(assets::MAX_VANILLA_REFS_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > assets::MAX_VANILLA_REFS_BYTES {
        return None;
    }
    let refs = assets::VanillaEntityRefs::from_json(&bytes);
    if refs.is_none() {
        bevy::log::warn!(
            "vanilla entity refs sidecar is malformed; rebuild with make entity-assets"
        );
    }
    refs
}

pub(super) fn load_entity_assets(
    world_asset_path: &Path,
) -> Result<LoadedEntityAssets, AssetStartupError> {
    let path = entity_asset_path(world_asset_path);
    let file = File::open(&path).map_err(|source| AssetStartupError::EntityAssetsRead {
        path: path.clone(),
        source,
        rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::EntityAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_ENTITY_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::EntityAssetsTooLarge {
            path,
            max_bytes: MAX_ENTITY_ASSET_BLOB_BYTES,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ENTITY_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::EntityAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_ENTITY_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::EntityAssetsTooLarge {
            path,
            max_bytes: MAX_ENTITY_ASSET_BLOB_BYTES,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        });
    }
    let runtime = Arc::new(RuntimeEntityAssets::decode(&bytes).map_err(|source| {
        AssetStartupError::EntityAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        }
    })?);
    let identity = runtime
        .carrier_identity()
        .expect("a decoded entity carrier knows its identity");
    let expected_manifest_sha256 = canonical_source_manifest_sha256(VANILLA_SOURCE_JSON);
    let actual_manifest_sha256 = runtime.source_manifest_sha256();
    if actual_manifest_sha256 != expected_manifest_sha256 {
        return Err(AssetStartupError::EntityAssetsProvenance {
            path,
            expected: format_sha256(expected_manifest_sha256),
            actual: format_sha256(actual_manifest_sha256),
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        });
    }
    Ok(LoadedEntityAssets {
        runtime,
        identity,
        selected_path: path,
    })
}

pub(super) fn load_font_assets(
    world_asset_path: &Path,
) -> Result<LoadedFontAssets, AssetStartupError> {
    // An explicit local carrier wins; the bundled Cinnangles Sans carrier is the default.
    let candidates = [
        (
            local_font_asset_path(world_asset_path),
            VANILLA_SOURCE_JSON,
            LOCAL_FONT_ASSETS_COMPILE_COMMAND,
        ),
        (
            font_asset_path(world_asset_path),
            FONT_SOURCE_JSON,
            FONT_ASSETS_COMPILE_COMMAND,
        ),
    ];
    let mut selected = None;
    for (path, source_manifest, rebuild_command) in candidates {
        match File::open(&path) {
            Ok(file) => {
                selected = Some((path, file, source_manifest, rebuild_command));
                break;
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AssetStartupError::FontAssetsRead {
                    path,
                    source,
                    rebuild_command,
                });
            }
        }
    }
    let Some((path, file, source_manifest, rebuild_command)) = selected else {
        return diagnostic_font_assets(font_asset_path(world_asset_path));
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::FontAssetsRead {
            path: path.clone(),
            source,
            rebuild_command,
        })?
        .len();
    if length > MAX_FONT_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::FontAssetsTooLarge {
            path,
            max_bytes: MAX_FONT_ASSET_BLOB_BYTES,
            rebuild_command,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_FONT_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::FontAssetsRead {
            path: path.clone(),
            source,
            rebuild_command,
        })?;
    if bytes.len() as u64 > MAX_FONT_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::FontAssetsTooLarge {
            path,
            max_bytes: MAX_FONT_ASSET_BLOB_BYTES,
            rebuild_command,
        });
    }
    let expected_manifest_sha256 = canonical_source_manifest_sha256(source_manifest);
    let runtime =
        RuntimeFontCatalog::decode(&bytes, expected_manifest_sha256).map_err(|source| {
            AssetStartupError::FontAssetsDecode {
                path: path.clone(),
                source: Box::new(source),
                rebuild_command,
            }
        })?;
    Ok(LoadedFontAssets {
        runtime: Arc::new(runtime.with_coverage_pages()),
        selected_path: path,
        diagnostic: false,
    })
}

/// Quotes a path for copy-paste into the platform shell running `make`.
#[cfg(windows)]
pub(crate) fn shell_quote_path(path: &Path) -> String {
    let path = make_path_value(path).replace('\\', "/");
    format!("'{}'", path.replace('\'', "''"))
}

#[cfg(not(windows))]
pub(crate) fn shell_quote_path(path: &Path) -> String {
    format!("'{}'", make_path_value(path).replace('\'', "'\"'\"'"))
}

/// Rejects paths that make or the build recipe shell could execute instead of treating literally.
fn make_path_value(path: &Path) -> std::borrow::Cow<'_, str> {
    let value = path.to_string_lossy();
    if value.contains(['$', '`', '"', '\n', '\r']) {
        "$(error Unsafe carrier path; choose a path without shell/make syntax and run make assets)"
            .into()
    } else {
        value
    }
}

pub(super) fn load_atmosphere_assets(
    world_asset_path: &Path,
) -> Result<LoadedAtmosphereAssets, AssetStartupError> {
    let path = atmosphere_asset_path(world_asset_path);
    let file = File::open(&path).map_err(|source| AssetStartupError::AtmosphereRead {
        path: path.clone(),
        source,
        rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::AtmosphereRead {
            path: path.clone(),
            source,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_ATMOSPHERE_BLOB_BYTES {
        return Err(AssetStartupError::AtmosphereTooLarge {
            path,
            max_bytes: MAX_ATMOSPHERE_BLOB_BYTES,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ATMOSPHERE_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::AtmosphereRead {
            path: path.clone(),
            source,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_ATMOSPHERE_BLOB_BYTES {
        return Err(AssetStartupError::AtmosphereTooLarge {
            path,
            max_bytes: MAX_ATMOSPHERE_BLOB_BYTES,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        });
    }
    let runtime = Arc::new(RuntimeAtmosphereAssets::decode(&bytes).map_err(|source| {
        AssetStartupError::AtmosphereDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        }
    })?);
    let identity = runtime.carrier_identity();
    world_provenance::verify_atmosphere_carrier(&path, &runtime)?;
    Ok(LoadedAtmosphereAssets {
        runtime,
        identity,
        selected_path: path,
    })
}

#[cfg(all(test, not(windows)))]
mod review_tests {
    use super::*;

    #[test]
    fn review_rebuild_paths_cannot_expand_make_shell_functions() {
        let directory =
            std::env::temp_dir().join(format!("cinnabar-make-path-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let sentinel = directory.join("expanded");
        let path = PathBuf::from(format!("$(shell touch {})", sentinel.display()));
        let mut child = std::process::Command::new("sh")
            .args([
                "-c",
                &format!("make -f - CARRIER={}", shell_quote_path(&path)),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::io::Write::write_all(
            child.stdin.as_mut().unwrap(),
            b"all:\n\t@printf '%s\\n' \"$(CARRIER)\"\n",
        )
        .unwrap();
        drop(child.stdin.take());
        let _ = child.wait().unwrap();
        let executed = sentinel.exists();
        std::fs::remove_dir_all(directory).unwrap();
        assert!(
            !executed,
            "recovery guidance executed a make expression from the path"
        );
    }
}
