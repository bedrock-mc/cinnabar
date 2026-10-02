use protocol::world_control::{
    Backend, Difficulty, GameMode, Generator, Prefs, Setup, SetupState, UnavailableReason, World,
    WorldState, WorldStatus, WorldUpdate,
};

use super::form::{CreateForm, EditForm, validate_name};
use super::progress::{self, Progress};
use super::prompt::{DOCKER_URL, Prompt, PromptButton, PromptFor, PromptKind};

const FALLBACK_ERROR: &str = "The local world could not be started";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Screen {
    #[default]
    List,
    Create,
    /// Create from template: the core has no templates, so this is vanilla's empty state.
    Templates,
    /// World settings of the selected world.
    Edit,
    /// Delete confirmation over the edit screen.
    ConfirmDelete,
    /// "Do you want to save your changes?" when leaving the edit screen with edits.
    ConfirmLeaveEdit,
    /// Waiting for the core to bring the chosen world up.
    Opening,
    /// The user must accept the Minecraft EULA before the server is downloaded.
    Eula,
    /// Default worlds need Docker; see [`WorldsMenu::prompt`].
    BackendPrompt,
    Error,
}

/// User intents; the embedding menu maps its buttons and text fields onto these.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Input {
    Refresh,
    Select(usize),
    BeginCreate,
    OpenTemplates,
    SelectTab(Tab),
    SetName(String),
    SetSeed(String),
    /// Game mode of the create or edit form, whichever is up.
    SetGameMode(GameMode),
    SetDifficulty(Difficulty),
    SetFlat(bool),
    SubmitCreate,
    /// Opens the settings of the world at this list index.
    BeginEdit(usize),
    SetEditName(String),
    SubmitEdit,
    DiscardEdit,
    /// The edit screen's Play: saves any edits, then opens the world.
    PlayFromEdit,
    RequestDelete,
    ConfirmDelete,
    Play,
    AcceptEula,
    OpenEulaLink,
    Prompt(PromptButton),
    /// Cancels the current screen; while opening it also closes the world.
    Back,
}

/// Control-channel work for the executor; each yields at most one [`Event`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    List,
    Create(protocol::world_control::NewWorld),
    Delete(String),
    Update {
        id: String,
        update: WorldUpdate,
    },
    Open(String),
    /// Waits briefly, then reads the open world's status.
    PollStatus,
    Close,
    /// Its outcome is never surfaced.
    SetPaused(bool),
    LoadPrefs,
    SetPrefs {
        dismiss_docker_prompt: bool,
        redetect: bool,
    },
    AcceptEula,
    /// Handled by the embedding resource, not the control worker.
    OpenUrl(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    Listed(Vec<World>),
    Created(World),
    Deleted(String),
    Updated(World),
    Status(WorldStatus),
    Prefs(Prefs, WorldStatus),
    EulaRequired,
    EulaAccepted,
    Failed(String),
    FailedPrefs(String),
}

pub(crate) const EULA_URL: &str = "https://www.minecraft.net/eula";

/// The create and edit screens' side-menu tabs this build implements.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Tab {
    #[default]
    General,
    Advanced,
}

/// What the create, edit and modal screens present, mirrored into the menu view each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct WorldsView {
    pub(crate) screen: Screen,
    pub(crate) tab: Tab,
    pub(crate) create: CreateForm,
    pub(crate) edit: Option<EditForm>,
    /// The saved world behind the edit form (its size and last-saved date).
    pub(crate) edited: Option<World>,
    pub(crate) form_error: Option<&'static str>,
    pub(crate) error: Option<String>,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) progress: Option<Progress>,
    pub(crate) busy: bool,
    /// Where a default world cannot run, so the create screen marks Flat as the only choice.
    pub(crate) bds_can_run: bool,
}

/// What a Docker modal or EULA gate resumes once cleared.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending {
    Create,
    SubmitCreate,
    Play,
}

/// State of the world-management screens; pure, so it runs without a window or core.
#[derive(Debug, Default)]
pub(crate) struct WorldsMenu {
    screen: Screen,
    worlds: Vec<World>,
    selected: Option<usize>,
    tab: Tab,
    create: CreateForm,
    edit: Option<EditForm>,
    play_after_update: bool,
    form_error: Option<&'static str>,
    opening: Option<String>,
    opening_status: Option<WorldStatus>,
    ready: Option<String>,
    error: Option<String>,
    busy: bool,
    /// A submitted disk mutation still owns the busy state after navigation.
    mutation_pending: bool,
    prefs: Prefs,
    setup: Option<Setup>,
    unavailable: Option<UnavailableReason>,
    prompt_acknowledged: bool,
    pending: Option<Pending>,
    eula_for: Option<String>,
}

impl WorldsMenu {
    pub(crate) fn screen(&self) -> Screen {
        self.screen
    }

    pub(crate) fn worlds(&self) -> &[World] {
        &self.worlds
    }

    pub(crate) fn selected(&self) -> Option<&World> {
        self.selected.and_then(|index| self.worlds.get(index))
    }

    pub(crate) fn create_form(&self) -> &CreateForm {
        &self.create
    }

    pub(crate) fn view(&self) -> WorldsView {
        WorldsView {
            screen: self.screen,
            tab: self.tab,
            create: self.create.clone(),
            edit: self.edit.clone(),
            edited: self
                .edit
                .as_ref()
                .and_then(|edit| self.worlds.iter().find(|w| w.id == edit.id))
                .cloned(),
            form_error: self.form_error,
            error: self.error.clone(),
            prompt: self.prompt(),
            progress: self.progress(),
            busy: self.busy,
            bds_can_run: self.bds_can_run(),
        }
    }

    pub(crate) fn edit_form(&self) -> Option<&EditForm> {
        self.edit.as_ref()
    }

    /// Validation message for the create or edit form, if the last submit failed.
    pub(crate) fn form_error(&self) -> Option<&str> {
        self.form_error
    }

    /// Message for [`Screen::Error`].
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// True while a request is in flight; the UI should disable its buttons.
    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    /// Takes the id of a world that finished opening; the caller then joins the game socket.
    pub(crate) fn take_ready(&mut self) -> Option<String> {
        self.ready.take()
    }

    /// The loading screen while a world opens.
    pub(crate) fn progress(&self) -> Option<Progress> {
        let id = self
            .opening
            .as_deref()
            .filter(|_| self.screen == Screen::Opening)?;
        let name = self
            .worlds
            .iter()
            .find(|w| w.id == id)
            .map_or("", |w| w.name.as_str());
        Some(progress::progress(self.opening_status.as_ref(), name))
    }

    /// False once the core reports the dedicated server cannot run here.
    pub(crate) fn bds_can_run(&self) -> bool {
        self.setup
            .as_ref()
            .is_none_or(|setup| setup.state != SetupState::Unsupported)
    }

    /// The Docker modal, while [`Screen::BackendPrompt`] is up.
    pub(crate) fn prompt(&self) -> Option<Prompt> {
        if self.screen != Screen::BackendPrompt {
            return None;
        }
        let blocking = match self.pending? {
            Pending::Create => PromptFor::BeginCreate,
            Pending::SubmitCreate => PromptFor::CreateDefault,
            Pending::Play => PromptFor::Play,
        };
        Some(Prompt {
            kind: self.prompt_kind(blocking)?,
            blocking,
        })
    }

    /// Why default worlds cannot run, when that should interrupt `blocking`.
    fn prompt_kind(&self, blocking: PromptFor) -> Option<PromptKind> {
        let kind = match self.unavailable? {
            UnavailableReason::DockerMissing => PromptKind::DockerMissing,
            UnavailableReason::DockerNotRunning => PromptKind::DockerNotRunning,
            UnavailableReason::Other => return None,
        };
        // Only the informational modal on the way into the create screen can be put away.
        let informational = blocking == PromptFor::BeginCreate
            && (self.prompt_acknowledged
                || (kind == PromptKind::DockerMissing && self.prefs.docker_prompt_dismissed));
        (!informational).then_some(kind)
    }

    fn note_status(&mut self, status: &WorldStatus) {
        self.unavailable = status.backend_unavailable_reason;
        if status.setup.is_some() {
            self.setup.clone_from(&status.setup);
        }
    }

    /// Runs `pending` now, or parks it behind the Docker modal.
    fn gate(&mut self, pending: Pending) -> Vec<Effect> {
        let blocking = match pending {
            Pending::Create => Some(PromptFor::BeginCreate),
            Pending::SubmitCreate => {
                (self.create.generator == Generator::Normal).then_some(PromptFor::CreateDefault)
            }
            Pending::Play => self
                .selected()
                .is_some_and(|w| w.backend == Backend::Bds)
                .then_some(PromptFor::Play),
        };
        if blocking.is_some_and(|blocking| self.prompt_kind(blocking).is_some()) {
            self.pending = Some(pending);
            self.screen = Screen::BackendPrompt;
            return Vec::new();
        }
        self.proceed(pending)
    }

    fn proceed(&mut self, pending: Pending) -> Vec<Effect> {
        self.pending = None;
        match pending {
            Pending::Create => {
                self.create = CreateForm::default();
                self.tab = Tab::General;
                if !self.bds_can_run() {
                    // Default terrain is BDS-only; Flat is all the built-in server hosts.
                    self.create.generator = Generator::Flat;
                }
                self.form_error = None;
                self.screen = Screen::Create;
                Vec::new()
            }
            Pending::SubmitCreate => match self.create.build() {
                Ok(new_world) => {
                    self.form_error = None;
                    self.busy = true;
                    self.mutation_pending = true;
                    self.screen = Screen::Create;
                    vec![Effect::Create(new_world)]
                }
                Err(error) => {
                    self.form_error = Some(error.message());
                    self.screen = Screen::Create;
                    Vec::new()
                }
            },
            Pending::Play => {
                let Some(id) = self.selected().map(|w| w.id.clone()) else {
                    self.screen = Screen::List;
                    return Vec::new();
                };
                self.begin_opening(id)
            }
        }
    }

    fn begin_opening(&mut self, id: String) -> Vec<Effect> {
        self.opening = Some(id.clone());
        self.opening_status = None;
        self.ready = None;
        self.screen = Screen::Opening;
        vec![Effect::Open(id)]
    }

    fn prompt_button(&mut self, button: PromptButton) -> Vec<Effect> {
        if self.screen != Screen::BackendPrompt {
            return Vec::new();
        }
        match button {
            PromptButton::CreateFlat => {
                self.prompt_acknowledged = true;
                match self.pending.take() {
                    Some(Pending::SubmitCreate) => {
                        self.create.generator = Generator::Flat;
                        self.proceed(Pending::SubmitCreate)
                    }
                    _ => self.proceed(Pending::Create),
                }
            }
            PromptButton::GetDocker => vec![Effect::OpenUrl(DOCKER_URL)],
            PromptButton::DontShowAgain => {
                self.prefs.docker_prompt_dismissed = true;
                let mut effects = vec![Effect::SetPrefs {
                    dismiss_docker_prompt: true,
                    redetect: false,
                }];
                effects.extend(self.proceed(Pending::Create));
                effects
            }
            PromptButton::Retry => {
                self.busy = true;
                vec![Effect::SetPrefs {
                    dismiss_docker_prompt: false,
                    redetect: true,
                }]
            }
            PromptButton::Cancel => self.back(),
        }
    }

    fn select_clamped(&mut self, index: Option<usize>) {
        self.selected = index
            .filter(|_| !self.worlds.is_empty())
            .map(|i| i.min(self.worlds.len() - 1));
    }

    fn fail(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.screen = Screen::Error;
        self.busy = false;
    }

    pub(crate) fn update(&mut self, input: Input) -> Vec<Effect> {
        if self.busy && !matches!(input, Input::Back) {
            return Vec::new();
        }
        match input {
            Input::Refresh => {
                self.busy = true;
                vec![Effect::List]
            }
            Input::Select(index) => {
                if self.screen == Screen::List && index < self.worlds.len() {
                    self.selected = Some(index);
                }
                Vec::new()
            }
            Input::BeginCreate if self.screen == Screen::List => self.gate(Pending::Create),
            Input::OpenTemplates if self.screen == Screen::List => {
                self.screen = Screen::Templates;
                Vec::new()
            }
            Input::SetName(name) => {
                self.create.name = name;
                Vec::new()
            }
            Input::SetSeed(seed) => {
                self.create.seed_text = seed;
                Vec::new()
            }
            Input::SelectTab(tab) => {
                self.tab = tab;
                Vec::new()
            }
            Input::SetGameMode(mode) => {
                match (&mut self.edit, self.screen) {
                    (Some(edit), Screen::Edit) => edit.game_mode = mode,
                    // New worlds offer Survival and Creative; Adventure is for edits and templates.
                    _ if mode != GameMode::Adventure => self.create.game_mode = mode,
                    _ => {}
                }
                Vec::new()
            }
            Input::SetDifficulty(difficulty) => {
                match (&mut self.edit, self.screen) {
                    (Some(edit), Screen::Edit) => edit.difficulty = difficulty,
                    _ => self.create.difficulty = difficulty,
                }
                Vec::new()
            }
            Input::SetFlat(flat) => {
                self.create.generator = if flat {
                    Generator::Flat
                } else {
                    Generator::Normal
                };
                Vec::new()
            }
            Input::SubmitCreate if self.screen == Screen::Create => {
                self.gate(Pending::SubmitCreate)
            }
            Input::BeginEdit(index) if self.screen == Screen::List => {
                if let Some(world) = self.worlds.get(index) {
                    self.edit = Some(EditForm::of(world));
                    self.selected = Some(index);
                    self.tab = Tab::General;
                    self.form_error = None;
                    self.screen = Screen::Edit;
                }
                Vec::new()
            }
            Input::SetEditName(name) => {
                if let Some(edit) = &mut self.edit {
                    edit.name = name;
                }
                Vec::new()
            }
            Input::SubmitEdit if matches!(self.screen, Screen::Edit | Screen::ConfirmLeaveEdit) => {
                self.play_after_update = false;
                self.submit_edit()
            }
            Input::DiscardEdit if self.screen == Screen::ConfirmLeaveEdit => {
                self.edit = None;
                self.screen = Screen::List;
                Vec::new()
            }
            Input::PlayFromEdit if self.screen == Screen::Edit => {
                self.play_after_update = true;
                self.submit_edit()
            }
            Input::RequestDelete if self.screen == Screen::Edit => {
                self.screen = Screen::ConfirmDelete;
                Vec::new()
            }
            Input::ConfirmDelete if self.screen == Screen::ConfirmDelete => {
                let Some(id) = self.edit.as_ref().map(|edit| edit.id.clone()) else {
                    return Vec::new();
                };
                self.busy = true;
                self.mutation_pending = true;
                vec![Effect::Delete(id)]
            }
            Input::Play if self.screen == Screen::List && self.selected().is_some() => {
                self.gate(Pending::Play)
            }
            Input::AcceptEula if self.screen == Screen::Eula => {
                self.busy = true;
                vec![Effect::AcceptEula]
            }
            Input::OpenEulaLink => vec![Effect::OpenUrl(EULA_URL)],
            Input::Prompt(button) => self.prompt_button(button),
            Input::Back => self.back(),
            _ => Vec::new(),
        }
    }

    fn submit_edit(&mut self) -> Vec<Effect> {
        let Some(edit) = &self.edit else {
            return Vec::new();
        };
        let Some(saved) = self.worlds.iter().find(|w| w.id == edit.id) else {
            return Vec::new();
        };
        let name = match validate_name(&edit.name) {
            Ok(name) => name,
            Err(error) => {
                self.form_error = Some(error.message());
                return Vec::new();
            }
        };
        let update = edit_changes(edit, saved, name);
        self.form_error = None;
        if update == WorldUpdate::default() {
            self.edit = None;
            self.screen = Screen::List;
            return self.play_edited();
        }
        self.busy = true;
        self.mutation_pending = true;
        vec![Effect::Update {
            id: edit.id.clone(),
            update,
        }]
    }

    /// Opens the world whose settings were just saved, when the edit screen's Play asked for it.
    fn play_edited(&mut self) -> Vec<Effect> {
        if std::mem::take(&mut self.play_after_update) {
            self.gate(Pending::Play)
        } else {
            Vec::new()
        }
    }

    /// True when the edit form differs from the saved world.
    fn edit_dirty(&self) -> bool {
        let Some(edit) = &self.edit else {
            return false;
        };
        self.worlds
            .iter()
            .find(|w| w.id == edit.id)
            .is_some_and(|saved| {
                edit_changes(edit, saved, edit.name.trim().to_owned()) != WorldUpdate::default()
            })
    }

    fn back(&mut self) -> Vec<Effect> {
        match self.screen {
            Screen::Edit if self.edit_dirty() => {
                self.screen = Screen::ConfirmLeaveEdit;
                Vec::new()
            }
            Screen::ConfirmLeaveEdit => {
                self.screen = Screen::Edit;
                Vec::new()
            }
            Screen::Opening => {
                self.opening = None;
                self.opening_status = None;
                self.busy = false;
                self.screen = Screen::List;
                vec![Effect::Close]
            }
            Screen::ConfirmDelete => {
                self.busy = false;
                self.screen = Screen::Edit;
                Vec::new()
            }
            Screen::BackendPrompt if self.pending == Some(Pending::SubmitCreate) => {
                self.pending = None;
                self.busy = false;
                self.screen = Screen::Create;
                Vec::new()
            }
            Screen::Create
            | Screen::Templates
            | Screen::Edit
            | Screen::Error
            | Screen::Eula
            | Screen::BackendPrompt => {
                self.form_error = None;
                self.error = None;
                self.pending = None;
                self.eula_for = None;
                self.edit = None;
                self.play_after_update = false;
                self.busy = self.mutation_pending;
                self.screen = Screen::List;
                Vec::new()
            }
            Screen::List => Vec::new(),
        }
    }

    pub(crate) fn apply(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Listed(worlds) => {
                let keep = self.selected().map(|w| w.id.clone());
                self.worlds = worlds;
                let index = keep
                    .and_then(|id| self.worlds.iter().position(|w| w.id == id))
                    .or(Some(0));
                self.select_clamped(index);
                self.busy = self.mutation_pending;
                Vec::new()
            }
            Event::Created(world) => {
                let open = self.mutation_pending && self.screen == Screen::Create;
                self.mutation_pending = false;
                self.worlds.insert(0, world);
                self.selected = Some(0);
                self.busy = false;
                if open {
                    self.gate(Pending::Play)
                } else {
                    Vec::new()
                }
            }
            Event::Deleted(id) => {
                self.mutation_pending = false;
                self.worlds.retain(|w| w.id != id);
                let index = self.selected;
                self.select_clamped(index);
                self.edit = None;
                self.busy = false;
                self.screen = Screen::List;
                Vec::new()
            }
            Event::Updated(world) => {
                self.mutation_pending = false;
                if let Some(slot) = self.worlds.iter_mut().find(|w| w.id == world.id) {
                    let size = slot.size_bytes;
                    *slot = World {
                        size_bytes: size,
                        ..world
                    };
                }
                self.edit = None;
                self.busy = false;
                self.screen = Screen::List;
                self.play_edited()
            }
            Event::Status(status) => {
                self.note_status(&status);
                self.apply_status(status)
            }
            Event::Prefs(prefs, status) => {
                self.prefs = prefs;
                self.note_status(&status);
                self.busy = self.mutation_pending;
                if self.screen == Screen::BackendPrompt && self.prompt().is_none() {
                    return match self.pending.take() {
                        Some(pending) => self.proceed(pending),
                        None => {
                            self.screen = Screen::List;
                            Vec::new()
                        }
                    };
                }
                Vec::new()
            }
            Event::EulaRequired => {
                self.eula_for = self.opening.take();
                self.busy = false;
                self.screen = Screen::Eula;
                Vec::new()
            }
            Event::EulaAccepted => {
                self.busy = false;
                match self.eula_for.take() {
                    Some(id) => self.begin_opening(id),
                    None => {
                        self.screen = Screen::List;
                        Vec::new()
                    }
                }
            }
            Event::FailedPrefs(message) if self.mutation_pending => {
                self.error = Some(message);
                Vec::new()
            }
            Event::Failed(message) | Event::FailedPrefs(message) => {
                self.mutation_pending = false;
                self.opening = None;
                self.fail(message);
                Vec::new()
            }
        }
    }

    fn apply_status(&mut self, status: WorldStatus) -> Vec<Effect> {
        let Some(opening) = self.opening.clone() else {
            return Vec::new();
        };
        if status.world_id.as_deref().is_some_and(|id| id != opening) {
            return Vec::new();
        }
        let state = status.state;
        let error = status.error.clone();
        self.opening_status = Some(status);
        match state {
            WorldState::Running => {
                self.opening = None;
                self.ready = Some(opening);
                self.screen = Screen::List;
                Vec::new()
            }
            WorldState::Starting | WorldState::Stopping => vec![Effect::PollStatus],
            WorldState::Idle | WorldState::Failed => {
                self.opening = None;
                self.opening_status = None;
                // Docker stopped since it was detected: offer Retry instead of a dead end.
                if self.unavailable == Some(UnavailableReason::DockerNotRunning) {
                    self.pending = Some(Pending::Play);
                    self.screen = Screen::BackendPrompt;
                } else {
                    self.fail(error.unwrap_or_else(|| FALLBACK_ERROR.to_owned()));
                }
                vec![Effect::Close]
            }
        }
    }
}

fn edit_changes(edit: &EditForm, saved: &World, name: String) -> WorldUpdate {
    WorldUpdate {
        name: (name != saved.name).then_some(name),
        game_mode: (edit.game_mode != saved.game_mode).then_some(edit.game_mode),
        difficulty: (edit.difficulty != saved.difficulty).then_some(edit.difficulty),
    }
}

#[cfg(test)]
mod tests;
