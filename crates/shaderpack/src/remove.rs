//! Removal of imported library copies without following links to their original sources.

use std::{
    fs, io,
    path::{Component, Path},
};

/// Deletes one plain-name entry in `dir` and its option file. Links are never followed.
/// Run this blocking file operation on a worker, after user confirmation.
pub fn remove_pack(dir: &Path, name: &str) -> io::Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(component)), None) if component == std::ffi::OsStr::new(name)
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid shader pack name",
        ));
    }
    let _library = crate::install::LIBRARY_IO
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let path = dir.join(name);
    let options = crate::option_file(&path);
    if options.as_ref().is_some_and(|options| {
        fs::symlink_metadata(options).is_ok_and(|metadata| metadata.is_dir())
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "shader settings path is a directory",
        ));
    }
    let kind = fs::symlink_metadata(&path)?.file_type();
    if kind.is_dir() {
        fs::remove_dir_all(&path)?;
    } else {
        fs::remove_file(&path)?;
    }
    if let Some(options) = options {
        match fs::remove_file(options) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_imported_copies_and_their_settings_only() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("shaderpacks");
        fs::create_dir(&library).unwrap();
        for name in ["Dusk.zip", "Lumen"] {
            let path = library.join(name);
            if name.ends_with(".zip") {
                fs::write(&path, b"copy").unwrap();
            } else {
                fs::create_dir_all(path.join("Nested/shaders")).unwrap();
            }
            fs::write(crate::option_file(&path).unwrap(), b"QUALITY=2").unwrap();
            remove_pack(&library, name).unwrap();
            assert!(!path.exists());
            assert!(!crate::option_file(&path).unwrap().exists());
        }
        let source = root.path().join("Downloads/Imported");
        fs::create_dir_all(source.join("shaders")).unwrap();
        assert!(matches!(
            crate::add_dropped_packs(std::slice::from_ref(&source), &library),
            crate::DropOutcome::Added(_)
        ));
        remove_pack(&library, "Imported").unwrap();
        assert!(source.join("shaders").is_dir());
        let original = root.path().join("original.zip");
        fs::write(&original, b"original").unwrap();
        for name in [
            "../original.zip",
            "..",
            ".",
            "",
            "/tmp/pack.zip",
            "Dusk/.",
            "Dusk/",
            "./Dusk",
        ] {
            assert!(remove_pack(&library, name).is_err());
        }
        assert_eq!(fs::read(original).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_folder_link_preserves_the_original_pack() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("shaderpacks");
        let original = root.path().join("original");
        fs::create_dir(&library).unwrap();
        fs::create_dir_all(original.join("shaders")).unwrap();
        std::os::unix::fs::symlink(&original, library.join("Linked")).unwrap();
        remove_pack(&library, "Linked").unwrap();
        assert!(original.join("shaders").is_dir());
        assert!(fs::symlink_metadata(library.join("Linked")).is_err());
    }
}
