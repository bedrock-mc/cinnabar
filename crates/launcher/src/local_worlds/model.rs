use super::world::{
    Difficulty, Generator, NewWorld, Prefs, Setup, SetupState, UnavailableReason, World,
    WorldStatus, WorldUpdate,
};
use bridge::{Backend, GameMode, WorldState};
use std::path::PathBuf;

use super::form::{CreateForm, EditForm, validate_name};
use super::progress::{self, Progress};
use super::prompt::{DOCKER_URL, Prompt, PromptButton, PromptFor, PromptKind};

const FALLBACK_ERROR: &str = "The local world could not be started";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Screen {
    #[default]
    List,
    Create,
    /// Create from template: there are no templates, so this is vanilla's empty state.
    Templates,
    /// World settings of the selected world.
    Edit,
    /// Delete confirmation over the edit screen.
    ConfirmDelete,
    /// Waiting for the chosen world's server to come up.
    Opening,
    /// The user must accept the Minecraft EULA before the server is downloaded.
    Eula,
    /// BDS worlds need Docker on macOS; see [`WorldsMenu::prompt`].
    BackendPrompt,
    /// Copying picked or dropped world files into the saved worlds.
    Importing,
    Error,
}

/// User intents; the embedding menu maps its buttons and text fields onto these.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
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
    SetOption(super::options::WorldOption, i32),
    /// Raw text of a numeric option; a valid value also applies at once.
    SetOptionText(super::options::WorldOption, String),
    SetFlat(bool),
    SetBackend(Backend),
    SubmitCreate,
    /// Opens the settings of the world at this list index.
    BeginEdit(usize),
    SetEditName(String),
    SubmitEdit,
    /// The edit screen's Play: saves any edits, then opens the world.
    PlayFromEdit,
    RequestDelete,
    ConfirmDelete,
    Play,
    AcceptEula,
    OpenEulaLink,
    Prompt(PromptButton),
    /// Picks world files to import.
    Import,
    /// Imports dropped world files.
    ImportFiles(Vec<PathBuf>),
    /// Cancels the current screen; while opening it also closes the world.
    Back,
}

/// World-service work for the executor; each yields at most one [`Event`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    List,
    Create(NewWorld),
    Delete(String),
    Update {
        id: String,
        update: WorldUpdate,
    },
    Open(String),
    /// Reads the open world's status once it changes, or after a short wait.
    PollStatus,
    Close,
    /// Its outcome is never surfaced.
    SetPaused(bool),
    /// Readies a server ahead of a likely open; its outcome is never surfaced.
    Prepare,
    LoadPrefs,
    /// Waits briefly, then reloads preferences; repeats while Docker is still being detected.
    PollPrefs,
    SetPrefs {
        dismiss_docker_prompt: bool,
        redetect: bool,
    },
    AcceptEula,
    /// Imports world files; `None` asks the file picker for one first.
    Import(Option<Vec<PathBuf>>),
    /// Handled by the embedding resource, not the world worker.
    OpenUrl(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Listed(Vec<World>),
    Created(World),
    Deleted(String),
    Updated(World),
    Status(WorldStatus),
    Prefs(Prefs, WorldStatus),
    EulaRequired,
    EulaAccepted,
    /// The imported worlds and why any file was refused; both empty when the picker was cancelled.
    Imported {
        worlds: Vec<World>,
        errors: Vec<String>,
    },
    Failed(String),
    FailedPrefs(String),
}

pub const EULA_URL: &str = "https://www.minecraft.net/eula";

macro_rules! world_tabs {
    ($first:ident: $first_label:literal, $first_supported:literal; $($tab:ident: $label:literal, $supported:literal;)*) => {
        /// Categories shown in the create and edit sidebar.
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub enum Tab { #[default] $first, $($tab,)* }
        impl Tab {
            pub const ALL: &'static [Self] = &[Self::$first, $(Self::$tab,)*];
            /// The sidebar label for this tab.
            pub const fn label(self) -> &'static str {
                match self { Self::$first => $first_label, $(Self::$tab => $label,)* }
            }
            /// Whether this tab has configurable settings.
            pub const fn supported(self) -> bool {
                match self { Self::$first => $first_supported, $(Self::$tab => $supported,)* }
            }
        }
    };
}
world_tabs! {
    General: "General", true;
    Advanced: "Advanced", true;
    Multiplayer: "Multiplayer", true;
    Cheats: "Cheats", true;
    ResourcePacks: "Resource packs", false;
    BehaviourPacks: "Behaviour packs", false;
    Experiments: "Experiments", false;
}

/// What the create, edit and modal screens present, mirrored into the menu view each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldsView {
    pub screen: Screen,
    pub tab: Tab,
    pub create: CreateForm,
    pub edit: Option<EditForm>,
    /// The saved world behind the edit form (its size and last-saved date).
    pub edited: Option<World>,
    pub form_error: Option<&'static str>,
    pub option_text: std::collections::BTreeMap<super::options::WorldOption, String>,
    pub error: Option<String>,
    pub prompt: Option<Prompt>,
    pub progress: Option<Progress>,
    pub busy: bool,
    /// Whether the selected BDS backend can run on this machine.
    pub bds_can_run: bool,
    pub bds_unavailable: Option<UnavailableReason>,
}

impl WorldsView {
    /// A numeric option's text as typed, or its current value before any typing.
    pub fn numeric_text(&self, id: super::options::WorldOption) -> String {
        if let Some(text) = self.option_text.get(&id) {
            return text.clone();
        }
        self.edit
            .as_ref()
            .filter(|_| self.screen == Screen::Edit)
            .map_or(&self.create.options, |edit| &edit.options)
            .get(id)
            .to_string()
    }

    /// Whether the active form uses the BDS backend that supports world options, with level
    /// data small enough to read and write them.
    pub fn options_available(&self) -> bool {
        if self.edit.is_some() {
            self.edited
                .as_ref()
                .is_some_and(|world| world.backend == Backend::Bds && !world.options_unavailable)
        } else {
            self.create.backend == Backend::Bds
        }
    }
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
pub struct WorldsMenu {
    screen: Screen,
    worlds: Vec<World>,
    selected: Option<usize>,
    tab: Tab,
    create: CreateForm,
    edit: Option<EditForm>,
    play_after_update: bool,
    form_error: Option<&'static str>,
    /// Numeric fields' text as typed, kept until a new form opens.
    option_text: std::collections::BTreeMap<super::options::WorldOption, String>,
    opening: Option<String>,
    opening_status: Option<WorldStatus>,
    ready: Option<String>,
    error: Option<String>,
    busy: bool,
    /// A submitted disk mutation still owns the busy state after navigation.
    mutation_pending: bool,
    setup: Option<Setup>,
    unavailable: Option<UnavailableReason>,
    pending: Option<Pending>,
    eula_for: Option<String>,
}

impl WorldsMenu {
    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn worlds(&self) -> &[World] {
        &self.worlds
    }

    pub fn selected(&self) -> Option<&World> {
        self.selected.and_then(|index| self.worlds.get(index))
    }

    pub fn create_form(&self) -> &CreateForm {
        &self.create
    }

    pub fn view(&self) -> WorldsView {
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
            option_text: self.option_text.clone(),
            error: self.error.clone(),
            prompt: self.prompt(),
            progress: self.progress(),
            busy: self.busy,
            bds_can_run: self.bds_can_run(),
            bds_unavailable: self.unavailable,
        }
    }

    pub fn edit_form(&self) -> Option<&EditForm> {
        self.edit.as_ref()
    }

    /// Validation message for the create or edit form, if the last submit failed.
    pub fn form_error(&self) -> Option<&str> {
        self.form_error
    }

    /// Message for [`Screen::Error`].
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// True while a request is in flight; the UI should disable its buttons.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Takes the id of a world that finished opening; the caller then joins the session endpoint.
    pub fn take_ready(&mut self) -> Option<String> {
        self.ready.take()
    }

    /// The open world's player limit, the host included, once its server reported one.
    pub fn max_players(&self) -> Option<u32> {
        self.opening_status
            .as_ref()?
            .max_players
            .filter(|max| *max > 0)
    }

    /// The loading screen while a world opens.
    pub fn progress(&self) -> Option<Progress> {
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

    /// False once the dedicated server is reported unable to run here.
    pub fn bds_can_run(&self) -> bool {
        self.setup
            .as_ref()
            .is_none_or(|setup| setup.state != SetupState::Unsupported)
    }

    /// The Docker modal, while [`Screen::BackendPrompt`] is up.
    pub fn prompt(&self) -> Option<Prompt> {
        if self.screen != Screen::BackendPrompt {
            return None;
        }
        let blocking = match self.pending? {
            Pending::Create => return None,
            Pending::SubmitCreate => PromptFor::CreateBds,
            Pending::Play => PromptFor::Play,
        };
        Some(Prompt {
            kind: self.prompt_kind()?,
            blocking,
        })
    }

    /// Why the BDS backend cannot run on this machine.
    fn prompt_kind(&self) -> Option<PromptKind> {
        match self.unavailable? {
            UnavailableReason::DockerMissing => Some(PromptKind::DockerMissing),
            UnavailableReason::DockerNotRunning => Some(PromptKind::DockerNotRunning),
        }
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
            Pending::Create => None,
            Pending::SubmitCreate => {
                (self.create.backend == Backend::Bds).then_some(PromptFor::CreateBds)
            }
            Pending::Play => self
                .selected()
                .is_some_and(|w| w.backend == Backend::Bds)
                .then_some(PromptFor::Play),
        };
        if blocking.is_some() && self.prompt_kind().is_some() {
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
                self.option_text.clear();
                self.tab = Tab::General;
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
            PromptButton::UseDragonfly => match self.pending.take() {
                Some(Pending::SubmitCreate) => {
                    self.create.backend = Backend::Dragonfly;
                    self.proceed(Pending::SubmitCreate)
                }
                _ => self.proceed(Pending::Create),
            },
            PromptButton::GetDocker => vec![Effect::OpenUrl(DOCKER_URL)],
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

    pub fn update(&mut self, input: Input) -> Vec<Effect> {
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
            Input::BeginCreate if matches!(self.screen, Screen::List | Screen::Templates) => {
                self.gate(Pending::Create)
            }
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
                if tab == Tab::General || self.view().options_available() {
                    self.tab = tab;
                }
                Vec::new()
            }
            Input::SetGameMode(mode) => {
                match (&mut self.edit, self.screen) {
                    (Some(edit), Screen::Edit) => {
                        edit.game_mode = mode;
                        if mode == GameMode::Creative {
                            edit.options
                                .set(super::options::WorldOption::ActivateCheats, 1);
                        }
                    }
                    // New worlds offer Survival and Creative; Adventure is for edits and templates.
                    _ if mode != GameMode::Adventure => {
                        self.create.game_mode = mode;
                        if mode == GameMode::Creative {
                            self.create
                                .options
                                .set(super::options::WorldOption::ActivateCheats, 1);
                        }
                    }
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
            Input::SetOption(id, value)
                if matches!(self.screen, Screen::Create | Screen::Edit)
                    && self.view().options_available() =>
            {
                self.form_error = None;
                let options = match (&mut self.edit, self.screen) {
                    (Some(edit), Screen::Edit) if edit.options.editable(id, true) => {
                        &mut edit.options
                    }
                    (_, Screen::Create) if self.create.options.editable(id, false) => {
                        &mut self.create.options
                    }
                    _ => return Vec::new(),
                };
                let before = options.clone();
                options.set(id, value);
                // Options a toggle gates or resets show their value instead of earlier typing.
                self.option_text.retain(|&other, _| {
                    other == id || (!id.gates(other) && options.get(other) == before.get(other))
                });
                Vec::new()
            }
            Input::SetOptionText(id, text)
                if matches!(self.screen, Screen::Create | Screen::Edit)
                    && self.view().options_available() =>
            {
                let value = text.parse::<i32>().ok().filter(|value| id.accepts(*value));
                self.option_text.insert(id, text);
                match value {
                    Some(value) => self.update(Input::SetOption(id, value)),
                    None => Vec::new(),
                }
            }
            Input::SetFlat(flat) => {
                self.create.generator = if flat {
                    Generator::Flat
                } else {
                    Generator::Normal
                };
                Vec::new()
            }
            Input::SetBackend(backend) if self.screen == Screen::Create => {
                self.create.backend = backend;
                Vec::new()
            }
            Input::SubmitCreate if self.screen == Screen::Create => {
                if self.refuse_option_text() {
                    return Vec::new();
                }
                self.gate(Pending::SubmitCreate)
            }
            Input::BeginEdit(index) if self.screen == Screen::List => {
                if let Some(world) = self.worlds.get(index) {
                    self.edit = Some(EditForm::of(world));
                    self.option_text.clear();
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
            Input::SubmitEdit if self.screen == Screen::Edit => {
                self.play_after_update = false;
                self.submit_edit()
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
            Input::Import if self.screen == Screen::List => self.begin_import(None),
            Input::ImportFiles(files) if self.screen == Screen::List && !files.is_empty() => {
                self.begin_import(Some(files))
            }
            Input::Prompt(button) => self.prompt_button(button),
            Input::Back => self.back(),
            _ => Vec::new(),
        }
    }

    /// Holds the list behind the import modal until the worker reports every file.
    fn begin_import(&mut self, files: Option<Vec<PathBuf>>) -> Vec<Effect> {
        self.busy = true;
        self.mutation_pending = true;
        self.screen = Screen::Importing;
        vec![Effect::Import(files)]
    }

    /// Validates and saves edits, leaving the form open until the write succeeds.
    fn submit_edit(&mut self) -> Vec<Effect> {
        if self.refuse_option_text() {
            return Vec::new();
        }
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

    /// Flags typed numbers that the active form's enabled options do not accept.
    fn refuse_option_text(&mut self) -> bool {
        let (options, editing) = match (&self.edit, self.screen) {
            (Some(edit), Screen::Edit) => (&edit.options, true),
            _ => (&self.create.options, false),
        };
        let invalid = self.option_text.iter().any(|(&id, text)| {
            options.editable(id, editing)
                && !text.parse::<i32>().is_ok_and(|value| id.accepts(value))
        });
        if invalid {
            self.form_error = Some("Enter a valid number within the displayed range");
        }
        invalid
    }

    /// Opens the world whose settings were just saved, when the edit screen's Play asked for it.
    fn play_edited(&mut self) -> Vec<Effect> {
        if std::mem::take(&mut self.play_after_update) {
            self.gate(Pending::Play)
        } else {
            Vec::new()
        }
    }

    fn back(&mut self) -> Vec<Effect> {
        match self.screen {
            Screen::Edit => {
                self.play_after_update = false;
                if self.busy {
                    Vec::new()
                } else {
                    self.submit_edit()
                }
            }
            Screen::Error if self.edit.is_some() => {
                self.error = None;
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
                self.busy = self.mutation_pending;
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
            // The copy cannot be interrupted; its result returns to the list.
            Screen::List | Screen::Importing => Vec::new(),
        }
    }

    pub fn apply(&mut self, event: Event) -> Vec<Effect> {
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
            Event::Prefs(_, status) => {
                self.note_status(&status);
                self.busy = self.mutation_pending;
                // The Docker probe outlives startup; keep reading until it reports a verdict.
                let detecting = status
                    .setup
                    .as_ref()
                    .is_some_and(|setup| setup.state == SetupState::CheckingRuntime);
                let mut effects = Vec::new();
                if self.screen == Screen::BackendPrompt && self.prompt().is_none() {
                    effects = match self.pending.take() {
                        Some(pending) => self.proceed(pending),
                        None => {
                            self.screen = Screen::List;
                            Vec::new()
                        }
                    };
                }
                if detecting {
                    effects.push(Effect::PollPrefs);
                }
                effects
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
            Event::Imported { worlds, errors } => {
                self.mutation_pending = false;
                self.busy = false;
                self.screen = Screen::List;
                if !worlds.is_empty() {
                    self.selected = Some(0);
                }
                for world in worlds {
                    self.worlds.insert(0, world);
                }
                if !errors.is_empty() {
                    self.fail(errors.join("\n"));
                }
                Vec::new()
            }
            Event::FailedPrefs(message) if self.mutation_pending => {
                self.error = Some(message);
                Vec::new()
            }
            Event::Failed(message) | Event::FailedPrefs(message) => {
                self.mutation_pending = false;
                self.play_after_update = false;
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

/// Includes only fields that differ from the saved world, using the validated name.
fn edit_changes(edit: &EditForm, saved: &World, name: String) -> WorldUpdate {
    WorldUpdate {
        name: (name != saved.name).then_some(name),
        game_mode: (edit.game_mode != saved.game_mode).then_some(edit.game_mode),
        difficulty: (edit.difficulty != saved.difficulty).then_some(edit.difficulty),
        options: {
            let changes = edit.options.changes_from(&edit.original_options);
            (!changes.is_empty()).then_some(changes)
        },
    }
}

#[cfg(test)]
mod tests;
