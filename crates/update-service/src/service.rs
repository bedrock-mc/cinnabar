//! The update flow's I/O: checks, the explicit install and the helper handoff.

use crate::{
    ManualPackage, UpdateState, download, handoff, preferences, releases,
    state::{CheckError, Machine, Offer, UpdateStage},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use time::OffsetDateTime;
use update_installer::{runtime, stage, target};
use update_manifest::{TrustedKeys, build::Build, channel::Channel, select::Check};

/// The pending install the helper should complete before the next launcher start.
const HANDOFF_FILE: &str = "handoff.json";

/// Where this installation lives and what it is running.
pub struct Config {
    /// Per-user update data: caches, stages, preferences and the runtime gate.
    pub update_dir: PathBuf,
    /// Version and publication number of the running release unit.
    pub current: Build,
    /// The bundle, MSI directory or AppImage the helper replaces; none for development builds.
    pub target: Option<PathBuf>,
    /// The packaged offline helper beside the launcher.
    pub helper: PathBuf,
}

/// What the caller does after an accepted install.
pub enum Handoff {
    /// The copied helper installs once this process exits; quit now without starting the game.
    Exit,
    /// Open this verified package with the platform installer, then call [`ManualPackage::opened`].
    Package(ManualPackage),
}

#[derive(Deserialize, Serialize)]
struct Pending {
    version: String,
    build: u64,
}

/// One update flow per launcher; `listener` sees every state change.
pub struct Service {
    config: Config,
    keys: TrustedKeys,
    platform: Option<String>,
    channel: Mutex<Channel>,
    machine: Mutex<Machine>,
    listener: Box<dyn Fn(&UpdateState) + Send + Sync>,
}

impl Service {
    /// Loads the saved channel, migrating `legacy` once, and reports a handoff that did not finish.
    pub fn new(
        config: Config,
        legacy: impl FnOnce() -> Option<Channel>,
        listener: impl Fn(&UpdateState) + Send + Sync + 'static,
    ) -> Self {
        let channel = preferences::load(&config.update_dir, legacy);
        let initial = unfinished_handoff(&config.update_dir, &config.current)
            .unwrap_or(UpdateState::Idle { last_checked: None });
        Self {
            keys: update_manifest::parse_keys(update_installer::TRUSTED_KEYS).unwrap_or_default(),
            platform: update_manifest::platform_key(std::env::consts::OS, std::env::consts::ARCH),
            config,
            channel: Mutex::new(channel),
            machine: Mutex::new(Machine::new(initial)),
            listener: Box::new(listener),
        }
    }

    pub fn state(&self) -> UpdateState {
        self.machine().state().clone()
    }

    /// The desired channel, which may differ from the installed payload's channel.
    pub fn channel(&self) -> Channel {
        *self
            .channel
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Saves `channel` and retires work for the old one; refused while installing.
    pub fn set_channel(&self, channel: Channel) -> Result<()> {
        ensure!(
            channel.is_published(),
            "{} has no releases yet",
            channel.label()
        );
        if self.channel() == channel {
            return Ok(());
        }
        ensure!(
            self.mutate(Machine::change_channel),
            "wait for the update to finish installing"
        );
        preferences::save(&self.config.update_dir, channel)?;
        *self
            .channel
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = channel;
        Ok(())
    }

    /// Shows `state` without an offer behind it, for development screenshots only.
    #[cfg(debug_assertions)]
    pub fn show_fixture(&self, state: UpdateState) {
        self.mutate(|machine| {
            machine.show(state);
            true
        });
    }

    /// Marks the next offer as required after a server refused this build as outdated.
    pub fn require(&self) {
        self.mutate(|machine| {
            machine.require();
            true
        });
    }

    /// Checks the selected channel's signed manifests; blocks on the network.
    pub fn check(&self, now: OffsetDateTime) {
        let Some(generation) = self.begin(Machine::begin_check) else {
            return;
        };
        let channel = self.channel();
        let outcome = self.find_offer(channel, now).map_err(|error| {
            tracing::debug!("update check: {error:#}");
            match error.downcast_ref::<Unavailable>() {
                Some(reason) => CheckError::Unavailable(reason.to_string()),
                None => CheckError::Failed(user_message(&error)),
            }
        });
        let checked = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        self.mutate(|machine| machine.finish_check(generation, outcome, &checked));
    }

    /// Downloads, verifies and hands off the offered release after the player's click; blocks.
    /// Refuses while any installed game, core or crash monitor runs.
    pub fn install(&self, clock: impl Fn() -> OffsetDateTime) -> Result<Handoff> {
        let Some((generation, offer)) = self.begin(Machine::begin_install) else {
            bail!("no update is ready to install");
        };
        let result = self.run_install(generation, &offer, &clock);
        match &result {
            Ok(Handoff::Package(_)) => {
                self.mutate(|machine| machine.handed_off(generation));
            }
            Ok(Handoff::Exit) => {}
            Err(error) => {
                tracing::warn!("update install: {error:#}");
                let message = user_message(error);
                self.mutate(|machine| machine.fail(generation, message, true));
            }
        }
        result
    }

    fn find_offer(&self, channel: Channel, now: OffsetDateTime) -> Result<Option<Offer>> {
        let (platform, _) = self.prerequisites(channel)?;
        let check = self.check_for(channel, &platform, false);
        let root = &self.config.update_dir;
        std::fs::create_dir_all(root)?;
        let client = download::http_client()?;
        let fetched = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(releases::select(
                root,
                &self.keys,
                check,
                false,
                now,
                async |channel| fetch_channel(&client, channel).await,
            ));
        // Offline, a still-valid authenticated cache keeps the last offer visible.
        let selected = match fetched {
            Ok(selected) => selected,
            Err(error) => releases::cached(root, &self.keys, check, now).ok_or(error)?,
        };
        let manifest = &selected.manifest;
        let current = &self.config.current;
        let newer = update_manifest::select::newer_build(
            &manifest.version,
            manifest.build_number,
            &current.version,
            current.number,
        )?;
        let older = update_manifest::select::newer_build(
            &current.version,
            current.number,
            &manifest.version,
            manifest.build_number,
        )?;
        Ok((newer || older).then(|| Offer {
            version: manifest.version.clone(),
            build: manifest.build_number,
            channel: Channel::parse(&manifest.channel).unwrap_or(channel),
            notes_url: (!manifest.notes_url.is_empty()).then(|| manifest.notes_url.clone()),
            size: selected.artifact.size.max(0) as u64,
            downgrade: older,
        }))
    }

    fn run_install(
        &self,
        generation: u64,
        offer: &Offer,
        clock: &impl Fn() -> OffsetDateTime,
    ) -> Result<Handoff> {
        let root = &self.config.update_dir;
        ensure!(!runtime::is_live(root), Live);
        let channel = self.channel();
        let (platform, _) = self.prerequisites(channel)?;
        // A downgrade is an explicit switch; the helper never installs one automatically.
        let check = self.check_for(channel, &platform, offer.downgrade);
        std::fs::create_dir_all(root)?;
        update_installer::helper::prune(root, std::time::SystemTime::now());
        let lease =
            update_installer::lease::Lease::acquire(root).context("lock the update directory")?;
        let destination = root.join("ready");
        stage::recover(&destination, &self.keys, check, clock())?;
        let client = download::http_client()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut prefer_cached = true;
        download::retry_missing(
            || {
                let selected = runtime.block_on(releases::select(
                    root,
                    &self.keys,
                    check,
                    std::mem::take(&mut prefer_cached),
                    clock(),
                    async |channel| fetch_channel(&client, channel).await,
                ))?;
                ensure!(
                    selected.manifest.version == offer.version
                        && selected.manifest.build_number == offer.build,
                    "the channel published another release; check again"
                );
                if stage::refresh_envelope(&destination, &lease, &selected)? {
                    return Ok(());
                }
                let mut reported = 0;
                runtime.block_on(download::artifact(
                    &client,
                    root,
                    &destination,
                    selected,
                    |received, total| {
                        // Report about 200 steps, not every network chunk.
                        if received == total || received - reported.min(received) >= total / 200 {
                            reported = received;
                            self.mutate(|machine| {
                                machine.progress(generation, received, Some(total))
                            })
                        } else {
                            self.machine().active(generation)
                        }
                    },
                ))?;
                Ok(())
            },
            || self.machine().active(generation),
        )?;
        ensure!(
            self.mutate(|machine| machine.verifying(generation)),
            Cancelled
        );
        let selected = stage::verify(&destination, &self.keys, check, clock())?;
        let automatic = !offer.downgrade
            && self.config.target.as_deref().is_some_and(|target| {
                target::access(target).is_ok_and(|access| access == target::Access::Writable)
            });
        if !automatic {
            drop(lease);
            let name = update_manifest::platforms::artifact_name(
                std::env::consts::OS,
                std::env::consts::ARCH,
            )
            .context("unknown native installer")?;
            let package = handoff::snapshot_package(&destination, name, &self.keys, check, clock)?;
            return Ok(Handoff::Package(package));
        }
        let target = self
            .config
            .target
            .as_deref()
            .context("not an installed build")?;
        let (stage, reader) = stage::retain(&destination, &lease, &selected)?;
        drop(lease);
        ensure!(!runtime::is_live(root), Live);
        ensure!(
            self.mutate(|machine| machine.installing(generation)),
            Cancelled
        );
        write_pending(root, offer)?;
        reader.set_modified(std::time::SystemTime::now())?;
        update_installer::helper::start(
            root,
            &self.config.helper,
            &[
                stage.into_os_string(),
                target.as_os_str().to_owned(),
                std::process::id().to_string().into(),
                OsString::from(selected.manifest.channel.as_str()),
            ],
        )
        .inspect_err(|_| {
            let _ = std::fs::remove_file(root.join(HANDOFF_FILE));
        })?;
        // The helper's own reader overlaps this one until this process exits.
        std::mem::forget(reader);
        Ok(Handoff::Exit)
    }

    /// The platform key and the channel's manifest URL, or why this build cannot update.
    fn prerequisites(&self, channel: Channel) -> Result<(String, String)> {
        ensure!(
            !self.keys.is_empty(),
            Unavailable("This build has no update keys.")
        );
        let platform = self
            .platform
            .clone()
            .ok_or(Unavailable("Updates aren't available on this platform."))?;
        let url = channel
            .manifest_url()
            .ok_or(Unavailable("This channel has no releases yet."))?;
        Ok((platform, url))
    }

    fn check_for<'a>(&'a self, channel: Channel, platform: &'a str, switch_now: bool) -> Check<'a> {
        Check {
            channel: channel.name(),
            platform,
            current: &self.config.current.version,
            current_build: self.config.current.number,
            switch_now,
        }
    }

    fn begin<T>(&self, start: impl FnOnce(&mut Machine) -> Option<T>) -> Option<T> {
        let (result, state) = {
            let mut machine = self.machine();
            let result = start(&mut machine)?;
            (result, machine.state().clone())
        };
        (self.listener)(&state);
        Some(result)
    }

    /// Applies one transition and notifies the listener when it took effect.
    fn mutate(&self, change: impl FnOnce(&mut Machine) -> bool) -> bool {
        let (changed, state) = {
            let mut machine = self.machine();
            let changed = change(&mut machine);
            (changed, machine.state().clone())
        };
        if changed {
            (self.listener)(&state);
        }
        changed
    }

    fn machine(&self) -> MutexGuard<'_, Machine> {
        self.machine
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// An installed game process still runs from the installation.
#[derive(Debug, thiserror::Error)]
#[error("Close Cinnabar before updating.")]
struct Live;

/// A channel change retired this install.
#[derive(Debug, thiserror::Error)]
#[error("The update was cancelled.")]
struct Cancelled;

/// Updates cannot work in this build, so retrying cannot help.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Unavailable(&'static str);

/// Short player-facing text; the full chain goes to the log.
fn user_message(error: &anyhow::Error) -> String {
    if let Some(live) = error.downcast_ref::<Live>() {
        return live.to_string();
    }
    if let Some(unavailable) = error.downcast_ref::<Unavailable>() {
        return unavailable.to_string();
    }
    if let Some(cancelled) = error.downcast_ref::<Cancelled>() {
        return cancelled.to_string();
    }
    format!("Couldn't update: {}", error.root_cause())
}

async fn fetch_channel(client: &reqwest::Client, channel: Channel) -> Result<Vec<u8>> {
    download::fetch(
        client,
        &channel
            .manifest_url()
            .context("unpublished update channel")?,
    )
    .await
}

fn write_pending(root: &Path, offer: &Offer) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(
        &mut file,
        &Pending {
            version: offer.version.clone(),
            build: offer.build,
        },
    )?;
    file.persist(root.join(HANDOFF_FILE))?;
    Ok(())
}

/// Consumes the last handoff record; a mismatch with `current` means the helper did not install.
fn unfinished_handoff(root: &Path, current: &Build) -> Option<UpdateState> {
    let path = root.join(HANDOFF_FILE);
    let pending: Option<Pending> = serde_json::from_slice(&std::fs::read(&path).ok()?).ok();
    let _ = std::fs::remove_file(&path);
    let pending = pending?;
    (pending.version != current.version || pending.build != current.number).then(|| {
        UpdateState::Failed {
            message: format!(
                "Cinnabar {} wasn't installed. Your current version still works.",
                pending.version
            ),
            stage: UpdateStage::Install,
            retryable: true,
            can_play_installed: true,
        }
    })
}

#[cfg(test)]
mod tests;
