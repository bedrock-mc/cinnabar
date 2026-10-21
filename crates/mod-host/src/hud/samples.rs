//! Representative facts scoped to preview-only policies; never installed as player state.

use crate::{
    PlayerStateEffect, PlayerStateItem, PlayerStateSlot, PlayerStateSnapshot, SidebarRow,
    SidebarSnapshot,
};
use ui::mod_hud::NativeRows;

/// Expands admitted sample identities before any native source consumes its formatting policy.
pub(super) fn expand(hud: &mut ui::mod_hud::Hud) {
    let mut samples = Vec::new();
    for card in &mut hud.cards {
        if let Some(id) = card.preview_id.take() {
            let mut sample = card.clone();
            sample.id = id;
            sample.preview_only = true;
            sample.sample = true;
            samples.push(sample);
        }
    }
    hud.cards.extend(samples);
}

/// Creates a known item using the installed native fact table for durability.
fn item(identifier: &str, count: u16) -> PlayerStateSlot {
    let maximum = assets::facts::ItemFacts::builtin().ok().and_then(|facts| {
        facts
            .by_identifier(identifier)
            .and_then(|fact| fact.durability.maximum())
    });
    PlayerStateSlot {
        known: true,
        item: Some(PlayerStateItem {
            identifier: Some(identifier.into()),
            network_id: 1,
            metadata: 0,
            count,
            block: identifier == "minecraft:stone",
            damage: maximum.map(|_| 0),
            max_durability: maximum,
        }),
    }
}

/// Supplies one bounded native fixture; the caller still runs the ordinary source policy.
pub(super) fn snapshot(kind: &NativeRows) -> PlayerStateSnapshot {
    let empty = PlayerStateSlot {
        known: true,
        item: None,
    };
    let mut inventory = vec![empty.clone(); mod_api::PLAYER_STATE_INVENTORY_SLOTS];
    for (index, identifier, count) in [
        (0, "minecraft:clock", 1),
        (1, "minecraft:diamond_sword", 1),
        (2, "minecraft:stone", 64),
        (9, "minecraft:ender_pearl", 16),
        (10, "minecraft:golden_apple", 12),
        (11, "minecraft:arrow", 32),
        (18, "minecraft:diamond_pickaxe", 1),
        (19, "minecraft:torch", 64),
        (27, "minecraft:stone", 32),
        (28, "minecraft:totem_of_undying", 1),
    ] {
        inventory[index] = item(identifier, count);
    }
    if let NativeRows::ItemCount {
        items,
        blocks,
        potion_effect,
        ..
    } = kind
    {
        inventory.fill(empty.clone());
        let identifier = items.first().map(String::as_str).unwrap_or(if *blocks {
            "minecraft:stone"
        } else {
            "minecraft:potion"
        });
        inventory[0] = item(identifier, 12);
        if let Some(effect) = potion_effect
            && let Some((index, _)) = assets::potion::POTION_DESCRIPTORS
                .iter()
                .enumerate()
                .find(|(_, p)| p.effects.iter().any(|entry| entry.effect == effect))
        {
            inventory[0] = item("minecraft:potion", 12);
            inventory[0].item.as_mut().expect("sample potion").metadata = index as u32;
        }
    }
    PlayerStateSnapshot {
        session: 1,
        dimension: 0,
        selected_slot: Some(0),
        inventory,
        armor: [
            "minecraft:diamond_helmet",
            "minecraft:diamond_chestplate",
            "minecraft:diamond_leggings",
            "minecraft:diamond_boots",
        ]
        .into_iter()
        .map(|id| item(id, 1))
        .collect(),
        offhand: item("minecraft:shield", 1),
        effects: [(1, 1200), (5, 20)]
            .into_iter()
            .map(|(effect_id, ticks)| PlayerStateEffect {
                effect_id,
                amplifier: 3,
                remaining_ticks: Some(ticks),
                ambient: false,
                particles: true,
            })
            .collect(),
    }
}

/// Exercises the same title, formatting and optional score columns as a server sidebar.
pub(super) fn sidebar() -> SidebarSnapshot {
    SidebarSnapshot {
        title: "Example match".into(),
        rows: [("Kills", 3), ("Wins", 12), ("Players", 8)]
            .into_iter()
            .map(|(label, score)| SidebarRow {
                label: label.into(),
                score,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::mod_hud::{Card, Hud, RowSource};

    /// One published policy produces an isolated sample while its live source remains empty.
    #[test]
    fn native_alias_materializes_without_fabricating_live_observations() {
        let mut state = super::super::super::State::new(
            crate::ModGrants {
                hud: true,
                player_state: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        super::super::set_content(&mut state,r#"{"cards":[{"id":"inventory","preview_id":"sample_inventory","rows":[],"source":{"kind":{"kind":"inventory","hotbar":false},"limit":36}}]}"#.into()).unwrap().unwrap();
        state.hud.commit();
        super::super::sources::refresh(&mut state);
        let hud = state.hud.resolved.as_ref().unwrap();
        assert!(hud.cards[0].rows.is_empty());
        assert!(hud.cards[0].preview_id.is_none());
        assert_eq!(hud.cards[1].id, "sample_inventory");
        assert_eq!(hud.cards[1].rows.len(), 27);
        assert!(hud.cards[1].sample && !hud.cards[1].gameplay_visible());
        hud.validate().unwrap();
        assert!(state.player_state.snapshot.is_none());
    }

    /// Samples and real observations run identical policies, including each configurable branch.
    #[test]
    fn samples_match_native_rows_without_leaking_into_gameplay() {
        for kind in [
            NativeRows::Inventory { hotbar: false },
            NativeRows::Inventory { hotbar: true },
            NativeRows::Armor {
                held: true,
                percent: false,
                empty: false,
                compact: true,
                offhand: true,
                warning: 20.,
            },
            NativeRows::Armor {
                held: false,
                percent: true,
                empty: false,
                compact: true,
                offhand: false,
                warning: 20.,
            },
            NativeRows::Effects {
                icons: true,
                stacked: true,
                warnings: true,
            },
            NativeRows::Effects {
                icons: false,
                stacked: true,
                warnings: false,
            },
            NativeRows::Sidebar { scores: true },
            NativeRows::Sidebar { scores: false },
            NativeRows::ItemCount {
                label: "Items".into(),
                items: vec!["minecraft:ender_pearl".into()],
                blocks: false,
                potion_effect: None,
            },
            NativeRows::ItemCount {
                label: String::new(),
                items: Vec::new(),
                blocks: false,
                potion_effect: Some("minecraft:instant_health".into()),
            },
            NativeRows::ItemCount {
                label: "Blocks".into(),
                items: Vec::new(),
                blocks: true,
                potion_effect: None,
            },
        ] {
            let facts = snapshot(&kind);
            super::super::super::player_state::validate(Some(&facts)).unwrap();
            let source = RowSource {
                kind,
                limit: 36,
                color: [1.; 4],
                brackets: false,
            };
            let mut sample = Hud {
                cards: vec![Card {
                    id: "sample".into(),
                    preview_only: true,
                    sample: true,
                    source: Some(source.clone()),
                    ..Default::default()
                }],
                ..Default::default()
            };
            sample.validate().unwrap();
            let mut live = sample.clone();
            live.cards[0].sample = false;
            live.cards[0].preview_only = false;
            super::super::sources::hydrate(&mut sample, None, None);
            super::super::sources::hydrate(&mut live, Some(&facts), Some(&sidebar()));
            assert_eq!(
                sample.cards[0].rows, live.cards[0].rows,
                "{:?}",
                source.kind
            );
            assert!(!sample.cards[0].gameplay_visible());
            assert!(!sample.cards[0].rows.is_empty());
        }
    }
}
