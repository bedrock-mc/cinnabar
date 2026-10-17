//! Gathers what an actor wears and holds from the world stream (and, for the local player, the
//! client-owned inventory) into runtime input.

use actor_animation::AttachableAnimationInput;
use assets::ItemVisualRoute;
use chunk_pipeline::WorldStream;
use client_world::{ActorArmorSnapshot, CanonicalItemStack};
use protocol::ActorHandedness;

use super::runtime::{ActorEquipmentInput, HeldKind, WornItem};
use client_ui::ui_runtime::UiRuntime;

impl ActorEquipmentInput {
    /// Carries owner identity and complete equipment into attachable queries while retaining timing.
    pub fn attachable_input<'a>(
        &'a self,
        timing: AttachableAnimationInput<'a>,
    ) -> AttachableAnimationInput<'a> {
        AttachableAnimationInput {
            is_local_player: self.is_local_player,
            owner_main_hand: self.main.as_ref().map(|item| item.identifier.as_ref()),
            owner_off_hand: self.off.as_ref().map(|item| item.identifier.as_ref()),
            ..timing
        }
    }
}

/// A drawable worn item, or `None` for an empty or unresolved stack.
pub(super) fn worn_item(
    item: &CanonicalItemStack,
    dye_rgb: Option<u32>,
    use_facts: client_world::EffectiveItemUse,
) -> Option<WornItem> {
    if item.identity.is_empty() {
        return None;
    }
    Some(WornItem {
        charged_projectile: item
            .charged_projectile
            .as_ref()
            .map(|_| item.charged_projectile_role),
        use_facts,
        grip: item.grip,
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
        glint: item.glint,
        variant: (&item.identity).into(),
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
    .map(|piece| worn_item(&piece.item, piece.dye_rgb, Default::default()))
}

/// A remote actor's equipment from its replicated equipment and armor events.
pub fn remote_input(stream: &WorldStream, runtime_id: u64) -> ActorEquipmentInput {
    let held = |hand| {
        stream
            .authority()
            .actor_equipment_in_hand(runtime_id, hand)
            .and_then(|equipment| {
                worn_item(
                    &equipment.item,
                    None,
                    stream
                        .authority()
                        .item_use_profile(equipment.item.identity.network_id),
                )
            })
    };
    let actor = stream.authority().actor(runtime_id);
    ActorEquipmentInput {
        is_local_player: false,
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
    use inventory::inventory_ledger::InventoryTarget;
    let actor = stream.authority().actor(runtime_id);
    let resolve = |stack: &protocol::NetworkItemStack, dye_rgb: Option<u32>| {
        stream
            .authority()
            .canonical_item_stack(stack)
            .and_then(|item| {
                worn_item(
                    &item,
                    dye_rgb,
                    stream
                        .authority()
                        .item_use_profile(item.identity.network_id),
                )
            })
    };
    let armor = ui
        .map(|ui| ui.local_armor(player_runtime))
        .unwrap_or_default();
    ActorEquipmentInput {
        is_local_player: true,
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

/// Builds a canonical test stack with independent enchantment and intrinsic shine.
#[cfg(test)]
pub(super) fn canonical_fixture(
    identifier: &str,
    enchanted: bool,
    intrinsic: bool,
) -> CanonicalItemStack {
    CanonicalItemStack {
        identity: assets::ItemStackIdentity {
            network_id: 1,
            count: 1,
            ..assets::ItemStackIdentity::empty()
        },
        identifier: Some(identifier.into()),
        enchanted,
        glint: assets::item::glint::ItemGlint::Session { enabled: intrinsic },
        visual: assets::ItemVisualRoute::Missing,
        grip: assets::item::grip::ItemGrip::Unresolved,
        use_animation: assets::item_use::ItemUseAnimation::None,
        charged_projectile: None,
        charged_projectile_role: assets::item_use::ItemProjectileRole::None,
        damage: None,
        durability: assets::item::durability::ItemDurability::NotDamageable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_equipment_ownership_is_independent_of_attachable_perspective() {
        let stream = WorldStream::new(protocol::WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let player = player_state::PlayerState::new(1);
        let local = local_input(&player, &stream, None, 1);
        let remote = remote_input(&stream, 2);
        for worn in [false, true] {
            let timing = AttachableAnimationInput {
                worn,
                ..Default::default()
            };
            let local = local.attachable_input(timing);
            let remote = remote.attachable_input(timing);
            assert!(
                local.is_local_player,
                "local held and worn items retain ownership"
            );
            assert!(!remote.is_local_player, "remote items remain remote");
            assert!(!local.first_person && !remote.first_person);
            assert_eq!(local.worn, worn);
            assert_eq!(remote.worn, worn);
        }
    }

    #[test]
    fn local_attachable_input_retains_other_hand_and_render_timing() {
        let item = |identifier: &str| WornItem {
            charged_projectile: None,
            use_facts: client_world::EffectiveItemUse::default(),
            grip: assets::item::grip::builtin_item_grip(identifier),
            identifier: identifier.into(),
            metadata: 0,
            damage: None,
            kind: HeldKind::Other,
            dye_rgb: None,
            enchanted: false,
            glint: Default::default(),
            variant: Default::default(),
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
    #[test]
    fn worn_glint_combines_intrinsic_dispositions_and_enchantment_facts() {
        let mut item = CanonicalItemStack {
            use_animation: assets::item_use::ItemUseAnimation::None,
            charged_projectile_role: assets::item_use::ItemProjectileRole::None,
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::builtin_item_glint("minecraft:enchanted_book"),
            durability: assets::item::durability::ItemDurability::Unresolved,
            identity: assets::ItemStackIdentity {
                network_id: 7,
                metadata: 0,
                stack_network_id: -1,
                count: 1,
                nbt_digest: [0; 32],
                block_runtime_id: 0,
            },
            identifier: Some(std::sync::Arc::from("minecraft:enchanted_book")),
            visual: ItemVisualRoute::Missing,
            charged_projectile: None,
            damage: None,
            enchanted: false,
        };
        assert!(
            worn_item(&item, None, Default::default())
                .unwrap()
                .has_glint()
        );
        item.glint = assets::item::glint::ItemGlint::Unresolved;
        assert!(
            !worn_item(&item, None, Default::default())
                .unwrap()
                .has_glint()
        );
        item.enchanted = true;
        assert!(
            worn_item(&item, None, Default::default())
                .unwrap()
                .has_glint()
        );
        item.glint = assets::item::glint::ItemGlint::Session { enabled: false };
        assert!(
            worn_item(&item, None, Default::default())
                .unwrap()
                .has_glint()
        );
    }
}
