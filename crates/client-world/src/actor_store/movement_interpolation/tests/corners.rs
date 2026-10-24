use super::*;

/// Sends a player movement update with the actor's network origin restored.
fn move_to(store: &mut ActorStore, sequence: u64, feet: [f32; 3], teleport: bool) {
    let mut position = feet;
    position[1] += store.get(1).unwrap().network_position_offset();
    assert_eq!(
        store.apply_player_move(
            1,
            sequence,
            0,
            protocol::MovePlayerEvent {
                runtime_id: 1,
                position,
                teleported: teleport,
                mode: if teleport {
                    protocol::MovePlayerMode::Teleport
                } else {
                    protocol::MovePlayerMode::Normal
                },
                ..Default::default()
            },
        ),
        ActorApplyResult::Updated,
    );
}

/// Checks the displayed actor bounds against a unit block at the origin.
fn overlaps_corner(actor: &ActorSnapshot, feet: [f32; 3]) -> bool {
    let (min, max) = actor.bounding_box_at(feet).unwrap();
    (0..3).all(|axis| min[axis] < 1.0 && max[axis] > 0.0)
}

#[test]
fn remote_player_interpolation_can_cross_a_corner_between_clear_targets() {
    for reverse in [false, true] {
        let mut store = player(0.0);
        let (mut start, mut target) = ([-0.31, 0.0, 0.5], [0.5, 0.0, -0.31]);
        if reverse {
            std::mem::swap(&mut start, &mut target);
        }
        move_to(&mut store, 2, start, true);
        let actor = store.get(1).unwrap();
        assert!(!overlaps_corner(actor, start));
        assert!(!overlaps_corner(actor, target));

        move_to(&mut store, 3, target, false);
        let mut crossed_corner = false;
        for _ in 0..crate::actor_store::ACTOR_INTERPOLATION_TICKS {
            store.advance_interpolation_ticks(1);
            let actor = store.get(1).unwrap();
            let displayed = actor.interpolated_position(0.5).unwrap();
            crossed_corner |= overlaps_corner(actor, displayed);
            assert_eq!(actor.last_received_pose().position, target);
        }
        let actor = store.get(1).unwrap();
        assert!(
            crossed_corner,
            "remote presentation follows the received segment through the corner",
        );
        assert_eq!(actor.position, target);
        assert_eq!(actor.interpolation_ticks_remaining, 0);
        assert!(!overlaps_corner(actor, actor.position));
    }
}

#[test]
fn a_remote_teleport_discards_the_segment_crossing_a_corner() {
    let mut store = player(0.0);
    let start = [-0.31, 0.0, 0.5];
    let target = [0.5, 0.0, -0.31];
    move_to(&mut store, 2, start, true);
    move_to(&mut store, 3, target, false);
    store.advance_interpolation_ticks(1);
    let actor = store.get(1).unwrap();
    assert!(overlaps_corner(actor, actor.position));

    move_to(&mut store, 4, target, true);
    for ticks in 0..=1 {
        store.advance_interpolation_ticks(ticks);
        let actor = store.get(1).unwrap();
        assert_eq!(actor.interpolation_ticks_remaining, 0);
        for alpha in [0.0, 0.5, 1.0] {
            let displayed = actor.interpolated_position(alpha).unwrap();
            assert_eq!(displayed, target);
            assert!(!overlaps_corner(actor, displayed));
        }
    }
}
