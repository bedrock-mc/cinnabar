//! Installed pack artwork is decoded on the import worker and uses the menu atlas.

use launcher::global_resources::Snapshot;
use resource_pack::GlobalPackLibrary;
use std::{
    hash::{Hash, Hasher},
    io::Cursor,
    path::Path,
};

/// Refreshes bounded, validated icons independently of the active pack stack.
pub(super) fn refresh(library: &GlobalPackLibrary, snapshot: &mut Snapshot, root: &Path) {
    snapshot.icons.clear();
    if std::fs::create_dir_all(root).is_err() {
        return;
    }
    let packs = library.available().iter().chain(
        library
            .active()
            .iter()
            .filter_map(|active| library.metadata(active)),
    );
    for pack in packs {
        let Some(bytes) = library.pack_icon(pack).ok().flatten() else {
            continue;
        };
        let Some(png) = thumbnail(&bytes) else {
            continue;
        };
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        png.hash(&mut hash);
        let path = root.join(format!("{}-{:016x}.png", pack.id, hash.finish()));
        if (path.exists() || std::fs::write(&path, png).is_ok())
            && let Some(path) = path.to_str()
        {
            snapshot
                .icons
                .insert((pack.id.to_string(), pack.revision), path.to_owned());
        }
    }
}

/// Rejects corrupt or oversized images before preparing a small menu thumbnail.
fn thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(256, 256);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_bounds_pack_icons() {
        assert!(thumbnail(b"not an image").is_none());
        let mut png = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(512, 128, image::Rgba([12, 34, 56, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = image::load_from_memory(&thumbnail(png.get_ref()).unwrap()).unwrap();
        assert_eq!((image.width(), image.height()), (256, 64));
    }
}
