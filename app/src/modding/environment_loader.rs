//! Environment-selected component reloads prepare guests, fonts and UI together off-frame.

use super::{ModRuntime, RELOAD_INTERVAL, admission, hud_editor};
use bevy::prelude::*;
use client_extensions::modding::font;
use client_shell::ui_resources::UiPresentationResource;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select};
use mod_host::{ModGrants, ModHost};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, thread::JoinHandle, time::Instant};

#[derive(Clone)]
pub(super) struct Source {
    pub(super) owner: mod_ui::PanelUiOwner,
    pub(super) component: PathBuf,
    pub(super) grants: ModGrants,
    pub(super) font: Option<PathBuf>,
    pub(super) ui: Option<PathBuf>,
}

struct Request {
    source: Source,
    previous: [u8; 32],
    settings: Option<(PathBuf, String)>,
}
struct Update {
    source: Source,
    previous: [u8; 32],
    result: Result<admission::PreparedMod, String>,
}
enum Message {
    Updated(Box<Update>),
    Complete,
}

#[derive(Resource)]
pub(super) struct Loader {
    sources: Vec<Source>,
    requests: Option<Sender<Vec<Request>>>,
    results: Receiver<Message>,
    retired: Sender<ModHost>,
    pending_retired: Vec<ModHost>,
    thread: Option<JoinHandle<()>>,
    pending: bool,
    last_poll: Instant,
}

impl Loader {
    /// Starts one bounded worker for the already accepted environment component identities.
    pub(super) fn start(sources: Vec<Source>) -> Result<Self, String> {
        if sources.len() > mod_api::MAX_LOADED_MODS {
            return Err("environment component list exceeds the host limit".into());
        }
        let (requests, incoming) = bounded::<Vec<Request>>(1);
        // One outstanding batch contains at most MAX_LOADED_MODS results plus its completion.
        let (results, completed) = bounded(mod_api::MAX_LOADED_MODS + 1);
        let (retired, retiring) = bounded(mod_api::MAX_LOADED_MODS * 2);
        let thread = std::thread::Builder::new().name("mod-environment-loader".into())
            .spawn(move || {
                let mut attempted = BTreeMap::new();
                loop {
                    select! {
                        recv(retiring) -> host => match host {
                            Ok(host) => drop(host),
                            Err(_) => break,
                        },
                        recv(incoming) -> batch => match batch {
                            Ok(batch) => {
                                for request in batch {
                                    if let Some(update) = prepare(request, &mut attempted)
                                        && results.send(Message::Updated(Box::new(update))).is_err() {
                                        return;
                                    }
                                }
                                if results.send(Message::Complete).is_err() { return; }
                            },
                            Err(_) => break,
                        }
                    }
                }
            }).map_err(|error| error.to_string())?;
        Ok(Self {
            sources,
            requests: Some(requests),
            results: completed,
            retired,
            pending_retired: Vec::new(),
            thread: Some(thread),
            pending: false,
            last_poll: Instant::now(),
        })
    }

    /// Hands retired hosts back to the worker without joining their execution/settings lanes.
    fn retire(&mut self, host: ModHost) {
        match self.retired.try_send(host) {
            Ok(()) => {}
            Err(TrySendError::Full(host)) => self.pending_retired.push(host),
            Err(TrySendError::Disconnected(host)) => self.pending_retired.push(host),
        }
    }
}

impl Drop for Loader {
    /// Joins the worker during client shutdown, after closing its bounded request lane.
    fn drop(&mut self) {
        self.requests.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Captures one changed component and all explicit overrides before initializing its candidate.
fn prepare(
    request: Request,
    attempted: &mut BTreeMap<(mod_ui::PanelUiOwner, PathBuf), [u8; 32]>,
) -> Option<Update> {
    let key = (request.source.owner, request.source.component.clone());
    let mut source_hash = None;
    let result = (|| {
        let bytes = mod_host::local_files::read(
            &request.source.component,
            mod_host::MAX_COMPONENT_SOURCE_BYTES,
        )
        .map_err(|error| error.to_string())?;
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        if hash == request.previous || attempted.get(&key) == Some(&hash) {
            return Ok(None);
        }
        source_hash = Some(hash);
        let read = |path: &Option<PathBuf>, max| -> Result<Option<Vec<u8>>, String> {
            if !request.source.grants.controls {
                return Ok(None);
            }
            path.as_deref()
                .map(|path| {
                    mod_host::local_files::read(path, max).map_err(|error| error.to_string())
                })
                .transpose()
        };
        let font = read(&request.source.font, font::MAX_SOURCE_BYTES)?;
        let ui = read(&request.source.ui, ui::mod_panel::MAX_PANEL_BUNDLE_BYTES)?;
        let prepared = admission::prepare(
            &request.source.component,
            &bytes,
            request.source.grants.clone(),
            font.as_deref(),
            ui.as_deref(),
            request
                .settings
                .as_ref()
                .map(|(path, json)| (path.as_path(), json.as_str())),
        )?;
        Ok(Some(prepared))
    })();
    match result {
        Ok(None) => None,
        Ok(Some(prepared)) => Some(Update {
            source: request.source,
            previous: request.previous,
            result: Ok(prepared),
        }),
        Err(error) => {
            if let Some(hash) = source_hash {
                attempted.insert(key, hash);
            }
            Some(Update {
                source: request.source,
                previous: request.previous,
                result: Err(error),
            })
        }
    }
}

/// Publishes an accepted guest and both resources together before native input sampling.
fn apply(world: &mut World, update: Update) -> Option<ModHost> {
    let index = world.get_resource::<ModRuntime>().and_then(|runtime| {
        runtime
            .metadata
            .iter()
            .enumerate()
            .find_map(|(index, metadata)| {
                let host = runtime.hosts.host(index);
                (metadata.owner == update.source.owner
                    && metadata.registration.is_none()
                    && host.component_source() == Some(update.source.component.as_path())
                    && host.source_digest() == update.previous)
                    .then_some(index)
            })
    });
    let Some(index) = index else {
        return update.result.ok().map(|prepared| prepared.host);
    };
    let mut prepared = match update.result {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!(
                "Cinnabar environment component reload rejected; previous guest retained: {error}"
            );
            return None;
        }
    };
    let old = world.resource::<ModRuntime>().hosts.host(index);
    if prepared.host.settings_path().is_some()
        && prepared.host.settings_path() == old.settings_path()
        && prepared.host.settings_seed() != old.settings_snapshot()
    {
        // Successful preparation is never cached as a failed attempt, so the next
        // bounded poll retries this source with the current committed preferences.
        return Some(prepared.host);
    }
    let Some(mut presentation) = world.remove_resource::<UiPresentationResource>() else {
        return Some(prepared.host);
    };
    let retired = {
        let mut runtime = world.resource_mut::<ModRuntime>();
        if runtime
            .hud_editor_owner
            .is_some_and(|owner| owner.host == index)
        {
            hud_editor::cancel(&mut runtime, &mut presentation, false);
        }
        prepared
            .host
            .activate_settings(Some(runtime.hosts.host_mut(index)));
        let retired = std::mem::replace(runtime.hosts.host_mut(index), prepared.host);
        runtime.metadata[index].font = prepared.font;
        runtime.metadata[index].branding = prepared.branding;
        presentation
            .mod_ui_mut()
            .set_prepared_panel_ui(update.source.owner, prepared.panel_ui);
        runtime.render_sources.clear();
        retired
    };
    world.insert_resource(presentation);
    if let Some(mut cues) = world.get_resource_mut::<super::ModCueFeed>() {
        cues.0.clear();
    }
    super::registration::presentation::refresh_presentation(world);
    Some(retired)
}

/// Polls bounded channels each frame; source reads, parsing, compilation and rasterization stay off-frame.
pub(super) fn poll(world: &mut World) {
    let Some(mut loader) = world.remove_resource::<Loader>() else {
        return;
    };
    while let Ok(message) = loader.results.try_recv() {
        match message {
            Message::Updated(update) => {
                if let Some(retired) = apply(world, *update) {
                    loader.retire(retired);
                }
            }
            Message::Complete => loader.pending = false,
        }
    }
    let retired = std::mem::take(&mut loader.pending_retired);
    for host in retired {
        loader.retire(host);
    }
    if !loader.pending
        && loader.pending_retired.is_empty()
        && loader.last_poll.elapsed() >= RELOAD_INTERVAL
    {
        loader.last_poll = Instant::now();
        if let Some(runtime) = world.get_resource::<ModRuntime>() {
            let requests = loader
                .sources
                .iter()
                .filter_map(|source| {
                    let index = runtime.metadata.iter().position(|metadata| {
                        metadata.owner == source.owner && metadata.registration.is_none()
                    })?;
                    let host = runtime.hosts.host(index);
                    (host.component_source() == Some(source.component.as_path())).then(|| Request {
                        source: source.clone(),
                        previous: host.source_digest(),
                        settings: host
                            .settings_path()
                            .zip(host.settings_snapshot())
                            .map(|(path, json)| (path.to_owned(), json.to_owned())),
                    })
                })
                .collect();
            if let Some(sender) = &loader.requests
                && sender.try_send(requests).is_ok()
            {
                loader.pending = true;
            }
        }
    }
    world.insert_resource(loader);
}

#[cfg(test)]
mod tests;
