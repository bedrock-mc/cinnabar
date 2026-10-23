use super::*;
use simulation_clock::identity::PositionAuthorityGeneration;

#[cfg(not(any(target_arch = "wasm32", target_os = "vita")))]
mod native;
#[cfg(not(any(target_arch = "wasm32", target_os = "vita")))]
pub use native::spawn_network;
#[cfg(test)]
use native::{run_session_runtime, unless_cancelled};

/// Shared login result carrying startup facts and the presentation owner's prepared payload.
pub type PreparedLogin<R, P> = Result<
    (
        protocol::PlaySession<R>,
        (
            protocol::GameData,
            Option<Result<PreparedSession<P>, String>>,
        ),
    ),
    protocol::ProtocolError,
>;

/// Creates bounded session channels and a task for the host's executor. The login factory
/// receives the same cancellation signal as the pump and uses the shared protocol login.
pub fn prepare_network<R, P, F, L, E>(
    session_generation: SessionGeneration,
    login: F,
    executor: E,
    observation: SessionTrace,
) -> (NetworkHandle<P>, impl Future<Output = ()>)
where
    R: super::execution::SessionRead,
    P: Send + 'static,
    F: FnOnce(watch::Receiver<bool>) -> L,
    L: Future<Output = PreparedLogin<R, P>>,
    E: super::execution::NetworkExecutor,
{
    let (handle, channels) = network_channels(session_generation);
    (handle, channels.run(login, executor, observation))
}

/// Endpoints owned by the one login and play task.
struct NetworkChannels<P> {
    session_generation: SessionGeneration,
    control_event_tx: mpsc::Sender<NetworkControlEvent<P>>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    command_rx: mpsc::Receiver<NetworkCommand>,
    shutdown_rx: watch::Receiver<bool>,
    readiness_ingress: Arc<ReadinessIngressCounter>,
    experience_gate: Arc<experience::ExperienceGate>,
}

/// Allocates the same bounded FIFOs for native threads and local browser tasks.
fn network_channels<P>(
    session_generation: SessionGeneration,
) -> (NetworkHandle<P>, NetworkChannels<P>) {
    let (control_event_tx, control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = frame_flush::command_channel(COMMAND_CAPACITY);
    let (physics_reanchor, _physics_reanchor_rx) =
        watch::channel(PositionAuthorityGeneration::default());
    let (shutdown, shutdown_rx) = watch::channel(false);
    let readiness_ingress = Arc::new(ReadinessIngressCounter::default());
    let experience_gate = Arc::new(experience::ExperienceGate::default());
    let channels = NetworkChannels {
        session_generation,
        control_event_tx,
        world_event_tx,
        command_rx,
        shutdown_rx,
        readiness_ingress: Arc::clone(&readiness_ingress),
        experience_gate: Arc::clone(&experience_gate),
    };
    let handle = NetworkHandle {
        session_generation: session_generation.get(),
        control_events,
        world_events,
        commands,
        pending_latency_reply: Mutex::new(None),
        physics_reanchor,
        shutdown,
        thread: None,
        readiness_ingress,
        experience_gate,
        unflushed: Default::default(),
    };
    (handle, channels)
}

impl<P: Send + 'static> NetworkChannels<P> {
    /// Publishes the shared bootstrap, then pumps this join until cancellation or a terminal.
    async fn run<R, F, L, E>(self, login: F, executor: E, observation: SessionTrace)
    where
        R: super::execution::SessionRead,
        F: FnOnce(watch::Receiver<bool>) -> L,
        L: Future<Output = PreparedLogin<R, P>>,
        E: super::execution::NetworkExecutor,
    {
        let Self {
            session_generation,
            control_event_tx,
            world_event_tx,
            command_rx,
            mut shutdown_rx,
            readiness_ingress: network_readiness_ingress,
            experience_gate: network_experience_gate,
        } = self;
        let Some(login) =
            wait_for_login_or_cancel(login(shutdown_rx.clone()), &mut shutdown_rx).await
        else {
            return;
        };
        let (session, (game_data, packs)) = match login {
            Ok(connected) => connected,
            Err(error) => {
                send_login_error(&control_event_tx, &mut shutdown_rx, error).await;
                return;
            }
        };
        tracing::info!(
            base_game_version = %game_data.settings.base_game_version,
            "world lighting compatibility version"
        );
        let packs = match packs {
            None => return,
            Some(Ok(packs)) => packs,
            Some(Err(error)) => {
                send_startup_failure(&control_event_tx, &mut shutdown_rx, error, None).await;
                return;
            }
        };
        let PreparedSession {
            custom_blocks,
            packs_applied,
            packs,
        } = packs;
        let bootstrap = game_data.world;
        let settings = &game_data.settings;
        let inventory = start_game_inventory_authority(&game_data);
        let item_registry = match start_game_item_registry(&game_data) {
            Ok(registry) => registry,
            Err(error) => {
                send_startup_failure(&control_event_tx, &mut shutdown_rx, error, None).await;
                return;
            }
        };
        if !send_control_event_or_cancel(
            &control_event_tx,
            &mut shutdown_rx,
            NetworkControlEvent::Bootstrap {
                session_generation,
                identity: session.identity().clone(),
                world: bootstrap,
                environment: game_data.environment,
                custom_blocks,
                inventory,
                item_registry,
                player_game_mode: settings.player_game_mode,
                world_default_game_mode: settings.world_default_game_mode,
                player_game_mode_uses_world_default: settings.player_game_mode_uses_world_default,
                server_authoritative_block_breaking: settings.server_authoritative_block_breaking,
                rewind_history_size: settings.rewind_history_size,
                hardcore: settings.hardcore,
                hud_rules: settings.hud_rules,
                death_rules: settings.death_rules,
                crafting_rules: settings.crafting_rules,
                packs,
                terrain_before_spawn: session.terrain_before_spawn(),
                level_identity: game_data.level_identity,
            },
        )
        .await
        {
            return;
        }
        let sequencer = NetworkSequencer::new(
            session_generation,
            bootstrap.dimension,
            bootstrap.local_player_runtime_id,
        );
        let pump = async {
            run_network_pump_with_readiness_ingress(
                session,
                sequencer,
                command_rx,
                control_event_tx,
                world_event_tx,
                shutdown_rx,
                (network_readiness_ingress, network_experience_gate),
                observation,
                executor.clone(),
            )
            .await;
        };
        if packs_applied {
            run_with_pack_report(pump, |applied| executor.report_packs(applied)).await;
        } else {
            pump.await;
        }
    }
}

/// Routes startup transfers to the same reconnect owner as play-phase transfers.
async fn send_login_error<P>(
    controls: &mpsc::Sender<NetworkControlEvent<P>>,
    shutdown: &mut watch::Receiver<bool>,
    error: protocol::ProtocolError,
) {
    if let Some(transfer) = error.server_transfer() {
        send_startup_transfer(controls, shutdown, transfer).await;
    } else {
        let disconnect = error.server_disconnect();
        send_startup_failure(controls, shutdown, error, disconnect).await;
    }
}

/// Publishes the terminal transfer only while the startup task still owns the session.
async fn send_startup_transfer<P>(
    controls: &mpsc::Sender<NetworkControlEvent<P>>,
    shutdown: &mut watch::Receiver<bool>,
    transfer: protocol::ServerTransferEvent,
) {
    let target = SessionTransferTarget {
        host: transfer.host,
        port: transfer.port,
    };
    emit_network_pump_transfer_marker(&target, transfer.reload_world, 0);
    let _ = send_control_event_or_cancel(
        controls,
        shutdown,
        NetworkControlEvent::Transferred {
            target,
            decode_error_count: 0,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LoadingSession(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl NetworkSession for LoadingSession {
        type Error = &'static str;
        type Outbound = super::super::tests::PacketOutbound<&'static str>;

        fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
            let calls = Arc::clone(&self.0);
            Ok(super::super::tests::PacketOutbound::new(move |_| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(Err("completion write failed"))
            })
            .with_finish_loading(vec![protocol::modal_form_cancel_response(1)]))
        }

        /// This fixture's inbound never ends, so a failed write has no boundary to drain.
        async fn drain_to_boundary(&mut self) {}

        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
            std::future::pending().await
        }

        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    #[tokio::test]
    async fn pump_waits_for_readiness_command_and_reports_completion_send_failure() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (commands, command_rx) = mpsc::channel(2);
        let (controls, mut events) = mpsc::channel(1);
        let (world, _world_rx) = mpsc::channel(1);
        let (_shutdown, shutdown_rx) = watch::channel(false);
        let mut pump = std::pin::pin!(run_network_pump(
            LoadingSession(Arc::clone(&calls)),
            NetworkSequencer::new(simulation_clock::identity::SessionGeneration::new(1), 0, 42),
            command_rx,
            controls,
            world,
            shutdown_rx
        ));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(pump.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        commands.try_send(NetworkCommand::FinishLoading).unwrap();
        commands
            .try_send(NetworkCommand::FlushFrame {
                cache_opportunity: false,
            })
            .unwrap();
        pump.await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(matches!(
            events.recv().await,
            Some(NetworkControlEvent::Failed {
                origin: NetworkFailureOrigin::Send,
                ..
            })
        ));
    }

    // Cancelling a join while its pack preparation runs ends the network thread mid-compile.
    #[test]
    fn cancelled_preparation_releases_the_network_thread() {
        let (release, blocked) = std::sync::mpsc::channel::<()>();
        let (finished, thread_done) = std::sync::mpsc::channel();
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let thread = thread::spawn(move || {
            run_session_runtime(runtime, async move {
                let join = tokio::task::spawn_blocking(move || blocked.recv().is_ok());
                let prepared = wait_for_login_or_cancel(join, &mut shutdown_rx).await;
                assert!(prepared.is_none());
            });
            finished.send(()).unwrap();
        });
        shutdown.send_replace(true);
        let ended = thread_done.recv_timeout(std::time::Duration::from_secs(5));
        let _ = release.send(());
        thread.join().unwrap();
        ended.expect("the network thread outlived its cancelled preparation");
    }

    // Shutdown must not wait on the watch channel while a compile is running.
    #[test]
    fn shutdown_returns_while_presentation_compiles() {
        let (shutdown, cancelled) = watch::channel(false);
        let (started, compiling) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel::<()>();
        let worker = thread::spawn(move || {
            unless_cancelled(&cancelled, || {
                started.send(()).unwrap();
                blocked.recv().is_ok()
            })
        });
        compiling.recv().unwrap();
        let (sent, shutdown_done) = std::sync::mpsc::channel();
        let notifier = thread::spawn(move || {
            shutdown.send_replace(true);
            sent.send(()).unwrap();
        });
        let returned = shutdown_done.recv_timeout(std::time::Duration::from_secs(5));
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Some(true));
        notifier.join().unwrap();
        returned.expect("shutdown blocked behind the running compile");
    }

    #[tokio::test]
    async fn startup_transfer_reaches_reconnect_unless_cancelled() {
        for cancelled in [false, true] {
            let (controls, mut events) = mpsc::channel(1);
            let (_shutdown, mut receiver) = watch::channel(cancelled);
            let transfer = protocol::ServerTransferEvent {
                host: "next.example.test".into(),
                port: 19133,
                reload_world: false,
            };
            send_startup_transfer::<()>(&controls, &mut receiver, transfer.clone()).await;
            drop(controls);
            let event = events.recv().await;
            if cancelled {
                assert!(event.is_none(), "cancelled joins must not start a redial");
            } else {
                assert!(
                    matches!(event, Some(NetworkControlEvent::Transferred { target, .. })
                    if target.host == transfer.host && target.port == transfer.port)
                );
                assert!(
                    events.recv().await.is_none(),
                    "transfer must not become a failure"
                );
            }
        }
    }
}

#[cfg(test)]
mod prepared_tests {
    use super::*;

    /// Refuses outbound execution because these tests cancel or fail during login.
    #[derive(Clone)]
    struct NoRuntime;

    impl NetworkExecutor for NoRuntime {
        type OutboundTask = ();

        fn spawn_outbound(&self, _: impl Future<Output = ()> + Send + 'static) {
            panic!("login has not admitted an outbound session");
        }

        fn timers(&self) -> bridge::tasks::Timers {
            bridge::tasks::Timers(|_| Box::pin(std::future::pending()))
        }
    }

    #[test]
    fn host_task_cancels_pending_login_without_creating_a_runtime() {
        let (mut handle, task) =
            prepare_network::<bridge::FramedReader<tokio::io::Empty>, (), _, _, _>(
                SessionGeneration::new(1),
                |_| std::future::pending(),
                NoRuntime,
                SessionTrace::default(),
            );
        let mut task = Box::pin(task);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(task.as_mut().poll(&mut context).is_pending());
        handle.shutdown();
        assert!(task.as_mut().poll(&mut context).is_ready());
        assert!(handle.control_events.try_recv().is_err());
    }
}

/// Keeps application and reversion reports around an admitted session.
async fn run_with_pack_report<P, R, F>(pump: P, report: R)
where
    P: std::future::Future<Output = ()>,
    R: Fn(bool) -> F,
    F: std::future::Future<Output = ()>,
{
    report(true).await;
    pump.await;
    report(false).await;
}

#[cfg(test)]
mod pack_report_tests {
    use super::run_with_pack_report;
    #[tokio::test]
    async fn review_pack_application_finishes_before_the_pump_and_reversion() {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let pump_events = events.clone();
        let report_events = events.clone();
        run_with_pack_report(
            async move {
                pump_events.lock().unwrap().push("pump");
            },
            move |applied| {
                let events = report_events.clone();
                async move {
                    if applied {
                        tokio::task::yield_now().await;
                    }
                    events
                        .lock()
                        .unwrap()
                        .push(if applied { "applied" } else { "reverted" });
                }
            },
        )
        .await;
        assert_eq!(*events.lock().unwrap(), ["applied", "pump", "reverted"]);
    }
}
