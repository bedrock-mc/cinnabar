//! Native network thread, runtime and capture teardown.
use super::companion::SessionCompanion;
#[cfg(feature = "native-networking")]
use super::companion::run_with_companion;
use super::*;
#[cfg(feature = "native-networking")]
use crate::session::execution::NativeNetworkExecutor;
#[cfg(feature = "native-networking")]
use protocol::LoginSequence;

/// Starts login on the caller's retained reactor, keeping original blocking presentation workers.
pub fn spawn_network<P: Send + 'static>(
    config: NetworkConfig,
    execution: bridge::tasks::NativeExecution,
    prepare_presentation: impl FnOnce(
        bedrock_packs::ResourcePackHandoff,
        &protocol::GameData,
        &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<PreparedSession<P>, String>>
    + Send
    + 'static,
    observation: SessionTrace,
    packet_relay: Option<Arc<dyn protocol::PacketRelay>>,
) -> Result<NetworkHandle<P>, std::io::Error> {
    spawn_network_with_companion(
        config,
        execution,
        prepare_presentation,
        observation,
        packet_relay,
        (),
    )
}

/// Retains one application service through the whole existing network worker and explicit teardown.
pub fn spawn_network_with_companion<P: Send + 'static>(
    config: NetworkConfig,
    execution: bridge::tasks::NativeExecution,
    prepare_presentation: impl FnOnce(
        bedrock_packs::ResourcePackHandoff,
        &protocol::GameData,
        &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<PreparedSession<P>, String>>
    + Send
    + 'static,
    observation: SessionTrace,
    packet_relay: Option<Arc<dyn protocol::PacketRelay>>,
    companion: impl SessionCompanion,
) -> Result<NetworkHandle<P>, std::io::Error> {
    #[cfg(not(feature = "native-networking"))]
    {
        let _ = (
            config,
            execution,
            prepare_presentation,
            observation,
            packet_relay,
            companion,
        );
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "native networking is unavailable in this build",
        ))
    }
    #[cfg(feature = "native-networking")]
    {
        let mut companion = companion;
        let runtime = execution.runtime().clone();
        let tasks = execution.tasks().clone();
        let profile = bridge::join_profile::JoinProfile::current();
        let (mut handle, channels) = network_channels(config.session_generation);
        let failure = channels.control_event_tx.clone();
        let shutdown = handle.shutdown.clone();
        let terminal = Arc::clone(&channels.terminal);
        let completion = tasks.token();
        let retirement = config.retirement.as_ref().map(|tasks| tasks.token());
        let work = move || {
            let (packet_tap, packet_capture) =
                packet_log::session_tap(config.packet_capture.as_ref());
            let executor = NativeNetworkExecutor { tasks };
            let session = async {
                let mut config = config;
                {
                    let mut cancelled = channels.shutdown_rx.clone();
                    let preparation =
                        wait_for_login_or_cancel(companion.prepare(), &mut cancelled).await;
                    match preparation {
                        None => {
                            companion.close().await;
                            return;
                        }
                        Some(Ok(None)) => {}
                        Some(result) => {
                            config.native_preparation = Some(Box::pin(async move {
                                result?.ok_or_else(|| {
                                    std::io::Error::other(
                                        "native service preparation is unavailable",
                                    )
                                })
                            }));
                        }
                    }
                }
                let work = channels.run(
                    move |cancelled| async move {
                        let prepare = move |handoff, game_data: protocol::GameData| {
                            let packs = unless_cancelled(&cancelled, || {
                                prepare_presentation(handoff, &game_data, &|| *cancelled.borrow())
                            })
                            .flatten();
                            (game_data, packs)
                        };
                        let login = protocol::SessionLogin {
                            #[cfg(not(target_arch = "wasm32"))]
                            execution: Some(&execution),
                            display_name: &config.display_name,
                            client_random_id: config.client_random_id,
                            cache: Some(config.client_blob_cache.clone()),
                            skin: Some(config.player_skin),
                            settings: &config.login_settings,
                            tap: Some(packet_tap),
                            relay: packet_relay,
                            target: None,
                        };
                        if let Some(acquire) = config.native_preparation {
                            let (target, authentication) = acquire
                                .await
                                .map_err(protocol::ProtocolError::Authentication)?;
                            return LoginSequence::connect_native_authenticated(
                                &target,
                                login,
                                authentication,
                                prepare,
                            )
                            .await;
                        }
                        LoginSequence::connect_endpoint(&config.endpoint, login, prepare).await
                    },
                    executor,
                    observation,
                );
                let result = run_with_companion(work, &mut companion, &shutdown, &terminal).await;
                if let Err(error) = result {
                    let event = NetworkControlEvent::Failed {
                        message: error.to_string(),
                        decode_error_count: 0,
                        server_disconnect: None,
                        origin: NetworkFailureOrigin::Receive,
                    };
                    // Retain the existing ordered control prefix while bounding final publication.
                    let _ = tokio::time::timeout(FINAL_CONTROL_FLUSH_TIMEOUT, failure.send(event))
                        .await;
                }
                companion.close().await;
            };
            tracing::info!(shutdown_phase = "session_runtime_begin");
            run_session_runtime(&runtime, profile.in_scope(session));
            tracing::info!(shutdown_phase = "session_runtime_returned");
            tracing::info!(shutdown_phase = "session_capture_check_begin");
            if let Some(capture) = packet_capture
                && !capture.finish(packet_log::CAPTURE_FINISH_TIMEOUT)
            {
                tracing::warn!("packet capture writer did not finish in time");
            }
            tracing::info!(shutdown_phase = "session_capture_check_returned");
        };
        // Destroy an unstarted worker's captures before either completion token on spawn failure.
        let worker = (work, completion, retirement);
        handle.thread = Some(
            thread::Builder::new()
                .name("bedrock-network".to_owned())
                .spawn(move || run_worker(worker))?,
        );
        Ok(handle)
    }
}

/// Captures the whole ordered owner, retaining completion through the actual body and its drops.
#[cfg(feature = "native-networking")]
fn run_worker(
    worker: (
        impl FnOnce(),
        tokio_util::task::task_tracker::TaskTrackerToken,
        Option<tokio_util::task::task_tracker::TaskTrackerToken>,
    ),
) {
    let (work, _completion, _retirement) = worker;
    let worker_thread = thread::current().id();
    tracing::info!(shutdown_phase = "session_worker_body_begin", ?worker_thread);
    work();
    tracing::info!(
        shutdown_phase = "session_worker_body_returned",
        ?worker_thread
    );
}

/// Runs only this session; its caller keeps the shared reactor alive for unrelated owners.
#[cfg(any(feature = "native-networking", test))]
pub(super) fn run_session_runtime(
    runtime: &tokio::runtime::Handle,
    session: impl Future<Output = ()>,
) {
    runtime.block_on(session);
}

/// Runs `work` unless the session is already cancelled, without borrowing the channel meanwhile.
#[cfg(any(feature = "native-networking", test))]
pub(super) fn unless_cancelled<T>(
    cancelled: &watch::Receiver<bool>,
    work: impl FnOnce() -> T,
) -> Option<T> {
    let cancelled = *cancelled.borrow();
    (!cancelled).then(work)
}
