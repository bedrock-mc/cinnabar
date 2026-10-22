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
    Loading(Mutex<shaderpack::worker::Running>, Instant),
    Ready(Box<Editor>),
    Failed(String),
}

impl Open {
    /// Starts a bounded options compiler; closing the screen kills and reaps that child.
    fn start(pack: SelectedPack, compiler: Option<&Path>) -> Open {
        let state = compiler
            .ok_or_else(|| "the isolated shader compiler is unavailable".to_owned())
            .and_then(|compiler| {
                shaderpack::worker::Running::start(
                    compiler,
                    &shaderpack::worker::Request {
                        pack: pack.path,
                        options: pack.options.clone(),
                        dimension: String::new(),
                        environment: shaderpack::Environment::default(),
                        purpose: shaderpack::worker::Purpose::Options,
                    },
                )
            });
        Open {
            name: pack.name,
            file: pack.options,
            state: match state {
                Ok(worker) => State::Loading(Mutex::new(worker), Instant::now()),
                Err(error) => State::Failed(error),
            },
            saving: None,
            save_error: None,
        }
    }

    /// The screen to show.
    fn view(&self) -> View {
        let mut view = match &self.state {
            State::Loading(..) => View::loading(&self.name),
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

/// Runs the screen's commands, saving and reloading the pack when values are applied, and
/// publishes the screen whenever it changes.
pub(super) fn drive_shader_options(
    config: Res<RenderModeConfig>,
    settings: Res<RuntimeSettings>,
    menu: Option<ResMut<MenuRuntime>>,
    mut host: ResMut<ShaderOptionsHost>,
    compiler: Option<Res<render_scene::shaderpack::ShaderPackCompilerExecutable>>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    let ShaderOptionsHost { open, writer } = &mut *host;
    let mut changed = false;
    for action in menu.take_shader_option_actions() {
        changed = true;
        if action == Action::Open {
            if open.as_ref().is_none_or(|open| open.saving.is_none()) {
                // Reap the previous reader before starting its replacement.
                *open = None;
                *open =
                    selected_pack(&config, &settings.user_settings_update().1.video).map(|pack| {
                        Open::start(
                            pack,
                            compiler.as_deref().map(|compiler| compiler.0.as_path()),
                        )
                    });
            }
            continue;
        }
        let Some(editor) = open.as_mut() else {
            continue;
        };
        if editor.handle(action, writer) {
            *open = None;
        }
    }
    if let Some(editor) = open.as_mut() {
        let progress = editor.poll(writer);
        changed |= progress.changed;
        if progress.reload {
            menu.request_shader_pack_reload();
        }
        if progress.close {
            *open = None;
        }
    }
    if changed {
        menu.sync_shader_options(open.as_ref().map(Open::view));
    }
}

#[cfg(test)]
mod tests;
