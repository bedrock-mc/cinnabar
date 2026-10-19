//! `.mcworld` export: packages a saved world's folder into a standard Bedrock zip archive.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

use bridge::Backend;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::{
    store::{LEVEL_DIR, META_FILE, Store, THUMBNAIL_FILE},
    world::{WorldError, failed},
};

const LEVEL_DAT: &str = "level.dat";
const LEVEL_NAME_FILE: &str = "levelname.txt";
const LOCK_FILE: &str = "LOCK";

/// Exports the saved world `id` as a `.mcworld` archive at `destination`.
pub(crate) fn export(store: &Store, id: &str, destination: &Path) -> Result<(), WorldError> {
    let world = store.get(id)?;
    let world_dir = store.dir(id)?;

    // Locate the world data directory holding level.dat
    let level_dir = world_dir.join(LEVEL_DIR);
    let source_dir = if level_dir.join(LEVEL_DAT).is_file() {
        level_dir
    } else if world_dir.join(LEVEL_DAT).is_file() {
        world_dir.clone()
    } else {
        return Err(WorldError::NoWorldData);
    };

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| failed(format!("create export directory: {e}")))?;
    }

    let temp_name = format!(".export-{id}-{}.tmp", super::provision::unix_now());
    let temp_dest = destination
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(temp_name);

    let result = (|| -> Result<(), WorldError> {
        let file = File::create(&temp_dest)
            .map_err(|e| failed(format!("create temporary export file: {e}")))?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let mut has_level_name = false;
        let mut has_world_icon = false;

        pack_directory(
            &source_dir,
            &source_dir,
            &mut zip,
            options,
            &mut has_level_name,
            &mut has_world_icon,
        )?;

        if !has_level_name && !world.name.is_empty() {
            zip.start_file(LEVEL_NAME_FILE, options)
                .map_err(|e| failed(format!("start levelname entry: {e}")))?;
            zip.write_all(world.name.as_bytes())
                .map_err(|e| failed(format!("write levelname entry: {e}")))?;
        }

        if !has_world_icon {
            let icon_path = world_dir.join(THUMBNAIL_FILE);
            if icon_path.is_file()
                && let Ok(mut icon_file) = File::open(&icon_path)
            {
                zip.start_file(THUMBNAIL_FILE, options)
                    .map_err(|e| failed(format!("start thumbnail entry: {e}")))?;
                io::copy(&mut icon_file, &mut zip)
                    .map_err(|e| failed(format!("write thumbnail entry: {e}")))?;
            }
        }

        if world.backend == Backend::Dragonfly {
            let resources_dir = world_dir.join("resources");
            if resources_dir.is_dir() {
                pack_directory_as(
                    &resources_dir,
                    &resources_dir,
                    "resource_packs",
                    &mut zip,
                    options,
                )?;
            }
        }

        zip.finish()
            .map_err(|e| failed(format!("finalize export zip: {e}")))?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = fs::remove_file(&temp_dest);
        return Err(err);
    }

    if fs::rename(&temp_dest, destination).is_err() {
        let _ = fs::remove_file(destination);
        if let Err(e) = fs::rename(&temp_dest, destination) {
            let _ = fs::remove_file(&temp_dest);
            return Err(failed(format!("save export file: {e}")));
        }
    }

    Ok(())
}

fn pack_directory(
    root: &Path,
    current: &Path,
    zip: &mut ZipWriter<File>,
    options: SimpleFileOptions,
    has_level_name: &mut bool,
    has_world_icon: &mut bool,
) -> Result<(), WorldError> {
    let entries =
        fs::read_dir(current).map_err(|e| failed(format!("read world directory: {e}")))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| failed(format!("inspect file: {e}")))?;

        if file_type.is_dir() {
            pack_directory(root, &path, zip, options, has_level_name, has_world_icon)?;
        } else if file_type.is_file() {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if name_str == LOCK_FILE
                || name_str == META_FILE
                || name_str.starts_with('.')
                || name_str.ends_with(".tmp")
                || name_str.ends_with(".bak")
            {
                continue;
            }

            let rel_path = path
                .strip_prefix(root)
                .map_err(|e| failed(format!("relative path: {e}")))?;
            let zip_path = rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");

            if zip_path == LEVEL_NAME_FILE {
                *has_level_name = true;
            }
            if zip_path == THUMBNAIL_FILE {
                *has_world_icon = true;
            }

            zip.start_file(&zip_path, options)
                .map_err(|e| failed(format!("start zip entry {zip_path}: {e}")))?;
            let mut file = File::open(&path)
                .map_err(|e| failed(format!("read file {}: {e}", path.display())))?;
            io::copy(&mut file, zip)
                .map_err(|e| failed(format!("write file {}: {e}", path.display())))?;
        }
    }
    Ok(())
}

fn pack_directory_as(
    root: &Path,
    current: &Path,
    prefix: &str,
    zip: &mut ZipWriter<File>,
    options: SimpleFileOptions,
) -> Result<(), WorldError> {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            pack_directory_as(root, &path, prefix, zip, options)?;
        } else if file_type.is_file() {
            let rel_path = match path.strip_prefix(root) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let sub_path = rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let zip_path = format!("{prefix}/{sub_path}");

            if zip.start_file(&zip_path, options).is_ok()
                && let Ok(mut file) = File::open(&path)
            {
                let _ = io::copy(&mut file, zip);
            }
        }
    }
    Ok(())
}
