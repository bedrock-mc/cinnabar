use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use thiserror::Error;

mod cli;
mod copy;
mod hashing;
mod layout;
pub use cli::parse_args;
use copy::{copy_validated, executable_destination};
use hashing::hash_file;
#[cfg(test)]
use layout::ASSET_FILES;
use layout::input_files;

pub(crate) const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    pub platform: Platform,
    pub client: PathBuf,
    pub core: PathBuf,
    pub assets: PathBuf,
    pub physics: PathBuf,
    pub notices: PathBuf,
    pub target_triple: String,
    pub git_commit: String,
    pub output: PathBuf,
}

#[derive(Debug, Error)]
pub enum DistError {
    #[error(
        "{0}\nusage: dist-local --platform <windows|linux|macos> --client <FILE> --core <FILE> --assets <DIR> --physics <FILE> --notices <FILE> --target <TRIPLE> --git-commit <40_HEX> --out <NEW_DIR>"
    )]
    Usage(String),
    #[error("unsafe input path `{0}`")]
    UnsafePath(PathBuf),
    #[error("input is not a regular, non-symlink file: `{0}`")]
    NotRegular(PathBuf),
    #[error("input changed while it was being opened: `{0}`")]
    InputChanged(PathBuf),
    #[error("input path may contain secret material: `{0}`")]
    SecretPath(PathBuf),
    #[error("input `{path}` exceeds the {limit}-byte bound")]
    FileTooLarge { path: PathBuf, limit: u64 },
    #[error("staged payload exceeds the {0}-byte total bound")]
    TotalTooLarge(u64),
    #[error("output already exists: `{0}`")]
    OutputExists(PathBuf),
    #[error("staging destination collision: `{0}`")]
    Collision(String),
    #[error("I/O failure for `{path}`: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("encode deterministic manifest: {0}")]
    Manifest(#[from] serde_json::Error),
}

#[derive(Serialize)]
struct Manifest {
    distribution_scope: &'static str,
    platform: Platform,
    target_triple: String,
    git_commit: String,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    path: String,
    bytes: u64,
    sha256: String,
}

pub fn run_from<I, S>(arguments: I) -> Result<(), DistError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    stage(&parse_args(arguments)?)
}

pub fn stage(options: &Options) -> Result<(), DistError> {
    validate_output(&options.output)?;
    validate_directory(&options.assets)?;
    reject_output_inside(&options.output, &options.assets)?;
    let files = input_files(options);
    let mut destinations = BTreeSet::new();
    let mut total = 0_u64;
    for (source, destination) in &files {
        validate_input(source)?;
        let bytes = metadata(source)?.len();
        if bytes > MAX_FILE_BYTES {
            return Err(DistError::FileTooLarge {
                path: source.clone(),
                limit: MAX_FILE_BYTES,
            });
        }
        total = total
            .checked_add(bytes)
            .ok_or(DistError::TotalTooLarge(MAX_TOTAL_BYTES))?;
        if total > MAX_TOTAL_BYTES {
            return Err(DistError::TotalTooLarge(MAX_TOTAL_BYTES));
        }
        if !destinations.insert(destination.clone()) {
            return Err(DistError::Collision(destination.clone()));
        }
    }

    let temporary = options
        .output
        .with_extension(format!("dist-tmp-{}", std::process::id()));
    if temporary.exists() {
        return Err(DistError::OutputExists(temporary));
    }
    fs::create_dir_all(&temporary).map_err(|source| io_error(&temporary, source))?;
    let result = stage_into(options, &temporary, files);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    if let Err(source) = fs::rename(&temporary, &options.output) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(io_error(&options.output, source));
    }
    Ok(())
}

fn stage_into(
    options: &Options,
    root: &Path,
    mut files: Vec<(PathBuf, String)>,
) -> Result<(), DistError> {
    files.sort_by(|left, right| left.1.cmp(&right.1));
    let mut manifest_files = Vec::with_capacity(files.len());
    let mut total_bytes = 0_u64;
    for (source, relative) in files {
        let destination = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        let parent = destination
            .parent()
            .ok_or_else(|| DistError::UnsafePath(destination.clone()))?;
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
        copy_validated(
            &source,
            &destination,
            executable_destination(options.platform, &relative),
            &mut total_bytes,
        )?;
        let (bytes, sha256) = hash_file(&destination)?;
        manifest_files.push(ManifestFile {
            path: relative,
            bytes,
            sha256,
        });
    }
    let manifest = Manifest {
        distribution_scope: "local-development-only",
        platform: options.platform,
        target_triple: options.target_triple.clone(),
        git_commit: options.git_commit.clone(),
        files: manifest_files,
    };
    let mut encoded = serde_json::to_vec_pretty(&manifest)?;
    encoded.push(b'\n');
    let path = root.join("bundle-manifest.json");
    fs::write(&path, encoded).map_err(|source| io_error(&path, source))
}

fn validate_output(path: &Path) -> Result<(), DistError> {
    validate_lexical_path(path)?;
    if path.exists() {
        return Err(DistError::OutputExists(path.to_owned()));
    }
    if let Some(parent) = path.parent() {
        reject_existing_symlinks(parent)?;
    }
    Ok(())
}

pub(crate) fn validate_input(path: &Path) -> Result<(), DistError> {
    validate_lexical_path(path)?;
    if path
        .components()
        .any(|component| secret_component(component.as_os_str()))
    {
        return Err(DistError::SecretPath(path.to_owned()));
    }
    reject_existing_symlinks(path)?;
    let meta = metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(DistError::NotRegular(path.to_owned()));
    }
    Ok(())
}

fn validate_directory(path: &Path) -> Result<(), DistError> {
    validate_lexical_path(path)?;
    reject_existing_symlinks(path)?;
    if !metadata(path)?.is_dir() {
        return Err(DistError::NotRegular(path.to_owned()));
    }
    Ok(())
}

fn reject_output_inside(output: &Path, input: &Path) -> Result<(), DistError> {
    let input = fs::canonicalize(input).map_err(|source| io_error(input, source))?;
    let absolute = if output.is_absolute() {
        output.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| io_error(Path::new("."), source))?
            .join(output)
    };
    let ancestor = absolute
        .ancestors()
        .find(|candidate| candidate.exists())
        .ok_or_else(|| DistError::UnsafePath(absolute.clone()))?;
    let suffix = absolute
        .strip_prefix(ancestor)
        .map_err(|_| DistError::UnsafePath(absolute.clone()))?;
    let output = fs::canonicalize(ancestor)
        .map_err(|source| io_error(ancestor, source))?
        .join(suffix);
    if output.starts_with(input) {
        return Err(DistError::UnsafePath(output));
    }
    Ok(())
}

fn validate_lexical_path(path: &Path) -> Result<(), DistError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(DistError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn reject_existing_symlinks(path: &Path) -> Result<(), DistError> {
    let mut candidate = PathBuf::new();
    for component in path.components() {
        candidate.push(component.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(DistError::NotRegular(path.to_owned()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error(&candidate, source)),
        }
    }
    Ok(())
}

fn secret_component(component: &OsStr) -> bool {
    let lower = component.to_string_lossy().to_ascii_lowercase();
    lower == "auth"
        || ["credential", "token", "secret", "private-key", "cookie"]
            .iter()
            .any(|marker| lower.contains(marker))
}

fn metadata(path: &Path) -> Result<fs::Metadata, DistError> {
    fs::symlink_metadata(path).map_err(|source| io_error(path, source))
}

pub(crate) fn io_error(path: &Path, source: io::Error) -> DistError {
    DistError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests;
