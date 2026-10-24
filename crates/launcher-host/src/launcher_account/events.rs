//! The existing account event runtime continuously polls the selected service owners.
use super::*;

/// Retains service work across account retirement, cancelling only on final process stop.
pub(super) fn poll(
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
    runtime: &Handle,
    process_stop: impl std::future::Future<Output = ()>,
) {
    let guard = lifetime.worker_guard();
    runtime.block_on(run(shared, requests, lifetime, process_stop));
    drop(guard);
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
/// Finishes the original product handle when its retained socket future completes or is dropped.
struct ProductFinish(Option<crate::product_api::Client>);

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl Drop for ProductFinish {
    fn drop(&mut self) {
        if let Some(api) = &self.0 {
            api.finish();
        }
    }
}

/// Drives the original three retained futures; the existing poll caller owns their worker guard.
async fn run(
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
    process_stop: impl std::future::Future<Output = ()>,
) {
    let control = async {
        control(shared, requests, lifetime).await;
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
    let product = {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        let (api, account) = {
            let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
            (
                snapshot.native_product_api.clone(),
                snapshot.native_presence_account.clone(),
            )
        };
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        // Arm before polling so even a sticky stop retires the original handle.
        let finish = ProductFinish(api.clone());
        async move {
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            {
                let _finish = finish;
                if let Some(api) = api {
                    tokio::select! { biased;
                        _=lifetime.retiring()=>{},
                        _=api.run(account)=>{},
                    }
                }
            }
        }
    };
    // Drop the actual joined futures before the outer worker can publish its terminal outcome.
    tokio::select! { biased;
        _ = process_stop => {},
        _ = async { tokio::join!(control, publisher, product); } => {},
    }
}

/// Retains each control round while the publisher makes independent progress on the same executor.
async fn control(
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
) {
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
                lifetime.retire();
                let api = shared
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .native_product_api
                    .clone();
                if let Some(api) = api {
                    api.stop();
                    api.finished().await;
                }
                lifetime.finished().await;
                return;
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
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            let (api, config, generation) = {
                let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                (
                    snapshot.native_product_api.clone(),
                    snapshot.cinnabar_api.clone(),
                    snapshot.auth_generation,
                )
            };
            if let Some(api) = api {
                if let Some(config) = config {
                    api.configure(config);
                }
                let release = api.take_release();
                let announced = {
                    let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                    if snapshot.same_generation(generation)
                        && new_release(&mut seen_release, release.as_ref()).is_some()
                    {
                        release
                    } else {
                        None
                    }
                };
                if let Some(release) = announced {
                    crate::lifecycle::update::release_published(&release);
                }
            }
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
            let pongs = tokio::select! { biased;
                _ = lifetime.retiring() => return,
                pongs = server_ping::round(&targets) => pongs,
            };
            let pings = round_results(&targets, pongs);
            publish(shared, |snapshot| snapshot.pings = Some(pings));
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
