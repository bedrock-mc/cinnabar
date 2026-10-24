//! Copies a user pack directory into a bounded in-memory archive for ordinary validation.

use crate::MAX_LOCAL_ARCHIVE_BYTES;
use crate::{
    AdmissionError, LibraryError, MAX_DECLARED_BYTES_PER_PACK, MAX_ENTRIES_PER_PACK, MAX_FILE_BYTES,
};
use std::{
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    path::Path,
};
use zip::{ZipWriter, write::SimpleFileOptions};

/// Rejects links and non-regular files while preserving each pack's relative paths.
pub(super) fn archive(root: &Path) -> Result<Vec<u8>, LibraryError> {
    let mut writer = ZipWriter::new(BoundedArchive(Cursor::new(Vec::new())));
    let mut entries = 0usize;
    let mut total = 0u64;
    append(root, root, 0, &mut writer, &mut entries, &mut total)?;
    Ok(writer
        .finish()
        .map_err(|_| AdmissionError::MalformedZip)?
        .0
        .into_inner())
}

/// Walks only below the chosen pack, with limits on depth, entries and actual payload bytes.
fn append(
    root: &Path,
    directory: &Path,
    depth: usize,
    writer: &mut ZipWriter<BoundedArchive>,
    entries: &mut usize,
    total: &mut u64,
) -> Result<(), LibraryError> {
    if depth > 32 {
        return Err(AdmissionError::TooManyEntries.into());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        *entries += 1;
        if *entries > MAX_ENTRIES_PER_PACK {
            return Err(AdmissionError::TooManyEntries.into());
        }
        paths.push(entry?.path());
    }
    paths.sort();
    for path in paths {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(AdmissionError::InvalidFileData.into());
        }
        if metadata.is_dir() {
            append(root, &path, depth + 1, writer, entries, total)?;
            continue;
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(AdmissionError::FileTooLarge.into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| AdmissionError::InvalidFileData)?;
        let name = relative
            .to_str()
            .ok_or(AdmissionError::InvalidFileData)?
            .replace('\\', "/");
        if crate::parser::canonical_path(name.as_bytes()).is_none() {
            return Err(AdmissionError::InvalidFileData.into());
        }
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|_| AdmissionError::MalformedZip)?;
        let bytes = io::copy(&mut fs::File::open(path)?.take(MAX_FILE_BYTES + 1), writer)?;
        if bytes > MAX_FILE_BYTES {
            return Err(AdmissionError::FileTooLarge.into());
        }
        *total = total
            .checked_add(bytes)
            .filter(|total| *total <= MAX_DECLARED_BYTES_PER_PACK)
            .ok_or(AdmissionError::DeclaredSizeTooLarge)?;
    }
    Ok(())
}

/// Caps output as compression runs, including ZIP metadata written at finalization.
struct BoundedArchive(Cursor<Vec<u8>>);

impl Write for BoundedArchive {
    /// Refuses writes past the archive ceiling before the buffer can grow.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.position().saturating_add(bytes.len() as u64) > MAX_LOCAL_ARCHIVE_BYTES as u64 {
            return Err(io::Error::other(
                "resource pack exceeds the archive size limit",
            ));
        }
        self.0.write(bytes)
    }

    /// Flushes the in-memory cursor used by the ZIP writer.
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Seek for BoundedArchive {
    /// Lets the ZIP writer update headers without allocating additional payloads.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}
