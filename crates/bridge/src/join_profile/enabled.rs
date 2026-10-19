//! Clock and log ownership for diagnostic join attempts.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use web_time::Instant;

tokio::task_local! { static TASK: JoinProfile; }
thread_local! { static THREAD: std::cell::RefCell<Option<JoinProfile>> = const { std::cell::RefCell::new(None) }; }

static NEXT: AtomicU64 = AtomicU64::new(1);
static CURRENT: Mutex<JoinProfile> = Mutex::new(JoinProfile(None));

struct Attempt {
    id: u64,
    started: Instant,
    finished: AtomicBool,
    milestones: AtomicU64,
    presentation_pending: AtomicBool,
}

/// A captured attempt, safe to move to a network or compilation worker.
#[derive(Clone, Default)]
pub struct JoinProfile(Option<Arc<Attempt>>);

/// One overlapping section; its duration is independent of adjacent milestones.
pub struct Stage {
    profile: JoinProfile,
    name: &'static str,
    started: Instant,
}

impl JoinProfile {
    /// Starts at the accepted connection request, replacing any previous attempt.
    pub fn begin() -> Self {
        let profile = Self(Some(Arc::new(Attempt {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            started: Instant::now(),
            finished: AtomicBool::new(false),
            milestones: AtomicU64::new(0),
            presentation_pending: AtomicBool::new(false),
        })));
        let previous = std::mem::replace(
            &mut *CURRENT.lock().unwrap_or_else(|error| error.into_inner()),
            profile.clone(),
        );
        previous.finish("superseded");
        profile.mark("request_accepted");
        profile
    }

    /// Captures the current attempt before scheduling asynchronous work.
    pub fn current() -> Self {
        if let Some(profile) = THREAD.with(|slot| slot.borrow().clone()) {
            return profile;
        }
        if let Ok(profile) = TASK.try_with(Clone::clone) {
            return profile;
        }
        CURRENT
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Pins this attempt across future suspension and task migration.
    pub async fn in_scope<F: std::future::Future>(&self, future: F) -> F::Output {
        TASK.scope(self.clone(), future).await
    }

    /// Pins this attempt while a blocking or Rayon worker runs one closure.
    pub fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        let _binding = self.enter();
        work()
    }

    /// Binds this thread until the returned non-Send guard is dropped.
    pub fn enter(&self) -> Binding {
        let previous = THREAD.with(|slot| slot.replace(Some(self.clone())));
        Binding {
            previous,
            thread: std::marker::PhantomData,
        }
    }

    /// Returns the local correlation ID while this attempt remains open.
    pub fn id(&self) -> Option<u64> {
        self.0
            .as_ref()
            .filter(|p| !p.finished.load(Ordering::Relaxed))
            .map(|p| p.id)
    }

    /// Marks the loading gate, leaving the terminal timing to the actual render present.
    pub fn ready_for_presentation(&self) {
        if let Some(attempt) = &self.0
            && !attempt.presentation_pending.swap(true, Ordering::Relaxed)
        {
            self.mark("terrain_ready");
        }
    }

    /// Whether the next extracted gameplay frame should complete this attempt.
    pub fn presentation_pending(&self) -> bool {
        self.0.as_ref().is_some_and(|p| {
            !p.finished.load(Ordering::Relaxed) && p.presentation_pending.load(Ordering::Relaxed)
        })
    }

    /// Logs only the first observation of each bounded startup or terrain milestone.
    pub fn mark_once(&self, milestone: super::Milestone) {
        if let Some(attempt) = &self.0 {
            let bit = 1 << milestone as u8;
            if attempt.milestones.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
                self.mark(milestone.name());
            }
        }
    }

    /// Logs a milestone in milliseconds since the accepted request.
    pub fn mark(&self, stage: &'static str) {
        if let Some(attempt) = self
            .0
            .as_ref()
            .filter(|p| !p.finished.load(Ordering::Relaxed))
        {
            tracing::debug!(target: "join_profile", join_profile_id = attempt.id, stage,
                elapsed_ms = attempt.started.elapsed().as_secs_f64() * 1000.0, "join milestone");
        }
    }

    /// Starts an independently timed section; sections may overlap on workers.
    pub fn stage(&self, name: &'static str) -> Stage {
        self.mark(name);
        Stage {
            profile: self.clone(),
            name,
            started: Instant::now(),
        }
    }

    /// Logs the terminal milestone exactly once and closes only this handle.
    pub fn finish(&self, stage: &'static str) {
        if let Some(attempt) = &self.0
            && !attempt.finished.swap(true, Ordering::Relaxed)
        {
            tracing::debug!(target: "join_profile", join_profile_id = attempt.id, stage,
                elapsed_ms = attempt.started.elapsed().as_secs_f64() * 1000.0, "join finished");
        }
    }
}

impl Drop for Stage {
    /// Logs this section's duration without treating a dropped section as success.
    fn drop(&mut self) {
        if let Some(id) = self.profile.id() {
            tracing::debug!(target: "join_profile", join_profile_id = id, stage = self.name,
                duration_ms = self.started.elapsed().as_secs_f64() * 1000.0,
                elapsed_ms = self.profile.0.as_ref().unwrap().started.elapsed().as_secs_f64() * 1000.0,
                "join section ended");
        }
    }
}

/// Restores the previous profiling context on the same worker thread.
pub struct Binding {
    previous: Option<JoinProfile>,
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for Binding {
    /// Restores nesting even when a worker unwinds.
    fn drop(&mut self) {
        THREAD.with(|slot| slot.replace(self.previous.take()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finishing a cancelled attempt cannot finish a separately captured successor.
    #[test]
    fn finishing_one_handle_preserves_another_attempt() {
        let old = JoinProfile(Some(Arc::new(Attempt {
            id: 1,
            started: Instant::now(),
            finished: AtomicBool::new(false),
            milestones: AtomicU64::new(0),
            presentation_pending: AtomicBool::new(false),
        })));
        let new = JoinProfile(Some(Arc::new(Attempt {
            id: 2,
            started: Instant::now(),
            finished: AtomicBool::new(false),
            milestones: AtomicU64::new(0),
            presentation_pending: AtomicBool::new(false),
        })));
        let worker = old.clone();
        old.finish("cancelled");
        worker.finish("late_completion");
        assert_eq!(worker.id(), None);
        assert_eq!(new.id(), Some(2));
    }
    /// Nested workers restore the enclosing attempt rather than reading a newer global one.
    #[test]
    fn worker_context_restores_the_parent() {
        let first = JoinProfile(Some(Arc::new(Attempt {
            id: 11,
            started: Instant::now(),
            finished: AtomicBool::new(false),
            milestones: AtomicU64::new(0),
            presentation_pending: AtomicBool::new(false),
        })));
        let second = JoinProfile(Some(Arc::new(Attempt {
            id: 12,
            started: Instant::now(),
            finished: AtomicBool::new(false),
            milestones: AtomicU64::new(0),
            presentation_pending: AtomicBool::new(false),
        })));
        first.run(|| {
            assert_eq!(JoinProfile::current().id(), Some(11));
            second.run(|| assert_eq!(JoinProfile::current().id(), Some(12)));
            assert_eq!(JoinProfile::current().id(), Some(11));
        });
    }

    /// A suspended task retains its captured attempt when another task runs.
    #[tokio::test]
    async fn task_context_survives_suspension() {
        let profile = JoinProfile(Some(Arc::new(Attempt {
            id: 21,
            started: Instant::now(),
            finished: AtomicBool::new(false),
            milestones: AtomicU64::new(0),
            presentation_pending: AtomicBool::new(false),
        })));
        profile
            .in_scope(async {
                tokio::task::yield_now().await;
                assert_eq!(JoinProfile::current().id(), Some(21));
                let captured = JoinProfile::current();
                assert_eq!(
                    tokio::task::spawn_blocking(
                        move || captured.run(|| JoinProfile::current().id())
                    )
                    .await
                    .unwrap(),
                    Some(21)
                );
            })
            .await;
    }
}
