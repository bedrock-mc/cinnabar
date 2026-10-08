//! Imported capes retain source texels and private file ownership.

use super::*;
use launcher::dressing_room::DressingRoomCape;

const CROPPED_CAPE_DIMENSIONS: (u32, u32) = (46, 22);

pub(super) fn load(layout: &InstallLayout, saved: Vec<ImportedCape>) -> Vec<DressingRoomCape> {
    saved
        .into_iter()
        .take(MAX_IMPORTED_ITEMS)
        .filter_map(|entry| {
            if !single_filename(&entry.file) {
                return None;
            }
            let path = layout.dressing_room_capes_dir().join(entry.file);
            let cape = decode(&png_bytes(&path).ok()?).ok()?;
            Some(DressingRoomCape {
                id: entry.id,
                name: entry.name,
                path: path.to_string_lossy().into_owned(),
                cape,
                imported: true,
            })
        })
        .collect()
}

pub(crate) fn select_cape(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    index: Option<usize>,
) -> Result<(), String> {
    if index.is_some_and(|index| view.capes.get(index).is_none()) {
        return Err("This cape is no longer available.".to_owned());
    }
    let previous = view.selected_cape;
    view.selected_cape = index;
    if let Err(error) = save(layout, view) {
        view.selected_cape = previous;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn import_cape(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    source: &Path,
) -> Result<(), String> {
    let bytes = png_bytes(source)?;
    let cape = decode(&bytes)?;
    let digest = Sha256::digest(&bytes);
    let id = format!("cape:{digest:x}");
    if let Some(index) = view.capes.iter().position(|entry| entry.id == id) {
        return select_cape(layout, view, Some(index));
    }
    if view.capes.iter().filter(|entry| entry.imported).count() >= MAX_IMPORTED_ITEMS {
        return Err(format!(
            "The cape library already contains {MAX_IMPORTED_ITEMS} imported capes."
        ));
    }
    let dir = layout.dressing_room_capes_dir();
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!("{digest:x}.png"));
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let entry = DressingRoomCape {
        id,
        name: source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Imported cape")
            .to_owned(),
        path: path.to_string_lossy().into_owned(),
        cape,
        imported: true,
    };
    let old = view.clone();
    let mut capes = view.capes.to_vec();
    capes.push(entry);
    view.selected_cape = Some(capes.len() - 1);
    view.capes = capes.into();
    if let Err(error) = save(layout, view) {
        *view = old;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn rename_cape(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    index: usize,
    name: &str,
) -> Result<(), String> {
    let name = edits::valid_name(name)?;
    let mut capes = view.capes.to_vec();
    capes
        .get_mut(index)
        .filter(|entry| entry.imported)
        .ok_or("Only imported capes can be renamed.")?
        .name = name.to_owned();
    let old = view.capes.clone();
    view.capes = capes.into();
    if let Err(error) = save(layout, view) {
        view.capes = old;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn delete_cape(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    index: usize,
) -> Result<(), String> {
    let path = PathBuf::from(
        &view
            .capes
            .get(index)
            .filter(|entry| entry.imported)
            .ok_or("Only imported capes can be deleted.")?
            .path,
    );
    if path.parent() != Some(layout.dressing_room_capes_dir().as_path()) {
        return Err("The imported cape does not belong to this library.".to_owned());
    }
    let mut next = view.clone();
    let mut capes = view.capes.to_vec();
    capes.remove(index);
    next.selected_cape = match view.selected_cape {
        Some(selected) if selected == index => None,
        Some(selected) if selected > index => Some(selected - 1),
        selected => selected,
    };
    let remove_image = !capes
        .iter()
        .any(|entry| entry.path == path.to_string_lossy());
    next.capes = capes.into();
    edits::commit_deletion(layout, view, next, &path, remove_image)
}

pub(super) fn decode(bytes: &[u8]) -> Result<protocol::CapeImage, String> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = protocol::CAPE_DIMENSIONS.iter().map(|size| size.0).max();
    limits.max_image_height = protocol::CAPE_DIMENSIONS.iter().map(|size| size.1).max();
    reader.limits(limits);
    let rgba = reader
        .decode()
        .map_err(|error| format!("The cape PNG could not be read: {error}"))?
        .to_rgba8();
    let padded_dimensions = protocol::CAPE_DIMENSIONS
        .iter()
        .copied()
        .find(|(width, _)| {
            let scale = width / protocol::CAPE_DIMENSIONS[0].0;
            rgba.dimensions()
                == (
                    CROPPED_CAPE_DIMENSIONS.0 * scale,
                    CROPPED_CAPE_DIMENSIONS.1 * scale,
                )
        });
    let rgba = if let Some((width, height)) = padded_dimensions {
        // Cropped textures keep the full sheet's UV origin; only missing margins are transparent.
        let mut padded = image::RgbaImage::new(width, height);
        image::imageops::replace(&mut padded, &rgba, 0, 0);
        padded
    } else {
        rgba
    };
    let cape = protocol::CapeImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba8: rgba.into_raw().into(),
    };
    if !cape.is_valid() {
        return Err(format!(
            "Unsupported cape dimensions {}×{}. Choose a supported cape PNG.",
            cape.width, cape.height
        ));
    }
    Ok(cape)
}

#[cfg(test)]
mod tests;
