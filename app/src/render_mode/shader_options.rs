//! Hosts the Shader Pack Settings screen: reads the selected pack and its Iris option file on
//! a worker, runs the screen's commands through [`Editor`], saves changed values beside the
//! pack and reloads it, as Iris applies them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use bevy::prelude::*;
use launcher::shader_options::{Action, Editor, Status, View};

mod writer;
#[cfg(test)]
use shaderpack::{OptionSession, OptionValues, ShaderPack};
use writer::{Request, Writer};

use super::shader_pack::{SelectedPack, selected_pack};
use super::{MenuRuntime, RenderModeConfig, RuntimeSettings};

/// The open screen, if any.
#[derive(Resource, Default)]
pub(super) struct ShaderOptionsHost {
    open: Option<Open>,
    writer: Option<Writer>,
    retiring: Option<Mutex<shaderpack::worker::Running>>,
}

/// One open screen and where its pack's values are saved.
struct Open {
    name: String,
    file: Option<PathBuf>,
    state: State,
    saving: Option<Saving>,
    save_error: Option<String>,
}

/// An editor candidate committed only after its option file is saved.
struct Saving {
    candidate: Box<Editor>,
    close: bool,
}

/// UI and renderer changes authorized by a completed worker result.
#[derive(Default)]
struct Progress {
    changed: bool,
    reload: bool,
    close: bool,
}

enum State {
    Waiting(shaderpack::worker::Request),
    Loading(Mutex<shaderpack::worker::Running>, Instant),
    Ready(Box<Editor>),
    Failed(String),
}

impl Open {
    /// Keeps the latest request queued until the previous reader has retired.
    fn pending(pack: SelectedPack) -> Open {
        Open {
            name: pack.name,
            file: pack.options.clone(),
            state: State::Waiting(shaderpack::worker::Request {
                pack: pack.path,
                options: pack.options,
                dimension: String::new(),
                environment: shaderpack::Environment::default(),
                purpose: shaderpack::worker::Purpose::Options,
            }),
            saving: None,
            save_error: None,
        }
    }

    /// Starts the queued reader without allowing overlapping readers or result decoders.
    fn start(&mut self, compiler: Option<&Path>) -> bool {
        let State::Waiting(request) = &self.state else {
            return false;
        };
        let result = compiler
            .ok_or_else(|| "the isolated shader compiler is unavailable".to_owned())
            .and_then(|compiler| shaderpack::worker::Running::start(compiler, request));
        self.state = match result {
            Ok(worker) => State::Loading(Mutex::new(worker), Instant::now()),
            Err(reason) => State::Failed(reason),
        };
        true
    }

    /// The screen to show.
    fn view(&self) -> View {
        let mut view = match &self.state {
            State::Waiting(_) | State::Loading(..) => View::loading(&self.name),
            State::Ready(editor) => editor.view(),
            State::Failed(reason) => View::failed(&self.name, reason.clone()),
        };
        if self.saving.is_some() {
            view.status = Status::Saving;
        } else if let Some(error) = &self.save_error {
            view.status = Status::SaveFailed(error.clone());
        }
        view
    }

    /// Takes worker results and commits only saves accepted by storage.
    fn poll(&mut self, writer: &mut Option<Writer>) -> Progress {
        if self.saving.is_some() {
            return writer
                .as_mut()
                .and_then(Writer::poll)
                .map_or_else(Progress::default, |result| self.finish_save(result));
        }
        let State::Loading(worker, started) = &mut self.state else {
            return Progress::default();
        };
        let Some(result) = worker
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .poll(started.elapsed(), false)
        else {
            return Progress::default();
        };
        self.state = match result.and_then(shaderpack::worker::Response::into_options) {
            Ok(session) => State::Ready(Box::new(Editor::new(session, &self.name))),
            Err(reason) => State::Failed(reason),
        };
        Progress {
            changed: true,
            ..Default::default()
        }
    }

    /// Keeps failed edits available, or commits the saved candidate and requests its reload.
    fn finish_save(&mut self, result: Result<(), String>) -> Progress {
        let Some(saving) = self.saving.take() else {
            return Progress::default();
        };
        match result {
            Ok(()) => {
                self.state = State::Ready(saving.candidate);
                self.save_error = None;
                Progress {
                    changed: true,
                    reload: true,
                    close: saving.close,
                }
            }
            Err(error) => {
                self.save_error = Some(error);
                Progress {
                    changed: true,
                    ..Default::default()
                }
            }
        }
    }

    /// Runs a command against a candidate, retaining the live editor until its save succeeds.
    fn handle(&mut self, action: Action, writer: &mut Option<Writer>) -> bool {
        if self.saving.is_some() {
            return false;
        }
        let State::Ready(editor) = &mut self.state else {
            return matches!(action, Action::Back | Action::Cancel | Action::Done);
        };
        let saves = match action {
            Action::Apply | Action::Done | Action::Reset => true,
            Action::Back => editor.session().screen_id().is_none(),
            Action::Open | Action::Element { .. } | Action::Cancel => false,
        };
        if !saves {
            return editor.handle(action).close;
        }
        let mut candidate = editor.clone();
        let outcome = candidate.handle(action);
        let Some(values) = outcome.save else {
            *editor = candidate;
            return outcome.close;
        };
        let result = (|| {
            let path = self
                .file
                .clone()
                .ok_or("the pack has no option-file path")?;
            if writer.is_none() {
                *writer = Some(Writer::new().map_err(|error| error.to_string())?);
            }
            writer
                .as_ref()
                .expect("the writer was started")
                .submit(Request {
                    path,
                    options: candidate.session().options().to_vec(),
                    values,
                })
        })();
        match result {
            Ok(()) => {
                self.save_error = None;
                self.saving = Some(Saving {
                    candidate,
                    close: outcome.close,
                });
            }
            Err(error) => {
                self.save_error = Some(error);
                *writer = None;
            }
        }
        false
    }
}

/// Cancels a screen's active reader without making the frame wait for its result decoder.
fn retire_reader(
    open: &mut Option<Open>,
    retiring: &mut Option<Mutex<shaderpack::worker::Running>>,
) {
    if let Some(Open {
        state: State::Loading(reader, _),
        ..
    }) = open.take()
    {
        debug_assert!(retiring.is_none(), "only one reader may run at a time");
        *retiring = Some(reader);
    }
}

/// Reaps a cancelled reader after it has stopped, without waiting on the frame thread.
fn settle_reader(retiring: &mut Option<Mutex<shaderpack::worker::Running>>) {
    if retiring.as_mut().is_some_and(|reader| {
        reader
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .poll(std::time::Duration::ZERO, true)
            .is_some()
    }) {
        *retiring = None;
    }
}

/// Runs the screen's commands, saving and reloading the pack when values are applied, and
/// publishes the screen whenever it changes.
pub(super) fn drive_shader_options(
    config: Res<RenderModeConfig>,
    settings: Res<RuntimeSettings>,
    menu: Option<ResMut<MenuRuntime>>,
    mut host: ResMut<ShaderOptionsHost>,
    compiler: Option<Res<render_scene::shaderpack::ShaderPackCompilerExecutable>>,
) {
    let ShaderOptionsHost {
        open,
        writer,
        retiring,
    } = &mut *host;
    let Some(mut menu) = menu else {
        retire_reader(open, retiring);
        settle_reader(retiring);
        return;
    };
    let mut changed = false;
    for action in menu.take_shader_option_actions() {
        changed = true;
        if action == Action::Open {
            if open.as_ref().is_none_or(|open| open.saving.is_none()) {
                retire_reader(open, retiring);
                *open = selected_pack(&config, &settings.user_settings_update().1.video)
                    .map(Open::pending);
            }
            continue;
        }
        let Some(editor) = open.as_mut() else {
            continue;
        };
        if editor.handle(action, writer) {
            retire_reader(open, retiring);
        }
    }
    if open.as_ref().is_some_and(|editor| editor.saving.is_none())
        && menu.view().shader_options.is_none()
    {
        retire_reader(open, retiring);
        changed = true;
    }
    settle_reader(retiring);
    if let Some(editor) = open.as_mut() {
        if retiring.is_none() {
            changed |= editor.start(compiler.as_deref().map(|compiler| compiler.0.as_path()));
        }
        let progress = editor.poll(writer);
        changed |= progress.changed;
        if progress.reload {
            menu.request_shader_pack_reload();
        }
        if progress.close {
            retire_reader(open, retiring);
        }
    }
    if changed {
        menu.sync_shader_options(open.as_ref().map(Open::view));
    }
}

#[cfg(test)]
mod tests;
