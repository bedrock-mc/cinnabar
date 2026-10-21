//! Hosts the Shader Pack Settings screen: reads the selected pack and its Iris option file on
//! a worker, runs the screen's commands through [`Editor`], saves changed values beside the
//! pack and reloads it, as Iris applies them.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};

use bevy::prelude::*;
use launcher::shader_options::{Action, Editor, Status, View};

mod writer;
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
    Loading(Mutex<mpsc::Receiver<Result<OptionSession, String>>>),
    Ready(Box<Editor>),
    Failed(String),
}

impl Open {
    /// Starts reading `pack` on a worker.
    fn start(pack: SelectedPack) -> Open {
        let (send, receive) = mpsc::channel();
        let (path, file) = (pack.path, pack.options.clone());
        let spawned = std::thread::Builder::new()
            .name("shader pack options".into())
            .spawn(move || {
                let _ = send.send(read_session(&path, file.as_deref()));
            });
        let state = match spawned {
            Ok(_) => State::Loading(Mutex::new(receive)),
            Err(error) => State::Failed(error.to_string()),
        };
        Open {
            name: pack.name,
            file: pack.options,
            state,
            saving: None,
            save_error: None,
        }
    }

    /// The screen to show.
    fn view(&self) -> View {
        let mut view = match &self.state {
            State::Loading(_) => View::loading(&self.name),
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
        let State::Loading(receive) = &mut self.state else {
            return Progress::default();
        };
        let received = receive
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv();
        self.state = match received {
            Ok(Ok(session)) => State::Ready(Box::new(Editor::new(session, &self.name))),
            Ok(Err(reason)) => State::Failed(reason),
            Err(mpsc::TryRecvError::Empty) => return Progress::default(),
            Err(mpsc::TryRecvError::Disconnected) => {
                State::Failed("the options reader stopped".to_owned())
            }
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

/// Opens the pack at `path` with the values saved in `file`.
fn read_session(path: &Path, file: Option<&Path>) -> Result<OptionSession, String> {
    let pack = ShaderPack::open(path).map_err(|error| error.to_string())?;
    let saved = match file {
        Some(file) => shaderpack::read_option_file(file)
            .map_err(|error| format!("{}: {error}", file.display()))?,
        None => OptionValues::new(),
    };
    Ok(OptionSession::new(&pack, &saved))
}

/// Runs the screen's commands, saving and reloading the pack when values are applied, and
/// publishes the screen whenever it changes.
pub(super) fn drive_shader_options(
    config: Res<RenderModeConfig>,
    settings: Res<RuntimeSettings>,
    menu: Option<ResMut<MenuRuntime>>,
    mut host: ResMut<ShaderOptionsHost>,
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
                *open = selected_pack(&config, &settings.user_settings_update().1.video)
                    .map(Open::start);
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
