//! Servers that run ahead of an open where starts are slow: the most recently played world
//! started early, and a left world kept briefly for a return. Both stay hidden from status and the
//! core until an open of the same world adopts them.

use std::{
    sync::{Arc, MutexGuard},
    time::Instant,
};

use bridge::{Backend, WorldState};
use crossbeam_channel::{Sender, TryRecvError};
use launcher::local_worlds::{options::OptionChanges, world::SetupState};

use super::{Life, Manager, Retiring, host_xuid_of};
use crate::local_worlds::{
    Cancel, cancel_pair, lock,
    process::{HostGrant, Instance},
};

impl Manager {
    /// Starts the most recently played world's server in the background, hidden until an open of
    /// that world adopts it; a new world counts as played when created. Does nothing unless
    /// standby is configured, no world is open, that world runs on BDS, and the server is installed
    /// with its EULA accepted, so a prestart never downloads.
    pub fn prestart(&self) {
        if self.inner.standby.is_none() {
            return;
        }
        let Some(setup) = &self.inner.setup else {
            return;
        };
        let (status, _) = setup.status();
        if status.state == SetupState::Unsupported
            || !status.eula_accepted
            || setup.installed().is_none()
        {
            return;
        }
        let latest = match self.inner.store.list() {
            Ok(worlds) => worlds.into_iter().next(),
            Err(error) => {
                tracing::warn!("list local worlds to prestart one: {error}");
                return;
            }
        };
        let Some(world) = latest.filter(|world| world.backend == Backend::Bds) else {
            return;
        };
        let life = lock(&self.inner.life);
        if life.state != WorldState::Idle {
            return;
        }
        tracing::info!("prestarting local world {}", world.id);
        if let Err(error) = self.begin(life, &world.id, None, true) {
            tracing::warn!("prestart local world {}: {error}", world.id);
        }
    }

    /// Leaves the open world. With standby configured, a running BDS world's server keeps
    /// running, hidden, for the standby period, so returning to it skips its startup. A world with
    /// saved settings its server has not taken live, any other world and any other state close as
    /// [`Self::close`] does, so the next open starts with those settings.
    pub fn leave(&self) {
        let Some(keep) = self.inner.standby else {
            return self.close();
        };
        let mut life = lock(&self.inner.life);
        let keeps = life.state == WorldState::Running
            && !life.standby
            && life.instance.is_some()
            && life
                .world
                .as_ref()
                .is_some_and(|world| world.backend == Backend::Bds)
            && self.server_current(&life);
        if !keeps {
            return self.close_locked(life);
        }
        let (timer, expired) = cancel_pair();
        life.standby = true;
        life.standby_timer = Some(timer);
        life.unrecorded = Some(OptionChanges::default());
        life.paused = false;
        self.set_state(&mut life, WorldState::Running);
        drop(life);
        tracing::info!(
            "keeping the left local world's server for {} s",
            keep.as_secs()
        );
        let started = self.background(move |manager| {
            let cancelled = crossbeam_channel::select! {
                recv(expired.receiver()) -> _ => true,
                recv(crossbeam_channel::after(keep)) -> _ => false,
            };
            if !cancelled {
                manager.end_standby(&expired);
            }
        });
        if !started {
            self.close();
        }
    }

    /// Whether the running server already holds every saved setting of its world: nothing it was
    /// launched without remains pending, apart from edits it took live, and the world's name, game
    /// mode and difficulty are those it was launched with.
    fn server_current(&self, life: &Life) -> bool {
        let (Some(open), Some(launched)) = (&life.world, &life.launched) else {
            return false;
        };
        let Ok((saved, mut pending, _)) = self.inner.store.snapshot(&open.id) else {
            return false;
        };
        if let Some(applied) = &life.unrecorded {
            pending.acknowledge(applied);
        }
        if let Some((generation, live)) = &life.live_applied
            && *generation == life.generation
        {
            pending.acknowledge(live);
        }
        pending.is_empty()
            && saved.name == launched.name
            && saved.game_mode == launched.game_mode
            && saved.difficulty == launched.difficulty
    }

    /// Closes the hidden server whose keep-alive wait `timer` ended, unless an open adopted it or
    /// a close already ended it.
    fn end_standby(&self, timer: &Cancel) {
        let life = lock(&self.inner.life);
        if life.standby && !timer.is_cancelled() {
            tracing::info!("stopping the left local world's server");
            self.close_locked(life);
        }
    }

    /// Hands the hidden server to an open of its world. One still starting finishes as a normal
    /// start; a running one reports running once the host's operator grant is reconciled. Returns
    /// the cleanup to run after the lifecycle lock is released when no worker could take it over.
    pub(super) fn adopt(&self, life: &mut MutexGuard<'_, Life>) -> Option<AdoptCleanup> {
        life.standby = false;
        life.standby_timer = None;
        tracing::info!("opening the local world whose server is already starting or running");
        let instance = match (&life.instance, life.state) {
            (Some(instance), WorldState::Running) => Arc::clone(instance),
            _ => {
                let state = life.state;
                self.set_state(life, state);
                return None;
            }
        };
        let (handle, cancel) = cancel_pair();
        let (running, done) = crossbeam_channel::bounded::<()>(0);
        life.cancel_start = Some(handle);
        life.start_done = Some(done.clone());
        self.set_state(life, WorldState::Starting);
        let granted = life.granted.clone();
        let generation = life.generation;
        let fallback = Arc::clone(&instance);
        let barrier = running.clone();
        let started = self.background(move |manager| {
            manager.finish_adopt(instance, granted, &cancel, generation);
            drop(running);
        });
        if started {
            return None;
        }
        // Keeps the save ahead of any later open, as a close does.
        if let Some(world_id) = life.world.as_ref().map(|world| world.id.clone()) {
            life.retiring.push(Retiring { world_id, done });
        }
        let saved = Self::take_saved(life);
        life.cancel_start = None;
        life.start_done = None;
        life.instance = None;
        life.failure = Some(super::WorldError::Cancelled.status_text());
        self.set_state(life, WorldState::Failed);
        Some(AdoptCleanup {
            instance: fallback,
            saved,
            barrier,
        })
    }

    /// Reconciles an adopted server's operator grant, launched as `granted`, then reports it
    /// running unless it exited meanwhile. A cancelled or failed adoption stops the server and
    /// records the live edits it saved.
    fn finish_adopt(
        &self,
        instance: Arc<dyn Instance>,
        granted: Option<String>,
        cancel: &Cancel,
        generation: u64,
    ) {
        let adopted = Arc::clone(&instance);
        let deadline = Instant::now() + self.inner.host_wait;
        let reconciled = match self.inner.runners.host_grant(Backend::Bds) {
            HostGrant::WhenReady => self
                .grant_when_ready(instance, granted.as_deref(), deadline, cancel)
                .map(|(_, host)| Some(host)),
            HostGrant::None | HostGrant::AtLaunch => Ok(None),
        };
        let mut life = lock(&self.inner.life);
        if cancel.is_cancelled() || reconciled.is_err() {
            let saved = (life.generation == generation)
                .then(|| Self::take_saved(&mut life))
                .flatten();
            if let Err(error) = &reconciled
                && !cancel.is_cancelled()
            {
                // The failed grant already stopped the server.
                tracing::error!("adopt the running local world: {error}");
                life.instance = None;
                life.failure = Some(error.status_text());
                self.set_state(&mut life, WorldState::Failed);
            }
            drop(life);
            if let Ok(host) = reconciled {
                drop(host);
            }
            self.stop_and_acknowledge(adopted.as_ref(), saved);
            if cancel.is_cancelled() {
                self.idle_if_current(generation);
            }
            return;
        }
        let Ok(host) = reconciled else { return };
        if matches!(adopted.exited().try_recv(), Err(TryRecvError::Disconnected)) {
            // Its watcher saw the adoption's start state and left the exit to this check.
            drop(host);
            return self.fail_exited(life, &adopted);
        }
        if let Some(host) = &host {
            life.granted = host_xuid_of(host);
        }
        let applied = life.unrecorded.take().unwrap_or_default();
        let id = life.world.as_ref().map(|world| world.id.clone());
        self.set_state(&mut life, WorldState::Running);
        drop(host);
        drop(life);
        if let Some(id) = id {
            self.record_played(&id, &applied);
        }
    }

    /// Stops a hidden server of world `id` before the world changes; `wait` also waits until it
    /// has saved and exited.
    pub(super) fn retire_standby(&self, id: &str, wait: bool) {
        let life = lock(&self.inner.life);
        if !life.standby || life.world.as_ref().is_none_or(|world| world.id != id) {
            return;
        }
        self.close_locked(life);
        if wait {
            let saving: Vec<_> = lock(&self.inner.life)
                .retiring
                .iter()
                .filter(|retiring| retiring.world_id == id)
                .map(|retiring| retiring.done.clone())
                .collect();
            for done in saving {
                let _ = done.recv();
            }
        }
    }

    /// Records that world `id` was played and its start applied `applied`.
    pub(super) fn record_played(&self, id: &str, applied: &OptionChanges) {
        if let Err(error) = self.inner.store.touch(id, applied) {
            tracing::warn!("record when {id} was last played: {error}");
        }
    }
}

/// Stops a server whose adoption no worker took over, outside the lifecycle lock.
pub(super) struct AdoptCleanup {
    instance: Arc<dyn Instance>,
    saved: Option<(String, OptionChanges)>,
    /// Releases the save barrier once the server has stopped.
    barrier: Sender<()>,
}

impl AdoptCleanup {
    /// Stops the server, records the live edits it saved, then releases its barrier.
    pub(super) fn run(self, manager: &Manager) {
        manager.stop_and_acknowledge(self.instance.as_ref(), self.saved);
        drop(self.barrier);
    }
}
