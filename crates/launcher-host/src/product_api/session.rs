//! One welcomed socket's bounded requests, coalesced writes and committed results.

use super::*;

impl Client {
    /// Installs only a welcomed original socket and retains the account revision through reconnects.
    pub(super) fn welcome(
        &self,
        epoch: u64,
        config: &CinnabarApiConfig,
        scope: ApiScope,
        message: Inbound,
    ) -> bool {
        let accepted = self.with(|state| {
            if state.stopped || state.epoch != epoch {
                return false;
            }
            state.parameters = message.parameters.normalized();
            state.scope = Some(scope);
            state.revision = state
                .revision
                .max(message.loadout_revision.min(policy::MAX_BUILD));
            state.live = Some(Live {
                visible: config.visible,
                next_id: 0,
                players: Players::welcomed(
                    message.xuid,
                    message.owned_cosmetics.unwrap_or_default(),
                ),
                pending: BTreeMap::new(),
            });
            true
        });
        if accepted {
            self.0.changed.notify_waiters();
        }
        accepted
    }

    /// Claims exactly one current payload; a failed/partial write retires this socket and never requeues it.
    pub(super) fn next_payload(&self, epoch: u64, now: Instant) -> Option<(Value, Instant)> {
        self.with(|state| {
            if state.epoch != epoch || state.stopped {
                return None;
            }
            let live = state.live.as_mut()?;
            live.pending
                .retain(|_, p| !p.reply.is_closed() && p.deadline > now);
            let visible = state.config.as_ref()?.visible;
            if visible != live.visible {
                live.visible = visible;
                live.players.visibility_changed();
                return Some((
                    json!({"type":"visibility","visible":visible}),
                    now + REQUEST_TIMEOUT,
                ));
            }
            if let Some((id, p)) = live.pending.iter_mut().find(|(_, p)| !p.sent) {
                p.sent = true;
                let mut message = json!({"type":"lookup","id":id});
                if !p.xuids.is_empty() {
                    message["xuids"] = json!(p.xuids);
                }
                if !p.uuids.is_empty() {
                    message["uuids"] = json!(p.uuids);
                }
                return Some((message, p.deadline.min(now + REQUEST_TIMEOUT)));
            }
            live.players
                .next_payload()
                .map(|message| (message, now + REQUEST_TIMEOUT))
        })
    }

    /// Commits an already-read reply only under the unchanged epoch after its native identity fence.
    pub(super) fn receive(&self, epoch: u64, message: Inbound, now: Instant) {
        self.with(|state| {
            if state.stopped || state.epoch != epoch {
                return;
            }
            if message.kind == "release" {
                if state
                    .config
                    .as_ref()
                    .is_some_and(|config| config.channel == message.channel)
                    && !message.version.is_empty()
                {
                    state.release = Some(CinnabarRelease {
                        channel: message.channel,
                        version: message.version,
                        build: message.build,
                        published_at: message.published_at,
                    });
                }
                return;
            }
            let Some(live) = state.live.as_mut() else {
                return;
            };
            match message.kind.as_str() {
                "lookup_result" | "error" => {
                    let Some(pending) = message.id.and_then(|id| live.pending.remove(&id)) else {
                        return;
                    };
                    if pending.deadline <= now || !pending.sent {
                        return;
                    }
                    let result = if message.kind == "error" {
                        Err(if message.code == "rate_limited" {
                            rate_limited()
                        } else {
                            unavailable()
                        })
                    } else {
                        Ok(CinnabarLookup {
                            cinnabar: message
                                .cinnabar
                                .into_iter()
                                .filter(|id| pending.xuids.contains(id))
                                .collect(),
                            cinnabar_uuids: message
                                .cinnabar_uuids
                                .into_iter()
                                .filter(|id| pending.uuids.contains(id))
                                .collect(),
                            resolved: pending.xuids,
                            resolved_uuids: pending.uuids,
                            cache_ttl_ms: u64::try_from(state.parameters.cache_ttl.as_millis())
                                .unwrap_or(u64::MAX),
                        })
                    };
                    let _ = pending.reply.send(result);
                }
                "emote" | "loadout" | "cosmetic_ownership" => live.players.receive(message, now),
                _ => {}
            }
        });
    }
}
