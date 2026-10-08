use assets::AssetError;
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

struct StagedOutput<'a> {
    target: &'a Path,
    directory: tempfile::TempDir,
    previous: bool,
    published: bool,
}

/// Stages every output before publication and restores prior files on rename failure.
pub(super) fn write_output_bundle(outputs: &[(&Path, &[u8])]) -> Result<(), AssetError> {
    publish_bundle(outputs, |from, to| fs::rename(from, to))
}

/// Publishes staged files with a rename operation that can report filesystem failures.
fn publish_bundle(
    outputs: &[(&Path, &[u8])],
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), AssetError> {
    let mut staged = Vec::new();
    for &(target, bytes) in outputs {
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let prepare = || -> io::Result<tempfile::TempDir> {
            fs::create_dir_all(parent)?;
            let directory = tempfile::Builder::new()
                .prefix(".assetc-")
                .tempdir_in(parent)?;
            let mut file = fs::File::create(directory.path().join("new"))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok(directory)
        };
        let directory = prepare().map_err(|source| AssetError::Io {
            path: target.into(),
            source,
        })?;
        staged.push(StagedOutput {
            target,
            directory,
            previous: false,
            published: false,
        });
    }
    let result = (|| -> Result<(), AssetError> {
        for output in &mut staged {
            let old = output.directory.path().join("old");
            match fs::symlink_metadata(output.target) {
                Ok(metadata) if metadata.is_dir() => {
                    return Err(AssetError::Io {
                        path: output.target.into(),
                        source: io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "output destination is a directory",
                        ),
                    });
                }
                Ok(_) => {
                    rename(output.target, &old).map_err(|source| AssetError::Io {
                        path: output.target.into(),
                        source,
                    })?;
                    output.previous = true;
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(AssetError::Io {
                        path: output.target.into(),
                        source,
                    });
                }
            }
            rename(&output.directory.path().join("new"), output.target).map_err(|source| {
                AssetError::Io {
                    path: output.target.into(),
                    source,
                }
            })?;
            output.published = true;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut recovery_failure = None;
        for output in staged.into_iter().rev() {
            let restored = if output.previous {
                rename(&output.directory.path().join("old"), output.target)
            } else if output.published {
                fs::remove_file(output.target)
            } else {
                Ok(())
            };
            if let Err(source) = restored {
                let recovery = output.directory.keep();
                recovery_failure = Some(AssetError::Io {
                    path: output.target.into(),
                    source: io::Error::new(
                        source.kind(),
                        format!(
                            "{error}; rollback failed: {source}; recovery files: {}",
                            recovery.display()
                        ),
                    ),
                });
            }
        }
        return Err(recovery_failure.unwrap_or(error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_rename_failure_restores_every_previous_output() {
        let root = tempfile::tempdir().unwrap();
        let carrier = root.path().join("carrier");
        let report = root.path().join("report");
        fs::write(&carrier, b"old carrier").unwrap();
        fs::write(&report, b"old report").unwrap();
        let result = publish_bundle(
            &[(&carrier, b"new carrier"), (&report, b"new report")],
            |from, to| {
                if to == report && from.file_name().unwrap() == "new" {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected rename failure",
                    ))
                } else {
                    fs::rename(from, to)
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(carrier).unwrap(), b"old carrier");
        assert_eq!(fs::read(report).unwrap(), b"old report");
    }

    #[test]
    fn review_unwritable_report_preserves_the_existing_carrier() {
        let root = tempfile::tempdir().unwrap();
        let carrier = root.path().join("carrier");
        let parent = root.path().join("blocked");
        fs::write(&carrier, b"old").unwrap();
        fs::write(&parent, b"not a directory").unwrap();
        let report = parent.join("report");
        assert!(write_output_bundle(&[(&carrier, b"new"), (&report, b"report")]).is_err());
        assert_eq!(fs::read(carrier).unwrap(), b"old");
    }
}
