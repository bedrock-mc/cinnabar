//! Shared asset identity and the explicit dependency before surface work can run.

use std::sync::{LazyLock, Mutex, OnceLock, Weak};

use super::producer::Executor;
use super::{
    admission::{Admission, JOB_OVERHEAD},
    *,
};

/// The retained allocation, rather than a reusable address, identifies prepared colours.
/// Assets and the completed colour table are immutable source context. They outlive every
/// dependent job and are released when the last producer using them goes away.
pub(super) struct PreparedAssets {
    assets: Arc<RuntimeAssets>,
    ready: OnceLock<Arc<SurfaceColours>>,
    running: AtomicBool,
    waiting: Mutex<Vec<Sender<Event>>>,
}

static PREPARED: LazyLock<Mutex<Vec<Weak<PreparedAssets>>>> = LazyLock::new(Mutex::default);

impl PreparedAssets {
    /// Shares preparation across feeds and dimensions while retaining the asset allocation.
    pub(super) fn shared(assets: Arc<RuntimeAssets>) -> Arc<Self> {
        let mut cached = PREPARED.lock().unwrap_or_else(|poison| poison.into_inner());
        cached.retain(|entry| entry.strong_count() != 0);
        if let Some(prepared) = cached
            .iter()
            .filter_map(Weak::upgrade)
            .find(|entry| Arc::ptr_eq(&entry.assets, &assets))
        {
            return prepared;
        }
        let prepared = Arc::new(Self {
            assets,
            ready: OnceLock::new(),
            running: AtomicBool::new(false),
            waiting: Mutex::default(),
        });
        cached.push(Arc::downgrade(&prepared));
        prepared
    }

    /// Tests identity while retaining the allocation, so addresses cannot be recycled.
    pub(super) fn matches(&self, assets: &Arc<RuntimeAssets>) -> bool {
        Arc::ptr_eq(&self.assets, assets)
    }

    /// Subscribes once per inbox so a deadline drain wakes after the dependency finishes.
    pub(super) fn subscribe(&self, tx: Sender<Event>) {
        let mut waiting = self
            .waiting
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if self.ready.get().is_none() {
            waiting.push(tx);
        }
    }

    /// Returns a prepared table without starting or waiting for an initializer.
    pub(super) fn colours(&self) -> Option<Arc<SurfaceColours>> {
        self.ready.get().cloned()
    }

    /// Admits exactly one finite preparation job. Consumers remain with their producers.
    /// No worker waits on another initializer. The table transfers directly to this retained
    /// source context; the job reservation covers its handles and publication metadata.
    pub(super) fn prepare(self: &Arc<Self>, admission: &Arc<Admission>, executor: &Executor) {
        if self.ready.get().is_some() || self.running.load(Ordering::Acquire) {
            return;
        }
        let Some(permit) = admission.acquire(JOB_OVERHEAD) else {
            return;
        };
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let guard = Preparing(Arc::clone(self));
        executor(Box::new(move || {
            let prepared = &guard.0;
            let colours = Arc::new(SurfaceColours::new(&prepared.assets));
            let waiting = {
                let mut waiting = prepared
                    .waiting
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                let _ = prepared.ready.set(colours);
                std::mem::take(&mut *waiting)
            };
            for tx in waiting {
                workers::send_result(&tx, Event::Prepared);
            }
            drop(guard);
            drop(permit);
        }));
    }
}

/// Makes an abandoned queued initializer eligible for retry, including executor rejection.
struct Preparing(Arc<PreparedAssets>);

impl Drop for Preparing {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

/// The dimension's registry and biome routing stay with the immutable source snapshot.
pub(super) struct PendingContext {
    pub(super) prepared: Arc<PreparedAssets>,
    pub(super) air: u32,
    pub(super) assets: Arc<RuntimeAssets>,
    pub(super) mode: NetworkIdMode,
    pub(super) tints: Arc<ResolvedBiomeTints>,
    pub(super) default_biome: u32,
}

impl PendingContext {
    /// Copies the context handles only once the shared dependency has completed.
    pub(super) fn ready(&self) -> Option<SurfaceContext> {
        Some(SurfaceContext {
            air: self.air,
            assets: Arc::clone(&self.assets),
            mode: self.mode,
            colours: self.prepared.colours()?,
            tints: Arc::clone(&self.tints),
            default_biome: self.default_biome,
        })
    }
}
