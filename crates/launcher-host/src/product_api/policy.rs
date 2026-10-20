//! Caller-clock operating limits and identifier budget shared by every connection of this owner.

use bridge::contract::*;
use serde::Deserialize;
use std::time::{Duration, Instant};

pub(super) const MAX_BUILD: u64 = (1 << 53) - 1;
pub(super) const EMOTE_TTL: Duration = Duration::from_secs(15);
pub(super) const EMOTE_SPACING: Duration = Duration::from_millis(500);
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const RECONNECT_SPREAD: Duration = Duration::from_secs(30);

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Advertised {
    lookup_batch_max: i64,
    lookup_bucket_capacity: i64,
    lookup_refill_per_min: i64,
    cache_ttl_ms: i64,
    ping_interval_ms: i64,
    reconnect_max_ms: i64,
}

#[derive(Clone, Copy)]
pub(super) struct Parameters {
    pub(super) batch: usize,
    pub(super) capacity: u64,
    pub(super) refill: u64,
    pub(super) cache_ttl: Duration,
    pub(super) ping: Duration,
    pub(super) reconnect: Duration,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            batch: CINNABAR_API_MAX_LOOKUP_XUIDS,
            capacity: u64::from(CINNABAR_API_LOOKUP_BUCKET_CAPACITY),
            refill: u64::from(CINNABAR_API_LOOKUP_REFILL_PER_MIN),
            cache_ttl: Duration::from_millis(u64::from(CINNABAR_API_CACHE_TTL_MS)),
            ping: Duration::from_millis(u64::from(CINNABAR_API_PING_INTERVAL_MS)),
            reconnect: Duration::from_millis(u64::from(CINNABAR_API_RECONNECT_MAX_MS)),
        }
    }
}

impl Advertised {
    /// Ignores absent/unusable extensions, caps only batch size, and bounds duration arithmetic.
    pub(super) fn normalized(self) -> Parameters {
        let mut p = Parameters::default();
        if let Ok(value) = usize::try_from(self.lookup_batch_max)
            && value > 0
        {
            p.batch = value.min(p.batch);
        }
        if self.lookup_bucket_capacity > 0 {
            p.capacity = self.lookup_bucket_capacity as u64;
        }
        if self.lookup_refill_per_min > 0 {
            p.refill = self.lookup_refill_per_min as u64;
        }
        for (value, target) in [
            (self.cache_ttl_ms, &mut p.cache_ttl),
            (self.ping_interval_ms, &mut p.ping),
            (self.reconnect_max_ms, &mut p.reconnect),
        ] {
            if value > 0 && value <= i64::MAX / 3_000_000 {
                *target = Duration::from_millis(value as u64);
            }
        }
        p
    }
}

#[derive(Default)]
pub(super) struct Budget {
    tokens: f64,
    at: Option<Instant>,
}

impl Budget {
    /// Charges both namespaces once; neither socket replacement nor welcome resets the owner budget.
    pub(super) fn take(&mut self, now: Instant, count: usize, p: Parameters) -> bool {
        self.tokens = match self.at {
            Some(at) => {
                self.tokens
                    + now.saturating_duration_since(at).as_secs_f64() / 60.0 * p.refill as f64
            }
            None => p.capacity as f64,
        }
        .min(p.capacity as f64);
        self.at = Some(now);
        if self.tokens < count as f64 {
            return false;
        }
        self.tokens -= count as f64;
        true
    }
}

/// Validates a whole subscription, with empty lists reserved for clearing nearby watches.
pub(super) fn valid_watches(xuids: &[String], uuids: &[String]) -> bool {
    xuids.len().saturating_add(uuids.len()) <= CINNABAR_API_MAX_LOOKUP_XUIDS
        && xuids.iter().all(|id| bridge::valid_cinnabar_xuid(id))
        && uuids.iter().all(|id| bridge::valid_cinnabar_uuid(id))
}

/// Accepts the schema's shared clip identifier alphabet without imposing catalog ownership locally.
pub(super) fn valid_emote(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.:-".contains(&b))
}

/// Bounds a complete loadout replacement and rejects duplicate/malformed slot and item identities.
pub(super) fn valid_slots(slots: &[CinnabarCosmeticSlot]) -> bool {
    if slots.len() > 16 {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    slots.iter().all(|slot| {
        let slot_chars = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
        };
        let valid_slot = match slot.slot.split_once(':') {
            Some((namespace, name)) => slot_chars(namespace) && slot_chars(name),
            None => slot_chars(&slot.slot),
        };
        slot.slot.len() <= 48 && valid_slot && valid_item(&slot.item_id) && seen.insert(&slot.slot)
    })
}

/// Validates the shared cosmetic identifier, including nested catalog paths.
fn valid_item(id: &str) -> bool {
    let valid = |part: &str, punctuation: &[u8]| {
        !part.is_empty()
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || punctuation.contains(&byte)
            })
    };
    id.len() <= 128
        && id
            .split_once(':')
            .is_some_and(|(namespace, path)| valid(namespace, b"_.-") && valid(path, b"_./-"))
}

/// Admits one whole ownership snapshot, preserving first occurrence order and clearing invalid grants.
pub(super) fn admitted_ownership(mut ids: Vec<String>) -> Vec<String> {
    if ids.len() > cosmetics::MAX_ITEMS || ids.iter().any(|id| !valid_item(id)) {
        return Vec::new();
    }
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    ids
}

/// Matches the product's version spelling without permitting an arbitrary User-Agent/header value.
pub(super) fn valid_config(config: &bridge::CinnabarApiConfig) -> bool {
    let version = &config.client_version;
    let (number, suffix) = version
        .split_once('-')
        .map_or((version.as_str(), None), |(n, s)| (n, Some(s)));
    let mut parts = number.split('.');
    let valid_number = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    }) && parts.next().is_none();
    valid_number
        && version.len() <= 64
        && config.client_build <= MAX_BUILD
        && suffix.is_none_or(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        })
        && update_manifest::channel::Channel::parse(&config.channel).is_some()
}

/// Equal-jitter retry cap; advertised minima are applied separately and never reduced by this cap.
pub(super) fn backoff(failures: u32, cap: Duration, random: u64) -> Duration {
    let ceiling = Duration::from_secs(1)
        .checked_mul(
            1u32.checked_shl(failures.saturating_sub(1))
                .unwrap_or(u32::MAX),
        )
        .unwrap_or(cap)
        .min(cap);
    let half = ceiling / 2;
    let spread = u64::try_from((ceiling - half).as_nanos()).unwrap_or(u64::MAX);
    half + Duration::from_nanos(random % spread.saturating_add(1))
}
