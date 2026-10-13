//! Observation stays responsive while a separate worker admits guests or drains saves.

use super::*;
use sha2::{Digest, Sha256};
use std::time::Instant;

/// Runs one coordinator and one bounded loader, sharing no guest instances with the frame.
#[allow(
    clippy::too_many_arguments,
    reason = "worker owns the bounded channels and shared authority"
)]
pub(super) fn run(
    directory: PathBuf,
    messages: Receiver<Message>,
    updates: Sender<Update>,
    retired: Receiver<ModHost>,
    latest: Arc<AtomicU64>,
    desired: Arc<Mutex<Desired>>,
    settings: Arc<Mutex<BTreeMap<PathBuf, SettingsSnapshot>>>,
) {
    let (loads, receive_loads) = bounded::<LoadRequest>(1);
    let (results, receive_results) = bounded(1);
    let (fences, receive_fences) = crossbeam_channel::unbounded();
    let generation = latest.clone();
    let root = directory.clone();
    let loader=thread::Builder::new().name("local-mod-loader".into()).spawn(move || {
        loop {
            crossbeam_channel::select! {
                recv(receive_loads) -> request => {
                    let Ok(request)=request else {break};
                    if request.generation!=generation.load(Ordering::Acquire){continue;}
                    let changes=after_retirement(&retired,||request.sources.into_iter().map(|(entry,snapshot)|{
                        let current=settings.lock().expect("mod settings snapshots").get(&entry.component).cloned();
                        let result=snapshot.and_then(|snapshot|build_candidate_with_settings(snapshot,current.as_ref())).map(Box::new);
                        Change {entry,result}
                    }).collect());
                    if request.generation==generation.load(Ordering::Acquire) {
                        let _=results.send(Update{generation:request.generation,registration:request.registration,changes});
                    }
                }
                recv(retired) -> host => {let Ok(host)=host else {break};drop(host);}
                recv(receive_fences) -> fence => {let Ok(fence)=fence else {break};removal::finish(&retired,fence,&root,&generation);}
            }
        }
    });
    let Ok(loader) = loader else { return };
    let mut fingerprint = None;
    let mut installed = BTreeMap::new();
    let mut statuses: Vec<Status> = Vec::new();
    let mut request_id = String::new();
    let mut pending_load = None;
    let mut pending_update = None;
    let mut next_poll = Instant::now();
    let mut removal = removal::Pending::default();
    loop {
        if Instant::now() >= next_poll {
            next_poll = Instant::now() + RELOAD_INTERVAL;
            let observed = source::observe(&directory.join(REGISTRATION_FILE));
            removal.observe(observed.request_id.as_deref());
            let registration = observed
                .result
                .unwrap_or_default()
                .unwrap_or_else(Registration::empty);
            let mut digest = Sha256::new();
            digest.update(observed.fingerprint);
            let mut sources = Vec::new();
            for entry in registration.mods.iter().filter(|entry| entry.enabled) {
                let snapshot = source::source_snapshot(entry.clone());
                match &snapshot {
                    Ok(snapshot) => digest.update(snapshot.identity),
                    Err(error) => digest.update(error.as_bytes()),
                }
                if snapshot.as_ref().map_or(true, |snapshot| {
                    installed.get(&entry.component) != Some(&snapshot.identity)
                }) {
                    sources.push((entry.clone(), snapshot));
                }
            }
            let current: [u8; 32] = digest.finalize().into();
            if fingerprint != Some(current) {
                fingerprint = Some(current);
                let mut selection = desired.lock().expect("mod desired list");
                let generation = latest.fetch_add(1, Ordering::AcqRel) + 1;
                *selection = Desired {
                    generation,
                    registration: Some(registration.clone()),
                };
                drop(selection);
                request_id = registration.request_id.clone();
                pending_load = None;
                pending_update = None;
                if sources.is_empty() {
                    pending_update = Some(Update {
                        generation,
                        registration,
                        changes: Vec::new(),
                    });
                } else {
                    pending_load = Some(LoadRequest {
                        generation,
                        registration,
                        sources,
                    });
                }
            }
        }
        if let Some(request) = pending_load.take()
            && let Err(TrySendError::Full(request)) = loads.try_send(request)
        {
            pending_load = Some(request);
        }
        for update in receive_results.try_iter() {
            if update.generation == latest.load(Ordering::Acquire) {
                pending_update = Some(update);
            }
        }
        if let Some(update) = pending_update.take()
            && let Err(TrySendError::Full(update)) = updates.try_send(update)
        {
            pending_update = Some(update);
        }
        match messages.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
            Ok(Message::Acknowledge {
                generation,
                request_id: id,
                installed: accepted,
                statuses: changed,
            }) => {
                if generation != latest.load(Ordering::Acquire) {
                    continue;
                }
                installed = accepted;
                let current = desired.lock().expect("mod desired list");
                statuses.retain(|status| {
                    current.registration.as_ref().is_some_and(|registration| {
                        registration
                            .mods
                            .iter()
                            .any(|entry| entry.enabled && entry.component == status.component)
                    })
                });
                drop(current);
                for status in changed {
                    statuses.retain(|old| old.component != status.component);
                    statuses.push(status);
                }
                removal.applied(generation, id.clone(), &fences);
                let _ = status::write_json(
                    &directory.join(STATUS_FILE),
                    &StatusList::new(id, statuses.clone()),
                );
            }
            Ok(Message::Failed { component, error }) => {
                statuses.retain(|status| status.component != component);
                statuses.push(Status::new(component, "error", Some(error)));
                let _ = status::write_json(
                    &directory.join(STATUS_FILE),
                    &StatusList::new(request_id.clone(), statuses.clone()),
                );
            }
            Ok(Message::Retry {
                generation,
                inactive,
            }) => {
                if generation != latest.load(Ordering::Acquire) {
                    continue;
                }
                for component in inactive {
                    installed.remove(&component);
                }
                fingerprint = None;
                next_poll = Instant::now();
            }
            Ok(Message::AwaitRetirement {
                selection,
                complete,
            }) => {
                removal.wait(latest.load(Ordering::Acquire), selection, complete, &fences);
                next_poll = Instant::now();
            }
            Ok(Message::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
    latest.fetch_add(1, Ordering::AcqRel);
    drop(receive_results);
    drop(loads);
    drop(fences);
    let _ = loader.join();
}
