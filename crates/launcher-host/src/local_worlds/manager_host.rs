//! Host identity resolution and confirmed operator grants during a world start.

use crossbeam_channel::{Receiver, TryRecvError};
use std::{
    sync::{Arc, MutexGuard},
    time::Instant,
};

use super::{HOST_POLL, Host, Manager};
use crate::local_worlds::{
    Cancel, lock,
    process::Instance,
    world::{WorldError, failed},
};

impl Manager {
    /// Names the account that later starts make an operator; a malformed XUID grants nobody.
    pub fn set_host(&self, host: Host) {
        let host = match host {
            Host::Xuid(xuid) if !bridge::valid_cinnabar_xuid(&xuid) => Host::SignedOut,
            host => host,
        };
        *lock(&self.inner.host) = host;
        self.inner.host_changed.notify_all();
    }

    /// The host's XUID now, without waiting for one that is loading.
    pub(super) fn known_host(&self) -> Option<String> {
        match &*lock(&self.inner.host) {
            Host::Xuid(xuid) => Some(xuid.clone()),
            Host::SignedOut | Host::Loading => None,
        }
    }

    /// The host's XUID once it is no longer loading, `deadline` passes or `cancel` fires; logs how
    /// the wait ended, without the XUID.
    pub(super) fn await_host(
        &self,
        deadline: Instant,
        cancel: &Cancel,
        stage: &str,
        exited: Option<&Receiver<()>>,
    ) -> Option<String> {
        let waiting = Instant::now();
        let mut host = lock(&self.inner.host);
        let loading = *host == Host::Loading;
        #[cfg(test)]
        if loading {
            self.inner
                .host_waits
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        while *host == Host::Loading && !cancel.is_cancelled() {
            if exited.is_some_and(|exit| {
                matches!(
                    exit.try_recv(),
                    Err(crossbeam_channel::TryRecvError::Disconnected)
                )
            }) {
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            host = self
                .inner
                .host_changed
                .wait_timeout(host, left.min(HOST_POLL))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        let (xuid, outcome) = match (&*host, loading) {
            (Host::Xuid(xuid), false) => (Some(xuid.clone()), "already signed in"),
            (Host::Xuid(xuid), true) => (Some(xuid.clone()), "signed in while loading"),
            (Host::SignedOut, false) => (None, "signed out"),
            (Host::SignedOut, true) => (None, "signed out while loading"),
            (Host::Loading, _) if cancel.is_cancelled() => (None, "cancelled while loading"),
            (Host::Loading, _)
                if exited.is_some_and(|exit| {
                    matches!(
                        exit.try_recv(),
                        Err(crossbeam_channel::TryRecvError::Disconnected)
                    )
                }) =>
            {
                (None, "server exited while loading")
            }
            (Host::Loading, _) => (None, "still loading at the deadline"),
        };
        drop(host);
        let waited = waiting.elapsed().as_millis();
        tracing::info!("local world host {stage}: {outcome} after {waited} ms");
        xuid
    }

    /// Reconciles host changes through the last acknowledgement and holds the host lock until
    /// the caller publishes Running, so a startup account switch cannot retain an old grant.
    pub(super) fn grant_when_ready<'a>(
        &'a self,
        instance: Arc<dyn Instance>,
        launched_with: Option<&str>,
        deadline: Instant,
        cancel: &Cancel,
    ) -> Result<(Arc<dyn Instance>, MutexGuard<'a, Host>), WorldError> {
        let exited = instance.exited();
        let mut applied = launched_with.map(str::to_owned);
        let reconciled = (|| {
            loop {
                if cancel.is_cancelled() {
                    return Err(WorldError::Cancelled);
                }
                let target = self.await_host(deadline, cancel, "after launch", Some(&exited));
                if matches!(exited.try_recv(), Err(TryRecvError::Disconnected)) {
                    return Err(failed("local server exited while waiting for the host"));
                }
                if cancel.is_cancelled() {
                    return Err(WorldError::Cancelled);
                }
                if target != applied {
                    instance.grant_host(target.as_deref(), cancel)?;
                    applied = target;
                }
                let host = lock(&self.inner.host);
                let current = match &*host {
                    Host::Xuid(xuid) => Some(xuid.as_str()),
                    Host::SignedOut | Host::Loading => None,
                };
                let settled = *host != Host::Loading || Instant::now() >= deadline;
                if settled && current == applied.as_deref() {
                    return Ok(host);
                }
            }
        })();
        match reconciled {
            Ok(host) => Ok((instance, host)),
            Err(error) => {
                self.stop_instance(instance.as_ref());
                Err(error)
            }
        }
    }
}
