//! Owns the completed silent video until audio muxing has published its replacement.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Resolves setup before moving the video and restores it on any failed mux operation.
pub(super) fn with_staged_video(
    output: &Path,
    resolve: impl FnOnce() -> Result<PathBuf, String>,
    run: impl FnOnce(&Path, &Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let executable = resolve()?;
    let mut transaction = StagedVideo::new(output)?;
    match run(&executable, &transaction.staged, output) {
        Ok(()) => {
            transaction.armed = false;
            let _ = fs::remove_file(&transaction.staged);
            Ok(())
        }
        Err(error) => match transaction.restore() {
            Ok(()) => Err(format!(
                "{error}; the silent video is at {}",
                output.display()
            )),
            Err(restore) => Err(format!(
                "{error}; restoring {} failed: {restore}; the silent video is at {}",
                output.display(),
                transaction.staged.display()
            )),
        },
    }
}

struct StagedVideo {
    output: PathBuf,
    staged: PathBuf,
    armed: bool,
}

impl StagedVideo {
    /// Uses a distinct sibling so a prior recovered recording is never overwritten.
    fn new(output: &Path) -> Result<Self, String> {
        let staged = output.with_file_name(format!("{}.video.mp4", uuid::Uuid::new_v4()));
        fs::rename(output, &staged)
            .map_err(|error| format!("stage video {}: {error}", output.display()))?;
        Ok(Self {
            output: output.to_owned(),
            staged,
            armed: true,
        })
    }

    /// Removes only the mux destination before restoring, including on Windows.
    fn restore(&mut self) -> io::Result<()> {
        match fs::remove_file(&self.output) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(&self.staged, &self.output)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for StagedVideo {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.restore();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("cinnabar-mux-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let output = dir.join("recording.mp4");
        fs::write(&output, b"completed silent video").unwrap();
        (dir, output)
    }

    #[test]
    fn executable_lookup_failure_leaves_the_completed_video_in_place() {
        let (dir, output) = fixture();
        let error = with_staged_video(
            &output,
            || Err("lookup failed".into()),
            |_, _, _| panic!("failed setup must not start a mux"),
        )
        .unwrap_err();
        assert_eq!(error, "lookup failed");
        assert_eq!(fs::read(&output).unwrap(), b"completed silent video");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn spawn_and_failed_mux_restore_even_a_partial_destination() {
        for partial in [false, true] {
            let (dir, output) = fixture();
            let error = with_staged_video(
                &output,
                || Ok("ffmpeg".into()),
                |_, _, path| {
                    if partial {
                        fs::write(path, b"incomplete mux").unwrap();
                    }
                    Err(if partial {
                        "mux failed"
                    } else {
                        "spawn failed"
                    }
                    .into())
                },
            )
            .unwrap_err();
            assert!(error.contains(&output.display().to_string()));
            assert_eq!(fs::read(&output).unwrap(), b"completed silent video");
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn failed_restore_identifies_the_retained_recovery_file() {
        let (dir, output) = fixture();
        let mut recovery = None;
        let error = with_staged_video(
            &output,
            || Ok("ffmpeg".into()),
            |_, staged, path| {
                recovery = Some(staged.to_owned());
                fs::create_dir(path).unwrap();
                Err("mux failed".into())
            },
        )
        .unwrap_err();
        let recovery = recovery.unwrap();
        assert!(error.contains(&recovery.display().to_string()));
        assert_eq!(fs::read(&recovery).unwrap(), b"completed silent video");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn successful_mux_commits_its_replacement() {
        let (dir, output) = fixture();
        with_staged_video(
            &output,
            || Ok("ffmpeg".into()),
            |_, video, path| {
                assert_eq!(fs::read(video).unwrap(), b"completed silent video");
                fs::write(path, b"completed audiovisual recording").unwrap();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(&output).unwrap(),
            b"completed audiovisual recording"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
