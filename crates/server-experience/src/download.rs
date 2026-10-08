//! One cancellable download job per granted server session, away from rendering.

use crate::{
    bundle::VerifiedBundle, cache::BundleCache, negotiation::Grant, policy::MAX_EXPANDED_BYTES,
};
use anyhow::{Result, ensure};
use std::{path::PathBuf, sync::mpsc};
use tokio::sync::watch;

pub struct Download {
    cancel: watch::Sender<bool>,
    result: std::sync::Mutex<mpsc::Receiver<Result<Vec<VerifiedBundle>>>>,
}

impl Download {
    /// Sequential downloads bound aggregate connections, archive bytes and expanded data.
    pub fn start(grant: Grant, cache_root: PathBuf) -> Result<Self> {
        let (cancel, mut cancelled) = watch::channel(false);
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("experience-download".into())
            .spawn(move || {
                let work = (|| -> Result<Vec<VerifiedBundle>> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(async {
                        let cache = BundleCache::open_wait(
                            &cache_root,
                            &mut cancelled,
                            tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                        )
                        .await?;
                        let mut bundles = Vec::new();
                        let mut expanded = 0u64;
                        for offer in &grant.offer.offer.packages {
                            ensure!(!*cancelled.borrow(), "download cancelled");
                            let bytes = if let Some(bytes) = cache.read(&offer.digest)? {
                                bytes
                            } else {
                                let download = crate::fetch::fetch(
                                    &offer.url,
                                    &grant.offer.offer.scope.origins,
                                    offer.bytes as usize,
                                    None,
                                );
                                let bytes = tokio::select! {
                                    biased;
                                    _ = cancelled.changed() => anyhow::bail!("download cancelled"),
                                    result = download => result?,
                                };
                                ensure!(!*cancelled.borrow(), "download cancelled");
                                cache.publish(&offer.digest, &bytes)?;
                                bytes
                            };
                            let bundle = VerifiedBundle::read(
                                &bytes,
                                offer,
                                &grant.offer.offer.scope,
                                MAX_EXPANDED_BYTES - expanded,
                            )?;
                            expanded = expanded
                                .checked_add(bundle.expanded_bytes())
                                .ok_or_else(|| anyhow::anyhow!("expanded size overflow"))?;
                            ensure!(
                                expanded <= MAX_EXPANDED_BYTES,
                                "aggregate assets exceed limit"
                            );
                            bundles.push(bundle);
                        }
                        Ok(bundles)
                    })
                })();
                let _ = sender.send(work);
            })?;
        Ok(Self {
            cancel,
            result: std::sync::Mutex::new(result),
        })
    }

    /// Polls without waiting on network, hashing or archive decoding.
    pub fn poll(&self) -> Option<Result<Vec<VerifiedBundle>>> {
        match self.result.lock().ok()?.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("download worker stopped")))
            }
        }
    }
}

impl Drop for Download {
    /// Cancels the current request; late results cannot reach a replacement session.
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}
