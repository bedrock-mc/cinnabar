//! Ordered app adapter for UI scene retirement.
use crate::{menu::MenuRuntime, player_runtime::PlayerRuntime};
use bevy::prelude::{Res, ResMut};
use client_ui::ui_runtime::UiRuntime;
use client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene;

/// Applies damage-driven scene retirement at its existing authority phase.
pub(crate) fn close_scenes_on_player_hurt(
    mut player_runtime: ResMut<PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntime>,
) {
    runtime.close_scenes_on_player_hurt(
        &mut player_runtime,
        menu.as_deref().map(|menu| menu as &dyn MenuScene),
    );
}
