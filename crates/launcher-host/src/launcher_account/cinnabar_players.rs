//! Batched Cinnabar player lookups for the current server roster, paced under the API's rate limit.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use bridge::{
    BridgeError, CINNABAR_LOOKUP_RATE_LIMITED,
    contract::{CINNABAR_API_LOOKUPS_PER_MINUTE, CINNABAR_API_MAX_LOOKUP_XUIDS},
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};

use super::{Snapshot, publish, runtime};

/// Spacing that keeps lookups within the API's per-minute budget.
const LOOKUP_SPACING: Duration =
    Duration::from_millis(60_000 / CINNABAR_API_LOOKUPS_PER_MINUTE as u64);
/// How long a resolved XUID stays answered before it is asked again.
const RESOLVED_TTL: Duration = Duration::from_secs(120);
/// Pause after the core reports the API unavailable.
const UNAVAILABLE_RETRY: Duration = Duration::from_secs(15);
/// Pause after the API refuses a lookup over its rate limit.
const RATE_LIMITED_RETRY: Duration = Duration::from_secs(60);
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(20);
/// Most XUIDs the API accepts; longer strings are never real XUIDs.
const MAX_XUID_DIGITS: usize = 20;

/// One session's roster XUIDs, replaced whole on every roster change.
pub(super) struct Roster {
    pub session: u64,
    pub xuids: Vec<String>,
}

/// Which roster XUIDs to ask about next and what each lookup answered.
#[derive(Default)]
pub(super) struct Lookups {
    session: Option<u64>,
    roster: Vec<String>,
    resolved: HashMap<String, (Instant, bool)>,
    not_before: Option<Instant>,
}

impl Lookups {
    /// Replaces the roster with its deduplicated valid XUIDs; a new session forgets every answer.
    pub(super) fn set_roster(&mut self, roster: Roster) {
        if self.session != Some(roster.session) {
            self.session = Some(roster.session);
            self.resolved.clear();
        }
        let mut seen = HashSet::new();
        self.roster = roster
            .xuids
            .into_iter()
            .filter(|xuid| valid_xuid(xuid) && seen.insert(xuid.clone()))
            .collect();
    }

    /// The next batch of unresolved or expired XUIDs once the pacing allows another lookup.
    pub(super) fn next_batch(&mut self, now: Instant) -> Option<Vec<String>> {
        if self.not_before.is_some_and(|at| now < at) {
            return None;
        }
        let batch: Vec<String> = self
            .roster
            .iter()
            .filter(|xuid| self.due(xuid, now))
            .take(CINNABAR_API_MAX_LOOKUP_XUIDS)
            .cloned()
            .collect();
        if batch.is_empty() {
            return None;
        }
        self.not_before = Some(now + LOOKUP_SPACING);
        Some(batch)
    }

    /// Records a lookup's answer for every XUID it asked about.
    pub(super) fn resolve(&mut self, now: Instant, batch: &[String], cinnabar: &[String]) {
        let cinnabar: HashSet<&str> = cinnabar.iter().map(String::as_str).collect();
        for xuid in batch {
            self.resolved
                .insert(xuid.clone(), (now, cinnabar.contains(xuid.as_str())));
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
                Some((at, _)) => (*at + RESOLVED_TTL).saturating_duration_since(now),
                None => Duration::ZERO,
            })
            .min()
            .map(|due| due.max(pacing))
    }

    /// Roster XUIDs the API last reported as Cinnabar players.
    pub(super) fn players(&self) -> HashSet<String> {
        self.roster
            .iter()
            .filter(|xuid| self.resolved.get(*xuid).is_some_and(|(_, found)| *found))
            .cloned()
            .collect()
    }

    fn due(&self, xuid: &str, now: Instant) -> bool {
        self.resolved
            .get(xuid)
            .is_none_or(|(at, _)| now.saturating_duration_since(*at) >= RESOLVED_TTL)
    }
}

fn valid_xuid(xuid: &str) -> bool {
    !xuid.is_empty() && xuid.len() <= MAX_XUID_DIGITS && xuid.bytes().all(|b| b.is_ascii_digit())
}

/// Starts the lookup worker; it stops when the returned sender is dropped.
pub(super) fn start(socket_dir: PathBuf, shared: Arc<Mutex<Snapshot>>) -> Sender<Roster> {
    let (rosters, requests) = unbounded();
    thread::spawn(move || run(&socket_dir, &shared, &requests));
    rosters
}

fn run(socket_dir: &std::path::Path, shared: &Mutex<Snapshot>, requests: &Receiver<Roster>) {
    let Some(runtime) = runtime() else {
        return;
    };
    let mut lookups = Lookups::default();
    let mut published = HashSet::new();
    loop {
        let wait = lookups
            .next_wake(Instant::now())
            .unwrap_or(Duration::from_secs(3600));
        match requests.recv_timeout(wait) {
            Ok(roster) => {
                // Only the newest roster matters.
                let roster = requests.try_iter().last().unwrap_or(roster);
                lookups.set_roster(roster);
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Some(batch) = lookups.next_batch(Instant::now()) {
            let answer = runtime.block_on(async {
                tokio::time::timeout(
                    LOOKUP_TIMEOUT,
                    bridge::lookup_cinnabar_players(socket_dir, &batch),
                )
                .await
            });
            match answer {
                Ok(Ok(cinnabar)) => lookups.resolve(Instant::now(), &batch, &cinnabar),
                Ok(Err(BridgeError::ControlRpc { code, .. }))
                    if code == CINNABAR_LOOKUP_RATE_LIMITED =>
                {
                    lookups.fail(Instant::now(), RATE_LIMITED_RETRY);
                }
                _ => lookups.fail(Instant::now(), UNAVAILABLE_RETRY),
            }
        }
        let players = lookups.players();
        if players != published {
            published = players.clone();
            publish(shared, |snapshot| {
                snapshot.cinnabar_players = Arc::new(players);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(session: u64, xuids: &[&str]) -> Roster {
        Roster {
            session,
            xuids: xuids.iter().map(|xuid| (*xuid).to_owned()).collect(),
        }
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
        lookups.resolve(start, &batch, &["22".to_owned()]);
        assert_eq!(lookups.players(), HashSet::from(["22".to_owned()]));
        lookups.set_roster(roster(1, &["11", "22", "33"]));
        let batch = lookups.next_batch(start + LOOKUP_SPACING).unwrap();
        assert_eq!(batch, ["33"]);
        lookups.resolve(start + LOOKUP_SPACING, &batch, &[]);
        assert!(lookups.next_batch(start + 2 * LOOKUP_SPACING).is_none());
        let expired = start + RESOLVED_TTL;
        assert_eq!(lookups.next_batch(expired).unwrap(), ["11", "22"]);
    }

    /// Lookups never run closer together than the API's rate limit allows.
    #[test]
    fn lookups_are_paced_and_capped() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        let many: Vec<String> = (0..300).map(|n| n.to_string()).collect();
        lookups.set_roster(Roster {
            session: 1,
            xuids: many,
        });
        let first = lookups.next_batch(start).unwrap();
        assert_eq!(first.len(), CINNABAR_API_MAX_LOOKUP_XUIDS);
        assert!(lookups.next_batch(start).is_none());
        assert_eq!(lookups.next_wake(start), Some(LOOKUP_SPACING));
        assert!(LOOKUP_SPACING * CINNABAR_API_LOOKUPS_PER_MINUTE >= Duration::from_secs(60));
        lookups.fail(start, RATE_LIMITED_RETRY);
        assert!(lookups.next_batch(start + LOOKUP_SPACING).is_none());
        assert_eq!(
            lookups.next_batch(start + RATE_LIMITED_RETRY).unwrap(),
            first
        );
    }

    /// A new server session forgets every earlier answer.
    #[test]
    fn a_new_session_starts_over() {
        let start = Instant::now();
        let mut lookups = Lookups::default();
        lookups.set_roster(roster(1, &["11"]));
        let batch = lookups.next_batch(start).unwrap();
        lookups.resolve(start, &batch, &batch);
        lookups.set_roster(roster(2, &["11"]));
        assert!(lookups.players().is_empty());
        assert_eq!(lookups.next_batch(start + LOOKUP_SPACING).unwrap(), ["11"]);
        lookups.set_roster(roster(2, &[]));
        assert_eq!(lookups.next_wake(start), None);
    }
}
