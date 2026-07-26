use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use assets::AssetError;

use super::paths_alias;

pub(super) fn validate_output_bundle(blob: &Path, report: &Path) -> Result<(), AssetError> {
    let normalized_blob = normalized_absolute(blob)?;
    let normalized_report = normalized_absolute(report)?;
    if paths_alias(&normalized_blob, &normalized_report)
        || paths_alias(
            &canonicalized_location(&normalized_blob)?,
            &canonicalized_location(&normalized_report)?,
        )
    {
        return Err(output_alias_error(blob));
    }

    let mut both_exist = true;
    for path in [blob, report] {
        match fs::metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "atmosphere output destination is not a regular file",
                    ),
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => both_exist = false,
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    if both_exist {
        match same_file::is_same_file(blob, report) {
            Ok(true) => return Err(output_alias_error(blob)),
            Ok(false) => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: blob.to_path_buf(),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn output_alias_error(path: &Path) -> AssetError {
    AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "atmosphere blob and report paths must identify distinct files",
        ),
    }
}

fn normalized_absolute(path: &Path) -> Result<PathBuf, AssetError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| AssetError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source: io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "output path escapes its filesystem root",
                        ),
                    });
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

fn canonicalized_location(path: &Path) -> Result<PathBuf, AssetError> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut canonical) => {
                for component in suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let Some(file_name) = ancestor.file_name() else {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                };
                suffix.push(file_name.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                };
                ancestor = parent;
            }
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}
