//! Optional GitHub release checks and automatic downloads, kept off the frame thread.

mod download;
#[cfg(test)]
mod tests;
mod worker;
pub use download::Downloaded;

use crossbeam_channel::{Receiver, Sender, unbounded};
use launcher::mods::{AvailableUpdate, InstalledMod};
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
    generation: u64,
    permission: u64,
    candidates: Vec<Candidate>,
}

struct Response {
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
        std::thread::Builder::new()
            .name("mod-updates".into())
            .spawn(move || {
                let result = worker::network(
                    incoming,
                    outgoing,
                    current,
                    authorization,
                    max_component_bytes,
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
            updates: Vec::new(),
            downloads: Vec::new(),
        })
    }

    /// Replaces the check set after a library or preference change; absent sources do no work.
    pub fn configure(&mut self, installed: &[InstalledMod], enabled: bool, automatic: bool) {
        let automatic = enabled && automatic;
        if automatic != self.automatic {
            self.permission.fetch_add(1, Ordering::SeqCst);
            self.automatic = automatic;
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
        self.candidates = candidates;
        self.updates.clear();
        self.downloads.clear();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.requests.send(Request {
            generation,
            permission: self.permission.load(Ordering::SeqCst),
            candidates: self.candidates.clone(),
        });
    }

    /// Drains completed checks, ignoring revisions replaced or disabled during a request.
    pub fn updates(&mut self) -> &[AvailableUpdate] {
        for response in self.responses.try_iter() {
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

    /// Takes verified downloads from the current check set, transferring temporary-file ownership.
    pub fn take_downloads(&mut self) -> Vec<Downloaded> {
        std::mem::take(&mut self.downloads)
    }
}
