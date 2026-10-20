//! One tracked coordinator for the selected account's actual local-world host lifetime.

use super::*;
use crate::{
    local_worlds::NativeHostIntent,
    native_account::{HostedInvitation, NativeAccount},
};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) enum Desired {
    Idle,
    Host(NativeHostIntent),
    Stop,
}

/// Coalesces frame observations of the same physical opening without restarting acquisition.
pub(super) fn set(desired: &watch::Sender<Desired>, intent: Option<NativeHostIntent>) {
    desired.send_if_modified(|current| {
        if matches!(current, Desired::Stop) {
            return false;
        }
        let same = match (&*current, &intent) {
            (Desired::Idle, None) => true,
            (Desired::Host(old), Some(new)) => old.same(new),
            _ => false,
        };
        if same {
            return false;
        }
        *current = intent.map_or(Desired::Idle, Desired::Host);
        true
    });
}

/// Retires the actual worker independently of cloned snapshot observers.
pub(super) fn stop(desired: &watch::Sender<Desired>) {
    desired.send_replace(Desired::Stop);
}

/// Clears intent on account retirement without restarting a terminal worker.
pub(super) fn retire(desired: &watch::Sender<Desired>) {
    desired.send_if_modified(|current| {
        if matches!(current, Desired::Stop | Desired::Idle) {
            return false;
        }
        *current = Desired::Idle;
        true
    });
}

/// Starts only for the existing native selected owner and borrows the app reactor and tracker.
/// The original process bootstrap can block on its bounded pipe, so it stays off Bevy and reactor threads.
pub(super) fn start(shared: Arc<Mutex<Snapshot>>, runtime: Handle, tasks: &TaskTracker) {
    let (desired, changed) = watch::channel(Desired::Idle);
    let account = {
        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
        snapshot.native_host_wake = Some(desired.clone());
        snapshot.native_presence_account.clone()
    };
    if let Some(account) = account {
        thread::spawn(crate::worker::tracked(
            move || runtime.block_on(run(account, shared, changed)),
            tasks,
        ));
    }
}

/// Keeps each actual phase pinned across unrelated account/menu wakeups.
async fn selected<T>(
    phase: impl std::future::Future<Output = T>,
    intent: &NativeHostIntent,
    changed: &mut watch::Receiver<Desired>,
) -> Option<T> {
    tokio::pin!(phase);
    loop {
        let current =
            matches!(&*changed.borrow_and_update(), Desired::Host(current) if intent.same(current));
        if !current {
            return None;
        }
        tokio::select! { biased;
            result = changed.changed() => if result.is_err() { return None; },
            result = &mut phase => return Some(result),
        }
    }
}

/// Publishes only actual active incarnation commands; no account mutex crosses service IO.
fn publish(
    shared: &Mutex<Snapshot>,
    intent: &NativeHostIntent,
    invitation: Option<HostedInvitation>,
) {
    let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
    let current = snapshot.native_host_wake.as_ref().is_some_and(
        |wake| matches!(&*wake.borrow(), Desired::Host(current) if intent.same(current)),
    );
    snapshot.native_host_invitation = invitation.filter(|_| current);
}

/// Owns activation, active progress and bounded cleanup; failures wait for a new explicit intent.
async fn run(
    account: Arc<NativeAccount>,
    shared: Arc<Mutex<Snapshot>>,
    mut changed: watch::Receiver<Desired>,
) {
    let mut retiring: Option<NativeHostIntent> = None;
    loop {
        let desired = changed.borrow_and_update().clone();
        if let Some(previous) = &retiring {
            if previous.cleanup(deadline()).await.is_err() {
                tracing::warn!("native hosted membership cleanup is unconfirmed");
                if matches!(desired, Desired::Stop) {
                    return;
                }
                if changed.changed().await.is_err() {
                    return;
                }
                continue;
            }
            retiring = None;
        }
        let intent = match desired {
            Desired::Host(intent) => intent,
            Desired::Stop => return,
            Desired::Idle => {
                if changed.changed().await.is_err() {
                    return;
                }
                continue;
            }
        };
        retiring = Some(intent.clone());
        let cutoff = deadline();
        let activation = async {
            let world = intent.open(Arc::clone(&account), cutoff).await?;
            world.activate(cutoff).await
        };
        let mut cleanup_cutoff = None;
        if let Some(Ok(mut active)) = selected(activation, &intent, &mut changed).await {
            publish(&shared, &intent, Some(active.invitation()));
            let result = selected(active.run(), &intent, &mut changed).await;
            publish(&shared, &intent, None);
            let cutoff = deadline();
            cleanup_cutoff = Some(cutoff);
            let mut closing = active.begin_close(cutoff);
            if closing.finish_close().await.is_err() {
                tracing::warn!("native hosted world cleanup is unconfirmed");
            }
            if let Some(Err(error)) = result {
                tracing::debug!(kind = ?error.kind(), "native hosted world ended");
            }
        } else {
            publish(&shared, &intent, None);
            tracing::debug!("native hosted world activation did not complete");
        }
        // Finish the original obligation immediately, even if activation failed before Active.
        if intent
            .cleanup(cleanup_cutoff.unwrap_or_else(deadline))
            .await
            .is_ok()
        {
            retiring = None;
        } else {
            tracing::warn!("native hosted membership cleanup is unconfirmed");
        }
        // Cancellation uses the replacement intent immediately; a failed unchanged intent is not retried.
        let unchanged =
            matches!(&*changed.borrow(), Desired::Host(current) if intent.same(current));
        if unchanged && changed.changed().await.is_err() {
            return;
        }
    }
}

/// Gives each explicit activation/cleanup decision a finite application edge budget.
fn deadline() -> Instant {
    Instant::now() + crate::native_account::ACQUISITION_TIMEOUT
}
