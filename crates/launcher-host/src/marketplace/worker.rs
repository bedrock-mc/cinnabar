//! Serialized account delivery, bounded public HTTP and signed revocation refreshes.

use super::{
    Guard, Keys, SignedRelease, SignedRevocations, now_ms,
    storage::{self, Downloaded, Receipt},
};
use anyhow::{Result, ensure};
use bridge::contract::CinnabarModEntry;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub(super) const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_REVOCATIONS_BYTES: usize = 1024 * 1024;

/// Public transport is fixed by the client contract; only in-module tests substitute loopback HTTP.
struct Transport {
    client: reqwest::Client,
    origin: url::Url,
}

type Request = (String, String, String, String, String);

/// Queue admission deduplicates one account epoch; only verified receipts survive a switch.
#[derive(Default)]
struct Deliveries {
    epoch: Option<u64>,
    requests: HashSet<Request>,
}

impl Deliveries {
    /// Forgets cancelled queue-only work while retaining successful and locally removed sends.
    fn refresh(
        &mut self,
        epoch: u64,
        mods: &Path,
        keys: Option<&Keys>,
        limit: usize,
        guard: &Guard,
        now: u64,
    ) {
        if self.epoch == Some(epoch) {
            return;
        }
        self.requests = keys
            .map(|keys| storage::restore(mods, keys, limit, guard, now))
            .unwrap_or_default()
            .iter()
            .map(|receipt| request(&receipt.owner, &receipt.selected))
            .collect();
        self.epoch = Some(epoch);
    }

    /// Records a send only when the compatibility worker has accepted its complete download.
    fn queue(&mut self, key: Request, prepared: Downloaded, outgoing: &Sender<Downloaded>) {
        if outgoing.try_send(prepared).is_ok() {
            self.requests.insert(key);
        }
    }
}

/// Identifies an explicit website send, preserving local removals until the player sends it again.
fn request(owner: &str, selected: &CinnabarModEntry) -> Request {
    (
        owner.into(),
        selected.slug.clone(),
        selected.version.clone(),
        selected.sha256.clone(),
        selected.sent_at.clone(),
    )
}

/// Performs one bounded public GET with no credentials, arbitrary URLs, redirects or partial responses.
async fn fetch(
    transport: &Transport,
    path: &str,
    limit: usize,
) -> Result<(Vec<u8>, Option<String>)> {
    let url = transport.origin.join(path)?;
    let mut response = transport.client.get(url).send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "Marketplace returned {}.",
        response.status()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= limit as u64),
        "Marketplace response exceeds its byte limit."
    );
    let signature = response
        .headers()
        .get("X-Cinnabar-Signature")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "Marketplace response exceeds its byte limit."
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, signature))
}

/// Verifies and persists fresh revocations before allowing any new installation.
async fn refresh(
    transport: &Transport,
    mods: &Path,
    keys: &Keys,
    guard: &Guard,
    now: impl Fn() -> u64,
) -> Result<()> {
    let (bytes, signature) =
        fetch(transport, "/v1/mods/revocations", MAX_REVOCATIONS_BYTES).await?;
    let signed = SignedRevocations {
        body: String::from_utf8(bytes)?,
        signature: signature.ok_or_else(|| anyhow::anyhow!("Missing revocation signature."))?,
    };
    let state = signed.verify(keys, now(), guard.checkpoint())?;
    // Persist before advancing the in-memory checkpoint, so restarts retain rollback protection.
    storage::save(
        &mods.join(".marketplace/revocations.json"),
        &serde_json::to_vec(&signed)?,
    )?;
    guard.revocations(state);
    Ok(())
}

/// Retrieves the exact signed release, streams its digest-bound file and fences account changes.
#[allow(
    clippy::too_many_arguments,
    reason = "download binds public transport, source ceiling and account cancellation fence"
)]
async fn download(
    transport: &Transport,
    mods: &Path,
    keys: &Keys,
    owner: &str,
    selected: &CinnabarModEntry,
    limit: usize,
    fence: (Arc<AtomicU64>, u64, Arc<Guard>),
    now: impl Fn() -> u64,
) -> Result<Downloaded> {
    let (generation, epoch, guard) = fence;
    #[derive(Deserialize)]
    struct Response {
        release: SignedRelease,
    }
    // Encode the path segment even though the local bridge admits only canonical selectors.
    let mut url = transport.origin.clone();
    url.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("Invalid marketplace origin."))?
        .extend([
            "v1",
            "mods",
            &selected.slug,
            "releases",
            &selected.version,
            &selected.sha256,
        ]);
    let (body, _) = fetch(transport, url.path(), MAX_RESPONSE_BYTES).await?;
    let release = serde_json::from_slice::<Response>(&body)?.release;
    let manifest = release.verify(keys, selected, limit, now())?;
    let (bytes, _) = fetch(
        transport,
        &format!("/v1/mods/assets/{}", manifest.sha256),
        manifest.bytes as usize,
    )
    .await?;
    super::verify::verify_bytes(&bytes, &manifest)?;
    ensure!(
        generation.load(Ordering::SeqCst) == epoch && guard.permits(owner, &manifest.sha256, now()),
        "Marketplace download cancelled or held."
    );
    let mut file = tempfile::NamedTempFile::new_in(mods)?;
    file.write_all(&bytes)?;
    Ok(Downloaded {
        file,
        manifest,
        receipt: Receipt {
            owner: owner.into(),
            selected: selected.clone(),
            release,
        },
        generation,
        epoch,
        guard,
        mods: mods.into(),
    })
}

/// Owns one off-frame delivery loop and exits when the app drops its request channel.
#[allow(
    clippy::too_many_arguments,
    reason = "one worker owns its bounded channels, identity fence and runtime authority"
)]
pub(super) fn run(
    mods: PathBuf,
    limit: usize,
    guard: Arc<Guard>,
    generation: Arc<AtomicU64>,
    context: Arc<Mutex<Option<PathBuf>>>,
    incoming: Receiver<()>,
    outgoing: Sender<Downloaded>,
    events: &Sender<String>,
) -> Result<()> {
    let keys = Keys::parse(include_bytes!("../../data/marketplace-keys.json")).ok();
    fs::create_dir_all(&mods)?;
    let mods = mods.canonicalize()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;
    let transport = Transport {
        client,
        origin: bridge::contract::CINNABAR_API_BASE_URL.parse()?,
    };
    if let Some(keys) = &keys
        && let Ok(bytes) = storage::read(
            &mods.join(".marketplace/revocations.json"),
            2 * MAX_REVOCATIONS_BYTES + 1024,
        )
        && let Ok(signed) = serde_json::from_slice::<SignedRevocations>(&bytes)
        && let Ok(state) = signed.restore(keys, now_ms())
    {
        guard.revocations(state);
    }
    let mut deliveries = Deliveries::default();
    let mut refresh_at = 0;
    loop {
        let (socket, epoch) = {
            let endpoint = context.lock().unwrap_or_else(|e| e.into_inner());
            (endpoint.clone(), generation.load(Ordering::SeqCst))
        };
        deliveries.refresh(epoch, &mods, keys.as_ref(), limit, &guard, now_ms());
        if let Some(socket) = socket {
            if let Some(keys) = &keys
                && now_ms() >= refresh_at
            {
                if let Err(error) =
                    runtime.block_on(refresh(&transport, &mods, keys, &guard, now_ms))
                {
                    let _ =
                        events.try_send(format!("Marketplace checks could not refresh: {error:#}"));
                }
                refresh_at = now_ms().saturating_add(60_000);
            }
            if let Ok(Ok(library)) = runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), bridge::cinnabar_mods(&socket)).await
            }) {
                if bridge::valid_cinnabar_xuid(&library.xuid) {
                    guard.account(epoch, library.xuid.clone());
                }
                if bridge::valid_cinnabar_xuid(&library.xuid)
                    && library.mods.len() <= bridge::contract::CINNABAR_MODS_MAX_ENTRIES
                    && generation.load(Ordering::SeqCst) == epoch
                {
                    for selected in library.mods {
                        if selected.pulled {
                            guard.hold(&selected.sha256);
                        }
                        let Some(keys) = &keys else {
                            let _ = events.try_send("Marketplace downloads are not enabled in this build. Update Cinnabar or import a local mod.".into());
                            break;
                        };
                        if storage::removed(&mods, &library.xuid, &selected, keys, limit, now_ms())
                        {
                            let _ = runtime.block_on(async {
                                tokio::time::timeout(
                                    Duration::from_secs(2),
                                    bridge::remove_cinnabar_mod(&socket, &selected),
                                )
                                .await
                            });
                            continue;
                        }
                        if selected.pulled {
                            continue;
                        }
                        let key = request(&library.xuid, &selected);
                        if deliveries.requests.contains(&key) {
                            continue;
                        }
                        let _ = events
                            .try_send(format!("Preparing {} from the marketplace…", selected.slug));
                        let prepared = runtime.block_on(download(
                            &transport,
                            &mods,
                            keys,
                            &library.xuid,
                            &selected,
                            limit,
                            (generation.clone(), epoch, guard.clone()),
                            now_ms,
                        ));
                        match prepared {
                            Ok(prepared) => {
                                // Reconfirm the live account and exact selection after slow network I/O.
                                let current = runtime.block_on(async {
                                    tokio::time::timeout(
                                        Duration::from_secs(2),
                                        bridge::cinnabar_mods(&socket),
                                    )
                                    .await
                                });
                                if matches!(current, Ok(Ok(current)) if current.xuid == library.xuid && current.mods.contains(&selected))
                                {
                                    deliveries.queue(key, prepared, &outgoing);
                                }
                            }
                            Err(error) => {
                                let _ = events.try_send(format!(
                                    "Marketplace mod could not be prepared: {error:#}"
                                ));
                            }
                        }
                    }
                }
            }
        }
        if let Err(RecvTimeoutError::Disconnected) = incoming.recv_timeout(Duration::from_secs(5)) {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
