//! Byte limits for externally supplied startup and acceptance files.

use std::{
    io::{self, Read},
    path::Path,
};

/// Reads a file only when its contents fit within `limit` bytes.
pub fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds byte limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_bounded_file_rejects_excess_and_accepts_exact_limit() {
        let path = std::env::temp_dir().join(format!("cinnabar-bounded-{}", std::process::id()));
        std::fs::write(&path, [0; 9]).unwrap();
        let oversized = read(&path, 8);
        let exact = read(&path, 9).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(exact.len(), 9);
        assert_eq!(oversized.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
