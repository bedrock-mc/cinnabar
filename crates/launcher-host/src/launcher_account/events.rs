//! The existing account event runtime continuously polls the selected service owners.
use super::*;

/// Drives retained control, presence and product futures on the existing event executor.
pub(super) fn poll(
    socket_dir: Option<&std::path::Path>,
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
    runtime: &Handle,
) {
    let guard = lifetime.worker_guard();
    runtime.block_on(run(socket_dir, shared, requests, lifetime));
    drop(guard);
}

/// Drives the original three retained futures; the existing poll caller owns their worker guard.
async fn run(
    socket_dir: Option<&std::path::Path>,
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
) {
    let control = async {
        control(socket_dir, shared, requests, lifetime).await;
        lifetime.retire();
    };
    let publisher = async {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        native_presence::run(shared, lifetime).await;
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            lifetime.retiring().await;
            lifetime.complete(true);
        }
    };
    let product = async {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            let (api, account) = {
                let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                (
                    snapshot.native_product_api.clone(),
                    snapshot.native_presence_account.clone(),
                )
            };
            if let Some(api) = api {
                tokio::select! { biased;
                    _=lifetime.retiring()=>{},
                    _=api.run(account)=>{},
                }
                api.finish();
            }
        }
    };
    tokio::join!(control, publisher, product);
}

/// Retains each control round while the publisher makes independent progress on the same executor.
async fn control(
    socket_dir: Option<&std::path::Path>,
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
) {
    let mut sent_presence = None;
    let mut presence_failed = false;
    let mut sent_cinnabar = None;
    let mut seen_release = None;
    let mut ping_due = Instant::now();
    let mut pinged: Vec<String> = Vec::new();
    loop {
        let joining = shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .joining;
        let interval = if joining {
            JOIN_EVENT_INTERVAL
        } else {
            EVENT_INTERVAL
        };
        if lifetime.retirement_requested() {
            return;
        }
        tokio::select! {
            _ = lifetime.notified() => {},
            _ = tokio::time::sleep(interval) => {},
        }
        match requests.try_recv() {
            Ok(()) => {
                let native = shared
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .account_source
                    == AccountSource::Native;
                if native {
                    lifetime.retire();
                    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                    {
                        let api = shared
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .native_product_api
                            .clone();
                        if let Some(api) = api {
                            api.stop();
                            api.finished().await;
                        }
                    }
                    lifetime.finished().await;
                    if lifetime.retirement() != Some(TitlePresenceRetirement::Complete) {
                        return;
                    }
                }
                if native {
                    return;
                }
                if let Some(socket_dir) = socket_dir {
                    let _ = bridge::sign_out(socket_dir).await;
                }
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                lifetime.retire();
                return;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
        if lifetime.retirement_requested() {
            return;
        }
        let presence = shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .xbox_presence
            .clone();
        let core_presence = shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .title_presence_owner
            == crate::launcher_core::TitlePresenceOwner::Core;
        if core_presence
            && sent_presence.as_ref() != Some(&presence)
            && let Some(socket_dir) = socket_dir
        {
            let sent = tokio::time::timeout(
                Duration::from_secs(2),
                bridge::report_xbox_presence(socket_dir, &presence),
            )
            .await;
            if matches!(sent, Ok(Ok(()))) {
                sent_presence = Some(presence);
                presence_failed = false;
            } else if !presence_failed {
                tracing::warn!("Xbox presence control unavailable; retrying");
                presence_failed = true;
            }
        }
        let (cinnabar, account_source, generation) = {
            let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
            (
                snapshot.cinnabar_api.clone(),
                snapshot.account_source,
                snapshot.auth_generation,
            )
        };
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            let announced = {
                let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                if snapshot.same_account_source(generation, account_source)
                    && account_source == AccountSource::Native
                    && let Some(api) = &snapshot.native_product_api
                {
                    if let Some(config) = &cinnabar {
                        api.configure(config.clone());
                    }
                    let release = api.take_release();
                    if new_release(&mut seen_release, release.as_ref()).is_some() {
                        release
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(release) = announced {
                crate::lifecycle::update::release_published(&release);
            }
        }
        let current = shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .same_account_source(generation, account_source);
        if !current {
            continue;
        }
        let core_api = account_source == AccountSource::Core;
        if let Some(config) = &cinnabar
            && core_api
            && sent_cinnabar.as_ref() != Some(config)
            && let Some(socket_dir) = socket_dir
        {
            let sent = tokio::time::timeout(
                Duration::from_secs(2),
                bridge::configure_cinnabar_api(socket_dir, config),
            )
            .await;
            if matches!(sent, Ok(Ok(())))
                && shared
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .same_account_source(generation, account_source)
            {
                sent_cinnabar = Some(config.clone());
            }
        }
        if let Some(socket_dir) = socket_dir {
            poll_core_events(socket_dir, shared, core_api, &mut seen_release).await;
        }
        let targets = shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .ping_targets
            .clone();
        // The feeds worker publishes first, so rows it adds are new targets here
        // and are pinged on the next tick instead of a round later.
        if targets != pinged {
            ping_due = Instant::now();
        }
        if !targets.is_empty() && Instant::now() >= ping_due {
            ping_due = Instant::now() + PING_INTERVAL;
            pinged = targets.clone();
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            let pongs = if !core_presence {
                tokio::select! { biased;
                    _ = lifetime.retiring() => return,
                    pongs = server_ping::round(&targets) => Ok(pongs),
                }
            } else {
                match super::workers::require_core_directory(socket_dir) {
                    Ok(directory) => bridge::ping_servers(directory, &targets).await,
                    Err(error) => Err(error),
                }
            };
            #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
            let pongs = match super::workers::require_core_directory(socket_dir) {
                Ok(directory) => bridge::ping_servers(directory, &targets).await,
                Err(error) => Err(error),
            };
            let pings = round_results(
                &targets,
                settle("ping", pongs, &mut false).unwrap_or_default(),
            );
            publish(shared, |snapshot| snapshot.pings = Some(pings));
        }
    }
}

/// Keeps native authentication separate from legacy Core route observations.
/// A response belongs only to the account generation and source that requested it.
async fn poll_core_events(
    socket_dir: &std::path::Path,
    shared: &Mutex<Snapshot>,
    core_api: bool,
    seen_release: &mut Option<bridge::CinnabarRelease>,
) {
    let (generation, source) = {
        let snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
        (snapshot.auth_generation, snapshot.account_source)
    };
    let Ok(events) = bridge::poll_events(socket_dir).await else {
        return;
    };
    let announced = {
        let mut snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
        if !snapshot.same_account_source(generation, source) {
            return;
        }
        if let Some(disconnect) = events.disconnect
            && snapshot.last_disconnect != Some(disconnect.sequence)
        {
            // The first poll only records the standing disconnect.
            if snapshot.last_disconnect.is_some() {
                // An empty message reads as vanilla's no-reason line.
                let reason = disconnect.message.trim().to_owned();
                snapshot.events.push(AccountEvent::Disconnected { reason });
            }
            snapshot.last_disconnect = Some(disconnect.sequence);
        }
        snapshot.last_disconnect.get_or_insert(0);
        snapshot.connect = events.connect;
        snapshot.server_trust = events.server_trust;
        if source == AccountSource::Core {
            snapshot.set_account(events.auth);
        }
        core_api
            && source == AccountSource::Core
            && new_release(seen_release, events.cinnabar_release.as_ref()).is_some()
    };
    if announced {
        crate::lifecycle::update::release_published(
            events.cinnabar_release.as_ref().expect("announced release"),
        );
    }
}

#[cfg(all(test, unix, feature = "native-networking"))]
mod tests;
