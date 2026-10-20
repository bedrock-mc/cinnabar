//! One original deadline and one observed write, without a detached membership operation.

use super::*;
use tokio::sync::mpsc::UnboundedReceiver;

/// Keeps preparation cancelable and a possible native write owned until its original cutoff.
pub(super) async fn run(
    socket_dir: Option<PathBuf>,
    shared: Arc<Mutex<Snapshot>>,
    mut requests: UnboundedReceiver<Request>,
    mut changed: watch::Receiver<Wake>,
) {
    let mut pending = None;
    loop {
        if *changed.borrow_and_update() == Wake::Stop {
            return;
        }
        let request = match pending.take() {
            Some(request) => request,
            None => match requests.recv().await {
                Some(request) => request,
                None => return,
            },
        };
        let Request::Run {
            ticket,
            code,
            accept,
            selected,
        } = request
        else {
            continue;
        };
        let deadline = tokio::time::Instant::now() + profile_worker::RESPONSE_TIMEOUT;
        let prepared = wait(
            tokio::time::timeout_at(deadline, selected.source.prepare(&code)),
            &shared,
            &selected,
            &mut requests,
            &mut pending,
            &mut changed,
        )
        .await;
        let result = match prepared {
            Some(Ok(Ok(prepared))) => {
                if *changed.borrow() == Wake::Stop
                    || !selected.current(&shared.lock().unwrap_or_else(|p| p.into_inner()))
                {
                    continue;
                }
                let observes = prepared.observes_acceptance(accept);
                let phase =
                    tokio::time::timeout_at(deadline, prepared.run(socket_dir.as_deref(), accept));
                let result = if observes {
                    observe(phase, &mut requests, &mut pending, &mut changed).await
                } else {
                    wait(
                        phase,
                        &shared,
                        &selected,
                        &mut requests,
                        &mut pending,
                        &mut changed,
                    )
                    .await
                };
                match result {
                    Some(Ok(Ok(loaded))) if tokio::time::Instant::now() < deadline => {
                        match wait(
                            tokio::time::timeout_at(deadline, loaded.into_current()),
                            &shared,
                            &selected,
                            &mut requests,
                            &mut pending,
                            &mut changed,
                        )
                        .await
                        {
                            Some(Ok(result)) if tokio::time::Instant::now() < deadline => result,
                            Some(_) => Err(()),
                            None => continue,
                        }
                    }
                    Some(_) => Err(()),
                    None => continue,
                }
            }
            Some(_) => Err(()),
            None => continue,
        };
        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
        if *changed.borrow() == Wake::Stop || !selected.current(&snapshot) {
            continue;
        }
        let result = result.map(|reply| {
            if accept {
                snapshot.realm_catalog_revision = snapshot.realm_catalog_revision.wrapping_add(1);
                if let Some(realm) = reply.catalog {
                    let realms = snapshot.realms.get_or_insert_with(Vec::new);
                    if let Some(existing) = realms.iter_mut().find(|old| old.target == realm.target)
                    {
                        *existing = realm;
                    } else {
                        realms.push(realm);
                    }
                }
                if let Some(wake) = &snapshot.catalog_wake {
                    let _ = wake.try_send(());
                }
            }
            (reply.code, reply.card)
        });
        snapshot.realm_membership = Some((ticket, accept, result));
    }
}

/// Retains the same preparation/read future across unrelated account wakes.
async fn wait<T>(
    phase: impl std::future::Future<Output = T>,
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    requests: &mut UnboundedReceiver<Request>,
    pending: &mut Option<Request>,
    changed: &mut watch::Receiver<Wake>,
) -> Option<T> {
    tokio::pin!(phase);
    loop {
        if *changed.borrow_and_update() == Wake::Stop
            || !selected.current(&shared.lock().unwrap_or_else(|p| p.into_inner()))
        {
            return None;
        }
        tokio::select! { biased;
            request = requests.recv() => {
                if let Some(request @ Request::Run { .. }) = request { *pending = Some(request); }
                return None;
            }
            wake = changed.changed() => if wake.is_err() { return None; },
            result = &mut phase => return Some(result),
        }
    }
}

/// Observes a possible POST without retrying it; retirement hides only its eventual UI result.
async fn observe<T>(
    phase: impl std::future::Future<Output = T>,
    requests: &mut UnboundedReceiver<Request>,
    pending: &mut Option<Request>,
    changed: &mut watch::Receiver<Wake>,
) -> Option<T> {
    tokio::pin!(phase);
    let mut retired = false;
    let mut closed = false;
    let mut stopped = *changed.borrow_and_update() == Wake::Stop;
    loop {
        tokio::select! { biased;
            result = &mut phase => return (!retired && !stopped).then_some(result),
            request = requests.recv(), if !closed => {
                retired = true;
                match request {
                    Some(request @ Request::Run { .. }) => *pending = Some(request),
                    Some(Request::Cancel) => *pending = None,
                    None => closed = true,
                }
            }
            wake = changed.changed(), if !stopped => {
                stopped = wake.is_err() || *changed.borrow_and_update() == Wake::Stop;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
