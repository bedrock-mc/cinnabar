//! The live scene stack: the screens up this frame, bottom first, each with the
//! settings its vanilla screen definition declares. Composition, gameplay input
//! and mouse capture all read this one stack.

use std::{collections::HashMap, sync::Arc};

use json_ui::{Catalog, Context, SceneStack, ScreenSettings};

use super::UiRuntime;
use super::presentation::forms::scene_policy::MenuScene;
use crate::menu::MenuScreen;

const BED_SCREEN: &str = "bed.in_bed_screen";
const SERVER_FORM_SCREEN: &str = "server_form.third_party_server_screen";

/// A screen the host can have up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// The world and its in-world overlays: the native `InGamePlayScreen`.
    Gameplay,
    /// The crosshair overlay, which stays drawn under other screens.
    Crosshair,
    Hud,
    Bed,
    Container,
    Chat,
    Emote,
    Loading,
    SignEditor,
    ServerForm,
    Menu(MenuScreen),
    /// The server settings form, which opens over the settings menu.
    ServerSettingsForm,
    Credits,
}

impl Scene {
    /// Whether both draw one vanilla screen (menu tabs share theirs).
    fn same_screen(self, other: Self) -> bool {
        use super::presentation::forms::menu_reference;
        match (self, other) {
            (Self::Menu(a), Self::Menu(b)) => {
                a == b || menu_reference(a).is_some_and(|r| menu_reference(b) == Some(r))
            }
            _ => self == other,
        }
    }
}

/// What the host shows beyond [`UiRuntime`]'s own state.
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneHost {
    /// The visible menu screen.
    pub menu: Option<MenuScreen>,
    /// The menu opened over the session's world (pause, death) rather than the launcher's.
    pub over_world: bool,
    /// The world-loading cover is up.
    pub loading: bool,
}

impl SceneHost {
    /// Captures launcher visibility and the UI loading cover at the current frame point.
    pub fn of(menu: Option<&dyn MenuScene>, runtime: &UiRuntime) -> Self {
        Self {
            menu: menu.and_then(MenuScene::scene),
            over_world: menu.is_none_or(MenuScene::over_world),
            loading: runtime.loading_screen,
        }
    }
}

/// The settings of every screen the host can push, from the live catalog.
#[derive(Clone, Debug, Default)]
pub struct ScreenSettingsTable(HashMap<&'static str, ScreenSettings>);

impl ScreenSettingsTable {
    pub fn resolve(
        catalog: &Catalog,
        context: &Context,
        references: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self(
            references
                .into_iter()
                .filter_map(|reference| {
                    json_ui::screen_settings(reference, catalog, context)
                        .map(|settings| (reference, settings))
                })
                .collect(),
        )
    }

    /// Settings of every [`Self::host_references`] screen `catalog` defines.
    pub fn for_catalog(catalog: &Catalog, context: &Context) -> Self {
        Self::resolve(catalog, context, Self::host_references())
    }

    pub fn get(&self, reference: &str) -> Option<ScreenSettings> {
        self.0.get(reference).copied()
    }

    /// Every screen reference a [`Scene`] can resolve to.
    pub fn host_references() -> impl Iterator<Item = &'static str> {
        json_ui::ENGINE_SCREENS
            .iter()
            .copied()
            .chain([BED_SCREEN, SERVER_FORM_SCREEN])
            .chain(super::presentation::forms::host_screen_references())
    }
}

/// `InGamePlayScreen`'s overrides of `BaseScreen`: it passes input through (the
/// base default), is not a menu, captures the mouse and always renders.
const fn gameplay_settings() -> ScreenSettings {
    ScreenSettings {
        screen_not_flushable: false,
        always_accepts_input: false,
        render_game_behind: true,
        absorbs_input: false,
        is_showing_menu: false,
        is_modal: true,
        should_steal_mouse: true,
        low_frequency_rendering: false,
        screen_draws_last: false,
        force_render_below: false,
        send_telemetry: true,
        close_on_player_hurt: false,
        use_custom_pocket_toast: false,
        cache_screen: false,
        gamepad_cursor: false,
        gamepad_cursor_deflection_mode: false,
        vertical_scroll_delta: None,
        load_screen_immediately: false,
        render_only_when_topmost: false,
        should_be_skipped_during_automation: false,
    }
}

impl UiRuntime {
    #[cfg(test)]
    pub fn set_screen_settings(&mut self, table: Arc<ScreenSettingsTable>) {
        self.screen_settings = table;
    }

    /// Takes this frame's loading cover and the presentation catalog's settings.
    pub fn observe_presentation(&mut self, loading: bool, table: Arc<ScreenSettingsTable>) {
        self.loading_screen = loading;
        if !Arc::ptr_eq(&table, &self.screen_settings) {
            self.screen_settings = table;
        }
    }

    /// The screens up this frame, bottom first.
    pub fn scenes(
        &self,
        player_runtime: &player_state::PlayerState,
        host: SceneHost,
    ) -> SceneStack<Scene> {
        self.scenes_in(player_runtime, host, &self.screen_settings)
    }

    /// [`Self::scenes`] with `table`'s settings, for a caller holding a newer catalog's.
    pub fn scenes_in(
        &self,
        player_runtime: &player_state::PlayerState,
        host: SceneHost,
        table: &ScreenSettingsTable,
    ) -> SceneStack<Scene> {
        let json = |reference: Option<&str>| {
            reference
                .and_then(|reference| table.get(reference))
                .unwrap_or_default()
        };
        let mut stack = SceneStack::default();
        if self.session_id() != 0 && host.over_world {
            stack.push(Scene::Gameplay, gameplay_settings());
            // The HUD screens are JSON scenes; without their definitions nothing draws them.
            if let Some(settings) = table.get(json_ui::CROSSHAIR_SCREEN) {
                stack.push(Scene::Crosshair, settings);
            }
            if let Some(settings) = table.get(json_ui::HUD_SCREEN) {
                stack.push(Scene::Hud, settings);
            }
        }
        if self.local_sleeping() {
            stack.push(Scene::Bed, json(Some(BED_SCREEN)));
        }
        if self.inventory_open() {
            let reference =
                super::presentation::forms::container_screen_reference(player_runtime, self);
            stack.push(Scene::Container, json(reference));
        }
        if self.chat_focused() {
            stack.push(
                Scene::Chat,
                json(Some(super::presentation::forms::CHAT_SCREEN)),
            );
        }
        if self.emotes().is_open() {
            stack.push(
                Scene::Emote,
                json(Some(super::presentation::forms::EMOTE_SCREEN)),
            );
        }
        if host.loading && host.menu.is_none() {
            let reference = super::presentation::forms::LOADING_SCREEN;
            stack.push(Scene::Loading, json(Some(reference)));
        }
        if self.sign_editor().is_open() {
            let reference = super::presentation::forms::SIGN_SCREEN;
            stack.push(Scene::SignEditor, json(Some(reference)));
        }
        let form = self.server_forms().active();
        let settings_form = self.server_forms().settings_form_active();
        // An answered form keeps absorbing input until its answer is sent.
        if self.server_forms().owns_input() && !settings_form {
            stack.push(
                Scene::ServerForm,
                json(Some(form.map_or(SERVER_FORM_SCREEN, form_screen))),
            );
        }
        if let Some(screen) = host.menu {
            let reference = super::presentation::forms::menu_reference(screen);
            stack.push(Scene::Menu(screen), json(reference));
        }
        if let Some(entry) = form.filter(|_| settings_form) {
            stack.push(Scene::ServerSettingsForm, json(Some(form_screen(entry))));
        }
        if self.credits().owns_input() {
            stack.push(Scene::Credits, json(Some(super::credits::CREDITS_SCREEN)));
        }
        stack
    }

    /// Whether gameplay (attack, use, movement, look) receives input: no scene
    /// above it absorbs input. Before a world, the game takes what nothing absorbs.
    pub fn gameplay_input(
        &self,
        player_runtime: &player_state::PlayerState,
        menu: Option<&dyn MenuScene>,
    ) -> bool {
        self.scenes(player_runtime, SceneHost::of(menu, self))
            .scenes()
            .iter()
            .rev()
            .take_while(|scene| scene.key != Scene::Gameplay)
            .all(|scene| !scene.settings.absorbs_input)
    }

    /// Whether the top scene captures the mouse (`currentScreenShouldStealMouse`);
    /// with nothing up, the game underneath does.
    pub fn steals_mouse(
        &self,
        player_runtime: &player_state::PlayerState,
        menu: Option<&dyn MenuScene>,
    ) -> bool {
        self.scenes(player_runtime, SceneHost::of(menu, self))
            .top()
            .is_none_or(|scene| scene.settings.should_steal_mouse)
    }

    pub fn note_player_hurt(&mut self) {
        self.hurt_pending = true;
    }
}

/// When each scene's screen last entered, as the event clocks its transition
/// fades read: `screen.entrance_push` when pushed, `screen.entrance_pop` when
/// the scene above it leaves.
#[derive(Debug, Default)]
pub struct SceneClocks {
    previous: Vec<Scene>,
    entered: Vec<(Scene, &'static str, f64)>,
    started: bool, // the first frame's scenes appear without a transition
}

impl SceneClocks {
    /// Notes `stack` (bottom first) as this frame's scenes at `now` seconds.
    pub fn observe(&mut self, stack: &[Scene], now: f64) {
        if !std::mem::replace(&mut self.started, true) {
            self.previous = stack.to_vec();
            return;
        }
        for (index, scene) in stack.iter().enumerate() {
            let event = match self
                .previous
                .iter()
                .position(|open| open.same_screen(*scene))
            {
                None => "screen.entrance_push",
                Some(was) if index + 1 == stack.len() && was + 1 != self.previous.len() => {
                    "screen.entrance_pop"
                }
                Some(_) => continue,
            };
            self.entered
                .retain(|(open, name, _)| !(open.same_screen(*scene) && *name == event));
            self.entered.push((*scene, event, now));
        }
        self.entered.retain_mut(|(open, _, _)| {
            match stack.iter().find(|scene| scene.same_screen(*open)) {
                Some(scene) => {
                    *open = *scene;
                    true
                }
                None => false,
            }
        });
        self.previous = stack.to_vec();
    }

    /// The event clocks `scene`'s screen reads this frame.
    pub fn clocks(&self, scene: Scene) -> std::collections::BTreeMap<String, f64> {
        self.entered
            .iter()
            .filter(|(open, _, _)| *open == scene)
            .map(|(_, event, at)| ((*event).to_owned(), *at))
            .collect()
    }
}

fn form_screen(entry: &super::ServerFormEntry) -> &'static str {
    match entry.model {
        protocol::ServerFormModel::NpcDialogue(_) => super::presentation::forms::NPC_SCREEN,
        _ => SERVER_FORM_SCREEN,
    }
}

impl UiRuntime {
    /// Closes the top scene when the player is hurt and its screen asks for it.
    pub fn close_scenes_on_player_hurt(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        menu: Option<&dyn MenuScene>,
    ) {
        if !std::mem::take(&mut self.hurt_pending) {
            return;
        }
        let host = SceneHost::of(menu, self);
        match self.scenes(player_runtime, host).closes_on_hurt() {
            Some(Scene::Container) => {
                self.close_inventory(player_runtime);
            }
            Some(Scene::Chat) => {
                self.close_chat();
            }
            Some(Scene::SignEditor) => self.sign_editor_mut().close_on_hurt(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
