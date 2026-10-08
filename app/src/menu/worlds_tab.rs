//! The play screen's worlds tab over the local-worlds module: its list feeds
//! the tab's cards, the create/edit/template screens and their modals relay
//! presses to the module, an opened world is joined through the launcher core
//! and closed when its session ends, and the pause menu pauses it.

use protocol::world_control::{Backend, Difficulty, GameMode};

use super::{MenuAction, MenuField, MenuRuntime, MenuScreen};
use crate::local_worlds::{Input, LocalWorlds, Progress, Screen, WorldsView};

pub(crate) use launcher::menu::worlds_tab::LocalWorldAction;
use launcher::menu::worlds_tab::world_card;

/// The menu's side of the local-world screens: the mirrored view, queued presses and the
/// name and seed fields' editors.
#[derive(Debug)]
pub(super) struct LocalWorldsUi {
    view: WorldsView,
    actions: Vec<LocalWorldAction>,
    pub(super) name: ui::ChatEditor,
    pub(super) seed: ui::ChatEditor,
    /// The world being joined, for the loading screen's connect stage.
    joining: Option<String>,
    /// The joined world runs on the dedicated server, which the core hosts for Xbox friends.
    hosted: bool,
    /// The open world's player limit, as its server reported it.
    max_players: Option<u32>,
}

impl Default for LocalWorldsUi {
    fn default() -> Self {
        Self {
            view: WorldsView::default(),
            actions: Vec::new(),
            name: super::input::field_editor(MenuField::WorldName),
            seed: super::input::field_editor(MenuField::WorldSeed),
            joining: None,
            hosted: false,
            max_players: None,
        }
    }
}

impl MenuRuntime {
    /// Includes deliberate joins whose local-world preparation completes on a later frame.
    pub(super) fn gameplay_return_pending(&self) -> bool {
        use crate::local_worlds::{PromptButton, PromptFor};
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
    pub(crate) fn sync_local_worlds(&mut self, worlds: &mut LocalWorlds, in_session: bool) {
        self.sync_storage_worlds(worlds);
        self.push_local_text(worlds);
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
        let cards = worlds.menu().worlds().iter().map(world_card).collect();
        self.set_local_worlds(cards);
        if let Some(index) = self.take_local_world_request() {
            worlds.input(Input::Select(index));
            worlds.input(Input::Play);
            view = worlds.menu().view();
        }
        if let Some(id) = worlds.take_ready() {
            let world = worlds.menu().worlds().iter().find(|world| world.id == id);
            let hosted = world.is_some_and(|world| world.backend == Backend::Bds);
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
                Some(MenuField::WorldName | MenuField::WorldSeed)
            )
        {
            self.field = None;
        }
    }

    /// Keyboard and gamepad focus order on the local-world screens.
    pub(super) fn local_focus_actions(&self) -> Option<Vec<MenuAction>> {
        use LocalWorldAction as A;
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
            Screen::Create => Some(
                A::creation_focus(view)
                    .into_iter()
                    .map(MenuAction::LocalWorld)
                    .collect(),
            ),
            Screen::Edit => local(&[
                A::PlayFromEdit,
                A::NameField,
                A::GameMode(GameMode::Survival),
                A::GameMode(GameMode::Creative),
                A::GameMode(GameMode::Adventure),
                A::Difficulty(Difficulty::Peaceful),
                A::Difficulty(Difficulty::Easy),
                A::Difficulty(Difficulty::Normal),
                A::Difficulty(Difficulty::Hard),
                A::Delete,
            ]),
            Screen::Templates => Some(vec![
                MenuAction::LocalWorld(A::BeginCreate),
                MenuAction::Store(crate::store::OPEN),
            ]),
            Screen::ConfirmDelete => local(&[A::Back, A::ConfirmDelete]),
            Screen::ConfirmLeaveEdit => local(&[A::Save, A::Discard]),
            Screen::Eula => local(&[A::AcceptEula, A::ViewEula, A::Back]),
            Screen::Error => local(&[A::Back]),
        }
    }

    pub(super) fn request_local_world_join(&mut self, name: String, hosted: bool) {
        self.stop_catalog();
        self.local_world_joined = true;
        self.local_ui.joining = Some(name.clone());
        self.local_ui.hosted = hosted;
        self.intents.join = Some(crate::session::JoinIntent {
            address: name,
            auth_cache: None,
            local_world: true,
        });
        self.show_connecting();
    }

    /// The open world is hosted for Xbox friends under the signed-in account.
    pub(super) fn hosting_world(&self) -> bool {
        self.local_ui.hosted && !self.feeds.profile.xuid.is_empty()
    }

    /// The address Discord friends join the open world by, while the core hosts it for Xbox
    /// friends under the signed-in account.
    pub(crate) fn hosted_world_address(&self) -> Option<String> {
        self.hosting_world().then(|| {
            format!(
                "{}{}",
                launcher::menu::FRIEND_ADDRESS_PREFIX,
                self.feeds.profile.xuid
            )
        })
    }

    /// The hosted world's player limit, for the Discord card's party size.
    pub(crate) fn hosted_world_max_players(&self) -> Option<u32> {
        self.local_ui.max_players.filter(|_| self.hosting_world())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_prompt_only_retains_gameplay_return_for_available_join_actions() {
        use launcher::local_worlds::prompt::{Prompt, PromptButton, PromptFor, PromptKind};

        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        for (kind, blocking, expected) in [
            (
                PromptKind::DockerMissing,
                PromptFor::CreateBds,
                &[PromptButton::UseDragonfly][..],
            ),
            (
                PromptKind::DockerNotRunning,
                PromptFor::CreateBds,
                &[PromptButton::UseDragonfly, PromptButton::Retry][..],
            ),
            (PromptKind::DockerMissing, PromptFor::Play, &[][..]),
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

    #[test]
    fn a_chosen_card_selects_the_world_in_the_module() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        let mut worlds = LocalWorlds::default();
        menu.sync_local_worlds(&mut worlds, false);
        assert!(menu.view().local_worlds.is_empty());
        menu.activate(MenuAction::PlayLocalWorld(0));
        assert_eq!(menu.take_local_world_request(), None);
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
}
