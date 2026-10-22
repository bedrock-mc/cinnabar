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
    let options = match crate::option_file(&path) {
        Some(options) => match fs::symlink_metadata(&options) {
            Ok(metadata) if metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "shader settings path is a directory",
                ));
            }
            Ok(metadata) => Some((options, metadata.file_type())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        },
        None => None,
    };
    let kind = fs::symlink_metadata(&path)?.file_type();
    if kind.is_dir() {
        fs::remove_dir_all(&path)?;
    } else {
        remove_link_or_file(&path, kind)?;
    }
    if let Some((options, kind)) = options {
        match remove_link_or_file(&options, kind) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Unlinks a file or link. Windows directory links require the directory API even when
/// their target is populated; neither branch follows the target.
fn remove_link_or_file(path: &Path, kind: fs::FileType) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if kind.is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    #[cfg(not(windows))]
    let _ = kind;
    fs::remove_file(path)
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
    #[cfg(windows)]
    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "names the missing Windows link fixture"
    )]
    fn removing_a_windows_directory_link_preserves_the_populated_target() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("shaderpacks");
        let original = root.path().join("original");
        fs::create_dir(&library).unwrap();
        fs::create_dir_all(original.join("shaders")).unwrap();
        fs::write(original.join("shaders/final.fsh"), b"original shader").unwrap();
        if let Err(error) = std::os::windows::fs::symlink_dir(&original, library.join("Linked")) {
            if error.raw_os_error() == Some(1314) {
                eprintln!(
                    "skipping Windows shader directory link: missing fixture Windows symlink privilege"
                );
                return;
            }
            panic!("create Windows shader directory link: {error}");
        }
        remove_pack(&library, "Linked").unwrap();
        assert_eq!(
            fs::read(original.join("shaders/final.fsh")).unwrap(),
            b"original shader"
        );
        assert!(fs::symlink_metadata(library.join("Linked")).is_err());
    }
}
