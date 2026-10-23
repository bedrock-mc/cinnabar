//! Host state that data-driven UI answers depend on; its state lives in client-ui.

use bevy::prelude::{Res, ResMut};
use client_shell::{menu::MenuRuntime, ui_resources::UiRuntimeResource};

/// Publishes whether a host menu shows over the world, so screen requests arriving meanwhile
/// answer busy.
pub(crate) fn publish_menu_visibility(
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntimeResource>,
) {
    let visible = menu.as_ref().is_some_and(|menu| menu.is_visible());
    if runtime.data_driven_ui().menu_visible() != visible {
        runtime.set_menu_visible(visible);
    }
}
