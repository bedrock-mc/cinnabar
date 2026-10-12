//! Ordered app adapter for UI scene retirement.
use bevy::prelude::{Res, ResMut};
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene;
use {client_shell::menu::MenuRuntime, client_state::PlayerRuntime};

/// Applies damage-driven scene retirement at its existing authority phase.
pub(crate) fn close_scenes_on_player_hurt(
    mut player_runtime: ResMut<PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntimeResource>,
) {
    runtime.close_scenes_on_player_hurt(
        &mut player_runtime,
        menu.as_deref().map(|menu| menu as &dyn MenuScene),
    );
}
