//! Noncredential request handles and original account-lifetime cancellation.

use super::*;

impl Client {
    /// Applies newest identity without I/O; only identity changes replace the original socket.
    pub(crate) fn configure(&self, config: CinnabarApiConfig) {
        if !policy::valid_config(&config) {
            return;
        }
        self.with(|state| {
            if state.config.as_ref() == Some(&config) {
                return;
            }
            if state.config.as_ref().is_some_and(|old| {
                old.channel != config.channel
                    || old.client_version != config.client_version
                    || old.client_build != config.client_build
            }) {
                state.epoch = state.epoch.wrapping_add(1);
                state.live = None;
                state.release = None;
            }
            state.config = Some(config);
        });
        self.0.changed.notify_waiters();
    }

    /// Cancels old generation replies/caches before the menu accepts another account snapshot.
    pub(crate) fn retire_account(&self) {
        self.with(|state| {
            state.epoch = state.epoch.wrapping_add(1);
            state.live = None;
            state.release = None;
        });
        self.0.changed.notify_waiters();
    }

    /// Ends this original worker permanently and wakes every pending reply without blocking a frame.
    pub(crate) fn stop(&self) {
        self.with(|state| {
            state.stopped = true;
            state.live = None;
            state.release = None;
        });
        self.0.changed.notify_waiters();
    }

    /// Records actual socket-future cancellation/completion on the sole event owner.
    pub(crate) fn finish(&self) {
        self.with(|state| {
            state.stopped = true;
            state.finished = true;
            state.live = None;
            state.scope = None;
            state.release = None;
        });
        self.0.changed.notify_waiters();
    }

    /// Waits for the actual owner to drop its original I/O before the existing sign-out mutation.
    pub(crate) async fn finished(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.with(|state| state.finished) {
                return;
            }
            changed.await;
        }
    }

    /// Charges and queues one negotiated prefix, preserving existing roster-cache unresolved suffixes.
    pub(crate) async fn lookup(
        &self,
        identities: &[String],
    ) -> Result<CinnabarLookup, BridgeError> {
        let now = Instant::now();
        let (reply, answer) = oneshot::channel();
        let (epoch, scope) = self.with(|state| {
            if identities.is_empty()
                || identities.len() > CINNABAR_API_MAX_LOOKUP_XUIDS
                || identities
                    .iter()
                    .any(|id| !bridge::valid_cinnabar_xuid(id) && !bridge::valid_cinnabar_uuid(id))
            {
                return Err(unavailable());
            }
            if state.stopped {
                return Err(unavailable());
            }
            let p = state.parameters;
            let live = state.live.as_mut().ok_or_else(unavailable)?;
            live.pending
                .retain(|_, pending| !pending.reply.is_closed() && pending.deadline > now);
            if live.pending.len() >= PENDING_LIMIT {
                return Err(unavailable());
            }
            let limit = p
                .batch
                .min(usize::try_from(p.capacity).unwrap_or(usize::MAX));
            let xuids: Vec<_> = identities
                .iter()
                .filter(|id| bridge::valid_cinnabar_xuid(id))
                .take(limit)
                .cloned()
                .collect();
            let uuids: Vec<_> = identities
                .iter()
                .filter(|id| bridge::valid_cinnabar_uuid(id))
                .take(limit - xuids.len())
                .cloned()
                .collect();
            if !state.budget.take(now, xuids.len() + uuids.len(), p) {
                return Err(rate_limited());
            }
            while live.pending.contains_key(&live.next_id) {
                live.next_id = live.next_id.wrapping_add(1);
            }
            let id = live.next_id;
            live.next_id = live.next_id.wrapping_add(1);
            live.pending.insert(
                id,
                Pending {
                    xuids,
                    uuids,
                    deadline: now + REQUEST_TIMEOUT,
                    sent: false,
                    reply,
                },
            );
            Ok((state.epoch, state.scope.clone().ok_or_else(unavailable)?))
        })?;
        self.0.changed.notify_waiters();
        tokio::time::timeout_at(
            tokio::time::Instant::from_std(now + REQUEST_TIMEOUT),
            async {
                let result = answer.await.map_err(|_| unavailable())?;
                loop {
                    if Instant::now() >= now + REQUEST_TIMEOUT || !self.current(epoch) {
                        return Err(unavailable());
                    }
                    match scope.current() {
                        Some(true) => return result,
                        Some(false) => return Err(unavailable()),
                        None => tokio::time::sleep(crate::native_account::LEASE_RETRY).await,
                    }
                }
            },
        )
        .await
        .map_err(|_| unavailable())?
    }

    /// Coalesces the newest local player intent and reads aged, watched remote state without socket I/O.
    pub(crate) fn sync(&self, local: CinnabarPlayerState) -> PlayerSync {
        let Some((epoch, scope)) = self.with(|state| {
            if state.stopped || state.live.is_none() {
                return None;
            }
            Some((state.epoch, state.scope.clone()?))
        }) else {
            return PlayerSync::Unavailable;
        };
        match scope.current() {
            None => return PlayerSync::Pending,
            Some(false) => return PlayerSync::Unavailable,
            Some(true) => {}
        }
        let now = Instant::now();
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok())
            .unwrap_or_default();
        let result = self.with(|state| {
            if state.stopped || state.epoch != epoch {
                return None;
            }
            let State { live, revision, .. } = state;
            live.as_mut()
                .map(|live| live.players.sync(local, now, unix_ms, revision))
        });
        self.0.changed.notify_waiters();
        result.map_or(PlayerSync::Unavailable, PlayerSync::Current)
    }

    /// Delivers only the latest accepted announcement through the launcher's existing release path.
    pub(crate) fn take_release(&self) -> Option<CinnabarRelease> {
        let scope = self.with(|state| state.scope.clone())?;
        if scope.current() != Some(true) {
            return None;
        }
        self.with(|state| state.release.take())
    }
}
