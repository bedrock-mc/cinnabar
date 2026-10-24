//! What the menu and the core learn about the open world; a hidden server reports none.

use bridge::{Backend, LocalTarget, LocalWorld, LocalWorldInfo, Transport, WorldState};
use launcher::local_worlds::world::{World, WorldStatus};

use super::Manager;
use crate::local_worlds::{lock, process::Instance};

impl Manager {
    /// The open world's lifecycle and the dedicated server's setup.
    pub fn status(&self) -> WorldStatus {
        let mut status = {
            let life = lock(&self.inner.life);
            if life.standby {
                Self::idle_status()
            } else {
                WorldStatus {
                    state: life.state,
                    world_id: life.world.as_ref().map(|world| world.id.clone()),
                    backend: life.world.as_ref().map(|world| world.backend),
                    paused: life.paused,
                    pause_supported: life
                        .instance
                        .as_ref()
                        .is_none_or(|instance| instance.can_pause()),
                    error: life.failure.clone(),
                    setup: None,
                    backend_unavailable_reason: None,
                    max_players: life
                        .instance
                        .as_ref()
                        .and_then(|instance| instance.max_players()),
                }
            }
        };
        if let Some(setup) = &self.inner.setup {
            let (setup, unavailable) = setup.status();
            status.setup = Some(setup);
            status.backend_unavailable_reason = unavailable;
        }
        status
    }

    /// The open world as the core routes and hosts it.
    pub(crate) fn core_report(&self) -> LocalWorld {
        let life = lock(&self.inner.life);
        if life.standby {
            return LocalWorld::default();
        }
        let target = match (&life.instance, &life.world) {
            (Some(instance), Some(world)) if life.state == WorldState::Running => {
                Some(target_for(world, instance.as_ref()))
            }
            _ => None,
        };
        LocalWorld {
            state: life.state,
            world: life.world.as_ref().map(|world| LocalWorldInfo {
                id: world.id.clone(),
                name: world.name.clone(),
                game_mode: world.game_mode,
                backend: world.backend,
                multiplayer_disabled: !world
                    .options
                    .enabled(launcher::local_worlds::options::WorldOption::Multiplayer),
            }),
            target,
            error: life.failure.clone(),
        }
    }

    /// The status of a lifecycle with no open world, which a hidden server reports.
    fn idle_status() -> WorldStatus {
        WorldStatus {
            state: WorldState::Idle,
            world_id: None,
            backend: None,
            paused: false,
            pause_supported: true,
            error: None,
            setup: None,
            backend_unavailable_reason: None,
            max_players: None,
        }
    }
}

/// Where the core dials the running world: RakNet for Dragonfly, NetherNet for BDS, through its
/// LAN advertisement when the server publishes one.
pub(super) fn target_for(world: &World, instance: &dyn Instance) -> LocalTarget {
    let address = instance.address().to_owned();
    match (world.backend, instance.lan_address()) {
        (Backend::Bds, Some(lan)) if !lan.is_empty() => LocalTarget {
            address,
            transport: Transport::NetherNetLan,
            lan_address: Some(lan.to_owned()),
            level_name: Some(world.id.clone()),
        },
        (Backend::Bds, _) => LocalTarget {
            address,
            transport: Transport::NetherNet,
            lan_address: None,
            level_name: None,
        },
        (Backend::Dragonfly, _) => LocalTarget {
            address,
            transport: Transport::RakNet,
            lan_address: None,
            level_name: None,
        },
    }
}
