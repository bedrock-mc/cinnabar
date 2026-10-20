//! An opaque identity for the actual running server opening, without owning or stopping it.

use super::*;
use std::{io, net::SocketAddr, sync::Weak, time::Instant};

/// Holds weak ownership so retained remote cleanup cannot keep its local server manager alive.
#[derive(Clone)]
pub(crate) struct HostOpening {
    manager: Weak<Inner>,
    instance: Weak<dyn Instance>,
    generation: u64,
    world: LocalWorldInfo,
    target: LocalTarget,
    max_players: u32,
}

impl HostOpening {
    /// Captures the manager's actual live generation and server instance under its lifecycle lock.
    pub(super) fn capture(manager: &Manager) -> io::Result<Self> {
        let life = lock(&manager.inner.life);
        let instance = life
            .instance
            .as_ref()
            .filter(|_| life.state == WorldState::Running)
            .ok_or_else(unavailable)?;
        if !matches!(instance.exited().try_recv(), Err(TryRecvError::Empty)) {
            return Err(unavailable());
        }
        let world = life.world.as_ref().ok_or_else(unavailable)?;
        Ok(Self {
            manager: Arc::downgrade(&manager.inner),
            instance: Arc::downgrade(instance),
            generation: life.generation,
            world: LocalWorldInfo {
                id: world.id.clone(),
                name: world.name.clone(),
                game_mode: world.game_mode,
                backend: world.backend,
            },
            target: target_for(world, instance.as_ref()),
            max_players: instance
                .max_players()
                .unwrap_or(bridge::contract::HOSTED_WORLD_MAX_PLAYERS)
                .min(bridge::contract::HOSTED_WORLD_MAX_PLAYERS),
        })
    }

    /// Denies closed, failed, replaced and reopened servers, including reuse of the same world/address.
    pub(crate) fn validate(&self) -> io::Result<()> {
        let manager = self.manager.upgrade().ok_or_else(unavailable)?;
        let life = lock(&manager.life);
        if !self.current(&manager, &life) {
            return Err(unavailable());
        }
        Ok(())
    }

    /// Shares the same exact original-instance predicate with lock-held local retirement.
    fn current(&self, manager: &Arc<Inner>, life: &Life) -> bool {
        let Some(instance) = life
            .instance
            .as_ref()
            .filter(|_| life.state == WorldState::Running)
        else {
            return false;
        };
        self.manager.ptr_eq(&Arc::downgrade(manager))
            && life.generation == self.generation
            && self.instance.ptr_eq(&Arc::downgrade(instance))
            && life
                .world
                .as_ref()
                .is_some_and(|world| world.id == self.world.id)
            && matches!(instance.exited().try_recv(), Err(TryRecvError::Empty))
    }

    /// Requests saving/stop only for this original still-running process, on the worlds worker.
    pub(super) fn request_close(&self) {
        let Some(inner) = self.manager.upgrade() else {
            return;
        };
        let manager = Manager { inner };
        let life = lock(&manager.inner.life);
        if self.current(&manager.inner, &life) {
            manager.close_locked(life);
        }
    }

    /// Matches exact opening ownership without accepting display text as a locator.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.manager.ptr_eq(&other.manager)
            && self.instance.ptr_eq(&other.instance)
            && self.generation == other.generation
    }

    /// Borrows the metadata captured from this same actual local-world opening.
    pub(crate) fn world(&self) -> &LocalWorldInfo {
        &self.world
    }

    /// Borrows the exact immutable endpoint captured with this original process generation.
    pub(super) fn target(&self) -> &LocalTarget {
        &self.target
    }

    /// Returns the running instance's finite supported player cap, including its local host.
    pub(crate) fn max_players(&self) -> u32 {
        self.max_players
    }

    /// Projects only the original still-running local route, shared by player and friend serving.
    pub(crate) fn native_connection_target(&self) -> io::Result<protocol::NativeConnectionTarget> {
        self.validate()?;
        let local = self.target();
        if (self.world().backend, local.transport) == (Backend::Bds, Transport::NetherNetLan) {
            let address = local
                .lan_address
                .as_deref()
                .and_then(|value| value.parse::<SocketAddr>().ok())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "local world returned an invalid LAN endpoint",
                    )
                })?;
            let level = local
                .level_name
                .as_deref()
                .filter(|level| *level == self.world().id.as_str())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "local world returned another LAN level",
                    )
                })?;
            return protocol::NativeConnectionTarget::local_lan(
                address,
                level.to_owned(),
                std::time::Duration::from_millis(u64::from(
                    bridge::contract::LOCAL_LAN_DISCOVERY_TIMEOUT_MS,
                )),
            );
        }
        let transport = match (self.world().backend, local.transport) {
            (Backend::Dragonfly, Transport::RakNet) => protocol::NativeTransport::RakNet,
            (Backend::Bds, Transport::NetherNet) => protocol::NativeTransport::LocalNetherNetHttp,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "local world advertises an unsupported native transport",
                ));
            }
        };
        let address = local
            .address
            .parse::<SocketAddr>()
            .ok()
            .filter(|address| address.ip().is_loopback() && address.port() != 0)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "local world returned an invalid loopback endpoint",
                )
            })?;
        Ok(protocol::NativeConnectionTarget::new(
            address.to_string(),
            transport,
        ))
    }

    /// Uses only the captured original child; callers dispatch this pipe write on the existing worlds worker.
    pub(crate) fn request_bridge(
        &self,
        bootstrap: &bridge::host_world::contract::Bootstrap,
    ) -> io::Result<()> {
        self.validate()?;
        if self.world.backend != bridge::Backend::Dragonfly {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "native world bridge requires Dragonfly",
            ));
        }
        self.instance
            .upgrade()
            .ok_or_else(unavailable)?
            .native_world_bridge(bootstrap)?;
        self.validate()
    }
}

impl Manager {
    /// Reserves this manager's sole unadvertised native host directory owner on the caller's runtime.
    /// No server process is started/stopped; publication, service progress and cleanup are explicit.
    /// The selected account must already have been verified through its existing account operation.
    pub fn native_hosted_world(
        &self,
        account: Arc<crate::native_account::NativeAccount>,
        deadline: Instant,
    ) -> io::Result<crate::native_account::NativeHostedWorld> {
        self.inner
            .native_host
            .open(account, HostOpening::capture(self)?, deadline)
    }

    /// Resumes exact unresolved remote cleanup without requiring the old local server to remain open.
    /// Another host cannot reserve the slot until this retained obligation closes successfully.
    pub fn native_host_cleanup(
        &self,
    ) -> io::Result<Option<crate::native_account::NativeHostedWorld>> {
        self.inner.native_host.cleanup()
    }
}

/// Keeps local server identifiers and loopback destinations out of diagnostics.
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "native hosted world opening is no longer running",
    )
}

/// User intent tied to one actual original opening, never a mutable latest-world locator.
#[derive(Clone)]
pub struct NativeHostIntent {
    manager: Manager,
    opening: HostOpening,
}

impl NativeHostIntent {
    /// Compares exact server ownership without touching credentials or the filesystem on the frame.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.opening.same(&other.opening)
    }

    /// Verifies the selected account and reserves only this captured still-running opening.
    pub(crate) async fn open(
        &self,
        account: Arc<crate::native_account::NativeAccount>,
        deadline: Instant,
    ) -> io::Result<crate::native_account::NativeHostedWorld> {
        self.opening.validate()?;
        tokio::time::timeout_at(deadline.into(), Arc::clone(&account).validate())
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "native hosting account timed out")
            })??;
        self.opening.validate()?;
        self.manager
            .inner
            .native_host
            .open(account, self.opening.clone(), deadline)
    }

    /// Resumes this manager's exact remote obligation before another activation is attempted.
    pub(crate) async fn cleanup(&self, deadline: Instant) -> io::Result<()> {
        if let Some(mut pending) = self.manager.native_host_cleanup()? {
            pending.close(deadline).await?;
        }
        Ok(())
    }
}

impl Manager {
    /// Captures a supported original backend for the sole selected native hosting owner.
    pub fn native_host_intent(&self) -> io::Result<NativeHostIntent> {
        let opening = HostOpening::capture(self)?;
        if opening.max_players() < 2 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "native hosting requires an original visitor slot",
            ));
        }
        if opening.world().backend == bridge::Backend::Bds {
            opening.native_connection_target()?;
        }
        Ok(NativeHostIntent {
            manager: self.clone(),
            opening,
        })
    }
}
