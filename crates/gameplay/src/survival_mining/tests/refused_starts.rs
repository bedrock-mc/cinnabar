//! Block presses whose destroy fails at its start report the refusal as vanilla does.

use super::*;
use crate::melee::{Crosshair, RefusedDestroy, RefusedDestroyReason};

/// Presses a refused block on the first unsent tick and returns everything that reaches the wire.
fn refused_press(reason: RefusedDestroyReason) -> Vec<protocol::Packet> {
    let mut harness = MiningAdmission::new(ticker_with_ticks(1));
    assert!(harness.melee.observe_input(true, false));
    let mut press = harness.press(DestroyInput::Held(None)).unwrap();
    press.refused_destroy = Some(RefusedDestroy {
        position: [4, 5, 6],
        face: 3,
        reason,
    });
    harness.frame_with_attack(
        DestroyInput::Held(None),
        Server,
        Some((Crosshair::Block, press)),
        false,
    );
    harness.flush();
    assert!(harness.predictions.is_empty());
    assert_eq!(harness.mining.machine, DestroyMachine::default());
    harness.packets
}

fn encoded(packet: &protocol::Packet) -> Vec<u8> {
    protocol::encode(packet).unwrap().to_vec()
}

/// The press tick's block actions and whether it reports the missed swing.
fn carried(packet: &protocol::Packet) -> (Vec<(i32, [i32; 3], i32)>, bool) {
    let sample = protocol::player_auth_input_trace_sample(packet).expect("player auth input");
    (
        protocol::test_support::packets::input_block_actions(packet),
        sample.flag_names.contains(&"MissedSwing"),
    )
}

/// Adventure denial: attack swing, deny action, then an aborted start and a missed swing.
#[test]
fn a_denied_block_press_reports_the_denial_abort_and_missed_swing() {
    let packets = refused_press(RefusedDestroyReason::Denied);
    assert_eq!(packets.len(), 3);
    assert_eq!(
        encoded(&packets[0]),
        encoded(&protocol::swing_arm_packet(
            8,
            protocol::SwingSource::Attack
        ))
    );
    assert_eq!(
        encoded(&packets[1]),
        encoded(&protocol::deny_destroy_block_packet(8, [4, 5, 6], 3))
    );
    assert_eq!(carried(&packets[2]), (vec![(1, [4, 5, 6], 0)], true));
}

/// Creative with a no-break item: attack swing, creative destroy, then start and abort.
#[test]
fn a_creative_no_break_press_starts_then_aborts_with_a_missed_swing() {
    let packets = refused_press(RefusedDestroyReason::CreativeNoBreak);
    assert_eq!(packets.len(), 3);
    assert_eq!(
        encoded(&packets[0]),
        encoded(&protocol::swing_arm_packet(
            8,
            protocol::SwingSource::Attack
        ))
    );
    assert_eq!(
        encoded(&packets[1]),
        encoded(&protocol::creative_destroy_block_packet(8, [4, 5, 6], 3))
    );
    assert_eq!(
        carried(&packets[2]),
        (vec![(0, [4, 5, 6], 3), (1, [4, 5, 6], 0)], true)
    );
}
