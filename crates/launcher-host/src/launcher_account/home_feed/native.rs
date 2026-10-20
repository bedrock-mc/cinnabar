//! One native Home refresh/cache and exact-session report on the existing workers.

use super::*;
use crate::native_account::{NativeAccount, NativeHomeScope};
use tokio::time::Instant;

const REFRESH: Duration = Duration::from_secs(bridge::contract::HOME_FEED_REFRESH_SECONDS as u64);
const CACHE_TTL: Duration = Duration::from_secs(bridge::contract::HOME_FEED_CACHE_SECONDS as u64);

#[derive(Default)]
pub(super) struct Cache {
    generation: Option<u64>,
    account: Option<Arc<NativeAccount>>,
    language: String,
    home: Option<Home>,
    scope: Option<NativeHomeScope>,
    attempted: Option<Instant>,
    fetched: Option<Instant>,
    reported: HashSet<String>,
}

impl Cache {
    /// Preserves same-account partial results and the original 15-minute attempt interval.
    pub(super) async fn round(
        &mut self,
        account: &Option<Arc<NativeAccount>>,
        art: Option<&artwork::FeedArt>,
        shared: &Mutex<Snapshot>,
        selected: &Selection,
        changed: &mut watch::Receiver<Wake>,
    ) -> (Option<Home>, bool) {
        let Some(account) = account else {
            *self = Self::default();
            return (None, true);
        };
        if self.generation != Some(selected.generation)
            || self.language != selected.language
            || self
                .account
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, account))
        {
            *self = Self {
                generation: Some(selected.generation),
                account: Some(Arc::clone(account)),
                language: selected.language.clone(),
                ..Default::default()
            };
        }
        let now = Instant::now();
        if self.home.is_none()
            || (self
                .fetched
                .is_none_or(|at| now.duration_since(at) >= CACHE_TTL)
                && self
                    .attempted
                    .is_none_or(|at| now.duration_since(at) >= CACHE_TTL))
        {
            self.attempted = Some(now);
            let Some(loaded) = wait(
                Arc::clone(account).read_home(now + REFRESH, selected.language.clone()),
                shared,
                selected,
                changed,
            )
            .await
            else {
                return (None, true);
            };
            match loaded {
                Ok(mut loaded) => {
                    let empty = Home::default();
                    let same_session = self
                        .scope
                        .as_ref()
                        .is_some_and(|old| old.same_session(&loaded.scope));
                    let previous = if same_session {
                        self.home.as_ref().unwrap_or(&empty)
                    } else {
                        &empty
                    };
                    let Some(art) = art else {
                        return (None, true);
                    };
                    let Some((mut home, any_success)) = loaded.apply(previous, art.images()) else {
                        return (None, true);
                    };
                    if any_success {
                        art.fill_cached(artwork::home_slots(&mut home));
                        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                        if *changed.borrow() == Wake::Stop || !selected.current(&snapshot) {
                            return (None, true);
                        }
                        if !same_session {
                            self.reported.clear();
                        }
                        self.scope = Some(loaded.scope.clone());
                        self.home = Some(home.clone());
                        self.fetched = Some(Instant::now());
                        snapshot.home = Some(home);
                        snapshot.pending_home_scope = Scope::Native(loaded.scope.clone());
                        // Keep the verified Operation alive through this final publication.
                        drop(snapshot);
                        drop(loaded);
                    }
                }
                Err(_) => tracing::warn!("native Home refresh unavailable"),
            }
        }
        let (Some(home), Some(scope)) = (&mut self.home, &self.scope) else {
            return (None, true);
        };
        // Busy credential ownership defers this publication; it does not discard the cache.
        if !scope.current() {
            return (None, false);
        }
        if let Some(art) = art {
            art.fill_cached(artwork::home_slots(home));
        }
        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
        if *changed.borrow() == Wake::Stop || !selected.current(&snapshot) {
            return (None, true);
        }
        snapshot.home = Some(home.clone());
        snapshot.pending_home_scope = Scope::Native(scope.clone());
        for message in &home.messages {
            if SHOWN_SURFACES.contains(&message.surface.as_str())
                && self.reported.insert(message.instance_id.clone())
            {
                let event = MessageEvent {
                    event_type: bridge::contract::MESSAGE_EVENT_IMPRESSION.to_owned(),
                    instance_id: message.instance_id.clone(),
                    report_id: message.report_id.clone(),
                    button_id: String::new(),
                };
                if let Some(reports) = &snapshot.home_reports {
                    let _ = reports.send(Report {
                        selected: Selection::capture(&snapshot),
                        scope: Scope::Native(scope.clone()),
                        event,
                    });
                }
            }
        }
        // Native impressions use the same scoped queue; only Core reports inline.
        (None, false)
    }
}

/// Preparation remains cancelable; once polled, a possible report is observed to its first cutoff.
pub(super) async fn report(
    account: &Arc<NativeAccount>,
    scope: &NativeHomeScope,
    event: &MessageEvent,
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    changed: &mut watch::Receiver<Wake>,
) -> ReportOutcome {
    let deadline = Instant::now() + REFRESH;
    let prepared = wait(
        tokio::time::timeout_at(deadline, Arc::clone(account).prepare_home_report(scope)),
        shared,
        selected,
        changed,
    )
    .await;
    let prepared = match prepared {
        Some(Ok(Ok(Some(prepared)))) => prepared,
        Some(Ok(Ok(None))) | None => return ReportOutcome::Retired,
        Some(Ok(Err(error)))
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || crate::native_account::requires_sign_in(&error) =>
        {
            return ReportOutcome::Retired;
        }
        _ => return ReportOutcome::Retry,
    };
    if Instant::now() >= deadline
        || *changed.borrow() == Wake::Stop
        || !selected.current(&shared.lock().unwrap_or_else(|p| p.into_inner()))
    {
        return ReportOutcome::Retired;
    }
    // No Stop/select cancellation after this point: Drop/persistence and this finite cutoff own the possible POST.
    match tokio::time::timeout_at(deadline, prepared.run(event)).await {
        Ok(Ok(())) => ReportOutcome::Complete,
        _ => {
            tracing::warn!("native inbox event failed; retaining original scope for retry");
            ReportOutcome::Retry
        }
    }
}
