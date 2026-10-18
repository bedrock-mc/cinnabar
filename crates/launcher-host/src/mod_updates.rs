//! Optional GitHub release checks and automatic downloads, kept off the frame thread.

mod download;
mod progress;
#[cfg(test)]
mod tests;
mod worker;
pub use download::Downloaded;

use crossbeam_channel::{Receiver, Sender, unbounded};
use launcher::mods::{AvailableUpdate, InstalledMod};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    id: u64,
    repository: String,
    version: String,
    asset: Option<String>,
}

struct Request {
    manual: Option<u64>,
    generation: u64,
    permission: u64,
    candidates: Vec<Candidate>,
}

struct Response {
    manual: Option<u64>,
    message: Option<String>,
    generation: u64,
    updates: Vec<AvailableUpdate>,
    downloads: Vec<Downloaded>,
}

/// One session's release checks and publication fence; failures never block mod management.
pub struct Checker {
    requests: Sender<Request>,
    responses: Receiver<Response>,
    generation: Arc<AtomicU64>,
    candidates: Vec<Candidate>,
    permission: Arc<AtomicU64>,
    automatic: bool,
    enabled: bool,
    sources: HashMap<u64, String>,
    progress: progress::Progress,
    pending: Option<u64>,
    message: Option<String>,
    updates: Vec<AvailableUpdate>,
    downloads: Vec<Downloaded>,
}

impl Checker {
    /// Starts one worker with a per-repository session cache and no account credentials.
    pub fn start(max_component_bytes: usize) -> std::io::Result<Self> {
        let (requests, incoming) = unbounded();
        let (outgoing, responses) = unbounded();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let permission = Arc::new(AtomicU64::new(0));
        let authorization = permission.clone();
        let progress = progress::Progress::default();
        let transfers = progress.clone();
        std::thread::Builder::new()
            .name("mod-updates".into())
            .spawn(move || {
                let result = worker::network(
                    incoming,
                    outgoing,
                    current,
                    authorization,
                    max_component_bytes,
                    transfers,
                );
                if let Err(error) = result {
                    tracing::debug!("Mod update checks unavailable: {error:#}");
                }
            })?;
        Ok(Self {
            requests,
            responses,
            generation,
            candidates: Vec::new(),
            permission,
            automatic: false,
            enabled: false,
            sources: HashMap::new(),
            progress,
            pending: None,
            message: None,
            updates: Vec::new(),
            downloads: Vec::new(),
        })
    }

    /// Replaces the check set after a library or preference change; absent sources do no work.
    pub fn configure(&mut self, installed: &[InstalledMod], enabled: bool, automatic: bool) {
        self.sources = installed
            .iter()
            .filter_map(|item| {
                let source = item.metadata.update_url.as_ref()?;
                enabled.then(|| (item.id, source.asset().to_owned()))
            })
            .collect();
        let automatic = enabled && automatic;
        if automatic != self.automatic || enabled != self.enabled {
            self.permission.fetch_add(1, Ordering::SeqCst);
            self.automatic = automatic;
            self.enabled = enabled;
        }
        let candidates = if enabled {
            installed
                .iter()
                .filter_map(|item| {
                    let source = item.metadata.update_url.as_ref();
                    let repository = source
                        .map(|source| source.repository())
                        .or(item.metadata.update_repository.as_ref())?;
                    let version = item.metadata.version.as_ref()?;
                    // A non-SemVer installed version cannot establish that a release is newer.
                    update_manifest::select::newer(version, version).ok()?;
                    Some(Candidate {
                        id: item.id,
                        repository: repository.slug().into(),
                        version: version.clone(),
                        asset: source
                            .filter(|_| automatic)
                            .map(|source| source.asset().into()),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        if candidates == self.candidates {
            return;
        }
        if self.pending.take().is_some() {
            self.permission.fetch_add(1, Ordering::SeqCst);
            self.message =
                Some("The mod library or update preferences changed. Select Update again.".into());
        }
        self.candidates = candidates;
        self.progress.clear();
        self.updates.clear();
        self.downloads.clear();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.requests.send(Request {
            manual: None,
            generation,
            permission: self.permission.load(Ordering::SeqCst),
            candidates: self.candidates.clone(),
        });
    }

    /// Drains completed checks, ignoring revisions replaced or disabled during a request.
    pub fn updates(&mut self) -> &[AvailableUpdate] {
        for response in self.responses.try_iter() {
            if response.generation == self.generation.load(Ordering::SeqCst)
                && response.manual == self.pending
                && let Some(manual) = response.manual
            {
                self.pending = None;
                if response.message.is_some() {
                    self.progress.finish(manual, response.generation);
                }
                self.message = response.message;
            }
            if response.generation == self.generation.load(Ordering::SeqCst) {
                self.updates = response.updates;
            }
            self.downloads.extend(
                response
                    .downloads
                    .into_iter()
                    .filter(Downloaded::is_current),
            );
        }
        &self.updates
    }

    /// Queues an explicit in-app update without changing the automatic-install preference.
    /// Stale cards, notification-only sources and duplicate presses do not start downloads.
    pub fn install(&mut self, id: u64) -> bool {
        if self.pending.is_some() || !self.updates.iter().any(|update| update.id == id) {
            return false;
        }
        let Some(asset) = self.sources.get(&id) else {
            return false;
        };
        let mut candidates = self.candidates.clone();
        let Some(candidate) = candidates.iter_mut().find(|candidate| candidate.id == id) else {
            return false;
        };
        candidate.asset = Some(asset.clone());
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if self
            .requests
            .send(Request {
                manual: Some(id),
                generation,
                permission: self.permission.load(Ordering::SeqCst),
                candidates,
            })
            .is_err()
        {
            self.message =
                Some("Mod updates are unavailable. Restart Cinnabar to try again.".into());
            return false;
        }
        self.progress.set(
            id,
            generation,
            launcher::mods::UpdatePhase::Downloading,
            0,
            0,
        );
        self.pending = Some(id);
        self.message = None;
        true
    }

    /// The manually selected mod while its release artifact is being downloaded.
    pub fn pending(&self) -> Option<u64> {
        self.pending
    }

    /// Transfers a completed manual download failure or cancellation to the library message.
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// Samples live transfer bytes and the same smoothed speed and ETA used for packs.
    pub fn progress(&self) -> Vec<launcher::mods::UpdateProgress> {
        self.progress.snapshot(
            self.generation.load(Ordering::SeqCst),
            std::time::Instant::now(),
        )
    }

    /// Takes verified downloads from the current check set, transferring temporary-file ownership.
    pub fn take_downloads(&mut self) -> Vec<Downloaded> {
        let downloads = std::mem::take(&mut self.downloads);
        for download in &downloads {
            self.progress
                .finish(download.id, self.generation.load(Ordering::SeqCst));
        }
        downloads
    }
}
