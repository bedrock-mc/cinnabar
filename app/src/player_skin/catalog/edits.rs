//! Imported names and deletion keep preferences and private image ownership together.

use super::*;

pub(crate) fn rename(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    index: usize,
    name: &str,
) -> Result<(), String> {
    let name = valid_name(name)?;
    let mut skins = view.skins.to_vec();
    let entry = skins
        .get_mut(index)
        .filter(|entry| entry.imported)
        .ok_or("Only imported skins can be renamed.")?;
    entry.name = name.to_owned();
    let old = view.skins.clone();
    view.skins = skins.into();
    if let Err(error) = save(layout, view) {
        view.skins = old;
        return Err(error);
    }
    Ok(())
}

pub(super) fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().any(char::is_control)
        || name.len() > launcher::dressing_room::MAX_SKIN_NAME_BYTES
    {
        return Err("Choose a non-empty name within the text field's limit.".to_owned());
    }
    Ok(name)
}

pub(crate) fn delete(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    index: usize,
) -> Result<(), String> {
    let entry = view
        .skins
        .get(index)
        .filter(|entry| entry.imported)
        .ok_or("Only imported skins can be deleted.")?;
    let path = PathBuf::from(&entry.path);
    if path.parent() != Some(layout.dressing_room_dir().as_path()) {
        return Err("The imported skin does not belong to this library.".to_owned());
    }
    let mut next = view.clone();
    let mut skins = view.skins.to_vec();
    skins.remove(index);
    next.selected = match view.selected {
        Some(selected) if selected == index => Some(skins.iter().position(|entry|
            !entry.imported && entry.name == launcher::dressing_room::STARTER_SKIN_NAMES[0])
            .ok_or("Steve is unavailable. Install the vanilla skin assets before deleting the equipped skin.")?),
        Some(selected) if selected > index => Some(selected - 1),
        selected => selected,
    };
    let remove_image = !skins
        .iter()
        .any(|entry| entry.path == path.to_string_lossy());
    next.skins = skins.into();
    commit_deletion(layout, view, next, &path, remove_image)
}

pub(super) fn commit_deletion(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    next: DressingRoomView,
    path: &Path,
    remove_image: bool,
) -> Result<(), String> {
    let tombstone = path.with_extension(format!("{}.deleted", std::process::id()));
    let moved = if remove_image {
        match fs::rename(path, &tombstone) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("The imported image could not be removed: {error}")),
        }
    } else {
        false
    };
    if let Err(error) = save(layout, &next) {
        if moved {
            let _ = fs::rename(&tombstone, path);
        }
        return Err(error);
    }
    if moved && let Err(error) = fs::remove_file(tombstone) {
        bevy::log::warn!(?error, "deleted imported image cleanup failed");
    }
    *view = next;
    Ok(())
}
