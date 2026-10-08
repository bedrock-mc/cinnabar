//! Gathers what an actor wears and holds from the world stream (and, for the local player, the
//! client-owned inventory) into runtime input.

use assets::ItemVisualRoute;
use chunk_pipeline::WorldStream;
use client_world::{ActorArmorSnapshot, AttachableAnimationInput, CanonicalItemStack};
use protocol::ActorHandedness;

use super::runtime::{ActorEquipmentInput, HeldKind, WornItem};
use client_ui::ui_runtime::UiRuntime;

impl ActorEquipmentInput {
    /// Native attachables run item-name queries against their owner's complete equipment,
    /// not an artificial actor holding only the item currently being drawn.
    pub fn attachable_input<'a>(
        &'a self,
        timing: AttachableAnimationInput<'a>,
    ) -> AttachableAnimationInput<'a> {
        AttachableAnimationInput {
            owner_main_hand: self.main.as_ref().map(|item| item.identifier.as_ref()),
            owner_off_hand: self.off.as_ref().map(|item| item.identifier.as_ref()),
            ..timing
        }
    }
}

/// A drawable worn item, or `None` for an empty or unresolved stack.
pub(super) fn worn_item(item: &CanonicalItemStack, dye_rgb: Option<u32>) -> Option<WornItem> {
    if item.identity.is_empty() {
        return None;
    }
    Some(WornItem {
        identifier: item.identifier.clone()?,
        metadata: item.identity.metadata,
        damage: item.damage,
        kind: match item.visual {
            ItemVisualRoute::Compiled(_) => HeldKind::Sprite,
            ItemVisualRoute::BlockItem(visual) => HeldKind::Block(visual.0),
            _ => HeldKind::Other,
        },
        dye_rgb,
        enchanted: item.enchanted,
    })
}

fn armor_slots(armor: Option<&ActorArmorSnapshot>) -> [Option<WornItem>; 4] {
    let Some(armor) = armor else {
        return [None, None, None, None];
    };
    [
        &armor.helmet,
        &armor.chestplate,
        &armor.leggings,
        &armor.boots,
    ]
    .map(|piece| worn_item(&piece.item, piece.dye_rgb))
}

/// A remote actor's equipment from its replicated equipment and armor events.
pub fn remote_input(stream: &WorldStream, runtime_id: u64) -> ActorEquipmentInput {
    let held = |hand| {
        stream
            .authority()
            .actor_equipment_in_hand(runtime_id, hand)
            .and_then(|equipment| worn_item(&equipment.item, None))
    };
    let actor = stream.authority().actor(runtime_id);
    ActorEquipmentInput {
        main: held(ActorHandedness::Right),
        off: held(ActorHandedness::Left),
        armor: armor_slots(stream.authority().actor_armor(runtime_id)),
        sneaking: actor.is_some_and(|actor| actor.is_sneaking()),
        sleeping: actor.is_some_and(|actor| actor.is_sleeping()),
        java: None,
    }
}

/// The local player's equipment, all from its own containers as vanilla draws it: the selected
/// hotbar stack, the offhand (window 119) and the armor (window 120).
pub fn local_input(
    player_runtime: &player_state::PlayerState,
    stream: &WorldStream,
    ui: Option<&UiRuntime>,
    runtime_id: u64,
) -> ActorEquipmentInput {
    use client_ui::ui_runtime::inventory_ledger::InventoryTarget;
    let actor = stream.authority().actor(runtime_id);
    let resolve = |stack: &protocol::NetworkItemStack, dye_rgb: Option<u32>| {
        stream
            .authority()
            .canonical_item_stack(stack)
            .and_then(|item| worn_item(&item, dye_rgb))
    };
    let armor = ui
        .map(|ui| ui.local_armor(player_runtime))
        .unwrap_or_default();
    ActorEquipmentInput {
        main: ui
            .and_then(|_| player_runtime.selected_stack())
            .and_then(|stack| resolve(stack, None)),
        off: ui
            .and_then(|ui| {
                ui.inventory_ledger(player_runtime)
                    .target_stack(InventoryTarget::Offhand)
            })
            .and_then(|stack| resolve(stack, None)),
        armor: [
            &armor.helmet,
            &armor.chestplate,
            &armor.leggings,
            &armor.boots,
        ]
        .map(|stack| resolve(stack, protocol::item_custom_color(&stack.extra_data))),
        sneaking: actor.is_some_and(|actor| actor.is_sneaking()),
        sleeping: actor.is_some_and(|actor| actor.is_sleeping()),
        java: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_attachable_input_retains_other_hand_and_render_timing() {
        let item = |identifier: &str| WornItem {
            identifier: identifier.into(),
            metadata: 0,
            damage: None,
            kind: HeldKind::Other,
            dye_rgb: None,
            enchanted: false,
        };
        let equipment = ActorEquipmentInput {
            main: Some(item("minecraft:bow")),
            off: Some(item("minecraft:shield")),
            ..ActorEquipmentInput::default()
        };
        let input = equipment.attachable_input(AttachableAnimationInput {
            first_person: true,
            off_hand: true,
            frame_alpha: 0.75,
            use_elapsed_ticks: Some(2),
            ..AttachableAnimationInput::default()
        });
        assert_eq!(
            input.owner_main_hand,
            equipment.main.as_ref().map(|item| item.identifier.as_ref())
        );
        assert_eq!(
            input.owner_off_hand,
            equipment.off.as_ref().map(|item| item.identifier.as_ref())
        );
        assert!(input.first_person && input.off_hand);
        assert_eq!(input.frame_alpha, 0.75);
        assert_eq!(input.use_elapsed_ticks, Some(2));
    }
}
