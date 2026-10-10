//! Validated custom inputs and immutable private files for the skin catalog.
use super::*;
use launcher::skin_import::{self, SkinPackEntry};

/// Imports a loose PNG and sibling JSON, or every skin declared by a skin-pack archive.
pub(crate) fn import(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    source: &Path,
) -> Result<(), String> {
    let entries = if source
        .extension()
        .is_some_and(|v| v.eq_ignore_ascii_case("mcpack"))
    {
        skin_import::parse_skin_pack(&read_bounded(
            source,
            resource_pack::MAX_ARCHIVE_BYTES as u64,
        )?)
        .map_err(|error| error.to_string())?
    } else {
        let png = if source
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("json"))
        {
            let stem = source
                .file_stem()
                .and_then(|v| v.to_str())
                .ok_or("Choose a skin PNG and its geometry JSON.")?;
            source.with_file_name(format!("{}.png", stem.strip_suffix(".geo").unwrap_or(stem)))
        } else {
            source.to_owned()
        };
        let geometry_paths = [png.with_extension("json"), png.with_extension("geo.json")];
        let paths: Vec<_> = geometry_paths.iter().filter(|path| path.exists()).collect();
        if paths.len() > 1 {
            return Err("The skin has multiple geometry files. Keep one matching JSON file or import a skin pack.".to_owned());
        }
        let geometry_path = paths.first().copied().unwrap_or(&geometry_paths[0]);
        let geometry = if geometry_path.exists() {
            Some(
                skin_import::parse_skin_geometry(
                    &read_bounded(
                        geometry_path,
                        protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64,
                    )?,
                    None,
                )
                .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        vec![SkinPackEntry {
            name: png
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("Imported skin")
                .to_owned(),
            png: png_bytes(&png)?,
            model: if geometry.is_some() {
                SkinModel::Custom
            } else {
                SkinModel::Classic
            },
            geometry,
            engine_version: protocol::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
        }]
    };
    let mut next = view.clone();
    let mut files = Vec::new();
    let result = (|| {
        for input in entries {
            let mut skin = decode_png_with_model(
                &input.png,
                alpha_model(input.model, input.geometry.as_deref()),
            )?;
            skin.geometry = match input.geometry {
                Some(geometry) => Some(geometry),
                None => geometry_for_view(layout, view, input.model)?,
            };
            let mut hash = Sha256::new();
            hash.update(&input.png);
            if input.model == SkinModel::Custom {
                let geometry = skin
                    .geometry
                    .as_ref()
                    .ok_or("The custom skin is missing its model.")?;
                validate_source(geometry)?;
                for bytes in [
                    geometry.resource_patch.as_bytes(),
                    geometry.geometry_data.as_bytes(),
                    input.engine_version.as_bytes(),
                ] {
                    hash.update((bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                }
            }
            if input.model == SkinModel::Slim {
                hash.update(SkinModel::Slim.geometry().as_bytes());
            }
            if input.model != SkinModel::Custom
                && input.engine_version.as_ref() != protocol::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION
            {
                hash.update((input.engine_version.len() as u64).to_le_bytes());
                hash.update(input.engine_version.as_bytes());
            }
            let digest = format!("{:x}", hash.finalize());
            let id = format!("imported:{digest}");
            if let Some(index) = next.skins.iter().position(|entry| entry.id == id) {
                next.selected = Some(index);
                continue;
            }
            if next.skins.iter().filter(|entry| entry.imported).count() >= MAX_IMPORTED_ITEMS {
                return Err(format!(
                    "The skin library already contains {MAX_IMPORTED_ITEMS} imported skins."
                ));
            }
            fs::create_dir_all(layout.dressing_room_dir()).map_err(|error| error.to_string())?;
            let path = layout.dressing_room_dir().join(format!("{digest}.png"));
            write_new(&path, &input.png, &mut files)?;
            if input.model == SkinModel::Custom {
                let geometry = skin.geometry.as_ref().unwrap();
                write_new(
                    &path.with_extension("json"),
                    geometry.geometry_data.as_bytes(),
                    &mut files,
                )?;
            }
            let mut skins = next.skins.to_vec();
            skins.push(DressingRoomSkin {
                id,
                name: edits::valid_name(&input.name)?.to_owned(),
                path: path.to_string_lossy().into_owned(),
                imported: true,
                model: input.model,
                skin,
                engine_version: input.engine_version,
            });
            next.selected = Some(skins.len() - 1);
            next.skins = skins.into();
        }
        save(layout, &next)
    })();
    if result.is_err() {
        for path in files {
            let _ = fs::remove_file(path);
        }
    } else {
        *view = next;
    }
    result
}

/// Tracks only newly created immutable files, so rollback cannot delete an older import.
fn write_new(path: &Path, bytes: &[u8], files: &mut Vec<PathBuf>) -> Result<(), String> {
    use std::io::Write;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            files.push(path.to_owned());
            file.write_all(bytes).map_err(|error| error.to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_bounded(path, bytes.len() as u64)? == bytes {
                Ok(())
            } else {
                Err(
                    "An imported skin file is damaged. Remove it before importing again."
                        .to_owned(),
                )
            }
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Restores a named model from a private file after validating its metadata and size again.
pub(super) fn restore(
    layout: &InstallLayout,
    geometry: &ImportedGeometry,
) -> Result<Arc<protocol::SkinGeometrySource>, String> {
    if !single_filename(&geometry.file) {
        return Err("The saved model path is invalid.".to_owned());
    }
    let bytes = read_bounded(
        &layout.dressing_room_dir().join(&geometry.file),
        protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64,
    )?;
    let source = skin_import::parse_skin_geometry(&bytes, Some(&geometry.identifier))
        .map_err(|error| error.to_string())?;
    validate_source(&source)?;
    Ok(source)
}

/// Validates imports and saved models with the shared mesh builder.
fn validate_source(source: &protocol::SkinGeometrySource) -> Result<(), String> {
    let geometry = assets::parse_skin_geometry(&source.resource_patch, &source.geometry_data)
        .ok()
        .flatten()
        .ok_or("The skin model could not be read.")?;
    render_model::skin_geometry(&geometry, render_model::DIAGNOSTIC_RIG_ID)
        .map(|_| ())
        .map_err(|_| {
            "The skin model has no drawable geometry or exceeds the model limits.".to_owned()
        })
}

/// Built-in geometry names retain their native alpha rules even when the pack overrides their model.
pub(super) fn alpha_model(
    model: SkinModel,
    geometry: Option<&protocol::SkinGeometrySource>,
) -> SkinModel {
    if model == SkinModel::Custom
        && geometry
            .and_then(|source| assets::skin_geometry_name(&source.resource_patch))
            .is_some_and(|name| {
                name == SkinModel::Classic.geometry()
                    || name == SkinModel::Slim.geometry().to_ascii_lowercase()
            })
    {
        SkinModel::Classic
    } else {
        model
    }
}

/// Saved engine versions contain exactly three bounded unsigned components.
pub(super) fn valid_engine_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
}
