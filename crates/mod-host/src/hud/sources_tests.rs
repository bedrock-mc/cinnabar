//! Native source regressions cover bounds, grants, unknown cells, and session loss.

use super::*;
use crate::{ModGrants, PlayerStateItem, PlayerStateSlot, PlayerStateSnapshot};

/// Creates a known inventory with one real stack, one unknown cell and current effects.
fn snapshot() -> PlayerStateSnapshot {
    let mut inventory = vec![
        PlayerStateSlot {
            known: true,
            item: None
        };
        mod_api::PLAYER_STATE_INVENTORY_SLOTS
    ];
    inventory[0].item = Some(PlayerStateItem {
        identifier: Some("minecraft:stone".into()),
        network_id: 1,
        metadata: 0,
        count: 64,
        block: true,
        damage: None,
        max_durability: None,
    });
    inventory[1].known = false;
    PlayerStateSnapshot {
        session: 1,
        dimension: 0,
        selected_slot: Some(0),
        inventory,
        armor: vec![
            PlayerStateSlot {
                known: true,
                item: None
            };
            mod_api::PLAYER_STATE_ARMOR_SLOTS
        ],
        offhand: PlayerStateSlot {
            known: true,
            item: None,
        },
        effects: vec![crate::PlayerStateEffect {
            effect_id: 1,
            amplifier: 1,
            remaining_ticks: Some(120),
            ambient: false,
            particles: true,
        }],
    }
}

/// Counts only admitted known stacks, includes offhand, and clears stale counts on session loss.
#[test]
fn native_item_counts_filter_known_stacks_without_guest_snapshot_reads() {
    let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"count","rows":[],"source":{"kind":{"kind":"item_count","label":"Items","items":["minecraft:stone"],"blocks":false},"limit":1}}]}"#).unwrap();
    let mut snapshot = snapshot();
    snapshot.offhand = snapshot.inventory[0].clone();
    snapshot.inventory[1].item = snapshot.inventory[0].item.clone();
    let policy = hud.clone();
    sources::hydrate(&mut hud, Some(&snapshot), None);
    assert_eq!(hud.cards[0].rows[0].value, "128");
    assert!(hud.cards[0].source.is_none());
    hud.validate().unwrap();
    hud = policy.clone();
    if let ui::mod_hud::NativeRows::ItemCount { items, blocks, .. } =
        &mut hud.cards[0].source.as_mut().unwrap().kind
    {
        items.clear();
        *blocks = true;
    }
    sources::hydrate(&mut hud, Some(&snapshot), None);
    assert_eq!(hud.cards[0].rows[0].value, "128");
    hud = policy;
    sources::hydrate(&mut hud, None, None);
    assert_eq!(hud.cards[0].rows[0].value, "--");
}

/// Confirmed activation counts update without inventory snapshots and clear when their native interval ends.
#[test]
fn native_totem_sources_require_grants_and_follow_confirmed_counts() {
    let mut state = State::new(
        ModGrants {
            hud: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    );
    let json = r#"{"cards":[{"id":"totems","rows":[],"source":{"kind":{"kind":"totem_uses","flip":false,"count_color":true},"limit":1},"text_plate":{}}]}"#;
    assert!(set_content(&mut state, json.into()).unwrap().is_err());
    state.grants.player_state = true;
    assert!(set_content(&mut state, json.into()).unwrap().is_ok());
    state.hud.commit();
    for (count, expected) in [
        (None, "--"),
        (Some(0), "0"),
        (Some(2), "-2"),
        (Some(3), "-3"),
        (None, "--"),
    ] {
        state.player_state.observations.local_totem_uses = count;
        sources::refresh(&mut state);
        let hud = state.hud.resolved.as_ref().unwrap();
        assert_eq!(hud.cards[0].rows[0].value, expected);
        assert!(hud.cards[0].source.is_none());
        assert!(state.player_state.snapshot.is_none());
        hud.validate().unwrap();
    }
    let mut preview: Hud = serde_json::from_str(json).unwrap();
    if let ui::mod_hud::NativeRows::TotemUses { flip, count_color } =
        &mut preview.cards[0].source.as_mut().unwrap().kind
    {
        *flip = true;
        *count_color = false;
    }
    sources::hydrate_totems(&mut preview, Some(2));
    assert_eq!(preview.cards[0].rows[0].value, "2");
    assert_eq!(preview.cards[0].rows[0].color, [1.; 4]);
    sources::hydrate_preview(&mut preview, None, None);
    assert_eq!(preview.cards[0].rows[0].value, "2");
}

/// Potion counters include only the requested effect and soup identities, including known offhand stacks.
#[test]
fn native_potion_counts_use_effect_facts_and_ignore_unrelated_auxiliary_values() {
    let policy: Hud = serde_json::from_str(r#"{"cards":[{"id":"supplies","rows":[],"source":{"kind":{"kind":"item_count","label":"","items":["minecraft:mushroom_stew"],"blocks":false,"potion_effect":"minecraft:instant_health"},"limit":1},"text_plate":{}}]}"#).unwrap();
    policy.validate().unwrap();
    let mut facts = snapshot();
    for slot in &mut facts.inventory {
        slot.known = true;
        slot.item = None;
    }
    let auxiliary = |identifier: &str| {
        assets::potion::POTION_DESCRIPTORS
            .iter()
            .position(|p| p.identifier == identifier)
            .unwrap() as u32
    };
    for (slot, id, metadata, count, known) in [
        (
            0,
            "minecraft:potion",
            auxiliary("minecraft:healing"),
            3,
            true,
        ),
        (
            1,
            "minecraft:splash_potion",
            auxiliary("minecraft:strong_healing"),
            4,
            true,
        ),
        (
            2,
            "minecraft:lingering_potion",
            auxiliary("minecraft:healing"),
            2,
            true,
        ),
        (
            3,
            "minecraft:potion",
            auxiliary("minecraft:strength"),
            8,
            true,
        ),
        (
            4,
            "minecraft:potion",
            auxiliary("minecraft:regeneration"),
            7,
            true,
        ),
        (5, "minecraft:mushroom_stew", 0, 5, true),
        (6, "minecraft:potion", u32::MAX, 9, true),
        (
            7,
            "minecraft:stone",
            auxiliary("minecraft:healing"),
            64,
            true,
        ),
        (
            8,
            "minecraft:splash_potion",
            auxiliary("minecraft:healing"),
            6,
            false,
        ),
    ] {
        facts.inventory[slot] = PlayerStateSlot {
            known,
            item: Some(PlayerStateItem {
                identifier: Some(id.into()),
                network_id: 777,
                metadata,
                count,
                block: false,
                damage: None,
                max_durability: None,
            }),
        };
    }
    facts.offhand = facts.inventory[0].clone();
    let mut hud = policy.clone();
    sources::hydrate(&mut hud, Some(&facts), None);
    assert_eq!(hud.cards[0].rows[0].value, "17");
    assert!(hud.cards[0].rows[0].label.is_empty());
    hud.validate().unwrap();
    let mut hud = policy;
    sources::hydrate(&mut hud, None, None);
    assert_eq!(hud.cards[0].rows[0].value, "--");
}

/// A source cannot admit text that would later make its materialized rows invalid.
#[test]
fn count_labels_obey_the_ordinary_plain_text_contract() {
    let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"count","rows":[],"source":{"kind":{"kind":"item_count","label":"Items","items":[],"blocks":true},"limit":1}}]}"#).unwrap();
    for label in ["§aItems", "Items\n", "Items\t"] {
        if let ui::mod_hud::NativeRows::ItemCount { label: current, .. } =
            &mut hud.cards[0].source.as_mut().unwrap().kind
        {
            *current = label.into();
        }
        assert!(hud.validate().is_err(), "{label:?}");
    }
}

/// Editor placeholders retain a selectable height without fabricating inventory facts.
#[test]
fn empty_sources_get_editor_captions_without_changing_gameplay_content() {
    let policy: Hud = serde_json::from_str(r#"{"cards":[{"id":"armor","editor_label":"Armor Status","rows":[],"source":{"kind":{"kind":"armor","held":false,"percent":true,"empty":false},"limit":5}}]}"#).unwrap();
    let mut gameplay = policy.clone();
    sources::hydrate(&mut gameplay, Some(&snapshot()), None);
    assert!(gameplay.cards[0].rows.is_empty() && gameplay.cards[0].title.is_empty());
    let mut preview = policy.clone();
    sources::hydrate_preview(&mut preview, Some(&snapshot()), None);
    assert_eq!(preview.cards[0].title, "Armor Status");
    assert!(preview.cards[0].rows.is_empty());
    preview.validate().unwrap();
    assert!(policy.cards[0].title.is_empty());
}

/// Native source values are independently granted and only publish after commit.
#[test]
fn native_sources_are_granted_transactional_bounded_and_cleared_on_session_loss() {
    let mut state = State::new(
        ModGrants {
            hud: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    );
    let json = r#"{"cards":[{"id":"inventory","rows":[],"source":{"kind":{"kind":"inventory","hotbar":true},"limit":36}},{"id":"effects","rows":[],"source":{"kind":{"kind":"effects","icons":true},"limit":12}}]}"#;
    assert!(set_content(&mut state, json.into()).unwrap().is_err());
    state.grants.player_state = true;
    assert!(set_content(&mut state, json.into()).unwrap().is_ok());
    assert!(state.hud.content.is_none());
    state.hud.commit();
    state.player_state.set_snapshot(Some(snapshot())).unwrap();
    sources::refresh(&mut state);
    let resolved = state.hud.resolved.as_ref().unwrap();
    resolved.validate().unwrap();
    assert_eq!(resolved.cards[0].rows.len(), 36);
    assert!(
        resolved.cards[0].source.is_none(),
        "materialized output must merge by its actual row count"
    );
    assert_eq!(resolved.cards[0].rows[0].value, "64");
    assert_eq!(
        resolved.cards[0].rows[0].item.as_deref(),
        Some("minecraft:stone")
    );
    assert_eq!(resolved.cards[0].rows[1].value, "?");
    assert_eq!(
        resolved.cards[1].rows[0].label,
        assets::effect_descriptor(1).unwrap().name_key
    );
    assert!(resolved.cards[1].rows[0].localized);
    assert!(
        state.hud.content.as_ref().unwrap().cards[0].rows.is_empty(),
        "authored policy must not retain observations"
    );
    state.player_state.set_snapshot(None).unwrap();
    sources::refresh(&mut state);
    assert!(
        state
            .hud
            .resolved
            .as_ref()
            .unwrap()
            .cards
            .iter()
            .all(|card| card.rows.is_empty())
    );
    let invalid = r#"{"cards":[{"id":"bad","rows":[],"source":{"kind":{"kind":"inventory","hotbar":true},"limit":0}}]}"#;
    assert!(set_content(&mut state, invalid.into()).unwrap().is_err());
}

/// Source budgets reserve their possible rows even before inventory data has arrived.
#[test]
fn native_rows_cannot_exceed_the_combined_hud_budget() {
    let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"a","rows":[],"source":{"kind":{"kind":"inventory","hotbar":true},"limit":64}}]}"#).unwrap();
    for i in 1..=ui::mod_hud::MAX_HUD_ROWS / 64 {
        let mut card = hud.cards[0].clone();
        card.id = format!("card{i}");
        hud.cards.push(card);
    }
    assert!(hud.validate().is_err());
}

/// Native actor labels require their own grants and cannot retain actors from another session.
#[test]
fn native_health_is_granted_session_fenced_and_does_not_accumulate_labels() {
    let mut state = State::new(
        ModGrants {
            hud: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    );
    let json =
        r#"{"cards":[],"actor_health":{"players":false,"mobs":true,"range":32,"color":[1,0,0,1]}}"#;
    assert!(set_content(&mut state, json.into()).unwrap().is_err());
    state.grants.entities = true;
    assert!(set_content(&mut state, json.into()).unwrap().is_ok());
    state.hud.commit();
    state.player_state.world = Some(crate::WorldObservation {
        session: 1,
        world: "offline".into(),
        dimension: 0,
        position: (0., 64., 0.),
        alive: true,
    });
    state
        .player_state
        .observations
        .actors
        .push(crate::ObservedActor {
            actor: crate::ActorObservation {
                session: 1,
                dimension: 0,
                runtime_id: 1,
                player: false,
                position: (1., 64., 2.),
                boxes: Vec::new(),
                name: "Offline actor".into(),
                identifier: String::new(),
                yaw: 0.,
                pitch: 0.,
                pickable: true,
                hittable: None,
                look_origin: None,
                look_direction: None,
                invisible: false,
                sneaking: false,
                hurt: false,
                health: Some(20.),
            },
            max_health: Some(40.),
        });
    for _ in 0..3 {
        sources::refresh(&mut state);
        let hud = state.hud.resolved.as_ref().unwrap();
        hud.validate().unwrap();
        assert_eq!(hud.world_labels.len(), 1);
        assert_eq!(hud.world_labels[0].text, "20 HP");
        assert!(state.hud.content.as_ref().unwrap().world_labels.is_empty());
    }
    state.player_state.observations.actors[0].actor.health = Some(12.);
    sources::refresh(&mut state);
    assert_eq!(
        state.hud.resolved.as_ref().unwrap().world_labels[0].text,
        "12 HP"
    );
    state.player_state.observations.actors[0].actor.session = 2;
    sources::refresh(&mut state);
    assert!(state.hud.resolved.as_ref().unwrap().world_labels.is_empty());
}
