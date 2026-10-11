//! Bounded INFO diagnostics for where server items land: the session registry and icons,
//! inventory contents, and remote equipment events.

use std::{collections::HashSet, sync::Mutex};

use bevy::log::info;
use client_world::{EquipmentNotice, EquipmentOutcome};
use protocol::{InventoryEvent, ItemRegistryEvent};

use client_ui::ui_runtime::presentation::SessionIcons;

/// Lines each category may log per session.
const MAX_LINES: usize = 256;

#[derive(Default)]
struct State {
    contents: HashSet<(Option<i32>, Option<u8>, usize, usize)>,
    equipment: HashSet<(u64, bool, Option<EquipmentOutcome>)>,
    recipe_skips: u64,
    recipe_skip_lines: usize,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state(apply: impl FnOnce(&mut State)) {
    if let Ok(mut state) = STATE.lock() {
        apply(state.get_or_insert_with(State::default));
    }
}

/// Logs the StartGame registry's shape and forgets the previous session's lines.
pub(super) fn session_registry(registry: Option<&ItemRegistryEvent>) {
    with_state(|state| *state = State::default());
    let Some(registry) = registry else {
        info!("StartGame carried no usable item registry; vanilla ids only");
        return;
    };
    let custom = registry
        .entries
        .iter()
        .filter(|entry| !entry.identifier.starts_with("minecraft:"))
        .count();
    let component_based = registry
        .entries
        .iter()
        .filter(|entry| entry.component_based)
        .count();
    info!(
        entries = registry.entries.len(),
        custom, component_based, "session item registry"
    );
}

/// Logs how many server item icons the pack stack resolved.
pub(super) fn session_icons(keys: usize, icons: Option<&SessionIcons>) {
    info!(
        registry_icon_keys = keys,
        resolved = icons.map_or(0, |icons| icons.icons.len()),
        explicit_misses = icons.map_or(0, |icons| icons.misses.len()),
        "session item icons compiled from the pack stack"
    );
}

/// Logs bounded station recipe skips and distinct inventory content shapes.
pub(super) fn inventory(event: &InventoryEvent) {
    if let InventoryEvent::Recipes(update) = event {
        let skipped = update.skipped_screen_recipes();
        if skipped > 0 {
            with_state(|state| {
                state.recipe_skips = state.recipe_skips.saturating_add(skipped as u64);
                if state.recipe_skip_lines < MAX_LINES {
                    info!(
                        skipped,
                        total = state.recipe_skips,
                        "station recipes skipped: unsupported inputs or output"
                    );
                    state.recipe_skip_lines += 1;
                }
            });
        }
        return;
    }
    let InventoryEvent::Content(content) = event else {
        return;
    };
    let filled = content
        .slots
        .iter()
        .filter(|stack| !stack.is_empty())
        .count();
    let key = (
        content.container.window_id,
        content.container.slot_type,
        content.slots.len(),
        filled,
    );
    with_state(|state| {
        if state.contents.len() < MAX_LINES && state.contents.insert(key) {
            info!(
                window_id = ?key.0,
                slot_type = ?key.1,
                slots = key.2,
                non_empty = key.3,
                "inventory content received"
            );
        }
    });
}

/// Logs each actor's first armor and held-item event, and every distinct refusal.
pub(super) fn equipment(notices: Vec<EquipmentNotice>) {
    if notices.is_empty() {
        return;
    }
    with_state(|state| {
        for notice in notices {
            // Past the log bound nothing more is printed, so nothing more is remembered.
            if state.equipment.len() >= MAX_LINES * 4 {
                break;
            }
            let first = state
                .equipment
                .insert((notice.runtime_id, notice.armor, None));
            let refused = notice.outcome != EquipmentOutcome::Applied
                && state
                    .equipment
                    .insert((notice.runtime_id, notice.armor, Some(notice.outcome)));
            if !(first || refused) {
                continue;
            }
            let items = notice
                .items
                .iter()
                .map(|item| item.as_deref().map_or("<unregistered id>", |id| id))
                .collect::<Vec<_>>();
            info!(
                runtime_id = notice.runtime_id,
                kind = if notice.armor { "armor" } else { "held" },
                outcome = ?notice.outcome,
                items = ?items,
                "equipment event"
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long session's stream of new actors must not grow the diagnostic memory.
    #[test]
    fn equipment_diagnostics_stop_remembering_past_the_log_bound() {
        session_registry(None);
        equipment(
            (0..(MAX_LINES as u64 * 8))
                .map(|runtime_id| EquipmentNotice {
                    runtime_id,
                    armor: true,
                    outcome: EquipmentOutcome::Applied,
                    items: Box::new([]),
                })
                .collect(),
        );
        let remembered = STATE
            .lock()
            .unwrap()
            .as_ref()
            .map_or(0, |state| state.equipment.len());
        assert_eq!(remembered, MAX_LINES * 4);
    }
}
