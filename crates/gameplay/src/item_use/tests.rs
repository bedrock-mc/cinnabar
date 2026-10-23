use std::sync::Arc;

use protocol::test_support::transactions::{
    InventorySourceKind, ReleaseAction, TransactionBody, TransactionObservation,
    observe_transaction,
};
use protocol::{CONTAINER_NAME_INVENTORY, NetworkItemStack, VerifiedNetworkItemStack};
use sha2::{Digest, Sha256};

use super::fixtures::classify;
use {super::*, inventory::crossbow_animation_frame};

mod admission;
mod crossbow;
mod delayfix;
mod frame_order;
mod progress;
mod respawn;
mod riptide;
mod rockets;

fn crossbow_duration() -> u32 {
    match classify("minecraft:crossbow", false, 0, None).unwrap() {
        AirUse::Hold { max_ticks, .. } => max_ticks,
        _ => panic!("an unloaded crossbow charges"),
    }
}

#[test]
fn native_crossbow_frames_follow_charge_duration_and_projectile() {
    let duration = crossbow_duration();
    assert_eq!(crossbow_animation_frame(None, duration, None, false), 0);
    assert_eq!(crossbow_animation_frame(Some(0), duration, None, false), 0);
    assert_eq!(
        crossbow_animation_frame(Some(duration), duration, None, false),
        4
    );
    assert_eq!(
        crossbow_animation_frame(
            None,
            duration,
            Some(assets::item_use::ItemProjectileRole::Arrow),
            false
        ),
        4
    );
    assert_eq!(
        crossbow_animation_frame(
            None,
            duration,
            Some(assets::item_use::ItemProjectileRole::Firework),
            false
        ),
        5
    );
    assert_eq!(crossbow_animation_frame(Some(23), duration, None, true), 5);
    assert_eq!(crossbow_animation_frame(Some(14), 15, None, false), 4);
}

#[test]
fn unadmitted_use_duration_does_not_imply_food_or_consumption() {
    let identifier = "minecraft:fixture_unadmitted_use";
    assert_eq!(classify(identifier, false, 0, Some(32)), None);
    assert_eq!(
        client_world::resolve_item_use(
            &assets::item_use::diagnostic_profile(identifier),
            Some(32),
            None
        )
        .consumption,
        assets::item_use::ItemConsumption::Unresolved
    );
}

const BOW: i32 = 300;
const SNOWBALL: i32 = 388;
const MENU_ITEM: i32 = 20329;

fn stack(slot: u8, network_id: i32, count: u16) -> FrozenMiningSelection {
    let extra_data: Arc<[u8]> = Arc::from([]);
    let stack = NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: 41,
        count,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id: 0,
        extra_data,
    };
    FrozenMiningSelection {
        slot,
        item: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
    }
}

fn selection(slot: u8, network_id: i32) -> FrozenMiningSelection {
    stack(slot, network_id, 1)
}

fn frame(tick: u64, held: bool) -> UseFrame {
    UseFrame {
        riptide_facing: Default::default(),
        riptide: None,
        tick,
        now_millis: tick * 50,
        position: [0.5, 65.62, 0.5],
        held,
        selection: Some(selection(2, BOW)),
        air_use: classify("minecraft:bow", false, 0, None),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
    }
}

fn item_frame(
    tick: u64,
    held: bool,
    selection: FrozenMiningSelection,
    identifier: &str,
) -> UseFrame {
    UseFrame {
        selection: Some(selection),
        air_use: classify(identifier, false, 0, Some(32)),
        ..frame(tick, held)
    }
}

/// Each packet as "use" or "release".
fn kinds(outcome: &UseOutcome) -> Vec<&'static str> {
    outcome.packets.iter().map(use_kind).collect()
}

/// An item-use transaction as "use", a released item as "release", anything else as "other".
fn use_kind(packet: &protocol::Packet) -> &'static str {
    match observe_transaction(packet).map(|transaction| transaction.body) {
        Some(TransactionBody::ItemUse(_)) => "use",
        Some(TransactionBody::Release(release)) if release.action == ReleaseAction::Release => {
            "release"
        }
        _ => "other",
    }
}

/// The packet after an encode/decode round trip.
fn wire(packet: &protocol::Packet) -> protocol::Packet {
    let bytes = protocol::encode(packet).unwrap();
    protocol::decode_batch(bytes).unwrap().remove(0)
}

/// The transaction a packet carries after an encode/decode round trip.
fn transaction(packet: &protocol::Packet) -> TransactionObservation {
    observe_transaction(&wire(packet)).expect("an inventory transaction")
}

/// The legacy request id, the action count, and every stack's size and net id (actions first).
fn summary(transaction: &TransactionObservation) -> (i32, usize, Vec<u16>, Vec<Option<i32>>) {
    let held = match &transaction.body {
        TransactionBody::ItemUse(body) => Some(&body.item),
        TransactionBody::ItemUseOnActor(body) => Some(&body.item),
        TransactionBody::Release(body) => Some(&body.item),
        TransactionBody::Normal | TransactionBody::Mismatch => None,
    };
    let stacks = transaction
        .actions
        .iter()
        .flat_map(|action| [&action.from_item, &action.to_item])
        .chain(held)
        .collect::<Vec<_>>();
    (
        transaction.legacy_request_id,
        transaction.actions.len(),
        stacks.iter().map(|stack| stack.count).collect(),
        stacks.iter().map(|stack| stack.stack_network_id).collect(),
    )
}

#[test]
fn crossbow_charge_follows_quick_charge_and_a_loaded_one_fires() {
    let hold = |ticks| AirUse::Hold {
        max_ticks: ticks,
        needs: Needs::ArrowOrOffhandRocket,
        slowdown: 0.35,
        cooldown: None,
    };
    assert_eq!(
        classify("minecraft:crossbow", false, 0, None),
        Some(hold(25))
    );
    assert_eq!(
        classify("minecraft:crossbow", false, 3, None),
        Some(hold(10))
    );
    assert_eq!(
        classify("minecraft:crossbow", false, 9, None),
        Some(hold(1))
    );
    assert_eq!(
        classify("minecraft:crossbow", true, 0, None),
        classify("minecraft:crossbow", true, 0, None)
    );
    assert_eq!(classify("minecraft:shield", false, 0, None), None);
}

/// Foods eat for their pack duration and need appetite unless `can_always_eat`; drinks, spears
/// and custom use items follow their own vanilla rules.
#[test]
fn held_uses_follow_the_vanilla_item_rules() {
    let hold = |max_ticks, needs, slowdown| {
        Some(AirUse::Hold {
            max_ticks,
            needs,
            slowdown,
            cooldown: None,
        })
    };
    assert_eq!(
        classify("minecraft:bread", false, 0, Some(32)),
        hold(32, Needs::Appetite, 0.35)
    );
    assert_eq!(
        classify("minecraft:dried_kelp", false, 0, Some(16)),
        hold(16, Needs::Appetite, 0.35)
    );
    assert_eq!(
        classify("minecraft:golden_apple", false, 0, Some(32)),
        hold(32, Needs::Nothing, 0.35)
    );
    assert_eq!(
        classify("minecraft:honey_bottle", false, 0, Some(40)),
        hold(40, Needs::Nothing, 0.35)
    );
    for drink in ["minecraft:potion", "minecraft:milk_bucket"] {
        assert_eq!(
            classify(drink, false, 0, None),
            hold(32, Needs::Nothing, 0.35)
        );
    }
    assert_eq!(
        classify("minecraft:iron_spear", false, 0, Some(1_440_000)),
        hold(1_440_000, Needs::Nothing, 1.0)
    );
    assert_eq!(classify("zeqa:item.snack", false, 0, Some(20)), None);
    assert_eq!(classify("zeqa:item.ffa", false, 0, None), None);
    assert_eq!(classify("minecraft:camera", false, 0, Some(100_000)), None);
    assert_eq!(classify("minecraft:stick", false, 0, None), None);
}

#[test]
fn throwables_consume_one_and_pearls_and_wind_charges_cool_down() {
    for name in [
        "minecraft:snowball",
        "minecraft:egg",
        "minecraft:experience_bottle",
        "minecraft:splash_potion",
        "minecraft:lingering_potion",
    ] {
        assert_eq!(
            classify(name, false, 0, None),
            Some(AirUse::Throw { cooldown: None })
        );
    }
    let cooldown = |name| classify(name, false, 0, None).and_then(AirUse::cooldown);
    assert_eq!(
        cooldown("minecraft:ender_pearl").map(|cooldown| (cooldown.category, cooldown.ticks)),
        Some((super::fixtures::category("ender_pearl"), 20))
    );
    assert_eq!(
        cooldown("minecraft:wind_charge").map(|cooldown| cooldown.ticks),
        Some(10)
    );
}

/// A bow press sends click-air and starts a use; button-up sends one release.
#[test]
fn bow_press_starts_a_use_and_button_up_releases_it() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let started = runtime.step(&frame(100, true));
    assert_eq!(kinds(&started), ["use"]);
    assert!(started.started && runtime.is_using());

    let holding = runtime.step(&frame(120, true));
    assert!(holding.packets.is_empty() && runtime.is_using());

    let released = runtime.step(&frame(130, false));
    assert_eq!(kinds(&released), ["release"]);
    assert!(!runtime.is_using());
    assert!(runtime.step(&frame(131, false)).packets.is_empty());
}

/// A depleted use completes locally: the client sends nothing.
#[test]
fn a_depleted_crossbow_charge_ends_without_a_packet() {
    let crossbow = |tick| UseFrame {
        air_use: classify("minecraft:crossbow", false, 0, None),
        charge_projectile: Some(assets::item_use::ItemProjectileRole::Arrow),
        ..frame(tick, true)
    };
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert!(runtime.step(&crossbow(10)).started);
    assert!(runtime.step(&crossbow(34)).packets.is_empty());
    assert!(runtime.step(&crossbow(35)).packets.is_empty());
    assert!(!runtime.is_using());
    // A crossbow never repeats while held.
    assert!(runtime.step(&crossbow(60)).packets.is_empty());
}

/// Without ammunition vanilla still sends click-air, but no use starts or releases.
#[test]
fn a_bow_without_arrows_sends_click_air_but_never_starts() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let outcome = runtime.step(&UseFrame {
        ready: false,
        ..frame(100, true)
    });
    assert_eq!(kinds(&outcome), ["use"]);
    assert!(!outcome.started && !runtime.is_using());
    assert!(runtime.step(&frame(101, false)).packets.is_empty());
}

/// Server-owned lobby items still send the air use, without a locally predicted hold.
#[test]
fn an_unpredicted_item_sends_click_air_once_per_press_with_its_verified_stack() {
    for identifier in [
        "minecraft:compass",
        "minecraft:emerald",
        "server:lobby_menu",
    ] {
        let mut runtime = ItemUseRuntime::default();
        let use_frame = UseFrame {
            air_use: classify(identifier, false, 0, None),
            ready: false,
            ..frame(100, true)
        };
        assert_eq!(use_frame.air_use, None);
        runtime.observe_press(true);
        let outcome = runtime.step(&use_frame);
        assert_eq!(kinds(&outcome), ["use"], "{identifier}");
        assert!(!outcome.started && !runtime.is_using());
        assert_eq!(runtime.movement_modifier(), None);

        let expected = protocol::click_air_packet(
            held_request(use_frame.selection.as_ref().unwrap(), &use_frame),
            None,
        )
        .unwrap();
        assert_eq!(
            protocol::encode(&outcome.packets[0]).unwrap(),
            protocol::encode(&expected).unwrap(),
        );
        for held in [true, false] {
            assert!(
                runtime
                    .step(&UseFrame {
                        tick: 101,
                        held,
                        ..use_frame.clone()
                    })
                    .packets
                    .is_empty()
            );
        }
        runtime.observe_press(true);
        assert_eq!(
            kinds(&runtime.step(&UseFrame {
                now_millis: use_frame.now_millis + USE_REARM_MILLIS + 1,
                ..use_frame
            })),
            ["use"]
        );
    }
}

#[test]
fn unpredicted_item_use_rejects_consumed_unverified_and_empty_selections() {
    let empty = protocol::NetworkItemStack::empty();
    let empty_selection = FrozenMiningSelection {
        slot: 2,
        item: VerifiedNetworkItemStack::try_new(empty.clone(), empty.nbt_digest).unwrap(),
    };
    for rejected in [
        UseFrame {
            press_consumed: true,
            ..frame(100, true)
        },
        UseFrame {
            selection: None,
            ..frame(100, true)
        },
        UseFrame {
            selection: Some(empty_selection),
            ..frame(100, true)
        },
    ] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let outcome = runtime.step(&UseFrame {
            air_use: None,
            ..rejected
        });
        assert!(outcome.packets.is_empty() && !runtime.has_work(false));
        assert!(!outcome.started && !runtime.is_using());
    }
}

#[test]
fn a_rejected_click_air_packet_does_not_start_a_predicted_use() {
    for rejected in [
        UseFrame {
            selection: Some(selection(9, BOW)),
            ..frame(100, true)
        },
        UseFrame {
            position: [f32::NAN, 65.62, 0.5],
            ..frame(100, true)
        },
    ] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let outcome = runtime.step(&rejected);
        assert!(outcome.packets.is_empty() && !runtime.has_work(false));
        assert!(!outcome.started && !runtime.is_using());
    }
}

/// Reselecting stops the use without a release; a consumed press starts nothing.
#[test]
fn switching_slot_stops_silently_and_consumed_presses_do_nothing() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&frame(100, true));
    let switched = runtime.step(&UseFrame {
        selection: Some(selection(3, BOW)),
        ..frame(101, true)
    });
    assert!(switched.packets.is_empty() && !runtime.is_using());

    runtime.observe_press(true);
    let consumed = runtime.step(&UseFrame {
        press_consumed: true,
        ..frame(110, true)
    });
    assert!(consumed.packets.is_empty() && !runtime.is_using());
    // Holding on after a consumed press never repeats.
    assert!(runtime.step(&frame(130, true)).packets.is_empty());
}

/// Replacing the selected count ends the old use without releasing the replacement stack.
#[test]
fn selected_count_change_cancels_the_old_use_without_release() {
    for count in [15, 17] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let started = runtime.step(&item_frame(
            100,
            true,
            stack(2, MENU_ITEM, 16),
            "minecraft:golden_apple",
        ));
        assert!(started.started && runtime.is_using());
        let released = runtime.step(&item_frame(
            101,
            false,
            stack(2, MENU_ITEM, count),
            "minecraft:golden_apple",
        ));
        assert!(released.packets.is_empty());
        assert!(!runtime.is_using());
    }

    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&item_frame(
        100,
        true,
        stack(2, MENU_ITEM, 16),
        "minecraft:golden_apple",
    ));
    let released = runtime.step(&item_frame(
        101,
        false,
        stack(2, MENU_ITEM, 16),
        "minecraft:golden_apple",
    ));
    assert_eq!(kinds(&released), ["release"]);
}

/// Count admission belongs to release; a held update does not replace its start snapshot.
#[test]
fn selected_count_change_while_held_keeps_the_original_use_until_release() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&item_frame(
        100,
        true,
        stack(2, MENU_ITEM, 16),
        "minecraft:golden_apple",
    ));
    let timing = runtime.active_timing();
    let held = runtime.step(&item_frame(
        101,
        true,
        stack(2, MENU_ITEM, 15),
        "minecraft:golden_apple",
    ));
    assert!(held.packets.is_empty() && !held.started);
    assert_eq!(runtime.active_timing(), timing);
    let released = runtime.step(&item_frame(
        102,
        false,
        stack(2, MENU_ITEM, 16),
        "minecraft:golden_apple",
    ));
    assert_eq!(kinds(&released), ["release"]);
}

/// A briefly unverifiable selection (inventory request in flight) keeps the use and its release.
#[test]
fn an_unverified_selection_keeps_the_use_and_still_releases() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&frame(100, true));
    let hidden = |held| UseFrame {
        selection: None,
        ..frame(101, held)
    };
    assert!(runtime.step(&hidden(true)).packets.is_empty() && runtime.is_using());
    assert_eq!(kinds(&runtime.step(&hidden(false))), ["release"]);
}

/// Held Use without a fresh press never starts a use.
#[test]
fn held_use_without_a_press_starts_nothing() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(false);
    assert!(runtime.step(&frame(100, true)).packets.is_empty());
    assert!(!runtime.is_using());
}

/// An accepted use slows movement by its item's factor until it ends.
#[test]
fn an_active_use_slows_movement_until_it_ends() {
    let mut runtime = ItemUseRuntime::default();
    assert_eq!(runtime.movement_modifier(), None);
    runtime.observe_press(true);
    runtime.step(&frame(100, true));
    assert_eq!(runtime.movement_modifier(), Some(0.35));
    runtime.step(&frame(110, false));
    assert_eq!(runtime.movement_modifier(), None);

    runtime.observe_press(true);
    runtime.step(&UseFrame {
        air_use: classify("minecraft:iron_spear", false, 0, Some(1_440_000)),
        ..frame(120, true)
    });
    assert_eq!(runtime.movement_modifier(), Some(1.0));
}

/// A server menu item (no use behavior) sends a plain click-air on press and every 200 ms held.
#[test]
fn a_custom_menu_item_sends_click_air_and_repeats_while_held() {
    let menu = |tick, held| UseFrame {
        air_use: None,
        ..item_frame(tick, held, selection(0, MENU_ITEM), "zeqa:item.ffa")
    };
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let pressed = runtime.step(&menu(100, true));
    assert_eq!(kinds(&pressed), ["use"]);
    assert!(!pressed.started && pressed.swung.is_none() && !runtime.is_using());
    let sent = transaction(&pressed.packets[0]);
    let (legacy, actions, _, _) = summary(&sent);
    assert_eq!((legacy, actions), (0, 0));
    let item_use = sent.item_use().unwrap();
    assert_eq!(item_use.slot, 0);
    assert_eq!(item_use.item.network_id, MENU_ITEM);

    // 200 ms re-arm: ticks 101..=104 are at most 200 ms later.
    for tick in 101..=104 {
        assert!(runtime.step(&menu(tick, true)).packets.is_empty());
    }
    assert_eq!(kinds(&runtime.step(&menu(105, true))), ["use"]);
    assert!(runtime.step(&menu(106, false)).packets.is_empty());
    assert!(runtime.step(&menu(120, false)).packets.is_empty());
}

/// A thrown snowball swings and reports the decrement; the next throw starts from the predicted
/// stack under the next legacy request id until the server restates the slot.
#[test]
fn a_snowball_throw_reports_its_predicted_decrement() {
    let snowballs = stack(4, SNOWBALL, 16);
    let throw = |tick, held| item_frame(tick, held, snowballs.clone(), "minecraft:snowball");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let first = runtime.step(&throw(100, true));
    assert_eq!(kinds(&first), ["use"]);
    assert_eq!(first.swung, Some(protocol::SwingSource::ThrowItem));
    assert!(!first.started);
    let sent = transaction(&first.packets[0]);
    let first_id = sent.legacy_request_id;
    assert!(protocol::is_legacy_request_id(first_id));
    assert_eq!(
        summary(&sent),
        (
            first_id,
            1,
            vec![16, 15, 16],
            vec![Some(41), Some(first_id), None]
        )
    );
    assert_eq!(
        sent.legacy_set_slots,
        Some(vec![(CONTAINER_NAME_INVENTORY, vec![4])])
    );
    assert!(sent.actions.iter().any(|action| {
        action.source == InventorySourceKind::Container && action.container_id == Some(0)
    }));

    let second = runtime.step(&throw(105, true));
    let second_id = transaction(&second.packets[0]).legacy_request_id;
    assert!(second_id < first_id && protocol::is_legacy_request_id(second_id));
    assert_eq!(
        summary(&transaction(&second.packets[0])),
        (
            second_id,
            1,
            vec![15, 14, 15],
            vec![Some(first_id), Some(second_id), None]
        )
    );

    // The server restating the slot replaces the prediction.
    let restated = stack(4, SNOWBALL, 14);
    let third = runtime.step(&item_frame(110, true, restated, "minecraft:snowball"));
    let (_, _, sizes, ids) = summary(&transaction(&third.packets[0]));
    assert_eq!((sizes[0], ids[0]), (14, Some(41)));
}

/// Every admitted throw reports its slot, source revision and full decrement once.
#[test]
fn throwing_items_reports_each_admitted_inventory_change() {
    let snowballs = |tick, count| UseFrame {
        inventory_revision: Some(7),
        ..item_frame(tick, false, stack(3, SNOWBALL, count), "minecraft:snowball")
    };
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert!(runtime.step(&snowballs(100, 2)).swung.is_some());
    let (slot, revision, change) = runtime.take_inventory_change().unwrap();
    assert_eq!(
        (slot, revision, change.from.count(), change.to.count()),
        (3, 7, 2, 1)
    );
    assert!(runtime.take_inventory_change().is_none());
    // The server has not restated the slot, so this throws the predicted last snowball.
    runtime.observe_press(true);
    assert!(runtime.step(&snowballs(110, 2)).swung.is_some());
    let (slot, revision, change) = runtime.take_inventory_change().unwrap();
    assert_eq!((slot, revision), (3, 7));
    assert!(change.to.is_empty());
    assert!(runtime.take_inventory_change().is_none());

    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&UseFrame {
        creative: true,
        ..snowballs(100, 1)
    });
    assert!(runtime.take_inventory_change().is_none());
}

#[test]
fn fishing_rod_use_swings_without_consuming_the_stack() {
    let rod = |tick, held| item_frame(tick, held, stack(2, MENU_ITEM, 1), "minecraft:fishing_rod");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let outcome = runtime.step(&rod(100, true));

    assert_eq!(kinds(&outcome), ["use"]);
    assert_eq!(outcome.swung, Some(protocol::SwingSource::UseItem));
    assert!(runtime.take_inventory_change().is_none());
    assert!(runtime.step(&rod(105, true)).packets.is_empty());
}

/// Creative throws swing but change no stack, so they carry no action or legacy request.
#[test]
fn a_creative_throw_reports_no_inventory_change() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let outcome = runtime.step(&UseFrame {
        creative: true,
        ..item_frame(100, true, stack(1, SNOWBALL, 16), "minecraft:snowball")
    });
    assert!(outcome.swung.is_some());
    let (legacy, actions, _, _) = summary(&transaction(&outcome.packets[0]));
    assert_eq!((legacy, actions), (0, 0));
}

/// An ender pearl on cooldown still sends click-air, but neither swings nor consumes.
#[test]
fn an_ender_pearl_on_cooldown_sends_a_plain_click_air() {
    const PEARL: i32 = 422;
    let pearl = |tick, pressed_count| {
        item_frame(
            tick,
            false,
            stack(2, PEARL, pressed_count),
            "minecraft:ender_pearl",
        )
    };
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let thrown = runtime.step(&pearl(100, 16));
    assert!(thrown.swung.is_some());
    runtime.observe_press(true);
    let cooling = runtime.step(&pearl(110, 15));
    assert_eq!(kinds(&cooling), ["use"]);
    assert!(cooling.swung.is_none());
    assert_eq!(summary(&transaction(&cooling.packets[0])).1, 0);
    runtime.observe_press(true);
    assert!(runtime.step(&pearl(120, 15)).swung.is_some());
}

/// Eating runs its duration, completes silently and, still held, starts again after the re-arm;
/// letting go early releases.
#[test]
fn eating_completes_silently_repeats_while_held_and_releases_early() {
    let bread = |tick, held| item_frame(tick, held, stack(0, 257, 8), "minecraft:bread");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert!(runtime.step(&bread(100, true)).started);
    assert!(runtime.step(&bread(131, true)).packets.is_empty() && runtime.is_using());
    let completed = runtime.step(&bread(132, true));
    assert_eq!(kinds(&completed), ["use"]);
    assert!(completed.started && runtime.is_using());
    assert_eq!(kinds(&runtime.step(&bread(140, false))), ["release"]);
    assert!(!runtime.is_using());
}

/// A full player's food sends click-air but starts no use.
#[test]
fn food_without_appetite_does_not_start() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let outcome = runtime.step(&UseFrame {
        ready: false,
        ..item_frame(100, true, stack(0, 257, 8), "minecraft:bread")
    });
    assert_eq!(kinds(&outcome), ["use"]);
    assert!(!outcome.started && !runtime.is_using());
}

#[test]
fn a_throw_response_cannot_settle_either_of_two_world_drops() {
    let mut ledger = inventory::PlayerInventoryLedger::default();
    ledger.apply(&protocol::InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    let source = NetworkItemStack {
        network_id: SNOWBALL,
        stack_network_id: 41,
        count: 5,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&protocol::InventoryEvent::Slot(
        protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: protocol::ContainerIdentity::window(0),
                slot: 4,
            },
            stack: source,
            storage_item: None,
        },
    ));
    let mut drop_ids = Vec::new();
    for _ in 0..2 {
        ledger.begin_world_drop(4, Some(1)).unwrap();
        let (packet, _) = ledger.pending_batch().unwrap().unwrap();
        drop_ids.push(transaction(&packet).legacy_request_id);
        assert!(ledger.mark_transport_enqueued(0));
    }
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let outcome = runtime.step(&item_frame(
        100,
        false,
        stack(4, SNOWBALL, 3),
        "minecraft:snowball",
    ));
    let throw_id = transaction(&outcome.packets[0]).legacy_request_id;
    assert!(protocol::is_legacy_request_id(throw_id));
    assert!(drop_ids.iter().all(|id| *id != throw_id));
    assert_ne!(drop_ids[0], drop_ids[1]);
    let before = ledger.displayed_stack(4).cloned();
    ledger.apply(&protocol::InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                request_id: throw_id,
                status: protocol::StackResponseStatus::Rejected,
                containers: Arc::from([]),
            }]),
        },
    ));
    assert_eq!(ledger.displayed_stack(4), before.as_ref());
}

/// An ender pearl press sends its click-air transaction and starts no held use.
#[test]
fn an_ender_pearl_press_sends_click_air() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let outcome = runtime.step(&UseFrame {
        air_use: classify("minecraft:ender_pearl", false, 0, None),
        ..frame(100, true)
    });
    assert_eq!(kinds(&outcome), ["use"]);
    assert!(!outcome.started && !runtime.is_using());
}

/// A press that sends nothing names why for the click-drop trace; a held item never reads as
/// having no air use, since every item sends click-air.
#[test]
fn a_dropped_press_names_its_reason() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert_eq!(runtime.press_drop_reason(&frame(100, true)), None);
    let consumed = UseFrame {
        press_consumed: true,
        ..frame(100, true)
    };
    assert_eq!(
        runtime.press_drop_reason(&consumed),
        Some("consumed_by_block_or_attack")
    );
    let unverified = UseFrame {
        selection: None,
        ..frame(100, true)
    };
    assert_eq!(
        runtime.press_drop_reason(&unverified),
        Some("selection_unverified")
    );
    runtime.step(&frame(100, true));
    assert_eq!(
        runtime.press_drop_reason(&frame(101, true)),
        None,
        "no press waits"
    );
}

/// A local admission fixture: transport owns the actual queue and is tested in client-session.
struct AdmissionQueue {
    capacity: Option<usize>,
    accepted: std::cell::Cell<usize>,
}
impl AdmissionQueue {
    /// Creates a bounded admission sink for rollback and retry scenarios.
    fn with_command_capacity(capacity: usize) -> (Self, ()) {
        (
            Self {
                capacity: Some(capacity),
                accepted: std::cell::Cell::new(0),
            },
            (),
        )
    }
    /// Creates a closed admission sink for session-loss scenarios.
    fn disconnected() -> Self {
        Self {
            capacity: None,
            accepted: std::cell::Cell::new(0),
        }
    }
    /// Reports how many packet permits the fixture accepted.
    fn pending_command_count(&self) -> usize {
        self.accepted.get()
    }
    /// Consumes capacity only when the entire ordered batch fits.
    fn send_inventory_packets(&self, packets: Vec<protocol::Packet>) -> Result<(), BatchSendError> {
        let capacity = self.capacity.ok_or(BatchSendError::Closed)?;
        let count = self.accepted.get() + packets.len();
        if count > capacity {
            return Err(BatchSendError::Full);
        }
        self.accepted.set(count);
        Ok(())
    }
    /// Occupies one permit before the use under test.
    fn send_inventory_packet(&self, packet: protocol::Packet) -> Result<(), BatchSendError> {
        self.send_inventory_packets(vec![packet])
    }
}

#[test]
fn delay_fix_admits_once_per_tick_and_session_reset_restores_rearm() {
    let first = item_frame(1, false, stack(2, SNOWBALL, 16), "minecraft:snowball");
    let mut runtime = ItemUseRuntime::default();
    runtime.synchronize(1);
    runtime.set_delay_fix(true);
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung.is_some());
    runtime.observe_press(true);
    assert_eq!(
        runtime.press_drop_reason(&first),
        Some("use_tick_already_admitted")
    );
    assert!(runtime.step(&first).packets.is_empty());
    let next = UseFrame {
        tick: 2,
        now_millis: first.now_millis + 50,
        selection: Some(stack(3, SNOWBALL, 16)),
        ..first.clone()
    };
    runtime.observe_press(true);
    assert!(runtime.step(&next).swung.is_some());
    runtime.synchronize(2);
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung.is_some());
    runtime.observe_press(true);
    assert!(runtime.step(&next).swung.is_none());
}
