//! The frame system that runs the store: it attaches the worker to the launcher core, feeds queued
//! player actions and worker events through [`StoreState`], and publishes the result to the menu.

use std::sync::Arc;

use bevy::prelude::{Commands, Res, ResMut};

use super::state::StoreState;
use crate::menu::{MenuRuntime, launcher_account::LauncherAccount};
use launcher::store::snapshot::StoreSnapshot;
use {
    super::worker::StoreWorker,
    launcher::store::worker::{StoreError, StoreRequest},
};

/// Send `requests`, telling the state about any the queue refused.
fn send_all(state: &mut StoreState, worker: &StoreWorker, requests: Vec<StoreRequest>) {
    for request in requests {
        if !worker.send(request.clone()) {
            state.refused(&request);
        }
    }
}

pub(crate) fn drive_store(
    mut commands: Commands,
    mut menu: ResMut<MenuRuntime>,
    account: Option<Res<LauncherAccount>>,
    worker: Option<Res<StoreWorker>>,
    state: Option<ResMut<StoreState>>,
) {
    let Some(account) = account else {
        // Without a launcher core the store has no service; drop any session left over.
        if worker.is_some() {
            commands.remove_resource::<StoreWorker>();
            commands.remove_resource::<StoreState>();
        }
        let _ = menu.take_store_actions();
        let unavailable = menu.in_store().then(|| {
            Arc::new(StoreSnapshot {
                loading: false,
                failure: Some(StoreError::Unavailable),
                ..StoreSnapshot::empty()
            })
        });
        menu.set_store_snapshot(unavailable);
        return;
    };
    let (Some(worker), Some(mut state)) = (worker, state) else {
        let mut fresh = StoreState::new();
        fresh.set_settings(launcher::store::settings::load(&menu.store_settings_path()));
        commands.insert_resource(StoreWorker::new(account.socket_dir().to_path_buf()));
        commands.insert_resource(fresh);
        let loading = menu.in_store().then(|| Arc::new(StoreSnapshot::empty()));
        menu.set_store_snapshot(loading);
        return;
    };
    for action in menu.take_store_actions() {
        let requests = state.act(action);
        send_all(&mut state, &worker, requests);
    }
    for event in worker.poll() {
        let requests = state.apply(event);
        send_all(&mut state, &worker, requests);
    }
    if state.take_exit() {
        menu.leave_store();
    }
    if state.take_dirty() {
        let snapshot = menu.in_store().then(|| Arc::new(state.snapshot()));
        menu.set_store_snapshot(snapshot);
    }
}
