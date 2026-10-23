//! Builds a mining target from the current interaction facts for ordered attack admission.

use crate::interaction_authority::observe_block;
use client_interaction::mining::hand_interaction_selection;
use client_world::game_mode_capabilities::GameModeCapabilities;
use gameplay::{
    interaction_authority::within_pick_range,
    mining::{creative_reach, protocol_input_mode, survival_reach},
    survival_mining::{
        DestroyTarget, ToolWear, destroys_in_creative, exempt_from_airborne_penalty, eyes_in_water,
        percent_roll, tool_break_damage, unbreaking_keeps_damage,
    },
};
use semantic_input::identity::{InputAuthorityGeneration, InputSequence};
use sim::{ComponentDigger, DestroyConditions, HeldTool, PaletteWorld};
use simulation_clock::identity::PositionAuthorityGeneration;

/// Reads the destroy facts used for this frame's block press.
pub(crate) fn observe_destroy_target(
    player_runtime: &client_state::PlayerRuntime,
    context: &crate::melee::MeleeContext,
    origin: &client_presentation::local_player::InteractionOriginSnapshot,
    caps: Option<GameModeCapabilities>,
    input_mode: semantic_input::InputMode,
    input_authority: (InputAuthorityGeneration, InputSequence),
    position_authority_generation: PositionAuthorityGeneration,
) -> Option<DestroyTarget> {
    let ui = &context.ui;
    // The capability gate in the producer already confirmed this mode edits;
    // here only an open UI blocks the pick.
    let caps = caps?;
    if context.focus.ui_focused {
        return None;
    }
    let selection = hand_interaction_selection(player_runtime)?;
    let input_mode = protocol_input_mode(input_mode);
    let observed = observe_block(
        origin,
        ui,
        &context.client_world,
        &context.collisions,
        selection,
        player_runtime.facts.player_game_mode(),
        (
            input_mode,
            if caps.creative_reach {
                creative_reach(input_mode)
            } else {
                survival_reach(input_mode)
            },
            input_authority,
            position_authority_generation,
        ),
    )?;
    if !within_pick_range(&observed) {
        return None;
    }
    let stream = context.client_world.stream.as_ref()?;
    let held = stream.authority().held_edit_item(
        &observed.selection.item,
        protocol::ItemBlockList::CanDestroy,
        observed.target.runtime_id,
    );
    if !caps.permits_edit(held) {
        return None;
    }
    let mode = stream.network_id_mode();
    let native_destroy_facts = stream
        .runtime_assets()
        .resolve(mode, observed.target.runtime_id)
        .destroy();
    let mut destroy_facts = native_destroy_facts;
    let item = &observed.selection.item;
    let fact = ui
        .inventory_ledger(player_runtime)
        .item_facts()
        .get(item.network_id());
    let entry = ui
        .inventory_ledger(player_runtime)
        .negotiated_item_entry(item.network_id());
    if let Some((mining, effective_tools)) = context
        .collisions
        .custom_block_mining(mode, observed.target.runtime_id)
    {
        let builtin = assets::facts::ItemFacts::builtin().ok();
        let seconds = pack_runtime::custom_mining_seconds(
            mining,
            &context.client_world.mining_filters,
            entry.map(|entry| entry.identifier.as_ref()),
            item.metadata(),
            &|tag| {
                entry.is_some_and(|entry| entry.item_tags.iter().any(|value| value.as_ref() == tag))
                    || builtin
                        .as_ref()
                        .and_then(|facts| facts.tag(tag))
                        .is_some_and(|tag| fact.tags.contains(tag))
            },
        );
        destroy_facts = assets::compiled::block_destroy::BlockDestroyFacts::Custom {
            seconds_to_destroy_bits: seconds.to_bits(),
            effective_tools,
        };
    }
    let block = sim::BlockDestroyInfo::from_facts(destroy_facts);
    let instant = caps.instant_break;
    if instant && !destroys_in_creative(fact) {
        return None;
    }
    let tool = HeldTool::from_facts(fact);
    let block_tags = context
        .collisions
        .block_tags(mode, observed.target.runtime_id);
    let has_block_tag = |tag: &str| {
        if block_tags.iter().any(|value| value.as_ref() == tag) {
            Some(true)
        } else if context
            .collisions
            .block_tags_complete(mode, observed.target.runtime_id)
        {
            Some(false)
        } else {
            native_destroy_facts.native_tool_tag(tag)
        }
    };
    let component_digger = entry
        .and_then(|entry| entry.components.as_ref()?.digger.as_deref())
        .map(|digger| {
            component_digger(
                digger,
                &context.client_world.digger_filters,
                context
                    .collisions
                    .block_identifier(mode, observed.target.runtime_id),
                context
                    .collisions
                    .block_states(mode, observed.target.runtime_id),
                &has_block_tag,
                protocol::item_enchantment_level(item.extra_data(), EFFICIENCY_ENCHANTMENT_ID)
                    .unwrap_or(0),
            )
        });
    let world = PaletteWorld::new(
        stream.collision_store(),
        context.collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let effects = context.effects.mining_effects();
    let helmet = ui.local_armor(player_runtime).helmet;
    let unbreaking =
        protocol::item_enchantment_level(item.extra_data(), UNBREAKING_ENCHANTMENT_ID).unwrap_or(0);
    let wear = tool.filter(|_| !instant).and_then(|tool| {
        (item.stack_network_id() > 0).then(|| ToolWear {
            // Outstanding and corrected predictions outrank the stack's own tag.
            current_damage: ui
                .inventory_ledger(player_runtime)
                .predicted_slot_damage(observed.selection.slot)
                .or_else(|| {
                    protocol::item_extra_damage(item.extra_data())
                        .and_then(|damage| i32::try_from(damage).ok())
                })
                .unwrap_or(0),
            break_damage: if protocol::item_extra_unbreakable(item.extra_data())
                || !unbreaking_keeps_damage(unbreaking, percent_roll())
            {
                0
            } else {
                tool_break_damage(tool.kind)
            },
        })
    });
    Some(DestroyTarget {
        local_runtime_id: stream.local_player_runtime_id(),
        position: observed.target.position,
        face: observed.target.face,
        runtime_id: observed.target.runtime_id,
        wire_runtime_id: stream.block_network_id(observed.target.runtime_id)?,
        relative_hit: observed.target.relative_hit,
        block,
        conditions: DestroyConditions {
            tool,
            component_digger,
            efficiency_level: protocol::item_enchantment_level(
                item.extra_data(),
                EFFICIENCY_ENCHANTMENT_ID,
            )
            .unwrap_or(0),
            haste_amplifier: effects.haste,
            conduit_power_amplifier: effects.conduit_power,
            mining_fatigue_amplifier: effects.mining_fatigue,
            on_ground: true,
            flying: exempt_from_airborne_penalty(
                context.physics.as_ref().map(|physics| physics.mode()),
            ),
            riding: ui.gameplay_hud().mount_unique_id().is_some(),
            eyes_in_water: eyes_in_water(&world, observed.ray.origin),
            aqua_affinity: protocol::item_enchantment_level(
                &helmet.extra_data,
                AQUA_AFFINITY_ENCHANTMENT_ID,
            )
            .is_some_and(|level| level > 0),
        },
        selection: observed.selection,
        wear,
        instant,
    })
}

/// Resolves a component item's digger speed for one block. A digger replaces the item's vanilla
/// tool rules, so an unmatched entry list, an unknown block or an entry that cannot be decided
/// all mine like an empty hand.
fn component_digger(
    digger: &protocol::ItemDigger,
    filters: &pack_runtime::DiggerFilters,
    block: Option<&str>,
    states: Option<&[(std::sync::Arc<str>, protocol::CustomStateValue)]>,
    has_tag: &dyn Fn(&str) -> Option<bool>,
    efficiency_level: u8,
) -> ComponentDigger {
    let matched = block.map_or(pack_runtime::DiggerMatch::Unresolved, |block| {
        pack_runtime::digger_match(digger, filters, block, states, has_tag)
    });
    match matched {
        pack_runtime::DiggerMatch::Speed(speed) => {
            let level = f32::from(efficiency_level);
            // The Efficiency bonus applies only when the digger opts in and the item is enchanted.
            let bonus = if digger.use_efficiency && efficiency_level > 0 {
                level * level + 1.0
            } else {
                0.0
            };
            ComponentDigger {
                speed: speed as f32 + bonus,
                matched: true,
            }
        }
        pack_runtime::DiggerMatch::None | pack_runtime::DiggerMatch::Unresolved => {
            ComponentDigger {
                speed: 1.0,
                matched: false,
            }
        }
    }
}

const AQUA_AFFINITY_ENCHANTMENT_ID: i16 = 8;
const EFFICIENCY_ENCHANTMENT_ID: i16 = 15;
const UNBREAKING_ENCHANTMENT_ID: i16 = 17;

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{DiggerBlock, DiggerSpeed, ItemDigger, ItemRegistryEntry};
    use std::sync::Arc;

    const HAND: ComponentDigger = ComponentDigger {
        speed: 1.0,
        matched: false,
    };

    fn pickaxe_query(use_efficiency: bool) -> ItemDigger {
        ItemDigger {
            use_efficiency,
            destroy_speeds: Arc::new([DiggerSpeed {
                block: DiggerBlock::Tags(
                    "q.any_tag('minecraft:is_pickaxe_item_destructible')".into(),
                ),
                speed: 8,
            }]),
        }
    }

    fn prepared(digger: &ItemDigger) -> pack_runtime::DiggerFilters {
        let entry = ItemRegistryEntry {
            components: Some(Arc::new(protocol::ItemComponents {
                digger: Some(Arc::new(digger.clone())),
                ..Default::default()
            })),
            ..protocol::vanilla_item_registry()[0].clone()
        };
        pack_runtime::DiggerFilters::new([&entry])
    }

    fn pickaxe_tag(tag: &str) -> Option<bool> {
        Some(tag == "minecraft:is_pickaxe_item_destructible")
    }

    #[test]
    fn undecidable_component_digger_mines_like_a_hand() {
        let digger = pickaxe_query(true);
        let unprepared = pack_runtime::DiggerFilters::default();
        let concrete = Some("minecraft:red_concrete");
        assert_eq!(
            component_digger(&digger, &unprepared, concrete, None, &pickaxe_tag, 5),
            HAND
        );
        assert_eq!(
            component_digger(&digger, &prepared(&digger), None, None, &pickaxe_tag, 5),
            HAND
        );
    }

    #[test]
    fn matched_component_digger_adds_efficiency_only_when_it_opts_in() {
        let concrete = Some("minecraft:red_concrete");
        let opted_in = pickaxe_query(true);
        assert_eq!(
            component_digger(
                &opted_in,
                &prepared(&opted_in),
                concrete,
                None,
                &pickaxe_tag,
                2
            ),
            ComponentDigger {
                speed: 13.0,
                matched: true,
            }
        );
        let plain = pickaxe_query(false);
        assert_eq!(
            component_digger(&plain, &prepared(&plain), concrete, None, &pickaxe_tag, 2),
            ComponentDigger {
                speed: 8.0,
                matched: true,
            }
        );
        assert_eq!(
            component_digger(
                &plain,
                &prepared(&plain),
                concrete,
                None,
                &|_| Some(false),
                2
            ),
            HAND
        );
    }
}
