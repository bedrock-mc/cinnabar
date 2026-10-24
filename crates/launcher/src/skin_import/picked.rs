//! File-picker admission, before browser files are read or copied into Wasm.

use super::{SkinImportError, SkinModel, SkinPackEntry};

/// One loose PNG, its optional companion, or a single skin archive per gesture.
pub const MAX_PICKED_FILES: usize = 2;
/// Includes JSON escaping of bounded filenames and the numeric byte lengths.
pub const MAX_PICKED_METADATA_BYTES: usize =
    MAX_PICKED_FILES * (resource_pack::MAX_PATH_BYTES * 6 + 32);

/// An owned file admitted by name and size before reading its contents.
#[derive(Debug)]
pub struct PickedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Rejects unsafe names, unrelated companions and oversized selections before any byte copy.
pub fn admit(files: &[(&str, u64)], cape: bool) -> Result<(), SkinImportError> {
    if files.is_empty() || files.len() > MAX_PICKED_FILES {
        return Err(SkinImportError::TooLarge);
    }
    let mut png = None;
    let mut json = None;
    for &(name, size) in files {
        if name.is_empty()
            || name.len() > resource_pack::MAX_PATH_BYTES
            || name.contains(['/', '\\', ':'])
            || name.chars().any(char::is_control)
        {
            return Err(SkinImportError::Archive);
        }
        let lower = name.to_ascii_lowercase();
        let limit = if let Some(stem) = lower.strip_suffix(".png") {
            if png.replace(stem.to_owned()).is_some() {
                return Err(SkinImportError::Metadata);
            }
            resource_pack::MAX_PACK_TEXTURE_BYTES
        } else if let Some(stem) = lower.strip_suffix(".json").filter(|_| !cape) {
            if json
                .replace(stem.strip_suffix(".geo").unwrap_or(stem).to_owned())
                .is_some()
            {
                return Err(SkinImportError::Metadata);
            }
            render_api::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64
        } else if lower.ends_with(".mcpack") && !cape && files.len() == 1 {
            resource_pack::MAX_LOCAL_ARCHIVE_BYTES as u64
        } else {
            return Err(SkinImportError::Unsupported);
        };
        if size == 0 || size > limit {
            return Err(SkinImportError::TooLarge);
        }
    }
    if json.is_some() && json != png {
        return Err(SkinImportError::MissingFile);
    }
    Ok(())
}

/// Validates loose input through the same geometry parser as an archive import.
pub fn loose(mut files: Vec<PickedFile>) -> Result<SkinPackEntry, SkinImportError> {
    admit(
        &files
            .iter()
            .map(|file| (file.name.as_str(), file.bytes.len() as u64))
            .collect::<Vec<_>>(),
        false,
    )?;
    let index = files
        .iter()
        .position(|file| file.name.to_ascii_lowercase().ends_with(".png"))
        .ok_or(SkinImportError::MissingFile)?;
    let png = files.remove(index);
    let geometry = files
        .first()
        .map(|file| super::parse_skin_geometry(&file.bytes, None))
        .transpose()?;
    Ok(SkinPackEntry {
        name: png.name[..png.name.len() - ".png".len()].to_owned(),
        png: png.bytes,
        model: if geometry.is_some() {
            SkinModel::Custom
        } else {
            SkinModel::Classic
        },
        geometry,
        engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_admission_rejects_invalid_large_and_unrelated_files() {
        for files in [
            vec![("skin.png", resource_pack::MAX_PACK_TEXTURE_BYTES + 1)],
            vec![("../skin.png", 1)],
            vec![("skin.png", 1), ("other.json", 1)],
            vec![("skin.png", 1), ("skin.json", 1), ("extra.png", 1)],
            vec![("skin.png", 1), ("skin.PNG", 1)],
            vec![("skin.json", 1)],
        ] {
            assert!(admit(&files, false).is_err(), "{files:?}");
        }
        assert!(admit(&[("Skin.PNG", 1), ("skin.geo.JSON", 1)], false).is_ok());
        assert!(admit(&[("skin.mcpack", 1)], false).is_ok());
        assert!(admit(&[("skin.mcpack", 1)], true).is_err());
        assert!(admit(&[("cape.png", 1)], true).is_ok());
    }

    #[test]
    fn companion_geometry_uses_the_existing_parser_and_preserves_original_bytes() {
        let geometry = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.picker.fixture","texture_width":64,"texture_height":64},
            "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[0,0]}]}]
        }]}).to_string().into_bytes();
        let entry = loose(vec![
            PickedFile {
                name: "Skin.geo.json".into(),
                bytes: geometry.clone(),
            },
            PickedFile {
                name: "Skin.PNG".into(),
                bytes: vec![1, 2, 3],
            },
        ])
        .unwrap();
        assert_eq!(entry.name, "Skin");
        assert_eq!(entry.model, SkinModel::Custom);
        assert_eq!(entry.png, [1, 2, 3]);
        assert_eq!(entry.geometry.unwrap().geometry_data.as_bytes(), geometry);
    }
}
