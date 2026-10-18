//! Ordered selection and tick equipment share one accepted announcement.

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum Admission {
    Equipment(u8),
    Attack(i32),
    Input(u64),
}

/// Observes selection, actor attacks and movement in their actual FIFO order.
fn admissions(packets: &[protocol::Packet]) -> Vec<Admission> {
    packets
        .iter()
        .filter_map(|packet| {
            if let Some(report) = equipment(packet) {
                assert_eq!(
                    i32::from(report.container_id),
                    protocol::PLAYER_INVENTORY_WINDOW_ID
                );
                return Some(Admission::Equipment(report.selected_slot));
            }
            if let Some(actor) = observe_transaction(packet).and_then(|tx| tx.actor_use().cloned())
                && actor.action == Some(ActorUseAction::Attack)
            {
                assert_eq!(actor.runtime_id, 99);
                return Some(Admission::Attack(actor.slot));
            }
            protocol::player_auth_input_trace_sample(packet)
                .map(|sample| Admission::Input(sample.tick))
        })
        .collect()
}

#[test]
fn ordered_equipment_selection_is_not_repeated_by_the_next_completed_tick() {
    let (mut app, mut captured) = app_fixture(8);
    next_slot(app.world_mut());
    before_physics(&mut app);
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1),
        "the ordered selection must be accepted before physics completes",
    );

    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    after_physics(&mut app);
    assert_eq!(
        admissions(&captured.drain()),
        [Admission::Equipment(1), Admission::Input(102)],
        "one accepted selection must not become a second tick equipment announcement",
    );

    attack_progress_tests::ticks(app.world_mut(), 103, 104);
    frame(&mut app);
    assert_eq!(admissions(&captured.drain()), [Admission::Input(103)]);
}

#[test]
fn ordered_equipment_full_prefix_keeps_only_accepted_actions_before_movement() {
    const CAPACITY: usize = 8;
    for available in [0, 1, 3] {
        let (mut app, mut captured) = app_fixture(CAPACITY);
        fill(&app, CAPACITY - available);
        next_slot(app.world_mut());
        tap(app.world_mut(), MouseButton::Left);
        frame(&mut app);
        let mut packets = captured.drain();
        let expected_prefix = match available {
            0 => vec![],
            1 => vec![Admission::Equipment(1)],
            _ => vec![Admission::Equipment(1), Admission::Attack(1)],
        };
        assert_eq!(
            admissions(&packets),
            expected_prefix,
            "available: {available}"
        );
        assert_eq!(
            app.world()
                .resource::<client_state::PlayerRuntime>()
                .selected_hotbar_slot(),
            Some(u8::from(available != 0)),
            "Full must not publish a refused selection",
        );

        for _ in 0..3 {
            frame(&mut app);
            packets.extend(captured.drain());
        }
        assert!(!crate::ordered_interactions::pending(app.world()));
        assert_eq!(
            admissions(&packets),
            expected_prefix,
            "later frames must preserve only the admitted prefix: {available}",
        );

        attack_progress_tests::ticks(app.world_mut(), 102, 103);
        frame(&mut app);
        packets.extend(captured.drain());
        let mut expected = expected_prefix;
        expected.push(Admission::Input(102));
        assert_eq!(
            admissions(&packets),
            expected,
            "the completed tick must preserve the accepted prefix: {available}",
        );
    }
}
