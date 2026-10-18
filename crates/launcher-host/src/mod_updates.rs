//! Optional GitHub release notices for installed mods, checked off-frame once per launch.

#[cfg(test)]
mod tests;
mod worker;

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
}

struct Request {
    generation: u64,
    candidates: Vec<Candidate>,
}

struct Response {
    generation: u64,
    updates: Vec<AvailableUpdate>,
}

/// One session's release checks and publication fence; failures never block mod management.
pub struct Checker {
    requests: Sender<Request>,
    responses: Receiver<Response>,
    generation: Arc<AtomicU64>,
    candidates: Vec<Candidate>,
    updates: Vec<AvailableUpdate>,
}

impl Checker {
    /// Starts one worker with a per-repository session cache and no account credentials.
    pub fn start() -> std::io::Result<Self> {
        let (requests, incoming) = unbounded();
        let (outgoing, responses) = unbounded();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        std::thread::Builder::new()
            .name("mod-updates".into())
            .spawn(move || {
                let result = worker::network(incoming, outgoing, current);
                if let Err(error) = result {
                    tracing::debug!("Mod update checks unavailable: {error:#}");
                }
            })?;
        Ok(Self {
            requests,
            responses,
            generation,
            candidates: Vec::new(),
            updates: Vec::new(),
        })
    }

    /// Replaces the check set after a library or preference change; absent sources do no work.
    pub fn configure(&mut self, installed: &[InstalledMod], enabled: bool) {
        let candidates = if enabled {
            installed
                .iter()
                .filter_map(|item| {
                    let repository = item.metadata.update_repository.as_ref()?;
                    let version = item.metadata.version.as_ref()?;
                    // A non-SemVer installed version cannot establish that a release is newer.
                    update_manifest::select::newer(version, version).ok()?;
                    Some(Candidate {
                        id: item.id,
                        repository: repository.slug().into(),
                        version: version.clone(),
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
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.requests.send(Request {
            generation,
            candidates: self.candidates.clone(),
        });
    }

    /// Drains completed checks, ignoring revisions replaced or disabled during a request.
    pub fn updates(&mut self) -> &[AvailableUpdate] {
        for response in self.responses.try_iter() {
            if response.generation == self.generation.load(Ordering::SeqCst) {
                self.updates = response.updates;
            }
        }
        &self.updates
    }
}
