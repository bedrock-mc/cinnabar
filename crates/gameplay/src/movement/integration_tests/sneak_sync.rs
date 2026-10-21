use super::*;
use crate::movement::{LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocomotionState};

/// Server stance updates must preserve local intent and send the corrective action once.
#[test]
fn server_sneak_updates_preserve_controls_and_correct_the_next_input() {
    for toggle in [false, true] {
        for sneaking in [false, true] {
            let (mut physics, mut ticker) = walked_physics(0);
            let mut locals = LocomotionState::default();
            let mut effects = LocalMovementEffectTimeline::default();
            let mut speed = LocalMovementSpeedAuthority::default();
            let frame = |delta, pressed, released| {
                let mut frame = speed_frame(Default::default());
                frame.delta = delta;
                frame.toggle_sneak = toggle;
                frame.sneak = semantic_input::ActionPhase {
                    pressed,
                    held: pressed || (sneaking && !toggle),
                    released,
                };
                frame
            };
            assert!(locals.advance(
                frame(Duration::from_millis(50), sneaking, false),
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            physics.apply_server_movement_flags(0, flags(|flags| flags.sneaking = Some(!sneaking)));
            assert!(locals.advance(
                frame(Duration::ZERO, false, sneaking && toggle),
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            assert!(locals.advance(
                frame(Duration::from_millis(100), false, false),
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            let action = if sneaking {
                PlayerInputFlags::START_SNEAKING
            } else {
                PlayerInputFlags::STOP_SNEAKING
            };
            for tick in [102, 103] {
                let sample = physics.sample_at(tick).unwrap();
                assert_eq!(
                    sample.input.sneak_down, sneaking,
                    "toggle={toggle}, tick={tick}"
                );
                assert_eq!(sample.processed.sneaking, sneaking);
                assert_eq!(sample.input.sneak.held, sneaking && !toggle);
                assert!(!sample.input.sneak.pressed);
                let snapshot = &ticker.pending_snapshots()[(tick - 101) as usize];
                assert_eq!(snapshot.flags.bits() & action.bits() != 0, tick == 102);
            }
            let expected = ticker.pending_snapshots()[1..].to_vec();
            reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
            assert_eq!(ticker.pending_snapshots(), expected);

            if toggle {
                assert!(locals.advance(
                    frame(Duration::from_millis(50), true, false),
                    &mut physics,
                    &mut ticker,
                    &mut effects,
                    &mut speed,
                    &VersionedFloor(1),
                ));
                assert_eq!(physics.sample_at(104).unwrap().input.sneak_down, !sneaking);
            }
        }
    }
}
