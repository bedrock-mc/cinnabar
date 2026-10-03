//! The live scene stack: the screens up this frame, bottom first, each with the
//! settings its vanilla screen definition declares. Composition, gameplay input
//! and mouse capture all read this one stack.

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::{Res, ResMut};
use json_ui::{Catalog, Context, SceneStack, ScreenSettings};

use super::UiRuntime;
use crate::menu::{MenuRuntime, MenuScreen};

const BED_SCREEN: &str = "bed.in_bed_screen";
const SERVER_FORM_SCREEN: &str = "server_form.third_party_server_screen";

/// A screen the host can have up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scene {
    /// The world and its in-world overlays: the native `InGamePlayScreen`.
    Gameplay,
    /// The crosshair overlay, which stays drawn under other screens.
    Crosshair,
    Hud,
    Bed,
    Container,
    Chat,
    Loading,
    SignEditor,
    ServerForm,
    Menu(MenuScreen),
    /// The server settings form, which opens over the settings menu.
    ServerSettingsForm,
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
pub(crate) struct SceneHost {
    /// The visible menu screen.
    pub(crate) menu: Option<MenuScreen>,
    /// The menu opened over the session's world (pause, death) rather than the launcher's.
    pub(crate) over_world: bool,
    /// The world-loading cover is up.
    pub(crate) loading: bool,
}

impl SceneHost {
    pub(crate) fn of(menu: Option<&MenuRuntime>, runtime: &UiRuntime) -> Self {
        Self {
            menu: menu.and_then(MenuRuntime::scene),
            over_world: menu.is_none_or(MenuRuntime::over_world),
            loading: runtime.loading_screen,
        }
    }
}

/// The settings of every screen the host can push, from the live catalog.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScreenSettingsTable(HashMap<&'static str, ScreenSettings>);

impl ScreenSettingsTable {
    pub(crate) fn resolve(
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
    pub(crate) fn for_catalog(catalog: &Catalog, context: &Context) -> Self {
        Self::resolve(catalog, context, Self::host_references())
    }

    pub(crate) fn get(&self, reference: &str) -> Option<ScreenSettings> {
        self.0.get(reference).copied()
    }

    /// Every screen reference a [`Scene`] can resolve to.
    pub(crate) fn host_references() -> impl Iterator<Item = &'static str> {
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
    pub(crate) fn set_screen_settings(&mut self, table: Arc<ScreenSettingsTable>) {
        self.screen_settings = table;
    }

    /// Takes this frame's loading cover and the presentation catalog's settings.
    pub(crate) fn observe_presentation(&mut self, loading: bool, table: Arc<ScreenSettingsTable>) {
        self.loading_screen = loading;
        if !Arc::ptr_eq(&table, &self.screen_settings) {
            self.screen_settings = table;
        }
    }

    /// The screens up this frame, bottom first.
    pub(crate) fn scenes(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        host: SceneHost,
    ) -> SceneStack<Scene> {
        self.scenes_in(player_runtime, host, &self.screen_settings)
    }

    /// [`Self::scenes`] with `table`'s settings, for a caller holding a newer catalog's.
    pub(crate) fn scenes_in(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
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
        stack
    }

    /// Whether gameplay (attack, use, movement, look) receives input: no scene
    /// above it absorbs input. Before a world, the game takes what nothing absorbs.
    pub(crate) fn gameplay_input(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        menu: Option<&MenuRuntime>,
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
    pub(crate) fn steals_mouse(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        menu: Option<&MenuRuntime>,
    ) -> bool {
        self.scenes(player_runtime, SceneHost::of(menu, self))
            .top()
            .is_none_or(|scene| scene.settings.should_steal_mouse)
    }

    pub(crate) fn note_player_hurt(&mut self) {
        self.hurt_pending = true;
    }
}

/// When each scene's screen last entered, as the event clocks its transition
/// fades read: `screen.entrance_push` when pushed, `screen.entrance_pop` when
/// the scene above it leaves.
#[derive(Debug, Default)]
pub(crate) struct SceneClocks {
    previous: Vec<Scene>,
    entered: Vec<(Scene, &'static str, f64)>,
    started: bool, // the first frame's scenes appear without a transition
}

impl SceneClocks {
    /// Notes `stack` (bottom first) as this frame's scenes at `now` seconds.
    pub(crate) fn observe(&mut self, stack: &[Scene], now: f64) {
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
    pub(crate) fn clocks(&self, scene: Scene) -> std::collections::BTreeMap<String, f64> {
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

/// Closes the top scene when the player is hurt and its screen asks for it.
pub(crate) fn close_scenes_on_player_hurt(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntime>,
) {
    if !std::mem::take(&mut runtime.hurt_pending) {
        return;
    }
    let host = SceneHost::of(menu.as_deref(), &runtime);
    match runtime.scenes(&player_runtime, host).closes_on_hurt() {
        Some(Scene::Container) => {
            runtime.close_inventory(&mut player_runtime);
        }
        Some(Scene::Chat) => {
            runtime.close_chat();
        }
        Some(Scene::SignEditor) => runtime.sign_editor_mut().close_on_hurt(),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
