//! Bounded JSON-lines protocol with the download helper; all network and disk work stays off-frame.

use super::{CHANNEL, Ready, Status, Updater, platform_key, storage};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::Arc,
};

const MAX_LINE_BYTES: u64 = 16 * 1024;

#[derive(Deserialize)]
struct Event {
    state: String,
    #[serde(default)]
    downloaded: u64,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    stage: std::path::PathBuf,
    #[serde(default)]
    latest: String,
    #[serde(default)]
    notes_url: String,
}

/// Starts at most one worker; manual retries bypass the daily successful-check stamp.
pub(super) fn start(updater: &'static Updater, force: bool) {
    let mut state = updater.state();
    if !state.enabled || state.running {
        return;
    }
    if !force && (matches!(state.status, Status::Ready(_)) || !storage::check_due(&updater.layout))
    {
        return;
    }
    let Some(url) = storage::manifest_url(&updater.layout) else {
        return;
    };
    let Some(platform) = platform_key(std::env::consts::OS, std::env::consts::ARCH) else {
        return;
    };
    state.generation = state.generation.wrapping_add(1);
    let generation = state.generation;
    state.running = true;
    state.status = Status::Checking;
    drop(state);
    std::thread::spawn(move || {
        let result = download(updater, generation, &url, &platform);
        let mut state = updater.state();
        if state.generation != generation {
            return;
        }
        state.running = false;
        state.child = None;
        state.status = match result {
            Ok(ready) => match storage::record(&updater.layout, ready.as_ref()) {
                Ok(()) => ready.map_or(Status::Idle, Status::Ready),
                Err(error) => Status::Error(format!("Could not save update state: {error}")),
            },
            Err(error) => Status::Error(format!("{error:#}")),
        };
    });
}

/// Downloads through the core, accepting readiness only after its successful exit.
fn download(
    updater: &Updater,
    generation: u64,
    url: &str,
    platform: &str,
) -> Result<Option<Ready>> {
    let root = storage::directory(&updater.layout);
    fs::create_dir_all(&root).context("create update cache")?;
    let log_path = root.join("download-error.txt");
    let log = fs::File::create(&log_path).context("create update log")?;
    let child = Arc::new(
        super::super::children::spawn(
            Command::new(&updater.layout.core_executable)
                .args([
                    "download-update",
                    "--manifest-url",
                    url,
                    "--channel",
                    CHANNEL,
                    "--platform",
                    platform,
                ])
                .args(["--current", env!("CARGO_PKG_VERSION"), "--cache-dir"])
                .arg(&root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(log),
        )
        .context("start update download")?,
    );
    {
        let mut state = updater.state();
        if generation != state.generation {
            child.kill();
            bail!("Update cancelled");
        }
        state.child = Some(Arc::clone(&child));
    }
    let result = read_events(updater, generation, &child);
    if result.is_err() {
        child.kill();
    }
    let status = child.wait().context("wait for update download")?;
    if !status.success() {
        bail!("{}", storage::diagnostic(&log_path));
    }
    result
}

/// Parses bounded progress lines and rejects incomplete or contradictory helper output.
fn read_events(
    updater: &Updater,
    generation: u64,
    child: &super::super::children::Spawned,
) -> Result<Option<Ready>> {
    let mut reader = BufReader::new(child.take_stdout().context("read update progress")?);
    let mut completed = None;
    let mut ready = None;
    loop {
        let mut line = String::new();
        let read = reader
            .by_ref()
            .take(MAX_LINE_BYTES + 1)
            .read_line(&mut line)?;
        if read == 0 {
            break;
        }
        if read as u64 > MAX_LINE_BYTES {
            bail!("Update progress exceeds size limit");
        }
        if completed.is_some() {
            bail!("Unexpected progress after update completion");
        }
        let event: Event = serde_json::from_str(&line).context("invalid update progress")?;
        match event.state.as_str() {
            "downloading" => {
                let mut state = updater.state();
                if state.generation == generation {
                    state.status = Status::Downloading {
                        downloaded: event.downloaded,
                        total: event.total,
                    };
                }
            }
            "current" => completed = Some(()),
            "ready" => {
                if event.stage.as_os_str().is_empty()
                    || !storage::is_stage(&updater.layout, &event.stage)
                    || event.latest.is_empty()
                {
                    bail!("Update helper returned an invalid stage");
                }
                ready = Some(Ready {
                    current: env!("CARGO_PKG_VERSION").into(),
                    latest: event.latest,
                    stage: event.stage,
                    notes_url: event.notes_url,
                });
                completed = Some(());
            }
            _ => bail!("Unknown update progress state"),
        }
    }
    completed.context("Update helper stopped before verification completed")?;
    Ok(ready)
}
