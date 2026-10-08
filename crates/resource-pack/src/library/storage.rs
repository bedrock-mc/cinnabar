//! Retires immutable archives only after their replacement has been acknowledged.
use super::{CATALOG_FILE, Catalog, LibraryError, atomic_write, encode_catalog};
use std::{fs, path::Path};

/// Removes obsolete revisions after the newest preview has been acknowledged.
pub(super) fn prune(root: &Path, catalog: &mut Catalog) -> Result<(), LibraryError> {
    let mut candidate = catalog.clone();
    let mut failure = None;
    candidate.retained.retain(|pack| {
        if candidate
            .active
            .iter()
            .any(|active| active.id == pack.id && active.revision == pack.revision)
        {
            return true;
        }
        match fs::remove_file(root.join(pack.filename())) {
            Ok(()) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                failure.get_or_insert(error);
                true
            }
        }
    });
    atomic_write(&root.join(CATALOG_FILE), &encode_catalog(&candidate)?)?;
    *catalog = candidate;
    match failure {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}
