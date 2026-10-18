use std::path::{Path, PathBuf};

use assets::AssetError;
use image::ImageFormat;

/// Native extension priority for an extensionless resource reference.
pub const IMAGE_EXTENSIONS: [(&str, ImageFormat); 4] = [
    ("tga", ImageFormat::Tga),
    ("png", ImageFormat::Png),
    ("jpg", ImageFormat::Jpeg),
    ("jpeg", ImageFormat::Jpeg),
];

/// Selects an explicit path or the highest-priority supported static image.
pub fn static_texture_path(root: &Path, source: &str, key: &str) -> Result<PathBuf, AssetError> {
    if Path::new(source).extension().is_some() {
        return Ok(root.join(source));
    }
    for (extension, format) in IMAGE_EXTENSIONS {
        if !matches!(format, ImageFormat::Png | ImageFormat::Tga) {
            continue;
        }
        let path = root.join(format!("{source}.{extension}"));
        if path.try_exists().map_err(|source| AssetError::TextureIo {
            key: key.into(),
            path: path.clone(),
            source,
        })? {
            return Ok(path);
        }
    }
    Ok(root.join(format!("{source}.png")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensionless_images_choose_tga_and_explicit_png_remains_explicit() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("grass.png"), b"png").unwrap();
        std::fs::write(root.path().join("grass.tga"), b"tga").unwrap();
        assert_eq!(
            static_texture_path(root.path(), "grass", "grass").unwrap(),
            root.path().join("grass.tga")
        );
        assert_eq!(
            static_texture_path(root.path(), "grass.png", "grass").unwrap(),
            root.path().join("grass.png")
        );
    }
}
