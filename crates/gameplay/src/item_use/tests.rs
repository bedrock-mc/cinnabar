use std::sync::Arc;

use protocol::{BedrockSession, NetworkItemStack, VerifiedNetworkItemStack};
use sha2::{Digest, Sha256};

use super::*;

mod admission;
mod crossbow;

fn crossbow_duration() -> u32 {
    match classify("minecraft:crossbow", false, 0, None).unwrap() {
        AirUse::Hold { max_ticks, .. } => max_ticks,
        _ => panic!("an unloaded crossbow charges"),
    }
}

#[test]
fn native_bow_frames_are_not_the_pose_charge_curve() {
    assert_eq!(ranged_animation_frame(None), 0);
    for tick in 0..=8 {
        assert_eq!(ranged_animation_frame(Some(tick)), 1);
    }
    for tick in 9..=14 {
        assert_eq!(ranged_animation_frame(Some(tick)), 2);
    }
    for tick in 15..=30 {
        assert_eq!(ranged_animation_frame(Some(tick)), 3);
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
        crossbow_animation_frame(None, duration, Some("minecraft:arrow"), false),
        4
    );
    assert_eq!(
        crossbow_animation_frame(None, duration, Some("minecraft:firework_rocket"), false),
        5
    );
    assert_eq!(crossbow_animation_frame(Some(23), duration, None, true), 5);
    assert_eq!(crossbow_animation_frame(Some(14), 15, None, false), 4);
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
    outcome
        .packets
        .iter()
        .map(|packet| {
            let debug = format!("{:?}", packet.data);
            if debug.contains("ItemUseInventoryTransaction(") {
                "use"
            } else if debug.contains("action_type: Release") {
                "release"
            } else {
                "other"
            }
        })
        .collect()
}

/// The packet after an encode/decode round trip, in debug form.
fn wire(packet: &protocol::Packet) -> String {
    let session = BedrockSession { shield_item_id: 0 };
    let bytes = protocol::encode(packet, &session).unwrap();
    format!(
        "{:?}",
        protocol::decode_batch(bytes, &session).unwrap()[0].data
    )
}

/// Every value printed for `key`, in order.
fn values(debug: &str, key: &str) -> Vec<String> {
    let needle = format!("{key}: ");
    debug
        .match_indices(&needle)
        .map(|(start, _)| {
            let rest = &debug[start + needle.len()..];
            let mut depth = 0;
            let end = rest
                .char_indices()
                .find(|(_, c)| match c {
                    '(' | '[' | '{' => {
                        depth += 1;
                        false
                    }
                    ')' | ']' | '}' if depth > 0 => {
                        depth -= 1;
                        false
                    }
                    ',' | ')' | ']' | '}' => true,
                    _ => false,
                })
                .map_or(rest.len(), |(end, _)| end);
            rest[..end].trim().to_owned()
        })
        .collect()
}

/// The legacy request id, the action count, and every stack's size and net id (actions first).
fn summary(debug: &str) -> (String, usize, Vec<String>, Vec<String>) {
    let legacy = debug
        .split("legacy_request_id: ")
        .nth(1)
        .and_then(|rest| values(rest, "id").into_iter().next())
        .unwrap();
    (
        legacy,
        debug.matches("InventoryAction {").count(),
        values(debug, "stacksize"),
        values(debug, "net_id_variant"),
    )
}

#[test]
fn crossbow_charge_follows_quick_charge_and_a_loaded_one_fires() {
    let hold = |ticks| AirUse::Hold {
        max_ticks: ticks,
        needs: Needs::ArrowOrOffhandRocket,
        slowdown: 0.35,
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
        Some(hold(0))
    );
    assert_eq!(
        classify("minecraft:crossbow", true, 0, None),
        Some(AirUse::Instant)
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
    assert_eq!(
        classify("zeqa:item.snack", false, 0, Some(20)),
        hold(20, Needs::Nothing, 0.35)
    );
    assert_eq!(classify("zeqa:item.ffa", false, 0, None), None);
    assert_eq!(classify("minecraft:camera", false, 0, Some(100_000)), None);
    assert_eq!(classify("minecraft:stick", false, 0, None), None);
    assert_eq!(
        classify::pack_identifier("minecraft:enchanted_golden_apple"),
        Some("minecraft:appleEnchanted")
    );
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
        Some(("ender_pearl", 20))
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

/// A depleted use completes locally: the client sends nothing (`Player::completeUsingItem`).
#[test]
fn a_depleted_crossbow_charge_ends_without_a_packet() {
    let crossbow = |tick| UseFrame {
        air_use: classify("minecraft:crossbow", false, 0, None),
        charge_projectile: Some("minecraft:arrow"),
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

/// Server-owned lobby items still use `baseUseItem`, without a locally predicted hold.
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
        let session = protocol::BedrockSession { shield_item_id: 0 };
        assert_eq!(
            protocol::encode(&outcome.packets[0], &session).unwrap(),
            protocol::encode(&expected, &session).unwrap(),
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
    assert!(!pressed.started && !pressed.swung && !runtime.is_using());
    let debug = wire(&pressed.packets[0]);
    let (legacy, actions, _, _) = summary(&debug);
    assert_eq!((legacy.as_str(), actions), ("0", 0));
    assert_eq!(values(&debug, "slot"), ["0"]);
    assert!(debug.contains(&format!("id: {MENU_ITEM}")));

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
    assert!(first.swung && !first.started);
    let debug = wire(&first.packets[0]);
    assert_eq!(
        summary(&debug),
        (
            "-4".to_owned(),
            1,
            vec!["16".to_owned(), "15".to_owned(), "16".to_owned()],
            vec![
                "Some(41)".to_owned(),
                "Some(-4)".to_owned(),
                "Some(41)".to_owned()
            ],
        )
    );
    assert!(debug.contains("container_enum: Inventorycontainer, slots: [4]"));
    assert!(debug.contains("source_type: Containerinventory, container_id: Some(0)"));

    let second = runtime.step(&throw(105, true));
    assert_eq!(
        summary(&wire(&second.packets[0])),
        (
            "-6".to_owned(),
            1,
            vec!["15".to_owned(), "14".to_owned(), "15".to_owned()],
            vec![
                "Some(-4)".to_owned(),
                "Some(-6)".to_owned(),
                "Some(-4)".to_owned()
            ],
        )
    );

    // The server restating the slot replaces the prediction.
    let restated = stack(4, SNOWBALL, 14);
    let third = runtime.step(&item_frame(110, true, restated, "minecraft:snowball"));
    let (_, _, sizes, ids) = summary(&wire(&third.packets[0]));
    assert_eq!((sizes[0].as_str(), ids[0].as_str()), ("14", "Some(41)"));
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
    assert!(outcome.swung);
    let (legacy, actions, _, _) = summary(&wire(&outcome.packets[0]));
    assert_eq!((legacy.as_str(), actions), ("0", 0));
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
    assert!(thrown.swung);
    runtime.observe_press(true);
    let cooling = runtime.step(&pearl(110, 15));
    assert_eq!(kinds(&cooling), ["use"]);
    assert!(!cooling.swung);
    assert_eq!(summary(&wire(&cooling.packets[0])).1, 0);
    runtime.observe_press(true);
    assert!(runtime.step(&pearl(120, 15)).swung);
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

/// `TypedClientNetId::_generateNext` restarts at -4 once the counter leaves the negative range.
#[test]
fn legacy_request_ids_step_down_by_two_and_wrap() {
    let mut runtime = ItemUseRuntime::default();
    assert_eq!(runtime.next_legacy_request_id(), -4);
    assert_eq!(runtime.next_legacy_request_id(), -6);
    runtime.last_legacy_request_id = i32::MIN;
    assert_eq!(runtime.next_legacy_request_id(), -4);
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
