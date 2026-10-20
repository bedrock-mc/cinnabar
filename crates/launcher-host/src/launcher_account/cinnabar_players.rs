//! Batched Cinnabar player lookups for the current server roster, paced under the API's rate limit.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use bridge::{
    BridgeError,
    contract::{
        CINNABAR_API_CACHE_TTL_MS, CINNABAR_API_MAX_LOOKUP_XUIDS, RPC_CINNABAR_RATE_LIMITED,
    },
};
use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};

#[cfg(all(test, unix))]
use super::worker_runtime;
use super::{Snapshot, publish};
use tokio::runtime::Handle;
use tokio_util::task::TaskTracker;

/// A short gap coalesces roster churn; the core enforces the negotiated identifier budget.
const LOOKUP_SPACING: Duration = Duration::from_secs(3);
/// How long a resolved identifier stays answered before it is asked again.
const RESOLVED_TTL: Duration = Duration::from_millis(CINNABAR_API_CACHE_TTL_MS as u64);
/// Pause after the core reports the API unavailable.
const UNAVAILABLE_RETRY: Duration = Duration::from_secs(15);
/// Pause after the API refuses a lookup over its rate limit.
const RATE_LIMITED_RETRY: Duration = Duration::from_secs(60);
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(20);

/// One session's roster identifiers, replaced whole on every roster change.
pub(super) struct Roster {
    pub session: u64,
    pub identities: Vec<String>,
}

/// One answer's fixed refresh threshold and expiry, set when it arrives.
struct Answer {
    refresh_after: Instant,
    expires_at: Instant,
    found: bool,
}

/// Which roster identifiers to ask about next and what each lookup answered.
pub(super) struct Lookups {
    session: Option<u64>,
    roster: Vec<String>,
    resolved: HashMap<String, Answer>,
    not_before: Option<Instant>,
    /// Lifetime to assign to the next received answer, without changing retained answers.
    ttl: Duration,
    refresh: HashSet<String>,
}

impl Default for Lookups {
    /// Starts with the v1 fallback lifetime until a server advertises another one.
    fn default() -> Self {
        Self {
            session: None,
            roster: Vec::new(),
            resolved: HashMap::new(),
            not_before: None,
            ttl: RESOLVED_TTL,
            refresh: HashSet::new(),
        }
    }
}

impl Lookups {
    /// Replaces the roster with its deduplicated valid identifiers; a new session forgets every answer.
    pub(super) fn set_roster(&mut self, roster: Roster) {
        if self.session != Some(roster.session) {
            self.session = Some(roster.session);
            self.resolved.clear();
            self.refresh.clear();
            self.not_before = None;
        }
        let mut seen = HashSet::new();
        self.roster = roster
            .identities
            .into_iter()
            .filter(|xuid| {
                (bridge::valid_cinnabar_xuid(xuid) || bridge::valid_cinnabar_uuid(xuid))
                    && seen.insert(xuid.clone())
            })
            .collect();
        self.resolved.retain(|xuid, _| seen.contains(xuid));
        self.refresh.retain(|xuid| seen.contains(xuid));
        tracing::debug!(
            players = self.roster.len(),
            uuids = self
                .roster
                .iter()
                .filter(|id| bridge::valid_cinnabar_uuid(id))
                .count(),
            "Cinnabar roster accepted"
        );
    }

    /// The next batch of unresolved or expired identifiers once the pacing allows another lookup.
    pub(super) fn next_batch(&mut self, now: Instant) -> Option<Vec<String>> {
        if self.not_before.is_some_and(|at| now < at) {
            return None;
        }
        if self.refresh.is_empty()
            && self.roster.iter().any(|xuid| {
                self.resolved
                    .get(xuid)
                    .is_some_and(|answer| now >= answer.expires_at)
            })
        {
            // Freeze the round until every negotiated prefix has answered, even past its TTL.
            self.refresh.extend(
                self.roster
                    .iter()
                    .filter(|xuid| {
                        self.resolved
                            .get(*xuid)
                            .is_some_and(|answer| now > answer.refresh_after)
                    })
                    .cloned(),
            );
        }
        // Unanswered joiners go first, even during a large refresh round.
        let batch: Vec<String> = self
            .roster
            .iter()
            .filter(|xuid| !self.resolved.contains_key(*xuid))
            .chain(
                self.roster
                    .iter()
                    .filter(|xuid| self.refresh.contains(*xuid)),
            )
            .take(CINNABAR_API_MAX_LOOKUP_XUIDS)
            .cloned()
            .collect();
        if batch.is_empty() {
            return None;
        }
        self.not_before = Some(now + LOOKUP_SPACING);
        Some(batch)
    }

    /// Records a lookup's answer for every identifier it asked about.
    pub(super) fn resolve(
        &mut self,
        now: Instant,
        session: u64,
        batch: &[String],
        cinnabar: &[String],
    ) {
        if self.session != Some(session) {
            return;
        }
        let cinnabar: HashSet<&str> = cinnabar.iter().map(String::as_str).collect();
        for xuid in batch {
            if !self.roster.contains(xuid) {
                continue;
            }
            self.refresh.remove(xuid);
            self.resolved.insert(
                xuid.clone(),
                Answer {
                    refresh_after: now + self.ttl / 2,
                    expires_at: now + self.ttl,
                    found: cinnabar.contains(xuid.as_str()),
                },
            );
        }
    }

    /// Defers the next lookup after a failure; the batch stays unresolved.
    pub(super) fn fail(&mut self, now: Instant, retry_after: Duration) {
        let retry = now + retry_after;
        self.not_before = Some(self.not_before.map_or(retry, |at| at.max(retry)));
    }

    /// How long until another lookup could be due, or `None` with nothing left to ask.
    pub(super) fn next_wake(&self, now: Instant) -> Option<Duration> {
        let pacing = self
            .not_before
            .map_or(Duration::ZERO, |at| at.saturating_duration_since(now));
        self.roster
            .iter()
            .map(|xuid| match self.resolved.get(xuid) {
                Some(answer) if !self.refresh.contains(xuid) => {
                    answer.expires_at.saturating_duration_since(now)
                }
                Some(_) => Duration::ZERO,
                None => Duration::ZERO,
            })
            .min()
            .map(|due| due.max(pacing))
    }

    /// Roster XUIDs the API last reported as Cinnabar players.
    pub(super) fn players(&self, now: Instant) -> HashSet<String> {
        self.roster
            .iter()
            .filter(|xuid| {
                self.resolved
                    .get(*xuid)
                    .is_some_and(|answer| answer.found && now < answer.expires_at)
            })
            .cloned()
            .collect()
    }

    /// The next visible positive's expiry, even when a request is pending or retries are deferred.
    fn next_expiry(&self, now: Instant) -> Option<Duration> {
        self.resolved
            .values()
            .filter(|answer| answer.found && answer.expires_at > now)
            .map(|answer| answer.expires_at.saturating_duration_since(now))
            .min()
    }
}

/// Starts the lookup worker; it stops when the returned sender is dropped.
pub(super) fn start(
    socket_dir: Option<PathBuf>,
    shared: Arc<Mutex<Snapshot>>,
    runtime: Handle,
    tasks: TaskTracker,
) -> Sender<Roster> {
    let (rosters, requests) = unbounded();
    thread::spawn(crate::worker::tracked(
        move || run(socket_dir.as_deref(), &shared, &requests, &runtime),
        &tasks,
    ));
    rosters
}

/// Schedules lookups and expirations while the network thread handles a single pending request.
fn run(
    socket_dir: Option<&std::path::Path>,
    shared: &Mutex<Snapshot>,
    requests: &Receiver<Roster>,
    runtime: &Handle,
) {
    thread::scope(|scope| {
        let (send, work) = bounded::<Vec<String>>(1);
        let (results, answers) = bounded(1);
        scope.spawn(move || {
            while let Ok(batch) = work.recv() {
                let answer = runtime.block_on(async {
                    tokio::time::timeout(LOOKUP_TIMEOUT, async {
                        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                        {
                            let api = shared
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .native_product_api
                                .clone();
                            if let Some(api) = api {
                                return api.lookup(&batch).await;
                            }
                        }
                        bridge::lookup_cinnabar_players(
                            super::workers::require_core_directory(socket_dir)?,
                            &batch,
                        )
                        .await
                    })
                    .await
                });
                if results.send(answer).is_err() {
                    break;
                }
            }
        });
        let mut lookups = Lookups::default();
        let mut pending: Option<(u64, u64)> = None;
        let mut generation = super::auth_generation(shared);
        loop {
            if generation != super::auth_generation(shared) {
                generation = super::auth_generation(shared);
                lookups.resolved.clear();
                lookups.refresh.clear();
                lookups.not_before = None;
            }
            let now = Instant::now();
            let due = pending.is_none().then(|| lookups.next_wake(now)).flatten();
            let wait = due
                .into_iter()
                .chain(lookups.next_expiry(now))
                .min()
                .unwrap_or(Duration::from_secs(3600));
            select! {
                recv(requests) -> roster => {
                    let Ok(roster) = roster else { return; };
                    lookups.set_roster(requests.try_iter().last().unwrap_or(roster));
                }
                recv(answers) -> answer => {
                    let Some((session,requested_generation)) = pending.take() else { return; };
                    if lookups.session == Some(session) && requested_generation==generation && generation==super::auth_generation(shared) {
                        match answer {
                            Ok(Ok(Ok(mut answer))) => {
                                tracing::debug!(resolved = answer.resolved.len(), resolved_uuids = answer.resolved_uuids.len(), found = answer.cinnabar.len(), found_uuids = answer.cinnabar_uuids.len(), "Cinnabar lookup completed");
                                answer.resolved.extend(answer.resolved_uuids);
                                answer.cinnabar.extend(answer.cinnabar_uuids);
                                lookups.ttl = Duration::from_millis(answer.cache_ttl_ms.max(1));
                                lookups.resolve(Instant::now(), session, &answer.resolved, &answer.cinnabar);
                            }
                            Ok(Ok(Err(BridgeError::ControlRpc { code, .. }))) if code == RPC_CINNABAR_RATE_LIMITED => {
                                tracing::debug!("Cinnabar lookup rate limited");
                                lookups.fail(Instant::now(), RATE_LIMITED_RETRY);
                            }
                            failure => {
                                let outcome = match failure {
                                    Err(_) => "worker_stopped",
                                    Ok(Err(_)) => "timeout",
                                    Ok(Ok(Err(BridgeError::ControlRpc { .. }))) => "control_rejected",
                                    _ => "control_failed",
                                };
                                tracing::debug!(outcome, "Cinnabar lookup failed");
                                lookups.fail(Instant::now(), UNAVAILABLE_RETRY);
                            }
                        }
                    } else {
                        tracing::debug!("Cinnabar lookup discarded after session change");
                    }
                }
                default(wait) => {}
            }
            // Adopt roster changes before applying or publishing an answer from the previous session.
            if let Some(roster) = requests.try_iter().last() {
                lookups.set_roster(roster);
            }
            let now = Instant::now();
            publish(shared, |snapshot| {
                if snapshot.auth_generation == generation
                    && snapshot.cinnabar_roster_key.map(|key| key.0) == lookups.session
                {
                    let players = lookups.players(now);
                    if *snapshot.cinnabar_players != players {
                        tracing::debug!(found = players.len(), "Cinnabar badges published");
                        snapshot.cinnabar_players = Arc::new(players);
                    }
                }
            });
            if pending.is_none()
                && let Some(batch) = lookups.next_batch(now)
            {
                pending = lookups.session.map(|session| (session, generation));
                tracing::debug!(
                    players = batch.len(),
                    uuids = batch
                        .iter()
                        .filter(|id| bridge::valid_cinnabar_uuid(id))
                        .count(),
                    "Cinnabar lookup started"
                );
                if send.send(batch).is_err() {
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(session: u64, xuids: &[&str]) -> Roster {
        Roster {
            session,
            identities: xuids.iter().map(|xuid| (*xuid).to_owned()).collect(),
        }
    }

    /// UUID-only roster identities must be requested and cached just like XUIDs.
    #[test]
    fn uuid_only_roster_gets_looked_up_and_published() {
        let now = Instant::now();
        let uuid = "00112233-4455-6677-8899-aabbccddeeff";
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &[uuid]));
        let batch = lookups
            .next_batch(now)
            .expect("UUID-only roster was discarded");
        assert_eq!(batch, [uuid]);
        lookups.resolve(now, 1, &batch, &batch);
        assert_eq!(lookups.players(now), HashSet::from([uuid.into()]));
    }

    /// Mixed rosters ask once per key and cache XUID and UUID answers independently.
    #[test]
    fn mixed_roster_caches_each_identifier() {
        let now = Instant::now();
        let uuid = "00112233-4455-6677-8899-aabbccddeeff";
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(
            1,
            &["11", uuid, "00112233-4455-6677-8899-AABBCCDDEEFF"],
        ));
        let batch = lookups.next_batch(now).unwrap();
        assert_eq!(batch, ["11", uuid]);
        lookups.resolve(now, 1, &batch, &[uuid.into()]);
        assert_eq!(lookups.players(now), HashSet::from([uuid.into()]));
        assert!(lookups.next_batch(now + LOOKUP_SPACING).is_none());
    }

    /// A resolved XUID is not asked again until its answer expires, and only valid unique XUIDs are sent.
    #[test]
    fn resolved_xuids_wait_for_their_ttl() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(
            1,
            &["11", "22", "11", "", "x9", "123456789012345678901"],
        ));
        let batch = lookups.next_batch(start).unwrap();
        assert_eq!(batch, ["11", "22"]);
        lookups.resolve(start, 1, &batch, &["22".to_owned()]);
        assert_eq!(lookups.players(start), HashSet::from(["22".to_owned()]));
        lookups.set_roster(roster(1, &["11", "22", "33"]));
        let batch = lookups.next_batch(start + LOOKUP_SPACING).unwrap();
        assert_eq!(batch, ["33"]);
        lookups.resolve(start + LOOKUP_SPACING, 1, &batch, &[]);
        assert!(lookups.next_batch(start + 2 * LOOKUP_SPACING).is_none());
        let expired = start + RESOLVED_TTL;
        assert_eq!(lookups.next_batch(expired).unwrap(), ["11", "22", "33"]);
    }

    /// Lookups never run closer together than the API's rate limit allows.
    #[test]
    fn lookups_are_paced_and_capped() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        let many: Vec<String> = (0..300).map(|n| n.to_string()).collect();
        lookups.set_roster(Roster {
            session: 1,
            identities: many,
        });
        let first = lookups.next_batch(start).unwrap();
        assert_eq!(first.len(), CINNABAR_API_MAX_LOOKUP_XUIDS);
        assert!(lookups.next_batch(start).is_none());
        assert_eq!(lookups.next_wake(start), Some(LOOKUP_SPACING));

        lookups.fail(start, RATE_LIMITED_RETRY);
        assert!(lookups.next_batch(start + LOOKUP_SPACING).is_none());
        assert_eq!(
            lookups.next_batch(start + RATE_LIMITED_RETRY).unwrap(),
            first
        );
    }

    /// A refresh also covers answers past half their lifetime, avoiding staggered expiry requests.
    #[test]
    fn refresh_coalesces_older_answers() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11", "22"]));
        lookups.resolve(start, 1, &["11".into()], &[]);
        lookups.resolve(start + RESOLVED_TTL / 4, 1, &["22".into()], &[]);
        assert_eq!(
            lookups.next_batch(start + RESOLVED_TTL).unwrap(),
            ["11", "22"]
        );
    }

    /// Negotiated one-XUID replies must reach the tail even when a round exceeds the cache TTL.
    #[test]
    fn refresh_round_drains_before_refreshed_players_are_reenqueued() {
        let start = Instant::now();
        let xuids: Vec<String> = (1..=80).map(|id| id.to_string()).collect();
        let mut lookups = Lookups::default();
        lookups.set_roster(Roster {
            session: 1,
            identities: xuids.clone(),
        });
        lookups.resolve(start, 1, &xuids, &xuids);
        let mut now = start + RESOLVED_TTL;
        for expected in &xuids {
            let offered = lookups.next_batch(now).unwrap();
            assert_eq!(offered.first(), Some(expected));
            // Only the negotiated prefix was sent by the core and answered by the server.
            lookups.resolve(now, 1, &offered[..1], &offered[..1]);
            now += LOOKUP_SPACING;
        }
        assert!(lookups.refresh.is_empty());
        assert_eq!(lookups.next_batch(now).unwrap().first(), xuids.first());
    }

    /// Roster churn and a delayed retry keep the current round while new players remain first.
    #[test]
    fn refresh_round_survives_retry_and_prioritizes_new_joiners() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11", "22", "33"]));
        lookups.resolve(start, 1, &["11".into(), "22".into(), "33".into()], &[]);
        let now = start + RESOLVED_TTL;
        let first = lookups.next_batch(now).unwrap();
        lookups.resolve(now, 1, &first[..1], &[]);
        lookups.set_roster(roster(1, &["11", "22", "44"]));
        lookups.fail(now, RESOLVED_TTL);
        assert!(lookups.next_batch(now + LOOKUP_SPACING).is_none());
        assert_eq!(
            lookups.next_batch(now + RESOLVED_TTL).unwrap(),
            ["44", "22"]
        );
    }

    /// Failed refreshes never extend a visible badge's lifetime.
    #[test]
    fn positive_expires_during_retry_backoff() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11"]));
        lookups.resolve(start, 1, &["11".into()], &["11".into()]);
        lookups.fail(start, RESOLVED_TTL * 2);
        assert!(
            !lookups
                .players(start + RESOLVED_TTL - Duration::from_millis(1))
                .is_empty()
        );
        assert!(lookups.players(start + RESOLVED_TTL).is_empty());
        assert_eq!(lookups.next_expiry(start), Some(RESOLVED_TTL));
    }

    /// A longer lifetime on a new reply cannot revive expired positive or negative answers.
    #[test]
    fn a_larger_negotiated_ttl_does_not_revive_expired_answers() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11", "22", "33"]));
        lookups.resolve(start, 1, &["11".into(), "22".into()], &["11".into()]);
        let now = start + RESOLVED_TTL;
        assert!(lookups.players(now).is_empty());
        lookups.ttl = RESOLVED_TTL * 4;
        lookups.resolve(now, 1, &["33".into()], &["33".into()]);
        assert_eq!(lookups.players(now), HashSet::from(["33".into()]));
        assert_eq!(lookups.next_expiry(now), Some(RESOLVED_TTL * 4));
        assert_eq!(lookups.next_wake(now), Some(Duration::ZERO));
        assert_eq!(lookups.next_batch(now).unwrap(), ["11", "22"]);
    }

    /// A shorter lifetime affects only the new answer, including its refresh scheduling.
    #[test]
    fn a_smaller_negotiated_ttl_does_not_shorten_retained_answers() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11", "22", "33"]));
        lookups.resolve(start, 1, &["11".into(), "22".into()], &["11".into()]);
        let received = start + LOOKUP_SPACING;
        lookups.ttl = RESOLVED_TTL / 4;
        lookups.resolve(received, 1, &["33".into()], &["33".into()]);
        assert_eq!(lookups.next_expiry(received), Some(RESOLVED_TTL / 4));
        assert_eq!(lookups.next_wake(received), Some(RESOLVED_TTL / 4));
        let now = received + RESOLVED_TTL / 4;
        assert_eq!(lookups.players(now), HashSet::from(["11".into()]));
        assert_eq!(lookups.next_expiry(now), Some((start + RESOLVED_TTL) - now));
        assert_eq!(lookups.next_batch(now).unwrap(), ["33"]);
        assert!(lookups.players(start + RESOLVED_TTL).is_empty());
    }

    /// Departed players and answers from older sessions cannot reappear in the cache.
    #[test]
    fn late_answers_and_departed_xuids_are_discarded() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11", "22"]));
        lookups.resolve(start, 1, &["11".into()], &["11".into()]);
        lookups.set_roster(roster(1, &["22"]));
        assert!(!lookups.resolved.contains_key("11"));
        lookups.resolve(start, 1, &["11".into()], &["11".into()]);
        assert!(!lookups.resolved.contains_key("11"));
        lookups.set_roster(roster(2, &["22"]));
        lookups.resolve(start, 1, &["22".into()], &["22".into()]);
        assert!(lookups.players(start).is_empty());
    }

    /// A new server session forgets every earlier answer.
    #[test]
    fn a_new_session_starts_over() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11"]));
        let batch = lookups.next_batch(start).unwrap();
        lookups.resolve(start, 1, &batch, &batch);
        lookups.set_roster(roster(2, &["11"]));
        assert!(lookups.players(start).is_empty());
        assert_eq!(lookups.next_batch(start + LOOKUP_SPACING).unwrap(), ["11"]);
        lookups.set_roster(roster(2, &[]));
        assert_eq!(lookups.next_wake(start), None);
    }
}

#[cfg(all(test, unix))]
mod worker_tests;
