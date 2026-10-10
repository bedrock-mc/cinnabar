//! Bounded skin-pack parsing without extracting archive paths onto disk.

use crate::dressing_room::SkinModel;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::{Cursor, Read},
    sync::Arc,
};
use zip::ZipArchive;

/// Imported items retained by one skin library.
pub const MAX_IMPORTED_SKINS: usize = 256;
/// Geometry without an explicit minimum engine version uses Bedrock's default.
use render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION;

/// Original image bytes and validated model inputs for one declared skin.
#[derive(Debug)]
pub struct SkinPackEntry {
    pub name: String,
    pub png: Vec<u8>,
    pub model: SkinModel,
    pub geometry: Option<Arc<render_api::SkinGeometrySource>>,
    pub engine_version: Arc<str>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkinImportError {
    #[error("The skin pack is malformed or has unsafe or duplicate paths.")]
    Archive,
    #[error("The skin pack exceeds the import size or item limit.")]
    TooLarge,
    #[error("The skin pack is missing a required file.")]
    MissingFile,
    #[error("The skin pack metadata could not be read.")]
    Metadata,
    #[error("The skin model is missing, malformed, ambiguous, or exceeds the renderer's limits.")]
    Geometry,
    #[error("The skin pack does not contain the named model.")]
    MissingGeometry,
    #[error("Only free static skin-pack entries can be imported.")]
    Unsupported,
}

#[derive(Deserialize)]
struct Catalog {
    skins: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    localization_name: String,
    texture: String,
    geometry: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    animations: serde_json::Value,
}

/// Reads every declared free skin, rejecting the entire import before any files are persisted.
pub fn parse_skin_pack(bytes: &[u8]) -> Result<Vec<SkinPackEntry>, SkinImportError> {
    if bytes.len() > resource_pack::MAX_ARCHIVE_BYTES {
        return Err(SkinImportError::TooLarge);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| SkinImportError::Archive)?;
    if zip.len() > resource_pack::MAX_ENTRIES_PER_PACK {
        return Err(SkinImportError::TooLarge);
    }
    let mut paths = HashSet::new();
    let mut total = 0u64;
    let mut catalogs = Vec::new();
    for index in 0..zip.len() {
        let file = zip
            .by_index_raw(index)
            .map_err(|_| SkinImportError::Archive)?;
        let name = file.name();
        if file.is_dir() {
            continue;
        }
        if !safe_path(name)
            || !paths.insert(name.to_ascii_lowercase())
            || file.encrypted()
            || file.is_symlink()
        {
            return Err(SkinImportError::Archive);
        }
        total = total
            .checked_add(file.size())
            .ok_or(SkinImportError::TooLarge)?;
        if file.size() > resource_pack::MAX_FILE_BYTES
            || total > resource_pack::MAX_DECLARED_BYTES_PER_PACK
        {
            return Err(SkinImportError::TooLarge);
        }
        if name == "skins.json" || name.ends_with("/skins.json") {
            catalogs.push(name.to_owned());
        }
    }
    if catalogs.len() != 1 {
        return Err(SkinImportError::Metadata);
    }
    let catalog_path = &catalogs[0];
    let root = catalog_path
        .strip_suffix("skins.json")
        .ok_or(SkinImportError::Metadata)?;
    let manifest = read(
        &mut zip,
        &format!("{root}manifest.json"),
        resource_pack::MAX_MANIFEST_BYTES,
    )?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest).map_err(|_| SkinImportError::Metadata)?;
    let modules = manifest
        .get("modules")
        .and_then(|v| v.as_array())
        .ok_or(SkinImportError::Metadata)?;
    if modules.is_empty()
        || modules
            .iter()
            .any(|v| v.get("type").and_then(|v| v.as_str()) != Some("skin_pack"))
    {
        return Err(SkinImportError::Unsupported);
    }
    let engine_version = engine_version(&manifest)?;
    let catalog = read(
        &mut zip,
        catalog_path,
        render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES,
    )?;
    let catalog: Catalog =
        serde_json::from_slice(&catalog).map_err(|_| SkinImportError::Metadata)?;
    if catalog.skins.is_empty() || catalog.skins.len() > MAX_IMPORTED_SKINS {
        return Err(SkinImportError::TooLarge);
    }
    let data = if paths.contains(&format!("{root}geometry.json").to_ascii_lowercase()) {
        Some(read(
            &mut zip,
            &format!("{root}geometry.json"),
            render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES,
        )?)
    } else {
        None
    };
    let mut entries = Vec::with_capacity(catalog.skins.len());
    let mut retained = 0usize;
    for entry in catalog.skins {
        if entry.kind != "free" || !entry.animations.is_null() {
            return Err(SkinImportError::Unsupported);
        }
        if !safe_path(&entry.texture) || !entry.texture.to_ascii_lowercase().ends_with(".png") {
            return Err(SkinImportError::Archive);
        }
        if entry.localization_name.trim().is_empty()
            || entry.localization_name.chars().any(char::is_control)
            || entry.localization_name.len() > crate::dressing_room::MAX_SKIN_NAME_BYTES
        {
            return Err(SkinImportError::Metadata);
        }
        let model = match entry.geometry.as_str() {
            name if name == SkinModel::Classic.geometry() => SkinModel::Classic,
            name if name == SkinModel::Slim.geometry() => SkinModel::Slim,
            _ => SkinModel::Custom,
        };
        let geometry = match &data {
            Some(data) => match parse_skin_geometry(data, Some(&entry.geometry)) {
                Ok(geometry) => Some(geometry),
                Err(SkinImportError::MissingGeometry) if model != SkinModel::Custom => None,
                Err(error) => return Err(error),
            },
            None if model != SkinModel::Custom => None,
            None => return Err(SkinImportError::MissingFile),
        };
        let model = if geometry.is_some() {
            SkinModel::Custom
        } else {
            model
        };
        let png = read(
            &mut zip,
            &format!("{root}{}", entry.texture),
            resource_pack::MAX_PACK_TEXTURE_BYTES as usize,
        )?;
        retained = retained
            .checked_add(png.len() + geometry.as_ref().map_or(0, |source| source.byte_len()))
            .ok_or(SkinImportError::TooLarge)?;
        if retained > resource_pack::MAX_ARCHIVE_BYTES {
            return Err(SkinImportError::TooLarge);
        }
        entries.push(SkinPackEntry {
            name: entry.localization_name,
            png,
            model,
            geometry,
            engine_version: engine_version.clone(),
        });
    }
    Ok(entries)
}

/// Validates an explicit geometry identifier, or the sole model in a loose JSON file.
pub fn parse_skin_geometry(
    bytes: &[u8],
    identifier: Option<&str>,
) -> Result<Arc<render_api::SkinGeometrySource>, SkinImportError> {
    if bytes.len() > render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES {
        return Err(SkinImportError::TooLarge);
    }
    let data = std::str::from_utf8(bytes).map_err(|_| SkinImportError::Geometry)?;
    let root: serde_json::Value =
        serde_json::from_str(data).map_err(|_| SkinImportError::Geometry)?;
    let mut names = Vec::new();
    if let Some(models) = root.get("minecraft:geometry").and_then(|v| v.as_array()) {
        for model in models {
            names.push(
                model
                    .get("description")
                    .and_then(|v| v.get("identifier"))
                    .and_then(|v| v.as_str())
                    .ok_or(SkinImportError::Geometry)?,
            );
        }
    } else if let Some(root) = root.as_object() {
        names.extend(
            root.keys()
                .filter(|name| name.starts_with("geometry."))
                .map(String::as_str),
        );
    }
    let name = match identifier {
        Some(name)
            if names.iter().any(|candidate| {
                candidate.eq_ignore_ascii_case(name)
                    || candidate
                        .split(':')
                        .next()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            }) =>
        {
            name
        }
        None if names.len() == 1 => names[0]
            .split(':')
            .next()
            .ok_or(SkinImportError::Geometry)?,
        Some(_) => return Err(SkinImportError::MissingGeometry),
        None => return Err(SkinImportError::Geometry),
    };
    if !name.starts_with("geometry.") || name.chars().any(char::is_control) {
        return Err(SkinImportError::Geometry);
    }
    let patch = serde_json::json!({"geometry":{"default":name}}).to_string();
    if patch.len() + bytes.len() > render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES {
        return Err(SkinImportError::TooLarge);
    }
    let model = assets::parse_skin_geometry(&patch, data)
        .map_err(|_| SkinImportError::Geometry)?
        .ok_or(SkinImportError::Geometry)?;
    if model.bones.iter().all(|bone| bone.cubes.is_empty())
        && model
            .poly_meshes
            .iter()
            .all(|mesh| mesh.as_ref().is_none_or(|mesh| mesh.vertices.is_empty()))
    {
        return Err(SkinImportError::Geometry);
    }
    Ok(Arc::new(render_api::SkinGeometrySource {
        resource_patch: patch.into(),
        geometry_data: data.into(),
        animations: Arc::from([]),
    }))
}

/// Accepts relative ZIP paths consistently on Windows and Unix.
fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= resource_pack::MAX_PATH_BYTES
        && !name.contains(['\\', ':'])
        && !name.chars().any(char::is_control)
        && name.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// Bounded decompression also detects truncated content and CRC errors.
fn read(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: usize,
) -> Result<Vec<u8>, SkinImportError> {
    let file = zip
        .by_name(path)
        .map_err(|_| SkinImportError::MissingFile)?;
    if file.size() > limit as u64 {
        return Err(SkinImportError::TooLarge);
    }
    let expected = file.size();
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SkinImportError::Archive)?;
    if bytes.len() > limit {
        return Err(SkinImportError::TooLarge);
    }
    if bytes.len() as u64 != expected {
        return Err(SkinImportError::Archive);
    }
    Ok(bytes)
}

/// Keeps the manifest's minimum engine version separate from the geometry schema version.
fn engine_version(manifest: &serde_json::Value) -> Result<Arc<str>, SkinImportError> {
    let Some(version) = manifest
        .get("header")
        .and_then(|v| v.get("min_engine_version"))
    else {
        return Ok(DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into());
    };
    let numbers = version
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or(SkinImportError::Metadata)?;
    let numbers = numbers
        .iter()
        .map(|v| {
            v.as_u64()
                .filter(|v| *v <= u16::MAX as u64)
                .ok_or(SkinImportError::Metadata)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("{}.{}.{}", numbers[0], numbers[1], numbers[2]).into())
}

#[cfg(test)]
mod tests;
