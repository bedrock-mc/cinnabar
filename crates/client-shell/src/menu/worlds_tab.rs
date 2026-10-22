//! The play screen's worlds tab over the local-worlds module: its list feeds
//! the tab's cards, the create/edit/template screens and their modals relay
//! presses to the module, an opened world is joined through the launcher core
//! and closed when its session ends, and the pause menu pauses it.

use bridge::{Backend, GameMode};
use launcher::local_worlds::world::Difficulty;

use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuField, MenuScreen},
};
use {
    crate::local_worlds::LocalWorlds,
    launcher::local_worlds::{
        model::{Input, Screen, Tab, WorldsView},
        progress::Progress,
    },
};

use launcher::menu::worlds_tab::world_card;
use launcher::menu::{auth::AuthState, view::MenuProfile, worlds_tab::LocalWorldAction};
use launcher_host::local_worlds::Host;

/// The menu's side of the local-world screens: the mirrored view, queued presses and the
/// name and seed fields' editors.
#[derive(Debug)]
pub(super) struct LocalWorldsUi {
    view: WorldsView,
    actions: Vec<LocalWorldAction>,
    pub(super) name: ui::ChatEditor,
    pub(super) seed: ui::ChatEditor,
    pub(super) options:
        std::collections::BTreeMap<launcher::local_worlds::options::WorldOption, ui::ChatEditor>,
    /// The world being joined, for the loading screen's connect stage.
    joining: Option<String>,
    world_id: Option<String>,
    /// The joined world runs on the dedicated server, which the core hosts for Xbox friends.
    hosted: bool,
    /// The open world's player limit, as its server reported it.
    max_players: Option<u32>,
    /// The worlds screen was showing last frame, so a server was already readied for it.
    prepared: bool,
    /// Numeric options focused since the last sync, whose text still goes to the module after
    /// focus leaves.
    editing: std::collections::BTreeSet<launcher::local_worlds::options::WorldOption>,
    /// The world being played, as Settings' Game section shows it.
    in_world: Option<std::sync::Arc<launcher::local_worlds::in_world::View>>,
    /// Game section presses waiting for the next sync.
    in_world_actions: Vec<launcher::local_worlds::in_world::Action>,
    /// The Game section's numeric options typed into since the last sync, as [`Self::editing`].
    in_world_editing: std::collections::BTreeSet<launcher::local_worlds::options::WorldOption>,
}

impl LocalWorldsUi {
    /// Starts editing a numeric option from the text the module keeps for it, or from the
    /// played world's value when Settings' Game section has the box.
    pub(super) fn begin_option_edit(
        &mut self,
        id: launcher::local_worlds::options::WorldOption,
        in_world: bool,
    ) {
        let text = match &self.in_world {
            Some(world) if in_world => {
                if !self.in_world_editing.insert(id) {
                    return;
                }
                world.numeric_text(id)
            }
            _ => {
                self.editing.insert(id);
                self.view.numeric_text(id)
            }
        };
        self.options
            .get_mut(&id)
            .expect("numeric world option")
            .set_text(&text);
    }

    /// Local preparation progress covers standalone prompts and owns their input.
    pub(super) fn progress_open(&self) -> bool {
        self.view.progress.is_some()
    }
}

impl Default for LocalWorldsUi {
    fn default() -> Self {
        Self {
            view: WorldsView::default(),
            actions: Vec::new(),
            name: super::input::fields::field_editor(MenuField::WorldName),
            seed: super::input::fields::field_editor(MenuField::WorldSeed),
            options: launcher::local_worlds::options::WorldOption::ALL
                .iter()
                .filter(|id| {
                    matches!(
                        id.definition().control,
                        launcher::local_worlds::options::Control::Number { .. }
                    )
                })
                .map(|&id| {
                    (
                        id,
                        super::input::fields::field_editor(MenuField::WorldOption(id)),
                    )
                })
                .collect(),
            joining: None,
            world_id: None,
            hosted: false,
            max_players: None,
            prepared: false,
            editing: Default::default(),
            in_world: None,
            in_world_actions: Vec::new(),
            in_world_editing: Default::default(),
        }
    }
}

impl MenuRuntime {
    /// The local world the player is in, by id; `None` outside one.
    #[must_use]
    pub fn playing_local_world(&self) -> Option<&str> {
        self.local_world_active
            .then_some(self.local_ui.world_id.as_deref())
            .flatten()
    }

    /// Whether the Worlds tab is on screen, where dropped world files import.
    #[must_use]
    pub fn showing_worlds(&self) -> bool {
        self.visible && self.screen == MenuScreen::Play
    }

    /// The core's stable local-world identifier, independent of a rename or invite.
    pub fn local_world_identity(&self) -> Option<&str> {
        self.local_ui.world_id.as_deref()
    }

    /// Includes deliberate joins whose local-world preparation completes on a later frame.
    pub(super) fn gameplay_return_pending(&self) -> bool {
        use launcher::local_worlds::prompt::{PromptButton, PromptFor};
        self.intents.join.is_some()
            || self.local_world_requested.is_some()
            || self.local_ui.actions.iter().any(|action| match *action {
                LocalWorldAction::Create
                | LocalWorldAction::PlayFromEdit
                | LocalWorldAction::AcceptEula => true,
                LocalWorldAction::Prompt(button) => {
                    self.local_ui.view.prompt.is_some_and(|prompt| {
                        prompt.buttons().contains(&button)
                            && matches!(
                                (button, prompt.blocking),
                                (PromptButton::UseDragonfly, PromptFor::CreateBds)
                                    | (PromptButton::Retry, PromptFor::Play | PromptFor::CreateBds)
                            )
                    })
                }
                _ => false,
            })
    }

    /// Mirror the module's worlds and screens, forward presses and typed text, join a world
    /// that finished opening, and track whether a local-world session is live.
    pub fn sync_local_worlds(&mut self, worlds: &mut LocalWorlds, in_session: bool) {
        worlds.set_host(self.local_world_host());
        self.sync_storage_worlds(worlds);
        self.push_local_text(worlds);
        let on_worlds = self.showing_worlds();
        if on_worlds && !self.local_ui.prepared {
            worlds.prepare();
        }
        self.local_ui.prepared = on_worlds;
        for action in std::mem::take(&mut self.local_ui.actions) {
            if let Some(input) = action.input() {
                worlds.input(input);
            }
        }
        let before = self.local_ui.view.screen;
        let mut view = worlds.menu().view();
        if view.screen != before {
            self.load_local_text(&view);
        }
        let cards = worlds
            .menu()
            .worlds()
            .iter()
            .map(|world| launcher::menu::LocalWorldCard {
                thumbnail: worlds.thumbnail(&world.id),
                ..world_card(world)
            })
            .collect();
        self.set_local_worlds(cards);
        if let Some(index) = self.take_local_world_request() {
            worlds.input(Input::Select(index));
            worlds.input(Input::Play);
            view = worlds.menu().view();
        }
        if let Some(id) = worlds.take_ready() {
            let world = worlds.menu().worlds().iter().find(|world| world.id == id);
            let hosted = world.is_some_and(|world| {
                world.backend == Backend::Bds
                    && world
                        .options
                        .enabled(launcher::local_worlds::options::WorldOption::Multiplayer)
            });
            self.local_ui.world_id = Some(id.clone());
            let name = world.map_or(id, |world| world.name.clone());
            self.request_local_world_join(name, hosted);
            self.local_ui.max_players = worlds.menu().max_players();
        }
        if self.local_world_joined && self.is_connecting() {
            let name = self.local_ui.joining.as_deref().unwrap_or_default();
            view.progress = Some(Progress::connecting(name));
        }
        self.finish_storage_world(view.screen);
        self.local_ui.view = view;
        let active = self.local_world_joined && (in_session || self.is_connecting());
        self.sync_world_settings(worlds, active && in_session);
        if active != self.local_world_active {
            self.local_world_active = active;
            if active {
                worlds.set_playing(true);
            } else {
                // The core saves and stops the world once its session is over.
                worlds.leave_world();
                self.local_world_joined = false;
                self.local_ui.joining = None;
                self.local_ui.hosted = false;
                self.local_ui.max_players = None;
            }
        }
        // The invite screen opens over the pause screen, which stays up beneath it.
        let paused = matches!(self.screen, MenuScreen::Pause | MenuScreen::Invite);
        worlds.set_pause_menu(active && self.visible && paused);
    }

    /// Sends Settings' Game section presses and typed numbers to the played world, then mirrors it.
    fn sync_world_settings(&mut self, worlds: &mut LocalWorlds, playing: bool) {
        use launcher::local_worlds::in_world::{Action, View};
        let id = self.local_ui.world_id.clone().filter(|_| playing);
        let field = self.field;
        let ui = &mut self.local_ui;
        for &option in &ui.in_world_editing {
            let current = ui
                .in_world
                .as_ref()
                .map(|world| world.world.options.get(option));
            if let Ok(value) = ui.options[&option].as_str().parse::<i32>()
                && current.is_some_and(|current| current != value)
            {
                ui.in_world_actions.push(Action::Option(option, value));
            }
        }
        ui.in_world_editing
            .retain(|&option| field == Some(MenuField::WorldOption(option)));
        for action in std::mem::take(&mut ui.in_world_actions) {
            if let Some(id) = &id {
                worlds.input(Input::InWorld(id.clone(), action));
            }
        }
        ui.in_world = id
            .and_then(|id| worlds.menu().worlds().iter().find(|world| world.id == id))
            .and_then(|world| {
                let mut view = View::of(world)?;
                view.missed = worlds.menu().missed_live(&world.id);
                Some(std::sync::Arc::new(view))
            });
        if ui.in_world.is_none() {
            ui.in_world_editing.clear();
        }
    }

    /// Queues a press in Settings' Game section; a numeric box takes focus instead.
    pub(super) fn activate_world_settings(
        &mut self,
        action: launcher::local_worlds::in_world::Action,
    ) {
        if !matches!(
            action,
            launcher::local_worlds::in_world::Action::OptionField(_)
        ) {
            self.local_ui.in_world_actions.push(action);
        }
    }

    /// The played world's Game section, with its numeric boxes' live text.
    pub(super) fn world_settings_view(
        &self,
    ) -> Option<std::sync::Arc<launcher::local_worlds::in_world::View>> {
        let mut world = self.local_ui.in_world.clone()?;
        for &id in &self.local_ui.in_world_editing {
            std::sync::Arc::make_mut(&mut world)
                .option_text
                .insert(id, self.local_ui.options[&id].as_str().to_owned());
        }
        Some(world)
    }

    /// The local-world screens' state for the menu view, with the fields' live text.
    pub(super) fn local_view(&self) -> WorldsView {
        let mut view = self.local_ui.view.clone();
        match view.screen {
            Screen::Create => {
                self.local_ui
                    .name
                    .as_str()
                    .clone_into(&mut view.create.name);
                self.local_ui
                    .seed
                    .as_str()
                    .clone_into(&mut view.create.seed_text);
            }
            Screen::Edit => {
                if let Some(edit) = &mut view.edit {
                    self.local_ui.name.as_str().clone_into(&mut edit.name);
                }
            }
            _ => {}
        }
        for &id in &self.local_ui.editing {
            view.option_text
                .insert(id, self.local_ui.options[&id].as_str().to_owned());
        }
        view
    }

    /// Whether a local-world screen covers the worlds tab (Escape then backs out of it).
    pub(super) fn local_screen_open(&self) -> bool {
        matches!(
            self.screen,
            MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers
        ) && !matches!(self.local_ui.view.screen, Screen::List | Screen::Opening)
    }

    pub(super) fn queue_local_action(&mut self, action: LocalWorldAction) {
        self.local_ui.actions.push(action);
    }

    fn push_local_text(&mut self, worlds: &mut LocalWorlds) {
        let ui = &self.local_ui;
        for &id in &ui.editing {
            let text = ui.options[&id].as_str();
            if matches!(ui.view.screen, Screen::Create | Screen::Edit)
                && text != ui.view.numeric_text(id)
            {
                worlds.input(Input::SetOptionText(id, text.to_owned()));
            }
        }
        let field = self.field;
        self.local_ui
            .editing
            .retain(|&id| field == Some(MenuField::WorldOption(id)));
        let ui = &self.local_ui;
        let view = &ui.view;
        match view.screen {
            Screen::Create
                if ui.name.as_str() != view.create.name
                    || ui.seed.as_str() != view.create.seed_text =>
            {
                worlds.input(Input::SetName(ui.name.as_str().to_owned()));
                worlds.input(Input::SetSeed(ui.seed.as_str().to_owned()));
            }
            Screen::Edit
                if view
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.name != ui.name.as_str()) =>
            {
                worlds.input(Input::SetEditName(ui.name.as_str().to_owned()));
            }
            _ => {}
        }
    }

    /// Entering a form loads its text; leaving the forms drops their field focus.
    fn load_local_text(&mut self, view: &WorldsView) {
        match view.screen {
            Screen::Create => {
                self.local_ui.name.set_text(&view.create.name);
                self.local_ui.seed.set_text(&view.create.seed_text);
            }
            Screen::Edit => {
                if let Some(edit) = &view.edit {
                    self.local_ui.name.set_text(&edit.name);
                }
            }
            _ => {}
        }
        if !matches!(view.screen, Screen::Create | Screen::Edit)
            && matches!(
                self.field,
                Some(MenuField::WorldName | MenuField::WorldSeed | MenuField::WorldOption(_))
            )
        {
            self.field = None;
        }
    }

    /// Keyboard and gamepad focus order on the local-world screens.
    pub(super) fn local_focus_actions(&self) -> Option<Vec<MenuAction>> {
        use launcher::menu::worlds_tab::LocalWorldAction as A;
        let view = &self.local_ui.view;
        let local = |actions: &[LocalWorldAction]| {
            Some(
                actions
                    .iter()
                    .copied()
                    .map(MenuAction::LocalWorld)
                    .collect(),
            )
        };
        if let Some(prompt) = view.prompt {
            return local(
                &prompt
                    .buttons()
                    .iter()
                    .map(|button| A::Prompt(*button))
                    .collect::<Vec<_>>(),
            );
        }
        match view.screen {
            Screen::List | Screen::Opening | Screen::BackendPrompt => None,
            // The import modal has no buttons and the copy cannot be interrupted.
            Screen::Importing => Some(Vec::new()),
            Screen::Create | Screen::Edit => {
                let editing = view.screen == Screen::Edit;
                let options = view
                    .edit
                    .as_ref()
                    .filter(|_| editing)
                    .map_or(&view.create.options, |edit| &edit.options);
                let mut actions = vec![if editing { A::PlayFromEdit } else { A::Create }];
                actions.extend(
                    Tab::ALL
                        .iter()
                        .copied()
                        .filter(|tab| {
                            tab.supported() && (*tab == Tab::General || view.options_available())
                        })
                        .map(A::Tab),
                );
                match view.tab {
                    Tab::General => {
                        actions.push(A::NameField);
                        actions.extend(
                            [GameMode::Survival, GameMode::Creative]
                                .into_iter()
                                .map(A::GameMode),
                        );
                        if editing {
                            actions.push(A::GameMode(GameMode::Adventure));
                        }
                        actions.extend(
                            [
                                Difficulty::Peaceful,
                                Difficulty::Easy,
                                Difficulty::Normal,
                                Difficulty::Hard,
                            ]
                            .into_iter()
                            .map(A::Difficulty),
                        );
                        if editing {
                            actions.push(A::Delete);
                        }
                    }
                    Tab::Advanced if !editing => {
                        actions.extend([A::SeedField, A::Flat(false), A::Flat(true)])
                    }
                    _ => {}
                }
                for &id in launcher::local_worlds::options::WorldOption::ALL {
                    let definition = id.definition();
                    if !view.options_available()
                        || definition.tab != view.tab
                        || !options.editable(id, editing)
                    {
                        continue;
                    }
                    match definition.control {
                        launcher::local_worlds::options::Control::Toggle => {
                            actions.push(A::Option(id, 1 - options.get(id)))
                        }
                        launcher::local_worlds::options::Control::Choice(choices) => {
                            actions.extend(choices.iter().map(|choice| A::Option(id, choice.value)))
                        }
                        launcher::local_worlds::options::Control::Number { .. } => {
                            actions.push(A::OptionField(id))
                        }
                    }
                }
                local(&actions)
            }
            Screen::Templates => Some(vec![MenuAction::LocalWorld(A::BeginCreate)]),
            Screen::ConfirmDelete => local(&[A::Back, A::ConfirmDelete]),
            Screen::Eula => local(&[A::AcceptEula, A::ViewEula, A::Back]),
            Screen::Error => local(&[A::Back]),
        }
    }

    pub fn request_local_world_join(&mut self, name: String, hosted: bool) {
        self.retry_target = None;
        self.remember_session_origin();
        self.stop_catalog();
        self.local_world_joined = true;
        self.local_ui.joining = Some(name.clone());
        self.local_ui.hosted = hosted;
        self.intents.join = Some(client_state::JoinIntent {
            address: name,
            auth_cache: None,
            local_world: true,
        });
        self.show_connecting();
    }

    /// The account that hosts local worlds, from the menu's account state.
    fn local_world_host(&self) -> Host {
        let state = self.auth_process.as_ref().map(|process| process.state());
        local_world_host(
            &self.feeds.profile,
            matches!(state, Some(AuthState::Checking | AuthState::Authenticated)),
        )
    }

    /// The open world is hosted for Xbox friends under the signed-in account.
    pub(super) fn hosting_world(&self) -> bool {
        self.local_ui.hosted && !self.feeds.profile.xuid.is_empty()
    }

    /// The address Discord friends join the open world by, while the core hosts it for Xbox
    /// friends under the signed-in account.
    pub fn hosted_world_address(&self) -> Option<String> {
        self.hosting_world().then(|| {
            format!(
                "{}{}",
                launcher::menu::FRIEND_ADDRESS_PREFIX,
                self.feeds.profile.xuid
            )
        })
    }

    /// The hosted world's player limit, for the Discord card's party size.
    pub fn hosted_world_max_players(&self) -> Option<u32> {
        self.local_ui.max_players.filter(|_| self.hosting_world())
    }
}

/// The loaded profile's XUID; a signed-in account whose profile has not loaded yet is loading, and
/// an unavailable profile grants nobody.
fn local_world_host(profile: &MenuProfile, signed_in: bool) -> Host {
    if profile.loaded && !profile.xuid.is_empty() {
        Host::Xuid(profile.xuid.clone())
    } else if signed_in && !profile.loaded {
        Host::Loading
    } else {
        Host::SignedOut
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        launcher::local_worlds::model::Screen,
        launcher::local_worlds::progress::Progress,
        launcher::menu::worlds_tab::LocalWorldAction,
        launcher::menu::{MenuAction, MenuField, MenuScreen},
    };

    #[test]
    fn backend_prompt_only_retains_gameplay_return_for_available_join_actions() {
        use launcher::local_worlds::prompt::{Prompt, PromptButton, PromptFor, PromptKind};

        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        for (kind, blocking, expected) in [
            (
                PromptKind::DockerMissing,
                PromptFor::CreateBds,
                &[PromptButton::Retry][..],
            ),
            (
                PromptKind::DockerNotRunning,
                PromptFor::CreateBds,
                &[PromptButton::Retry][..],
            ),
            (
                PromptKind::DockerMissing,
                PromptFor::Play,
                &[PromptButton::Retry][..],
            ),
            (
                PromptKind::DockerNotRunning,
                PromptFor::Play,
                &[PromptButton::Retry][..],
            ),
        ] {
            menu.local_ui.view.prompt = Some(Prompt { kind, blocking });
            for button in [
                PromptButton::UseDragonfly,
                PromptButton::Retry,
                PromptButton::GetDocker,
                PromptButton::Cancel,
            ] {
                menu.local_ui.actions.clear();
                assert!(!menu.gameplay_return_pending());
                menu.queue_local_action(LocalWorldAction::Prompt(button));
                assert_eq!(
                    menu.gameplay_return_pending(),
                    expected.contains(&button),
                    "{kind:?}, {blocking:?}, {button:?}",
                );
            }
        }
        menu.local_ui.view.prompt = None;
        menu.local_ui.actions.clear();
        menu.queue_local_action(LocalWorldAction::Prompt(PromptButton::UseDragonfly));
        assert!(!menu.gameplay_return_pending());
    }

    /// Starts wait for a signed-in account's profile, and a failed profile load grants nobody.
    #[test]
    fn the_local_world_host_follows_the_account() {
        let mut profile = MenuProfile::default();
        assert_eq!(local_world_host(&profile, false), Host::SignedOut);
        assert_eq!(local_world_host(&profile, true), Host::Loading);
        assert_eq!(
            local_world_host(&MenuProfile::unavailable(), true),
            Host::SignedOut
        );
        profile.loaded = true;
        profile.xuid = "2535400000000002".to_owned();
        assert_eq!(
            local_world_host(&profile, true),
            Host::Xuid("2535400000000002".to_owned())
        );
    }

    #[test]
    fn a_chosen_card_selects_the_world_in_the_module() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        let mut worlds = LocalWorlds::default();
        menu.sync_local_worlds(&mut worlds, false);
        assert!(menu.view().local_worlds.is_empty());
        menu.activate(MenuAction::PlayLocalWorld(0));
        assert_eq!(menu.take_local_world_request(), None);
    }

    /// Keyboard and gamepad focus cannot reach the tab or navigation behind the import modal.
    #[test]
    fn importing_blocks_focus_behind_its_modal() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.local_ui.view.screen = Screen::Importing;
        assert_eq!(menu.focus_actions(), Vec::<MenuAction>::new());
        menu.move_focus(1);
        menu.activate_focused();
        assert_eq!(menu.view().screen, MenuScreen::Play);
        assert!(
            menu.local_ui.actions.is_empty(),
            "nothing reached the module"
        );
    }

    #[test]
    fn a_local_world_session_closes_the_world_when_it_ends() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        let mut worlds = LocalWorlds::default();
        menu.request_local_world_join("Home".to_owned(), false);
        assert!(
            menu.intents
                .join
                .as_ref()
                .is_some_and(|join| join.local_world)
        );
        menu.sync_local_worlds(&mut worlds, false);
        assert!(menu.local_world_active, "connecting counts as live");
        assert_eq!(
            menu.view().local.progress,
            Some(Progress::connecting("Home")),
            "the loading screen's last stage is the join"
        );
        menu.intents.join = None;
        menu.sync_local_worlds(&mut worlds, false);
        assert!(!menu.local_world_active && !menu.local_world_joined);
    }

    #[test]
    fn only_a_dedicated_server_world_with_a_signed_in_host_is_joinable_and_only_while_open() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        let mut worlds = LocalWorlds::default();
        menu.request_local_world_join("Home".to_owned(), true);
        assert_eq!(menu.hosted_world_address(), None, "no signed-in XUID yet");
        menu.feeds.profile.xuid = "2535400000000001".to_owned();
        assert_eq!(
            menu.hosted_world_address(),
            Some(format!(
                "{}2535400000000001",
                launcher::menu::FRIEND_ADDRESS_PREFIX
            ))
        );
        menu.request_local_world_join("Flat".to_owned(), false);
        assert_eq!(
            menu.hosted_world_address(),
            None,
            "dragonfly worlds are not hosted"
        );
        menu.request_local_world_join("Home".to_owned(), true);
        menu.sync_local_worlds(&mut worlds, false);
        menu.intents.join = None;
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(
            menu.hosted_world_address(),
            None,
            "the closed world is no longer hosted"
        );
    }

    /// Create-screen presses reach the module, and typed text lands in its form before submit.
    /// Back during a saved world's Docker probe must reach the module, or a late verdict joins it.
    #[test]
    fn back_during_the_docker_probe_reaches_the_module() {
        use launcher::local_worlds::world::{Prefs, UnavailableReason, World, WorldStatus};
        use launcher::local_worlds::{Event, Input, WorldsMenu};

        let mut worlds = WorldsMenu::default();
        worlds.apply(Event::Listed(vec![World {
            id: "saved".into(),
            name: "Saved".into(),
            game_mode: Default::default(),
            generator: Default::default(),
            difficulty: Default::default(),
            backend: bridge::Backend::Bds,
            seed: 1,
            created_unix: 0,
            last_played_unix: 0,
            size_bytes: 0,
            options: Default::default(),
            options_unavailable: false,
        }]));
        worlds.apply(Event::Prefs(
            Prefs::default(),
            WorldStatus {
                state: bridge::WorldState::Idle,
                world_id: None,
                backend: None,
                paused: false,
                pause_supported: true,
                error: None,
                setup: None,
                backend_unavailable_reason: Some(UnavailableReason::DockerMissing),
                max_players: None,
            },
        ));
        assert!(!worlds.update(Input::Play).is_empty());
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.local_ui.view = worlds.view();
        menu.local_ui.actions.clear();
        menu.go_back();
        assert_eq!(menu.local_ui.actions, vec![LocalWorldAction::Back]);
    }

    #[test]
    fn create_screen_presses_and_text_reach_the_module() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        let mut worlds = LocalWorlds::default();
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::BeginCreate));
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(menu.view().local.screen, Screen::Create);
        assert_eq!(
            menu.local_ui.name.as_str(),
            "My World",
            "the form's text loads"
        );
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::NameField));
        assert_eq!(menu.field, Some(MenuField::WorldName));
        menu.local_ui.name.set_text("Castle");
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::GameMode(
            GameMode::Creative,
        )));
        menu.sync_local_worlds(&mut worlds, false);
        let form = worlds.menu().create_form();
        assert_eq!(
            (form.name.as_str(), form.game_mode),
            ("Castle", GameMode::Creative)
        );
        assert!(menu.local_screen_open());
        menu.go_back();
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(menu.view().local.screen, Screen::List);
        assert_eq!(menu.field, None, "leaving the form drops its field");
    }
    #[test]
    fn world_options_numeric_edits_survive_losing_field_focus() {
        use launcher::local_worlds::options::WorldOption as O;
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        let mut worlds = LocalWorlds::default();
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::BeginCreate));
        menu.sync_local_worlds(&mut worlds, false);
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Tab(Tab::Advanced)));
        menu.sync_local_worlds(&mut worlds, false);
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::OptionField(
            O::SleepPercentage,
        )));
        menu.local_ui
            .options
            .get_mut(&O::SleepPercentage)
            .unwrap()
            .set_text("35");
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Tab(Tab::Cheats)));
        assert_eq!(menu.field, None);
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(
            worlds.menu().create_form().options.get(O::SleepPercentage),
            35
        );
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Tab(Tab::Advanced)));
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::OptionField(
            O::SleepPercentage,
        )));
        assert_eq!(menu.local_ui.options[&O::SleepPercentage].as_str(), "35");
        menu.local_ui
            .options
            .get_mut(&O::SleepPercentage)
            .unwrap()
            .set_text("999");
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Create));
        menu.sync_local_worlds(&mut worlds, false);
        assert!(worlds.menu().form_error().is_some());
        assert!(!worlds.menu().busy());
    }
    /// Leaving a field keeps its invalid text instead of reverting to the last valid prefix.
    #[test]
    fn invalid_numeric_text_survives_losing_field_focus() {
        use launcher::local_worlds::options::WorldOption as O;
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        let mut worlds = LocalWorlds::default();
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::BeginCreate));
        menu.sync_local_worlds(&mut worlds, false);
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Tab(Tab::Advanced)));
        menu.sync_local_worlds(&mut worlds, false);
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::OptionField(
            O::SleepPercentage,
        )));
        let editor = menu.local_ui.options.get_mut(&O::SleepPercentage).unwrap();
        editor.set_text("35");
        menu.sync_local_worlds(&mut worlds, false);
        let editor = menu.local_ui.options.get_mut(&O::SleepPercentage).unwrap();
        editor.set_text("355");
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Tab(Tab::Cheats)));
        assert_eq!(menu.field, None);
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(worlds.menu().view().numeric_text(O::SleepPercentage), "355");
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Create));
        menu.sync_local_worlds(&mut worlds, false);
        assert!(worlds.menu().form_error().is_some());
        assert!(!worlds.menu().busy());
    }

    /// Settings' Game section shows only while the world is played, and its presses and typed
    /// numbers change that world through the module.
    #[test]
    fn game_section_changes_reach_the_played_world() {
        use launcher::local_worlds::{
            in_world::Action, model::Event, options::WorldOption as O, world::World,
        };
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        let mut worlds = LocalWorlds::default();
        worlds.apply(Event::Listed(vec![World {
            options: Default::default(),
            id: "world-id".into(),
            name: "Saved".into(),
            game_mode: GameMode::Survival,
            generator: launcher::local_worlds::world::Generator::Normal,
            difficulty: Difficulty::Normal,
            backend: Backend::Bds,
            seed: 1,
            created_unix: 0,
            last_played_unix: 0,
            options_unavailable: false,
            size_bytes: 0,
        }]));
        menu.local_ui.world_id = Some("world-id".into());
        menu.local_world_joined = true;
        menu.sync_local_worlds(&mut worlds, false);
        assert!(menu.view().world_settings.is_none(), "not yet in the world");
        menu.sync_local_worlds(&mut worlds, true);
        assert!(menu.view().world_settings.is_some());
        menu.activate(MenuAction::WorldSettings(Action::Difficulty(
            Difficulty::Hard,
        )));
        menu.sync_local_worlds(&mut worlds, true);
        assert_eq!(worlds.menu().worlds()[0].difficulty, Difficulty::Hard);
        menu.screen = MenuScreen::Settings;
        menu.activate(MenuAction::WorldSettings(Action::OptionField(
            O::RespawnRadius,
        )));
        assert_eq!(menu.field, Some(MenuField::WorldOption(O::RespawnRadius)));
        assert_eq!(menu.local_ui.options[&O::RespawnRadius].as_str(), "10");
        menu.local_ui
            .options
            .get_mut(&O::RespawnRadius)
            .unwrap()
            .set_text("24");
        menu.sync_local_worlds(&mut worlds, true);
        assert_eq!(worlds.menu().worlds()[0].options.get(O::RespawnRadius), 24);
        assert_eq!(
            menu.view()
                .world_settings
                .map(|world| world.numeric_text(O::RespawnRadius)),
            Some("24".to_owned())
        );
    }

    /// Back saves an edit, so an invalid number must stop it before the last valid prefix saves.
    #[test]
    fn edit_back_refuses_an_invalid_numeric_option() {
        use launcher::local_worlds::{model::Event, options::WorldOption as O, world::World};
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        let mut worlds = LocalWorlds::default();
        worlds.apply(Event::Listed(vec![World {
            options: Default::default(),
            id: "world-id".into(),
            name: "Saved".into(),
            game_mode: GameMode::Survival,
            generator: launcher::local_worlds::world::Generator::Normal,
            difficulty: launcher::local_worlds::world::Difficulty::Normal,
            backend: Backend::Bds,
            seed: 1,
            created_unix: 0,
            last_played_unix: 0,
            options_unavailable: false,
            size_bytes: 0,
        }]));
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Edit(0)));
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(worlds.menu().screen(), Screen::Edit);
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Tab(Tab::Advanced)));
        menu.sync_local_worlds(&mut worlds, false);
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::OptionField(
            O::SleepPercentage,
        )));
        let editor = menu.local_ui.options.get_mut(&O::SleepPercentage).unwrap();
        editor.set_text("35");
        menu.sync_local_worlds(&mut worlds, false);
        let editor = menu.local_ui.options.get_mut(&O::SleepPercentage).unwrap();
        editor.set_text("355");
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::Back));
        menu.sync_local_worlds(&mut worlds, false);
        assert_eq!(worlds.menu().screen(), Screen::Edit);
        assert!(worlds.menu().form_error().is_some());
        assert!(!worlds.menu().busy(), "nothing was saved");
    }
}
