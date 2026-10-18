//! Bounded unauthenticated GitHub requests and a launch-scoped repository cache.

use super::{AvailableUpdate, Candidate, Request, Response, download};
use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, Sender};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
pub(super) const MAX_REPOSITORIES: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Deserialize)]
pub(super) struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<download::Asset>,
}

impl Release {
    /// Compares semantic versions and constructs the release link from the trusted repository.
    fn offer(&self, candidate: &Candidate) -> Option<AvailableUpdate> {
        if self.draft
            || self.prerelease
            || self.tag_name.len() > launcher::mods::MAX_UPDATE_VERSION_BYTES
            || self.tag_name.split('+').next()?.contains('-')
            || !update_manifest::select::newer(&self.tag_name, &candidate.version).ok()?
        {
            return None;
        }
        let mut url = url::Url::parse(&format!(
            "https://github.com/{}/releases/tag/",
            candidate.repository
        ))
        .ok()?;
        url.path_segments_mut()
            .ok()?
            .pop_if_empty()
            .push(&self.tag_name);
        Some(AvailableUpdate {
            id: candidate.id,
            version: self.tag_name.clone(),
            release_url: url.into(),
        })
    }
}

/// Builds one runtime and HTTP client for the worker's lifetime.
pub(super) fn network(
    incoming: Receiver<Request>,
    outgoing: Sender<Response>,
    generation: Arc<AtomicU64>,
    permission: Arc<AtomicU64>,
    max_component_bytes: usize,
    progress: super::progress::Progress,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = reqwest::Client::builder()
        .user_agent(format!(
            "{}/{}",
            launcher::PRODUCT_NAME,
            launcher::PRODUCT_VERSION
        ))
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let download_client = download::client()?;
    run(
        incoming,
        outgoing,
        &generation,
        |repository| runtime.block_on(fetch(&client, repository)),
        |candidate, release, revision, authorization| {
            let asset = download::select(
                &release.assets,
                candidate.asset.as_deref()?,
                max_component_bytes,
            )?;
            match runtime.block_on(download::fetch(
                &download_client,
                candidate,
                &release.tag_name,
                asset,
                download::Fence {
                    generation: generation.clone(),
                    revision,
                    permission: permission.clone(),
                    authorization,
                    progress: progress.clone(),
                },
                max_component_bytes,
            )) {
                Ok(download) => Some(download),
                Err(error) => {
                    progress.finish(candidate.id, revision);
                    tracing::debug!(repository = candidate.repository, "Mod download: {error:#}");
                    None
                }
            }
        },
    );
    Ok(())
}

/// Checks each repository at most once, coalescing obsolete library snapshots before work.
pub(super) fn run(
    incoming: Receiver<Request>,
    outgoing: Sender<Response>,
    generation: &AtomicU64,
    mut fetch: impl FnMut(&str) -> Result<Option<Release>>,
    mut download: impl FnMut(&Candidate, &Release, u64, u64) -> Option<download::Downloaded>,
) {
    let mut cache = HashMap::new();
    let mut attempted = HashSet::new();
    let mut downloads_started = 0;
    let mut rate_limited = false;
    while let Ok(request) = incoming.recv() {
        let request = incoming.try_iter().last().unwrap_or(request);
        let mut updates = Vec::new();
        let mut manual_completed = false;
        for candidate in &request.candidates {
            if request.generation != generation.load(Ordering::SeqCst) {
                break;
            }
            if !cache.contains_key(&candidate.repository) {
                // Bound network work even if a directory contains hundreds of components.
                if rate_limited || cache.len() >= MAX_REPOSITORIES {
                    continue;
                }
                let release = fetch(&candidate.repository).unwrap_or_else(|error| {
                    rate_limited = error
                        .downcast_ref::<reqwest::Error>()
                        .and_then(|error| error.status())
                        .is_some_and(|status| {
                            matches!(
                                status,
                                reqwest::StatusCode::FORBIDDEN
                                    | reqwest::StatusCode::TOO_MANY_REQUESTS
                            )
                        });
                    tracing::debug!(
                        repository = candidate.repository,
                        "Mod update check: {error:#}"
                    );
                    None
                });
                cache.insert(candidate.repository.clone(), release);
            }
            if let Some(release) = cache.get(&candidate.repository).and_then(Option::as_ref)
                && let Some(update) = release.offer(candidate)
            {
                updates.push(update);
                let key = (candidate.id, candidate.asset.clone(), request.permission);
                if candidate.asset.is_some()
                    && request.generation == generation.load(Ordering::SeqCst)
                    && (request.manual == Some(candidate.id)
                        || downloads_started < MAX_REPOSITORIES)
                    && (request.manual == Some(candidate.id) || attempted.insert(key.clone()))
                {
                    if request.manual != Some(candidate.id) {
                        downloads_started += 1;
                    }
                    if let Some(download) =
                        download(candidate, release, request.generation, request.permission)
                    {
                        let manual = request.manual.filter(|id| *id == candidate.id);
                        manual_completed |= manual.is_some();
                        if outgoing
                            .send(Response {
                                manual,
                                message: None,
                                generation: request.generation,
                                updates: updates.clone(),
                                downloads: vec![download],
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    if request.generation != generation.load(Ordering::SeqCst) {
                        attempted.remove(&key);
                    }
                }
            }
        }
        let manual = request.manual.filter(|_| !manual_completed);
        let message = manual.map(|_| {
            "Could not download the mod update. Your installed mod was kept. Try again.".into()
        });
        if outgoing
            .send(Response {
                manual,
                message,
                generation: request.generation,
                updates,
                downloads: Vec::new(),
            })
            .is_err()
        {
            return;
        }
    }
}

/// Reads only GitHub's latest published release; missing repositories/releases are silent.
async fn fetch(client: &reqwest::Client, repository: &str) -> Result<Option<Release>> {
    let mut response = client
        .get(format!(
            "https://api.github.com/repos/{repository}/releases/latest"
        ))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    response.error_for_status_ref()?;
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= MAX_RESPONSE_BYTES as u64),
        "release response exceeds byte limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_RESPONSE_BYTES,
            "release response exceeds byte limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .context("invalid GitHub release")
        .map(Some)
}
