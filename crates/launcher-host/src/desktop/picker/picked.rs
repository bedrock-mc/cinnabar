use std::path::{Path, PathBuf};

/// A chosen document. Android's private copy is removed after its importer finishes.
#[derive(Debug)]
pub struct PickedFile {
    path: PathBuf,
    copied: bool,
}

impl From<PathBuf> for PickedFile {
    /// Borrows a user's source file without taking deletion ownership.
    fn from(path: PathBuf) -> Self {
        Self {
            path,
            copied: false,
        }
    }
}

impl AsRef<Path> for PickedFile {
    /// Borrows the document while its private copy, if any, remains alive.
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl PickedFile {
    /// Owns a copy under the Android picker's selection/index directories.
    #[cfg(any(target_os = "android", test))]
    pub(super) fn copied(path: PathBuf) -> Self {
        Self { path, copied: true }
    }
}

impl Drop for PickedFile {
    /// Removes only this copy and empty picker directories, preserving other active selections.
    fn drop(&mut self) {
        if self.copied {
            let _ = std::fs::remove_file(&self.path);
            if let Some(index) = self.path.parent() {
                let _ = std::fs::remove_dir(index);
                if let Some(selection) = index.parent() {
                    let _ = std::fs::remove_dir(selection);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_live_until_consumed_and_cleanup_preserves_other_selections() {
        let root = tempfile::tempdir().unwrap();
        let paths: Vec<_> = ["one/0/mod.wasm", "one/1/pack.zip", "two/0/skin.png"]
            .map(|name| {
                let path = root.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, b"document").unwrap();
                path
            })
            .into();
        let mut files: Vec<_> = paths.iter().cloned().map(PickedFile::copied).collect();
        drop(files.remove(0));
        assert!(!paths[0].exists());
        assert!(paths[1].exists() && paths[2].exists());
        drop(files.remove(0));
        assert!(!root.path().join("one").exists());
        assert_eq!(std::fs::read(files[0].as_ref()).unwrap(), b"document");
        drop(files);
        assert!(!root.path().join("two").exists());
    }

    #[test]
    fn desktop_sources_are_never_removed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.wasm");
        std::fs::write(&path, b"original").unwrap();
        drop(PickedFile::from(path.clone()));
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
}
