//! What the client reads of the recipe book outside its screen: whether it shows, and the icon
//! and tooltip of each listed entry.

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod tests;

use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::inventory_actions::recipe_book_entries;
use ui::IconRef;
use ui_screens::recipe_book::BookEntry;

/// Whether the panel shows: creative opens on it and the toggle flips either way.
pub fn recipe_book_shown(player_runtime: &player_state::PlayerState, runtime: &UiRuntime) -> bool {
    if ui_screens::furnace_book::active(player_runtime) {
        return runtime.screen_state().furnace_book_open;
    }
    let creative =
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
    creative != runtime.screen_state().book_open
}

/// The listed entries' icons, in list order.
pub fn recipe_book_icons(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    icon: impl Fn(&protocol::NetworkItemStack) -> Option<IconRef>,
) -> Vec<Option<IconRef>> {
    if ui_screens::furnace_book::active(player_runtime) {
        return super::furnace_book::entries(player_runtime, runtime)
            .iter()
            .map(|recipe| icon(&ui_screens::furnace_book::output_stack(recipe)))
            .collect();
    }
    recipe_book_entries(player_runtime, runtime)
        .iter()
        .map(|entry| icon(&entry.stack()))
        .collect()
}

/// The stack a hovered entry's tooltip describes; a group head names its group.
pub fn recipe_book_hover(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    index: u16,
) -> Option<(protocol::NetworkItemStack, Option<std::sync::Arc<str>>)> {
    if ui_screens::furnace_book::active(player_runtime) {
        return Some((
            ui_screens::furnace_book::output_stack(
                super::furnace_book::entries(player_runtime, runtime).get(usize::from(index))?,
            ),
            None,
        ));
    }
    let entries = recipe_book_entries(player_runtime, runtime);
    let entry = entries.get(usize::from(index))?;
    let name = match entry {
        BookEntry::Group { group, .. } => Some(
            runtime
                .translation(&group.name)
                .unwrap_or_else(|| std::sync::Arc::clone(&group.name)),
        ),
        _ => None,
    };
    Some((entry.stack(), name))
}
