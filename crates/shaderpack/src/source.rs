//! Where a pack's files come from: an unpacked folder or a zip archive.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Why a pack could not be opened.
#[derive(Debug)]
pub enum PackError {
    Io {
        path: PathBuf,
        error: io::Error,
    },
    Zip {
        path: PathBuf,
        error: zip::result::ZipError,
    },
    /// Neither the path nor its top-level folder holds a `shaders/` directory.
    NotAShaderPack(PathBuf),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            PackError::Zip { path, error } => write!(f, "{}: {error}", path.display()),
            PackError::NotAShaderPack(path) => {
                write!(f, "{}: no shaders/ folder found", path.display())
            }
        }
    }
}

impl std::error::Error for PackError {}

/// Where the pack was loaded from, for reading binary assets (custom textures) later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackOrigin {
    /// The pack's `shaders/` directory.
    Directory(PathBuf),
    /// The archive path and the entry prefix of its `shaders/` folder (`""` or `"<top>/"`
    /// followed by `shaders/`).
    Zip { archive: PathBuf, prefix: String },
    /// Files supplied in memory (tests and tools).
    Memory,
}

/// Extensions never read as text: images, audio, fonts and archives.
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "tga", "bmp", "gif", "dat", "bin", "raw", "ktx", "dds", "hdr", "exr",
    "ogg", "wav", "mp3", "ttf", "otf", "zip", "jar", "class", "psd", "webp",
];

/// The text files under a pack's `shaders/` folder, keyed by `shaders/`-relative path with
/// forward slashes. Loaded eagerly so translation is pure and can run on many threads.
#[derive(Clone, Debug)]
pub struct PackSource {
    origin: PackOrigin,
    files: BTreeMap<String, String>,
}

impl PackSource {
    /// Opens a pack folder (containing `shaders/`, or the `shaders/` folder itself) or a zip
    /// whose root or single top-level folder contains `shaders/`.
    pub fn open(path: &Path) -> Result<PackSource, PackError> {
        if path.is_dir() {
            Self::open_dir(path)
        } else {
            Self::open_zip(path)
        }
    }

    /// Builds a source from in-memory files (`shaders/`-relative path → text).
    pub fn from_files<I, K, V>(files: I) -> PackSource
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        PackSource {
            origin: PackOrigin::Memory,
            files: files
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    fn open_dir(path: &Path) -> Result<PackSource, PackError> {
        let root = if path.join("shaders").is_dir() {
            path.join("shaders")
        } else if path.file_name().is_some_and(|n| n == "shaders") {
            path.to_path_buf()
        } else {
            return Err(PackError::NotAShaderPack(path.to_path_buf()));
        };
        let mut files = BTreeMap::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir).map_err(|error| PackError::Io {
                path: dir.clone(),
                error,
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| PackError::Io {
                    path: dir.clone(),
                    error,
                })?;
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let Ok(rel) = p.strip_prefix(&root) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if is_binary_name(&rel) {
                    continue;
                }
                let bytes = std::fs::read(&p).map_err(|error| PackError::Io {
                    path: p.clone(),
                    error,
                })?;
                if let Some(text) = decode_text(&bytes) {
                    files.insert(rel, text);
                }
            }
        }
        Ok(PackSource {
            origin: PackOrigin::Directory(root),
            files,
        })
    }

    fn open_zip(path: &Path) -> Result<PackSource, PackError> {
        let zip_err = |error| PackError::Zip {
            path: path.to_path_buf(),
            error,
        };
        let file = std::fs::File::open(path).map_err(|error| PackError::Io {
            path: path.to_path_buf(),
            error,
        })?;
        let mut archive = zip::ZipArchive::new(io::BufReader::new(file)).map_err(zip_err)?;
        let prefix = shaders_prefix(archive.file_names())
            .ok_or_else(|| PackError::NotAShaderPack(path.to_path_buf()))?;
        let mut files = BTreeMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(zip_err)?;
            if entry.is_dir() {
                continue;
            }
            let Some(rel) = entry.name().strip_prefix(prefix.as_str()) else {
                continue;
            };
            let rel = rel.to_string();
            if is_binary_name(&rel) || rel.split('/').any(|seg| seg == "..") {
                continue;
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| PackError::Io {
                    path: path.join(&rel),
                    error,
                })?;
            if let Some(text) = decode_text(&bytes) {
                files.insert(rel, text);
            }
        }
        Ok(PackSource {
            origin: PackOrigin::Zip {
                archive: path.to_path_buf(),
                prefix,
            },
            files,
        })
    }

    /// Where the pack came from.
    pub fn origin(&self) -> &PackOrigin {
        &self.origin
    }

    /// Text of a `shaders/`-relative file.
    pub fn text(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Every loaded text file, sorted by path.
    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Reads any `shaders/`-relative file as bytes, including binary assets.
    pub fn read_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        match &self.origin {
            PackOrigin::Directory(root) => std::fs::read(root.join(path)),
            PackOrigin::Zip { archive, prefix } => {
                let file = std::fs::File::open(archive)?;
                let mut zip =
                    zip::ZipArchive::new(io::BufReader::new(file)).map_err(io::Error::other)?;
                let mut entry = zip
                    .by_name(&format!("{prefix}{path}"))
                    .map_err(io::Error::other)?;
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                Ok(bytes)
            }
            PackOrigin::Memory => self
                .files
                .get(path)
                .map(|t| t.as_bytes().to_vec())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string())),
        }
    }
}

/// The entry prefix ending in `shaders/` at the archive root or under one top-level folder.
fn shaders_prefix<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<String> = None;
    for name in names {
        let parts: Vec<&str> = name.split('/').collect();
        let prefix = if parts.len() > 1 && parts[0] == "shaders" {
            "shaders/".to_string()
        } else if parts.len() > 2 && parts[1] == "shaders" {
            format!("{}/shaders/", parts[0])
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|b| prefix.len() < b.len()) {
            best = Some(prefix);
        }
    }
    best
}

fn is_binary_name(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| BINARY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn decode_text(bytes: &[u8]) -> Option<String> {
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return None;
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_prefix_accepts_root_or_one_top_folder() {
        assert_eq!(
            shaders_prefix(["shaders/a.fsh", "x.txt"].into_iter()).as_deref(),
            Some("shaders/")
        );
        assert_eq!(
            shaders_prefix(["Pack v1/shaders/world0/a.fsh"].into_iter()).as_deref(),
            Some("Pack v1/shaders/")
        );
        assert_eq!(shaders_prefix(["a/b/shaders/x.fsh"].into_iter()), None);
    }
}
