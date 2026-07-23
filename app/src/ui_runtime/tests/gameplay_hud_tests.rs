//! Authoritative gameplay-HUD state coverage: effects, metadata, armor,
//! mounts, hotbar/offhand mirroring, and the derived heart variants.

use protocol::{
    ActorEffectAction, ActorEffectEvent, ActorHandedness, ActorMetadata, ActorMetadataValue,
    ArmorEquipmentEvent, ContainerIdentity, EquipmentEvent, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, NetworkItemStack, SelectedSlotEvent, SlotIdentity,
};
use sha2::Digest;

use super::*;
use crate::ui_runtime::gameplay_hud::{HeartVariant, MAX_HUD_EFFECTS};

fn effect(
    action: ActorEffectAction,
    effect_id: i32,
    duration_ticks: i32,
    tick: u64,
) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action,
        effect_id,
        amplifier: 0,
        particles: true,
        ambient: false,
        duration_ticks,
        tick,
    }
}

fn stack(network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: -1,
        count: 1,
        nbt_digest: sha2::Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: std::sync::Arc::from([]),
    }
}

fn inventory_container(window_id: i32) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window_id),
        slot_type: None,
        dynamic_id: None,
    }
}

#[test]
fn local_effects_metadata_armor_and_mount_fan_into_gameplay_hud_state() {
    let mut runtime = UiRuntime::new(4);

    runtime
        .apply_local_effect(4, 1, effect(ActorEffectAction::Add, 19, 600, 100), 0)
        .unwrap();
    runtime
        .apply_local_effect(4, 2, effect(ActorEffectAction::Add, 1, -1, 100), 0)
        .unwrap();
    assert_eq!(runtime.gameplay_hud().effects().len(), 2);
    assert_eq!(
        runtime.gameplay_hud().heart_variant(Some(500)),
        HeartVariant::Poisoned
    );
    // The poison effect expires on the authoritative clock; the infinite
    // speed effect stays.
    assert_eq!(
        runtime.gameplay_hud().heart_variant(Some(701)),
        HeartVariant::Normal
    );

    runtime
        .apply_local_metadata(
            4,
            3,
            &[
                ActorMetadata {
                    key: 7,
                    value: ActorMetadataValue::Short(150),
                },
                ActorMetadata {
                    key: 42,
                    value: ActorMetadataValue::Short(300),
                },
                ActorMetadata {
                    key: 120,
                    value: ActorMetadataValue::Float(1.0),
                },
            ],
        )
        .unwrap();
    assert_eq!(runtime.gameplay_hud().air_ticks(), Some((150, 300)));
    assert_eq!(
        runtime
            .hud()
            .air()
            .map(|air| (air.current(), air.maximum())),
        Some((150, 300))
    );
    // Full freezing wins over the (expired) poison recolor.
    assert_eq!(
        runtime.gameplay_hud().heart_variant(Some(500)),
        HeartVariant::Frozen
    );

    runtime
        .apply_local_armor(
            4,
            4,
            &ArmorEquipmentEvent {
                actor_runtime_id: 1,
                helmet: stack(100),
                chestplate: NetworkItemStack::empty(),
                leggings: NetworkItemStack::empty(),
                boots: stack(101),
                body: NetworkItemStack::empty(),
            },
        )
        .unwrap();
    let armor = runtime.gameplay_hud().armor().expect("armor retained");
    assert_eq!(armor.helmet.network_id, 100);
    assert!(armor.chestplate.is_empty());

    runtime.apply_local_mount(4, 5, Some(-9)).unwrap();
    assert_eq!(runtime.gameplay_hud().mount_unique_id(), Some(-9));
    runtime.apply_local_mount(4, 6, None).unwrap();
    assert_eq!(runtime.gameplay_hud().mount_unique_id(), None);

    // Session replacement clears every retained gameplay-HUD surface.
    runtime.begin_session(5);
    assert!(runtime.gameplay_hud().effects().is_empty());
    assert_eq!(runtime.gameplay_hud().armor(), None);
    assert_eq!(runtime.gameplay_hud().air_ticks(), None);
    assert_eq!(runtime.gameplay_hud().mount_unique_id(), None);
}

#[test]
fn stale_local_gameplay_events_fail_without_mutation() {
    let mut runtime = UiRuntime::new(4);
    runtime
        .apply_local_effect(4, 10, effect(ActorEffectAction::Add, 19, 600, 100), 0)
        .unwrap();
    assert!(matches!(
        runtime.apply_local_effect(4, 10, effect(ActorEffectAction::Add, 20, 600, 100), 0),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
    assert!(matches!(
        runtime.apply_local_mount(3, 11, Some(1)),
        Err(UiRuntimeError::WrongSession { .. })
    ));
    assert_eq!(runtime.gameplay_hud().effects().len(), 1);
}

#[test]
fn wither_outranks_poison_and_unknown_effect_actions_are_counted() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_effect(1, 1, effect(ActorEffectAction::Add, 19, -1, 0), 0)
        .unwrap();
    runtime
        .apply_local_effect(1, 2, effect(ActorEffectAction::Add, 20, -1, 0), 0)
        .unwrap();
    assert_eq!(
        runtime.gameplay_hud().heart_variant(None),
        HeartVariant::Withered
    );
    runtime
        .apply_local_effect(1, 3, effect(ActorEffectAction::Unknown(9), 21, -1, 0), 0)
        .unwrap();
    assert_eq!(
        runtime.gameplay_hud().diagnostics().skipped_effect_actions,
        1
    );
    assert_eq!(runtime.gameplay_hud().effects().len(), 2);

    // Removal restores the poison recolor, then normal.
    runtime
        .apply_local_effect(1, 4, effect(ActorEffectAction::Remove, 20, 0, 0), 0)
        .unwrap();
    assert_eq!(
        runtime.gameplay_hud().heart_variant(None),
        HeartVariant::Poisoned
    );
}

/// Every vanilla protocol-1001 effect id the HUD can present. Instant
/// effects (6, 7, 23) have no HUD surface.
pub(crate) const RENDERABLE_EFFECT_IDS: [i32; 27] = [
    1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 24, 25, 26, 27, 28,
    29, 30,
];

#[test]
fn unknown_effect_ids_are_counted_and_never_evict_renderable_effects() {
    let mut runtime = UiRuntime::new(1);
    let mut sequence = 0;
    let mut next = || {
        sequence += 1;
        sequence
    };
    // The full renderable catalog stays under the retention cap by design.
    for id in RENDERABLE_EFFECT_IDS {
        runtime
            .apply_local_effect(1, next(), effect(ActorEffectAction::Add, id, -1, 0), 0)
            .unwrap();
    }
    assert_eq!(
        runtime.gameplay_hud().effects().len(),
        RENDERABLE_EFFECT_IDS.len()
    );
    assert!(RENDERABLE_EFFECT_IDS.len() <= MAX_HUD_EFFECTS);

    // Unknown ids are odd remote data: counted, skipped, never stored, and
    // therefore never able to push a renderable effect out of the list.
    for (offset, unknown_id) in [0, 6, 7, 23, 31, 999, -3].into_iter().enumerate() {
        runtime
            .apply_local_effect(
                1,
                next(),
                effect(ActorEffectAction::Add, unknown_id, 100, 0),
                0,
            )
            .unwrap();
        assert_eq!(
            runtime.gameplay_hud().diagnostics().unknown_effect_ids,
            offset as u64 + 1
        );
    }
    assert_eq!(
        runtime.gameplay_hud().effects().len(),
        RENDERABLE_EFFECT_IDS.len()
    );
    assert_eq!(runtime.gameplay_hud().diagnostics().evicted_effects, 0);
}

#[test]
fn finite_effects_expire_on_the_session_clock_without_new_packets() {
    let mut runtime = UiRuntime::new(1);
    // Speed for 100 ticks observed at tick 40, local millis 1_000.
    runtime
        .apply_local_effect(1, 1, effect(ActorEffectAction::Add, 1, 100, 40), 1_000)
        .unwrap();
    // Regeneration without a wire duration never locally expires.
    runtime
        .apply_local_effect(1, 2, effect(ActorEffectAction::Add, 10, -1, 40), 1_000)
        .unwrap();

    // 60 ticks later (3 seconds of local time), speed is still running.
    runtime.expire_gameplay_effects(4_000);
    assert_eq!(runtime.gameplay_hud().effects().len(), 2);
    assert_eq!(runtime.estimated_server_tick(4_000), Some(100));

    // 101 ticks after observation the finite effect is gone, with no packet
    // having arrived since the Add.
    runtime.expire_gameplay_effects(1_000 + 101 * 50);
    assert_eq!(runtime.gameplay_hud().effects().len(), 1);
    assert_eq!(runtime.gameplay_hud().effects()[0].effect_id, 10);
}

#[test]
fn offhand_equipment_echo_does_not_clobber_the_main_hand_slot() {
    let mut runtime = UiRuntime::new(1);
    runtime.retain_local_selected_equipment(
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(50),
            inventory_slot: 2,
            selected_slot: 2,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    assert_eq!(runtime.selected_hotbar_slot(), Some(2));

    runtime.retain_local_selected_equipment(
        2,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(60),
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 119,
            handedness: Some(ActorHandedness::Left),
        },
    );
    // The offhand echo landed in the offhand mirror, not the slot echo.
    assert_eq!(runtime.selected_hotbar_slot(), Some(2));
    assert_eq!(
        runtime
            .gameplay_hud()
            .offhand_stack()
            .map(|stack| stack.network_id),
        Some(60)
    );
}

#[test]
fn inventory_content_slot_and_forced_selection_mirror_into_the_hotbar() {
    let mut runtime = UiRuntime::new(1);
    let mut slots = vec![NetworkItemStack::empty(); 36];
    slots[0] = stack(11);
    slots[8] = stack(19);
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: inventory_container(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime
        .enqueue_inventory_event(
            1,
            2,
            InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: inventory_container(0),
                    slot: 3,
                },
                stack: stack(14),
                storage_item: None,
            }),
        )
        .unwrap();
    runtime.set_local_selected_slot(1);
    runtime
        .enqueue_inventory_event(
            1,
            3,
            InventoryEvent::SelectedSlot(SelectedSlotEvent {
                container: inventory_container(0),
                slot: 3,
                select_slot: true,
            }),
        )
        .unwrap();
    // Nothing is visible until the frame drain runs.
    assert!(!runtime.gameplay_hud().hotbar_known());
    runtime.drain_pending_inventory();

    assert!(runtime.gameplay_hud().hotbar_known());
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(0)
            .map(|stack| stack.network_id),
        Some(11)
    );
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(3)
            .map(|stack| stack.network_id),
        Some(14)
    );
    assert_eq!(runtime.gameplay_hud().hotbar_stack(1), None);
    // The server-forced selection replaced the older local prediction.
    assert_eq!(runtime.selected_hotbar_slot(), Some(3));
    assert_eq!(
        runtime.selected_stack().map(|stack| stack.network_id),
        Some(14)
    );

    // The selected-item identity clock arms once the stack is known.
    runtime.observe_selected_item_identity(1_000);
    assert_eq!(runtime.selected_item_changed_millis(), Some(1_000));
    runtime.observe_selected_item_identity(2_000);
    assert_eq!(runtime.selected_item_changed_millis(), Some(1_000));
    runtime.set_local_selected_slot(0);
    runtime.observe_selected_item_identity(3_000);
    assert_eq!(runtime.selected_item_changed_millis(), Some(3_000));
}

#[test]
fn odd_metadata_values_are_counted_and_skipped_without_disconnect() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_metadata(
            1,
            1,
            &[
                ActorMetadata {
                    key: 42,
                    value: ActorMetadataValue::Short(0),
                },
                ActorMetadata {
                    key: 120,
                    value: ActorMetadataValue::Float(f32::NAN),
                },
                ActorMetadata {
                    key: 7,
                    value: ActorMetadataValue::Int(5),
                },
            ],
        )
        .unwrap();
    assert_eq!(runtime.gameplay_hud().diagnostics().odd_metadata_values, 3);
    assert_eq!(runtime.gameplay_hud().air_ticks(), None);
    assert_eq!(runtime.gameplay_hud().freezing_strength(), 0.0);
}

#[test]
fn lang_catalog_resolves_rawtext_translation_and_item_names() {
    let entries = [
        ("commands.op.success", "Opped: %s"),
        ("item.emerald.name", "Emerald"),
        ("tile.grass.name", "Grass"),
    ]
    .into_iter()
    .map(|(key, value)| assets::LangEntry {
        key: key.into(),
        value: value.into(),
    })
    .collect::<Vec<_>>();
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    let catalog = std::sync::Arc::new(assets::RuntimeLangCatalog::decode(&bytes).unwrap());

    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(std::sync::Arc::clone(&catalog));

    // Localized item names prefer item.* then tile.* keys; unknown identifiers
    // fall back to the mechanical title case.
    assert_eq!(runtime.localized_item_name("minecraft:emerald"), "Emerald");
    assert_eq!(runtime.localized_item_name("minecraft:grass"), "Grass");
    assert_eq!(
        runtime.localized_item_name("minecraft:mystery_thing"),
        "Mystery Thing"
    );

    // A translate rawtext document resolves through the catalog with its
    // argument substituted, and reaches chat as human text.
    let document = protocol::parse_raw_text(
        r#"{"rawtext":[{"translate":"commands.op.success","with":["Steve"]}]}"#,
    )
    .unwrap();
    assert!(document.has_unresolved_components());
    let event = protocol::RawTextEvent {
        text: protocol::TextEvent {
            category: protocol::TextCategory::MessageOnly,
            kind: protocol::TextKind::Raw,
            needs_translation: false,
            source: None,
            message: std::sync::Arc::from(""),
            parameters: std::sync::Arc::from([]),
            xuid: std::sync::Arc::from(""),
            platform_chat_id: std::sync::Arc::from(""),
            filtered_message: None,
        },
        document,
    };
    runtime
        .apply(crate::ui_runtime::SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: protocol::UiEvent::RawText(event),
        })
        .unwrap();
    assert_eq!(
        runtime.chat().messages().back().unwrap().message.as_ref(),
        "Opped: Steve"
    );

    // An unknown key presents verbatim, exactly like the vanilla client.
    let unknown = protocol::parse_raw_text(r#"{"rawtext":[{"translate":"no.such.key"}]}"#).unwrap();
    let event = protocol::RawTextEvent {
        text: protocol::TextEvent {
            category: protocol::TextCategory::MessageOnly,
            kind: protocol::TextKind::Raw,
            needs_translation: false,
            source: None,
            message: std::sync::Arc::from(""),
            parameters: std::sync::Arc::from([]),
            xuid: std::sync::Arc::from(""),
            platform_chat_id: std::sync::Arc::from(""),
            filtered_message: None,
        },
        document: unknown,
    };
    runtime
        .apply(crate::ui_runtime::SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 10,
            server_tick: None,
            event: protocol::UiEvent::RawText(event),
        })
        .unwrap();
    assert_eq!(
        runtime.chat().messages().back().unwrap().message.as_ref(),
        "no.such.key"
    );
}

fn raw_text_event(json: &str) -> protocol::UiEvent {
    protocol::UiEvent::RawText(protocol::RawTextEvent {
        text: protocol::TextEvent {
            category: protocol::TextCategory::MessageOnly,
            kind: protocol::TextKind::Raw,
            needs_translation: false,
            source: None,
            message: std::sync::Arc::from(""),
            parameters: std::sync::Arc::from([]),
            xuid: std::sync::Arc::from(""),
            platform_chat_id: std::sync::Arc::from(""),
            filtered_message: None,
        },
        document: protocol::parse_raw_text(json).unwrap(),
    })
}

#[test]
fn rawtext_scores_resolve_real_owners_and_selectors_use_known_state() {
    let mut runtime = UiRuntime::new(1);
    runtime.set_chat_source_name(std::sync::Arc::from("Reader"));

    // A sidebar objective with one real player owner (unique id 42), one
    // real entity owner (unique id 7), and the reader's own fake-player row.
    runtime
        .apply(envelope(
            1,
            1,
            UiEvent::Objective(protocol::ObjectiveEvent::Display {
                display_slot: std::sync::Arc::from("sidebar"),
                objective_name: std::sync::Arc::from("coins"),
                display_name: std::sync::Arc::from("Coins"),
                criteria_name: std::sync::Arc::from("dummy"),
                sort_order: 1,
            }),
        ))
        .unwrap();
    let entry = |scoreboard_id, score, identity| protocol::ScoreEntry {
        scoreboard_id,
        objective_name: std::sync::Arc::from("coins"),
        score,
        identity,
    };
    runtime
        .apply(envelope(
            1,
            2,
            UiEvent::Score(protocol::ScoreEvent {
                action: protocol::ScoreAction::Change,
                entries: vec![
                    entry(1, 31, protocol::ScoreIdentity::Player(42)),
                    entry(2, 55, protocol::ScoreIdentity::Entity(7)),
                    entry(
                        3,
                        99,
                        protocol::ScoreIdentity::FakePlayer(std::sync::Arc::from("Reader")),
                    ),
                ]
                .into(),
            }),
        ))
        .unwrap();

    // The world stream authority supplies the id-to-name map and player list.
    runtime.refresh_raw_text_identities(
        |unique_id| match unique_id {
            42 => Some(std::sync::Arc::from("Steve")),
            7 => Some(std::sync::Arc::from("Dinnerbone the Pig")),
            _ => None,
        },
        vec![
            std::sync::Arc::from("Reader"),
            std::sync::Arc::from("Steve"),
        ],
    );

    // Real player and entity owners resolve by their authoritative display
    // names; the `*` sentinel resolves as the reader's own row.
    runtime
        .apply(envelope(
            1,
            3,
            raw_text_event(
                r#"{"rawtext":[{"score":{"name":"Steve","objective":"coins"}},{"text":"/"},{"score":{"name":"Dinnerbone the Pig","objective":"coins"}},{"text":"/"},{"score":{"name":"*","objective":"coins"}}]}"#,
            ),
        ))
        .unwrap();
    assert_eq!(
        runtime.chat().messages().back().unwrap().message.as_ref(),
        "31/55/99"
    );

    // `@s` resolves to the reader and `@a` to the sorted known player list;
    // an entity selector needs a live query and presents as empty.
    runtime
        .apply(envelope(
            1,
            4,
            raw_text_event(
                r#"{"rawtext":[{"selector":"@s"},{"text":" | "},{"selector":"@a"},{"text":" | ("},{"selector":"@e[type=cow]"},{"text":")"}]}"#,
            ),
        ))
        .unwrap();
    assert_eq!(
        runtime.chat().messages().back().unwrap().message.as_ref(),
        "Reader | Reader, Steve | ()"
    );
}

#[test]
fn lang_catalog_translates_rawtext_and_localizes_item_names() {
    let entries = [
        assets::LangEntry {
            key: "commands.give.success".into(),
            value: std::sync::Arc::from("Gave %s * %d to %s"),
        },
        assets::LangEntry {
            key: "item.golden_apple.name".into(),
            value: std::sync::Arc::from("Golden Apple"),
        },
    ];
    let bytes = assets::encode_lang_catalog([7; 32], [8; 32], &entries).unwrap();
    let catalog = std::sync::Arc::new(assets::RuntimeLangCatalog::decode(&bytes).unwrap());

    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(catalog);

    // Localized item names prefer the catalog and fall back mechanically.
    assert_eq!(
        runtime.localized_item_name("minecraft:golden_apple"),
        "Golden Apple"
    );
    assert_eq!(
        runtime.localized_item_name("minecraft:unmapped_thing"),
        "Unmapped Thing"
    );

    // A translate component formats its arguments through the catalog; the
    // unknown-key fallback still presents the key verbatim.
    let json = r#"{"rawtext":[{"translate":"commands.give.success","with":[{"text":"Apple"},{"text":"2"},{"text":"Hashim"}]},{"text":" / "},{"translate":"missing.key"}]}"#;
    let document = protocol::parse_raw_text(json).unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: protocol::UiEvent::RawText(protocol::RawTextEvent {
                text: protocol::TextEvent {
                    category: protocol::TextCategory::MessageOnly,
                    kind: protocol::TextKind::Raw,
                    needs_translation: false,
                    source: None,
                    message: std::sync::Arc::from(document.literal_text()),
                    parameters: std::sync::Arc::from([]),
                    xuid: std::sync::Arc::from(""),
                    platform_chat_id: std::sync::Arc::from(""),
                    filtered_message: None,
                },
                document,
            }),
        })
        .unwrap();
    assert_eq!(
        runtime.chat().messages().back().unwrap().message.as_ref(),
        "Gave Apple * 2 to Hashim / missing.key"
    );
}

#[test]
fn mount_jump_charge_ramps_while_held_and_resets_on_release_or_dismount() {
    let mut runtime = UiRuntime::new(1);
    // Holding jump without a mount never charges.
    runtime.set_mount_jump_held(true, 1_000);
    assert_eq!(runtime.mount_jump_charge(1_250), 0.0);

    runtime.apply_local_mount(1, 1, Some(-9)).unwrap();
    runtime.set_mount_jump_held(true, 1_000);
    assert_eq!(runtime.mount_jump_charge(1_000), 0.0);
    assert_eq!(runtime.mount_jump_charge(1_250), 0.5);
    assert_eq!(runtime.mount_jump_charge(1_500), 1.0);
    // The charge saturates at full while held.
    assert_eq!(runtime.mount_jump_charge(9_000), 1.0);

    // Releasing resets to empty; a fresh hold restarts the ramp.
    runtime.set_mount_jump_held(false, 2_000);
    assert_eq!(runtime.mount_jump_charge(2_000), 0.0);
    runtime.set_mount_jump_held(true, 3_000);
    assert_eq!(runtime.mount_jump_charge(3_100), 0.2);

    // Dismounting clears the charge even while jump stays held.
    runtime.apply_local_mount(1, 2, None).unwrap();
    runtime.set_mount_jump_held(true, 3_200);
    assert_eq!(runtime.mount_jump_charge(3_300), 0.0);
}

#[test]
fn the_selected_slot_presents_the_equipment_echo_before_inventory_content() {
    let mut runtime = UiRuntime::new(1);
    runtime.retain_local_selected_equipment(
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(41),
            inventory_slot: 2,
            selected_slot: 2,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    // No inventory content has arrived: the MobEquipment echo is the
    // authoritative selected stack, and only for its own slot.
    assert_eq!(
        runtime
            .presented_hotbar_stack(2)
            .map(|stack| stack.network_id),
        Some(41)
    );
    assert_eq!(runtime.presented_hotbar_stack(1), None);
}

#[test]
fn player_list_overlay_rows_pair_names_with_resolved_list_scores() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(envelope(
            1,
            1,
            UiEvent::Objective(protocol::ObjectiveEvent::Display {
                display_slot: std::sync::Arc::from("list"),
                objective_name: std::sync::Arc::from("deaths"),
                display_name: std::sync::Arc::from("Deaths"),
                criteria_name: std::sync::Arc::from("dummy"),
                sort_order: 1,
            }),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            1,
            2,
            UiEvent::Score(protocol::ScoreEvent {
                action: protocol::ScoreAction::Change,
                entries: vec![protocol::ScoreEntry {
                    scoreboard_id: 1,
                    objective_name: std::sync::Arc::from("deaths"),
                    score: 4,
                    identity: protocol::ScoreIdentity::Player(42),
                }]
                .into(),
            }),
        ))
        .unwrap();
    runtime.refresh_raw_text_identities(
        |unique_id| (unique_id == 42).then(|| std::sync::Arc::from("Steve")),
        vec![std::sync::Arc::from("Alex"), std::sync::Arc::from("Steve")],
    );
    let rows = runtime.player_list_overlay_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0.as_ref(), "Alex");
    assert_eq!(rows[0].1, None);
    assert_eq!(rows[1].0.as_ref(), "Steve");
    assert_eq!(rows[1].1, Some(4));
}
