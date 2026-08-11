use super::DistError;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use {
    super::io_error,
    std::{fs, os::unix::fs::MetadataExt, path::Component},
};

#[cfg(not(unix))]
pub(super) fn canonicalize_top_level_alias(path: &Path) -> Result<PathBuf, DistError> {
    Ok(path.to_owned())
}

#[cfg(unix)]
pub(super) fn canonicalize_top_level_alias(path: &Path) -> Result<PathBuf, DistError> {
    canonicalize_top_level_alias_with(path, |alias| {
        let metadata = fs::symlink_metadata(alias).map_err(|source| io_error(alias, source))?;
        if !metadata.file_type().is_symlink() {
            return Ok(None);
        }
        if metadata.uid() != 0 {
            return Err(DistError::NotRegular(path.to_owned()));
        }
        fs::read_link(alias)
            .map(Some)
            .map_err(|source| io_error(alias, source))
    })
}

// Resolve only an operating-system-owned first Unix path component. In
// particular, macOS commonly returns temporary paths below `/var` while that
// component is a root-owned alias. The resolved target is still traversed one
// component at a time, and links anywhere below this boundary remain rejected.
#[cfg(unix)]
fn canonicalize_top_level_alias_with<F>(path: &Path, inspect: F) -> Result<PathBuf, DistError>
where
    F: FnOnce(&Path) -> Result<Option<PathBuf>, DistError>,
{
    if !path.is_absolute() || path == Path::new(std::path::MAIN_SEPARATOR_STR) {
        return Ok(path.to_owned());
    }
    let Some(Component::Normal(component)) = path.components().nth(1) else {
        return Ok(path.to_owned());
    };
    let alias = Path::new(std::path::MAIN_SEPARATOR_STR).join(component);
    let Some(target) = inspect(&alias)? else {
        return Ok(path.to_owned());
    };
    let target = if target.is_absolute() {
        target
    } else {
        alias
            .parent()
            .ok_or_else(|| DistError::UnsafePath(alias.clone()))?
            .join(target)
    };
    if target
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(DistError::UnsafePath(target));
    }
    let remainder = path
        .strip_prefix(&alias)
        .map_err(|_| DistError::UnsafePath(path.to_owned()))?;
    Ok(target.join(remainder))
}

#[cfg(all(test, unix))]
mod tests {
    use super::canonicalize_top_level_alias_with;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_one_trusted_top_level_unix_alias() {
        let raw = Path::new("/trusted-alias/cache/objects");
        let canonical = canonicalize_top_level_alias_with(raw, |alias| {
            assert_eq!(alias, Path::new("/trusted-alias"));
            Ok(Some(PathBuf::from("private/var")))
        })
        .unwrap();
        assert_eq!(canonical, Path::new("/private/var/cache/objects"));
    }
}
