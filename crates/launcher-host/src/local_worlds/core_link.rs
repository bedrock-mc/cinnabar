use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use bridge::{LocalWorld, WorldState};
use crossbeam_channel::Sender;
use launcher::local_worlds::world::WorldStatus;

use super::{lock, manager::Manager};

/// Bounds one `local_world.v1` call.
const REPORT_TIMEOUT: Duration = Duration::from_secs(3);
/// Spaces retries while the core cannot be reached.
const RETRY_DELAY: Duration = Duration::from_millis(250);

/// Keeps one core's route to the open local world current until dropped.
pub struct CoreLink {
    _stop: Sender<()>,
    route: CoreRoute,
}

/// The open world as the core last acknowledged it.
#[derive(Clone, Default)]
pub struct CoreRoute(Arc<Mutex<Option<LocalWorld>>>);

impl CoreRoute {
    /// The manager's status with a running world held at starting until the core has
    /// acknowledged that exact server (world and address), so a join never reaches a core that
    /// would still fall through elsewhere or dial a previous instance.
    #[must_use]
    pub fn status(&self, manager: &Manager) -> WorldStatus {
        let status = manager.status();
        self.gate(status, &manager.core_report())
    }

    fn gate(&self, mut status: WorldStatus, current: &LocalWorld) -> WorldStatus {
        let routed = current.state == WorldState::Running
            && current.target.is_some()
            && current.world.as_ref().map(|world| world.id.as_str()) == status.world_id.as_deref()
            && lock(&self.0).as_ref() == Some(current);
        if status.state == WorldState::Running && !routed {
            status.state = WorldState::Starting;
        }
        status
    }

    fn acknowledge(&self, report: LocalWorld) {
        *lock(&self.0) = Some(report);
    }
}

impl CoreLink {
    /// Reports the manager's open world to the core at `socket_dir` now and after every change.
    pub fn spawn(manager: Manager, socket_dir: PathBuf) -> io::Result<Self> {
        let (stop, stopped) = crossbeam_channel::bounded::<()>(0);
        let route = CoreRoute::default();
        let acknowledged = route.clone();
        let changes = manager.subscribe();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new()
            .name("local-world-core".to_owned())
            .spawn(move || {
                let mut reported: Option<LocalWorld> = None;
                loop {
                    let report = manager.core_report();
                    let mut pending = false;
                    if reported.as_ref() != Some(&report) {
                        match runtime.block_on(deliver(&socket_dir, &report)) {
                            Ok(()) => {
                                acknowledged.acknowledge(report.clone());
                                reported = Some(report);
                            }
                            Err(error) => {
                                tracing::debug!("local world route not delivered yet: {error}");
                                pending = true;
                            }
                        }
                    }
                    let retry = if pending {
                        crossbeam_channel::after(RETRY_DELAY)
                    } else {
                        crossbeam_channel::never()
                    };
                    crossbeam_channel::select! {
                        recv(stopped) -> _ => return,
                        recv(changes) -> _ => {}
                        recv(retry) -> _ => {}
                    }
                }
            })?;
        Ok(Self { _stop: stop, route })
    }

    /// What this core has acknowledged, for gating readiness on it.
    #[must_use]
    pub fn route(&self) -> CoreRoute {
        self.route.clone()
    }
}

async fn deliver(socket_dir: &std::path::Path, report: &LocalWorld) -> Result<(), String> {
    match tokio::time::timeout(REPORT_TIMEOUT, bridge::set_local_world(socket_dir, report)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("the core did not answer".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use bridge::{Backend, GameMode, LocalTarget, LocalWorldInfo, Transport};

    use super::*;

    fn status(state: WorldState) -> WorldStatus {
        WorldStatus {
            state,
            world_id: Some("0123456789abcdef".into()),
            backend: Some(Backend::Bds),
            paused: false,
            pause_supported: false,
            error: None,
            setup: None,
            backend_unavailable_reason: None,
            max_players: Some(8),
        }
    }

    fn running(id: &str) -> LocalWorld {
        LocalWorld {
            state: WorldState::Running,
            world: Some(LocalWorldInfo {
                id: id.into(),
                name: "Home".into(),
                game_mode: GameMode::Survival,
                backend: Backend::Bds,
            }),
            target: Some(LocalTarget {
                address: "127.0.0.1:5000".into(),
                transport: Transport::NetherNet,
                lan_address: None,
                level_name: None,
            }),
            error: None,
        }
    }

    fn at(mut report: LocalWorld, address: &str) -> LocalWorld {
        if let Some(target) = report.target.as_mut() {
            target.address = address.into();
        }
        report
    }

    /// A world is joinable only once its core routes sessions to this very server.
    #[test]
    fn readiness_waits_for_the_core_to_route_the_world() {
        let route = CoreRoute::default();
        let current = running("0123456789abcdef");
        let gate = |route: &CoreRoute| route.gate(status(WorldState::Running), &current).state;
        assert_eq!(gate(&route), WorldState::Starting, "nothing acknowledged");
        route.acknowledge(LocalWorld {
            state: WorldState::Starting,
            ..current.clone()
        });
        assert_eq!(
            gate(&route),
            WorldState::Starting,
            "only the start acknowledged"
        );
        route.acknowledge(running("fedcba9876543210"));
        assert_eq!(
            gate(&route),
            WorldState::Starting,
            "another world acknowledged"
        );
        route.acknowledge(at(current.clone(), "127.0.0.1:6000"));
        assert_eq!(
            gate(&route),
            WorldState::Starting,
            "a previous instance acknowledged"
        );
        route.acknowledge(current.clone());
        assert_eq!(gate(&route), WorldState::Running);
        for state in [WorldState::Idle, WorldState::Starting, WorldState::Failed] {
            assert_eq!(
                CoreRoute::default().gate(status(state), &current).state,
                state
            );
        }
    }
}
