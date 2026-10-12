//! Installed pack artwork is decoded on the import worker, stored beside the packs, and uses the
//! menu atlas.

use launcher::global_resources::Snapshot;
use resource_pack::{GlobalPackLibrary, InstalledPack};
use std::io::Cursor;

/// Refreshes bounded, validated icons independently of the active pack stack. Each archive
/// revision's thumbnail is made once and kept as `icons/<id>-<revision>.png`, so later refreshes
/// read no archives; a browser holds only the archives it is reading.
pub(super) fn refresh(library: &mut GlobalPackLibrary, snapshot: &mut Snapshot) {
    snapshot.icons.clear();
    let packs: Vec<InstalledPack> = library
        .available()
        .iter()
        .chain(
            library
                .active()
                .iter()
                .filter_map(|active| library.metadata(active)),
        )
        .cloned()
        .collect();
    for pack in packs {
        let name = format!("icons/{}-{}.png", pack.id, pack.revision);
        let stored = library.storage().contains(&name) || {
            let png = library
                .pack_icon(&pack)
                .ok()
                .flatten()
                .and_then(|bytes| thumbnail(&bytes));
            png.is_some_and(|png| library.storage_mut().write(&name, &png).is_ok())
        };
        if stored && let Some(path) = library.storage().path(&name).to_str() {
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
