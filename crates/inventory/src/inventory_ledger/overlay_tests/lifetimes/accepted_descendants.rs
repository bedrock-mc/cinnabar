use super::*;

/// Builds one response with an explicit count, damage and optional name pair.
fn metadata_cell(
    container: u8,
    slot: u8,
    count: u8,
    id: i32,
    damage: i32,
    name: &str,
    filtered: &str,
) -> StackResponseContainer {
    let mut reply = correction(container, slot, count, id);
    let value = &mut Arc::make_mut(&mut reply.slots)[0];
    value.durability_correction = damage;
    value.custom_name = Arc::from(name);
    value.filtered_custom_name = Arc::from(filtered);
    reply
}

/// Checks materialized metadata, unrelated NBT, its digest and numeric damage consumers.
fn assert_metadata(stack: &NetworkItemStack, damage: i32, name: &str, filtered: Option<&str>) {
    assert_eq!(stack.extra_data, response_item_data(damage, name, filtered));
    assert_eq!(
        stack.nbt_digest,
        <[u8; 32]>::from(Sha256::digest(&stack.extra_data))
    );
    assert_eq!(
        protocol::item_custom_name(&stack.extra_data).as_deref(),
        Some(name)
    );
    assert_eq!(
        protocol::item_filtered_custom_name(&stack.extra_data).as_deref(),
        filtered
    );
    assert_eq!(
        protocol::item_stack_damage(stack),
        u32::try_from(damage).ok()
    );
}

/// An accepted sent transfer retains earlier accepted metadata when its names are unstated.
#[test]
fn accepted_take_keeps_preceding_mining_metadata() {
    let mut ledger = open_ledger(&[(0, named_stack(10, 1, 5, "Old", Some("Filtered old")))]);
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    let take = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    mining_reply(&mut ledger, mining, "Accepted", "Filtered accepted", 21, 6);
    assert_eq!(
        ledger
            .confirmed
            .get(Cell::Inventory(0))
            .map(|held| &held.stack),
        Some(&named_stack(
            21,
            1,
            6,
            "Accepted",
            Some("Filtered accepted")
        ))
    );
    let mut cursor = correction(CONTAINER_NAME_CURSOR, 0, 1, 22);
    Arc::make_mut(&mut cursor.slots)[0].durability_correction = 7;
    respond(
        &mut ledger,
        take,
        StackResponseStatus::Accepted,
        vec![
            correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
            cursor,
        ],
    );
    let retained = ledger.cursor_stack().unwrap();
    eprintln!(
        "accepted descendant name={:?} filtered={:?} damage={:?} overlay={:?}",
        protocol::item_custom_name(&retained.extra_data),
        protocol::item_filtered_custom_name(&retained.extra_data),
        protocol::item_stack_damage(retained),
        ledger.cursor_overlay()
    );
    assert_eq!(
        retained,
        &named_stack(22, 1, 7, "Accepted", Some("Filtered accepted"))
    );
    assert_eq!(
        retained.nbt_digest,
        <[u8; 32]>::from(Sha256::digest(&retained.extra_data))
    );
    assert_eq!(protocol::item_stack_damage(retained), Some(7));
    assert_eq!(
        ledger.cursor_overlay(),
        Some(&StackResponseOverlay {
            custom_name: Some(Arc::from("Accepted")),
            filtered_custom_name: Some(Arc::from("Filtered accepted")),
            durability_correction: Some(7),
        })
    );
    assert!(ledger.displayed_stack(0).is_none());
}

/// Sent and unsent split halves inherit input metadata, while responses own their stated fields.
#[test]
fn split_metadata_follows_acceptance_rejection_and_explicit_names() {
    for sent in [false, true] {
        for accepted in [false, true] {
            for explicit in [false, true] {
                let mut ledger =
                    open_ledger(&[(0, named_stack(10, 4, 5, "Old", Some("Filtered old")))]);
                let mining = ledger.begin_mining_request(0, 6).unwrap();
                let split = ledger.begin_take_count(0, 2).unwrap();
                if sent {
                    send_all(&mut ledger);
                }
                respond(
                    &mut ledger,
                    mining,
                    StackResponseStatus::Accepted,
                    vec![metadata_cell(
                        CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                        0,
                        4,
                        21,
                        6,
                        "Accepted",
                        "Filtered accepted",
                    )],
                );
                assert_metadata(
                    ledger.displayed_stack(0).unwrap(),
                    6,
                    "Accepted",
                    Some("Filtered accepted"),
                );
                assert_metadata(
                    ledger.cursor_stack().unwrap(),
                    6,
                    "Accepted",
                    Some("Filtered accepted"),
                );
                assert_eq!(ledger.displayed_stack(0).unwrap().count, 2);
                assert_eq!(ledger.cursor_stack().unwrap().count, 2);
                assert_eq!(
                    ledger
                        .presented_slot_overlay(0)
                        .unwrap()
                        .custom_name
                        .as_deref(),
                    Some("Accepted")
                );
                assert_eq!(
                    ledger
                        .view()
                        .get(Cell::Cursor)
                        .unwrap()
                        .overlay
                        .as_ref()
                        .unwrap()
                        .durability_correction,
                    Some(6)
                );
                send_all(&mut ledger);
                let (name, filtered) = if explicit {
                    ("Later", "")
                } else {
                    ("", "Ignored")
                };
                respond(
                    &mut ledger,
                    split,
                    if accepted {
                        StackResponseStatus::Accepted
                    } else {
                        StackResponseStatus::Rejected
                    },
                    vec![
                        metadata_cell(
                            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                            0,
                            2,
                            22,
                            7,
                            "",
                            "Ignored",
                        ),
                        metadata_cell(CONTAINER_NAME_CURSOR, 0, 2, 23, 8, name, filtered),
                    ],
                );
                if accepted {
                    assert_metadata(
                        ledger.displayed_stack(0).unwrap(),
                        7,
                        "Accepted",
                        Some("Filtered accepted"),
                    );
                    let (name, filtered) = if explicit {
                        ("Later", None)
                    } else {
                        ("Accepted", Some("Filtered accepted"))
                    };
                    assert_metadata(ledger.cursor_stack().unwrap(), 8, name, filtered);
                    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 23);
                    assert_eq!(
                        ledger.cursor_overlay().unwrap().custom_name.as_deref(),
                        Some(name)
                    );
                    assert!(!ledger.resync_required());
                } else {
                    assert_metadata(
                        ledger.displayed_stack(0).unwrap(),
                        6,
                        "Accepted",
                        Some("Filtered accepted"),
                    );
                    assert_eq!(ledger.displayed_stack(0).unwrap().count, 4);
                    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 21);
                    assert!(ledger.cursor_stack().is_none());
                }
            }
        }
    }
}

/// Chained sent results retain the corrected input in either descendant response order.
#[test]
fn chained_sent_transfers_preserve_metadata_in_both_response_orders() {
    for place_first in [false, true] {
        let mut ledger = open_ledger(&[(0, named_stack(10, 1, 5, "Old", Some("Filtered old")))]);
        let mining = ledger.begin_mining_request(0, 6).unwrap();
        let take = ledger.begin_click(0).unwrap();
        let place = ledger.begin_click(5).unwrap();
        send_all(&mut ledger);
        mining_reply(&mut ledger, mining, "Accepted", "Filtered accepted", 21, 6);
        assert_metadata(
            ledger.displayed_stack(5).unwrap(),
            6,
            "Accepted",
            Some("Filtered accepted"),
        );
        let take_cells = vec![
            correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
            metadata_cell(CONTAINER_NAME_CURSOR, 0, 1, 22, 7, "", "Ignored"),
        ];
        let place_cells = vec![
            correction(CONTAINER_NAME_CURSOR, 0, 0, 0),
            metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                5,
                1,
                24,
                9,
                "Final",
                "",
            ),
        ];
        for (request, cells) in if place_first {
            [(place, place_cells), (take, take_cells)]
        } else {
            [(take, take_cells), (place, place_cells)]
        } {
            respond(&mut ledger, request, StackResponseStatus::Accepted, cells);
        }
        assert_eq!(ledger.displayed_stack(5).unwrap().stack_network_id, 24);
        assert_metadata(ledger.displayed_stack(5).unwrap(), 9, "Final", None);
        assert!(ledger.cursor_stack().is_none());
        assert!(ledger.displayed_stack(0).is_none());
    }
}

/// Rollback cells and the restored cursor share the same corrected item metadata.
#[test]
fn rejected_sent_place_restores_corrected_cursor() {
    let mut ledger = open_ledger(&[(0, named_stack(10, 1, 5, "Old", Some("Filtered old")))]);
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    let take = ledger.begin_click(0).unwrap();
    let place = ledger.begin_click(5).unwrap();
    send_all(&mut ledger);
    mining_reply(&mut ledger, mining, "Accepted", "Filtered accepted", 21, 6);
    respond(
        &mut ledger,
        take,
        StackResponseStatus::Accepted,
        vec![
            correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
            metadata_cell(CONTAINER_NAME_CURSOR, 0, 1, 22, 7, "", ""),
        ],
    );
    respond(&mut ledger, place, StackResponseStatus::Rejected, vec![]);
    assert_metadata(
        ledger.cursor_stack().unwrap(),
        7,
        "Accepted",
        Some("Filtered accepted"),
    );
    assert!(ledger.displayed_stack(5).is_none());
    assert_eq!(
        ledger.cursor_overlay().unwrap().custom_name.as_deref(),
        Some("Accepted")
    );
}

/// Swaps route each input's data to its own result, leaving the other item independent.
#[test]
fn swapped_metadata_stays_with_its_item() {
    for sent in [false, true] {
        for accepted in [false, true] {
            let other = named_stack(50, 1, 3, "Other", Some("Filtered other"));
            let mut ledger = open_ledger(&[
                (0, named_stack(10, 1, 5, "Old", Some("Filtered old"))),
                (5, other.clone()),
            ]);
            let mining = ledger.begin_mining_request(0, 6).unwrap();
            let swap = ledger
                .begin_hotbar_swap(InventoryTarget::Player(5), 0)
                .unwrap();
            if sent {
                send_all(&mut ledger);
            }
            mining_reply(&mut ledger, mining, "Accepted", "Filtered accepted", 21, 6);
            assert_metadata(
                ledger.displayed_stack(5).unwrap(),
                6,
                "Accepted",
                Some("Filtered accepted"),
            );
            assert_metadata(
                ledger.displayed_stack(0).unwrap(),
                3,
                "Other",
                Some("Filtered other"),
            );
            send_all(&mut ledger);
            respond(
                &mut ledger,
                swap,
                if accepted {
                    StackResponseStatus::Accepted
                } else {
                    StackResponseStatus::Rejected
                },
                vec![
                    metadata_cell(
                        CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                        0,
                        1,
                        51,
                        3,
                        "",
                        "",
                    ),
                    metadata_cell(
                        CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                        5,
                        1,
                        52,
                        7,
                        "",
                        "",
                    ),
                ],
            );
            if accepted {
                assert_metadata(
                    ledger.displayed_stack(5).unwrap(),
                    7,
                    "Accepted",
                    Some("Filtered accepted"),
                );
                assert_metadata(
                    ledger.displayed_stack(0).unwrap(),
                    3,
                    "Other",
                    Some("Filtered other"),
                );
            } else {
                assert_metadata(
                    ledger.displayed_stack(0).unwrap(),
                    6,
                    "Accepted",
                    Some("Filtered accepted"),
                );
                assert_eq!(ledger.displayed_stack(5), Some(&other));
            }
        }
    }
}

/// Corrections of a transfer source cannot replace an occupied merge destination's metadata.
#[test]
fn occupied_destination_keeps_its_metadata_authority() {
    let mut ledger = open_ledger(&[
        (0, named_stack(10, 4, 5, "Old", Some("Filtered old"))),
        (5, named_stack(50, 2, 5, "Old", Some("Filtered old"))),
    ]);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        protocol::ITEM_DEFAULT_MAX_STACK_SIZE,
        0,
    ));
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    ledger.begin_take_count(0, 2).unwrap();
    let place = ledger.begin_place_count(5, 2).unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![metadata_cell(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            4,
            21,
            6,
            "Accepted",
            "Filtered accepted",
        )],
    );
    assert_metadata(
        ledger.displayed_stack(5).unwrap(),
        5,
        "Old",
        Some("Filtered old"),
    );
    respond(
        &mut ledger,
        place,
        StackResponseStatus::Accepted,
        vec![
            correction(CONTAINER_NAME_CURSOR, 0, 0, 0),
            metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                5,
                4,
                53,
                5,
                "",
                "Ignored",
            ),
        ],
    );
    assert_metadata(
        ledger.displayed_stack(5).unwrap(),
        5,
        "Old",
        Some("Filtered old"),
    );
    assert!(ledger.slot_overlay(5).unwrap().custom_name.is_none());
}

/// Fresh backing writes break the predecessor's metadata lineage even for the same item type.
#[test]
fn replacement_metadata_isolated_from_stale_mining() {
    let mut ledger = open_ledger(&[(0, named_stack(10, 1, 5, "Old", Some("Filtered old")))]);
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    let take = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    let replacement = named_stack(99, 2, 3, "Replacement", None);
    ledger.apply(&slot_push(0, replacement.clone()));
    mining_reply(&mut ledger, mining, "Stale", "Filtered stale", 21, 6);
    respond(&mut ledger, take, StackResponseStatus::Rejected, vec![]);
    assert_eq!(ledger.displayed_stack(0), Some(&replacement));
    assert!(ledger.slot_overlay(0).is_none());
    assert!(ledger.cursor_stack().is_none());
}

/// Admitted drops keep their own metadata while older corrections settle beneath them.
#[test]
fn admitted_drop_metadata_preserves_count_identity_and_rollback() {
    for accepted in [false, true] {
        let mut ledger = open_ledger(&[(0, named_stack(10, 4, 5, "Old", Some("Filtered old")))]);
        let mining = ledger.begin_mining_request(0, 6).unwrap();
        ledger.begin_world_drop(0, Some(1)).unwrap();
        send_all(&mut ledger);
        let legacy = ledger.displayed_stack(0).unwrap().stack_network_id;
        assert!(protocol::is_legacy_request_id(legacy));
        respond(
            &mut ledger,
            mining,
            StackResponseStatus::Accepted,
            vec![metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                4,
                21,
                6,
                "Accepted",
                "Filtered accepted",
            )],
        );
        let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
        assert_eq!((backing.count, backing.stack_network_id), (3, legacy));
        assert_metadata(backing, 5, "Old", Some("Filtered old"));
        assert!(ledger.slot_overlay(0).is_none());
        assert!(ledger.target_retains_stack_id(InventoryTarget::Player(0), 10));
        respond(
            &mut ledger,
            legacy,
            if accepted {
                StackResponseStatus::Accepted
            } else {
                StackResponseStatus::Rejected
            },
            vec![metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                3,
                31,
                7,
                "",
                "Ignored",
            )],
        );
        let retained = ledger.displayed_stack(0).unwrap();
        assert_eq!(
            (retained.count, retained.stack_network_id),
            if accepted { (3, 31) } else { (4, 10) }
        );
        assert_metadata(
            retained,
            if accepted { 7 } else { 5 },
            "Old",
            Some("Filtered old"),
        );
    }
}

/// Repeated corrections use current input metadata and preserve the full-width damage fix.
#[test]
fn repeated_metadata_corrections_reach_sent_descendants() {
    for reversed in [false, true] {
        for explicit in [false, true] {
            let mut ledger =
                open_ledger(&[(0, named_stack(10, 1, 65_535, "Old", Some("Filtered old")))]);
            let first = ledger.begin_mining_request(0, 0).unwrap();
            let second = ledger.begin_mining_request(0, 0).unwrap();
            let take = ledger.begin_click(0).unwrap();
            send_all(&mut ledger);
            let (first, second) = if reversed {
                (second, first)
            } else {
                (first, second)
            };
            mining_reply(
                &mut ledger,
                first,
                "Accepted",
                "Filtered accepted",
                21,
                65_535,
            );
            mining_reply(
                &mut ledger,
                second,
                if explicit { "Later" } else { "" },
                "",
                22,
                65_535,
            );
            let (name, filtered) = if explicit {
                ("Later", None)
            } else {
                ("Accepted", Some("Filtered accepted"))
            };
            assert_metadata(ledger.cursor_stack().unwrap(), -1, name, filtered);
            respond(
                &mut ledger,
                take,
                StackResponseStatus::Accepted,
                vec![
                    correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
                    metadata_cell(CONTAINER_NAME_CURSOR, 0, 1, 23, 65_535, "", "Ignored"),
                ],
            );
            assert_metadata(ledger.cursor_stack().unwrap(), -1, name, filtered);
            assert_eq!(ledger.cursor_overlay().unwrap().durability_correction, None);
        }
    }
}

/// Once split results settle independently, a later input correction cannot rename the cursor.
#[test]
fn explicit_split_names_survive_either_correction_order() {
    for split_first in [false, true] {
        let mut ledger = open_ledger(&[(0, named_stack(10, 4, 5, "Old", Some("Filtered old")))]);
        let mining = ledger.begin_mining_request(0, 6).unwrap();
        let split = ledger.begin_take_count(0, 2).unwrap();
        send_all(&mut ledger);
        let mining_cells = vec![metadata_cell(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            if split_first { 2 } else { 4 },
            21,
            6,
            "Accepted",
            "Filtered accepted",
        )];
        let split_cells = vec![
            metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                2,
                22,
                7,
                "",
                "Ignored",
            ),
            metadata_cell(
                CONTAINER_NAME_CURSOR,
                0,
                2,
                23,
                8,
                "Independent",
                "Filtered independent",
            ),
        ];
        for (request, cells) in if split_first {
            [(split, split_cells), (mining, mining_cells)]
        } else {
            [(mining, mining_cells), (split, split_cells)]
        } {
            respond(&mut ledger, request, StackResponseStatus::Accepted, cells);
        }
        assert_metadata(
            ledger.displayed_stack(0).unwrap(),
            if split_first { 6 } else { 7 },
            "Accepted",
            Some("Filtered accepted"),
        );
        assert_metadata(
            ledger.cursor_stack().unwrap(),
            8,
            "Independent",
            Some("Filtered independent"),
        );
    }
}

/// An unsent merge is revalidated against corrected names rather than sent with stale inputs.
#[test]
fn corrected_metadata_cancels_an_incompatible_unsent_merge() {
    let mut ledger = open_ledger(&[
        (0, named_stack(10, 4, 5, "Old", Some("Filtered old"))),
        (5, named_stack(50, 2, 5, "Old", Some("Filtered old"))),
    ]);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        protocol::ITEM_DEFAULT_MAX_STACK_SIZE,
        0,
    ));
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    let take = ledger.begin_take_count(0, 2).unwrap();
    let place = ledger.begin_place_count(5, 2).unwrap();
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![metadata_cell(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            4,
            21,
            6,
            "Accepted",
            "Filtered accepted",
        )],
    );
    assert!(
        !ledger
            .pending_requests()
            .any(|request| request.request_id == place)
    );
    assert!(
        ledger
            .pending_requests()
            .any(|request| request.request_id == take)
    );
    assert_metadata(
        ledger.displayed_stack(5).unwrap(),
        5,
        "Old",
        Some("Filtered old"),
    );
    assert!(ledger.pending_batch().unwrap().is_some());
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::Take { source, amount: 2, .. }) if source.stack_network_id == 21
    ));
    send_all(&mut ledger);
}

/// A split's new lineage still has explicit metadata dependency edges through later transfers.
#[test]
fn split_off_metadata_reaches_a_later_sent_transfer() {
    let mut ledger = open_ledger(&[(0, named_stack(10, 4, 5, "Old", Some("Filtered old")))]);
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    let take = ledger.begin_take_count(0, 2).unwrap();
    let place = ledger.begin_click(5).unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![metadata_cell(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            4,
            21,
            6,
            "Accepted",
            "Filtered accepted",
        )],
    );
    assert_metadata(
        ledger.displayed_stack(5).unwrap(),
        6,
        "Accepted",
        Some("Filtered accepted"),
    );
    respond(
        &mut ledger,
        take,
        StackResponseStatus::Accepted,
        vec![
            metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                2,
                22,
                7,
                "",
                "",
            ),
            metadata_cell(CONTAINER_NAME_CURSOR, 0, 2, 23, 7, "", "Ignored"),
        ],
    );
    assert_metadata(
        ledger.displayed_stack(5).unwrap(),
        7,
        "Accepted",
        Some("Filtered accepted"),
    );
    respond(
        &mut ledger,
        place,
        StackResponseStatus::Accepted,
        vec![
            correction(CONTAINER_NAME_CURSOR, 0, 0, 0),
            metadata_cell(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                5,
                2,
                24,
                8,
                "",
                "Ignored",
            ),
        ],
    );
    assert_metadata(
        ledger.displayed_stack(5).unwrap(),
        8,
        "Accepted",
        Some("Filtered accepted"),
    );
    assert_eq!(
        (
            ledger.displayed_stack(5).unwrap().count,
            ledger.displayed_stack(5).unwrap().stack_network_id
        ),
        (2, 24)
    );
}
