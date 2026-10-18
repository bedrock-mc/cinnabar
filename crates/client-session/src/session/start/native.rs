//! Native network thread, runtime and capture teardown.
use super::*;
use crate::session::execution::NativeNetworkExecutor;
use protocol::LoginSequence;

/// Starts native login with its original Tokio runtime and blocking presentation workers.
pub fn spawn_network<P: Send + 'static>(
    config: NetworkConfig,
    prepare_presentation: impl FnOnce(
        protocol::ResourcePackHandoff,
        &protocol::GameData,
        &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<PreparedSession<P>, String>>
    + Send
    + 'static,
    observation: SessionTrace,
) -> Result<NetworkHandle<P>, std::io::Error> {
    let profile = bridge::join_profile::JoinProfile::current();
    let (mut handle, channels) = network_channels(config.session_generation);
    let failure = channels.control_event_tx.clone();
    handle.thread = Some(
        thread::Builder::new()
            .name("bedrock-network".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = failure.try_send(NetworkControlEvent::Failed {
                            message: format!("failed to create network runtime: {error}"),
                            decode_error_count: 0,
                            server_disconnect: None,
                            origin: NetworkFailureOrigin::Startup,
                        });
                        return;
                    }
                };
                let (packet_tap, packet_capture) =
                    packet_log::session_tap(config.packet_capture.as_ref());
                let executor = NativeNetworkExecutor {
                    socket_dir: config.socket_dir.clone(),
                };
                run_session_runtime(
                    runtime,
                    profile.in_scope(channels.run(
                        move |cancelled| async move {
                            LoginSequence::connect_session(
                                &config.socket_dir,
                                &config.display_name,
                                config.client_random_id,
                                Some(config.client_blob_cache.clone()),
                                Some(config.player_skin),
                                &config.login_settings,
                                Some(packet_tap),
                                move |handoff, game_data| {
                                    let packs = unless_cancelled(&cancelled, || {
                                        prepare_presentation(handoff, &game_data, &|| {
                                            *cancelled.borrow()
                                        })
                                    })
                                    .flatten();
                                    (game_data, packs)
                                },
                            )
                            .await
                        },
                        executor,
                        observation,
                    )),
                );
                if let Some(capture) = packet_capture
                    && !capture.finish(packet_log::CAPTURE_FINISH_TIMEOUT)
                {
                    tracing::warn!("packet capture writer did not finish in time");
                }
            })?,
    );
    Ok(handle)
}

/// Ends the runtime without waiting for cancelled blocking presentation work.
pub(super) fn run_session_runtime(
    runtime: tokio::runtime::Runtime,
    session: impl Future<Output = ()>,
) {
    runtime.block_on(session);
    runtime.shutdown_background();
}

/// Runs `work` unless the session is already cancelled, without borrowing the channel meanwhile.
pub(super) fn unless_cancelled<T>(
    cancelled: &watch::Receiver<bool>,
    work: impl FnOnce() -> T,
) -> Option<T> {
    let cancelled = *cancelled.borrow();
    (!cancelled).then(work)
}
