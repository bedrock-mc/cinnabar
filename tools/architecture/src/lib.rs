use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod completion_plan;
mod dependencies;
mod markers;
mod paths;
mod policy;
mod sources;

use dependencies::check_dependencies;
use markers::check_markers;
use paths::{ignored_directory, is_vendored, relative_slash};
use policy::Policy;
use sources::{check_artifacts, check_sources};

#[derive(Debug, thiserror::Error)]
pub enum ArchitectureError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Policy {
        path: PathBuf,
        source: toml::de::Error,
    },
}

pub fn check_repository(root: &Path, policy_path: &Path) -> Result<Vec<String>, ArchitectureError> {
    let policy_text = read(policy_path)?;
    let policy =
        toml::from_str::<Policy>(&policy_text).map_err(|source| ArchitectureError::Policy {
            path: policy_path.to_path_buf(),
            source,
        })?;
    let mut files = Vec::new();
    collect_files(root, root, &policy, &mut files)?;
    files.sort();

    let mut diagnostics = Vec::new();
    check_vendor_records(root, &policy, &mut diagnostics);
    check_artifacts(root, &policy, &files, &mut diagnostics);
    check_sources(root, &policy, &files, &mut diagnostics)?;
    check_dependencies(root, &policy, &mut diagnostics)?;
    check_markers(root, &policy, &files, &mut diagnostics)?;
    diagnostics.sort();
    diagnostics.dedup();
    Ok(diagnostics)
}

fn check_vendor_records(root: &Path, policy: &Policy, diagnostics: &mut Vec<String>) {
    for rule in &policy.vendored {
        let record = root.join(&rule.ownership_record);
        if !record.is_file() {
            diagnostics.push(format!(
                "vendored path `{}` has missing ownership record `{}`",
                rule.path, rule.ownership_record
            ));
        }
    }
}

pub(crate) fn read(path: &Path) -> Result<String, ArchitectureError> {
    fs::read_to_string(path).map_err(|source| ArchitectureError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    policy: &Policy,
    files: &mut Vec<PathBuf>,
) -> Result<(), ArchitectureError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| ArchitectureError::Read {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| ArchitectureError::Read {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let relative = relative_slash(root, &path);
        if entry
            .file_type()
            .map_err(|source| ArchitectureError::Read {
                path: path.clone(),
                source,
            })?
            .is_dir()
        {
            if ignored_directory(&relative) || is_vendored(&relative, policy) {
                continue;
            }
            collect_files(root, &path, policy, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}
