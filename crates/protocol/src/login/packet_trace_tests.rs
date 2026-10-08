//! Packet trace drains, overflow and cancellation remain bounded.

use super::*;

#[test]
fn packet_id_trace_incremental_drains_are_lifetime_bounded_with_one_terminal_overflow() {
    let mut trace = PacketIdTraceState::default();
    trace.begin();
    trace.observe(McpePacketName::StartGamePacket);
    let first = trace
        .drain()
        .expect("first observed ID is drained promptly");
    assert_eq!(
        first.packet_ids.as_ref(),
        &[McpePacketName::StartGamePacket as u32]
    );
    assert_eq!(first.overflow, 0);
    assert!(!first.timed_out);

    for _ in 1..MAX_PACKET_ID_TRACE_ENTRIES {
        trace.observe(McpePacketName::StartGamePacket);
    }
    let remainder = trace.drain().expect("remaining bounded IDs are drainable");
    assert_eq!(remainder.packet_ids.len(), MAX_PACKET_ID_TRACE_ENTRIES - 1);
    assert!(
        remainder
            .packet_ids
            .iter()
            .all(|id| *id == McpePacketName::StartGamePacket as u32)
    );
    assert_eq!(remainder.overflow, 0);
    assert!(!remainder.timed_out);

    for _ in 0..7 {
        trace.observe(McpePacketName::CommandOutputPacket);
    }
    assert!(
        trace.drain().is_none(),
        "overflow alone must not emit marker spam"
    );

    trace.started_at = Some(std::time::Instant::now() - PACKET_ID_TRACE_DURATION);
    trace.observe(McpePacketName::CommandOutputPacket);
    let terminal = trace.drain().expect("timeout is reported once");
    assert!(terminal.packet_ids.is_empty());
    assert_eq!(terminal.overflow, 7);
    assert!(terminal.timed_out);
    assert!(trace.drain().is_none());
}

#[test]
fn packet_id_trace_cancel_discards_arm_and_all_pending_evidence() {
    let mut trace = PacketIdTraceState::default();
    trace.begin();
    trace.observe(McpePacketName::StartGamePacket);
    trace.cancel();

    assert!(trace.started_at.is_none());
    assert_eq!(trace.recorded, 0);
    assert_eq!(trace.overflow, 0);
    assert!(!trace.timed_out);
    assert!(trace.drain().is_none());
}
