//! What the client reads of the recipe book outside its screen: whether it shows, and the icon
//! and tooltip of each listed entry.

#[cfg(test)]
mod cache_tests;
mod icons;
pub(crate) use icons::BookIconCache;
pub use icons::BookIconSources;
#[cfg(test)]
mod large_catalog_tests;
#[cfg(test)]
mod tests;

use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::inventory_actions::recipe_book_entries;
use ui::IconRef;
use ui_screens::recipe_book::BookEntry;

/// Reads the open controller's active layout, including its context default.
pub fn recipe_book_shown(player_runtime: &player_state::PlayerState, runtime: &UiRuntime) -> bool {
    if ui_screens::furnace_book::active(player_runtime) {
        return runtime
            .furnace_options(player_runtime)
            .is_some_and(|options| options.layout == protocol::FurnaceLayout::RecipeBook);
    }
    runtime
        .inventory_controller(player_runtime)
        .is_some_and(|controller| {
            matches!(
                controller.layout(),
                protocol::InventoryLayout::RecipeBook | protocol::InventoryLayout::RecipeBookOnly
            )
        })
}

/// The listed entries' icons, in list order, resolved again only after an entry or an icon
/// source in `sources` changes.
pub fn recipe_book_icons(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    sources: BookIconSources,
    icon: impl Fn(&protocol::NetworkItemStack) -> Option<IconRef>,
) -> std::sync::Arc<[Option<IconRef>]> {
    let stacks = if ui_screens::furnace_book::active(player_runtime) {
        super::furnace_book::entries(player_runtime, runtime)
            .iter()
            .map(ui_screens::furnace_book::output_stack)
            .collect()
    } else {
        recipe_book_entries(player_runtime, runtime)
            .iter()
            .map(BookEntry::stack)
            .collect()
    };
    runtime
        .recipe_book_icons
        .lock()
        .unwrap()
        .icons(stacks, sources, icon)
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
