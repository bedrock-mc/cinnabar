//! Bounded unauthenticated GitHub requests and a launch-scoped repository cache.

use super::{AvailableUpdate, Candidate, Request, Response};
use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, Sender};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_REPOSITORIES: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Deserialize)]
pub(super) struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
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
    run(incoming, outgoing, &generation, |repository| {
        runtime.block_on(fetch(&client, repository))
    });
    Ok(())
}

/// Checks each repository at most once, coalescing obsolete library snapshots before work.
pub(super) fn run(
    incoming: Receiver<Request>,
    outgoing: Sender<Response>,
    generation: &AtomicU64,
    mut fetch: impl FnMut(&str) -> Result<Option<Release>>,
) {
    let mut cache = HashMap::new();
    let mut rate_limited = false;
    while let Ok(request) = incoming.recv() {
        let request = incoming.try_iter().last().unwrap_or(request);
        let mut updates = Vec::new();
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
            if let Some(update) = cache
                .get(&candidate.repository)
                .and_then(Option::as_ref)
                .and_then(|release| release.offer(candidate))
            {
                updates.push(update);
            }
        }
        if outgoing
            .send(Response {
                generation: request.generation,
                updates,
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
