//! Servers that run ahead of an open where starts are slow: the most recently played world
//! started early, and a left world kept briefly for a return. Both stay hidden from status and the
//! core until an open of the same world adopts them.

use std::{
    sync::{Arc, MutexGuard},
    time::Instant,
};

use bridge::{Backend, WorldState};
use launcher::local_worlds::{options::OptionChanges, world::SetupState};

use super::{Life, Manager, host_xuid_of};
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
    /// running, hidden, for the standby period, so returning to it skips its startup; any other
    /// world or state closes as [`Self::close`] does.
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
                .is_some_and(|world| world.backend == Backend::Bds);
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
    /// start; a running one reports running once the host's operator grant is reconciled.
    pub(super) fn adopt(&self, life: &mut MutexGuard<'_, Life>) {
        life.standby = false;
        life.standby_timer = None;
        tracing::info!("opening the local world whose server is already starting or running");
        let instance = match (&life.instance, life.state) {
            (Some(instance), WorldState::Running) => Arc::clone(instance),
            _ => {
                let state = life.state;
                self.set_state(life, state);
                return;
            }
        };
        let (handle, cancel) = cancel_pair();
        let (running, done) = crossbeam_channel::bounded::<()>(0);
        life.cancel_start = Some(handle);
        life.start_done = Some(done);
        self.set_state(life, WorldState::Starting);
        let granted = life.granted.clone();
        let generation = life.generation;
        let fallback = Arc::clone(&instance);
        let started = self.background(move |manager| {
            manager.finish_adopt(instance, granted, &cancel, generation);
            drop(running);
        });
        if !started {
            // The exit sweep reaps the server once it has saved.
            fallback.request_stop();
            life.instance = None;
            life.failure = Some(super::WorldError::Cancelled.status_text());
            self.set_state(life, WorldState::Failed);
        }
    }

    /// Reconciles an adopted server's operator grant, launched as `granted`, then reports it
    /// running; a cancelled adoption stops the server.
    fn finish_adopt(
        &self,
        instance: Arc<dyn Instance>,
        granted: Option<String>,
        cancel: &Cancel,
        generation: u64,
    ) {
        let deadline = Instant::now() + self.inner.host_wait;
        let reconciled = match self.inner.runners.host_grant(Backend::Bds) {
            HostGrant::WhenReady => self
                .grant_when_ready(instance, granted.as_deref(), deadline, cancel)
                .map(|(instance, host)| (instance, Some(host))),
            HostGrant::None | HostGrant::AtLaunch => Ok((instance, None)),
        };
        let mut life = lock(&self.inner.life);
        if cancel.is_cancelled() {
            drop(life);
            if let Ok((instance, host)) = reconciled {
                drop(host);
                self.stop_instance(instance.as_ref());
            }
            self.idle_if_current(generation);
            return;
        }
        match reconciled {
            Ok((_, host)) => {
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
            Err(error) => {
                // The failed grant already stopped the server.
                tracing::error!("adopt the running local world: {error}");
                life.instance = None;
                life.failure = Some(error.status_text());
                self.set_state(&mut life, WorldState::Failed);
            }
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
