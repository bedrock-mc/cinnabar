//! Coalesced playback and loadouts for one authenticated socket, using caller-supplied clocks.

use super::{policy::*, wire::Inbound};
use bridge::contract::{
    CinnabarCosmeticSlot, CinnabarPlayerState, CinnabarPlayerUpdates, CinnabarRemoteEmote,
    CinnabarRemoteLoadout,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Players {
    local: CinnabarPlayerState,
    ownership_xuid: String,
    owned_cosmetics: Vec<String>,
    queued: Option<Instant>,
    watch_sent: Option<Instant>,
    sent_xuids: Vec<String>,
    sent_uuids: Vec<String>,
    emote_dirty: bool,
    loadout_sent: bool,
    loadout_dirty: bool,
    slots: Vec<CinnabarCosmeticSlot>,
    revision: u64,
    emotes: BTreeMap<String, (CinnabarRemoteEmote, Instant)>,
    loadouts: BTreeMap<String, CinnabarRemoteLoadout>,
}

impl Players {
    /// Binds grants to the account authenticated by this original socket's welcome.
    pub(super) fn welcomed(xuid: String, ids: Vec<String>) -> Self {
        if !bridge::valid_cinnabar_xuid(&xuid) {
            return Self::default();
        }
        Self {
            ownership_xuid: xuid,
            owned_cosmetics: admitted_ownership(ids),
            ..Self::default()
        }
    }

    /// Replaces the latest bounded local intent and snapshots only currently watched remote identities.
    pub(super) fn sync(
        &mut self,
        state: CinnabarPlayerState,
        now: Instant,
        unix_ms: u64,
        revision: &mut u64,
    ) -> CinnabarPlayerUpdates {
        if !valid_watches(&state.xuids, &state.uuids)
            || !valid_slots(&state.slots)
            || (!state.emote_id.is_empty() && !valid_emote(&state.emote_id))
        {
            return CinnabarPlayerUpdates::default();
        }
        if !self.loadout_sent && state.slots.is_empty() && self.revision == 0 {
            self.loadout_sent = true;
        } else if (!self.loadout_sent || self.slots != state.slots) && *revision < MAX_BUILD {
            *revision = revision.saturating_add(1).max(unix_ms).min(MAX_BUILD);
            self.revision = *revision;
            self.slots.clone_from(&state.slots);
            self.loadout_sent = true;
            self.loadout_dirty = true;
        }
        let changed = self.local.emote_id != state.emote_id
            || self.local.playback_id != state.playback_id
            || self.local.xuids != state.xuids
            || self.local.uuids != state.uuids;
        let watches_pending = self.sent_xuids != state.xuids || self.sent_uuids != state.uuids;
        let watch_due = self
            .watch_sent
            .is_none_or(|sent| now.saturating_duration_since(sent) >= EMOTE_TTL / 5);
        let age = self.queued.map(|at| now.saturating_duration_since(at));
        if age.is_none_or(|age| age >= EMOTE_SPACING)
            && (changed
                || (watches_pending && watch_due)
                || age.is_none_or(|age| age >= EMOTE_TTL / 3))
        {
            self.local = state.clone();
            self.queued = Some(now);
            self.emote_dirty = true;
        }
        self.local.xuids = state.xuids;
        self.local.uuids = state.uuids;
        self.snapshot(now)
    }

    /// Invalidates only the persistent loadout's sent marker after an actual visibility update.
    pub(super) fn visibility_changed(&mut self) {
        self.loadout_sent = false;
    }

    /// Emits one newest full replacement; no frame-owned queue retains obsolete player states.
    pub(super) fn next_payload(&mut self) -> Option<Value> {
        if self.loadout_dirty {
            self.loadout_dirty = false;
            let slots: BTreeMap<_, _> = self
                .slots
                .iter()
                .map(|slot| (&slot.slot, &slot.item_id))
                .collect();
            return Some(json!({"type":"loadout","revision":self.revision,"slots":slots}));
        }
        if !self.emote_dirty {
            return None;
        }
        self.emote_dirty = false;
        let mut message = json!({"type":"emote_state","emote_id":if self.local.emote_id.is_empty() { None } else {Some(&self.local.emote_id)},"elapsed_ms":self.local.elapsed_ms});
        let changed = self.sent_xuids != self.local.xuids || self.sent_uuids != self.local.uuids;
        let queued = self.queued.expect("queued playback");
        if self
            .watch_sent
            .is_none_or(|sent| changed && queued.saturating_duration_since(sent) >= EMOTE_TTL / 5)
        {
            message["xuids"] = json!(self.local.xuids);
            message["uuids"] = json!(self.local.uuids);
            self.sent_xuids.clone_from(&self.local.xuids);
            self.sent_uuids.clone_from(&self.local.uuids);
            self.watch_sent = Some(queued);
        }
        Some(message)
    }

    /// Replaces local grants or retains bounded, watched playback and same/newer remote loadout revisions.
    pub(super) fn receive(&mut self, msg: Inbound, now: Instant) {
        if msg.kind == "cosmetic_ownership" {
            self.owned_cosmetics = admitted_ownership(msg.owned_cosmetics.unwrap_or_default());
            return;
        }
        if !bridge::valid_cinnabar_xuid(&msg.xuid)
            || !bridge::valid_cinnabar_uuid(&msg.uuid)
            || !self.watches(&msg.xuid, &msg.uuid)
        {
            return;
        }
        if msg.kind == "emote" {
            let Some(id) = msg.emote_id.filter(|id| !id.is_empty()) else {
                self.emotes.remove(&msg.xuid);
                return;
            };
            if msg.valid_for_ms == 0 {
                self.emotes.remove(&msg.xuid);
                return;
            }
            if !valid_emote(&id)
                || Duration::from_millis(msg.valid_for_ms) > EMOTE_TTL
                || (self.emotes.len() >= bridge::contract::CINNABAR_API_MAX_LOOKUP_XUIDS
                    && !self.emotes.contains_key(&msg.xuid))
            {
                return;
            }
            self.emotes.insert(
                msg.xuid.clone(),
                (
                    CinnabarRemoteEmote {
                        xuid: msg.xuid,
                        uuid: msg.uuid,
                        emote_id: id,
                        elapsed_ms: u64::from(msg.elapsed_ms),
                        valid_for_ms: msg.valid_for_ms,
                    },
                    now,
                ),
            );
        } else if msg.kind == "loadout" {
            if msg.revision > MAX_BUILD
                || msg.slots.len() > 16
                || self
                    .loadouts
                    .get(&msg.xuid)
                    .is_some_and(|old| old.revision > msg.revision)
            {
                return;
            }
            let slots: Vec<_> = msg
                .slots
                .into_iter()
                .filter_map(|(slot, id)| id.map(|item_id| CinnabarCosmeticSlot { slot, item_id }))
                .collect();
            if !valid_slots(&slots) {
                return;
            }
            if slots.is_empty() {
                self.loadouts.remove(&msg.xuid);
                return;
            }
            if self.loadouts.len() >= bridge::contract::CINNABAR_API_MAX_LOOKUP_XUIDS
                && !self.loadouts.contains_key(&msg.xuid)
            {
                return;
            }
            self.loadouts.insert(
                msg.xuid.clone(),
                CinnabarRemoteLoadout {
                    xuid: msg.xuid,
                    uuid: msg.uuid,
                    revision: msg.revision,
                    slots,
                },
            );
        }
    }

    /// Compares either authenticated namespace against the latest actual nearby subscriptions.
    fn watches(&self, xuid: &str, uuid: &str) -> bool {
        self.local.xuids.iter().any(|id| id == xuid) || self.local.uuids.iter().any(|id| id == uuid)
    }

    /// Ages playback without a network roundtrip and removes departed peers without imposing a loadout lease.
    fn snapshot(&mut self, now: Instant) -> CinnabarPlayerUpdates {
        let local = &self.local;
        let watches =
            |xuid: &String, uuid: &String| local.xuids.contains(xuid) || local.uuids.contains(uuid);
        self.emotes.retain(|_, (value, received)| {
            watches(&value.xuid, &value.uuid)
                && now.saturating_duration_since(*received)
                    < Duration::from_millis(value.valid_for_ms)
        });
        self.loadouts
            .retain(|_, value| watches(&value.xuid, &value.uuid));
        CinnabarPlayerUpdates {
            ownership_xuid: self.ownership_xuid.clone(),
            owned_cosmetics: self.owned_cosmetics.clone(),
            emotes: self
                .emotes
                .values()
                .map(|(value, received)| {
                    let age = u64::try_from(now.saturating_duration_since(*received).as_millis())
                        .unwrap_or(u64::MAX);
                    let mut value = value.clone();
                    value.elapsed_ms = value.elapsed_ms.saturating_add(age);
                    value.valid_for_ms = value.valid_for_ms.saturating_sub(age);
                    value
                })
                .collect(),
            loadouts: self.loadouts.values().cloned().collect(),
        }
    }
}
