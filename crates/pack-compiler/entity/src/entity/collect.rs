//! Filesystem walkers that gather the vanilla entity source families.

use super::*;

pub(super) fn collect_family(
    root: &Path,
    relative_root: &str,
    allowed_extensions: &[&str],
    output: &mut Vec<(Box<str>, PathBuf)>,
) -> Result<(), AssetError> {
    let absolute_root = root.join(relative_root);
    let metadata = fs::symlink_metadata(&absolute_root).map_err(|source| AssetError::Io {
        path: absolute_root.clone(),
        source,
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("entity asset family root must be a real directory"));
    }
    collect_directory(root, &absolute_root, allowed_extensions, output, 0, true)
}

/// Collects an optional source tree in stable order, rejecting symlinks and unsafe entries.
pub(crate) fn collect_optional_family(
    root: &Path,
    relative_root: &str,
    allowed_extensions: &[&str],
    output: &mut Vec<(Box<str>, PathBuf)>,
) -> Result<(), AssetError> {
    let absolute_root = root.join(relative_root);
    match fs::symlink_metadata(&absolute_root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            collect_directory(root, &absolute_root, allowed_extensions, output, 0, false)
        }
        Ok(_) => Err(invalid(
            "optional entity asset family must be a real directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AssetError::Io {
            path: absolute_root,
            source,
        }),
    }
}

pub(super) fn collect_optional_file(
    root: &Path,
    relative_path: &str,
    output: &mut Vec<(Box<str>, PathBuf)>,
) -> Result<(), AssetError> {
    let path = root.join(relative_path);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            output.push((relative_path.into(), path));
            if output.len() > MAX_ENTITY_ASSET_SOURCES {
                return Err(invalid("entity asset source count exceeds bound"));
            }
            Ok(())
        }
        Ok(_) => Err(invalid("optional entity asset source must be a real file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AssetError::Io { path, source }),
    }
}

/// Collects the block-texture rasters the item atlas names, so an item drawn from block art keeps
/// its sprite. Only plain `textures/blocks/` paths are admitted.
pub(super) fn collect_atlas_block_rasters(
    root: &Path,
    output: &mut Vec<(Box<str>, PathBuf)>,
) -> Result<(), AssetError> {
    let atlas = root.join("textures/item_texture.json");
    if !atlas.is_file() {
        return Ok(());
    }
    let atlas = parse_semantic_json(&atlas, &read_bounded_source(root, &atlas)?)?;
    let Some(texture_data) = atlas.get("texture_data").and_then(Value::as_object) else {
        return Ok(());
    };
    let mut stems = BTreeSet::new();
    for definition in texture_data.values() {
        let textures = match definition.get("textures") {
            Some(Value::String(texture)) => vec![texture.as_str()],
            Some(Value::Array(textures)) => textures.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        for texture in textures {
            let texture = texture.replace('\\', "/");
            let stem = texture
                .strip_suffix(".png")
                .or_else(|| texture.strip_suffix(".tga"))
                .unwrap_or(&texture);
            let plain = stem.strip_prefix("textures/blocks/").is_some_and(|rest| {
                rest.split('/').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                })
            });
            if plain {
                stems.insert(stem.to_owned());
            }
        }
    }
    for stem in stems {
        for extension in ["png", "tga"] {
            collect_optional_file(root, &format!("{stem}.{extension}"), output)?;
        }
    }
    Ok(())
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    allowed_extensions: &[&str],
    output: &mut Vec<(Box<str>, PathBuf)>,
    depth: usize,
    reject_unsupported: bool,
) -> Result<(), AssetError> {
    if depth > MAX_ENTITY_SOURCE_DIRECTORY_DEPTH {
        return Err(invalid("entity asset source directory depth exceeds bound"));
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|source| AssetError::Io {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| AssetError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| AssetError::Io {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                "entity asset source trees may not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            collect_directory(
                root,
                &path,
                allowed_extensions,
                output,
                depth + 1,
                reject_unsupported,
            )?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid(
                "entity asset source tree contains a non-file entry",
            ));
        }
        let extension = path.extension().and_then(|extension| extension.to_str());
        if !extension.is_some_and(|extension| allowed_extensions.contains(&extension)) {
            if !reject_unsupported {
                continue;
            }
            return Err(invalid(format!(
                "unsupported entity asset source extension at {}",
                path.display()
            )));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("entity asset source escaped the pack root"))?
            .to_string_lossy()
            .replace('\\', "/");
        output.push((relative.into_boxed_str(), path));
        if output.len() > MAX_ENTITY_ASSET_SOURCES {
            return Err(invalid("entity asset source count exceeds bound"));
        }
    }
    Ok(())
}
