use super::*;

#[test]
fn pending_attack_keeps_existing_destroy_progress_and_cancellation_advancing() {
    for authority in [Server, Client] {
        for target_state in 0..4 {
            let first = target([1, 2, 3], "minecraft:stone", None);
            let other = target([2, 2, 3], "minecraft:stone", None);
            let mut harness = MiningAdmission::new(ticker_with_ticks(1));
            harness.frame(DestroyInput::Held(Some(&first)), authority);
            harness.flush();
            harness.packets.clear();
            assert_eq!(
                harness.mining.destroying_target(),
                Some((first.position, first.face))
            );
            let mut expected = harness.mining.machine.clone();
            let input = match target_state {
                0 => DestroyInput::Held(Some(&first)),
                1 => DestroyInput::Held(None),
                2 => DestroyInput::Released,
                _ => DestroyInput::Held(Some(&other)),
            };
            let maintenance = if target_state == 3 {
                DestroyInput::Held(None)
            } else {
                input
            };
            harness.melee.observe_input(true, true);
            let mut expected_actions = Vec::new();
            for tick in 102..105 {
                harness
                    .ticker
                    .enqueue_completed_physics(completed(tick))
                    .unwrap();
                expected_actions.push(
                    expected
                        .step(maintenance, STILL, authority)
                        .actions
                        .iter()
                        .map(|action| {
                            (
                                action.kind.wire_value(),
                                action.position,
                                i32::from(action.face),
                            )
                        })
                        .collect::<Vec<_>>(),
                );
            }
            harness.frame_with_attack(input, authority, None, false);
            assert!(harness.melee.press_pending());
            assert_eq!(
                harness.mining.machine, expected,
                "maintenance stalled for {authority:?}, target state {target_state}"
            );
            harness.flush();
            assert_eq!(
                harness
                    .packets
                    .iter()
                    .filter(|packet| protocol::player_auth_input_trace_sample(packet).is_some())
                    .map(protocol::test_support::packets::input_block_actions)
                    .collect::<Vec<_>>(),
                expected_actions,
                "existing destroy actions must travel before the inputs drain"
            );
            assert!(
                harness.predictions.is_empty(),
                "a waiting press started a new destroy"
            );
        }
    }
}

#[test]
fn a_press_retry_waits_for_the_tick_already_advanced_by_destroy_maintenance() {
    for authority in [Server, Client] {
        for same_target in [false, true] {
            let first = target([1, 2, 3], "minecraft:stone", None);
            let next = if same_target {
                first.clone()
            } else {
                target([2, 2, 3], "minecraft:stone", None)
            };
            let mut harness = MiningAdmission::new(ticker_with_ticks(1));
            harness.frame(DestroyInput::Held(Some(&first)), authority);
            harness.flush();
            harness.packets.clear();
            harness.melee.observe_input(true, true);
            harness
                .ticker
                .enqueue_completed_physics(completed(102))
                .unwrap();
            let input = DestroyInput::Held(Some(&next));
            let press = harness.press(input).unwrap();
            let proposal = Some((crate::melee::Crosshair::Block, press));
            harness.frame_with_attack(input, authority, proposal.clone(), true);
            assert!(harness.melee.press_pending());
            let maintained = harness.mining.machine.clone();
            harness.frame_with_attack(input, authority, proposal, false);
            assert!(
                harness.melee.press_pending(),
                "a retry must follow its already-maintained input"
            );
            assert_eq!(
                harness.mining.machine, maintained,
                "retry stepped the same destroy tick twice"
            );
            harness.flush();
            harness.frame(input, authority);
            assert!(!harness.melee.press_pending());
            harness
                .ticker
                .enqueue_completed_physics(completed(103))
                .unwrap();
            harness.frame(input, authority);
            harness.flush();
            assert_eq!(
                harness.mining.destroying_target(),
                Some((next.position, next.face)),
                "the admitted start must use its later movement carrier"
            );
        }
    }
}
