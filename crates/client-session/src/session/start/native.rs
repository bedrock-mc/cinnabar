//! Native network thread, runtime and capture teardown.
use super::companion::{SessionCompanion, run_with_companion};
use super::*;
use crate::session::execution::NativeNetworkExecutor;
use protocol::LoginSequence;

/// Starts login on the caller's retained reactor, keeping original blocking presentation workers.
pub fn spawn_network<P: Send + 'static>(
    config: NetworkConfig,
    runtime: tokio::runtime::Handle,
    tasks: tokio_util::task::TaskTracker,
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
        runtime,
        tasks,
        prepare_presentation,
        observation,
        packet_relay,
        (),
    )
}

/// Retains one application service through the whole existing network worker and explicit teardown.
pub fn spawn_network_with_companion<P: Send + 'static>(
    config: NetworkConfig,
    runtime: tokio::runtime::Handle,
    tasks: tokio_util::task::TaskTracker,
    prepare_presentation: impl FnOnce(
        bedrock_packs::ResourcePackHandoff,
        &protocol::GameData,
        &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<PreparedSession<P>, String>>
    + Send
    + 'static,
    observation: SessionTrace,
    packet_relay: Option<Arc<dyn protocol::PacketRelay>>,
    mut companion: impl SessionCompanion,
) -> Result<NetworkHandle<P>, std::io::Error> {
    let profile = bridge::join_profile::JoinProfile::current();
    let (mut handle, channels) = network_channels(config.session_generation);
    let failure = channels.control_event_tx.clone();
    let shutdown = handle.shutdown.clone();
    let terminal = Arc::clone(&channels.terminal);
    let completion = tasks.token();
    let work = move || {
        let (packet_tap, packet_capture) = packet_log::session_tap(config.packet_capture.as_ref());
        let executor = NativeNetworkExecutor {
            socket_dir: config
                .endpoint
                .core_directory()
                .map(std::path::Path::to_path_buf),
            tasks,
        };
        let session = async {
            #[cfg(feature = "native-networking")]
            let mut config = config;
            #[cfg(feature = "native-networking")]
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
                                std::io::Error::other("native service preparation is unavailable")
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
                        display_name: &config.display_name,
                        client_random_id: config.client_random_id,
                        cache: Some(config.client_blob_cache.clone()),
                        skin: Some(config.player_skin),
                        settings: &config.login_settings,
                        tap: Some(packet_tap),
                        relay: packet_relay,
                        target: None,
                    };
                    #[cfg(feature = "native-networking")]
                    if let Some(acquire) = config.native_preparation {
                        let protocol::SessionEndpoint::Native(_) = &config.endpoint else {
                            return Err(protocol::ProtocolError::Authentication(
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "native account preparation requires a native endpoint",
                                ),
                            ));
                        };
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
                let _ =
                    tokio::time::timeout(FINAL_CONTROL_FLUSH_TIMEOUT, failure.send(event)).await;
            }
            companion.close().await;
        };
        run_session_runtime(&runtime, profile.in_scope(session));
        if let Some(capture) = packet_capture
            && !capture.finish(packet_log::CAPTURE_FINISH_TIMEOUT)
        {
            tracing::warn!("packet capture writer did not finish in time");
        }
    };
    handle.thread = Some(
        thread::Builder::new()
            .name("bedrock-network".to_owned())
            .spawn(move || {
                let _completion = completion;
                work();
            })?,
    );
    Ok(handle)
}

/// Runs only this session; its caller keeps the shared reactor alive for unrelated owners.
pub(super) fn run_session_runtime(
    runtime: &tokio::runtime::Handle,
    session: impl Future<Output = ()>,
) {
    runtime.block_on(session);
}

/// Runs `work` unless the session is already cancelled, without borrowing the channel meanwhile.
pub(super) fn unless_cancelled<T>(
    cancelled: &watch::Receiver<bool>,
    work: impl FnOnce() -> T,
) -> Option<T> {
    let cancelled = *cancelled.borrow();
    (!cancelled).then(work)
}
