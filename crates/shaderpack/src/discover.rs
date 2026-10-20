//! Lists the shader packs a player dropped into the shader-pack directory.

use std::path::{Path, PathBuf};

/// One pack found by [`discover_packs`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPack {
    /// The file or folder name inside the scanned directory; the stable selection key.
    pub name: String,
    /// What to open with [`crate::ShaderPack::open`]: the zip, the folder holding `shaders/`,
    /// or the single nested folder that holds it.
    pub path: PathBuf,
}

/// Entries of `dir` that look like shader packs: `.zip` files (not opened here) and folders
/// with a `shaders` folder directly or inside their single sub-folder. Sorted by name,
/// ignoring case; a missing or unreadable directory, and unreadable or non-UTF-8 entries,
/// yield nothing.
pub fn discover_packs(dir: &Path) -> Vec<DiscoveredPack> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut packs: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = entry.path();
            let path = if path.is_dir() {
                pack_folder(path)?
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
            {
                path
            } else {
                return None;
            };
            Some(DiscoveredPack { name, path })
        })
        .collect();
    packs.sort_by_cached_key(|pack| pack.name.to_lowercase());
    packs
}

/// The folder holding `shaders/`: `folder` itself, or its only sub-folder when that holds it.
fn pack_folder(folder: PathBuf) -> Option<PathBuf> {
    if folder.join("shaders").is_dir() {
        return Some(folder);
    }
    let mut folders = std::fs::read_dir(&folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());
    let nested = folders.next()?;
    (folders.next().is_none() && nested.join("shaders").is_dir()).then_some(nested)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Returns discovered pack names in their discovery order.
    fn names(packs: &[DiscoveredPack]) -> Vec<&str> {
        packs.iter().map(|pack| pack.name.as_str()).collect()
    }

    #[test]
    fn lists_zips_and_pack_folders_sorted_ignoring_case() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        fs::write(dir.join("dusk.zip"), b"not opened while listing").unwrap();
        fs::write(dir.join("Amber.ZIP"), b"").unwrap();
        fs::write(dir.join("readme.txt"), b"").unwrap();
        fs::create_dir_all(dir.join("Cobalt/shaders/world0")).unwrap();
        fs::create_dir_all(dir.join("Bramble v2/Bramble/shaders")).unwrap();
        fs::create_dir_all(dir.join("Empty")).unwrap();
        fs::create_dir_all(dir.join("Two/a/shaders")).unwrap();
        fs::create_dir_all(dir.join("Two/b")).unwrap();
        let packs = discover_packs(dir);
        assert_eq!(
            names(&packs),
            ["Amber.ZIP", "Bramble v2", "Cobalt", "dusk.zip"]
        );
        assert_eq!(packs[0].path, dir.join("Amber.ZIP"));
        assert_eq!(packs[1].path, dir.join("Bramble v2").join("Bramble"));
        assert_eq!(packs[2].path, dir.join("Cobalt"));
    }

    #[test]
    fn missing_directory_lists_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert!(discover_packs(&root.path().join("absent")).is_empty());
        let file = root.path().join("file");
        fs::write(&file, b"").unwrap();
        assert!(discover_packs(&file).is_empty());
    }
}
